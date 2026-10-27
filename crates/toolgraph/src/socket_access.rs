// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared native socket-access semantics. Callers own input/work admission.

const ACCESSES: [&str; 12] = [
    "create", "bind", "listen", "accept", "connect", "shutdown", "getattr", "setattr", "getopt",
    "setopt", "send", "receive",
];
const PEER_ACCESSES: u16 = (1 << 3) | (1 << 4) | (1 << 10) | (1 << 11);

#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Internal socket helpers need crate privacy; pub conflicts with compiler unreachable_pub."
)]
pub(super) struct InvalidAccess;

// The owning API admits its complete source scans/comparisons before this call.
#[expect(
    clippy::redundant_pub_crate,
    reason = "Internal socket helpers need crate privacy; pub conflicts with compiler unreachable_pub."
)]
pub(super) fn parse(source: &str) -> Result<u16, InvalidAccess> {
    let mut bits = 0;
    for segment in source.split(',') {
        if segment.trim_ascii().is_empty() {
            return Err(InvalidAccess);
        }
        for word in segment.split_ascii_whitespace() {
            let value = match word {
                "r" => 1 << 11,
                "w" => 1 << 10,
                "rw" => (1 << 10) | (1 << 11),
                _ => {
                    let index = ACCESSES
                        .iter()
                        .position(|candidate| *candidate == word)
                        .ok_or(InvalidAccess)?;
                    1 << index
                }
            };
            bits |= value;
        }
    }
    Ok(bits)
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "Internal socket helpers need crate privacy; pub conflicts with compiler unreachable_pub."
)]
pub(super) fn names(bits: u16) -> impl Iterator<Item = &'static str> {
    ACCESSES
        .into_iter()
        .enumerate()
        .filter_map(move |(index, access)| (bits & (1 << index) != 0).then_some(access))
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "Internal socket helpers need crate privacy; pub conflicts with compiler unreachable_pub."
)]
pub(super) fn incompatible_peer(bits: u16) -> Option<&'static str> {
    names(bits & !PEER_ACCESSES).next()
}
