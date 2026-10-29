// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact decimal JSON numbers, including exponents outside machine ranges.
mod decimal;
use super::operation::{Operation, ordinary};
use crate::contract::{
    Governor, Resource, ScratchLease, ScratchString, StopReason, format_output_governed,
    saturating_size,
};
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
    /// Construct a portable unsigned integer using the existing standard formatter.
    ///
    /// # Errors
    /// Returns resource/cancellation failure before formatting work or storage.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json::Number};
    /// assert_eq!(Number::from_unsigned_governed(u64::MAX, &mut Governor::new(Budget::default())).unwrap().as_str(), "18446744073709551615");
    /// ```
    pub fn from_unsigned_governed(value: u64, governor: &mut Governor) -> Result<Self, StopReason> {
        // The standard u64 formatter produces at most 20 decimal digits,
        // without allocating its own intermediate string.
        governor.charge(Resource::Work, 20)?;
        format_output_governed(format_args!("{value}"), governor).map(Self)
    }

    /// Validate and retain an exact numeric spelling with fallible storage.
    ///
    /// # Errors
    /// Distinguishes invalid numeric syntax from resource/cancellation failure.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json::Number};
    /// let value = Number::parse_governed("1.00e2", &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(value.as_str(), "1.00e2");
    /// ```
    pub fn parse_governed(input: &str, governor: &mut Governor) -> Result<Self, NumberParseError> {
        governor
            .charge(Resource::InputBytes, saturating_size(input.len()))
            .map_err(NumberParseError::Resource)?;
        Self::retain_governed(input, governor)
    }

    pub(super) fn retain_governed(
        input: &str,
        governor: &mut Governor,
    ) -> Result<Self, NumberParseError> {
        Self::retained_spelling(input, governor).map(|text| Self(text.into_string()))
    }

    pub(super) fn retain_owned(
        input: &str,
        governor: &mut Governor,
        lease: &mut ScratchLease,
    ) -> Result<Self, NumberParseError> {
        let (text, reservation) = Self::retained_spelling(input, governor)?.into_owned_parts();
        lease.absorb(reservation);
        Ok(Self(text))
    }

    fn retained_spelling(
        input: &str,
        governor: &mut Governor,
    ) -> Result<ScratchString, NumberParseError> {
        governor
            .charge(
                Resource::Work,
                saturating_size(input.len()).saturating_add(1),
            )
            .map_err(NumberParseError::Resource)?;
        validate(input).map_err(NumberParseError::Syntax)?;
        governor
            .charge(Resource::Work, saturating_size(input.len()))
            .map_err(NumberParseError::Resource)?;
        let mut retained = ScratchString::new(governor);
        retained
            .push_str(input)
            .map_err(NumberParseError::Resource)?;
        governor.check().map_err(NumberParseError::Resource)?;
        Ok(retained)
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
        ordinary(decimal::compare(
            self.as_str(),
            other.as_str(),
            &mut Operation(None),
        ))
    }

    /// Compare exact spellings through the caller's admitted decimal operations.
    pub(crate) fn compare_governed(
        &self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<core::cmp::Ordering, StopReason> {
        self.compare_admitted(other, &mut Operation(Some(governor)))
    }

    pub(super) fn compare_admitted(
        &self,
        other: &Self,
        operation: &mut Operation<'_>,
    ) -> Result<Ordering, StopReason> {
        decimal::compare(self.as_str(), other.as_str(), operation)
    }

    /// Read a nonnegative machine-sized integer, when representable exactly.
    #[must_use]
    pub fn to_usize(&self) -> Option<usize> {
        match self.integer_magnitude() {
            IntegerReading::Exact {
                negative: false,
                magnitude,
            } => usize::try_from(magnitude).ok(),
            _ => None,
        }
    }

    /// Read the exact integer value as a sign and a 128-bit magnitude.
    ///
    /// The reading uses the mathematical value of the spelling, so `1e2`,
    /// `100.0`, and `1.5e1` are integral while `1.5` and `1e-1` are not.
    /// Negative zero reads as nonnegative zero. A nonzero integral value whose
    /// magnitude exceeds `u128::MAX` is [`IntegerReading::OutOfRange`]; no
    /// floating conversion or exponent-sized allocation occurs. Infallible.
    ///
    /// ```
    /// use purrassist_core::json::{IntegerReading, Number};
    /// let read = |text: &str| text.parse::<Number>().unwrap().integer_magnitude();
    /// assert_eq!(read("-1.5e1"), IntegerReading::Exact { negative: true, magnitude: 15 });
    /// assert_eq!(read("-0"), IntegerReading::Exact { negative: false, magnitude: 0 });
    /// assert_eq!(read("1.5"), IntegerReading::Fractional);
    /// assert_eq!(read("1e39"), IntegerReading::OutOfRange);
    /// ```
    #[must_use]
    pub fn integer_magnitude(&self) -> IntegerReading {
        match read_integer(&self.0, |_| Ok::<(), core::convert::Infallible>(())) {
            Ok(reading) => reading,
            Err(never) => match never {},
        }
    }

    /// Read the existing exact integer contract with admitted allocation-free scans.
    ///
    /// # Errors
    /// Returns the original work/cancellation failure. No exponent-sized storage
    /// is allocated, even when a huge exponent cancels a long mantissa suffix.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json::{Number, IntegerReading}};
    /// let number: Number = "100e-2".parse().unwrap();
    /// assert_eq!(number.integer_magnitude_governed(&mut Governor::new(Budget::default())).unwrap(), IntegerReading::Exact { negative: false, magnitude: 1 });
    /// ```
    pub fn integer_magnitude_governed(
        &self,
        governor: &mut Governor,
    ) -> Result<IntegerReading, StopReason> {
        let reading = read_integer(&self.0, |bytes| {
            governor.charge(Resource::Work, saturating_size(bytes).saturating_add(1))
        })?;
        governor.check()?;
        Ok(reading)
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
        let parts = ordinary(decimal::parts(self.as_str(), &mut Operation(None)));
        Parts {
            digits: parts.digits.into_string(),
            exponent: Integer {
                negative: parts.exponent.negative,
                digits: parts.exponent.digits.into_string(),
            },
        }
    }
}

