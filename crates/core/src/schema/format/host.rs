// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 1123 `hostname` and IDNA2008 (RFC 5890-5893) `idn-hostname`, sharing
//! one per-label pipeline: both accept `xn--` A-labels (decoded and
//! re-validated as their U-label), and `idn-hostname` additionally accepts
//! raw Unicode labels through UTS-46 (`IdnaMappingTable.txt`) nontransitional
//! mapping before the same RFC 5892/5893 contextual and Bidi checks apply.
use crate::contract::{Governor, Resource, StopReason};
use crate::text::{bidi_class, canonical_combining_class, idna_status, joining_type, punycode};

const ASCII_LABEL_SEPARATOR: char = '.';
/// RFC 3490 section 3.1: dots equivalent to U+002E for label separation.
const IDN_LABEL_SEPARATORS: [char; 4] = ['.', '\u{3002}', '\u{FF0E}', '\u{FF61}'];

fn is_leading_combining_mark(c: char) -> bool {
    matches!(crate::text::general_category(c), "Mn" | "Mc" | "Me")
}

/// RFC 5891 section 4.2.3.1: never start/end with a hyphen, and never carry
/// the ACE-reserved hyphen pattern (`--` at the third/fourth character)
/// unless the label truly is an ACE (`xn--`) label.
fn hyphen_restriction_ok(chars: &[char]) -> bool {
    chars.first().is_none_or(|&c| c != '-')
        && chars.last().is_none_or(|&c| c != '-')
        && !(chars.len() >= 4 && chars[2] == '-' && chars[3] == '-')
}

fn contextj_zwnj_regexp_ok(chars: &[char], position: usize) -> bool {
    let before_ok = (0..position)
        .rev()
        .map(|i| joining_type(u32::from(chars[i])))
        .find(|jt| *jt != "T")
        .is_some_and(|jt| matches!(jt, "L" | "D"));
    let after_ok = (position + 1..chars.len())
        .map(|i| joining_type(u32::from(chars[i])))
        .find(|jt| *jt != "T")
        .is_some_and(|jt| matches!(jt, "R" | "D"));
    before_ok && after_ok
}

/// RFC 5892 Appendix A.1/A.2: ZWNJ/ZWJ contextual join rules.
fn contextj_ok(chars: &[char], position: usize) -> bool {
    let preceded_by_virama =
        position > 0 && canonical_combining_class(u32::from(chars[position - 1])) == 9;
    match chars[position] {
        '\u{200C}' => preceded_by_virama || contextj_zwnj_regexp_ok(chars, position),
        '\u{200D}' => preceded_by_virama,
        _ => true,
    }
}

/// RFC 5892 Appendix A.3-A.9: the remaining CONTEXTO code points.
fn contexto_ok(chars: &[char], position: usize) -> bool {
    let script_at = |index: usize| crate::pattern::script_of(u32::from(chars[index]));
    match chars[position] {
        '\u{B7}' => {
            position > 0 && chars[position - 1] == 'l' && chars.get(position + 1) == Some(&'l')
        }
        '\u{375}' => chars
            .get(position + 1)
            .is_some_and(|&c| crate::pattern::script_of(u32::from(c)) == "Greek"),
        '\u{5F3}' | '\u{5F4}' => position > 0 && script_at(position - 1) == "Hebrew",
        '\u{30FB}' => chars.iter().any(|&c| {
            matches!(
                crate::pattern::script_of(u32::from(c)),
                "Hiragana" | "Katakana" | "Han"
            )
        }),
        '\u{660}'..='\u{669}' => !chars.iter().any(|&c| ('\u{6F0}'..='\u{6F9}').contains(&c)),
        '\u{6F0}'..='\u{6F9}' => !chars.iter().any(|&c| ('\u{660}'..='\u{669}').contains(&c)),
        _ => true,
    }
}

