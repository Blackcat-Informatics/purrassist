// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Unicode property lookups for `\p{...}`/`\P{...}`: `General_Category`
//! (delegating to `crate::text`'s table), `Script`, `Script_Extensions`, and
//! every ECMA-262 binary property, from the generated tables in
//! `unicode_data.rs` (Unicode 17; inputs pinned in
//! `vendor/unicode/manifest.json`).
mod unicode_data;

use crate::contract::{Governor, Resource, ScratchVec, StopReason, saturating_size as size};
type Ranges = ScratchVec<(u32, u32)>;

/// The domain of values a Rust `char` can hold: every Unicode scalar value
/// except the surrogate range, which UTF-8 (and therefore `char`/`str`) can
/// never encode. Every character class this module or `parse.rs` builds is
/// clipped to this domain as the last step, so a negated class (`[^x]`,
/// `\D`, `\P{...}`) never includes a codepoint no `char` can ever equal.
pub(super) const SCALAR_DOMAIN: &[(u32, u32)] = &[(0x0, 0xD7FF), (0xE000, 0x10_FFFF)];

/// Copy immutable generated intervals into fallible owned storage.
pub(super) fn copy_ranges(
    ranges: &[(u32, u32)],
    governor: &mut Governor,
) -> Result<Ranges, StopReason> {
    governor.charge(Resource::Work, size(ranges.len()).saturating_add(1))?;
    let mut out = ScratchVec::new(governor);
    out.extend(ranges.iter().copied())?;
    Ok(out)
}

/// Fallible in-place heapsort: comparisons and swaps admit work individually.
fn sift(
    ranges: &mut [(u32, u32)],
    mut root: usize,
    end: usize,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    while root < end / 2 {
        governor.charge(Resource::Work, 1)?;
        let mut child = root * 2 + 1;
        if child + 1 < end {
            governor.charge(Resource::Work, 1)?;
            if ranges[child] < ranges[child + 1] {
                child += 1;
            }
        }
        governor.charge(Resource::Work, 1)?;
        if ranges[root] >= ranges[child] {
            break;
        }
        governor.charge(Resource::Work, 1)?;
        ranges.swap(root, child);
        root = child;
    }
    Ok(())
}

/// Sort and coalesce overlapping/adjacent intervals without another allocation.
pub(super) fn merge(mut ranges: Ranges, governor: &mut Governor) -> Result<Ranges, StopReason> {
    let len = ranges.len();
    for root in (0..len / 2).rev() {
        sift(ranges.as_mut_slice(), root, len, governor)?;
    }
    for end in (1..len).rev() {
        governor.charge(Resource::Work, 1)?;
        ranges.as_mut_slice().swap(0, end);
        sift(ranges.as_mut_slice(), 0, end, governor)?;
    }
    let mut unique = 0;
    for read in 0..len {
        governor.charge(Resource::Work, 1)?;
        let (start, end) = ranges[read];
        if unique > 0 && start <= ranges[unique - 1].1.saturating_add(1) {
            let last = &mut ranges.as_mut_slice()[unique - 1];
            last.1 = last.1.max(end);
        } else {
            ranges.as_mut_slice()[unique] = (start, end);
            unique += 1;
        }
    }
    while ranges.len() > unique {
        governor.charge(Resource::Work, 1)?;
        ranges.pop();
    }
    Ok(ranges)
}

/// `domain - ranges`, with sorted and merged inputs.
pub(super) fn complement_within(
    ranges: &[(u32, u32)],
    domain: &[(u32, u32)],
    governor: &mut Governor,
) -> Result<Ranges, StopReason> {
    let mut out = ScratchVec::new(governor);
    let mut iter = ranges.iter().copied().peekable();
    for &(dom_start, dom_end) in domain {
        governor.charge(Resource::Work, 1)?;
        let mut pos = dom_start;
        while pos <= dom_end {
            governor.charge(Resource::Work, 1)?;
            match iter.peek().copied() {
                Some((_start, end)) if end < pos => {
                    iter.next();
                }
                Some((start, _end)) if start > dom_end => break,
                Some((start, _end)) if start > pos => {
                    out.push((pos, start - 1))?;
                    pos = start;
                }
                Some((_start, end)) => {
                    pos = end.saturating_add(1);
                    if pos <= dom_end {
                        iter.next();
                    }
                }
                None => {
                    out.push((pos, dom_end))?;
                    pos = dom_end.saturating_add(1);
                }
            }
        }
    }
    merge(out, governor)
}

