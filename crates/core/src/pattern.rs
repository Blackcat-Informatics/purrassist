// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Budgeted regular-expression evaluation for schema patterns.
//! Implements Unicode-scalar matching with ECMA-262 character-class semantics.
use crate::contract::{Budget, Governor, Resource, StopReason};
use core::fmt;

/// A compiled pattern; no ambient regex engine or platform-dependent dialect.
#[derive(Clone, Debug)]
pub struct Pattern {
    expression: Expr,
    captures: usize,
}

/// A precise pattern compilation error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    /// Character offset in the pattern.
    pub offset: usize,
    /// Invalid or unsupported construct.
    pub message: String,
}
impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at pattern character {}", self.message, self.offset)
    }
}
impl std::error::Error for PatternError {}

#[derive(Clone, Debug)]
enum Expr {
    Empty,
    Literal(char),
    Dot,
    Class(Vec<ClassItem>, bool),
    Sequence(Vec<Self>),
    Alternate(Vec<Self>),
    Repeat(Repetition),
    Group(usize, Box<Self>),
    Start,
    End,
    Boundary(bool),
    Look {
        inner: Box<Self>,
        positive: bool,
        behind: bool,
    },
    Backref(usize),
}
#[derive(Clone, Debug)]
struct Repetition {
    inner: Box<Expr>,
    min: usize,
    max: Option<usize>,
    greedy: bool,
}
#[derive(Clone, Debug)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    Category(String, bool),
}
#[derive(Clone)]
struct State {
    position: usize,
    captures: Vec<Option<(usize, usize)>>,
}
struct Parser<'g> {
    chars: Vec<char>,
    position: usize,
    captures: usize,
    governor: &'g mut Governor,
}

/// Governed pattern compilation failure: either the pattern itself is
/// malformed, or compiling it exceeded the caller's resource limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternCompileError {
    /// The pattern's syntax is invalid or uses an unsupported construct.
    Syntax(PatternError),
    /// Compilation stopped before a complete pattern was available.
    Resource(StopReason),
}
impl fmt::Display for PatternCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::Resource(reason) => reason.fmt(formatter),
        }
    }
}
impl std::error::Error for PatternCompileError {}
impl From<PatternError> for PatternCompileError {
    fn from(error: PatternError) -> Self {
        Self::Syntax(error)
    }
}
impl From<StopReason> for PatternCompileError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

impl Pattern {
    /// Compile the pattern, refusing unsupported constructs rather than weakening it.
    ///
    /// Delegates to [`Pattern::compile_governed`] with a default, unshared
    /// [`Governor`]. Prefer `compile_governed` to bound compilation against a
    /// caller-owned budget shared with the rest of an operation.
    ///
    /// # Errors
    /// Returns malformed syntax, excessive nesting, or an unsupported escape.
    /// A resource limit exceeded under the default budget is reported as a
    /// syntax error naming the exhausted resource, since this entry point
    /// cannot report resource usage separately.
    pub fn compile(source: &str) -> Result<Self, PatternError> {
        match Self::compile_governed(source, &mut Governor::new(Budget::default())) {
            Ok(pattern) => Ok(pattern),
            Err(PatternCompileError::Syntax(error)) => Err(error),
            Err(PatternCompileError::Resource(reason)) => Err(PatternError {
                offset: 0,
                message: format!("resource exhausted during compilation: {reason}"),
            }),
        }
    }

