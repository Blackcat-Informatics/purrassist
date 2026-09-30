// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Small formatting adapters for the census's own identities and TOML escapes.
use std::fmt;
#[cfg(test)]
use std::fmt::Write;
pub(crate) struct Lower<'a>(pub(crate) &'a [u8]);
impl fmt::Display for Lower<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(output, "{byte:02x}")?;
        }
        Ok(())
    }
}
#[cfg(test)]
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    write!(output, "{}", Lower(bytes)).expect("String formatting cannot fail");
    output
}
pub(crate) const fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
