// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed AARE interiors and policy words, without a matcher or permission proof.

use std::{fmt, ops::Range};

use purrassist_core::{
    Governor, ScratchString, ScratchVec, StopReason,
    contract::{Resource, ScratchCharge, saturating_size as size},
};

/// One semantic operation in a flat, source-addressed [AppArmor](https://apparmor.net/) glob.
///
/// Indices refer to [`AppArmorGlob::tokens`]. Literals and classes operate on
/// bytes, including the individual bytes of UTF-8. Group links avoid recursive
/// ownership and let future consumers traverse without parsing source again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobTokenKind {
    /// Unescaped literal bytes in the token's source span.
    Literal,
    /// One literal byte with any punctuation escape already resolved.
    Byte(u8),
    /// Repeat bytes excluding slash and NUL: `*`; component context may require a byte.
    Star,
    /// Repeat non-NUL bytes: `**`; component context may require a non-slash byte.
    RecursiveStar,
    /// One byte excluding slash and NUL: `?`.
    AnyByte,
    /// Start of a class, linked to its closing token.
    ClassStart {
        /// `^` immediately after `[` negates the class; `!` does not.
        negated: bool,
        /// Index of the corresponding [`Self::ClassEnd`].
        end: usize,
    },
    /// An inclusive, ordered byte range within a class.
    ByteRange {
        /// First byte, inclusive.
        first: u8,
        /// Last byte, inclusive.
        last: u8,
    },
    /// End of a class, linked to its starting token.
    ClassEnd {
        /// Index of the corresponding [`Self::ClassStart`].
        start: usize,
    },
    /// Start of brace alternatives; branches may be empty.
    AlternationStart {
        /// Index of the corresponding [`Self::AlternationEnd`].
        end: usize,
    },
    /// Separator between alternatives of the identified brace group.
    Branch {
        /// Index of the enclosing [`Self::AlternationStart`].
        start: usize,
    },
    /// End of brace alternatives, linked to their starting token.
    AlternationEnd {
        /// Index of the corresponding [`Self::AlternationStart`].
        start: usize,
    },
    /// A reference to a caller-declared variable, without expanding its values.
    Variable {
        /// Index accepted by [`AppArmorGlob::declaration`].
        declaration: usize,
    },
}

/// One operation and its exact original byte span; see [`GlobTokenKind`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobToken {
    /// Original half-open byte span. Class bytes may split a UTF-8 scalar.
    pub span: Range<usize>,
    /// Parsed byte or structural semantics.
    pub kind: GlobTokenKind,
}

impl ScratchCharge for GlobToken {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 48);
        48
    };
}

#[derive(Debug)]
struct Declaration(Range<usize>);
impl ScratchCharge for Declaration {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 16);
        16
    };
}

struct Group {
    start: usize,
    branches: usize,
}
impl ScratchCharge for Group {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 16);
        16
    };
}

/// A refused glob or binding, retaining positions without copying error names.
///
/// `offset` addresses the original glob except in `Declaration` (the declaration
/// name) and `Binding` (the binding name or path, as identified by its problem).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobError {
    /// Malformed or ambiguous syntax at an original source byte.
    Syntax {
        /// Original byte offset, including end-of-input for missing syntax.
        offset: usize,
    },
    /// A recognized construct outside the documented admitted subset.
    Unsupported {
        /// Original byte offset.
        offset: usize,
    },
    /// The referenced variable was not declared by the caller.
    UndeclaredVariable {
        /// Start of the original `@{...}` reference.
        offset: usize,
    },
    /// A caller declaration has an invalid or duplicate name.
    Declaration {
        /// Index in the supplied declaration slice.
        index: usize,
        /// Invalid byte within the name, or zero for a duplicate name.
        offset: usize,
    },
    /// A supplied literal binding is invalid or cannot be applied exactly.
    Binding {
        /// Index in the supplied binding slice.
        index: usize,
        /// Byte within its path for path problems, otherwise within its name.
        offset: usize,
        /// The precise binding refusal.
        problem: GlobBindingProblem,
    },
    /// A referenced variable has no supplied literal binding.
    MissingBinding {
        /// Start of its original `@{...}` reference.
        offset: usize,
    },
    /// Exact work, input, depth, storage or cancellation outcome.
    Resource(StopReason),
}

