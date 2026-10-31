// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One governed call-framing transform over the positioned grammar tree.

use super::syntax::{Document, PExpr, Statement};
use super::{CompiledGrammar, Grammar, GrammarError, compile, syntax};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, saturating_size,
};
use crate::digest::{Digest, DigestBuilder};

/// Why a recipe suppresses its explicitly enumerated protocol markers.
///
/// These are format contracts. No model, tokenizer or network operation runs here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suppression {
    /// The pinned tokenizer classifies the enumerated markers as special tokens.
    SpecialTokens,
    /// The caller selects marker-suppressed text, independently of tokenizer flags.
    ProtocolMarkers,
}

/// Retained evidence required before a derived call becomes an attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallAnchor {
    /// Keep the source grammar's explicit retained call-shaped commit points.
    Retained,
    /// A call starts at a source line and commits only after native JSON evidence.
    /// With declared tools, whitespace between a plain name and arguments also
    /// requires a declared name or alias, including a native-admitted joint ID
    /// split; an unknown prose word stays content. A retained protocol prefix
    /// supplies its own evidence.
    LineStartArguments,
}

/// An honest source recipe for a literal or positioned-AST-derived grammar.
///
/// A derivation changes call-reachable framing only. Result, declaration and
/// reasoning conventions are inherited; deleting their framing must never turn
/// ordinary prose into a wildcard result or an invented reasoning block.
/// Derived recipes are read-only built-in presets. Callers can inspect and
/// recompile them; arbitrary custom marker projections are not supported.
///
/// ```
/// use purrassist_core::{Budget, Governor, grammar::GrammarRecipe};
/// let source = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
/// let recipe = GrammarRecipe::Literal(source);
/// let grammar = recipe.compile_governed(&mut Governor::new(Budget::default())).unwrap();
/// assert_eq!(grammar.id(), "g");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrammarRecipe<'a> {
    /// A complete first-party grammar source.
    Literal(&'a str),
    /// A reviewed built-in call-framing projection of its unchanged source.
    Derived(DerivedRecipe<'a>),
}

/// Immutable metadata for one reviewed built-in call-framing projection.
///
/// Obtain this payload from [`crate::tool_call::DialectId::source`]. Its private
/// fields prevent combining a preset's projection policy with arbitrary grammar
/// source. General custom grammars remain supported by [`GrammarRecipe::Literal`].
///
/// ```
/// use purrassist_core::{grammar::GrammarRecipe, tool_call::DialectId};
/// let Some(GrammarRecipe::Derived(recipe)) = DialectId::MistralV11Stripped.source() else {
///     panic!("built-in derived recipe");
/// };
/// assert_eq!(recipe.id(), "mistral-v11-stripped.v1");
/// assert!(recipe.markers().contains(&"[CALL_ID]"));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedRecipe<'a> {
    id: &'a str,
    source: &'a str,
    markers: &'a [&'a str],
    suppression: Suppression,
    anchor: CallAnchor,
}
impl<'a> DerivedRecipe<'a> {
    /// The derived versioned grammar identity.
    #[must_use]
    pub const fn id(self) -> &'a str {
        self.id
    }
    /// The unchanged parent grammar source.
    #[must_use]
    pub const fn source(self) -> &'a str {
        self.source
    }
    /// Exact suppressed marker spellings; confusable repairs remain separate.
    #[must_use]
    pub const fn markers(self) -> &'a [&'a str] {
        self.markers
    }
    /// The documented reason these markers are suppressed.
    #[must_use]
    pub const fn suppression(self) -> Suppression {
        self.suppression
    }
    /// The retained evidence required before a projected call becomes an attempt.
    #[must_use]
    pub const fn anchor(self) -> CallAnchor {
        self.anchor
    }
}

