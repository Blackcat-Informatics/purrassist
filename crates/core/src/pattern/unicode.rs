// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Unicode property lookups for `\p{...}`/`\P{...}`: `General_Category`
//! (delegating to `crate::text`'s table), `Script`, `Script_Extensions`, and
//! every ECMA-262 binary property, from the generated tables in
//! `unicode_data.rs` (Unicode 17; inputs pinned in
//! `vendor/unicode/manifest.json`).
mod unicode_data;

/// The domain of values a Rust `char` can hold: every Unicode scalar value
/// except the surrogate range, which UTF-8 (and therefore `char`/`str`) can
/// never encode. Every character class this module or `parse.rs` builds is
/// clipped to this domain as the last step, so a negated class (`[^x]`,
/// `\D`, `\P{...}`) never includes a codepoint no `char` can ever equal.
pub(super) const SCALAR_DOMAIN: &[(u32, u32)] = &[(0x0, 0xD7FF), (0xE000, 0x10_FFFF)];

/// Sort and coalesce overlapping/adjacent `(start, end)` pairs.
pub(super) fn merge(mut intervals: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    intervals.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(intervals.len());
    for (start, end) in intervals {
        if let Some(last) = out.last_mut()
            && start <= last.1.saturating_add(1)
        {
            last.1 = last.1.max(end);
            continue;
        }
        out.push((start, end));
    }
    out
}

/// `domain - ranges`, for `ranges` sorted and merged.
pub(super) fn complement_within(ranges: &[(u32, u32)], domain: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut iter = ranges.iter().copied().peekable();
    for &(dom_start, dom_end) in domain {
        let mut pos = dom_start;
        while pos <= dom_end {
            match iter.peek().copied() {
                Some((_start, end)) if end < pos => {
                    iter.next();
                }
                Some((start, _end)) if start > dom_end => break,
                Some((start, end)) if start > pos => {
                    out.push((pos, start - 1));
                    pos = start;
                    let _ = end;
                }
                Some((_start, end)) => {
                    pos = end.saturating_add(1);
                    if pos <= dom_end {
                        iter.next();
                    }
                }
                None => {
                    out.push((pos, dom_end));
                    pos = dom_end.saturating_add(1);
                }
            }
        }
    }
    merge(out)
}

/// Full-Unicode complement (`0..=0x10_FFFF` minus `ranges`), used for `[^...]`,
/// `\D`/`\W`/`\S`, and `\P{...}` before the final [`SCALAR_DOMAIN`] clip.
pub(super) fn negate(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    complement_within(ranges, &[(0x0, 0x10_FFFF)])
}

/// Restrict `ranges` to values a `char` can hold. Every class this module
/// returns must be passed through this before becoming a
/// [`super::ast::Node::Class`].
pub(super) fn clip_to_scalars(ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    let merged = merge(ranges);
    let mut out = Vec::with_capacity(merged.len());
    for &(dom_start, dom_end) in SCALAR_DOMAIN {
        for &(start, end) in &merged {
            let lo = start.max(dom_start);
            let hi = end.min(dom_end);
            if lo <= hi {
                out.push((lo, hi));
            }
        }
    }
    merge(out)
}

/// `true` if `code` falls in one of `intervals` (sorted, disjoint, merged).
pub(super) fn interval_contains(intervals: &[(u32, u32)], code: u32) -> bool {
    let position = intervals.partition_point(|(start, _)| *start <= code);
    position > 0 && intervals[position - 1].1 >= code
}