pub(super) fn negate(ranges: &[(u32, u32)], governor: &mut Governor) -> Result<Ranges, StopReason> {
    complement_within(ranges, &[(0, 0x10_FFFF)], governor)
}

/// Restrict canonical ranges to Unicode scalar values.
pub(super) fn clip_to_scalars(
    ranges: Ranges,
    governor: &mut Governor,
) -> Result<Ranges, StopReason> {
    let merged = merge(ranges, governor)?;
    let mut out = ScratchVec::new(governor);
    for &(dom_start, dom_end) in SCALAR_DOMAIN {
        governor.charge(Resource::Work, 1)?;
        for &(start, end) in &merged {
            governor.charge(Resource::Work, 1)?;
            let lo = start.max(dom_start);
            let hi = end.min(dom_end);
            if lo <= hi {
                out.push((lo, hi))?;
            }
        }
    }
    merge(out, governor)
}

/// Membership in a canonical interval set. Construction callers admit lookup
/// work; runtime matching retains its separate contract.
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

fn lookup_work(name: &str, entries: usize, governor: &mut Governor) -> Result<(), StopReason> {
    let comparisons = u64::from(entries.checked_ilog2().unwrap_or(0) + 2);
    let work = size(name.len())
        .checked_add(1)
        .and_then(|n| n.checked_mul(comparisons))
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    governor.charge(Resource::Work, work)
}

pub(super) fn category_ranges(
    name: &str,
    governor: &mut Governor,
) -> Result<Option<Ranges>, StopReason> {
    // The fixed match has fewer than 128 aliases; each comparison reads at
    // most the query bytes. The following table compares two-byte categories.
    let work = size(name.len())
        .checked_add(1)
        .and_then(|n| n.checked_mul(128))
        .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    governor.charge(Resource::Work, work)?;
    let Some(target) = category_alias(name) else {
        return Ok(None);
    };
    let mut out = ScratchVec::new(governor);
    let table = crate::text::general_categories();
    for &(start, end, actual) in table {
        governor.charge(Resource::Work, 16)?;
        if category_matches(target, actual) {
            out.push((start, end))?;
        }
    }
    if matches!(target, "C" | "Cn") {
        let mut assigned = ScratchVec::new(governor);
        for &(start, end, _) in table {
            governor.charge(Resource::Work, 1)?;
            assigned.push((start, end))?;
        }
        let assigned = merge(assigned, governor)?;
        let mut gaps = complement_within(&assigned, &[(0, 0x10_FFFF)], governor)?;
        governor.charge(Resource::Work, size(gaps.len()))?;
        out.append(&mut gaps)?;
    }
    Ok(Some(merge(out, governor)?))
}

fn script_alias(name: &str) -> Option<&'static str> {
    unicode_data::SCRIPT_ALIASES
        .binary_search_by_key(&name, |(alias, _)| alias)
        .ok()
        .map(|index| unicode_data::SCRIPT_ALIASES[index].1)
}

pub(super) fn script_ranges(
    name: &str,
    governor: &mut Governor,
) -> Result<Option<Ranges>, StopReason> {
    lookup_work(name, unicode_data::SCRIPT_ALIASES.len(), governor)?;
    let Some(canonical) = script_alias(name) else {
        return Ok(None);
    };
    let mut out = ScratchVec::new(governor);
    for &(start, end, script) in unicode_data::SCRIPTS {
        governor.charge(
            Resource::Work,
            size(script.len().min(canonical.len())).saturating_add(1),
        )?;
        if script == canonical {
            out.push((start, end))?;
        }
    }
    Ok(Some(out))
}