/// Why a literal host binding cannot be safely rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobBindingProblem {
    /// Invalid variable identifier; offset addresses the name.
    InvalidName,
    /// The name was not in the glob's explicit caller declarations.
    Undeclared,
    /// The name has more than one binding.
    Duplicate,
    /// The supplied record is not used by this glob.
    Unused,
    /// The path is empty or does not start with `/`.
    NotAbsolute,
    /// The path contains a control, glob/variable metacharacter or policy quote.
    UnsafePath,
}

impl fmt::Display for GlobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "AppArmor glob refused: {self:?}")
    }
}
impl std::error::Error for GlobError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource(reason) => Some(reason),
            _ => None,
        }
    }
}
impl From<StopReason> for GlobError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// A borrowed, literal host path for one explicitly declared variable.
///
/// The caller owns canonicalization and trust. Library code never reads the
/// filesystem or environment. See [`AppArmorGlob::bind_literal_paths`].
#[derive(Clone, Copy, Debug)]
pub struct LiteralPathBinding<'a> {
    /// An ASCII letter followed by ASCII letters, digits or underscores.
    pub name: &'a str,
    /// Absolute caller-canonicalized path, with no admitted pattern syntax.
    pub path: &'a str,
}

/// Parsed [AppArmor](https://apparmor.net/) AARE interior with explicit caller variable declarations.
///
/// Supports `*`, `**`, `?`, byte classes/ranges with `^` negation, nested brace
/// alternatives including empty branches, and `@{NAME}`. ASCII punctuation
/// escapes produce literal bytes. Alphabetic/numeric escapes, variable references
/// inside classes, unescaped class `*`/`?`, escaped class comma, empty classes,
/// bare class hyphens, descending ranges, singleton
/// brace groups, raw quotes and controls are refused. `!` in a class is literal.
/// Escaped caret is refused as the first atom of a nonnegated class; escaped
/// hyphen is admitted only as a leading literal, never a range endpoint. The policy parser
/// strips those escapes before interpreting class syntax, so broader admission
/// would silently change their intended byte semantics.
/// Surrounding policy quotes are not part of the input.
///
/// Parsing never expands products or bindings. Flat owned tokens avoid recursive
/// traversal and destruction. This is syntax admission, not matching, containment,
/// absolute-path proof, graph admission, policy synthesis or confinement.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{AppArmorGlob, LiteralPathBinding};
/// let mut governor = Governor::new(Budget::default());
/// let glob = AppArmorGlob::parse("@{ROOT}/{,cache/}**", &["ROOT"], &mut governor).unwrap();
/// assert_eq!(glob.to_policy_word(&mut governor).unwrap(), "\"@{ROOT}/{,cache/}**\"");
/// let bindings = [LiteralPathBinding { name: "ROOT", path: "/srv/work" }];
/// assert_eq!(glob.bind_literal_paths(&bindings, &mut governor).unwrap(), "\"/srv/work/{,cache/}**\"");
/// ```
#[derive(Debug)]
pub struct AppArmorGlob {
    source: String,
    names: String,
    declarations: Vec<Declaration>,
    tokens: Vec<GlobToken>,
}