/// Canonical `General_Category` value/group name for `name`, or `None` if it
/// names neither a value nor a recognized grouping. Matches
/// [`ECMA-262 §22.2.2.9.3`](https://tc39.es/ecma262/#table-nonbinary-unicode-properties)'s
/// `General_Category` aliases, plus its derived groupings (`L`, `LC`, `M`,
/// `N`, `P`, `S`, `Z`, `C`).
fn category_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "L" | "Letter" => "L",
        "LC" | "Cased_Letter" => "LC",
        "Lu" | "Uppercase_Letter" => "Lu",
        "Ll" | "Lowercase_Letter" => "Ll",
        "Lt" | "Titlecase_Letter" => "Lt",
        "Lm" | "Modifier_Letter" => "Lm",
        "Lo" | "Other_Letter" => "Lo",
        "M" | "Mark" | "Combining_Mark" => "M",
        "Mn" | "Nonspacing_Mark" => "Mn",
        "Mc" | "Spacing_Mark" => "Mc",
        "Me" | "Enclosing_Mark" => "Me",
        "N" | "Number" => "N",
        "Nd" | "Decimal_Number" | "digit" => "Nd",
        "Nl" | "Letter_Number" => "Nl",
        "No" | "Other_Number" => "No",
        "P" | "Punctuation" | "punct" => "P",
        "Pc" | "Connector_Punctuation" => "Pc",
        "Pd" | "Dash_Punctuation" => "Pd",
        "Ps" | "Open_Punctuation" => "Ps",
        "Pe" | "Close_Punctuation" => "Pe",
        "Pi" | "Initial_Punctuation" => "Pi",
        "Pf" | "Final_Punctuation" => "Pf",
        "Po" | "Other_Punctuation" => "Po",
        "S" | "Symbol" => "S",
        "Sm" | "Math_Symbol" => "Sm",
        "Sc" | "Currency_Symbol" => "Sc",
        "Sk" | "Modifier_Symbol" => "Sk",
        "So" | "Other_Symbol" => "So",
        "Z" | "Separator" => "Z",
        "Zs" | "Space_Separator" => "Zs",
        "Zl" | "Line_Separator" => "Zl",
        "Zp" | "Paragraph_Separator" => "Zp",
        "C" | "Other" => "C",
        "Cc" | "Control" | "cntrl" => "Cc",
        "Cf" | "Format" => "Cf",
        "Cs" | "Surrogate" => "Cs",
        "Co" | "Private_Use" => "Co",
        "Cn" | "Unassigned" => "Cn",
        _ => return None,
    })
}
fn category_matches(target: &str, actual: &str) -> bool {
    match target {
        "L" => matches!(actual, "Lu" | "Ll" | "Lt" | "Lm" | "Lo"),
        "LC" => matches!(actual, "Lu" | "Ll" | "Lt"),
        "M" => matches!(actual, "Mn" | "Mc" | "Me"),
        "N" => matches!(actual, "Nd" | "Nl" | "No"),
        "P" => matches!(actual, "Pc" | "Pd" | "Ps" | "Pe" | "Pi" | "Pf" | "Po"),
        "S" => matches!(actual, "Sm" | "Sc" | "Sk" | "So"),
        "Z" => matches!(actual, "Zs" | "Zl" | "Zp"),
        "C" => matches!(actual, "Cc" | "Cf" | "Cs" | "Co" | "Cn"),
        exact => exact == actual,
    }
}

/// `General_Category`/`gc` value or group ranges, including the `Cn`
/// (unassigned) gaps the table represents by omission (see
/// `crate::text::general_category`).
pub(super) fn category_ranges(name: &str) -> Option<Vec<(u32, u32)>> {
    let target = category_alias(name)?;
    let mut out = Vec::new();
    let table = crate::text::general_categories();
    for (start, end, actual) in table {
        if category_matches(target, actual) {
            out.push((*start, *end));
        }
    }
    if matches!(target, "C" | "Cn") {
        let assigned = merge(table.iter().map(|(start, end, _)| (*start, *end)).collect());
        let gaps = complement_within(&assigned, &[(0x0, 0x10_FFFF)]);
        out.extend(gaps);
    }
    Some(merge(out))
}

/// Every canonical `Script`/`Script_Extensions` value name (Unicode 17):
/// every distinct value `Scripts.txt` assigns some codepoint, deduplicated.
#[cfg(test)]
pub(super) fn script_names() -> std::collections::BTreeSet<&'static str> {
    unicode_data::SCRIPTS
        .iter()
        .map(|(_, _, name)| *name)
        .collect()
}

