// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Ordered, source-addressed JSON with exact decimal values.
mod document;
mod emission;
mod equivalence;
pub mod incremental;
mod number;
mod operation;
mod parser;
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchString, StopReason};
use core::ops::Range;
pub(crate) use document::OwnedDocument;
pub use incremental::recover_syntax;
pub(crate) use incremental::{Region, recover_region};
pub use number::{IntegerReading, Number, NumberError, NumberParseError};
pub(crate) use parser::{OwnedParseFailure, parse_owned};
pub use parser::{ParseError, ParseFailure, parse, parse_with};

/// One internal output buffer using the shared JSON escaping authority.
pub(crate) struct JsonOutput<'g> {
    governor: &'g mut Governor,
    output: ScratchString,
}

/// One borrowed member of a projected JSON object.
///
/// `None` emits JSON null; keys retain supplied order and duplicates. The
/// caller prepares this small view without cloning any child value.
#[derive(Clone, Copy, Debug)]
pub struct JsonField<'a> {
    /// Decoded member name, escaped by the canonical JSON writer.
    pub key: &'a str,
    /// Original value occurrence, or an explicit generated null.
    pub value: Option<&'a Node>,
}

impl ScratchCharge for JsonField<'_> {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<JsonField<'_>>() <= JsonField::CHARGE);

/// One borrowed value in an explicitly framed JSON Lines stream.
///
/// Values preserve original number spelling and nested duplicate members.
/// Objects select only the supplied borrowed fields; `Value(None)` emits null.
#[derive(Clone, Copy, Debug)]
pub enum JsonLine<'a> {
    /// An existing value occurrence, or an explicit generated null.
    Value(Option<&'a Node>),
    /// An object with fields in the supplied order.
    Object(&'a [JsonField<'a>]),
}

impl ScratchCharge for JsonLine<'_> {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<JsonLine<'_>>() <= JsonLine::CHARGE);

/// Serialize a complete borrowed JSON Lines stream with governed ownership.
///
/// Each value has exactly one trailing LF. An empty sequence emits no bytes;
/// there is no outer array. The original values are never changed or cloned.
/// The returned scratch owner releases its reservation on drop, including a
/// caller's later semantic refusal. Existing JSON cursors and escaping serve
/// this operation and ordinary [`Node::to_json_governed`] alike.
///
/// # Errors
/// Returns resource/cancellation refusal without successful partial output.
/// Each root has depth zero, and child values add one level.
///
/// ```
/// use purrassist_core::{Budget, Governor, json::{self, JsonField, JsonLine}};
/// let node = json::parse(r#"{"x":1.00e2}"#).unwrap();
/// let fields = [JsonField { key: "n", value: node.get("x") }];
/// let lines = [JsonLine::Object(&fields), JsonLine::Value(None)];
/// let output = json::lines_to_json_scratch(&lines, &mut Governor::new(Budget::default())).unwrap();
/// assert_eq!(output.as_str(), "{\"n\":1.00e2}\nnull\n");
/// ```
pub fn lines_to_json_scratch(
    lines: &[JsonLine<'_>],
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    emission::lines_to_json_scratch(lines, governor)
}

/// One source edit made by tolerant syntax recovery
/// ([`incremental::IncrementalRecovery`]).
///
/// `span` is a half-open byte range of the original input (absolute, even
/// when the input was supplied in several chunks); an empty span is an
/// insertion at that offset. `replacement` is the JSON text that stands in
/// for the span, and `guessed` marks an invented value (a missing value, an
/// unterminated string, a bare token read as a string, or an escape that had
/// to be kept literally) rather than a purely syntactic correction.
///
/// ```
/// use purrassist_core::{Budget, Governor, json::incremental::IncrementalRecovery};
/// let mut governor = Governor::new(Budget::default());
/// let mut parser = IncrementalRecovery::new(&governor);
/// parser.feed(b"{a: 1", &mut governor).unwrap();
/// let (_, _, edits) = parser.finish(&mut governor).unwrap();
/// assert_eq!(edits[0].rule, "json.syntax.bare-key");
/// assert_eq!(edits[0].span, 1..2);
/// assert_eq!(edits[0].replacement, "\"a\"");
/// assert!(!edits[0].guessed);
/// assert_eq!(edits[1].rule, "json.syntax.close-container");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxEdit {
    /// Stable rule identifier, such as `json.syntax.missing-comma`.
    pub rule: &'static str,
    /// Original byte range replaced (empty for an insertion).
    pub span: Range<usize>,
    /// Replacement JSON text.
    pub replacement: String,
    /// Whether the replacement invents content rather than correcting syntax.
    pub guessed: bool,
}