impl AppArmorGlob {
    /// Parse under explicit input, work, depth, storage and cancellation admission.
    ///
    /// Declared names must be distinct ASCII identifiers beginning with a letter.
    /// All comparisons, scans and retained capacities are admitted before use.
    /// The original input is retained unchanged on success; the type example
    /// shows parsing, inspection-free rendering and literal substitution.
    ///
    /// # Errors
    /// Returns [`GlobError`] with original byte positions or an exact resource
    /// reason. Failure returns no partial successful model.
    pub fn parse(
        input: &str,
        declarations: &[&str],
        governor: &mut Governor,
    ) -> Result<Self, GlobError> {
        governor.charge(Resource::InputBytes, size(input.len()))?;
        governor.charge(Resource::Depth, 0)?;
        if input.is_empty() {
            return Err(GlobError::Syntax { offset: 0 });
        }
        let mut names = ScratchString::new(governor);
        let mut retained_declarations = ScratchVec::new(governor);
        for (index, name) in declarations.iter().enumerate() {
            governor.charge(Resource::InputBytes, size(name.len()))?;
            governor.charge(Resource::Work, size(name.len()).saturating_add(1))?;
            if let Some(offset) = invalid_name(name) {
                return Err(GlobError::Declaration { index, offset });
            }
            for previous in &declarations[..index] {
                compare_work(name, previous, governor)?;
                if name == previous {
                    return Err(GlobError::Declaration { index, offset: 0 });
                }
            }
            let start = names.len();
            emit(&mut names, name, governor)?;
            retained_declarations.push(Declaration(start..names.len()))?;
        }
        let tokens = ScratchVec::new(governor);
        let groups = ScratchVec::new(governor);
        let mut parser = Parser {
            input,
            at: 0,
            declarations,
            governor,
            tokens,
            groups,
        };
        parser.parse()?;
        let mut source = ScratchString::new(parser.governor);
        emit(&mut source, input, parser.governor)?;
        parser.governor.check()?;
        Ok(Self {
            source: source.into_string(),
            names: names.into_string(),
            declarations: retained_declarations.into_vec(),
            tokens: parser.tokens.into_vec(),
        })
    }

    /// Original unchanged AARE interior; see the type example for rendering.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The flat semantic token sequence, with links described by [`GlobTokenKind`].
    #[must_use]
    pub fn tokens(&self) -> &[GlobToken] {
        &self.tokens
    }

    /// Resolve a semantic variable index to its caller-declared name.
    #[must_use]
    pub fn declaration(&self, index: usize) -> Option<&str> {
        self.declarations
            .get(index)
            .map(|declaration| &self.names[declaration.0.clone()])
    }

    /// Emit one quoted policy word, preserving the admitted pattern language.
    ///
    /// Quoting does not turn wildcards into literal paths. Explicit declarations
    /// are retained symbolically; the caller owns their eventual trusted values.
    /// This operation does not prove that they expand into absolute paths.
    ///
    /// # Errors
    /// Returns [`GlobError::Resource`] without partial successful output. See
    /// the type example for exact deterministic bytes.
    pub fn to_policy_word(&self, governor: &mut Governor) -> Result<String, GlobError> {
        let mut output = ScratchString::new(governor);
        emit(&mut output, "\"", governor)?;
        emit(&mut output, &self.source, governor)?;
        finish_word(output, governor)
    }