fn bidi_trigger(all_labels: &[Vec<char>]) -> bool {
    all_labels
        .iter()
        .flatten()
        .any(|&c| matches!(bidi_class(u32::from(c)), "R" | "AL" | "AN"))
}

/// RFC 5893 section 2, the six-condition Bidi rule for one label, applied to
/// every label once any label anywhere in the name is RTL/Arabic-indic.
fn bidi_rule_ok(chars: &[char]) -> bool {
    let classes: Vec<&str> = chars.iter().map(|&c| bidi_class(u32::from(c))).collect();
    let Some(&first) = classes.first() else {
        return false;
    };
    let last_non_nsm = classes.iter().rev().find(|&&class| class != "NSM").copied();
    if first == "L" {
        classes.iter().all(|class| {
            matches!(
                *class,
                "L" | "EN" | "ES" | "CS" | "ET" | "ON" | "BN" | "NSM"
            )
        }) && matches!(last_non_nsm, Some("L" | "EN"))
    } else if matches!(first, "R" | "AL") {
        let mut european_digit = false;
        let mut arabic_digit = false;
        for &class in &classes {
            if !matches!(
                class,
                "R" | "AL" | "AN" | "EN" | "ES" | "CS" | "ET" | "ON" | "BN" | "NSM"
            ) {
                return false;
            }
            european_digit |= class == "EN";
            arabic_digit |= class == "AN";
        }
        !(european_digit && arabic_digit) && matches!(last_non_nsm, Some("R" | "AL" | "EN" | "AN"))
    } else {
        false
    }
}

/// UTS-46 nontransitional mapping (`IdnaMappingTable.txt`), then NFC.
fn map_and_normalize(
    label: &str,
    governor: &mut Governor,
) -> Result<Option<Vec<char>>, StopReason> {
    let mut mapped = String::new();
    for character in label.chars() {
        governor.charge(Resource::Work, 1)?;
        let (status, replacement) = idna_status(u32::from(character));
        match status {
            "valid" | "deviation" => mapped.push(character),
            "mapped" | "disallowed_STD3_mapped" => {
                for &point in replacement {
                    mapped.push(char::from_u32(point).unwrap_or(char::REPLACEMENT_CHARACTER));
                }
            }
            "ignored" => {}
            _ => return Ok(None),
        }
    }
    let normalized = crate::text::normalize(&mapped, crate::text::Normalization::Nfc);
    Ok(Some(normalized.chars().collect()))
}

/// Decode an `xn--` A-label to its U-label, verifying the RFC 5891 section
/// 5.4 round-trip (lowercased before re-encoding, per section 5.3) and that
/// it is not all-ASCII (RFC 5890 section 2.3.2.1).
fn decode_a_label(remainder: &str) -> Option<Vec<char>> {
    let lowered = remainder.to_ascii_lowercase();
    let decoded = punycode::decode(&lowered)?;
    if decoded.iter().all(char::is_ascii) {
        return None;
    }
    (punycode::encode(&decoded)? == lowered).then_some(decoded)
}

/// The label's on-wire A-label octet length (RFC 5891 section 4.2.4): itself
/// if ASCII, or `xn--` plus its Punycode encoding otherwise.
fn a_label_len(chars: &[char]) -> Option<usize> {
    if chars.iter().all(char::is_ascii) {
        Some(chars.len())
    } else {
        Some(4 + punycode::encode(chars)?.len())
    }
}

/// RFC 5892 section 2.6's small, fixed DISALLOWED-override table: code
/// points UTS-46's `IdnaMappingTable.txt` marks `valid` but whose IDNA2008
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
/// idn-hostname format is IDNA2008, not bare UTS-46.
fn is_default_pvalid_category(category: &str) -> bool {
    matches!(
        category,
        "Ll" | "Lu" | "Lo" | "Lm" | "Lt" | "Mn" | "Mc" | "Me" | "Nd"
    )
}

