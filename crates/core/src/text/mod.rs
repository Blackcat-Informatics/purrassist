// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! First-party Unicode normalization and text comparison foundations.
mod idna_data;
mod normalization;
use crate::contract::{Operation, Text, ordinary};
use crate::{Governor, ScratchString, StopReason};
pub(crate) mod punycode;
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
    match ordinary(normalization::run(input, form, &mut Operation(None))) {
        Text::Plain(output) => output,
        Text::Admitted(_) => unreachable!("INVARIANT: ordinary normalization uses plain storage"),
    }
}

/// Normalize into a temporary owner without publishing intermediate text.
pub(crate) fn normalize_owned(
    input: &str,
    form: Normalization,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    normalization::run(input, form, &mut Operation(Some(governor))).map(Text::into_admitted)
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

fn category(character: char) -> u8 {
    let code = u32::from(character);
    let index = unicode_data::CATEGORIES.partition_point(|entry| entry.0 <= code);
    if index > 0 && unicode_data::CATEGORIES[index - 1].1 >= code {
        unicode_data::CATEGORIES[index - 1].2
    } else {
        0
    }
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

/// Admit the generated category lookup used by hostname checks.
pub(crate) fn general_category_governed(
    character: char,
    governor: &mut Governor,
) -> Result<&'static str, StopReason> {
    Operation(Some(governor)).lookup(unicode_data::GENERAL_CATEGORIES.len())?;
    Ok(general_category(character))
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
/// validation that consumes it is implemented in the idn-hostname format
/// validators, not here.
pub(crate) fn idna_status(
    code: u32,
    governor: &mut Governor,
) -> Result<(&'static str, &'static [u32]), StopReason> {
    Operation(Some(governor)).lookup(idna_data::IDNA_MAPPING.len())?;
    let entries = idna_data::IDNA_MAPPING;
    let position = entries.partition_point(|(_, end, _, _)| *end < code);
    Ok(entries
        .get(position)
        .filter(|(start, _, _, _)| *start <= code)
        .map_or(("disallowed", &[]), |(_, _, status, mapping)| {
            (status, mapping)
        }))
}

/// `Canonical_Combining_Class` (UAX #44): non-zero exactly for combining marks;
/// `9` identifies Virama, which the idn-hostname validator's RFC 5892
/// Appendix A.1/A.2 CONTEXTJ rule for ZWNJ/ZWJ requires (the same generated
/// table `normalize` uses for canonical reordering).
pub(crate) fn canonical_combining_class(
    code: u32,
    governor: &mut Governor,
) -> Result<u8, StopReason> {
    normalization::combining(code, &mut Operation(Some(governor)))
}

/// `Bidi_Class` (UAX #44) as an explicit, gap-free partition of the whole
/// scalar range (`@missing` defaults already expanded by the generator), for
/// the idn-hostname validator's RFC 5893 Bidi rule.
pub(crate) fn bidi_class(code: u32, governor: &mut Governor) -> Result<&'static str, StopReason> {
    Operation(Some(governor)).lookup(idna_data::BIDI_CLASS.len())?;
    let entries = idna_data::BIDI_CLASS;
    let position = entries.partition_point(|(_, end, _)| *end < code);
    Ok(entries
        .get(position)
        .filter(|(start, _, _)| *start <= code)
        .map_or("L", |(_, _, class)| *class))
}

/// `Joining_Type` (UAX #44) as an explicit, gap-free partition of the whole
/// scalar range (`U` = `Non_Joining` where the source `@missing` default
/// applies), for the idn-hostname validator's RFC 5892 Appendix A.1
/// CONTEXTJ rule for ZWNJ.
pub(crate) fn joining_type(code: u32, governor: &mut Governor) -> Result<&'static str, StopReason> {
    Operation(Some(governor)).lookup(idna_data::JOINING_TYPE.len())?;
    let entries = idna_data::JOINING_TYPE;
    let position = entries.partition_point(|(_, end, _)| *end < code);
    Ok(entries
        .get(position)
        .filter(|(start, _, _)| *start <= code)
        .map_or("U", |(_, _, kind)| *kind))
}

#[cfg(test)]
mod tests {
    use super::{
        Normalization, case_fold, comparison_key, depunctuate, general_categories, normalize,
        skeleton,
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
            ("a\u{315}\u{301}\u{300}", "á\u{300}\u{315}"),
        ] {
            assert_eq!(normalize(source, Normalization::Nfc), expected);
            assert_eq!(normalize(source, Normalization::Nfkc), expected);
        }
    }
    #[test]
    fn normalization_owns_buffers_and_preserves_refusals() {
        use crate::{Budget, Governor, StopReason, contract::Resource};
        for (source, form) in [
            ("a\u{315}\u{301}\u{300}", Normalization::Nfc),
            ("é각ﬃ", Normalization::Nfkd),
            ("e\u{301}", Normalization::Nfc),
        ] {
            let mut governor = Governor::new(Budget::default());
            let owned = super::normalize_owned(source, form, &mut governor).unwrap();
            assert_eq!(owned.as_str(), normalize(source, form));
            assert!(governor.scratch_meter().live() > 0);
            let usage = governor.usage();
            drop(owned);
            assert_eq!(governor.scratch_meter().live(), 0);
            for (budget, resource) in [
                (
                    Budget {
                        work: usage.work - 1,
                        ..Budget::default()
                    },
                    Resource::Work,
                ),
                (
                    Budget {
                        scratch_bytes: usage.peak_scratch_bytes - 1,
                        ..Budget::default()
                    },
                    Resource::ScratchBytes,
                ),
            ] {
                let mut refused = Governor::new(budget);
                assert_eq!(
                    super::normalize_owned(source, form, &mut refused).unwrap_err(),
                    StopReason::BudgetExceeded(resource)
                );
                assert_eq!(refused.scratch_meter().live(), 0);
            }
            let mut cancelled = Governor::new(Budget::default());
            cancelled.cancel();
            assert_eq!(
                super::normalize_owned(source, form, &mut cancelled).unwrap_err(),
                StopReason::Cancelled
            );
            assert_eq!(cancelled.scratch_meter().live(), 0);
        }
        let mut governor = Governor::new(Budget {
            depth: 0,
            ..Budget::default()
        });
        assert_eq!(
            super::normalize_owned("é", Normalization::Nfc, &mut governor).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Depth)
        );
        assert_eq!(governor.scratch_meter().live(), 0);
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
        let idna_status = |code| {
            super::idna_status(code, &mut crate::Governor::new(crate::Budget::default()))
                .expect("within budget")
        };
        // IdnaMappingTable.txt (Unicode 17): uppercase ASCII maps to lowercase.
        assert_eq!(idna_status(u32::from('A')), ("mapped", [0x61].as_slice()));
        // ASCII lowercase letters and digits are valid unchanged.
        assert_eq!(idna_status(u32::from('a')), ("valid", [].as_slice()));
        assert_eq!(idna_status(u32::from('0')), ("valid", [].as_slice()));
        // U+00DF LATIN SMALL LETTER SHARP S is a deviation code point (maps to
        // "ss" under the non-transitional processing that IDNA 2008 uses, but
        // is left as a deviation rather than an unconditional mapping).
        assert_eq!(idna_status(0x00DF), ("deviation", [0x73, 0x73].as_slice()));
        // Noncharacters are disallowed.
        assert_eq!(idna_status(0xFFFF).0, "disallowed");
        // The table partitions the full scalar range with no gaps, including
        // its final code point.
        assert_ne!(idna_status(0x10_FFFF).0, "");
    }
}
