// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! First-party Unicode normalization and text comparison foundations.
mod idna_data;
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

fn confusable_prototype(character: char, output: &mut Vec<u32>) {
    let code = u32::from(character);
    if let Ok(index) = unicode_data::CONFUSABLES.binary_search_by_key(&code, |entry| entry.0) {
        output.extend_from_slice(unicode_data::CONFUSABLES[index].1);
    } else {
        output.push(code);
    }
}

/// UTS #39 confusable skeleton: `NFD(map(NFD(input)))`.
///
/// Each scalar value is substituted for its confusable prototype from the
/// pinned Unicode 17.0.0 security data (`vendor/unicode/manifest.json`). The
/// published table is already the transitive closure of confusable
/// substitutions, so one pass suffices (UTS #39 section 4). Equal skeletons
/// are evidence of visual confusability, not identity or even similarity in
/// meaning; this is a named matching signal, weighed alongside others, never
/// an automatic acceptance rule on its own.
///
/// ```
/// use purrassist_core::text::skeleton;
/// // Cyrillic а (U+0430) is confusable with Latin a (U+0061).
/// assert_eq!(skeleton("\u{430}dmin"), skeleton("admin"));
/// assert_ne!(skeleton("admin"), skeleton("guest"));
/// ```
///
/// # Panics
/// Panics only if generated, conformance-tested tables contain a non-scalar value.
#[must_use]
pub fn skeleton(input: &str) -> String {
    let decomposed = normalize(input, Normalization::Nfd);
    let mut mapped = Vec::with_capacity(decomposed.len());
    for character in decomposed.chars() {
        confusable_prototype(character, &mut mapped);
    }
    let intermediate: String = mapped
        .into_iter()
        .map(|point| char::from_u32(point).expect("Unicode tables contain scalar values"))
        .collect();
    normalize(&intermediate, Normalization::Nfd)
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

/// The same table `general_category` searches, for `pattern`'s
/// `\p{General_Category=Assigned}` computation and Script/binary-property
/// generation cross-checks; not part of the public API.
pub(crate) const fn general_categories() -> &'static [(u32, u32, &'static str)] {
    unicode_data::GENERAL_CATEGORIES
}

/// IDNA/UTS-46 mapping status and (for `mapped`/`deviation` code points with a
/// mapping) the replacement scalar sequence, from the pinned Unicode 17
/// `IdnaMappingTable.txt` (`vendor/unicode/manifest.json`).
///
/// Status is one of `valid`, `mapped`, `disallowed`, `disallowed_STD3_valid`,
/// `disallowed_STD3_mapped`, `deviation`, or `ignored` (UTS #46 table 1). This
/// accessor only exposes the generated table; hostname/idn-hostname format
/// validation that consumes it is implemented in the Task 7 format
/// validators, not here.
#[allow(
    dead_code,
    reason = "generated table and accessor land in Task 6 for the Task 7 hostname/idn-hostname \
              format validators to consume; exercised here by \
              idna_mapping_matches_published_examples pending that caller"
)]
pub(crate) fn idna_status(code: u32) -> (&'static str, &'static [u32]) {
    let entries = idna_data::IDNA_MAPPING;
    let position = entries.partition_point(|(_, end, _, _)| *end < code);
    entries
        .get(position)
        .filter(|(start, _, _, _)| *start <= code)
        .map_or(("disallowed", &[]), |(_, _, status, mapping)| {
            (status, mapping)
        })
}

#[cfg(test)]
mod tests {
    use super::{
        Normalization, case_fold, comparison_key, depunctuate, general_categories, idna_status,
        normalize, skeleton,
    };
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
    #[test]
    fn confusable_skeletons_collapse_lookalikes_without_hiding_real_differences() {
        assert_eq!(skeleton("\u{430}dmin"), skeleton("admin"));
        assert_eq!(skeleton("p\u{430}yp\u{430}l"), skeleton("paypal"));
        assert_ne!(skeleton("admin"), skeleton("guest"));
        assert_eq!(skeleton(""), "");
    }
    #[test]
    fn general_category_table_is_sorted_and_nonoverlapping() {
        let table = general_categories();
        assert_ne!(table.len(), 0);
        for pair in table.windows(2) {
            assert!(
                pair[0].1 < pair[1].0,
                "{:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }
    }
    #[test]
    fn idna_mapping_matches_published_examples() {
        // IdnaMappingTable.txt (Unicode 17): uppercase ASCII maps to lowercase.
        assert_eq!(idna_status(u32::from('A')), ("mapped", [0x61].as_slice()));
        // ASCII lowercase letters and digits are valid unchanged.
        assert_eq!(idna_status(u32::from('a')), ("valid", [].as_slice()));
        assert_eq!(idna_status(u32::from('0')), ("valid", [].as_slice()));
        // U+00DF LATIN SMALL LETTER SHARP S is a deviation code point (maps to
        // "ss" under the non-transitional processing that IDNA2008 uses, but
        // is left as a deviation rather than an unconditional mapping).
        assert_eq!(idna_status(0x00DF), ("deviation", [0x73, 0x73].as_slice()));
        // Noncharacters are disallowed.
        assert_eq!(idna_status(0xFFFF).0, "disallowed");
        // The table partitions the full scalar range with no gaps, including
        // its final code point.
        assert_ne!(idna_status(0x10_FFFF).0, "");
    }
}