/// Make room for one more element by doubling, charging the added capacity
/// (at `T`'s portable [`ScratchCharge::CHARGE`]) before reserving it. Growth
/// depends only on the logical length, never on the target or on chunking.
fn reserve_one<T: ScratchCharge>(
    values: &mut Vec<T>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4);
        governor.charge(
            Resource::ScratchBytes,
            size((capacity - values.capacity()).saturating_mul(T::CHARGE)),
        )?;
        values
            .try_reserve_exact(capacity - values.len())
            .map_err(|_| StopReason::BudgetExceeded(Resource::ScratchBytes))?;
    }
    Ok(())
}

/// Append one scalar, charging doubled capacity before growing `output`.
fn push_char(
    output: &mut String,
    character: char,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    if output.len() + character.len_utf8() > output.capacity() {
        let capacity = (output.capacity().saturating_mul(2))
            .max(output.len() + character.len_utf8())
            .max(8);
        governor.charge(Resource::ScratchBytes, size(capacity - output.capacity()))?;
        output
            .try_reserve_exact(capacity - output.len())
            .map_err(|_| StopReason::BudgetExceeded(Resource::ScratchBytes))?;
    }
    output.push(character);
    Ok(())
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
        let mut current = self;
        for token in pointer_tokens(pointer)? {
            let token = token?;
            current = match &current.value {
                Value::Object(_) => current.get(&token)?,
                Value::Array(items) => items.get(array_index(&token)?)?,
                _ => return None,
            };
        }
        Some(current)
    }

    /// Resolve an RFC 6901 JSON Pointer for mutation, with the lookup rules
    /// of [`Node::pointer`] (ambiguous duplicate members fail).
    ///
    /// ```
    /// use purrassist_core::json::{self, Node, Value};
    /// let mut node = json::parse(r#"{"a":[1,{"b/c":2}],"d":1,"d":2}"#).unwrap();
    /// *node.pointer_mut("/a/1/b~1c").unwrap() = Node::new(Value::Bool(true));
    /// assert_eq!(node.to_json(), r#"{"a":[1,{"b/c":true}],"d":1,"d":2}"#);
    /// assert!(node.pointer_mut("/d").is_none());
    /// assert!(node.pointer_mut("/a/2").is_none());
    /// ```
    #[must_use]
    pub fn pointer_mut(&mut self, pointer: &str) -> Option<&mut Self> {
        let mut current = self;
        for token in pointer_tokens(pointer)? {
            let token = token?;
            current = match &mut current.value {
                Value::Object(members) => {
                    let mut matches = members.iter_mut().filter(|member| member.key == token);
                    let first = matches.next()?;
                    if matches.next().is_some() {
                        return None;
                    }
                    &mut first.value
                }
                Value::Array(items) => items.get_mut(array_index(&token)?)?,
                _ => return None,
            };
        }
        Some(current)
    }

    /// Semantic JSON equality, independent of source spans and object member order.
    /// Object duplicates compare as ordered occurrences instead of disappearing.
    #[must_use]
    pub fn equivalent(&self, other: &Self) -> bool {
        operation::ordinary(equivalence::compare(
            self,
            other,
            &mut operation::Operation(None),
        ))
    }

    /// Compare exact JSON values using the caller's resource and cancellation policy.
    ///
    /// Object key groups may be reordered, while duplicate values retain their
    /// occurrence order within each key. Numbers compare mathematically, including
    /// arbitrary decimal exponents and negative zero. Spans do not affect equality.
    /// Original DOM construction is caller-owned; neither original is changed.
    /// Root depth is zero. All comparison scans and temporary buffers are admitted
    /// and temporary reservations release on both equality and refusal.
    ///
    /// # Errors
    /// Returns native work/depth/storage or cancellation refusal, never inequality
    /// as a substitute for an unfinished comparison.
    ///
    /// ```
    /// use purrassist_core::{Budget,Governor,json};
    /// let left=json::parse(r#"{"a":1,"b":true,"a":2}"#).unwrap();
    /// let right=json::parse(r#"{"b":true,"a":1.0,"a":2e0}"#).unwrap();
    /// assert!(left.equivalent_governed(&right,&mut Governor::new(Budget::default())).unwrap());
    /// ```
    pub fn equivalent_governed(
        &self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let equal = equivalence::compare(self, other, &mut operation::Operation(Some(governor)))?;
        governor.check()?;
        Ok(equal)
    }

    /// Serialize to compact JSON, preserving member order and number spelling.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut output = String::new();
        self.write_json(&mut output);
        output
    }

    /// Serialize with governed work, depth and fallible retained output storage.
    ///
    /// Emission preserves the bytes of [`Self::to_json`], including duplicate
    /// members and number spelling. Iterative traversal retains only the active
    /// ancestor chain. Root depth is zero; member keys do not add a value level.
    /// Original tree construction is caller-owned. The returned string's backing
    /// capacity remains charged; no estimate is used to admit its allocation.
    ///
    /// # Errors
    /// Returns resource exhaustion or cancellation without partial successful
    /// output. The original tree is never modified.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json};
    /// let value = json::parse(r#"{"limit":1.00e2,"limit":2}"#).unwrap();
    /// let mut governor = Governor::new(Budget::default());
    /// assert_eq!(value.to_json_governed(&mut governor).unwrap(), r#"{"limit":1.00e2,"limit":2}"#);
    /// ```
    pub fn to_json_governed(&self, governor: &mut Governor) -> Result<String, StopReason> {
        emission::to_json_governed(self, governor)
    }

    pub(crate) fn to_json_scratch(
        &self,
        governor: &mut Governor,
    ) -> Result<crate::contract::ScratchString, StopReason> {
        emission::to_json_scratch(self, governor)
    }

    /// Estimate this value's heap footprint without serializing or cloning it.
    ///
    /// This traverses the tree without allocating and uses portable storage
    /// charges. It is not proof of serialized output capacity: JSON escaping
    /// can expand a string beyond its stored UTF-8 size. Historical callers
    /// also use this estimate in pre-admission; that usage is not certified by
    /// this method. Use [`Self::to_json_governed`] for fallible serialization
    /// that admits its actual retained output and cursor capacity.
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

    pub(crate) fn write_json(&self, output: &mut String) {
        emission::append(self, output);
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
    emission::quote(input, output);
}