impl<'a> GrammarRecipe<'a> {
    /// The unchanged source from which this recipe starts.
    ///
    /// For a derived recipe this is the parent source, not a stand-alone spelling
    /// of the derived grammar. Its declaration and result conventions are inherited.
    #[must_use]
    pub const fn base_text(self) -> &'a str {
        match self {
            Self::Literal(source) => source,
            Self::Derived(recipe) => recipe.source,
        }
    }

    pub(crate) const fn derived(
        id: &'a str,
        source: &'a str,
        markers: &'a [&'a str],
        suppression: Suppression,
        anchor: CallAnchor,
    ) -> Self {
        Self::Derived(DerivedRecipe {
            id,
            source,
            markers,
            suppression,
            anchor,
        })
    }

    /// Compile the recipe through the existing syntax parser and semantic compiler.
    ///
    /// # Errors
    /// Preserves original grammar refusals and caller work, storage, depth and stop
    /// failures. Empty marker spellings are refused as [`GrammarError::EmptyLiteral`].
    /// No partial grammar is returned. The identity includes the complete framed
    /// source, projection version, marker inventory, policy and derived name.
    pub fn compile_governed(
        self,
        governor: &mut Governor,
    ) -> Result<CompiledGrammar, GrammarError> {
        let Self::Derived(DerivedRecipe {
            id,
            source,
            markers,
            suppression,
            anchor,
        }) = self
        else {
            return Grammar::compile_governed(self.base_text(), governor);
        };
        syntax::validate_id(id, 0, governor)?;
        let mut document = syntax::parse(source, governor)?;
        for marker in markers {
            governor.charge(Resource::Work, 1)?;
            if marker.is_empty() {
                return Err(GrammarError::EmptyLiteral { offset: 0 });
            }
        }
        project(&mut document, markers, anchor, governor)?;
        let mut name = ScratchString::new(governor);
        governor.charge(Resource::Work, saturating_size(id.len()))?;
        name.push_str(id)?;
        document.id = name;
        let digest = identity(id, source, markers, suppression, anchor, governor)?;
        compile::compile_derived(document, digest, anchor, governor)
    }
}

fn identity(
    id: &str,
    source: &str,
    markers: &[&str],
    suppression: Suppression,
    anchor: CallAnchor,
    governor: &mut Governor,
) -> Result<Digest, GrammarError> {
    fn frame(
        hash: &mut DigestBuilder,
        bytes: &[u8],
        governor: &mut Governor,
    ) -> Result<(), GrammarError> {
        let length =
            u64::try_from(bytes.len()).map_err(|_| governor.stop_resource(Resource::Work))?;
        governor.charge(Resource::Work, length.saturating_add(8))?;
        hash.update(&length.to_be_bytes());
        hash.update(bytes);
        Ok(())
    }
    let mut hash = DigestBuilder::new();
    frame(
        &mut hash,
        b"purrassist/grammar/call-marker-projection/v1",
        governor,
    )?;
    frame(&mut hash, id.as_bytes(), governor)?;
    frame(&mut hash, source.as_bytes(), governor)?;
    frame(&mut hash, &[suppression as u8, anchor as u8], governor)?;
    for marker in markers {
        frame(&mut hash, marker.as_bytes(), governor)?;
    }
    Ok(hash.finish())
}

fn empty(at: usize, governor: &Governor) -> PExpr {
    PExpr::Sequence(ScratchVec::new(governor), at, 1)
}

/// Compact an owning admitted vector without discarding any kept element's owner.
fn retain<T: ScratchCharge>(values: &mut ScratchVec<T>, mut keep: impl FnMut(&T) -> bool) {
    let mut write = 0;
    for read in 0..values.len() {
        if keep(&values.as_slice()[read]) {
            values.as_mut_slice().swap(write, read);
            write += 1;
        }
    }
    while values.len() > write {
        values.pop();
    }
}

fn suppress(
    bytes: &mut ScratchVec<u8>,
    markers: &[&str],
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    let (mut read, mut write) = (0, 0);
    while read < bytes.len() {
        let mut longest = 0;
        for marker in markers {
            governor.charge(
                Resource::Work,
                saturating_size(marker.len().min(bytes.len() - read)).saturating_add(1),
            )?;
            if bytes.as_slice()[read..].starts_with(marker.as_bytes()) {
                longest = longest.max(marker.len());
            }
        }
        if longest > 0 {
            read += longest;
        } else {
            governor.charge(Resource::Work, 1)?;
            bytes.as_mut_slice()[write] = bytes.as_slice()[read];
            read += 1;
            write += 1;
        }
    }
    while bytes.len() > write {
        bytes.pop();
    }
    Ok(())
}

