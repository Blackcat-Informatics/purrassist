// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

use super::{Member, Node, OwnedDocument, OwnedParseFailure, Syntax, SyntaxMessage, Value};
use crate::contract::saturating_size as size;
use crate::contract::{
    Budget, Governor, Resource, ScratchCharge, ScratchLease, ScratchString, ScratchVec, StopReason,
    format_output_governed,
};
use core::fmt;

/// A syntax error at an exact original byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Original byte offset.
    pub offset: usize,
    /// Expected syntax or failed Unicode condition.
    pub message: String,
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}
impl std::error::Error for ParseError {}

/// Parsing distinguishes malformed input from exhausted resources.
///
/// The `From<StopReason>` conversion preserves the exact reason in this JSON
/// error type. Pattern compilation and schema compilation use their own
/// nominal error enums, whose syntax diagnostics have different contracts:
/// JSON reports original byte offsets, patterns report character offsets,
/// and schemas report schema locations. These typed conversions do not
/// classify, relabel or swallow a resource outcome.
///
/// ```
/// use purrassist_core::{StopReason, json::ParseFailure};
/// let failure = ParseFailure::from(StopReason::Cancelled);
/// assert_eq!(failure, ParseFailure::Resource(StopReason::Cancelled));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseFailure {
    /// Input does not satisfy JSON syntax.
    Syntax(ParseError),
    /// Resource admission or continued processing failed.
    Resource(StopReason),
}
impl fmt::Display for ParseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(f),
            Self::Resource(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ParseFailure {}
impl From<StopReason> for ParseFailure {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// Parse strict JSON with default resource limits, retaining duplicate occurrences.
///
/// # Errors
/// Returns precise syntax errors, invalid surrogate escapes, or resource exhaustion.
pub fn parse(input: &str) -> Result<Node, ParseFailure> {
    parse_with(input, &mut Governor::new(Budget::default()))
}

/// Parse with a shared governor, charging before admitting source and tree storage.
///
/// Construction buffers release on failure. A successful ordinary [`Node`]
/// retains its storage admission because the caller may subsequently mutate it;
/// dropping that node does not release the governor's published-output charge.
/// Syntax diagnostics admit their own output storage after construction releases.
///
/// ```
/// use purrassist_core::{Budget, Governor, json::parse_with};
/// let mut governor = Governor::new(Budget::default());
/// let node = parse_with(r#"{"x":1,"x":2}"#, &mut governor).unwrap();
/// assert!(node.get("x").is_none()); // Duplicate occurrences remain ambiguous.
/// assert!(governor.scratch_meter().live() > 0);
/// ```
///
/// # Errors
/// Returns syntax errors or a sticky governor failure. Unicode strings must decode
/// to scalar values; isolated surrogate escapes are reported explicitly.
pub fn parse_with(input: &str, governor: &mut Governor) -> Result<Node, ParseFailure> {
    match OwnedDocument::parse(input, governor) {
        Ok(document) => Ok(document.into_node()),
        Err(OwnedParseFailure::Resource(reason)) => Err(ParseFailure::Resource(reason)),
        Err(OwnedParseFailure::Syntax(error)) => {
            let message = format_output_governed(format_args!("{}", error.message), governor)?;
            Err(ParseFailure::Syntax(ParseError {
                offset: error.offset,
                message,
            }))
        }
    }
}

impl OwnedDocument {
    /// Strict parsing keeps every completed allocation in one document reservation.
    /// Partial trees and their reservations unwind together on any failure.
    pub(crate) fn parse(
        input: &str,
        governor: &mut Governor,
    ) -> Result<Self, OwnedParseFailure> {
        governor.charge(Resource::InputBytes, size(input.len()))?;
        let lease = governor.lease_scratch(0)?;
        let mut parser = Parser {
            input,
            position: 0,
            governor,
            lease,
        };
        parser.whitespace()?;
        let root = parser.value(0)?;
        parser.whitespace()?;
        if parser.position != input.len() {
            return parser.fail("trailing input");
        }
        parser.governor.check()?;
        Ok(Self {
            root,
            lease: parser.lease,
        })
    }
}

impl From<StopReason> for OwnedParseFailure {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}
impl fmt::Display for Syntax {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}
impl fmt::Display for SyntaxMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(message) => f.write_str(message),
            Self::ExpectedByte(byte) => write!(f, "expected '{}'", char::from(*byte)),
            Self::ExpectedLiteral(literal) => write!(f, "expected {literal}"),
        }
    }
}

