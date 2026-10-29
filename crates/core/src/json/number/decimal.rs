// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One exact decimal normalization and signed arithmetic authority.
use super::super::operation::{Operation, Text};
use crate::StopReason;
use core::cmp::Ordering;

pub(super) struct Integer {
    pub negative: bool,
    pub digits: Text,
}
pub(super) struct Parts {
    pub negative: bool,
    pub digits: Text,
    pub exponent: Integer,
}

pub(super) fn integer(input: &str, operation: &mut Operation<'_>) -> Result<Integer, StopReason> {
    operation.work(input.len())?;
    let digits = input.trim_start_matches(['+', '-']).trim_start_matches('0');
    Ok(Integer {
        negative: input.starts_with('-') && !digits.is_empty(),
        digits: operation.copy(if digits.is_empty() { "0" } else { digits })?,
    })
}

pub(super) fn magnitude(
    left: &str,
    right: &str,
    operation: &mut Operation<'_>,
) -> Result<Ordering, StopReason> {
    Ok(match left.len().cmp(&right.len()) {
        Ordering::Equal => operation.compare(left, right)?,
        order => order,
    })
}

pub(super) fn add(
    left_negative: bool,
    left: &str,
    right_negative: bool,
    right: &str,
    operation: &mut Operation<'_>,
) -> Result<Integer, StopReason> {
    let mut reversed = operation.text();
    let negative;
    if left_negative == right_negative {
        negative = left_negative;
        let mut left = left.bytes().rev();
        let mut right = right.bytes().rev();
        let mut carry = 0_u8;
        loop {
            operation.work(1)?;
            let a = left.next();
            let b = right.next();
            if a.is_none() && b.is_none() && carry == 0 {
                break;
            }
            let sum = a.map_or(0, |v| v - b'0') + b.map_or(0, |v| v - b'0') + carry;
            operation.push(&mut reversed, char::from(b'0' + sum % 10))?;
            carry = sum / 10;
        }
    } else {
        let order = magnitude(left, right, operation)?;
        let (large, small) = if order.is_lt() {
            negative = right_negative;
            (right, left)
        } else {
            negative = left_negative;
            (left, right)
        };
        let mut right = small.bytes().rev();
        let mut borrow = 0_i16;
        for digit in large.bytes().rev() {
            operation.work(1)?;
            let mut value =
                i16::from(digit - b'0') - i16::from(right.next().map_or(0, |v| v - b'0')) - borrow;
            borrow = i16::from(value < 0);
            if value < 0 {
                value += 10;
            }
            operation.push(
                &mut reversed,
                char::from(
                    b'0' + u8::try_from(value).expect("INVARIANT: decimal difference is 0..9"),
                ),
            )?;
        }
    }
    operation.work(reversed.as_str().len())?;
    let significant = reversed.as_str().trim_end_matches('0');
    let mut digits = operation.text();
    if significant.is_empty() {
        operation.push(&mut digits, '0')?;
    } else {
        for digit in significant.bytes().rev() {
            operation.work(1)?;
            operation.push(&mut digits, char::from(digit))?;
        }
    }
    Ok(Integer {
        negative: negative && digits.as_str() != "0",
        digits,
    })
}

pub(super) fn parts(input: &str, operation: &mut Operation<'_>) -> Result<Parts, StopReason> {
    operation.work(input.len().saturating_mul(4))?;
    let (mantissa, exponent) = input.split_once(['e', 'E']).unwrap_or((input, "0"));
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, digits)| digits.len());
    let first = mantissa
        .bytes()
        .position(|byte| byte.is_ascii_digit() && byte != b'0');
    let Some(first) = first else {
        return Ok(Parts {
            negative: false,
            digits: operation.copy("0")?,
            exponent: integer("0", operation)?,
        });
    };
    let last = mantissa
        .bytes()
        .rposition(|byte| byte.is_ascii_digit() && byte != b'0')
        .expect("INVARIANT: nonzero mantissa exists");
    let trailing = mantissa[last + 1..]
        .bytes()
        .filter(u8::is_ascii_digit)
        .count();
    let mut digits = operation.text();
    for byte in mantissa[first..=last].bytes().filter(u8::is_ascii_digit) {
        operation.work(1)?;
        operation.push(&mut digits, char::from(byte))?;
    }
    let exponent = integer(exponent, operation)?;
    let count = operation.count(trailing)?;
    let exponent = add(
        exponent.negative,
        exponent.digits.as_str(),
        false,
        count.as_str(),
        operation,
    )?;
    let count = operation.count(fraction)?;
    let exponent = add(
        exponent.negative,
        exponent.digits.as_str(),
        count.as_str() != "0",
        count.as_str(),
        operation,
    )?;
    Ok(Parts {
        negative: input.starts_with('-'),
        digits,
        exponent,
    })
}