pub(super) fn script_extension_ranges(
    name: &str,
    governor: &mut Governor,
) -> Result<Option<Ranges>, StopReason> {
    lookup_work(name, unicode_data::SCRIPT_ALIASES.len(), governor)?;
    let Some(canonical) = script_alias(name) else {
        return Ok(None);
    };
    lookup_work(canonical, unicode_data::SCRIPT_EXTENSIONS.len(), governor)?;
    let Some(index) = unicode_data::SCRIPT_EXTENSIONS
        .binary_search_by_key(&canonical, |(script, _)| *script)
        .ok()
    else {
        return Ok(None);
    };
    Ok(Some(copy_ranges(
        unicode_data::SCRIPT_EXTENSIONS[index].1,
        governor,
    )?))
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

pub(super) fn binary_property_ranges(
    name: &str,
    governor: &mut Governor,
) -> Result<Option<Ranges>, StopReason> {
    lookup_work(name, unicode_data::BINARY_PROPERTY_ALIASES.len(), governor)?;
    let Some(canonical) = binary_property_alias(name) else {
        return Ok(None);
    };
    lookup_work(canonical, unicode_data::BINARY_PROPERTIES.len(), governor)?;
    let Some(index) = unicode_data::BINARY_PROPERTIES
        .binary_search_by_key(&canonical, |(prop, _)| *prop)
        .ok()
    else {
        return Ok(None);
    };
    Ok(Some(copy_ranges(
        unicode_data::BINARY_PROPERTIES[index].1,
        governor,
    )?))
}

pub(super) fn lone_property_ranges(
    name: &str,
    governor: &mut Governor,
) -> Result<Option<Ranges>, StopReason> {
    binary_property_ranges(name, governor)?.map_or_else(
        || category_ranges(name, governor),
        |ranges| Ok(Some(ranges)),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;
    #[test]
    fn complement_and_scalar_clip_exclude_surrogates() {
        let mut governor = Governor::new(Budget::default());
        let all = negate(&[], &mut governor).unwrap();
        assert_eq!(all.as_slice(), [(0, 0x10_FFFF)]);
        let clipped = clip_to_scalars(all, &mut governor).unwrap();
        assert_eq!(clipped.as_slice(), [(0, 0xD7FF), (0xE000, 0x10_FFFF)]);
    }
    #[test]
    fn every_script_name_has_a_script_extensions_entry() {
        let mut governor = Governor::new(Budget::default());
        for name in script_names() {
            assert!(
                script_extension_ranges(name, &mut governor)
                    .unwrap()
                    .is_some(),
                "{name} has no Script_Extensions entry"
            );
        }
    }
    #[test]
    fn script_and_alias_agree() {
        let mut governor = Governor::new(Budget::default());
        let by_long = script_ranges("Greek", &mut governor).unwrap().unwrap();
        let by_short = script_ranges("Grek", &mut governor).unwrap().unwrap();
        assert_eq!(by_long, by_short);
        assert!(by_long.contains(&(0x370, 0x373)));
    }
    #[test]
    fn script_extensions_are_a_superset_of_script() {
        let mut governor = Governor::new(Budget::default());
        let script = script_ranges("Greek", &mut governor).unwrap().unwrap();
        let scx = script_extension_ranges("Greek", &mut governor)
            .unwrap()
            .unwrap();
        let missing = complement_within(&scx, &script, &mut governor).unwrap();
        assert!(
            missing.is_empty(),
            "Script(Greek) - Scx(Greek) = {missing:?}"
        );
    }
    #[test]
    fn binary_property_aliases_agree() {
        let mut governor = Governor::new(Budget::default());
        assert_eq!(
            binary_property_ranges("Alphabetic", &mut governor).unwrap(),
            binary_property_ranges("Alpha", &mut governor).unwrap()
        );
        assert!(
            binary_property_ranges("ASCII", &mut governor)
                .unwrap()
                .unwrap()
                .contains(&(0, 0x7F))
        );
        assert_eq!(
            binary_property_ranges("Any", &mut governor)
                .unwrap()
                .unwrap()
                .as_slice(),
            [(0, 0x10_FFFF)]
        );
    }
    #[test]
    fn category_group_lc_is_the_three_cased_letter_categories() {
        let mut governor = Governor::new(Budget::default());
        let lc = category_ranges("LC", &mut governor).unwrap().unwrap();
        let lu = category_ranges("Lu", &mut governor).unwrap().unwrap();
        assert!(
            complement_within(&lc, &lu, &mut governor)
                .unwrap()
                .is_empty()
        );
        let a = u32::from('a');
        assert!(lc.iter().any(|&(s, e)| s <= a && a <= e));
        assert!(!lu.iter().any(|&(s, e)| s <= a && a <= e));
    }
    #[test]
    fn unassigned_covers_a_known_noncharacter_gap() {
        let mut governor = Governor::new(Budget::default());
        let cn = category_ranges("Cn", &mut governor).unwrap().unwrap();
        assert!(cn.iter().any(|&(s, e)| s <= 0x378 && 0x378 <= e));
    }
}
