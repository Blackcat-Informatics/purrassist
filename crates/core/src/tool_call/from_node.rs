// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact conversion of validated JSON argument values into Rust types.
use crate::json::{IntegerReading, Node, Number, Value, pointer_segment};
use core::fmt;

/// A conversion failure at a precise location.
///
/// `pointer` is an RFC 6901 JSON Pointer relative to the converted value
/// (empty for the value itself); container conversions prefix the failing
/// element's segment.
///
/// ```
/// use purrassist_core::{json, tool_call::FromNode};
/// let error = Vec::<u8>::from_node(&json::parse("[1, 256]").unwrap()).unwrap_err();
/// assert_eq!(error.pointer, "/1");
/// assert_eq!(error.expected, "integer within the u8 range");
/// assert_eq!(error.found, "number 256");
/// assert_eq!(error.to_string(), "at /1: expected integer within the u8 range, found number 256");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FromNodeError {
    /// RFC 6901 pointer to the value that failed to convert.
    pub pointer: String,
    /// What the target type requires.
    pub expected: &'static str,
    /// What the value actually was.
    pub found: String,
}

impl FromNodeError {
    fn mismatch(expected: &'static str, node: &Node) -> Self {
        Self {
            pointer: String::new(),
            expected,
            found: describe(node),
        }
    }

    /// Prefix the pointer with one enclosing segment (escaped per RFC 6901).
    #[must_use]
    fn within(mut self, segment: &str) -> Self {
        self.pointer = format!("/{}{}", pointer_segment(segment), self.pointer);
        self
    }
}

impl fmt::Display for FromNodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pointer = if self.pointer.is_empty() {
            "the root"
        } else {
            &self.pointer
        };
        write!(
            formatter,
            "at {pointer}: expected {}, found {}",
            self.expected, self.found
        )
    }
}

impl std::error::Error for FromNodeError {}

fn describe(node: &Node) -> String {
    match &node.value {
        Value::Null => "null".into(),
        Value::Bool(value) => format!("boolean {value}"),
        Value::Number(number) => format!("number {}", number.as_str()),
        Value::String(_) => "string".into(),
        Value::Array(items) => format!("array of {} items", items.len()),
        Value::Object(members) => format!("object with {} members", members.len()),
    }
}

/// Exact conversion from a JSON value.
///
/// Conversions never coerce between JSON types (a string `"5"` is not an
/// integer) and never round integers. Integer targets accept any spelling
/// whose mathematical value is an integer within the target's range, so
/// `1e2`, `100.0`, and `1.5e1` convert to `100`, `100`, and `15`, while
/// `1.5` or `1e-1` fail with expected `"integer"`-style messages. Floating
/// targets round the decimal spelling to the nearest `f64` and reject values
/// that overflow to infinity. `Option<T>` maps `null` to `None`; `Vec<T>`
/// converts every item, reporting the first failure with its index.
///
/// ```
/// use purrassist_core::{json, tool_call::FromNode};
/// assert_eq!(i8::from_node(&json::parse("-128").unwrap()), Ok(-128));
/// assert!(i8::from_node(&json::parse("128").unwrap()).is_err());
/// assert_eq!(u64::from_node(&json::parse("1e2").unwrap()), Ok(100));
/// assert!(u64::from_node(&json::parse("1.5").unwrap()).is_err());
/// assert_eq!(Option::<bool>::from_node(&json::parse("null").unwrap()), Ok(None));
/// ```
pub trait FromNode: Sized {
    /// Convert `node` exactly.
    ///
    /// # Errors
    /// Returns a [`FromNodeError`] locating the first value whose JSON type,
    /// integrality, or range does not fit `Self`.
    fn from_node(node: &Node) -> Result<Self, FromNodeError>;
}

impl FromNode for String {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        node.as_str()
            .map(str::to_owned)
            .ok_or_else(|| FromNodeError::mismatch("string", node))
    }
}

impl FromNode for bool {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        match node.value {
            Value::Bool(value) => Ok(value),
            _ => Err(FromNodeError::mismatch("boolean", node)),
        }
    }
}

impl FromNode for Number {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        match &node.value {
            Value::Number(number) => Ok(number.clone()),
            _ => Err(FromNodeError::mismatch("number", node)),
        }
    }
}

impl FromNode for f64 {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        const EXPECTED: &str = "number within the finite f64 range";
        let Value::Number(number) = &node.value else {
            return Err(FromNodeError::mismatch(EXPECTED, node));
        };
        number
            .as_str()
            .parse::<Self>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| FromNodeError::mismatch(EXPECTED, node))
    }
}

impl FromNode for Node {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        Ok(node.clone())
    }
}

impl<T: FromNode> FromNode for Option<T> {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        match node.value {
            Value::Null => Ok(None),
            _ => T::from_node(node).map(Some),
        }
    }
}

impl<T: FromNode> FromNode for Vec<T> {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        let Value::Array(items) = &node.value else {
            return Err(FromNodeError::mismatch("array", node));
        };
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                T::from_node(item).map_err(|error| error.within(&index.to_string()))
            })
            .collect()
    }
}

/// The exact integer value of `node` as a sign and magnitude, failing with
/// `expected` when it is not an integral number.
fn integer_reading(node: &Node, expected: &'static str) -> Result<(bool, u128), FromNodeError> {
    match &node.value {
        Value::Number(number) => match number.integer_magnitude() {
            IntegerReading::Exact {
                negative,
                magnitude,
            } => Ok((negative, magnitude)),
            IntegerReading::Fractional | IntegerReading::OutOfRange => {
                Err(FromNodeError::mismatch(expected, node))
            }
        },
        _ => Err(FromNodeError::mismatch(expected, node)),
    }
}

macro_rules! integer_from_node {
    ($($target:ty),* $(,)?) => {$(
        impl FromNode for $target {
            fn from_node(node: &Node) -> Result<Self, FromNodeError> {
                const EXPECTED: &str =
                    concat!("integer within the ", stringify!($target), " range");
                let (negative, magnitude) = integer_reading(node, EXPECTED)?;
                let exact = if negative {
                    0_i128.checked_sub_unsigned(magnitude).and_then(|value| Self::try_from(value).ok())
                } else {
                    Self::try_from(magnitude).ok()
                };
                exact.ok_or_else(|| FromNodeError::mismatch(EXPECTED, node))
            }
        }
    )*};
}

integer_from_node!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);