/// Test divisibility after the public boundary has refused a zero denominator.
pub(super) fn multiple_of(
    numerator: Parts,
    denominator: Parts,
    operation: &mut Operation<'_>,
) -> Result<bool, StopReason> {
    if numerator.digits.as_str() == "0" {
        return Ok(true);
    }
    let delta = add(
        numerator.exponent.negative,
        numerator.exponent.digits.as_str(),
        !denominator.exponent.negative && denominator.exponent.digits.as_str() != "0",
        denominator.exponent.digits.as_str(),
        operation,
    )?;
    if delta.negative {
        return Ok(false);
    }
    let (top, top_two, top_five) = remove_decimal_factors(numerator.digits, operation)?;
    let (bottom, bottom_two, bottom_five) = remove_decimal_factors(denominator.digits, operation)?;
    for required in [
        bottom_two.saturating_sub(top_two),
        bottom_five.saturating_sub(top_five),
    ] {
        let required = operation.count(required)?;
        if magnitude(delta.digits.as_str(), required.as_str(), operation)?.is_lt() {
            return Ok(false);
        }
    }
    if bottom.as_str() == "1" {
        return Ok(true);
    }
    let mut remainder = operation.copy("0")?;
    for digit in top.as_str().bytes() {
        operation.work(1)?;
        if remainder.as_str() == "0" {
            remainder = operation.text();
        }
        operation.push(&mut remainder, char::from(digit))?;
        while magnitude(remainder.as_str(), bottom.as_str(), operation)?.is_ge() {
            remainder = add(false, remainder.as_str(), true, bottom.as_str(), operation)?.digits;
        }
    }
    Ok(remainder.as_str() == "0")
}

fn remove_decimal_factors(
    mut digits: Text,
    operation: &mut Operation<'_>,
) -> Result<(Text, usize, usize), StopReason> {
    let mut counts = [0, 0];
    for (index, divisor) in [2_u32, 5].into_iter().enumerate() {
        loop {
            operation.work(digits.as_str().len())?;
            let mut quotient = operation.text();
            let mut remainder = 0;
            for digit in digits.as_str().bytes() {
                let value = remainder * 10 + u32::from(digit - b'0');
                let next = value / divisor;
                if next != 0 || !quotient.as_str().is_empty() {
                    operation.push(
                        &mut quotient,
                        char::from_digit(next, 10).expect("INVARIANT: decimal digit"),
                    )?;
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

pub(super) fn compare(
    left: &str,
    right: &str,
    operation: &mut Operation<'_>,
) -> Result<Ordering, StopReason> {
    let left = parts(left, operation)?;
    let right = parts(right, operation)?;
    let order = if left.digits.as_str() == "0" || right.digits.as_str() == "0" {
        match (left.digits.as_str() == "0", right.digits.as_str() == "0") {
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
            (false, false) => unreachable!("INVARIANT: at least one zero"),
        }
    } else if left.negative != right.negative {
        if left.negative {
            Ordering::Less
        } else {
            Ordering::Greater
        }
    } else {
        let count = operation.count(left.digits.as_str().len())?;
        let a = add(
            left.exponent.negative,
            left.exponent.digits.as_str(),
            false,
            count.as_str(),
            operation,
        )?;
        let count = operation.count(right.digits.as_str().len())?;
        let b = add(
            right.exponent.negative,
            right.exponent.digits.as_str(),
            false,
            count.as_str(),
            operation,
        )?;
        let mut order = match (a.negative, b.negative) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => magnitude(a.digits.as_str(), b.digits.as_str(), operation)?,
            (true, true) => magnitude(a.digits.as_str(), b.digits.as_str(), operation)?.reverse(),
        };
        if order.is_eq() {
            operation.work(left.digits.as_str().len().max(right.digits.as_str().len()))?;
            let length = left.digits.as_str().len().max(right.digits.as_str().len());
            order = left
                .digits
                .as_str()
                .bytes()
                .chain(core::iter::repeat(b'0'))
                .take(length)
                .cmp(
                    right
                        .digits
                        .as_str()
                        .bytes()
                        .chain(core::iter::repeat(b'0'))
                        .take(length),
                );
        }
        if left.negative {
            order.reverse()
        } else {
            order
        }
    };
    operation.check()?;
    Ok(order)
}
