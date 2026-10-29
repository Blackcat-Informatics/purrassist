// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Python named-character escapes from pinned Unicode character names/aliases.
use crate::contract::{Resource, saturating_size as size};
use crate::{Governor, StopReason};
use core::cmp::Ordering;

#[rustfmt::skip]
#[path = "python_names_data.rs"]
mod data;

pub(super) fn lookup(name: &str, governor: &mut Governor) -> Result<Option<char>, StopReason> {
    governor.check()?;
    for &(start, end, prefix) in data::RANGES {
        governor.charge(Resource::Work, size(prefix.len()))?;
        if name.len() > prefix.len()
            && name.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
        {
            let hex = &name[prefix.len()..];
            governor.charge(Resource::Work, size(hex.len()))?;
            if !(4..=6).contains(&hex.len())
                || hex.starts_with('0')
                || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Ok(None);
            }
            if let Ok(code) = u32::from_str_radix(hex, 16)
                && (start..=end).contains(&code)
            {
                return Ok(char::from_u32(code));
            }
        }
    }
    let (mut low, mut high) = (0, data::NAMES.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let (candidate, code) = data::NAMES[middle];
        governor.charge(Resource::Work, size(name.len().min(candidate.len()) + 1))?;
        match name
            .bytes()
            .map(|byte| byte.to_ascii_uppercase())
            .cmp(candidate.bytes())
        {
            Ordering::Less => high = middle,
            Ordering::Greater => low = middle + 1,
            Ordering::Equal => return Ok(char::from_u32(code)),
        }
    }
    Ok(None)
}
