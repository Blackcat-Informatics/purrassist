// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 3492 Punycode, for the idn-hostname format validator's
//! A-label decode/encode and round-trip check (RFC 5891 section 5).
use crate::contract::{Operation, Text};
use crate::{Governor, ScratchString, ScratchVec, StopReason};
const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 0x80;

fn adapt(
    mut delta: u32,
    num_points: u32,
    first_time: bool,
    operation: &mut Operation<'_>,
) -> Result<u32, StopReason> {
    operation.work(1)?;
    delta = if first_time { delta / DAMP } else { delta / 2 };
    delta += delta / num_points;
    let mut k = 0;
    while delta > (BASE - TMIN) * TMAX / 2 {
        operation.work(1)?;
        delta /= BASE - TMIN;
        k += BASE;
    }
    Ok(k + (BASE - TMIN + 1) * delta / (delta + SKEW))
}
fn decode_digit(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 26),
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a')),
        _ => None,
    }
}
fn encode_digit(digit: u32) -> u8 {
    if digit < 26 {
        b'a' + u8::try_from(digit).expect("INVARIANT: digit < 26")
    } else {
        b'0' + u8::try_from(digit - 26).expect("INVARIANT: digit < 36")
    }
}
const fn threshold(k: u32, bias: u32) -> u32 {
    if k <= bias {
        TMIN
    } else if k >= bias + TMAX {
        TMAX
    } else {
        k - bias
    }
}
fn read_delta(
    extended: &mut &[u8],
    mut delta: u32,
    bias: u32,
    operation: &mut Operation<'_>,
) -> Result<Option<u32>, StopReason> {
    let mut weight = 1_u32;
    let mut k = BASE;
    loop {
        operation.work(1)?;
        let Some((&byte, rest)) = extended.split_first() else {
            return Ok(None);
        };
        *extended = rest;
        let Some(digit) = decode_digit(byte) else {
            return Ok(None);
        };
        let Some(value) = digit
            .checked_mul(weight)
            .and_then(|step| delta.checked_add(step))
        else {
            return Ok(None);
        };
        delta = value;
        let bound = threshold(k, bias);
        if digit < bound {
            return Ok(Some(delta));
        }
        let Some(value) = weight.checked_mul(BASE - bound) else {
            return Ok(None);
        };
        weight = value;
        k += BASE;
    }
}
/// Decode an A-label body, retaining actual temporary storage.
/// Malformed code points and RFC arithmetic overflow remain `None`.
#[allow(
    clippy::redundant_pub_crate,
    reason = "crate-private caller in schema format"
)]
pub(crate) fn decode(
    input: &str,
    governor: &mut Governor,
) -> Result<Option<ScratchVec<char>>, StopReason> {
    let mut operation = Operation(Some(governor));
    let result = decode_with(input, &mut operation)?;
    operation.check()?;
    Ok(result)
}
fn decode_with(
    input: &str,
    operation: &mut Operation<'_>,
) -> Result<Option<ScratchVec<char>>, StopReason> {
    operation.work(input.len().saturating_mul(2))?;
    if !input.is_ascii() {
        return Ok(None);
    }
    let bytes = input.as_bytes();
    let delimiter = bytes.iter().rposition(|&byte| byte == b'-');
    let (basic, mut extended) = delimiter.map_or((&bytes[..0], bytes), |position| {
        (&bytes[..position], &bytes[position + 1..])
    });
    let mut output = operation.buffer();
    for &byte in basic {
        operation.push_buffer(&mut output, char::from(byte))?;
    }
    let mut point = INITIAL_N;
    let mut index = 0_u32;
    let mut bias = INITIAL_BIAS;
    while !extended.is_empty() {
        let old_index = index;
        let Some(value) = read_delta(&mut extended, index, bias, operation)? else {
            return Ok(None);
        };
        index = value;
        let Some(count) = u32::try_from(output.as_slice().len())
            .ok()
            .and_then(|v| v.checked_add(1))
        else {
            return Ok(None);
        };
        let Some(delta) = index.checked_sub(old_index) else {
            return Ok(None);
        };
        bias = adapt(delta, count, old_index == 0, operation)?;
        let Some(value) = point.checked_add(index / count) else {
            return Ok(None);
        };
        point = value;
        index %= count;
        let Some(character) = char::from_u32(point) else {
            return Ok(None);
        };
        let Ok(position) = usize::try_from(index) else {
            return Ok(None);
        };
        if position > output.as_slice().len() {
            return Ok(None);
        }
        operation.work(output.as_slice().len() - position)?;
        operation.push_buffer(&mut output, character)?;
        output.as_mut_slice()[position..].rotate_right(1);
        index += 1;
    }
    Ok(Some(output.into_admitted()))
}
fn write_delta(
    mut delta: u32,
    bias: u32,
    output: &mut Text,
    operation: &mut Operation<'_>,
) -> Result<(), StopReason> {
    let mut k = BASE;
    loop {
        operation.work(1)?;
        let bound = threshold(k, bias);
        if delta < bound {
            break;
        }
        let digit = bound + (delta - bound) % (BASE - bound);
        operation.push(output, char::from(encode_digit(digit)))?;
        delta = (delta - bound) / (BASE - bound);
        k += BASE;
    }
    operation.push(output, char::from(encode_digit(delta)))
}
/// Encode a U-label body with owned storage; RFC arithmetic overflow is `None`.
#[allow(
    clippy::redundant_pub_crate,
    reason = "crate-private caller in schema format"
)]
pub(crate) fn encode(
    input: &[char],
    governor: &mut Governor,
) -> Result<Option<ScratchString>, StopReason> {
    let mut operation = Operation(Some(governor));
    let result = encode_with(input, &mut operation)?;
    operation.check()?;
    Ok(result)
}
fn encode_with(
    input: &[char],
    operation: &mut Operation<'_>,
) -> Result<Option<ScratchString>, StopReason> {
    let mut output = operation.text();
    let Ok(total) = u32::try_from(input.len()) else {
        return Ok(None);
    };
    let mut basic_count = 0_u32;
    for &character in input {
        operation.work(1)?;
        if character.is_ascii() {
            operation.push(&mut output, character)?;
            basic_count += 1;
        }
    }
    let mut handled = basic_count;
    if basic_count > 0 {
        operation.push(&mut output, '-')?;
    }
    let mut point = INITIAL_N;
    let mut delta = 0_u32;
    let mut bias = INITIAL_BIAS;
    let mut first = true;
    while handled < total {
        operation.work(input.len())?;
        let Some(next_point) = input
            .iter()
            .map(|&c| u32::from(c))
            .filter(|&code| code >= point)
            .min()
        else {
            return Ok(None);
        };
        let Some(value) = next_point
            .checked_sub(point)
            .and_then(|v| v.checked_mul(handled + 1))
            .and_then(|v| delta.checked_add(v))
        else {
            return Ok(None);
        };
        delta = value;
        point = next_point;
        for &character in input {
            operation.work(1)?;
            let code = u32::from(character);
            if code < point {
                let Some(value) = delta.checked_add(1) else {
                    return Ok(None);
                };
                delta = value;
            }
            if code == point {
                write_delta(delta, bias, &mut output, operation)?;
                bias = adapt(delta, handled + 1, first, operation)?;
                first = false;
                delta = 0;
                handled += 1;
            }
        }
        let Some(value) = delta.checked_add(1) else {
            return Ok(None);
        };
        delta = value;
        let Some(value) = point.checked_add(1) else {
            return Ok(None);
        };
        point = value;
    }
    Ok(Some(output.into_admitted()))
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use crate::{Budget, Governor, StopReason, contract::Resource};
    #[test]
    fn decodes_published_examples() {
        for (source, expected) in [
            ("9n2bp8q", Some("실례")),
            ("ihqwcrb4cv8a8dqg056pqjye", Some("他们为什么不说中文")),
            ("X", None),
        ] {
            let mut governor = Governor::new(Budget::default());
            let decoded = decode(source, &mut governor).expect("within budget");
            assert_eq!(
                decoded
                    .as_ref()
                    .map(|chars| chars.iter().collect::<String>()),
                expected.map(str::to_owned)
            );
            drop(decoded);
            assert_eq!(governor.scratch_meter().live(), 0);
        }
    }
    #[test]
    fn encode_decode_round_trips() {
        for text in ["실례", "he\u{300}llo", "xn", "a\u{1F600}b"] {
            let chars: Vec<char> = text.chars().collect();
            let round_trip = |governor: &mut Governor| -> Result<(), StopReason> {
                let encoded = encode(&chars, governor)?.expect("encodable");
                let decoded = decode(&encoded, governor)?.expect("decodable");
                assert_eq!(decoded.as_slice(), chars);
                assert!(governor.scratch_meter().live() > 0);
                Ok(())
            };
            let mut governor = Governor::new(Budget::default());
            round_trip(&mut governor).expect("within budget");
            assert_eq!(governor.scratch_meter().live(), 0);
            let usage = governor.usage();
            for (budget, resource) in [
                (
                    Budget {
                        work: usage.work - 1,
                        ..Budget::default()
                    },
                    Resource::Work,
                ),
                (
                    Budget {
                        scratch_bytes: usage.peak_scratch_bytes - 1,
                        ..Budget::default()
                    },
                    Resource::ScratchBytes,
                ),
            ] {
                let mut refused = Governor::new(budget);
                assert_eq!(
                    round_trip(&mut refused),
                    Err(StopReason::BudgetExceeded(resource))
                );
                assert_eq!(refused.scratch_meter().live(), 0);
            }
            let mut cancelled = Governor::new(Budget::default());
            cancelled.cancel();
            assert_eq!(round_trip(&mut cancelled), Err(StopReason::Cancelled));
            assert_eq!(cancelled.scratch_meter().live(), 0);
        }
    }
}