    /// Compile the pattern against a caller-owned governor.
    ///
    /// Charges `InputBytes` for the pattern's own length and `ScratchBytes`
    /// for the retained character buffer and syntax tree before building
    /// them, then charges `Depth`/`Work` for each nesting level and
    /// alternation parsed. The compiled pattern is retained by the caller, so
    /// its charge is never released, matching other permanently retained
    /// compiled artifacts (schemas, receipts).
    ///
    /// # Errors
    /// Returns [`PatternCompileError::Syntax`] for malformed patterns and
    /// [`PatternCompileError::Resource`] before any allocation once the
    /// governor's limits (or a shared cancellation token) would be exceeded.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::{Pattern, PatternCompileError}};
    /// let mut governor = Governor::new(Budget {
    ///     scratch_bytes: 4,
    ///     ..Budget::default()
    /// });
    /// let error = Pattern::compile_governed("a+", &mut governor).unwrap_err();
    /// assert!(matches!(error, PatternCompileError::Resource(_)));
    /// ```
    pub fn compile_governed(
        source: &str,
        governor: &mut Governor,
    ) -> Result<Self, PatternCompileError> {
        let length = u64::try_from(source.len()).unwrap_or(u64::MAX);
        governor.charge(Resource::InputBytes, length)?;
        let scalars = u64::try_from(source.chars().count()).unwrap_or(u64::MAX);
        governor.charge(
            Resource::ScratchBytes,
            scalars
                .saturating_mul(4) // the retained `chars: Vec<char>` buffer.
                .saturating_add(length.saturating_mul(48)) // an estimated AST.
                .saturating_add(256),
        )?;
        let mut parser = Parser {
            chars: source.chars().collect(),
            position: 0,
            captures: 0,
            governor,
        };
        let expression = parser.alternate(0)?;
        if parser.position != parser.chars.len() {
            return parser.fail("unexpected closing group");
        }
        Ok(Self {
            expression,
            captures: parser.captures,
        })
    }