impl FromStr for Number {
    type Err = NumberError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        validate(input)?;
        Ok(Self(input.into()))
    }
}

fn validate(input: &str) -> Result<(), NumberError> {
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
    Ok(())
}

fn read_integer<E>(
    input: &str,
    mut scan: impl FnMut(usize) -> Result<(), E>,
) -> Result<IntegerReading, E> {
    scan(input.len())?;
    let (mantissa, exponent) = input.split_once(['e', 'E']).unwrap_or((input, "0"));
    scan(mantissa.len())?;
    let Some(first) = mantissa
        .bytes()
        .position(|byte| byte.is_ascii_digit() && byte != b'0')
    else {
        return Ok(IntegerReading::Exact {
            negative: false,
            magnitude: 0,
        });
    };
    scan(mantissa.len())?;
    let last = mantissa
        .bytes()
        .rposition(|byte| byte.is_ascii_digit() && byte != b'0')
        .expect("INVARIANT: first nonzero digit exists");
    scan(mantissa.len())?;
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, digits)| digits.len());
    scan(mantissa.len() - last - 1)?;
    let trailing = mantissa[last + 1..]
        .bytes()
        .filter(u8::is_ascii_digit)
        .count();
    scan(exponent.len())?;
    let scale = integer_scale(exponent, trailing, fraction);
    let scale = match scale {
        Ok(scale) if scale <= 38 => scale,
        Ok(_) => return Ok(IntegerReading::OutOfRange),
        Err(reading) => return Ok(reading),
    };
    scan(last - first + 1)?;
    let coefficient = mantissa[first..=last]
        .bytes()
        .filter(u8::is_ascii_digit)
        .try_fold(0_u128, |value, digit| {
            value.checked_mul(10)?.checked_add(u128::from(digit - b'0'))
        });
    let value = coefficient.and_then(|value| {
        value.checked_mul(
            10_u128.checked_pow(u32::try_from(scale).expect("INVARIANT: scale is at most 38"))?,
        )
    });
    Ok(value.map_or(IntegerReading::OutOfRange, |magnitude| {
        IntegerReading::Exact {
            negative: input.starts_with('-'),
            magnitude,
        }
    }))
}

fn integer_scale(exponent: &str, trailing: usize, fraction: usize) -> Result<u128, IntegerReading> {
    let negative = exponent.starts_with('-');
    // A cancellation offset is bounded by the allocated mantissa's length,
    // hence by isize::MAX. An exponent exceeding u128 cannot be canceled into
    // the representable range on any supported pointer width (at most 128).
    let magnitude = exponent
        .trim_start_matches(['+', '-'])
        .parse::<u128>()
        .map_err(|_| {
            if negative {
                IntegerReading::Fractional
            } else {
                IntegerReading::OutOfRange
            }
        })?;
    let trailing = trailing as u128;
    let fraction = fraction as u128;
    if negative {
        if trailing < fraction {
            return Err(IntegerReading::Fractional);
        }
        (trailing - fraction)
            .checked_sub(magnitude)
            .ok_or(IntegerReading::Fractional)
    } else if trailing >= fraction {
        magnitude
            .checked_add(trailing - fraction)
            .ok_or(IntegerReading::OutOfRange)
    } else {
        magnitude
            .checked_sub(fraction - trailing)
            .ok_or(IntegerReading::Fractional)
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

/// The exact integer reading of a [`Number`] (see [`Number::integer_magnitude`]).
///
/// ```
/// use purrassist_core::json::{IntegerReading, Number};
/// let number: Number = "1e2".parse().unwrap();
/// assert_eq!(number.integer_magnitude(), IntegerReading::Exact { negative: false, magnitude: 100 });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerReading {
    /// An integral value with its sign and exact magnitude.
    Exact {
        /// Whether the value is strictly negative.
        negative: bool,
        /// The absolute value.
        magnitude: u128,
    },
    /// The mathematical value has a nonzero fractional part.
    Fractional,
    /// An integral value whose magnitude exceeds `u128::MAX`.
    OutOfRange,
}

/// Numeric syntax and resource failures remain separate.
///
/// Numeric syntax holds [`NumberError`], while
/// [`crate::pattern::PatternCompileError`] holds source-addressed pattern errors.
/// Each nominal result type delegates display to its own inner error, preserving
/// the corresponding syntax and resource contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumberParseError {
    /// The input is not an RFC 8259 numeric spelling.
    Syntax(NumberError),
    /// Admission or cancellation refused processing.
    Resource(StopReason),
}
impl fmt::Display for NumberParseError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(output),
            Self::Resource(reason) => reason.fmt(output),
        }
    }
}
impl std::error::Error for NumberParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::Resource(reason) => Some(reason),
        }
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
        let value = ordinary(decimal::integer(input, &mut Operation(None)));
        Self {
            negative: value.negative,
            digits: value.digits.into_string(),
        }
    }
    fn from_usize(value: usize) -> Self {
        Self::parse(&value.to_string())
    }
    pub(super) fn add(&self, other: &Self) -> Self {
        let value = ordinary(decimal::add(
            self.negative,
            &self.digits,
            other.negative,
            &other.digits,
            &mut Operation(None),
        ));
        Self {
            negative: value.negative,
            digits: value.digits.into_string(),
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
    ordinary(decimal::magnitude(left, right, &mut Operation(None)))
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
