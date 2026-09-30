// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 4122 hyphenated textual UUID representation (any version/variant
//! nibble; the format keyword checks shape, not a specific UUID version).
use crate::contract::{Governor, Resource, StopReason};

const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

pub(super) fn is_uuid(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let bytes = text.as_bytes();
    let mut cursor = 0;
    for (index, &width) in GROUPS.iter().enumerate() {
        if index > 0 {
            if bytes.get(cursor) != Some(&b'-') {
                return Ok(false);
            }
            cursor += 1;
        }
        let Some(group) = bytes.get(cursor..cursor + width) else {
            return Ok(false);
        };
        if !group.iter().all(u8::is_ascii_hexdigit) {
            return Ok(false);
        }
        cursor += width;
    }
    Ok(cursor == bytes.len())
}

#[cfg(test)]
mod tests {
    use super::is_uuid;
    use crate::{Budget, Governor};
    fn check(text: &str) -> bool {
        is_uuid(text, &mut Governor::new(Budget::default())).expect("within budget")
    }
    #[test]
    fn accepts_any_version_and_case() {
        assert!(check("2EB8AA08-AA98-11EA-B4AA-73B441D16380"));
        assert!(check("00000000-0000-0000-0000-000000000000"));
        assert!(check("99c17cbb-656f-f64a-940f-1a4568f03487"));
    }
    #[test]
    fn rejects_wrong_shape_or_non_hex() {
        assert!(!check("2eb8aa08-aa98-11ea-b4aa-73b441d1638"));
        assert!(!check("2eb8aa08aa9811eab4aa73b441d16380"));
        assert!(!check("2eb8aa08-aa98-11ea-b4ga-73b441d16380"));
        assert!(!check("2eb8aa08-aa98-11ea-b4aa-73b441d16380\n"));
        assert!(!check("urn:uuid:2eb8aa08-aa98-11ea-b4aa-73b441d16380"));
    }
}
