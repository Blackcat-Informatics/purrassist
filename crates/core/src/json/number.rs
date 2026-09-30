// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact decimal JSON numbers, including exponents outside machine ranges.
use core::{cmp::Ordering, fmt, str::FromStr};

/// An RFC 8259 number retaining its exact lexical spelling.
#[derive(Clone, Debug)]
pub struct Number(String);

impl Number {
    /// Construct an exact unsigned 64-bit integer without floating conversion.
    #[must_use]
    pub fn from_unsigned(value: u64) -> Self {
        Self(value.to_string())
    }
    /// Construct the exact decimal spelling of a target-sized count.
    ///
    /// This accepts `usize`, whereas [`Self::from_unsigned`] accepts the full
    /// `u64` domain. Neither constructor narrows or saturates its input.
    ///
    /// ```
    /// use purrassist_core::json::Number;
    /// assert_eq!(Number::from_count(usize::MAX).as_str(), usize::MAX.to_string());
    /// assert_eq!(Number::from_unsigned(u64::MAX).as_str(), u64::MAX.to_string());
    /// ```
    #[must_use]
    pub fn from_count(value: usize) -> Self {
        Self(value.to_string())
    }

    /// Original decimal spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the mathematical value is an integer, without floating conversion.
    #[must_use]
    pub fn is_integer(&self) -> bool {
        let parts = self.parts();
        parts.digits == "0" || !parts.exponent.negative
    }

    /// Mathematical ordering, preserving precision for arbitrarily large exponents.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Ordering {
        let left = self.parts();
        let right = other.parts();
        if left.digits == "0" || right.digits == "0" {
            match (left.digits == "0", right.digits == "0") {
                (true, true) => Ordering::Equal,
                (true, false) => {
                    if right.negative {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                }
                (false, true) => {
                    if left.negative {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (false, false) => unreachable!(),
            }
        } else if left.negative != right.negative {
            if left.negative {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        } else {
            let magnitude = left
                .exponent
                .add(&Integer::from_usize(left.digits.len()))
                .cmp(&right.exponent.add(&Integer::from_usize(right.digits.len())))
                .then_with(|| {
                    let length = left.digits.len().max(right.digits.len());
                    left.digits
                        .bytes()
                        .chain(core::iter::repeat(b'0'))
                        .take(length)
                        .cmp(
                            right
                                .digits
                                .bytes()
                                .chain(core::iter::repeat(b'0'))
                                .take(length),
                        )
                });
            if left.negative {
                magnitude.reverse()
            } else {
                magnitude
            }
        }
    }

    /// Read a nonnegative machine-sized integer, when representable exactly.
    #[must_use]
    pub fn to_usize(&self) -> Option<usize> {
        let parts = self.parts();
        if parts.digits == "0" {
            return Some(0);
        }
        if parts.negative || parts.exponent.negative {
            return None;
        }
        let exponent = parts.exponent.digits.parse::<u32>().ok()?;
        parts
            .digits
            .parse::<usize>()
            .ok()?
            .checked_mul(10_usize.checked_pow(exponent)?)
    }

    /// Test exact mathematical divisibility without allocating exponent-sized integers.
    ///
    /// # Errors
    /// Returns resource exhaustion or a zero divisor.
    pub fn is_multiple_of(
        &self,
        divisor: &Self,
        governor: &mut crate::Governor,
    ) -> Result<bool, crate::StopReason> {
        use crate::contract::Resource;
        let numerator = self.parts();
        let denominator = divisor.parts();
        if denominator.digits == "0" {
            return Err(crate::StopReason::InvalidInput(
                "multipleOf divisor is zero".into(),
            ));
        }
        if numerator.digits == "0" {
            return Ok(true);
        }
        let delta = numerator.exponent.subtract(&denominator.exponent);
        if delta.negative {
            return Ok(false);
        }
        let (top, top_two, top_five) = remove_decimal_factors(numerator.digits, governor)?;
        let (bottom, bottom_two, bottom_five) =
            remove_decimal_factors(denominator.digits, governor)?;
        if delta < Integer::from_usize(bottom_two.saturating_sub(top_two))
            || delta < Integer::from_usize(bottom_five.saturating_sub(top_five))
        {
            return Ok(false);
        }
        if bottom == "1" {
            return Ok(true);
        }
        let mut remainder = String::from("0");
        for digit in top.bytes() {
            governor.charge(
                Resource::Work,
                u64::try_from(bottom.len())
                    .unwrap_or(u64::MAX)
                    .saturating_mul(10),
            )?;
            if remainder == "0" {
                remainder.clear();
            }
            remainder.push(char::from(digit));
            while magnitude_cmp(&remainder, &bottom).is_ge() {
                remainder = Integer::parse(&remainder)
                    .subtract(&Integer::parse(&bottom))
                    .digits;
            }
        }
        Ok(remainder == "0")
    }

    pub(super) fn parts(&self) -> Parts {
        let (mantissa, exponent) = self.0.split_once(['e', 'E']).unwrap_or((&self.0, "0"));
        let negative = mantissa.starts_with('-');
        let fraction = mantissa
            .split_once('.')
            .map_or(0, |(_, digits)| digits.len());
        let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
        let leading = digits.bytes().take_while(|byte| *byte == b'0').count();
        if leading == digits.len() {
            return Parts {
                negative: false,
                digits: "0".into(),
                exponent: Integer::parse("0"),
            };
        }
        digits.drain(..leading);
        let trailing = digits
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'0')
            .count();
        digits.truncate(digits.len() - trailing);
        let exponent = Integer::parse(exponent)
            .add(&Integer::from_usize(trailing))
            .subtract(&Integer::from_usize(fraction));
        Parts {
            negative,
            digits,
            exponent,
        }
    }
}

impl FromStr for Number {
    type Err = NumberError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = input.as_bytes();
        let mut position = usize::from(bytes.first() == Some(&b'-'));
        match bytes.get(position) {
            Some(b'0') => position += 1,
            Some(b'1'..=b'9') => {
                position += 1;
                while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                    position += 1;
                }
            }
            _ => return Err(NumberError),
        }
        if bytes.get(position) == Some(&b'.') {
            position += 1;
            let start = position;
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
            if position == start {
                return Err(NumberError);
            }
        }
        if matches!(bytes.get(position), Some(b'e' | b'E')) {
            position += 1;
            if matches!(bytes.get(position), Some(b'+' | b'-')) {
                position += 1;
            }
            let start = position;
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
            if position == start {
                return Err(NumberError);
            }
        }
        if position != bytes.len() {
            return Err(NumberError);
        }
        Ok(Self(input.into()))
    }
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.compare(other).is_eq()
    }
}
impl Eq for Number {}
impl fmt::Display for Number {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Invalid JSON numeric spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberError;
impl fmt::Display for NumberError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid JSON number")
    }
}
impl std::error::Error for NumberError {}

