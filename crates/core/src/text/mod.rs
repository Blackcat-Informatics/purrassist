// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! First-party Unicode normalization and text comparison foundations.
mod unicode_data;

/// Unicode data version, independent of the compiler's built-in character tables.
pub const UNICODE_VERSION: &str = "17.0.0";

/// Unicode normalization form from UAX #15.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Normalization {
    /// Canonical decomposition.
    Nfd,
    /// Canonical decomposition and composition.
    Nfc,
    /// Compatibility decomposition.
    Nfkd,
    /// Compatibility decomposition and canonical composition.
    Nfkc,
}

/// Normalize text using pinned Unicode data, including algorithmic Hangul handling.
///
/// # Panics
/// Panics only if generated, conformance-tested tables contain a non-scalar value.
#[must_use]
pub fn normalize(input: &str, form: Normalization) -> String {
    let compatibility = matches!(form, Normalization::Nfkd | Normalization::Nfkc);
    let mut decomposed = Vec::with_capacity(input.len());
    for character in input.chars() {
        decompose(u32::from(character), compatibility, &mut decomposed);
    }
    let mut start = 0;
    for index in 1..decomposed.len() {
        if combining(decomposed[index]) == 0 {
            decomposed[start..index].sort_by_key(|point| combining(*point));
            start = index;
        }
    }
    decomposed[start..].sort_by_key(|point| combining(*point));
    if matches!(form, Normalization::Nfc | Normalization::Nfkc) {
        decomposed = compose(&decomposed);
    }
    decomposed
        .into_iter()
        .map(|point| char::from_u32(point).expect("Unicode tables contain scalar values"))
        .collect()
}

/// Full default Unicode case folding (C and F mappings; no locale tailoring).
///
/// # Panics
/// Panics only if generated, conformance-tested tables contain a non-scalar value.
#[must_use]
pub fn case_fold(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        let code = u32::from(character);
        if let Ok(index) = unicode_data::CASE_FOLD.binary_search_by_key(&code, |entry| entry.0) {
            output.extend(
                unicode_data::CASE_FOLD[index]
                    .1
                    .iter()
                    .map(|point| char::from_u32(*point).expect("Unicode scalar mapping")),
            );
        } else {
            output.push(character);
        }
    }
    output
}

/// A comparison key: NFKC of full case folding of NFKD input.
/// Original strings and path identities must remain separate from this key.
#[must_use]
pub fn comparison_key(input: &str) -> String {
    normalize(
        &case_fold(&normalize(input, Normalization::Nfkd)),
        Normalization::Nfkc,
    )
}

/// Remove punctuation and separator categories from a comparison key.
/// Symbols such as `+` remain significant.
#[must_use]
pub fn depunctuate(input: &str) -> String {
    comparison_key(input)
        .chars()
        .filter(|character| category(*character) & 4 == 0)
        .collect()
}