struct Parser<'a, 'g> {
    input: &'a str,
    position: usize,
    governor: &'g mut Governor,
    lease: ScratchLease,
}
impl Parser<'_, '_> {
    const fn fail<T>(&self, message: &'static str) -> Result<T, OwnedParseFailure> {
        Err(self.syntax(SyntaxMessage::Static(message)))
    }
    const fn syntax(&self, message: SyntaxMessage) -> OwnedParseFailure {
        OwnedParseFailure::Syntax(Syntax {
            offset: self.position,
            message,
        })
    }
    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }
    fn advance(&mut self, count: usize) -> Result<(), OwnedParseFailure> {
        self.governor.charge(Resource::Work, size(count))?;
        self.position += count;
        Ok(())
    }
    fn whitespace(&mut self) -> Result<(), OwnedParseFailure> {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.advance(1)?;
        }
        Ok(())
    }
    fn expect(&mut self, byte: u8) -> Result<(), OwnedParseFailure> {
        if self.byte() != Some(byte) {
            return Err(self.syntax(SyntaxMessage::ExpectedByte(byte)));
        }
        self.advance(1)
    }
    fn value(&mut self, depth: u32) -> Result<Node, OwnedParseFailure> {
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        let start = self.position;
        let value = match self.byte() {
            Some(b'n') => {
                self.literal("null")?;
                Value::Null
            }
            Some(b't') => {
                self.literal("true")?;
                Value::Bool(true)
            }
            Some(b'f') => {
                self.literal("false")?;
                Value::Bool(false)
            }
            Some(b'"') => Value::String(self.string()?),
            Some(b'[') => self.array(depth)?,
            Some(b'{') => self.object(depth)?,
            Some(b'-' | b'0'..=b'9') => self.number()?,
            _ => return self.fail("expected a JSON value"),
        };
        Ok(Node {
            value,
            span: Some(start..self.position),
        })
    }
    fn literal(&mut self, literal: &'static str) -> Result<(), OwnedParseFailure> {
        if !self.input[self.position..].starts_with(literal) {
            return Err(self.syntax(SyntaxMessage::ExpectedLiteral(literal)));
        }
        self.advance(literal.len())
    }
    fn number(&mut self) -> Result<Value, OwnedParseFailure> {
        let start = self.position;
        while matches!(
            self.byte(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.advance(1)?;
        }
        let lexical = &self.input[start..self.position];
        super::Number::retain_owned(lexical, self.governor, &mut self.lease)
            .map(Value::Number)
            .map_err(|failure| match failure {
                super::NumberParseError::Syntax(_) => OwnedParseFailure::Syntax(Syntax {
                    offset: start,
                    message: SyntaxMessage::Static("invalid JSON number"),
                }),
                super::NumberParseError::Resource(reason) => OwnedParseFailure::Resource(reason),
            })
    }
    fn string(&mut self) -> Result<String, OwnedParseFailure> {
        self.expect(b'"')?;
        let mut output = ScratchString::new(self.governor);
        loop {
            match self.byte() {
                Some(b'"') => {
                    self.advance(1)?;
                    let (text, lease) = output.into_owned_parts();
                    self.lease.absorb(lease);
                    return Ok(text);
                }
                Some(b'\\') => {
                    self.advance(1)?;
                    let character = match self.byte() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.advance(1)?;
                            let first = self.hex4()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let second = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return self.fail("expected a low surrogate");
                                }
                                0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
                            } else {
                                first
                            };
                            let Some(character) = char::from_u32(scalar) else {
                                return self.fail("isolated surrogate escape");
                            };
                            push_char(&mut output, character, self.governor)?;
                            continue;
                        }
                        _ => return self.fail("invalid string escape"),
                    };
                    self.advance(1)?;
                    push_char(&mut output, character, self.governor)?;
                }
                Some(0..=0x1f) => return self.fail("unescaped control character"),
                Some(_) => {
                    let Some(character) = self.input[self.position..].chars().next() else {
                        return self.fail("unterminated string");
                    };
                    self.advance(character.len_utf8())?;
                    push_char(&mut output, character, self.governor)?;
                }
                None => return self.fail("unterminated string"),
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, OwnedParseFailure> {
        let mut value = 0;
        for _ in 0..4 {
            let Some(digit) = self.byte().and_then(|byte| char::from(byte).to_digit(16)) else {
                return self.fail("expected four hexadecimal digits");
            };
            value = (value << 4) | digit;
            self.advance(1)?;
        }
        Ok(value)
    }
    fn array(&mut self, depth: u32) -> Result<Value, OwnedParseFailure> {
        self.expect(b'[')?;
        self.whitespace()?;
        let mut items = ScratchVec::new(self.governor);
        if self.byte() == Some(b']') {
            self.advance(1)?;
            return Ok(Value::Array(self.retain_vec(items)));
        }
        loop {
            let item = self.value(depth + 1)?;
            push(&mut items, item, self.governor)?;
            self.whitespace()?;
            if self.byte() == Some(b']') {
                self.advance(1)?;
                break;
            }
            self.expect(b',')?;
            self.whitespace()?;
        }
        Ok(Value::Array(self.retain_vec(items)))
    }
    fn object(&mut self, depth: u32) -> Result<Value, OwnedParseFailure> {
        self.expect(b'{')?;
        self.whitespace()?;
        let mut members = ScratchVec::new(self.governor);
        if self.byte() == Some(b'}') {
            self.advance(1)?;
            return Ok(Value::Object(self.retain_vec(members)));
        }
        loop {
            let start = self.position;
            let key = self.string()?;
            let key_span = Some(start..self.position);
            self.whitespace()?;
            self.expect(b':')?;
            self.whitespace()?;
            let value = self.value(depth + 1)?;
            push(
                &mut members,
                Member {
                    key,
                    key_span,
                    value,
                },
                self.governor,
            )?;
            self.whitespace()?;
            if self.byte() == Some(b'}') {
                self.advance(1)?;
                break;
            }
            self.expect(b',')?;
            self.whitespace()?;
        }
        Ok(Value::Object(self.retain_vec(members)))
    }
    fn retain_vec<T: ScratchCharge>(&mut self, values: ScratchVec<T>) -> Vec<T> {
        let (values, lease) = values.into_owned_parts();
        self.lease.absorb(lease);
        values
    }
}