/// The `Script` value of one code point (UAX #24); the generated table is a
/// gap-free partition of the whole scalar range, including the explicit
/// `Unknown` default the generator computes for every codepoint no
/// `Scripts.txt` range claims. Exposed crate-wide (via `pattern::script_of`)
/// for the idn-hostname format validator's RFC 5892 Appendix A
/// CONTEXTO rules (Greek/Hebrew/Hiragana/Katakana/Han membership checks).
pub(super) fn script_of(code: u32) -> &'static str {
    let position = unicode_data::SCRIPTS.partition_point(|(_, end, _)| *end < code);
    unicode_data::SCRIPTS
        .get(position)
        .filter(|(start, _, _)| *start <= code)
        .map_or("Unknown", |(_, _, name)| *name)
}

fn script_alias(name: &str) -> Option<&'static str> {
    unicode_data::SCRIPT_ALIASES
        .binary_search_by_key(&name, |(alias, _)| alias)
        .ok()
        .map(|index| unicode_data::SCRIPT_ALIASES[index].1)
}

/// `Script`/`sc` value ranges (UAX #24); accepts either the long value name
/// (as `Scripts.txt` spells it, e.g. `Greek`) or its short alias (`Grek`),
/// per `PropertyValueAliases.txt`.
pub(super) fn script_ranges(name: &str) -> Option<Vec<(u32, u32)>> {
    let canonical = script_alias(name)?;
    Some(
        unicode_data::SCRIPTS
            .iter()
            .filter(|(_, _, script)| *script == canonical)
            .map(|(start, end, _)| (*start, *end))
            .collect(),
    )
}

/// `Script_Extensions`/`scx` value ranges (UAX #24): the precomputed union of
/// each codepoint's default `{Script}` singleton (for codepoints
/// `ScriptExtensions.txt` does not override) and every codepoint the
/// override lists as including this script.
pub(super) fn script_extension_ranges(name: &str) -> Option<Vec<(u32, u32)>> {
    let canonical = script_alias(name)?;
    unicode_data::SCRIPT_EXTENSIONS
        .binary_search_by_key(&canonical, |(script, _)| *script)
        .ok()
        .map(|index| unicode_data::SCRIPT_EXTENSIONS[index].1.to_vec())
}

fn binary_property_alias(name: &str) -> Option<&'static str> {
    unicode_data::BINARY_PROPERTY_ALIASES
        .binary_search_by_key(&name, |(alias, _)| alias)
        .ok()
        .map(|index| unicode_data::BINARY_PROPERTY_ALIASES[index].1)
}

const _: () = assert!(unicode_data::BINARY_PROPERTIES.len() <= 128);
const _: () = assert!(unicode_data::BINARY_PROPERTY_ALIASES.len() <= 128);

/// Borrow the required generated identifier-start property without copying.
pub(super) fn identifier_start() -> &'static [(u32, u32)] {
    binary_property_intervals("ID_Start")
        .expect("generated Unicode data contains required ID_Start property")
}
/// Borrow the required generated identifier-continue property without copying.
pub(super) fn identifier_continue() -> &'static [(u32, u32)] {
    binary_property_intervals("ID_Continue")
        .expect("generated Unicode data contains required ID_Continue property")
}
fn binary_property_intervals(name: &str) -> Option<&'static [(u32, u32)]> {
    let canonical = binary_property_alias(name)?;
    unicode_data::BINARY_PROPERTIES
        .binary_search_by_key(&canonical, |(prop, _)| *prop)
        .ok()
        .map(|index| unicode_data::BINARY_PROPERTIES[index].1)
}