/// Letter/number/mark runs for lexical matching; this is a named matching profile,
/// not a claim to implement natural-language word segmentation.
#[must_use]
pub fn lexical_tokens(input: &str) -> Vec<String> {
    comparison_key(input)
        .split(|character| category(character).trailing_zeros() >= 2)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn combining(code: u32) -> u8 {
    unicode_data::COMBINING
        .binary_search_by_key(&code, |entry| entry.0)
        .map_or(0, |index| unicode_data::COMBINING[index].1)
}
fn category(character: char) -> u8 {
    let code = u32::from(character);
    let index = unicode_data::CATEGORIES.partition_point(|entry| entry.0 <= code);
    if index > 0 && unicode_data::CATEGORIES[index - 1].1 >= code {
        unicode_data::CATEGORIES[index - 1].2
    } else {
        0
    }
}
fn decompose(code: u32, compatibility: bool, output: &mut Vec<u32>) {
    if (0xac00..0xd7a4).contains(&code) {
        let index = code - 0xac00;
        output.push(0x1100 + index / 588);
        output.push(0x1161 + (index % 588) / 28);
        if !index.is_multiple_of(28) {
            output.push(0x11a7 + index % 28);
        }
        return;
    }
    if let Ok(index) = unicode_data::DECOMPOSITION.binary_search_by_key(&code, |entry| entry.0) {
        let (_, compat, mapping) = unicode_data::DECOMPOSITION[index];
        if !compat || compatibility {
            for point in mapping {
                decompose(*point, compatibility, output);
            }
            return;
        }
    }
    output.push(code);
}
fn pair(first: u32, second: u32) -> Option<u32> {
    if (0x1100..0x1113).contains(&first) && (0x1161..0x1176).contains(&second) {
        return Some(0xac00 + (first - 0x1100) * 588 + (second - 0x1161) * 28);
    }
    if (0xac00..0xd7a4).contains(&first)
        && (first - 0xac00).is_multiple_of(28)
        && (0x11a8..0x11c3).contains(&second)
    {
        return Some(first + second - 0x11a7);
    }
    unicode_data::COMPOSITION
        .binary_search_by_key(&(first, second), |entry| (entry.0, entry.1))
        .ok()
        .map(|index| unicode_data::COMPOSITION[index].2)
}
fn compose(input: &[u32]) -> Vec<u32> {
    let mut output = Vec::with_capacity(input.len());
    // Keep the index defined before a starter exists: optimized bounds-check
    // motion can inspect an Option payload before its discriminant guard.
    let mut starter = 0;
    let mut has_starter = false;
    let mut last_class = 0;
    for &code in input {
        let class = combining(code);
        if has_starter
            && (last_class == 0 || last_class < class)
            && let Some(composed) = pair(output[starter], code)
        {
            output[starter] = composed;
            continue;
        }
        if class == 0 {
            starter = output.len();
            has_starter = true;
        }
        output.push(code);
        last_class = class;
    }
    output
}

/// Unicode 17 general category, including `Cn` for unassigned scalar values.
#[must_use]
pub fn general_category(character: char) -> &'static str {
    let code = u32::from(character);
    let entries = unicode_data::GENERAL_CATEGORIES;
    let position = entries.partition_point(|(_, end, _)| *end < code);
    entries
        .get(position)
        .filter(|(start, _, _)| *start <= code)
        .map_or("Cn", |(_, _, category)| *category)
}

#[cfg(test)]
mod tests {
    use super::{Normalization, case_fold, comparison_key, depunctuate, normalize};
    #[test]
    fn canonical_compatibility_and_hangul() {
        assert_eq!(normalize("e\u{301}", Normalization::Nfc), "é");
        assert_eq!(normalize("é", Normalization::Nfd), "e\u{301}");
        assert_eq!(normalize("ﬃ ①", Normalization::Nfkc), "ffi 1");
        assert_eq!(
            normalize("\u{1100}\u{1161}\u{11a8}", Normalization::Nfc),
            "각"
        );
        assert_eq!(
            normalize("각", Normalization::Nfd),
            "\u{1100}\u{1161}\u{11a8}"
        );
    }
    #[test]
    fn composition_before_and_after_the_first_starter() {
        for (source, expected) in [
            ("", ""),
            ("\u{301}\u{300}", "\u{301}\u{300}"),
            ("\u{301}e\u{301}", "\u{301}é"),
            ("a\u{315}\u{300}", "à\u{315}"),
        ] {
            assert_eq!(normalize(source, Normalization::Nfc), expected);
            assert_eq!(normalize(source, Normalization::Nfkc), expected);
        }
    }
    #[test]
    fn casing_does_not_depend_on_host_locale() {
        assert_eq!(case_fold("Straße Σς"), "strasse σσ");
        assert_eq!(comparison_key("ＣＡＦÉ"), comparison_key("cafe\u{301}"));
        assert_eq!(depunctuate("User_Name.json"), "usernamejson");
        assert_eq!(depunctuate("C++"), "c++");
    }
}
