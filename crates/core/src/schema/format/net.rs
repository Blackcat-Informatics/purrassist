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
    governor.charge(Resource::Work, 1)?;
    Ok(ipv4_literal(text))
}

fn is_hex_group(part: &str) -> bool {
    !part.is_empty() && part.len() <= 4 && part.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Groups from one side of an (optional) `::` compression, `None` if any
/// group is empty (a stray single colon, as opposed to the deliberate empty
/// side of `::`).
fn split_side(side: &str) -> Option<Vec<&str>> {
    if side.is_empty() {
        return Some(Vec::new());
    }
    let groups: Vec<&str> = side.split(':').collect();
    groups
        .iter()
        .all(|group| !group.is_empty())
        .then_some(groups)
}

/// Every group must be 1-4 hex digits, except a trailing embedded IPv4
/// literal (RFC 4291 section 2.2), which counts as two 16-bit groups.
fn groups_valid(groups: &[&str]) -> Option<usize> {
    match groups.split_last() {
        None => Some(0),
        Some((last, rest)) if last.contains('.') => (ipv4_literal(last)
            && rest.iter().all(|group| is_hex_group(group)))
        .then_some(rest.len() + 2),
        Some(_) => groups
            .iter()
            .all(|group| is_hex_group(group))
            .then_some(groups.len()),
    }
}

/// `::1`, `2001:db8::1`, or an address with a trailing embedded IPv4 (RFC
/// 4291 section 2.2). No zone id, prefix length, or brackets: those are
/// scoped-address or URI-authority syntax, not part of the address text.
pub(super) fn ipv6_literal(text: &str) -> bool {
    // The longest valid textual form is 45 octets
    // ("0000:0000:0000:0000:0000:0000:255.255.255.255"); this bound keeps
    // every subsequent split/collect trivially small.
    if text.len() > 45 || !text.is_ascii() {
        return false;
    }
    if let Some((left, right)) = text.split_once("::") {
        if right.contains("::") {
            return false;
        }
        let Some(left_groups) = split_side(left) else {
            return false;
        };
        let Some(right_groups) = split_side(right) else {
            return false;
        };
        let (Some(left_slots), Some(right_slots)) =
            (groups_valid(&left_groups), groups_valid(&right_groups))
        else {
            return false;
        };
        // `::` must stand for at least one omitted 16-bit group.
        left_slots + right_slots < 8
    } else {
        let Some(groups) = split_side(text) else {
            return false;
        };
        !groups.is_empty() && groups_valid(&groups) == Some(8)
    }
}

pub(super) fn is_ipv6(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    Ok(ipv6_literal(text))
}

#[cfg(test)]
mod tests {
    use super::{is_ipv4, is_ipv6};
    use crate::{Budget, Governor};
    fn governed<T>(call: impl FnOnce(&mut Governor) -> Result<T, crate::StopReason>) -> T {
        call(&mut Governor::new(Budget::default())).expect("within budget")
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