    /// Substitute a complete literal binding set and emit one quoted policy word.
    ///
    /// Every referenced variable needs exactly one record. Undeclared, duplicate
    /// and unused records are refused. Values must be absolute and cannot contain
    /// `* ? [ ] { } @ \\ " ' , ^ ( )` or ASCII controls. They are never reparsed,
    /// recursively expanded or normalized. Canonical path identity and trust are
    /// caller-owned. The type example shows the production substitution path.
    ///
    /// # Errors
    /// Returns exact name/path/source offsets in [`GlobError`], or resource
    /// refusal. No partial successful word escapes.
    pub fn bind_literal_paths(
        &self,
        bindings: &[LiteralPathBinding<'_>],
        governor: &mut Governor,
    ) -> Result<String, GlobError> {
        let positions = self.prepare_bindings(bindings, governor)?;
        let mut output = ScratchString::new(governor);
        emit(&mut output, "\"", governor)?;
        let mut copied = 0;
        for token in &self.tokens {
            governor.charge(Resource::Work, 1)?;
            if let GlobTokenKind::Variable { declaration } = token.kind {
                let position = positions.as_slice()[declaration];
                emit(
                    &mut output,
                    &self.source[copied..token.span.start],
                    governor,
                )?;
                emit(&mut output, bindings[position].path, governor)?;
                copied = token.span.end;
            }
        }
        emit(&mut output, &self.source[copied..], governor)?;
        finish_word(output, governor)
    }
    /// Shared exact binding validation/indexing for printing and byte lowering.
    pub(super) fn prepare_bindings(
        &self,
        bindings: &[LiteralPathBinding<'_>],
        governor: &mut Governor,
    ) -> Result<ScratchVec<usize>, GlobError> {
        let mut positions = ScratchVec::new(governor);
        for _ in &self.declarations {
            governor.charge(Resource::Work, 1)?;
            positions.push(usize::MAX)?;
        }
        let mut used = ScratchVec::new(governor);
        for (index, binding) in bindings.iter().enumerate() {
            governor.charge(Resource::InputBytes, size(binding.name.len()))?;
            governor.charge(Resource::Work, size(binding.name.len()).saturating_add(1))?;
            if let Some(offset) = invalid_name(binding.name) {
                return Err(binding_error(
                    index,
                    offset,
                    GlobBindingProblem::InvalidName,
                ));
            }
            let mut declaration = None;
            for (candidate, retained) in self.declarations.iter().enumerate() {
                let name = &self.names[retained.0.clone()];
                compare_work(binding.name, name, governor)?;
                if binding.name == name {
                    declaration = Some(candidate);
                    break;
                }
            }
            let Some(declaration) = declaration else {
                return Err(binding_error(index, 0, GlobBindingProblem::Undeclared));
            };
            if positions.as_slice()[declaration] != usize::MAX {
                return Err(binding_error(index, 0, GlobBindingProblem::Duplicate));
            }
            validate_literal_path(binding.path, index, governor)?;
            positions.as_mut_slice()[declaration] = index;
            used.push(false)?;
        }
        for token in &self.tokens {
            governor.charge(Resource::Work, 1)?;
            if let GlobTokenKind::Variable { declaration } = token.kind {
                let position = positions.as_slice()[declaration];
                if position == usize::MAX {
                    return Err(GlobError::MissingBinding {
                        offset: token.span.start,
                    });
                }
                used.as_mut_slice()[position] = true;
            }
        }
        for (index, used) in used.as_slice().iter().enumerate() {
            governor.charge(Resource::Work, 1)?;
            if !used {
                return Err(binding_error(index, 0, GlobBindingProblem::Unused));
            }
        }
        governor.check()?;
        Ok(positions)
    }
}

/// Render one safe absolute literal path, using the binding operation's validator.
///
/// The caller owns canonicalization. Rejected characters and errors are the same
/// as [`AppArmorGlob::bind_literal_paths`]; a path failure uses binding index zero.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::literal_path_word;
/// let mut governor = Governor::new(Budget::default());
/// assert_eq!(literal_path_word("/srv/work files", &mut governor).unwrap(), "\"/srv/work files\"");
/// assert!(literal_path_word("/srv/*", &mut governor).is_err());
/// ```
///
/// # Errors
/// Returns a typed unsafe/nonabsolute path or resource refusal, never partial output.
pub fn literal_path_word(path: &str, governor: &mut Governor) -> Result<String, GlobError> {
    validate_literal_path(path, 0, governor)?;
    let mut output = ScratchString::new(governor);
    emit(&mut output, "\"", governor)?;
    emit(&mut output, path, governor)?;
    finish_word(output, governor)
}

fn invalid_name(name: &str) -> Option<usize> {
    if !name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
        return Some(0);
    }
    name.bytes()
        .position(|byte| !byte.is_ascii_alphanumeric() && byte != b'_')
}

fn compare_work(left: &str, right: &str, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(
        Resource::Work,
        size(left.len().max(right.len())).saturating_add(1),
    )
}

fn emit(output: &mut ScratchString, text: &str, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(Resource::Work, size(text.len()).saturating_add(1))?;
    output.push_str(text)
}

fn finish_word(mut output: ScratchString, governor: &mut Governor) -> Result<String, GlobError> {
    emit(&mut output, "\"", governor)?;
    governor.check()?;
    Ok(output.into_string())
}

const fn binding_error(index: usize, offset: usize, problem: GlobBindingProblem) -> GlobError {
    GlobError::Binding {
        index,
        offset,
        problem,
    }
}

fn validate_literal_path(
    path: &str,
    index: usize,
    governor: &mut Governor,
) -> Result<(), GlobError> {
    governor.charge(Resource::InputBytes, size(path.len()))?;
    governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
    if !path.starts_with('/') {
        return Err(binding_error(index, 0, GlobBindingProblem::NotAbsolute));
    }
    if let Some(offset) = path
        .bytes()
        .position(|byte| byte.is_ascii_control() || b"*?[]{}@\\\"',^()".contains(&byte))
    {
        return Err(binding_error(index, offset, GlobBindingProblem::UnsafePath));
    }
    Ok(())
}

struct Parser<'a, 'g> {
    input: &'a str,
    at: usize,
    declarations: &'a [&'a str],
    governor: &'g mut Governor,
    tokens: ScratchVec<GlobToken>,
    groups: ScratchVec<Group>,
}

