// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

use super::{Member, Node, Value};
use crate::contract::{Budget, Governor, Resource, StopReason};
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
/// # Errors
/// Returns syntax errors or a sticky governor failure. Unicode strings must decode
/// to scalar values; isolated surrogate escapes are reported explicitly.
pub fn parse_with(input: &str, governor: &mut Governor) -> Result<Node, ParseFailure> {
    governor.charge(Resource::InputBytes, size(input.len()))?;
    let mut parser = Parser {
        input,
        position: 0,
        governor,
    };
    parser.whitespace()?;
    let value = parser.value(0)?;
    parser.whitespace()?;
    if parser.position != input.len() {
        return parser.fail("trailing input");
    }
    Ok(value)
}

struct Parser<'a, 'g> {
    input: &'a str,
    position: usize,
    governor: &'g mut Governor,
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
impl Parser<'_, '_> {
    fn fail<T>(&self, message: &str) -> Result<T, ParseFailure> {
        Err(ParseFailure::Syntax(ParseError {
            offset: self.position,
            message: message.into(),
        }))
    }
    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.position).copied()
    }
    fn advance(&mut self, count: usize) -> Result<(), ParseFailure> {
        self.governor.charge(Resource::Work, size(count))?;
        self.position += count;
        Ok(())
    }
    fn whitespace(&mut self) -> Result<(), ParseFailure> {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.advance(1)?;
        }
        Ok(())
    }
    fn expect(&mut self, byte: u8) -> Result<(), ParseFailure> {
        if self.byte() != Some(byte) {
            return self.fail(&format!("expected '{}'", char::from(byte)));
        }
        self.advance(1)
    }
    fn value(&mut self, depth: u32) -> Result<Node, ParseFailure> {
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        self.governor
            .charge(Resource::ScratchBytes, size(core::mem::size_of::<Node>()))?;
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
    fn literal(&mut self, literal: &str) -> Result<(), ParseFailure> {
        if !self.input[self.position..].starts_with(literal) {
            return self.fail(&format!("expected {literal}"));
        }
        self.advance(literal.len())
    }
    fn number(&mut self) -> Result<Value, ParseFailure> {
        let start = self.position;
        while matches!(
            self.byte(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.advance(1)?;
        }
        let lexical = &self.input[start..self.position];
        self.governor
            .charge(Resource::ScratchBytes, size(lexical.len()))?;
        lexical.parse().map(Value::Number).map_err(|_| {
            ParseFailure::Syntax(ParseError {
                offset: start,
                message: "invalid JSON number".into(),
            })
        })
    }
    fn string(&mut self) -> Result<String, ParseFailure> {
        self.expect(b'"')?;
        let mut output = String::new();
        loop {
            match self.byte() {
                Some(b'"') => {
                    self.advance(1)?;
                    return Ok(output);
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
                            self.push_character(&mut output, character)?;
                            continue;
                        }
                        _ => return self.fail("invalid string escape"),
                    };
                    self.advance(1)?;
                    self.push_character(&mut output, character)?;
                }
                Some(0..=0x1f) => return self.fail("unescaped control character"),
                Some(_) => {
                    let Some(character) = self.input[self.position..].chars().next() else {
                        return self.fail("unterminated string");
                    };
                    self.advance(character.len_utf8())?;
                    self.push_character(&mut output, character)?;
                }
                None => return self.fail("unterminated string"),
            }
        }
    }
    fn push_character(&mut self, output: &mut String, character: char) -> Result<(), ParseFailure> {
        if output.len() + character.len_utf8() > output.capacity() {
            let capacity = (output.capacity().saturating_mul(2))
                .max(output.len() + character.len_utf8())
                .max(8);
            self.governor
                .charge(Resource::ScratchBytes, size(capacity - output.capacity()))?;
            output
                .try_reserve_exact(capacity - output.len())
                .map_err(|_| {
                    ParseFailure::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes))
                })?;
        }
        output.push(character);
        Ok(())
    }
    fn hex4(&mut self) -> Result<u32, ParseFailure> {
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
    fn array(&mut self, depth: u32) -> Result<Value, ParseFailure> {
        self.expect(b'[')?;
        self.whitespace()?;
        let mut items = Vec::new();
        if self.byte() == Some(b']') {
            self.advance(1)?;
            return Ok(Value::Array(items));
        }
        loop {
            let item = self.value(depth + 1)?;
            self.reserve(&mut items)?;
            items.push(item);
            self.whitespace()?;
            if self.byte() == Some(b']') {
                self.advance(1)?;
                break;
            }
            self.expect(b',')?;
            self.whitespace()?;
        }
        Ok(Value::Array(items))
    }
    fn object(&mut self, depth: u32) -> Result<Value, ParseFailure> {
        self.expect(b'{')?;
        self.whitespace()?;
        let mut members = Vec::new();
        if self.byte() == Some(b'}') {
            self.advance(1)?;
            return Ok(Value::Object(members));
        }
        loop {
            let start = self.position;
            let key = self.string()?;
            let key_span = Some(start..self.position);
            self.whitespace()?;
            self.expect(b':')?;
            self.whitespace()?;
            let value = self.value(depth + 1)?;
            self.reserve(&mut members)?;
            members.push(Member {
                key,
                key_span,
                value,
            });
            self.whitespace()?;
            if self.byte() == Some(b'}') {
                self.advance(1)?;
                break;
            }
            self.expect(b',')?;
            self.whitespace()?;
        }
        Ok(Value::Object(members))
    }
    fn reserve<T>(&mut self, values: &mut Vec<T>) -> Result<(), ParseFailure> {
        if values.len() == values.capacity() {
            let capacity = values.capacity().saturating_mul(2).max(4);
            self.governor.charge(
                Resource::ScratchBytes,
                size((capacity - values.capacity()).saturating_mul(core::mem::size_of::<T>())),
            )?;
            values
                .try_reserve_exact(capacity - values.len())
                .map_err(|_| {
                    ParseFailure::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes))
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ParseFailure, parse, parse_with};
    use crate::{Budget, Governor};
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