    /// Search for a match, including unanchored matches as required by JSON Schema.
    ///
    /// # Errors
    /// Returns a governor failure; this is never converted into a non-match.
    pub fn is_match(&self, text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
        governor.charge(
            Resource::InputBytes,
            u64::try_from(text.len()).unwrap_or(u64::MAX),
        )?;
        // The scalar buffer and per-position capture states below are all
        // transient to this one search; the lease releases them on every
        // return path (match, exhaustion, or error) so a repeated search on
        // the same governor never accumulates live scratch.
        let _lease = governor.lease_scratch(
            u64::try_from(text.len())
                .unwrap_or(u64::MAX)
                .saturating_mul(4),
        )?;
        let chars: Vec<_> = text.chars().collect();
        for position in 0..=chars.len() {
            let state = State {
                position,
                captures: vec![None; self.captures + 1],
            };
            if !evaluate(&self.expression, &chars, state, governor, 0)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
impl Parser<'_> {
    fn current(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }
    fn next(&mut self) -> Option<char> {
        let character = self.current()?;
        self.position += 1;
        Some(character)
    }
    fn fail<T>(&self, message: &str) -> Result<T, PatternCompileError> {
        Err(PatternCompileError::Syntax(PatternError {
            offset: self.position,
            message: message.into(),
        }))
    }
    fn alternate(&mut self, depth: usize) -> Result<Expr, PatternCompileError> {
        // The governor's `Depth` limit (128 by default) supersedes the fixed
        // nesting cap this pattern engine used before governed compilation.
        self.governor
            .charge(Resource::Depth, u64::try_from(depth).unwrap_or(u64::MAX))?;
        self.governor.charge(Resource::Work, 1)?;
        let mut choices = vec![self.sequence(depth)?];
        while self.current() == Some('|') {
            self.position += 1;
            choices.push(self.sequence(depth)?);
        }
        if choices.len() == 1 {
            Ok(choices.remove(0))
        } else {
            Ok(Expr::Alternate(choices))
        }
    }
    fn sequence(&mut self, depth: usize) -> Result<Expr, PatternCompileError> {
        self.governor.charge(Resource::Work, 1)?;
        let mut items = Vec::new();
        while !matches!(self.current(), None | Some('|' | ')')) {
            let atom = self.atom(depth)?;
            let repeat = match self.current() {
                Some('*') => {
                    self.position += 1;
                    Some((0, None))
                }
                Some('+') => {
                    self.position += 1;
                    Some((1, None))
                }
                Some('?') => {
                    self.position += 1;
                    Some((0, Some(1)))
                }
                Some('{') => {
                    self.position += 1;
                    let min = self.count()?;
                    let max = if self.current() == Some(',') {
                        self.position += 1;
                        if self.current() == Some('}') {
                            None
                        } else {
                            Some(self.count()?)
                        }
                    } else {
                        Some(min)
                    };
                    if self.next() != Some('}') || max.is_some_and(|max| max < min) {
                        return self.fail("invalid repetition range");
                    }
                    Some((min, max))
                }
                _ => None,
            };
            if let Some((min, max)) = repeat {
                items.push(Expr::Repeat(Repetition {
                    inner: Box::new(atom),
                    min,
                    max,
                    greedy: self.current() != Some('?'),
                }));
                if self.current() == Some('?') {
                    self.position += 1;
                }
            } else {
                items.push(atom);
            }
        }
        match items.len() {
            0 => Ok(Expr::Empty),
            1 => Ok(items.remove(0)),
            _ => Ok(Expr::Sequence(items)),
        }
    }
    fn count(&mut self) -> Result<usize, PatternCompileError> {
        let mut count = 0_usize;
        let start = self.position;
        while let Some(digit) = self.current().and_then(|character| character.to_digit(10)) {
            let Some(next) = count
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::try_from(digit).ok()?))
            else {
                return self.fail("repetition exceeds address space");
            };
            count = next;
            self.position += 1;
        }
        if start == self.position {
            return self.fail("expected a repetition count");
        }
        Ok(count)
    }
    fn atom(&mut self, depth: usize) -> Result<Expr, PatternCompileError> {
        match self.next() {
            Some('^') => Ok(Expr::Start),
            Some('$') => Ok(Expr::End),
            Some('.') => Ok(Expr::Dot),
            Some('[') => self.class(),
            Some('\\') => self.escape(false),
            Some('(') => {
                let mut capture = true;
                let mut look = None;
                if self.current() == Some('?') {
                    self.position += 1;
                    capture = false;
                    match self.next() {
                        Some(':') => {}
                        Some('=') => look = Some((true, false)),
                        Some('!') => look = Some((false, false)),
                        Some('<') => match self.next() {
                            Some('=') => look = Some((true, true)),
                            Some('!') => look = Some((false, true)),
                            _ => return self.fail("named captures are not supported"),
                        },
                        _ => return self.fail("invalid group prefix"),
                    }
                }
                let index = if capture {
                    self.captures += 1;
                    self.captures
                } else {
                    0
                };
                let inner = self.alternate(depth + 1)?;
                if self.next() != Some(')') {
                    return self.fail("unterminated group");
                }
                if let Some((positive, behind)) = look {
                    Ok(Expr::Look {
                        inner: Box::new(inner),
                        positive,
                        behind,
                    })
                } else if capture {
                    Ok(Expr::Group(index, Box::new(inner)))
                } else {
                    Ok(inner)
                }
            }
            Some('*' | '+' | '?' | '{' | '}') => self.fail("quantifier without an atom"),
            Some(character) => Ok(Expr::Literal(character)),
            None => self.fail("expected an atom"),
        }
    }
    fn escape(&mut self, in_class: bool) -> Result<Expr, PatternCompileError> {
        match self.next() {
            Some('d') => Ok(Expr::Class(vec![ClassItem::Digit(true)], false)),
            Some('D') => Ok(Expr::Class(vec![ClassItem::Digit(false)], false)),
            Some('w') => Ok(Expr::Class(vec![ClassItem::Word(true)], false)),
            Some('W') => Ok(Expr::Class(vec![ClassItem::Word(false)], false)),
            Some('s') => Ok(Expr::Class(vec![ClassItem::Space(true)], false)),
            Some('S') => Ok(Expr::Class(vec![ClassItem::Space(false)], false)),
            Some('b') => {
                if in_class {
                    Ok(Expr::Literal('\u{8}'))
                } else {
                    Ok(Expr::Boundary(true))
                }
            }
            Some('B') if !in_class => Ok(Expr::Boundary(false)),
            Some('n') => Ok(Expr::Literal('\n')),
            Some('r') => Ok(Expr::Literal('\r')),
            Some('t') => Ok(Expr::Literal('\t')),
            Some('f') => Ok(Expr::Literal('\u{c}')),
            Some('v') => Ok(Expr::Literal('\u{b}')),
            Some('0') => Ok(Expr::Literal('\0')),
            Some('x') => self.hex_escape(2),
            Some('u') => self.hex_escape(4),
            Some(character @ '1'..='9') if !in_class => {
                let mut index = usize::try_from(u32::from(character) - u32::from('0')).unwrap_or(0);
                while let Some(digit) = self.current().and_then(|character| character.to_digit(10))
                {
                    index = index
                        .checked_mul(10)
                        .and_then(|index| index.checked_add(usize::try_from(digit).ok()?))
                        .ok_or_else(|| {
                            PatternCompileError::Syntax(PatternError {
                                offset: self.position,
                                message: "backreference too large".into(),
                            })
                        })?;
                    self.position += 1;
                }
                Ok(Expr::Backref(index))
            }
            Some(kind @ ('p' | 'P')) => {
                if self.next() != Some('{') {
                    return self.fail("property escape requires braces");
                }
                let start = self.position;
                while self.current().is_some_and(|c| c != '}') {
                    self.position += 1;
                }
                let name: String = self.chars[start..self.position].iter().collect();
                if self.next() != Some('}') {
                    return self.fail("unterminated property escape");
                }
                let name = name
                    .strip_prefix("General_Category=")
                    .or_else(|| name.strip_prefix("gc="))
                    .unwrap_or(&name);
                let category = category_alias(name).ok_or_else(|| {
                    PatternCompileError::Syntax(PatternError {
                        offset: start,
                        message: format!("unsupported Unicode property: {name}"),
                    })
                })?;
                Ok(Expr::Class(
                    vec![ClassItem::Category(category.into(), kind == 'p')],
                    false,
                ))
            }
            Some('c') => {
                let letter = self
                    .next()
                    .filter(char::is_ascii_alphabetic)
                    .ok_or_else(|| {
                        PatternCompileError::Syntax(PatternError {
                            offset: self.position,
                            message: "control escape requires an ASCII letter".into(),
                        })
                    })?;
                Ok(Expr::Literal(char::from(
                    u8::try_from(u32::from(letter)).expect("ASCII letter") % 32,
                )))
            }
            Some('k') => self.fail("unsupported named-reference escape"),
            Some(character) => Ok(Expr::Literal(character)),
            None => self.fail("trailing escape"),
        }
    }
    fn hex_escape(&mut self, count: usize) -> Result<Expr, PatternCompileError> {
        let mut value = 0;
        for _ in 0..count {
            let Some(digit) = self.next().and_then(|character| character.to_digit(16)) else {
                return self.fail("invalid hexadecimal escape");
            };
            value = (value << 4) | digit;
        }
        char::from_u32(value).map(Expr::Literal).ok_or_else(|| {
            PatternCompileError::Syntax(PatternError {
                offset: self.position,
                message: "surrogate escape requires a Unicode scalar".into(),
            })
        })
    }
    fn class_atom(&mut self) -> Result<Vec<ClassItem>, PatternCompileError> {
        let expression = if self.current() == Some('\\') {
            self.position += 1;
            self.escape(true)?
        } else {
            Expr::Literal(self.next().ok_or_else(|| {
                PatternCompileError::Syntax(PatternError {
                    offset: self.position,
                    message: "unterminated character class".into(),
                })
            })?)
        };
        match expression {
            Expr::Literal(character) => Ok(vec![ClassItem::Range(character, character)]),
            Expr::Class(items, false) => Ok(items),
            _ => self.fail("invalid character class atom"),
        }
    }
    fn class(&mut self) -> Result<Expr, PatternCompileError> {
        let negative = self.current() == Some('^');
        if negative {
            self.position += 1;
        }
        let mut items = Vec::new();
        while self.current() != Some(']') {
            if self.current().is_none() {
                return self.fail("unterminated character class");
            }
            let first = self.class_atom()?;
            if self.current() == Some('-') && self.chars.get(self.position + 1) != Some(&']') {
                self.position += 1;
                let second = self.class_atom()?;
                match (&first[..], &second[..]) {
                    ([ClassItem::Range(start, a)], [ClassItem::Range(end, b)])
                        if start == a && end == b && start <= end =>
                    {
                        items.push(ClassItem::Range(*start, *end));
                    }
                    _ => return self.fail("invalid character class range"),
                }
            } else {
                items.extend(first);
            }
        }
        self.position += 1;
        Ok(Expr::Class(items, negative))
    }
}
const fn word(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
const fn space(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}
fn class_matches(items: &[ClassItem], character: char) -> bool {
    items.iter().any(|item| match item {
        ClassItem::Range(start, end) => (*start..=*end).contains(&character),
        ClassItem::Digit(positive) => character.is_ascii_digit() == *positive,
        ClassItem::Word(positive) => word(character) == *positive,
        ClassItem::Space(positive) => space(character) == *positive,
        ClassItem::Category(category, positive) => {
            crate::text::general_category(character).starts_with(category) == *positive
        }
    })
}
fn evaluate(
    expression: &Expr,
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<State>, StopReason> {
    governor.charge(Resource::Work, 1)?;
    governor.charge(Resource::Depth, u64::from(depth))?;
    // Released on every exit from this call (there are many, across the
    // match below and inside `assertion`/`repeat`), so a bounded but deep
    // backtracking search never leaves scratch charged once it unwinds.
    let _lease = governor.lease_scratch(
        u64::try_from(state.captures.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(32)
            .saturating_add(64),
    )?;
    let position = state.position;
    let single = |accept: bool, mut state: State, advance: usize| {
        if accept {
            state.position += advance;
            vec![state]
        } else {
            Vec::new()
        }
    };
    match expression {
        Expr::Empty => Ok(vec![state]),
        Expr::Literal(character) => Ok(single(text.get(position) == Some(character), state, 1)),
        Expr::Dot => Ok(single(
            text.get(position).is_some_and(|character| {
                !matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
            }),
            state,
            1,
        )),
        Expr::Class(items, negative) => Ok(single(
            text.get(position)
                .is_some_and(|character| class_matches(items, *character) != *negative),
            state,
            1,
        )),
        Expr::Start => Ok(single(position == 0, state, 0)),
        Expr::End => Ok(single(position == text.len(), state, 0)),
        Expr::Boundary(positive) => {
            let before = position
                .checked_sub(1)
                .and_then(|index| text.get(index))
                .is_some_and(|character| word(*character));
            let after = text.get(position).is_some_and(|character| word(*character));
            Ok(single((before != after) == *positive, state, 0))
        }
        Expr::Sequence(items) => {
            let mut states = vec![state];
            for item in items {
                let mut next = Vec::new();
                for state in states {
                    next.extend(evaluate(item, text, state, governor, depth + 1)?);
                }
                states = next;
                if states.is_empty() {
                    break;
                }
            }
            Ok(states)
        }
        Expr::Alternate(items) => {
            let mut results = Vec::new();
            for item in items {
                results.extend(evaluate(item, text, state.clone(), governor, depth + 1)?);
            }
            Ok(results)
        }
        Expr::Group(index, inner) => {
            let mut results = evaluate(inner, text, state, governor, depth + 1)?;
            for result in &mut results {
                result.captures[*index] = Some((position, result.position));
            }
            Ok(results)
        }
        Expr::Backref(index) => {
            let Some(Some((start, end))) = state.captures.get(*index) else {
                return Ok(vec![state]);
            };
            let length = end - start;
            Ok(single(
                text.get(position..position.saturating_add(length)) == text.get(*start..*end),
                state,
                length,
            ))
        }
        Expr::Look {
            inner,
            positive,
            behind,
        } => assertion(inner, (*positive, *behind), text, state, governor, depth),
        Expr::Repeat(repetition) => repeat(repetition, text, state, governor, depth),
    }
}
fn assertion(
    inner: &Expr,
    flags: (bool, bool),
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<State>, StopReason> {
    let (positive, behind) = flags;
    let position = state.position;
    let starts = if behind {
        0..=position
    } else {
        position..=position
    };
    let mut found = None;
    for start in starts {
        let mut probe = state.clone();
        probe.position = start;
        for mut result in evaluate(inner, text, probe, governor, depth + 1)? {
            if !behind || result.position == position {
                result.position = position;
                found = Some(result);
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    if positive {
        Ok(found.into_iter().collect())
    } else {
        Ok(if found.is_none() {
            vec![state]
        } else {
            Vec::new()
        })
    }
}
fn repeat(
    repetition: &Repetition,
    text: &[char],
    state: State,
    governor: &mut Governor,
    depth: u32,
) -> Result<Vec<State>, StopReason> {
    enum Frame {
        Expand(State, usize),
        Accept(State),
    }
    let bound = repetition
        .max
        .unwrap_or_else(|| text.len().saturating_add(repetition.min).saturating_add(1));
    // Every iteration below can clone `state` (once to keep exploring, once
    // to record an `Accept` frame) before `evaluate`'s own per-call lease
    // charges for the clone it receives; bracket the whole backtracking
    // search's `stack`/`results` growth and those extra clones in one lease,
    // released when this call returns (matching `evaluate`'s own pattern),
    // so a long repeated match never leaves scratch charged once `is_match`
    // returns.
    let clone_bytes = u64::try_from(state.captures.len().saturating_add(1))
        .unwrap_or(u64::MAX)
        .saturating_mul(32);
    let _lease = governor.lease_scratch(
        u64::try_from(bound)
            .unwrap_or(u64::MAX)
            .saturating_mul(clone_bytes.saturating_mul(3).saturating_add(64)),
    )?;
    let mut stack = vec![Frame::Expand(state, 0)];
    let mut results = Vec::new();
    while let Some(frame) = stack.pop() {
        governor.charge(Resource::Work, 1)?;
        let Frame::Expand(state, count) = frame else {
            if let Frame::Accept(state) = frame {
                results.push(state);
            }
            continue;
        };
        let accept = count >= repetition.min;
        if accept && repetition.greedy {
            stack.push(Frame::Accept(state.clone()));
        }
        if count < bound {
            let before = state.position;
            let advanced = evaluate(&repetition.inner, text, state.clone(), governor, depth + 1)?;
            for next in advanced.into_iter().rev() {
                if next.position != before || count < repetition.min {
                    stack.push(Frame::Expand(next, count + 1));
                }
            }
        }
        if accept && !repetition.greedy {
            stack.push(Frame::Accept(state));
        }
    }
    Ok(results)
}
fn category_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "L" | "Letter" => "L",
        "Lu" | "Uppercase_Letter" => "Lu",
        "Ll" | "Lowercase_Letter" => "Ll",
        "Lt" | "Titlecase_Letter" => "Lt",
        "Lm" | "Modifier_Letter" => "Lm",
        "Lo" | "Other_Letter" => "Lo",
        "M" | "Mark" => "M",
        "Mn" | "Nonspacing_Mark" => "Mn",
        "Mc" | "Spacing_Mark" => "Mc",
        "Me" | "Enclosing_Mark" => "Me",
        "N" | "Number" => "N",
        "Nd" | "Decimal_Number" | "digit" => "Nd",
        "Nl" | "Letter_Number" => "Nl",
        "No" | "Other_Number" => "No",
        "P" | "Punctuation" => "P",
        "Pc" | "Connector_Punctuation" => "Pc",
        "Pd" | "Dash_Punctuation" => "Pd",
        "Ps" | "Open_Punctuation" => "Ps",
        "Pe" | "Close_Punctuation" => "Pe",
        "Pi" | "Initial_Punctuation" => "Pi",
        "Pf" | "Final_Punctuation" => "Pf",
        "Po" | "Other_Punctuation" => "Po",
        "S" | "Symbol" => "S",
        "Sm" | "Math_Symbol" => "Sm",
        "Sc" | "Currency_Symbol" => "Sc",
        "Sk" | "Modifier_Symbol" => "Sk",
        "So" | "Other_Symbol" => "So",
        "Z" | "Separator" => "Z",
        "Zs" | "Space_Separator" => "Zs",
        "Zl" | "Line_Separator" => "Zl",
        "Zp" | "Paragraph_Separator" => "Zp",
        "C" | "Other" => "C",
        "Cc" | "Control" => "Cc",
        "Cf" | "Format" => "Cf",
        "Cs" | "Surrogate" => "Cs",
        "Co" | "Private_Use" => "Co",
        "Cn" | "Unassigned" => "Cn",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;
    #[test]
    fn assertions_classes_and_repetition() {
        for (pattern, text, expected) in [
            ("^[a-z]+[0-9]{2}$", "ab12", true),
            ("^[a-z]+[0-9]{2}$", "ab1", false),
            ("foo|bar", "xxbarxx", true),
            ("(?=abc)ab", "abc", true),
            ("a(?!b)", "ab", false),
            ("(?<=ab)c", "abc", true),
            (r"^(ab)\1$", "abab", true),
            (r"^\d+$", "１２", false),
            (r"^\s$", "\u{a0}", true),
            (".", "\u{2028}", false),
            ("[^]", "\n", true),
            ("^(a?)*$", "", true),
            ("^a$", "a\n", false),
        ] {
            let compiled = Pattern::compile(pattern).expect(pattern);
            assert_eq!(
                compiled.is_match(text, &mut Governor::new(Budget::default())),
                Ok(expected),
                "{pattern:?} {text:?}"
            );
        }
    }
    #[test]
    fn exhaustion_is_not_false() {
        let pattern = Pattern::compile("(a+)+$").expect("pattern");
        assert!(
            pattern
                .is_match(
                    "aaaaaaaaaaaa!",
                    &mut Governor::new(Budget {
                        work: 10,
                        ..Budget::default()
                    })
                )
                .is_err()
        );
    }
}
