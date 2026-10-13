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
    /// The value `node` is not what the target type requires.
    ///
    /// The pointer is empty (the value itself); `expected` is what the type
    /// requires and `found` describes `node` (`"null"`, `"boolean true"`,
    /// `"number 256"`, `"string"`, `"array of 2 items"`,
    /// `"object with 1 members"`).
    ///
    /// ```
    /// use purrassist_core::{json, tool_call::FromNodeError};
    /// let node = json::parse("[1, 2]").unwrap();
    /// let error = FromNodeError::mismatch("string", &node);
    /// assert_eq!(error.pointer, "");
    /// assert_eq!(error.found, "array of 2 items");
    /// ```
    #[must_use]
    pub fn mismatch(expected: &'static str, node: &Node) -> Self {
        Self {
            pointer: String::new(),
            expected,
            found: describe(node),
        }
    }

    /// A required member `name` is absent from the object being converted.
    ///
    /// The pointer addresses where the member was required (`/name`).
    ///
    /// ```
    /// use purrassist_core::tool_call::FromNodeError;
    /// let error = FromNodeError::missing("path");
    /// assert_eq!(error.to_string(), "at /path: expected required member, found missing");
    /// ```
    #[must_use]
    pub fn missing(name: &str) -> Self {
        Self::member(name, "required member", "missing")
    }

    /// The member `name` occurs more than once in the object being converted.
    ///
    /// Exact conversion never picks between occurrences; the pointer is
    /// `/name`.
    ///
    /// ```
    /// use purrassist_core::tool_call::FromNodeError;
    /// let error = FromNodeError::duplicate("path");
    /// assert_eq!(error.to_string(), "at /path: expected a single occurrence, found duplicate member");
    /// ```
    #[must_use]
    pub fn duplicate(name: &str) -> Self {
        Self::member(name, "a single occurrence", "duplicate member")
    }

    /// The object has a member `name` that the target type does not declare.
    ///
    /// ```
    /// use purrassist_core::tool_call::FromNodeError;
    /// let error = FromNodeError::unknown("colour");
    /// assert_eq!(error.to_string(), "at /colour: expected a declared member, found undeclared member");
    /// ```
    #[must_use]
    pub fn unknown(name: &str) -> Self {
        Self::member(name, "a declared member", "undeclared member")
    }

    /// Prefix the pointer with one enclosing segment (escaped per RFC 6901).
    ///
    /// ```
    /// use purrassist_core::tool_call::FromNodeError;
    /// let error = FromNodeError::missing("b").within("a/b").within("0");
    /// assert_eq!(error.pointer, "/0/a~1b/b");
    /// ```
    #[must_use]
    pub fn within(mut self, segment: &str) -> Self {
        self.pointer = format!("/{}{}", pointer_segment(segment), self.pointer);
        self
    }

    fn member(name: &str, expected: &'static str, found: &str) -> Self {
        Self {
            pointer: format!("/{}", pointer_segment(name)),
            expected,
            found: found.into(),
        }
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
/// targets (`f32`, `f64`) round the decimal spelling to the nearest value of
/// their own width and reject values that overflow to infinity. `Box<T>`
/// converts as `T`. `Option<T>` maps `null` to `None`; `Vec<T>`
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
///
/// A floating target parses the spelling directly (an `f32` is never read
/// through `f64`, which would round twice), so reading is round-to-nearest
/// while writing a float through [`Number`] is exact. Overflow is rejected:
///
/// ```
/// use purrassist_core::{json, tool_call::FromNode};
/// assert_eq!(f32::from_node(&json::parse("0.1").unwrap()), Ok(0.1_f32));
/// assert_eq!(f64::from_node(&json::parse("1e39").unwrap()), Ok(1e39_f64));
/// let error = f32::from_node(&json::parse("1e39").unwrap()).unwrap_err();
/// assert_eq!(error.expected, "number within the finite f32 range");
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

// Floating targets parse the spelling directly with `str::parse` (never `f32`
// through `f64`, which would round twice) and reject non-finite results.
macro_rules! float_from_node {
    ($($target:ty),* $(,)?) => {$(
        impl FromNode for $target {
            fn from_node(node: &Node) -> Result<Self, FromNodeError> {
                const EXPECTED: &str =
                    concat!("number within the finite ", stringify!($target), " range");
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
    )*};
}

float_from_node!(f32, f64);

impl<T: FromNode> FromNode for Box<T> {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        T::from_node(node).map(Self::new)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn node(text: &str) -> Node {
        json::parse(text).unwrap()
    }

    // The `as f32` below is the double-rounding path this test contrasts with.
    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn f32_rounds_the_spelling_once_to_nearest() {
        assert_eq!(f32::from_node(&node("0.1")), Ok(0.1_f32));
        assert_eq!(f32::from_node(&node("-0")), Ok(-0.0_f32));
        assert_eq!(f32::from_node(&node("3.4028235e38")), Ok(f32::MAX));
        // Just above the midpoint between 1.0 and the next f32: reading
        // through f64 would land exactly on the midpoint and round to 1.0.
        let above_midpoint = "1.00000005960464477539062500000001";
        assert_eq!(
            f32::from_node(&node(above_midpoint)),
            Ok(f32::from_bits(1.0_f32.to_bits() + 1))
        );
        assert_eq!(
            (above_midpoint.parse::<f64>().unwrap() as f32).to_bits(),
            1.0_f32.to_bits(),
            "the double-rounding path would differ"
        );
        // Underflow rounds to zero rather than failing.
        assert_eq!(f32::from_node(&node("1e-100")), Ok(0.0_f32));
    }

    #[test]
    fn f32_rejects_non_finite_and_non_numbers() {
        for text in ["1e39", "-1e39", "1e999999999999", "\"1\"", "null", "true"] {
            let error = f32::from_node(&node(text)).unwrap_err();
            assert_eq!(
                error.expected, "number within the finite f32 range",
                "{text}"
            );
        }
    }

    #[test]
    fn boxes_convert_as_their_target() {
        assert_eq!(<Box<u8>>::from_node(&node("7")), Ok(Box::new(7)));
        assert_eq!(<Option<Box<String>>>::from_node(&node("null")), Ok(None));
        let error = <Vec<Box<u8>>>::from_node(&node("[1, 300]")).unwrap_err();
        assert_eq!(error.pointer, "/1");
        assert_eq!(error.expected, "integer within the u8 range");
    }
}
