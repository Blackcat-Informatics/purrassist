// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 1123 `hostname` and IDNA 2008 (RFC 5890-5893) `idn-hostname`, sharing
//! one per-label pipeline: both accept `xn--` A-labels (decoded and
//! re-validated as their U-label), and `idn-hostname` additionally accepts
//! raw Unicode labels through UTS-46 (`IdnaMappingTable.txt`) nontransitional
//! mapping before the same RFC 5892/5893 contextual and Bidi checks apply.
use crate::contract::push_scratch;
use crate::contract::{Operation, Text};
use crate::schema::reference::work;
use crate::text::{
    bidi_class, canonical_combining_class, general_category_governed, idna_status, joining_type,
    punycode,
};
use crate::{Governor, ScratchString, ScratchVec, StopReason};
const ASCII_LABEL_SEPARATOR: char = '.';
const IDN_LABEL_SEPARATORS: [char; 4] = ['.', '\u{3002}', '\u{FF0E}', '\u{FF61}'];

fn hyphen_restriction_ok(chars: &[char]) -> bool {
    chars.first().is_none_or(|&c| c != '-')
        && chars.last().is_none_or(|&c| c != '-')
        && !(chars.len() >= 4 && chars[2] == '-' && chars[3] == '-')
}
fn contextj_zwnj_regexp_ok(
    chars: &[char],
    position: usize,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let mut before_ok = false;
    for &character in chars[..position].iter().rev() {
        let kind = joining_type(u32::from(character), governor)?;
        if kind != "T" {
            before_ok = matches!(kind, "L" | "D");
            break;
        }
    }
    let mut after_ok = false;
    for &character in &chars[position + 1..] {
        let kind = joining_type(u32::from(character), governor)?;
        if kind != "T" {
            after_ok = matches!(kind, "R" | "D");
            break;
        }
    }
    Ok(before_ok && after_ok)
}
fn contextj_ok(
    chars: &[char],
    position: usize,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let preceded_by_virama =
        position > 0 && canonical_combining_class(u32::from(chars[position - 1]), governor)? == 9;
    Ok(match chars[position] {
        '\u{200C}' => preceded_by_virama || contextj_zwnj_regexp_ok(chars, position, governor)?,
        '\u{200D}' => preceded_by_virama,
        _ => true,
    })
}
fn contexto_ok(
    chars: &[char],
    position: usize,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    work(governor, 1)?;
    Ok(match chars[position] {
        '\u{B7}' => {
            position > 0 && chars[position - 1] == 'l' && chars.get(position + 1) == Some(&'l')
        }
        '\u{375}' => match chars.get(position + 1) {
            Some(&c) => crate::pattern::script_of(u32::from(c), governor)? == "Greek",
            None => false,
        },
        '\u{5F3}' | '\u{5F4}' => {
            position > 0
                && crate::pattern::script_of(u32::from(chars[position - 1]), governor)? == "Hebrew"
        }
        '\u{30FB}' => {
            let mut found = false;
            for &character in chars {
                if matches!(
                    crate::pattern::script_of(u32::from(character), governor)?,
                    "Hiragana" | "Katakana" | "Han"
                ) {
                    found = true;
                    break;
                }
            }
            found
        }
        '\u{660}'..='\u{669}' => {
            work(governor, chars.len())?;
            !chars.iter().any(|&c| ('\u{6F0}'..='\u{6F9}').contains(&c))
        }
        '\u{6F0}'..='\u{6F9}' => {
            work(governor, chars.len())?;
            !chars.iter().any(|&c| ('\u{660}'..='\u{669}').contains(&c))
        }
        _ => true,
    })
}
fn bidi_trigger(labels: &[ScratchVec<char>], governor: &mut Governor) -> Result<bool, StopReason> {
    for label in labels {
        for &character in label {
            if matches!(
                bidi_class(u32::from(character), governor)?,
                "R" | "AL" | "AN"
            ) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
/// Apply the RFC 5893 rule without allocating an intermediate class vector.
fn bidi_rule_ok(chars: &[char], governor: &mut Governor) -> Result<bool, StopReason> {
    let Some(&first) = chars.first() else {
        return Ok(false);
    };
    let first = bidi_class(u32::from(first), governor)?;
    if !matches!(first, "L" | "R" | "AL") {
        return Ok(false);
    }
    let rtl = first != "L";
    let mut last = first;
    let mut european_digit = false;
    let mut arabic_digit = false;
    for &character in chars {
        let class = bidi_class(u32::from(character), governor)?;
        let valid = if rtl {
            matches!(
                class,
                "R" | "AL" | "AN" | "EN" | "ES" | "CS" | "ET" | "ON" | "BN" | "NSM"
            )
        } else {
            matches!(class, "L" | "EN" | "ES" | "CS" | "ET" | "ON" | "BN" | "NSM")
        };
        if !valid {
            return Ok(false);
        }
        if class != "NSM" {
            last = class;
        }
        european_digit |= class == "EN";
        arabic_digit |= class == "AN";
    }
    Ok(if rtl {
        !(european_digit && arabic_digit) && matches!(last, "R" | "AL" | "EN" | "AN")
    } else {
        matches!(last, "L" | "EN")
    })
}
fn map_and_normalize(
    label: &str,
    governor: &mut Governor,
) -> Result<Option<ScratchVec<char>>, StopReason> {
    let mut mapped = Text::Admitted(ScratchString::new(governor));
    for character in label.chars() {
        let (status, replacement) = idna_status(u32::from(character), governor)?;
        match status {
            "valid" | "deviation" => Operation(Some(governor)).push(&mut mapped, character)?,
            "mapped" | "disallowed_STD3_mapped" => {
                for &point in replacement {
                    work(governor, 1)?;
                    Operation(Some(governor)).push(
                        &mut mapped,
                        char::from_u32(point)
                            .expect("INVARIANT: IDNA mappings contain Unicode scalars"),
                    )?;
                }
            }
            "ignored" => {}
            _ => return Ok(None),
        }
    }
    let normalized =
        crate::text::normalize_owned(mapped.as_str(), crate::text::Normalization::Nfc, governor)?;
    let mut chars = ScratchVec::new(governor);
    for character in normalized.chars() {
        push_scratch(&mut chars, character, governor)?;
    }
    Ok(Some(chars))
}
fn decode_a_label(
    remainder: &[char],
    governor: &mut Governor,
) -> Result<Option<ScratchVec<char>>, StopReason> {
    let mut lowered = Text::Admitted(ScratchString::new(governor));
    for &character in remainder {
        work(governor, 1)?;
        Operation(Some(governor)).push(&mut lowered, character.to_ascii_lowercase())?;
    }
    let Some(decoded) = punycode::decode(lowered.as_str(), governor)? else {
        return Ok(None);
    };
    work(governor, decoded.len())?;
    if decoded.iter().all(char::is_ascii) {
        return Ok(None);
    }
    let Some(encoded) = punycode::encode(&decoded, governor)? else {
        return Ok(None);
    };
    work(governor, encoded.len().min(lowered.as_str().len()))?;
    Ok((encoded.as_str() == lowered.as_str()).then_some(decoded))
}
fn a_label_len(chars: &[char], governor: &mut Governor) -> Result<Option<usize>, StopReason> {
    work(governor, chars.len())?;
    if chars.iter().all(char::is_ascii) {
        Ok(Some(chars.len()))
    } else {
        Ok(punycode::encode(chars, governor)?.map(|text| 4 + text.len()))
    }
}

/// RFC 5892 section 2.6's small, fixed DISALLOWED-override table: code
/// points UTS-46's `IdnaMappingTable.txt` marks `valid` but whose IDNA 2008
/// derived property is DISALLOWED regardless (Arabic tatweel/NKO lajanyalan
/// as join-formatting characters, and the Hangul/kana tone/iteration marks).
const fn is_derived_disallowed(c: char) -> bool {
    matches!(
        c,
        '\u{640}'
            | '\u{7FA}'
            | '\u{302E}'
            | '\u{302F}'
            | '\u{3031}'
            | '\u{3032}'
            | '\u{3033}'
            | '\u{3034}'
            | '\u{3035}'
            | '\u{303B}'
    )
}

/// RFC 5892 section 2's default derived-property categories: letters,
/// combining marks, and decimal digits are PVALID unless a more specific
/// rule overrides them. Everything else (punctuation, symbols, separators,
/// control/format/private-use/surrogate/unassigned) defaults to
/// DISALLOWED, even when UTS-46's own `IdnaMappingTable.txt` calls it
/// `valid` for mapping purposes: the two specifications' code point sets
/// deliberately differ (RFC 5892 section 2 versus UTS #46 table 1), and the
/// idn-hostname format is IDNA 2008, not bare UTS-46.
fn is_default_pvalid_category(category: &str) -> bool {
    matches!(
        category,
        "Ll" | "Lu" | "Lo" | "Lm" | "Lt" | "Mn" | "Mc" | "Me" | "Nd"
    )
}

fn code_point_ok(c: char, governor: &mut Governor) -> Result<bool, StopReason> {
    work(governor, 1)?;
    if is_derived_disallowed(c) {
        return Ok(false);
    }
    Ok(matches!(
        c,
        '\u{DF}'
            | '\u{3C2}'
            | '\u{6FD}'
            | '\u{6FE}'
            | '\u{F0B}'
            | '\u{3007}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{B7}'
            | '\u{375}'
            | '\u{5F3}'
            | '\u{5F4}'
            | '\u{30FB}'
    ) || ('\u{660}'..='\u{669}').contains(&c)
        || ('\u{6F0}'..='\u{6F9}').contains(&c)
        || is_default_pvalid_category(general_category_governed(c, governor)?))
}
fn content_ok(chars: &[char], governor: &mut Governor) -> Result<bool, StopReason> {
    work(governor, 1)?;
    if chars.is_empty() || !hyphen_restriction_ok(chars) {
        return Ok(false);
    }
    if matches!(
        general_category_governed(chars[0], governor)?,
        "Mn" | "Mc" | "Me"
    ) {
        return Ok(false);
    }
    work(governor, chars.len())?;
    if chars.iter().all(char::is_ascii) {
        work(governor, chars.len())?;
        if !chars.iter().all(|&c| c.is_ascii_alphanumeric() || c == '-') {
            return Ok(false);
        }
    } else {
        for &character in chars {
            if !code_point_ok(character, governor)? {
                return Ok(false);
            }
        }
    }
    for position in 0..chars.len() {
        if !contextj_ok(chars, position, governor)? || !contexto_ok(chars, position, governor)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn process_label(
    label: &str,
    idn: bool,
    governor: &mut Governor,
) -> Result<Option<(ScratchVec<char>, usize)>, StopReason> {
    if label.is_empty() {
        return Ok(None);
    }
    let mapped = if idn {
        let Some(mapped) = map_and_normalize(label, governor)? else {
            return Ok(None);
        };
        mapped
    } else {
        work(governor, label.len())?;
        if !label.is_ascii() {
            return Ok(None);
        }
        let mut chars = ScratchVec::new(governor);
        for character in label.chars() {
            push_scratch(&mut chars, character, governor)?;
        }
        chars
    };
    work(governor, 4)?;
    let ace_prefixed = mapped.len() >= 4
        && mapped[0].eq_ignore_ascii_case(&'x')
        && mapped[1].eq_ignore_ascii_case(&'n')
        && mapped[2] == '-'
        && mapped[3] == '-';
    let chars = if ace_prefixed {
        let Some(decoded) = decode_a_label(&mapped[4..], governor)? else {
            return Ok(None);
        };
        decoded
    } else {
        mapped
    };
    if !content_ok(&chars, governor)? {
        return Ok(None);
    }
    let Some(length) = a_label_len(&chars, governor)? else {
        return Ok(None);
    };
    if length == 0 || length > 63 {
        return Ok(None);
    }
    Ok(Some((chars, length)))
}
pub(super) fn is_hostname(
    text: &str,
    idn: bool,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    let result = hostname(text, idn, governor)?;
    governor.check()?;
    Ok(result)
}
fn hostname(text: &str, idn: bool, governor: &mut Governor) -> Result<bool, StopReason> {
    work(governor, text.len().saturating_mul(2))?;
    if text.is_empty() {
        return Ok(false);
    }
    let separators: &[char] = if idn {
        &IDN_LABEL_SEPARATORS
    } else {
        std::slice::from_ref(&ASCII_LABEL_SEPARATOR)
    };
    if !idn && !text.is_ascii() {
        return Ok(false);
    }
    let mut labels = ScratchVec::new(governor);
    let mut total_length = 0usize;
    for (count, label) in text.split(|c: char| separators.contains(&c)).enumerate() {
        let Some((chars, length)) = process_label(label, idn, governor)? else {
            return Ok(false);
        };
        if count > 0 {
            total_length += 1;
        }
        total_length += length;
        push_scratch(&mut labels, chars, governor)?;
    }
    if bidi_trigger(&labels, governor)? {
        for label in &labels {
            if !bidi_rule_ok(label, governor)? {
                return Ok(false);
            }
        }
    }
    Ok(total_length <= 253)
}

#[cfg(test)]
mod tests {
    use super::is_hostname;
    use crate::{Budget, Governor};
    fn check(text: &str, idn: bool) -> bool {
        let mut governor = Governor::new(Budget::default());
        let valid = is_hostname(text, idn, &mut governor).expect("within budget");
        assert_eq!(governor.scratch_meter().live(), 0);
        valid
    }
    #[test]
    fn plain_hostnames_follow_ldh_rules() {
        assert!(check("www.example.com", false));
        assert!(check("host-name", false));
        assert!(check("a--b.com", false));
        assert!(!check("", false));
        assert!(!check("-hostname", false));
        assert!(!check("host_name", false));
        assert!(!check("example\u{ff0e}com", false));
    }
    #[test]
    fn a_labels_decode_and_re_validate_the_u_label() {
        assert!(check("xn--9n2bp8q.xn--9t4b11yi5a", false));
        assert!(!check("xn--X", false));
        assert!(!check("xn--07jt112bpxg.xn--9t4b11yi5a", false));
        assert!(check("xn--ll-0ea", false));
        assert!(!check("xn--al-0ea", false));
        assert!(check("xn--wva3je", false));
        assert!(!check("xn--wva3j", false));
        assert!(check("xn--11b2ezcw70k", false));
        assert!(!check("xn--11b2er09f", false));
    }
    #[test]
    fn idn_hostnames_map_and_normalize_before_validation() {
        assert!(check("실례.테스트", true));
        assert!(check("cafe\u{301}.com", true));
        assert!(check("\u{ff11}\u{ff12}\u{ff13}", true));
        assert!(check("a\u{200b}b", true));
        assert!(!check("\u{302e}실례.테스트", true));
        assert!(!check("0a.\u{5d0}", true));
        assert!(!check("a\u{5d0}", true));
    }
}
