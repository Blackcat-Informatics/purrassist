// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 3492 Punycode, for the idn-hostname format validator's
//! A-label decode/encode and round-trip check (RFC 5891 section 5).
const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 0x80;

const fn adapt(mut delta: u32, num_points: u32, first_time: bool) -> u32 {
    delta = if first_time { delta / DAMP } else { delta / 2 };
    delta += delta / num_points;
    let mut k = 0;
    while delta > (BASE - TMIN) * TMAX / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (BASE - TMIN + 1) * delta / (delta + SKEW)
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
        b'a' + u8::try_from(digit).expect("digit < 26")
    } else {
        b'0' + u8::try_from(digit - 26).expect("digit < 36")
    }
}

/// Decode one Punycode label body (the part after the `xn--` ACE prefix) to
/// its Unicode scalar sequence, or `None` on any malformed input (unknown
/// basic code point, digit overflow, or an out-of-range decoded scalar).
/// Never panics on adversarial input.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for an item in a `pub(crate) mod` nested inside a public one
// (https://github.com/rust-lang/rust-clippy/issues/6188); see
// `pattern/derive.rs`'s `Term` for the identical tension. `pub(crate)`
// matches `unreachable_pub`, so it wins.
#[allow(clippy::redundant_pub_crate)]
#[allow(
    clippy::many_single_char_names,
    reason = "n/i/bias/delta/w/k/t are RFC 3492's own pseudocode variable names"
)]
pub(crate) fn decode(input: &str) -> Option<Vec<char>> {
    if !input.is_ascii() {
        return None;
    }
    let bytes = input.as_bytes();
    let delimiter = bytes.iter().rposition(|&byte| byte == b'-');
    let (basic, mut extended) = delimiter.map_or((&bytes[..0], bytes), |position| {
        (&bytes[..position], &bytes[position + 1..])
    });
    let mut output: Vec<u32> = basic.iter().map(|&byte| u32::from(byte)).collect();
    let mut n = INITIAL_N;
    let mut i: u32 = 0;
    let mut bias = INITIAL_BIAS;
    while !extended.is_empty() {
        let old_i = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            let (&byte, rest) = extended.split_first()?;
            extended = rest;
            let digit = decode_digit(byte)?;
            i = i.checked_add(digit.checked_mul(w)?)?;
            let t = if k <= bias {
                TMIN
            } else if k >= bias + TMAX {
                TMAX
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
            w = w.checked_mul(BASE - t)?;
            k += BASE;
        }
        let point_count = u32::try_from(output.len()).ok()?.checked_add(1)?;
        bias = adapt(i.checked_sub(old_i)?, point_count, old_i == 0);
        n = n.checked_add(i / point_count)?;
        i %= point_count;
        let position = usize::try_from(i).ok()?;
        if position > output.len() {
            return None;
        }
        output.insert(position, n);
        i += 1;
    }
    output.into_iter().map(char::from_u32).collect()
}

/// Encode a Unicode scalar sequence into one Punycode label body (without
/// the `xn--` prefix). Returns `None` only on scratch-charged length
/// overflow (`u32`/`usize` arithmetic), never on well-formed input.
#[allow(
    clippy::redundant_pub_crate,
    reason = "see decode's identical tension note above"
)]
pub(crate) fn encode(input: &[char]) -> Option<String> {
    let mut output = String::new();
    let mut basic_count: u32 = 0;
    for &character in input {
        if character.is_ascii() {
            output.push(character);
            basic_count += 1;
        }
    }
    let mut handled = basic_count;
    let total = u32::try_from(input.len()).ok()?;
    if basic_count > 0 {
        output.push('-');
    }
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let mut first = true;
    while handled < total {
        let next_point = input
            .iter()
            .map(|&character| u32::from(character))
            .filter(|&code| code >= n)
            .min()?;
        delta = delta.checked_add((next_point.checked_sub(n)?).checked_mul(handled + 1)?)?;
        n = next_point;
        for &character in input {
            let code = u32::from(character);
            if code < n {
                delta = delta.checked_add(1)?;
            }
            if code == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias {
                        TMIN
                    } else if k >= bias + TMAX {
                        TMAX
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    let digit = t + (q - t) % (BASE - t);
                    output.push(char::from(encode_digit(digit)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(char::from(encode_digit(q)));
                bias = adapt(delta, handled + 1, first);
                first = false;
                delta = 0;
                handled += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n = n.checked_add(1)?;
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    #[test]
    fn decodes_published_examples() {
        assert_eq!(
            decode("9n2bp8q").map(|chars| chars.into_iter().collect::<String>()),
            Some("실례".into())
        );
        assert_eq!(
            decode("ihqwcrb4cv8a8dqg056pqjye").map(|chars| chars.into_iter().collect::<String>()),
            Some("他们为什么不说中文".into())
        );
        assert_eq!(decode("X"), None);
    }
    #[test]
    fn encode_decode_round_trips() {
        for text in ["실례", "he\u{300}llo", "xn", "a\u{1F600}b"] {
            let chars: Vec<char> = text.chars().collect();
            let encoded = encode(&chars).expect("encodable");
            let decoded = decode(&encoded).expect("decodable");
            assert_eq!(decoded, chars);
        }
    }
}