/// One non-ASCII code point's IDNA2008 derived property (RFC 5892 section
/// 2), folding in the section 2.6 exceptions table and the CONTEXTJ/
/// CONTEXTO code points (whose *contextual* validity `contextj_ok`/
/// `contexto_ok` check separately; here they are only exempted from the
/// default category-based DISALLOWED).
fn code_point_ok(c: char) -> bool {
    if is_derived_disallowed(c) {
        return false;
    }
    matches!(
        c,
        '\u{DF}' | '\u{3C2}' | '\u{6FD}' | '\u{6FE}' | '\u{F0B}' | '\u{3007}' // 2.6 PVALID
            | '\u{200C}' | '\u{200D}' // CONTEXTJ
            | '\u{B7}' | '\u{375}' | '\u{5F3}' | '\u{5F4}' | '\u{30FB}' // CONTEXTO
    ) || ('\u{660}'..='\u{669}').contains(&c)
        || ('\u{6F0}'..='\u{6F9}').contains(&c) // CONTEXTO digits
        || is_default_pvalid_category(crate::text::general_category(c))
}

fn content_ok(chars: &[char]) -> bool {
    if chars.is_empty() || !hyphen_restriction_ok(chars) {
        return false;
    }
    if chars.first().is_some_and(|&c| is_leading_combining_mark(c)) {
        return false;
    }
    let ascii_shape_ok = chars.iter().all(char::is_ascii);
    if ascii_shape_ok {
        if !chars.iter().all(|&c| c.is_ascii_alphanumeric() || c == '-') {
            return false;
        }
    } else if !chars.iter().all(|&c| code_point_ok(c)) {
        return false;
    }
    (0..chars.len()).all(|i| contextj_ok(chars, i) && contexto_ok(chars, i))
}

/// One label, `idn` selecting whether raw Unicode/UTS-46 mapping is
/// permitted (`hostname` only accepts `xn--` A-labels for non-ASCII
/// content). Returns the U-label's characters and its A-label octet length.
fn process_label(
    label: &str,
    idn: bool,
    governor: &mut Governor,
) -> Result<Option<(Vec<char>, usize)>, StopReason> {
    if label.is_empty() {
        return Ok(None);
    }
    let mapped: Vec<char> = if idn {
        let Some(mapped) = map_and_normalize(label, governor)? else {
            return Ok(None);
        };
        mapped
    } else {
        if !label.is_ascii() {
            return Ok(None);
        }
        label.chars().collect()
    };
    let mapped_text: String = mapped.iter().collect();
    let ace_prefixed = mapped_text.len() >= 4
        && mapped_text.as_bytes()[..2].eq_ignore_ascii_case(b"xn")
        && &mapped_text.as_bytes()[2..4] == b"--";
    let chars = if ace_prefixed {
        let Some(decoded) = decode_a_label(&mapped_text[4..]) else {
            return Ok(None);
        };
        decoded
    } else {
        mapped
    };
    if !content_ok(&chars) {
        return Ok(None);
    }
    let Some(length) = a_label_len(&chars) else {
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
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
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
    let mut label_chars = Vec::new();
    let mut total_length = 0usize;
    for (label_count, label) in text.split(|c: char| separators.contains(&c)).enumerate() {
        governor.charge(
            Resource::ScratchBytes,
            u64::try_from(label.len().saturating_add(64)).unwrap_or(u64::MAX),
        )?;
        let Some((chars, length)) = process_label(label, idn, governor)? else {
            return Ok(false);
        };
        if label_count > 0 {
            total_length += 1;
        }
        total_length += length;
        label_chars.push(chars);
    }
    if bidi_trigger(&label_chars) && !label_chars.iter().all(|chars| bidi_rule_ok(chars)) {
        return Ok(false);
    }
    Ok(total_length <= 253)
}

#[cfg(test)]
mod tests {
    use super::is_hostname;
    use crate::{Budget, Governor};
    fn check(text: &str, idn: bool) -> bool {
        is_hostname(text, idn, &mut Governor::new(Budget::default())).expect("within budget")
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
