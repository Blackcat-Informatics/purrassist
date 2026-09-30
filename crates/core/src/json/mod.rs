// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Ordered, source-addressed JSON with exact decimal values.
mod number;
mod parser;
mod repair;
use crate::contract::saturating_size as size;
use core::fmt::Write;
use core::ops::Range;
pub use number::{Number, NumberError};
pub use parser::{ParseError, ParseFailure, parse, parse_with};
#[derive(Clone, Debug)]
pub(crate) struct SyntaxEdit {
    pub rule: &'static str,
    pub span: core::ops::Range<usize>,
    pub replacement: String,
    pub guessed: bool,
}
pub(crate) fn recover_syntax(
    input: &str,
    governor: &mut crate::Governor,
) -> Result<(Node, usize, Vec<SyntaxEdit>), crate::StopReason> {
    repair::recover(input, governor)
}

/// A JSON value occurrence with an optional original byte span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The decoded value; objects retain ordering and duplicate occurrences.
    pub value: Value,
    /// Half-open source range; generated nodes have no original span.
    pub span: Option<Range<usize>>,
}

/// An ordered member occurrence; duplicate keys are never overwritten by parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Decoded member name.
    pub key: String,
    /// Source span of the quoted member name.
    pub key_span: Option<Range<usize>>,
    /// Member value and source location.
    pub value: Node,
}

/// The JSON data model, retaining exact numbers and duplicate members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// JSON null.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Arbitrary-precision decimal spelling.
    Number(Number),
    /// Decoded Unicode string.
    String(String),
    /// Ordered array occurrences.
    Array(Vec<Node>),
    /// Ordered object member occurrences.
    Object(Vec<Member>),
}

impl Node {
    /// Construct a generated value without claiming an original source span.
    #[must_use]
    pub const fn new(value: Value) -> Self {
        Self { value, span: None }
    }

    /// Get a uniquely named object member; duplicates deliberately yield `None`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        let Value::Object(members) = &self.value else {
            return None;
        };
        let mut matches = members.iter().filter(|member| member.key == key);
        let first = &matches.next()?.value;
        if matches.next().is_some() {
            None
        } else {
            Some(first)
        }
    }

    /// Borrow a decoded string, when this occurrence is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        if let Value::String(value) = &self.value {
            Some(value)
        } else {
            None
        }
    }

    /// Resolve an RFC 6901 JSON Pointer. Ambiguous duplicate members fail lookup.
    #[must_use]
    pub fn pointer(&self, pointer: &str) -> Option<&Self> {
        if pointer.is_empty() {
            return Some(self);
        }
        let mut current = self;
        for part in pointer.strip_prefix('/')?.split('/') {
            let mut key = String::new();
            let mut chars = part.chars();
            while let Some(character) = chars.next() {
                key.push(if character == '~' {
                    match chars.next()? {
                        '0' => '~',
                        '1' => '/',
                        _ => return None,
                    }
                } else {
                    character
                });
            }
            current = match &current.value {
                Value::Object(_) => current.get(&key)?,
                Value::Array(items) => {
                    if key.is_empty()
                        || !key.bytes().all(|b| b.is_ascii_digit())
                        || (key.len() > 1 && key.starts_with('0'))
                    {
                        return None;
                    }
                    items.get(key.parse::<usize>().ok()?)?
                }
                _ => return None,
            };
        }
        Some(current)
    }

    /// Semantic JSON equality, independent of source spans and object member order.
    /// Object duplicates compare as ordered occurrences instead of disappearing.
    #[must_use]
    pub fn equivalent(&self, other: &Self) -> bool {
        match (&self.value, &other.value) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.equivalent(b))
            }
            (Value::Object(a), Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter().all(|member| {
                        let left: Vec<_> =
                            a.iter().filter(|entry| entry.key == member.key).collect();
                        let right: Vec<_> =
                            b.iter().filter(|entry| entry.key == member.key).collect();
                        left.len() == right.len()
                            && left
                                .iter()
                                .zip(right)
                                .all(|(a, b)| a.value.equivalent(&b.value))
                    })
            }
            _ => false,
        }
    }

    /// Serialize to compact JSON, preserving member order and number spelling.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        self.write_json(&mut output);
        output
    }

    /// Estimate this value's heap footprint without serializing or cloning it.
    ///
    /// A conservative upper bound on the bytes a `.clone()` or a `.to_json()`
    /// call would need, computed by the same O(size) traversal those
    /// operations perform but without allocating anything: engines charge
    /// scratch with this estimate *before* cloning or serializing a value
    /// whose exact size is not otherwise known.
    ///
    /// ```
    /// use purrassist_core::json::{Node, Value};
    /// let node = Node::new(Value::String("hello".into()));
    /// assert!(node.estimated_bytes() >= 5);
    /// ```
    #[must_use]
    pub fn estimated_bytes(&self) -> u64 {
        // Uses `Self`/`Member`'s portable `ScratchCharge::CHARGE`, never
        // `size_of`, so this estimate (and every governor decision and
        // reported scratch usage it feeds) is identical on every target.
        let mut total = size(<Self as crate::contract::ScratchCharge>::CHARGE);
        match &self.value {
            Value::Null | Value::Bool(_) => {}
            Value::Number(number) => total = total.saturating_add(size(number.as_str().len())),
            Value::String(text) => total = total.saturating_add(size(text.len())),
            Value::Array(items) => {
                total =
                    total
                        .saturating_add(size(items.len()).saturating_mul(size(
                            <Self as crate::contract::ScratchCharge>::CHARGE,
                        )));
                for item in items {
                    total = total.saturating_add(item.estimated_bytes());
                }
            }
            Value::Object(members) => {
                total = total.saturating_add(
                    size(members.len())
                        .saturating_mul(size(<Member as crate::contract::ScratchCharge>::CHARGE)),
                );
                for member in members {
                    total = total
                        .saturating_add(size(member.key.len()))
                        .saturating_add(member.value.estimated_bytes());
                }
            }
        }
        total
    }

    fn write_json(&self, output: &mut String) {
        match &self.value {
            Value::Null => output.push_str("null"),
            Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => output.push_str(value.as_str()),
            Value::String(value) => quote_into(value, output),
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    value.write_json(output);
                }
                output.push(']');
            }
            Value::Object(values) => {
                output.push('{');
                for (index, member) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    quote_into(&member.key, output);
                    output.push(':');
                    member.value.write_json(output);
                }
                output.push('}');
            }
        }
    }
}

/// Escape a Unicode string into a JSON string literal, including its quotes.
#[must_use]
pub fn quote(input: &str) -> String {
    let mut output = String::new();
    quote_into(input, &mut output);
    output
}

fn quote_into(input: &str, output: &mut String) {
    output.push('"');
    for character in input.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            '\0'..='\u{1f}' => {
                write!(output, "\\u{:04x}", u32::from(character))
                    .expect("writing to a String is infallible");
            }
            _ => output.push(character),
        }
    }
    output.push('"');
}

/// Escape a single JSON Pointer segment.
#[must_use]
pub fn pointer_segment(input: &str) -> String {
    input.replace('~', "~0").replace('/', "~1")
}