fn push_char(
    output: &mut ScratchString,
    character: char,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let moved = if character.len_utf8() > output.capacity() - output.len() {
        output.len()
    } else {
        0
    };
    governor.charge(
        Resource::Work,
        size(moved).saturating_add(size(character.len_utf8())),
    )?;
    output.push(character)
}

fn push<T: ScratchCharge>(
    values: &mut ScratchVec<T>,
    value: T,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let moved = if values.len() == values.capacity() {
        values.len()
    } else {
        0
    };
    governor.charge(Resource::Work, size(moved).saturating_add(1))?;
    values.push(value)
}

#[cfg(test)]
mod tests {
    use super::{OwnedDocument, OwnedParseFailure, ParseFailure, parse, parse_with};
    use crate::{Budget, Governor, StopReason, contract::Resource};
    #[test]
    fn duplicate_members_and_spans_survive() {
        let input = r#" {"a":1,"a":2,"nested":["\uD83D\uDE00",1.2300e4]} "#;
        let root = parse(input).expect("valid");
        assert!(root.get("a").is_none());
        assert_eq!(
            root.pointer("/nested/0").and_then(super::Node::as_str),
            Some("😀")
        );
        assert_eq!(root.to_json(), r#"{"a":1,"a":2,"nested":["😀",1.2300e4]}"#);
        assert_eq!(root.span, Some(1..input.len() - 1));
        let mut governor = Governor::new(Budget::default());
        let document = OwnedDocument::parse(input, &mut governor).expect("owned valid");
        assert_eq!(document.root(), &root);
        assert!(governor.scratch_meter().live() > 0);
        drop(document);
        assert_eq!(governor.scratch_meter().live(), 0);
    }
    #[test]
    fn owned_documents_release_on_syntax_and_resource_refusal() {
        let input = r#"{"nested":[{"text":"é\uD83D\uDE00","number":1.2300e400},false]}"#;
        let mut measured = Governor::new(Budget::default());
        let document = OwnedDocument::parse(input, &mut measured).expect("measure valid");
        let usage = measured.usage();
        drop(document);
        assert_eq!(measured.scratch_meter().live(), 0);
        for (work, scratch_bytes, refused) in [
            (usage.work, usage.peak_scratch_bytes, None),
            (
                usage.work - 1,
                usage.peak_scratch_bytes,
                Some(Resource::Work),
            ),
            (
                usage.work,
                usage.peak_scratch_bytes - 1,
                Some(Resource::ScratchBytes),
            ),
        ] {
            let mut governor = Governor::new(Budget {
                work,
                scratch_bytes,
                ..Budget::default()
            });
            let result = OwnedDocument::parse(input, &mut governor);
            if let Some(resource) = refused {
                assert!(matches!(result, Err(OwnedParseFailure::Resource(
                    StopReason::BudgetExceeded(actual)
                )) if actual == resource));
            } else {
                assert!(result.is_ok(), "exact measured bounds admit the document");
                drop(result);
            }
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        for malformed in [format!("{input} trailing"), format!(r#"[{input},"bad\x"]"#)] {
            let mut governor = Governor::new(Budget::default());
            let failure = OwnedDocument::parse(&malformed, &mut governor).unwrap_err();
            assert!(matches!(failure, OwnedParseFailure::Syntax(_)));
            assert!(governor.usage().peak_scratch_bytes > 0);
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        let mut published = Governor::new(Budget::default());
        drop(parse_with(input, &mut published).expect("published valid"));
        assert!(published.scratch_meter().live() > 0);
    }
    #[test]
    fn strict_rejection_and_pointer_rules() {
        for input in [
            "[1,]",
            "{\"a\":1,}",
            "01",
            "[true false]",
            "\u{a0}1",
            "\"\\uD800\"",
            "\"\\x20\"",
            "{}{}",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
        let root = parse(r#"{"a/b":{"~":[3]}}"#).expect("valid");
        assert_eq!(root.pointer("/a~1b/~0/0").expect("pointer").to_json(), "3");
        assert!(root.pointer("/a~1b/~0/00").is_none());
        assert!(root.pointer("/a~2b").is_none());
    }
    #[test]
    fn budget_failure_is_not_a_syntax_error() {
        let mut governor = Governor::new(Budget {
            depth: 2,
            ..Budget::default()
        });
        assert!(matches!(
            parse_with("[[[0]]]", &mut governor),
            Err(ParseFailure::Resource(_))
        ));
    }
}