fn symbol(
    statements: &[Statement],
    name: &str,
    governor: &mut Governor,
) -> Result<Option<usize>, GrammarError> {
    for (index, statement) in statements.iter().enumerate() {
        governor.charge(Resource::Work, 1)?;
        let declared = match statement {
            Statement::Production { name, .. } | Statement::Closer { name, .. } => name.as_str(),
            _ => continue,
        };
        governor.charge(
            Resource::Work,
            saturating_size(declared.len().min(name.len())).saturating_add(1),
        )?;
        if declared == name {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn references(
    expr: &PExpr,
    statements: &[Statement],
    pending: &mut ScratchVec<usize>,
    depth: u64,
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    governor.charge(Resource::Depth, depth)?;
    governor.charge(Resource::Work, 1)?;
    match expr {
        PExpr::Name { name, .. } => {
            if let Some(index) = symbol(statements, name.as_str(), governor)? {
                pending.push(index)?;
            }
        }
        PExpr::Capture { inner, .. } | PExpr::Repeat { inner, .. } => {
            references(inner.get(), statements, pending, depth + 1, governor)?
        }
        PExpr::Sequence(items, ..) | PExpr::Choice(items, ..) => {
            for item in items {
                references(item, statements, pending, depth + 1, governor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn reachable(
    document: &Document,
    governor: &mut Governor,
) -> Result<ScratchVec<bool>, GrammarError> {
    let mut flags = ScratchVec::new(governor);
    flags.extend(core::iter::repeat_n(false, document.statements.len()))?;
    let mut pending = ScratchVec::new(governor);
    if let Some(call) = symbol(&document.statements, "call", governor)? {
        pending.push(call)?;
    }
    while let Some(index) = pending.pop() {
        governor.charge(Resource::Work, 1)?;
        if core::mem::replace(&mut flags.as_mut_slice()[index], true) {
            continue;
        }
        if let Statement::Production { expr, .. } = &document.statements.as_slice()[index] {
            references(expr, &document.statements, &mut pending, 1, governor)?;
        }
    }
    Ok(flags)
}

fn project(
    document: &mut Document,
    markers: &[&str],
    anchor: CallAnchor,
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    let reachable = reachable(document, governor)?;
    let mut removed = ScratchVec::new(governor);
    for (index, statement) in document.statements.as_mut_slice().iter_mut().enumerate() {
        governor.charge(Resource::Work, 1)?;
        if !reachable[index] {
            continue;
        }
        if let Statement::Closer {
            name, spellings, ..
        } = statement
        {
            for spelling in spellings.as_mut_slice() {
                suppress(spelling, markers, governor)?;
            }
            retain(spellings, |spelling| !spelling.is_empty());
            if spellings.is_empty() {
                let mut copy = ScratchString::new(governor);
                governor.charge(Resource::Work, saturating_size(name.len()))?;
                copy.push_str(name.as_str())?;
                removed.push(copy)?;
            }
        }
    }
    for (index, statement) in document.statements.as_mut_slice().iter_mut().enumerate() {
        governor.charge(Resource::Work, 1)?;
        if reachable[index]
            && let Statement::Production { expr, .. } = statement
        {
            expression(expr, markers, &removed, anchor, 1, governor)?;
        }
    }
    retain(
        &mut document.statements,
        |statement| !matches!(statement, Statement::Closer { spellings, .. } if spellings.is_empty()),
    );
    Ok(())
}

fn expression(
    expr: &mut PExpr,
    markers: &[&str],
    removed: &[ScratchString],
    anchor: CallAnchor,
    depth: u64,
    governor: &mut Governor,
) -> Result<(), GrammarError> {
    governor.charge(Resource::Depth, depth)?;
    governor.charge(Resource::Work, 1)?;
    let replacement = match expr {
        PExpr::Literal { bytes, at } => {
            suppress(bytes, markers, governor)?;
            bytes.is_empty().then_some(*at)
        }
        PExpr::Insert { bytes } => {
            suppress(bytes, markers, governor)?;
            bytes.is_empty().then_some(0)
        }
        PExpr::Name { name, at } => {
            let mut found = false;
            for deleted in removed {
                governor.charge(
                    Resource::Work,
                    saturating_size(deleted.len().min(name.len())).saturating_add(1),
                )?;
                if deleted.as_str() == name.as_str() {
                    found = true;
                    break;
                }
            }
            found.then_some(*at)
        }
        PExpr::Commit if anchor == CallAnchor::LineStartArguments => Some(0),
        PExpr::Slot { codec, .. }
            if anchor == CallAnchor::LineStartArguments && codec.as_str() == "json" =>
        {
            governor.charge(Resource::Work, 9)?;
            let mut replacement = ScratchString::new(governor);
            replacement.push_str("json-args")?;
            *codec = replacement;
            if let PExpr::Slot { literals, .. } = expr {
                for literal in literals.as_mut_slice() {
                    suppress(literal, markers, governor)?;
                }
                retain(literals, |literal| !literal.is_empty());
            }
            None
        }
        PExpr::Until { literals, .. } | PExpr::Slot { literals, .. } => {
            for literal in literals.as_mut_slice() {
                suppress(literal, markers, governor)?;
            }
            retain(literals, |literal| !literal.is_empty());
            None
        }
        PExpr::Capture { inner, .. } | PExpr::Repeat { inner, .. } => {
            expression(
                inner.get_mut(),
                markers,
                removed,
                anchor,
                depth + 1,
                governor,
            )?;
            None
        }
        PExpr::Sequence(items, ..) | PExpr::Choice(items, ..) => {
            for item in items.as_mut_slice() {
                expression(item, markers, removed, anchor, depth + 1, governor)?;
            }
            None
        }
        _ => None,
    };
    if let Some(at) = replacement {
        *expr = empty(at, governor);
    }
    Ok(())
}
