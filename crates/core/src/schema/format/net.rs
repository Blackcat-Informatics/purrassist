// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 791 dotted-quad `ipv4` and RFC 4291 textual `ipv6`.
use crate::contract::{Governor, Resource, StopReason};

fn valid_octet(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 3
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && !(part.len() > 1 && part.starts_with('0'))
        && part.parse::<u16>().is_ok_and(|value| value <= 255)
}

/// `192.168.0.1`: exactly four decimal octets, no sign, no leading zeros
/// (RFC 791 dotted-quad; the ambiguity around octal/hex-prefixed octets
/// makes strict decimal-only parsing the only interoperable reading).
pub(super) fn ipv4_literal(text: &str) -> bool {
    // The longest valid dotted-quad is 15 octets ("255.255.255.255"); reject
    // longer input before any further scanning.
    text.len() <= 15
        && text.is_ascii()
        && text.matches('.').count() == 3
        && text.split('.').all(valid_octet)
}

pub(super) fn is_ipv4(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    // The length guard precedes every scan. For admitted lengths, ASCII,
    // dot counting, splitting, digit validation and integer parsing each visit
    // at most n bytes; the sixth pass covers per-group predicates.
    let work = if text.len() <= 15 {
        6 * text.len() as u64 + 1
    } else {
        1
    };
    governor.charge(Resource::Work, work)?;
    let valid = ipv4_literal(text);
    governor.check()?;
    Ok(valid)
}

fn is_hex_group(part: &str) -> bool {
    !part.is_empty() && part.len() <= 4 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Validate borrowed groups on one side of optional `::` compression.
/// Only its final group may be an embedded IPv4 literal, counting as two slots.
fn group_slots(side: &str) -> Option<usize> {
    if side.is_empty() {
        return Some(0);
    }
    let mut groups = side.split(':').peekable();
    let mut slots = 0;
    while let Some(group) = groups.next() {
        if group.is_empty() {
            return None;
        }
        if groups.peek().is_none() && group.contains('.') {
            if !ipv4_literal(group) {
                return None;
            }
            slots += 2;
        } else {
            if !is_hex_group(group) {
                return None;
            }
            slots += 1;
        }
    }
    Some(slots)
}

/// `::1`, `2001:db8::1`, or an address with a trailing embedded IPv4 (RFC
/// 4291 section 2.2). No zone id, prefix length, or brackets: those are
/// scoped-address or URI-authority syntax, not part of the address text.
pub(super) fn ipv6_literal(text: &str) -> bool {
    // The longest valid textual form is 45 octets
    // ("0000:0000:0000:0000:0000:0000:255.255.255.255"); this bound keeps
    // every subsequent borrowed scan trivially small.
    if text.len() > 45 || !text.is_ascii() {
        return false;
    }
    if let Some((left, right)) = text.split_once("::") {
        if right.contains("::") {
            return false;
        }
        let (Some(left_slots), Some(right_slots)) = (group_slots(left), group_slots(right)) else {
            return false;
        };
        // `::` must stand for at least one omitted 16-bit group.
        left_slots + right_slots < 8
    } else {
        !text.is_empty() && group_slots(text) == Some(8)
    }
}

pub(super) fn is_ipv6(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    // For n <= 45: ASCII costs n; two two-byte delimiter searches cost at
    // most 4n; group splitting and final-dot scans cost 2n; group validation
    // including embedded IPv4 costs at most 6n; group predicates cost 2n + 8.
    // Larger inputs stop at the constant-time length guard.
    let work = if text.len() <= 45 {
        15 * text.len() as u64 + 8
    } else {
        1
    };
    governor.charge(Resource::Work, work)?;
    let valid = ipv6_literal(text);
    governor.check()?;
    Ok(valid)
}

#[cfg(test)]
mod tests {
    use super::{is_ipv4, is_ipv6};
    use crate::{Budget, Governor};
    fn governed<T>(call: impl FnOnce(&mut Governor) -> Result<T, crate::StopReason>) -> T {
        let mut governor = Governor::new(Budget::default());
        let value = call(&mut governor).expect("within budget");
        assert_eq!(governor.scratch_meter().live(), 0);
        value
    }
    #[test]
    fn ipv4_rejects_leading_zeros_and_wrong_shape() {
        assert!(governed(|g| is_ipv4("192.168.0.1", g)));
        assert!(governed(|g| is_ipv4("0.0.0.0", g)));
        assert!(!governed(|g| is_ipv4("1.2.3.4.5", g)));
        assert!(!governed(|g| is_ipv4("01.2.3.4", g)));
        assert!(!governed(|g| is_ipv4("256.0.0.0", g)));
        assert!(!governed(|g| is_ipv4("192.168.0.1 ", g)));
    }
    #[test]
    fn ipv6_handles_compression_and_embedded_ipv4() {
        assert!(governed(|g| is_ipv6("::1", g)));
        assert!(governed(|g| is_ipv6(
            "2001:0db8:0000:0000:0000:0000:0000:0001",
            g
        )));
        assert!(governed(|g| is_ipv6("::ffff:192.168.0.1", g)));
        assert!(governed(|g| is_ipv6("1:2:3:4:5:6:7:8", g)));
        assert!(governed(|g| is_ipv6("::1:2:3:4:5:6", g)));
        assert!(!governed(|g| is_ipv6("1:2:3:4:5:6:7", g)));
        assert!(!governed(|g| is_ipv6("1::d6::42", g)));
        assert!(!governed(|g| is_ipv6("::ffff:192.168.0.01", g)));
        assert!(!governed(|g| is_ipv6("fe80::a%eth1", g)));
        assert!(!governed(|g| is_ipv6("[::1]", g)));
        assert!(!governed(|g| is_ipv6("1:2:3:4:5:6:7:8::", g)));
        assert!(!governed(|g| is_ipv6(":2:3:4::8", g)));
    }
}