pub(super) struct Parts {
    pub negative: bool,
    pub digits: String,
    pub exponent: Integer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Integer {
    pub negative: bool,
    pub digits: String,
}
impl Integer {
    fn parse(input: &str) -> Self {
        let digits = input.trim_start_matches(['+', '-']).trim_start_matches('0');
        Self {
            negative: input.starts_with('-') && !digits.is_empty(),
            digits: if digits.is_empty() {
                "0".into()
            } else {
                digits.into()
            },
        }
    }
    fn from_usize(value: usize) -> Self {
        Self::parse(&value.to_string())
    }
    pub(super) fn add(&self, other: &Self) -> Self {
        if self.negative == other.negative {
            let mut out = Vec::new();
            let mut carry = 0_u8;
            let mut left = self.digits.bytes().rev();
            let mut right = other.digits.bytes().rev();
            loop {
                let a = left.next();
                let b = right.next();
                if a.is_none() && b.is_none() && carry == 0 {
                    break;
                }
                let sum = a.map_or(0, |v| v - b'0') + b.map_or(0, |v| v - b'0') + carry;
                out.push(b'0' + sum % 10);
                carry = sum / 10;
            }
            let digits: String = out.into_iter().rev().map(char::from).collect();
            Self {
                negative: self.negative && digits != "0",
                digits,
            }
        } else {
            let comparison = magnitude_cmp(&self.digits, &other.digits);
            let (large, small) = if comparison.is_lt() {
                (other, self)
            } else {
                (self, other)
            };
            let mut borrow = 0_i16;
            let mut right = small.digits.bytes().rev();
            let mut out = Vec::new();
            for digit in large.digits.bytes().rev() {
                let mut value = i16::from(digit - b'0')
                    - i16::from(right.next().map_or(0, |v| v - b'0'))
                    - borrow;
                borrow = i16::from(value < 0);
                if value < 0 {
                    value += 10;
                }
                out.push(char::from_digit(u32::try_from(value).unwrap_or(0), 10).unwrap_or('0'));
            }
            while out.len() > 1 && out.last() == Some(&'0') {
                out.pop();
            }
            let digits: String = out.into_iter().rev().collect();
            Self {
                negative: large.negative && digits != "0",
                digits,
            }
        }
    }
    pub(super) fn subtract(&self, other: &Self) -> Self {
        let mut negated = other.clone();
        if negated.digits != "0" {
            negated.negative = !negated.negative;
        }
        self.add(&negated)
    }
}
fn remove_decimal_factors(
    mut digits: String,
    governor: &mut crate::Governor,
) -> Result<(String, usize, usize), crate::StopReason> {
    let mut counts = [0, 0];
    for (index, divisor) in [2_u32, 5].into_iter().enumerate() {
        loop {
            governor.charge(
                crate::contract::Resource::Work,
                u64::try_from(digits.len()).unwrap_or(u64::MAX),
            )?;
            let mut quotient = String::with_capacity(digits.len());
            let mut remainder = 0;
            for digit in digits.bytes() {
                let value = remainder * 10 + u32::from(digit - b'0');
                let next = value / divisor;
                if next != 0 || !quotient.is_empty() {
                    quotient.push(char::from_digit(next, 10).expect("decimal digit"));
                }
                remainder = value % divisor;
            }
            if remainder != 0 {
                break;
            }
            digits = quotient;
            counts[index] += 1;
        }
    }
    Ok((digits, counts[0], counts[1]))
}

fn magnitude_cmp(left: &str, right: &str) -> Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}
impl Ord for Integer {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => magnitude_cmp(&self.digits, &other.digits),
            (true, true) => magnitude_cmp(&self.digits, &other.digits).reverse(),
        }
    }
}
impl PartialOrd for Integer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::Number;
    use core::cmp::Ordering;
    fn number(text: &str) -> Number {
        text.parse().expect("valid test number")
    }
    #[test]
    fn mathematical_equality_preserves_spelling() {
        assert_eq!(number("1.000"), number("10e-1"));
        assert_eq!(number("-0e99999999999999999999999999"), number("0"));
        assert_eq!(number("123000e-3"), number("123"));
        assert_eq!(number("1.000").as_str(), "1.000");
        assert_eq!(
            number("9007199254740993").compare(&number("9007199254740992")),
            Ordering::Greater
        );
    }
    #[test]
    fn exponent_does_not_require_machine_range() {
        assert_eq!(
            number("1e99999999999999999999999"),
            number("10e99999999999999999999998")
        );
        assert_eq!(
            number("1e-99999999999999999999999").compare(&number("0")),
            Ordering::Greater
        );
        assert!(!number("1e-99999999999999999999999").is_integer());
        assert!(number("100e-2").is_integer());
        assert_eq!(number("12e1").to_usize(), Some(120));
    }
    #[test]
    fn exact_multiple_of() {
        for (a, b, expected) in [
            ("0.3", "0.1", true),
            ("0.0075", "0.0001", true),
            ("1", "0.03", false),
            ("12", "0.3", true),
            ("1e999999999999999999999999", "2", true),
            ("1e999999999999999999999999", "3", false),
            ("15", "3", true),
            ("25", "50", false),
        ] {
            assert_eq!(
                number(a).is_multiple_of(
                    &number(b),
                    &mut crate::Governor::new(crate::Budget::default())
                ),
                Ok(expected),
                "{a}/{b}"
            );
        }
    }

    #[test]
    fn invalid_lexicals_are_rejected() {
        for bad in [
            "", "-", "+1", "01", ".1", "1.", "1e", "1e+", "NaN", "Infinity", " 1",
        ] {
            assert!(bad.parse::<Number>().is_err(), "{bad}");
        }
    }
}
