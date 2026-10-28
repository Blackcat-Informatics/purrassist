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
            reversed.push(char::from(b'0' + sum % 10))?;
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
            reversed.push(char::from(
                b'0' + u8::try_from(value).expect("INVARIANT: decimal difference is 0..9"),
            ))?;
        }
    }
    operation.work(reversed.as_str().len())?;
    let significant = reversed.as_str().trim_end_matches('0');
    let mut digits = operation.text();
    if significant.is_empty() {
        digits.push('0')?;
    } else {
        for digit in significant.bytes().rev() {
            operation.work(1)?;
            digits.push(char::from(digit))?;
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
        digits.push(char::from(byte))?;
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
