// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact decimal JSON numbers, including exponents outside machine ranges.
mod decimal;
use super::operation::{Operation, ordinary};
use crate::contract::{
    Governor, Resource, ScratchLease, ScratchString, StopReason, format_output_governed,
    format_scratch_governed, saturating_size,
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

    /// Validate an already owned spelling and transfer its reservation unchanged.
    pub(super) fn adopt_owned(
        text: ScratchString,
        governor: &mut Governor,
    ) -> Result<(Self, ScratchLease), NumberParseError> {
        Self::validate_spelling(text.as_str(), governor)?;
        governor.check().map_err(NumberParseError::Resource)?;
        let (text, lease) = text.into_owned_parts();
        Ok((Self(text), lease))
    }

    fn validate_spelling(input: &str, governor: &mut Governor) -> Result<(), NumberParseError> {
        governor
            .charge(
                Resource::Work,
                saturating_size(input.len()).saturating_add(1),
            )
            .map_err(NumberParseError::Resource)?;
        validate(input).map_err(NumberParseError::Syntax)?;
        Ok(())
    }

    fn retained_spelling(
        input: &str,
        governor: &mut Governor,
    ) -> Result<ScratchString, NumberParseError> {
        Self::validate_spelling(input, governor)?;
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
        !matches!(self.integer_magnitude(), IntegerReading::Fractional)
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

    /// Compare this number with a count using only transient admitted storage.
    pub(crate) fn compare_count_governed(
        &self,
        count: usize,
        governor: &mut Governor,
    ) -> Result<Ordering, StopReason> {
        // Supported 32-bit and 64-bit counts fit within 20 decimal digits.
        // Use the same target-independent allowance as from_unsigned_governed
        // before the formatter produces the temporary spelling.
        governor.charge(Resource::Work, 20)?;
        let count = format_scratch_governed(format_args!("{count}"), governor)?;
        decimal::compare(
            self.as_str(),
            count.as_str(),
            &mut Operation(Some(governor)),
        )
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
    /// Normalization, arithmetic and temporary storage use the caller's governor.
    /// All temporary reservations release on success or refusal; only a published
    /// zero-divisor diagnostic retains its output reservation.
    ///
    /// # Errors
    /// Returns the original resource/cancellation cause, or `InvalidInput` for a
    /// zero divisor when the diagnostic can be published within the budget.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json::Number};
    /// let value: Number = "0.3".parse()?;
    /// let divisor: Number = "0.1".parse()?;
    /// assert!(value.is_multiple_of(&divisor, &mut Governor::new(Budget::default()))?);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn is_multiple_of(
        &self,
        divisor: &Self,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let mut operation = Operation(Some(governor));
        let numerator = decimal::parts(self.as_str(), &mut operation)?;
        let denominator = decimal::parts(divisor.as_str(), &mut operation)?;
        if denominator.digits.as_str() == "0" {
            let message =
                format_scratch_governed(format_args!("multipleOf divisor is zero"), governor)?;
            governor.check()?;
            return Err(StopReason::InvalidInput(message.into_string()));
        }
        let result = decimal::multiple_of(numerator, denominator, &mut operation)?;
        operation.check()?;
        Ok(result)
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
        use crate::{Budget, Governor, StopReason, contract::Resource};
        for (a, b, expected) in [
            ("0.3", "0.1", true),
            ("0.0075", "0.0001", true),
            ("1", "0.03", false),
            ("12", "0.3", true),
            ("1e999999999999999999999999", "2", true),
            ("1e999999999999999999999999", "3", false),
            ("15", "3", true),
            ("25", "50", false),
            ("0", "-7", true),
            ("-21", "-3", true),
            ("123456789123456790", "3", false),
            ("1e2", "8", false),
            ("1e3", "8", true),
            ("1e2", "125", false),
            ("1e3", "125", true),
            (
                "1e-999999999999999999999999",
                "2e-999999999999999999999999",
                false,
            ),
            (
                "6e-999999999999999999999999",
                "3e-999999999999999999999999",
                true,
            ),
            ("1e-999999999999999999999999", "1", false),
        ] {
            let (a, b) = (number(a), number(b));
            let mut governor = Governor::new(Budget::default());
            assert_eq!(a.is_multiple_of(&b, &mut governor), Ok(expected), "{a}/{b}");
            assert_eq!(governor.scratch_meter().live(), 0, "{a}/{b}");
            let usage = governor.usage();
            for (budget, reason) in [
                (
                    Budget {
                        work: usage.work - 1,
                        ..Budget::default()
                    },
                    StopReason::BudgetExceeded(Resource::Work),
                ),
                (
                    Budget {
                        scratch_bytes: usage.peak_scratch_bytes - 1,
                        ..Budget::default()
                    },
                    StopReason::BudgetExceeded(Resource::ScratchBytes),
                ),
            ] {
                let mut governor = Governor::new(budget);
                assert_eq!(a.is_multiple_of(&b, &mut governor), Err(reason), "{a}/{b}");
                assert_eq!(governor.scratch_meter().live(), 0, "{a}/{b}");
            }
            let mut cancelled = Governor::new(Budget::default());
            cancelled.cancel();
            assert_eq!(
                a.is_multiple_of(&b, &mut cancelled),
                Err(StopReason::Cancelled)
            );
            assert_eq!(cancelled.scratch_meter().live(), 0);
        }
    }

    #[test]
    fn zero_divisor_publishes_only_completed_diagnostic() {
        use crate::{Budget, Governor, StopReason, contract::Resource};
        let (value, divisor) = (number("0"), number("-0e99"));
        let mut governor = Governor::new(Budget::default());
        let error = value.is_multiple_of(&divisor, &mut governor);
        assert_eq!(
            error,
            Err(StopReason::InvalidInput(
                "multipleOf divisor is zero".into()
            ))
        );
        let retained = governor.scratch_meter().live();
        let Err(StopReason::InvalidInput(message)) = &error else {
            unreachable!()
        };
        assert_eq!(retained, u64::try_from(message.capacity()).unwrap());
        drop(error);
        assert_eq!(governor.scratch_meter().live(), retained);
        for (budget, resource) in [
            (
                Budget {
                    work: governor.usage().work - 1,
                    ..Budget::default()
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: governor.usage().peak_scratch_bytes - 1,
                    ..Budget::default()
                },
                Resource::ScratchBytes,
            ),
        ] {
            let mut refused = Governor::new(budget);
            assert_eq!(
                value.is_multiple_of(&divisor, &mut refused),
                Err(StopReason::BudgetExceeded(resource))
            );
            assert_eq!(refused.scratch_meter().live(), 0);
        }
        let mut cancelled = Governor::new(Budget::default());
        cancelled.cancel();
        assert_eq!(
            value.is_multiple_of(&divisor, &mut cancelled),
            Err(StopReason::Cancelled)
        );
        assert_eq!(cancelled.scratch_meter().live(), 0);
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