impl Parser<'_, '_> {
    fn push(&mut self, start: usize, kind: GlobTokenKind) -> Result<(), GlobError> {
        self.tokens.push(GlobToken {
            span: start..self.at,
            kind,
        })?;
        Ok(())
    }

    fn read(&mut self) -> Result<Option<u8>, GlobError> {
        self.governor.charge(Resource::Work, 1)?;
        Ok(self.input.as_bytes().get(self.at).copied())
    }

    fn parse(&mut self) -> Result<(), GlobError> {
        while let Some(byte) = self.read()? {
            let start = self.at;
            self.at += 1;
            match byte {
                b'*' => {
                    let kind = if self.read()? == Some(b'*') {
                        self.at += 1;
                        GlobTokenKind::RecursiveStar
                    } else {
                        GlobTokenKind::Star
                    };
                    self.push(start, kind)?;
                }
                b'?' => self.push(start, GlobTokenKind::AnyByte)?,
                b'\\' => {
                    self.at = start;
                    let byte = self.atom()?;
                    self.push(start, GlobTokenKind::Byte(byte))?;
                }
                b'[' => self.class(start)?,
                b'{' => {
                    self.governor
                        .charge(Resource::Depth, size(self.groups.len()).saturating_add(1))?;
                    let index = self.tokens.len();
                    self.push(start, GlobTokenKind::AlternationStart { end: usize::MAX })?;
                    self.groups.push(Group {
                        start: index,
                        branches: 0,
                    })?;
                }
                b',' => {
                    let Some(group) = self.groups.as_mut_slice().last_mut() else {
                        return Err(GlobError::Syntax { offset: start });
                    };
                    group.branches += 1;
                    let group_start = group.start;
                    self.push(start, GlobTokenKind::Branch { start: group_start })?;
                }
                b'}' => {
                    let Some(group) = self.groups.pop() else {
                        return Err(GlobError::Syntax { offset: start });
                    };
                    if group.branches == 0 {
                        return Err(GlobError::Syntax { offset: start });
                    }
                    let end = self.tokens.len();
                    self.tokens.as_mut_slice()[group.start].kind =
                        GlobTokenKind::AlternationStart { end };
                    self.push(start, GlobTokenKind::AlternationEnd { start: group.start })?;
                }
                b'@' if self.read()? == Some(b'{') => self.variable(start)?,
                b']' | b'"' => return Err(GlobError::Syntax { offset: start }),
                byte if byte.is_ascii_control() => return Err(GlobError::Syntax { offset: start }),
                _ => {
                    // Coalesce literal UTF-8 bytes without inspecting them again
                    // or assuming that a wildcard consumes a Unicode scalar.
                    if let Some(previous) = self.tokens.as_mut_slice().last_mut()
                        && previous.kind == GlobTokenKind::Literal
                        && previous.span.end == start
                    {
                        previous.span.end = self.at;
                    } else {
                        self.push(start, GlobTokenKind::Literal)?;
                    }
                }
            }
        }
        if !self.groups.is_empty() {
            return Err(GlobError::Syntax { offset: self.at });
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<u8, GlobError> {
        let start = self.at;
        let Some(byte) = self.read()? else {
            return Err(GlobError::Syntax { offset: start });
        };
        self.at += 1;
        if byte == b'\\' {
            let Some(escaped) = self.read()? else {
                return Err(GlobError::Syntax { offset: self.at });
            };
            if !escaped.is_ascii_punctuation() {
                return Err(GlobError::Unsupported { offset: start });
            }
            self.at += 1;
            Ok(escaped)
        } else if byte.is_ascii_control() || byte == b'"' {
            Err(GlobError::Syntax { offset: start })
        } else if byte == b'[' || byte == b'-' || (byte == b'@' && self.read()? == Some(b'{')) {
            Err(GlobError::Unsupported { offset: start })
        } else {
            Ok(byte)
        }
    }

    fn class_atom(&mut self) -> Result<u8, GlobError> {
        let start = self.at;
        if matches!(self.read()?, Some(b'*' | b'?'))
            || self.input.as_bytes().get(start..start.saturating_add(2)) == Some(b"\\,")
        {
            // Native conversion expands these wildcards even inside classes;
            // an escaped comma retains a backslash as an additional class byte.
            return Err(GlobError::Unsupported { offset: start });
        }
        self.atom()
    }

    fn class(&mut self, start: usize) -> Result<(), GlobError> {
        self.governor
            .charge(Resource::Depth, size(self.groups.len()).saturating_add(1))?;
        let negated = self.read()? == Some(b'^');
        self.at += usize::from(negated);
        let index = self.tokens.len();
        self.push(
            start,
            GlobTokenKind::ClassStart {
                negated,
                end: usize::MAX,
            },
        )?;
        let mut count = 0;
        loop {
            if self.read()? == Some(b']') {
                if count == 0 {
                    return Err(GlobError::Syntax { offset: self.at });
                }
                let close = self.at;
                self.at += 1;
                let end = self.tokens.len();
                self.tokens.as_mut_slice()[index].kind = GlobTokenKind::ClassStart { negated, end };
                self.push(close, GlobTokenKind::ClassEnd { start: index })?;
                return Ok(());
            }
            let atom_start = self.at;
            let first = self.class_atom()?;
            if (first == b'^' && count == 0 && !negated) || (first == b'-' && count != 0) {
                return Err(GlobError::Unsupported { offset: atom_start });
            }
            let kind = if self.read()? == Some(b'-') {
                if first == b'-' {
                    return Err(GlobError::Unsupported { offset: atom_start });
                }
                self.at += 1;
                if self.read()? == Some(b']') {
                    return Err(GlobError::Syntax {
                        offset: self.at - 1,
                    });
                }
                let last_start = self.at;
                let last = self.class_atom()?;
                if last == b'-' {
                    return Err(GlobError::Unsupported { offset: last_start });
                }
                if first > last {
                    return Err(GlobError::Syntax { offset: atom_start });
                }
                GlobTokenKind::ByteRange { first, last }
            } else {
                GlobTokenKind::Byte(first)
            };
            self.push(atom_start, kind)?;
            count += 1;
        }
    }

    fn variable(&mut self, start: usize) -> Result<(), GlobError> {
        self.at += 1; // The caller already admitted and recognized the opening brace.
        let name_start = self.at;
        loop {
            match self.read()? {
                Some(b'}') => break,
                Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_' => self.at += 1,
                _ => return Err(GlobError::Syntax { offset: self.at }),
            }
        }
        let name = &self.input[name_start..self.at];
        self.governor
            .charge(Resource::Work, size(name.len()).saturating_add(1))?;
        if let Some(offset) = invalid_name(name) {
            return Err(GlobError::Syntax {
                offset: name_start + offset,
            });
        }
        self.at += 1;
        for (declaration, candidate) in self.declarations.iter().enumerate() {
            compare_work(name, candidate, self.governor)?;
            if name == *candidate {
                return self.push(start, GlobTokenKind::Variable { declaration });
            }
        }
        Err(GlobError::UndeclaredVariable { offset: start })
    }
}
