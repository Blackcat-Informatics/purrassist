// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Numeric bounds as digit grammars.
//!
//! A bound is read from its exact decimal spelling, so `1e2`, `100.0` and `100`
//! are one bound and nothing is rounded through a machine float.
//!
//! # Exactness
//!
//! For `integer` the grammar generates the plain decimal spelling of every
//! integer in the interval and of no other value. For `number` it generates the
//! plain decimal spelling (a sign, an integer part without a leading zero and
//! an optional fraction of at least one digit, trailing zeros allowed) of every
//! decimal in the interval and of no other value. The value set is the exact
//! set of finite decimals the bounds admit, with no rounding at either end: a
//! strict bound excludes its own value and a fraction is compared digit by
//! digit, so `0.1` against a bound of `0.10` and against `0.1000000001` is
//! decided exactly. A number with an exponent is not generated when a bound is
//! present: the relation between an exponent's value and a mantissa's length is
//! not context-free, and every finite decimal already has a plain spelling, so
//! the set of values is unchanged and only the spelling is canonical.

use purrassist_core::{Governor, contract::Resource, json::Number};

/// An exact decimal: a sign, the integer digits without leading zeros and the
/// fraction digits without trailing zeros. Zero is never negative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Decimal {
    negative: bool,
    int: String,
    frac: String,
}

/// A bound: a decimal and whether the bound's own value is excluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Bound {
    value: Decimal,
    exclusive: bool,
}

/// Why a bound could not be read.
#[derive(Debug)]
pub(super) enum BoundError {
    /// The spelling has an exponent that does not fit the digit grammar.
    Exponent,
    /// The governor refused the digits.
    Resource(purrassist_core::StopReason),
}

impl Decimal {
    /// Read the exact value of `number`, charging one work unit per digit.
    pub(super) fn read(number: &Number, governor: &mut Governor) -> Result<Self, BoundError> {
        let text = number.as_str();
        let (negative, text) = text
            .strip_prefix('-')
            .map_or((false, text), |rest| (true, rest));
        let (mantissa, exponent) = match text.split_once(['e', 'E']) {
            Some((mantissa, exponent)) => (
                mantissa,
                exponent.parse::<i64>().map_err(|_| BoundError::Exponent)?,
            ),
            None => (text, 0),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = format!("{whole}{fraction}");
        let point = i64::try_from(whole.len())
            .map_err(|_| BoundError::Exponent)?
            .checked_add(exponent)
            .ok_or(BoundError::Exponent)?;
        // The zeros an exponent spells out are admitted before they are built.
        governor
            .charge(
                Resource::Work,
                point
                    .unsigned_abs()
                    .saturating_add(u64::try_from(digits.len()).unwrap_or(u64::MAX)),
            )
            .map_err(BoundError::Resource)?;
        let (int, frac) = match usize::try_from(point) {
            Ok(point) if point >= digits.len() => (
                format!("{digits}{}", "0".repeat(point - digits.len())),
                String::new(),
            ),
            Ok(point) => (digits[..point].to_owned(), digits[point..].to_owned()),
            Err(_) => {
                let lead = usize::try_from(-point).map_err(|_| BoundError::Exponent)?;
                (String::new(), format!("{}{digits}", "0".repeat(lead)))
            }
        };
        let int = int.trim_start_matches('0');
        let frac = frac.trim_end_matches('0');
        let zero = int.is_empty() && frac.is_empty();
        Ok(Self {
            negative: negative && !zero,
            int: if int.is_empty() { "0" } else { int }.to_owned(),
            frac: frac.to_owned(),
        })
    }

    const fn is_zero(&self) -> bool {
        self.int.len() == 1 && self.int.as_bytes()[0] == b'0' && self.frac.is_empty()
    }
}

impl Bound {
    pub(super) const fn new(value: Decimal, exclusive: bool) -> Self {
        Self { value, exclusive }
    }
}

/// `"text"` as a GBNF literal of ASCII digits and signs.
fn lit(text: &str) -> String {
    if text.is_empty() {
        String::new()
    } else {
        format!("\"{text}\"")
    }
}

fn digit_class(low: u8, high: u8) -> String {
    if low == high {
        lit(&char::from(b'0' + low).to_string())
    } else {
        format!("[{}-{}]", char::from(b'0' + low), char::from(b'0' + high))
    }
}

/// `expressions` as one parenthesized alternation, or the single expression.
fn alt(mut expressions: Vec<String>) -> Option<String> {
    match expressions.len() {
        0 => None,
        1 => expressions.pop(),
        _ => Some(format!("({})", expressions.join(" | "))),
    }
}

fn digits_of(text: &str) -> impl Iterator<Item = u8> + '_ {
    text.bytes().map(|byte| byte - b'0')
}