/// Escape a Unicode string with explicit work, storage and cancellation admission.
///
/// Quotes and escapes are identical to [`quote`]. Input construction is caller-owned;
/// returned output capacity remains charged to the supplied governor.
///
/// # Errors
/// Returns the exact resource/cancellation reason without partial successful output.
///
/// ```
/// use purrassist_core::{Budget, Governor, json};
/// let mut governor = Governor::new(Budget::default());
/// assert_eq!(json::quote_governed("line\n", &mut governor).unwrap(), r#""line\n""#);
/// ```
pub fn quote_governed(input: &str, governor: &mut Governor) -> Result<String, StopReason> {
    emission::quote_governed(input, governor)
}

pub(crate) fn quote_scratch(
    input: &str,
    governor: &mut Governor,
) -> Result<crate::contract::ScratchString, StopReason> {
    emission::quote_scratch(input, governor)
}

/// The decoded reference tokens of an RFC 6901 pointer (`None` when it is
/// neither empty nor starts with `/`; a token is `None` when it holds an
/// invalid `~` escape).
fn pointer_tokens(pointer: &str) -> Option<impl Iterator<Item = Option<String>> + '_> {
    let rest = if pointer.is_empty() {
        None
    } else {
        Some(pointer.strip_prefix('/')?)
    };
    Some(
        rest.into_iter()
            .flat_map(|rest| rest.split('/'))
            .map(|part| {
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
                Some(key)
            }),
    )
}

/// An RFC 6901 array index token: decimal digits without a leading zero.
fn array_index(token: &str) -> Option<usize> {
    if token.is_empty()
        || !token.bytes().all(|byte| byte.is_ascii_digit())
        || (token.len() > 1 && token.starts_with('0'))
    {
        return None;
    }
    token.parse().ok()
}

/// Escape a single JSON Pointer segment.
#[must_use]
pub fn pointer_segment(input: &str) -> String {
    input.replace('~', "~0").replace('/', "~1")
}

pub(crate) fn equivalent_any(
    values: &[Node],
    node: &Node,
    governor: &mut crate::Governor,
) -> Result<bool, StopReason> {
    for value in values {
        if value.equivalent_governed(node, governor)? {
            return Ok(true);
        }
    }
    governor.check()?;
    Ok(false)
}