/// Every ECMA-262 binary Unicode property (`22.2.2.9`): `ASCII`,
/// `ASCII_Hex_Digit`, `Alphabetic`, `Any`, `Assigned`, `Bidi_Control`,
/// `Bidi_Mirrored`, `Case_Ignorable`, `Cased`, the six `Changes_When_*`
/// properties, `Dash`, `Default_Ignorable_Code_Point`, `Deprecated`,
/// `Diacritic`, the six `Emoji*` properties, `Extended_Pictographic`,
/// `Extender`, `Grapheme_Base`, `Grapheme_Extend`, `Hex_Digit`,
/// `IDS_Binary_Operator`, `IDS_Trinary_Operator`, `ID_Continue`, `ID_Start`,
/// `Ideographic`, `Join_Control`, `Logical_Order_Exception`, `Lowercase`,
/// `Math`, `Noncharacter_Code_Point`, `Pattern_Syntax`,
/// `Pattern_White_Space`, `Quotation_Mark`, `Radical`, `Regional_Indicator`,
/// `Sentence_Terminal`, `Soft_Dotted`, `Terminal_Punctuation`,
/// `Unified_Ideograph`, `Uppercase`, `Variation_Selector`, `White_Space`,
/// `XID_Continue`, and `XID_Start`, plus every property alias
/// (`PropertyAliases.txt`).
pub(super) fn binary_property_ranges(name: &str) -> Option<Vec<(u32, u32)>> {
    binary_property_intervals(name).map(<[_]>::to_vec)
}

/// A bare `\p{Name}`/`\P{Name}`: a recognized binary property, or (per
/// ECMA-262) a `General_Category` value/group name used without the
/// `General_Category=`/`gc=` prefix.
pub(super) fn lone_property_ranges(name: &str) -> Option<Vec<(u32, u32)>> {
    binary_property_ranges(name).or_else(|| category_ranges(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complement_and_scalar_clip_exclude_surrogates() {
        let all = negate(&[]);
        assert_eq!(all, vec![(0x0, 0x10_FFFF)]);
        let clipped = clip_to_scalars(all);
        assert_eq!(clipped, vec![(0x0, 0xD7FF), (0xE000, 0x10_FFFF)]);
    }
    #[test]
    fn every_script_name_has_a_script_extensions_entry() {
        for name in script_names() {
            assert!(
                script_extension_ranges(name).is_some(),
                "{name} has no Script_Extensions entry"
            );
        }
    }
    #[test]
    fn script_and_alias_agree() {
        let by_long = script_ranges("Greek").unwrap();
        let by_short = script_ranges("Grek").unwrap();
        assert_eq!(by_long, by_short);
        assert!(by_long.contains(&(0x370, 0x373)));
    }
    #[test]
    fn script_extensions_are_a_superset_of_script() {
        let script = script_ranges("Greek").unwrap();
        let scx = script_extension_ranges("Greek").unwrap();
        let missing: Vec<_> = complement_within(&scx, &script);
        assert!(
            missing.is_empty(),
            "Script(Greek) - Scx(Greek) = {missing:?}"
        );
    }
    #[test]
    fn binary_property_aliases_agree() {
        assert_eq!(
            binary_property_ranges("Alphabetic"),
            binary_property_ranges("Alpha")
        );
        assert!(
            binary_property_ranges("ASCII")
                .unwrap()
                .contains(&(0x0, 0x7F))
        );
        assert_eq!(binary_property_ranges("Any"), Some(vec![(0x0, 0x10_FFFF)]));
    }
    #[test]
    fn category_group_lc_is_the_three_cased_letter_categories() {
        let lc = category_ranges("LC").unwrap();
        let lu = category_ranges("Lu").unwrap();
        // Every `Lu` codepoint is also `LC` (`LC` = `Lu` | `Ll` | `Lt`).
        assert_eq!(complement_within(&lc, &lu), [] as [(u32, u32); 0]);
        // `a` is `Ll` (lowercase letter), so it is `LC` but not `Lu`.
        let a = u32::from('a');
        assert!(lc.iter().any(|&(s, e)| s <= a && a <= e));
        assert!(!lu.iter().any(|&(s, e)| s <= a && a <= e));
    }
    #[test]
    fn unassigned_covers_a_known_noncharacter_gap() {
        let cn = category_ranges("Cn").unwrap();
        // U+0378 is an unassigned Greek-block codepoint in Unicode 17.
        assert!(cn.iter().any(|&(s, e)| s <= 0x378 && 0x378 <= e));
    }
}