/// `count` copies of any digit.
fn any_digits(count: usize) -> String {
    vec!["[0-9]"; count].join(" ")
}

fn sequence(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Add one to an integer written in digits without a leading zero.
fn successor(text: &str) -> String {
    let mut digits: Vec<u8> = text.bytes().collect();
    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            return String::from_utf8(digits).unwrap_or_default();
        }
    }
    format!("1{}", String::from_utf8(digits).unwrap_or_default())
}

/// Subtract one from a positive integer written in digits without a leading zero.
fn predecessor(text: &str) -> String {
    let mut digits: Vec<u8> = text.bytes().collect();
    for digit in digits.iter_mut().rev() {
        if *digit == b'0' {
            *digit = b'9';
        } else {
            *digit -= 1;
            break;
        }
    }
    let text = String::from_utf8(digits).unwrap_or_default();
    match text.trim_start_matches('0') {
        "" => "0".to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

/// Order of two integers written in digits without a leading zero.
fn compare(left: &str, right: &str) -> std::cmp::Ordering {
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

/// Strings of exactly `text.len()` digits that are at least `text`.
fn same_length_from(text: &str) -> String {
    let digits: Vec<u8> = digits_of(text).collect();
    let mut alternatives = Vec::new();
    for (index, &digit) in digits.iter().enumerate() {
        if digit < 9 {
            alternatives.push(sequence(&[
                lit(&text[..index]),
                digit_class(digit + 1, 9),
                any_digits(digits.len() - index - 1),
            ]));
        }
    }
    alternatives.push(lit(text));
    alt(alternatives).unwrap_or_default()
}

/// Strings of exactly `text.len()` digits that are at most `text`.
fn same_length_to(text: &str, first_nonzero: bool) -> String {
    let digits: Vec<u8> = digits_of(text).collect();
    let mut alternatives = Vec::new();
    for (index, &digit) in digits.iter().enumerate() {
        let floor = u8::from(index == 0 && first_nonzero);
        if digit > floor {
            alternatives.push(sequence(&[
                lit(&text[..index]),
                digit_class(floor, digit - 1),
                any_digits(digits.len() - index - 1),
            ]));
        }
    }
    alternatives.push(lit(text));
    alt(alternatives).unwrap_or_default()
}

/// Up to `count` further digits, each present only after the one before it.
fn optional_digits(count: usize) -> String {
    match count {
        0 => String::new(),
        1 => "([0-9])?".to_owned(),
        _ => format!("([0-9] {})?", optional_digits(count - 1)),
    }
}

/// Integers without a leading zero whose length is `from..=to`, `from >= 2`.
fn lengths(from: usize, to: usize) -> Option<String> {
    (from <= to).then(|| {
        sequence(&[
            "[1-9]".to_owned(),
            any_digits(from - 1),
            optional_digits(to - from),
        ])
    })
}

/// The decimal integers `low..=high` (`high` is unbounded when `None`),
/// written without a leading zero.
fn integers(low: &str, high: Option<&str>) -> Option<String> {
    let Some(high) = high else {
        let longer = sequence(&[
            "[1-9]".to_owned(),
            any_digits(low.len()),
            "[0-9]*".to_owned(),
        ]);
        return alt(vec![same_length_from(low), longer]);
    };
    match compare(low, high) {
        std::cmp::Ordering::Greater => None,
        std::cmp::Ordering::Equal => Some(lit(low)),
        std::cmp::Ordering::Less => {
            let mut alternatives = Vec::new();
            if low.len() == high.len() {
                alternatives.push(same_length_range(low, high));
            } else {
                alternatives.push(same_length_from(low));
                alternatives.extend(lengths(low.len() + 1, high.len() - 1));
                alternatives.push(same_length_to(high, high.len() > 1));
            }
            alt(alternatives)
        }
    }
}

/// Strings of `low.len()` digits between two equal-length bounds.
fn same_length_range(low: &str, high: &str) -> String {
    if low == high {
        return lit(low);
    }
    let (first, last) = (low.as_bytes()[0] - b'0', high.as_bytes()[0] - b'0');
    if first == last {
        return sequence(&[lit(&low[..1]), same_length_range(&low[1..], &high[1..])]);
    }
    let rest = low.len() - 1;
    let mut alternatives = vec![sequence(&[
        lit(&low[..1]),
        if rest == 0 {
            String::new()
        } else {
            same_length_from(&low[1..])
        },
    ])];
    if first + 1 < last {
        alternatives.push(sequence(&[
            digit_class(first + 1, last - 1),
            any_digits(rest),
        ]));
    }
    alternatives.push(sequence(&[
        lit(&high[..1]),
        if rest == 0 {
            String::new()
        } else {
            same_length_to(&high[1..], false)
        },
    ]));
    alt(alternatives).unwrap_or_default()
}

/// The set of strings that can follow a decimal point (and the empty string).
struct Fraction {
    /// Nonempty digit strings, or `None` when there are none.
    nonempty: Option<String>,
    /// Whether the empty string, the value zero, belongs.
    empty: bool,
}

impl Fraction {
    /// `digit` followed by every string of `rest`.
    fn after(digit: u8, rest: &Self) -> Option<String> {
        let head = lit(&char::from(b'0' + digit).to_string());
        match (&rest.nonempty, rest.empty) {
            (Some(tail), true) => Some(format!("{head} ({tail})?")),
            (Some(tail), false) => Some(format!("{head} {tail}")),
            (None, true) => Some(head),
            (None, false) => None,
        }
    }
}

/// A bound on the fraction digits, trailing zeros removed, and whether strict.
type FractionBound<'a> = Option<(&'a str, bool)>;

/// `digits` without its first digit.
fn drop_first(digits: &str) -> &str {
    digits.get(1..).unwrap_or("")
}

/// The fractions `0.R` at or above (or strictly above) `lower` and at or below
/// (or strictly below) `upper`, each as a padded digit string.
fn fractions(lower: FractionBound<'_>, upper: FractionBound<'_>) -> Fraction {
    // A nonstrict bound at zero constrains nothing.
    let lower = lower.filter(|&(digits, strict)| !digits.is_empty() || strict);
    if let (Some((low, low_strict)), Some((high, high_strict))) = (lower, upper)
        && low.is_empty()
        && high.is_empty()
    {
        // Only zero is both above and below zero.
        let only_zero = !low_strict && !high_strict;
        return Fraction {
            nonempty: only_zero.then(|| "\"0\"+".to_owned()),
            empty: only_zero,
        };
    }
    let zero_allowed = lower.is_none_or(|(digits, strict)| digits.is_empty() && !strict)
        && upper.is_none_or(|(digits, strict)| !digits.is_empty() || !strict);
    let lower_tail = lower.map(|(digits, strict)| (drop_first(digits), strict));
    let upper_tail = upper.map(|(digits, strict)| (drop_first(digits), strict));
    let first = |bound: FractionBound<'_>| bound.and_then(|(digits, _)| digits.bytes().next());
    let (low, high) = (
        first(lower).map(|b| b - b'0'),
        first(upper).map(|b| b - b'0'),
    );
    let mut alternatives = Vec::new();
    match (lower, upper) {
        (None, None) => {
            return Fraction {
                nonempty: Some("[0-9]+".to_owned()),
                empty: true,
            };
        }
        (Some(_), None) => {
            let digit = low.unwrap_or(0);
            if lower.is_some_and(|(digits, _)| digits.is_empty()) {
                // Only the strictness of the zero bound is left.
                let strict = lower.is_some_and(|(_, strict)| strict);
                return Fraction {
                    nonempty: Some(if strict {
                        "\"0\"* [1-9] [0-9]*".to_owned()
                    } else {
                        "[0-9]+".to_owned()
                    }),
                    empty: !strict,
                };
            }
            if digit < 9 {
                alternatives.push(sequence(&[digit_class(digit + 1, 9), "[0-9]*".to_owned()]));
            }
            alternatives.extend(Fraction::after(digit, &fractions(lower_tail, None)));
        }
        (None, Some(_)) => {
            let digit = high.unwrap_or(0);
            if upper.is_some_and(|(digits, _)| digits.is_empty()) {
                let strict = upper.is_some_and(|(_, strict)| strict);
                return Fraction {
                    nonempty: (!strict).then(|| "\"0\"+".to_owned()),
                    empty: !strict,
                };
            }
            if digit > 0 {
                alternatives.push(sequence(&[digit_class(0, digit - 1), "[0-9]*".to_owned()]));
            }
            alternatives.extend(Fraction::after(digit, &fractions(None, upper_tail)));
        }
        (Some(_), Some(_)) => {
            let (low, high) = (low.unwrap_or(0), high.unwrap_or(0));
            if low > high {
                return Fraction {
                    nonempty: None,
                    empty: false,
                };
            }
            if low == high {
                alternatives.extend(Fraction::after(low, &fractions(lower_tail, upper_tail)));
            } else {
                alternatives.extend(Fraction::after(low, &fractions(lower_tail, None)));
                if low + 1 < high {
                    alternatives.push(sequence(&[
                        digit_class(low + 1, high - 1),
                        "[0-9]*".to_owned(),
                    ]));
                }
                alternatives.extend(Fraction::after(high, &fractions(None, upper_tail)));
            }
        }
    }
    Fraction {
        nonempty: alt(alternatives),
        empty: zero_allowed,
    }
}

/// The spelling of the fraction part: absent, or a point and the digits.
fn fraction_part(lower: FractionBound<'_>, upper: FractionBound<'_>) -> Option<String> {
    let set = fractions(lower, upper);
    match (set.nonempty, set.empty) {
        (Some(digits), true) => Some(format!("(\".\" {digits})?")),
        (Some(digits), false) => Some(format!("\".\" {digits}")),
        (None, true) => Some(String::new()),
        (None, false) => None,
    }
}

/// A bound on a nonnegative magnitude: integer digits, fraction digits, strict.
type Magnitude<'a> = (&'a str, &'a str, bool);

/// Nonnegative decimals within the magnitudes `lower` (at least zero when
/// `None`) and `upper` (unbounded when `None`).
fn magnitudes(lower: Option<Magnitude<'_>>, upper: Option<Magnitude<'_>>) -> Option<String> {
    let (low_int, low_frac, low_strict) = lower.unwrap_or(("0", "", false));
    let lower_fraction = Some((low_frac, low_strict));
    let any_fraction = "(\".\" [0-9]+)?";
    let mut alternatives = Vec::new();
    let mut with_fraction = |integer: String, fraction: Option<String>| {
        if let Some(fraction) = fraction {
            alternatives.push(sequence(&[integer, fraction]));
        }
    };
    match upper {
        None => {
            with_fraction(lit(low_int), fraction_part(lower_fraction, None));
            with_fraction(
                integers(&successor(low_int), None).unwrap_or_default(),
                Some(any_fraction.to_owned()),
            );
        }
        Some((high_int, high_frac, high_strict)) => {
            let upper_fraction = Some((high_frac, high_strict));
            match compare(low_int, high_int) {
                std::cmp::Ordering::Greater => {}
                std::cmp::Ordering::Equal => {
                    with_fraction(lit(low_int), fraction_part(lower_fraction, upper_fraction));
                }
                std::cmp::Ordering::Less => {
                    with_fraction(lit(low_int), fraction_part(lower_fraction, None));
                    let next = successor(low_int);
                    if compare(&next, high_int).is_lt() {
                        with_fraction(
                            integers(&next, Some(&predecessor(high_int))).unwrap_or_default(),
                            Some(any_fraction.to_owned()),
                        );
                    }
                    with_fraction(lit(high_int), fraction_part(None, upper_fraction));
                }
            }
        }
    }
    alt(alternatives)
}

const fn magnitude(bound: &Bound) -> Magnitude<'_> {
    (
        bound.value.int.as_str(),
        bound.value.frac.as_str(),
        bound.exclusive,
    )
}

/// The grammar of every decimal within the bounds, or `None` when no decimal is.
pub(super) fn number(lower: Option<&Bound>, upper: Option<&Bound>) -> Option<String> {
    let mut alternatives = Vec::new();
    // Nonnegative values: a negative upper bound leaves none.
    if upper.is_none_or(|bound| !(bound.value.negative || bound.exclusive && bound.value.is_zero()))
    {
        let floor = lower.filter(|bound| !bound.value.negative).map(magnitude);
        alternatives.extend(magnitudes(floor, upper.map(magnitude)));
    }
    // Negative values: their magnitudes run from the upper bound's to the
    // lower bound's, and exclude zero, which the nonnegative arm spells.
    if lower.is_none_or(|bound| bound.value.negative) {
        let least = upper
            .filter(|bound| bound.value.negative)
            .map_or(("0", "", true), magnitude);
        let most = lower.map(magnitude);
        if let Some(negative) = magnitudes(Some(least), most) {
            alternatives.push(sequence(&[lit("-"), negative]));
        }
    }
    alt(alternatives)
}

/// An integer: a sign and the digits of its magnitude without a leading zero.
struct Whole {
    negative: bool,
    magnitude: String,
}

/// The least integer at or above a bound (`ceiling`), or the greatest at or
/// below it, after a strict bound moves one step inward.
fn whole(bound: &Bound, upper: bool) -> Whole {
    let value = &bound.value;
    let has_fraction = !value.frac.is_empty();
    // Truncate toward zero, then correct toward the requested side.
    let mut result = Whole {
        negative: value.negative,
        magnitude: value.int.clone(),
    };
    let away = has_fraction && (value.negative == upper);
    if away {
        result.magnitude = successor(&result.magnitude);
    }
    // `result` is now the floor of the value when `upper`, else its ceiling.
    if bound.exclusive && !has_fraction {
        result = step(&result, upper);
    }
    result.negative &= result.magnitude != "0";
    result
}

/// One integer toward negative infinity when `upper`, else toward positive.
fn step(value: &Whole, down: bool) -> Whole {
    let zero = value.magnitude == "0";
    match (value.negative && !zero, down) {
        (false, true) if zero => Whole {
            negative: true,
            magnitude: "1".to_owned(),
        },
        (false, true) => Whole {
            negative: false,
            magnitude: predecessor(&value.magnitude),
        },
        (false, false) => Whole {
            negative: false,
            magnitude: successor(&value.magnitude),
        },
        (true, true) => Whole {
            negative: true,
            magnitude: successor(&value.magnitude),
        },
        (true, false) => {
            let magnitude = predecessor(&value.magnitude);
            Whole {
                negative: magnitude != "0",
                magnitude,
            }
        }
    }
}

/// The grammar of every integer within the bounds, or `None` when none is.
pub(super) fn integer(lower: Option<&Bound>, upper: Option<&Bound>) -> Option<String> {
    let low = lower.map(|bound| whole(bound, false));
    let high = upper.map(|bound| whole(bound, true));
    let mut alternatives = Vec::new();
    // Zero and above.
    let floor = low
        .as_ref()
        .filter(|low| !low.negative)
        .map_or("0", |low| low.magnitude.as_str());
    match high.as_ref() {
        Some(high) if high.negative => {}
        Some(high) => alternatives.extend(integers(floor, Some(&high.magnitude))),
        None => alternatives.extend(integers(floor, None)),
    }
    // Below zero: magnitudes from one (or the upper bound's) to the lower bound's.
    if low.as_ref().is_none_or(|low| low.negative) {
        let least = high
            .as_ref()
            .filter(|high| high.negative)
            .map_or("1", |high| high.magnitude.as_str());
        let most = low.as_ref().map(|low| low.magnitude.as_str());
        if let Some(negative) = integers(least, most) {
            alternatives.push(sequence(&[lit("-"), negative]));
        }
    }
    alt(alternatives)
}
