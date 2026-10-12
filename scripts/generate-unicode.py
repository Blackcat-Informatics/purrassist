#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Generate Unicode 17 data tables; explicit fetch, checked against a frozen manifest.

Single source of truth for every Unicode-derived table `purrassist-core` uses:
normalization/case folding/General_Category (`crates/core/src/text/unicode_data.rs`),
UTS #39 confusables (same file), Script/Script_Extensions/ECMA-262 binary
properties (`crates/core/src/pattern/unicode_data.rs`), and the IDNA/UTS-46
mapping table plus Bidi_Class/Joining_Type for the idn-hostname format
validator's Bidi rule (RFC 5893) and CONTEXTJ rule (RFC 5892 Appendix A)
(`crates/core/src/text/idna_data.rs`). Every input is
pinned by URL + sha256 in `vendor/unicode/manifest.json`; every generated
artifact's digest is recorded in `vendor/unicode/generated.json` and checked by
`scripts/check-repository.py`.
"""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request
import subprocess

ROOT = Path(__file__).resolve().parent.parent
UCD = 'https://www.unicode.org/Public/17.0.0/ucd/'
# Starting with Unicode 17.0.0, UTS #39 security data is published alongside
# the UCD release instead of the separate, independently versioned
# Public/security/ tree (see https://www.unicode.org/Public/security/ReadMe.txt).
SECURITY = 'https://www.unicode.org/Public/17.0.0/security/'
EMOJI = 'https://www.unicode.org/Public/17.0.0/ucd/emoji/'
# Starting with Unicode 17.0.0, IDNA data moved from the independently
# versioned Public/idna/ tree into the per-version UCD-adjacent tree (see
# https://www.unicode.org/Public/idna/ReadMe.txt).
IDNA = 'https://www.unicode.org/Public/17.0.0/idna/'
FILES = {
    'UnicodeData.txt': UCD + 'UnicodeData.txt',
    'CaseFolding.txt': UCD + 'CaseFolding.txt',
    'DerivedNormalizationProps.txt': UCD + 'DerivedNormalizationProps.txt',
    'NormalizationTest.txt': UCD + 'NormalizationTest.txt',
    'confusables.txt': SECURITY + 'confusables.txt',
    'Scripts.txt': UCD + 'Scripts.txt',
    'ScriptExtensions.txt': UCD + 'ScriptExtensions.txt',
    'PropertyValueAliases.txt': UCD + 'PropertyValueAliases.txt',
    'PropertyAliases.txt': UCD + 'PropertyAliases.txt',
    'PropList.txt': UCD + 'PropList.txt',
    'DerivedCoreProperties.txt': UCD + 'DerivedCoreProperties.txt',
    'emoji-data.txt': EMOJI + 'emoji-data.txt',
    'IdnaMappingTable.txt': IDNA + 'IdnaMappingTable.txt',
    # RFC 5893 Bidi rule and RFC 5892 Appendix A CONTEXTJ rule (the
    # idn-hostname format validator); extracted-property files, easier to
    # parse than deriving Bidi_Class/Joining_Type from UnicodeData.txt /
    # ArabicShaping.txt by hand.
    'DerivedBidiClass.txt': UCD + 'extracted/DerivedBidiClass.txt',
    'DerivedJoiningType.txt': UCD + 'extracted/DerivedJoiningType.txt',
}
parser = argparse.ArgumentParser()
parser.add_argument('--fetch', action='store_true')
parser.add_argument('--initialize-manifest', action='store_true')
args = parser.parse_args()
cache = ROOT / '.cache/unicode'
cache.mkdir(parents=True, exist_ok=True)
manifest_path = ROOT / 'vendor/unicode/manifest.json'
expected = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
manifest = {}
for name, url in FILES.items():
    path = cache / name
    if args.fetch and not path.exists():
        with urllib.request.urlopen(url, timeout=60) as response:
            path.write_bytes(response.read())
    data = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if not args.initialize_manifest and expected.get(name, {}).get('sha256') != digest:
        raise SystemExit(f'{name}: missing or mismatched pinned digest')
    manifest[name] = {'url': url, 'sha256': digest}
if args.initialize_manifest:
    for name, entry in expected.items():
        if manifest.get(name) != entry:
            raise SystemExit(f'refusing to replace an existing pinned entry with a different one: {name}')
    manifest_path.write_text(json.dumps(manifest, indent=2) + '\n')


def rows(name):
    """Yield `(fields, comment)` for each non-blank, non-comment-only line."""
    for line in (cache / name).read_text().splitlines():
        text, _, comment = line.partition('#')
        text = text.strip()
        if not text:
            continue
        yield [part.strip() for part in text.split(';')], comment


def parse_ranges(name, wanted=None):
    """`CODE[..CODE] ; Property_Name ...` files -> {name: [(start, end), ...]}."""
    table = {}
    for fields, _ in rows(name):
        code, prop = fields[0], fields[1]
        if wanted is not None and prop not in wanted:
            continue
        bounds = code.split('..')
        start = int(bounds[0], 16)
        end = int(bounds[-1], 16)
        table.setdefault(prop, []).append((start, end))
    return {prop: merge(ranges) for prop, ranges in table.items()}


def merge(intervals):
    """Sort and coalesce overlapping/adjacent `(start, end)` pairs."""
    ordered = sorted(intervals)
    out = []
    for start, end in ordered:
        if out and start <= out[-1][1] + 1:
            if end > out[-1][1]:
                out[-1] = (out[-1][0], end)
        else:
            out.append((start, end))
    return out


def subtract(a, b):
    """`a - b` over sorted, merged interval lists."""
    out = []
    b_iter = iter(b)
    cur = next(b_iter, None)
    for start, end in a:
        pos = start
        while cur is not None and cur[1] < pos:
            cur = next(b_iter, None)
        while pos <= end:
            if cur is None or cur[0] > end:
                out.append((pos, end))
                pos = end + 1
            elif cur[0] > pos:
                out.append((pos, cur[0] - 1))
                pos = cur[0]
            else:
                pos = cur[1] + 1
                if pos <= end:
                    cur = next(b_iter, None)
    return merge(out)


exclusions = set()
for fields, _ in rows('DerivedNormalizationProps.txt'):
    if fields[1] != 'Full_Composition_Exclusion':
        continue
    bounds = fields[0].split('..')
    exclusions.update(range(int(bounds[0], 16), int(bounds[-1], 16) + 1))

decompositions = []
combining = []
compositions = []
categories = []
general_categories = []
bidi_mirrored_codes = []
range_start = None
for line in (cache / 'UnicodeData.txt').read_text().splitlines():
    fields = line.split(';')
    code = int(fields[0], 16)
    category = fields[2]
    mask = (1 if category[0] in 'LN' else 0) | (2 if category[0] == 'M' else 0) | (4 if category[0] in 'PZ' else 0)
    if fields[1].endswith(', First>'):
        range_start = (code, mask, category)
    elif fields[1].endswith(', Last>'):
        if range_start is None:
            raise SystemExit('range end without range start')
        start, prior, prior_category = range_start
        if prior != mask or category != prior_category:
            raise SystemExit('invalid category range')
        if mask:
            categories.append((start, code, mask))
        general_categories.append((start, code, category))
        if fields[9] == 'Y':
            bidi_mirrored_codes.append((start, code))
        range_start = None
    else:
        if mask:
            categories.append((code, code, mask))
        general_categories.append((code, code, category))
        if fields[9] == 'Y':
            bidi_mirrored_codes.append((code, code))
    if int(fields[3]):
        combining.append((code, int(fields[3])))
    if fields[5]:
        mapping = fields[5].split()
        compatibility = mapping[0].startswith('<')
        if compatibility:
            mapping = mapping[1:]
        points = [int(point, 16) for point in mapping]
        decompositions.append((code, compatibility, points))
        if not compatibility and len(points) == 2 and code not in exclusions:
            compositions.append((points[0], points[1], code))
merged = []
for start, end, mask in categories:
    if merged and merged[-1][1] + 1 == start and merged[-1][2] == mask:
        merged[-1] = (merged[-1][0], end, mask)
    else:
        merged.append((start, end, mask))
general_merged = []
for start, end, category in general_categories:
    if general_merged and general_merged[-1][1] + 1 == start and general_merged[-1][2] == category:
        general_merged[-1] = (general_merged[-1][0], end, category)
    else:
        general_merged.append((start, end, category))
assigned_ranges = merge([(start, end) for start, end, _ in general_merged])
bidi_mirrored_ranges = merge(bidi_mirrored_codes)

folds = {}
for line in (cache / 'CaseFolding.txt').read_text().splitlines():
    text = line.split('#', 1)[0].strip()
    if not text:
        continue
    code, status, mapping, *_ = map(str.strip, text.split(';'))
    if status in {'C', 'F'}:
        folds[int(code, 16)] = [int(point, 16) for point in mapping.split()]

# UTS #39 confusables: each source scalar maps to its confusable prototype
# (one or more target scalars). The published table is already the
# transitive closure, so a single substitution pass suffices; skeleton()
# applies it once and re-normalizes (UTS #39 section 4).
confusables = []
seen_sources = set()
for line in (cache / 'confusables.txt').read_text().splitlines():
    text = line.split('#', 1)[0].strip()
    if not text:
        continue
    parts = [part.strip() for part in text.split(';')]
    if len(parts) < 2:
        continue
    source = int(parts[0], 16)
    targets = [int(point, 16) for point in parts[1].split()]
    if source in seen_sources:
        raise SystemExit(f'confusables.txt: duplicate source scalar {source:04X}')
    seen_sources.add(source)
    confusables.append((source, targets))
confusables.sort()

# --- Script / Script_Extensions (UAX #24) -------------------------------
script_ranges = parse_ranges('Scripts.txt')
# `Scripts.txt` gives every codepoint's Script value by an explicit range
# *except* `Unknown`, its `@missing` default for every codepoint no range
# claims (see the file's own header) -- exactly like `General_Category`'s
# `Cn` (see the `assigned_ranges`/gap handling above). Unlike `Cn`, ECMA-262
# property escapes (and this generator's own test262 fixtures) query
# `Script=Unknown` directly, so it needs a real, explicit range list too.
_assigned_script_ranges = merge([r for ranges in script_ranges.values() for r in ranges])
script_ranges['Unknown'] = subtract([(0x0, 0x10FFFF)], _assigned_script_ranges)
script_names = sorted(script_ranges)
script_aliases = {}
for fields, _ in rows('PropertyValueAliases.txt'):
    if fields[0] != 'sc':
        continue
    canonical = fields[2]
    for alias in (fields[1], fields[2], *fields[3:]):
        script_aliases[alias] = canonical
for name in script_names:
    script_aliases.setdefault(name, name)

scx_entries = []
for fields, _ in rows('ScriptExtensions.txt'):
    bounds = fields[0].split('..')
    start, end = int(bounds[0], 16), int(bounds[-1], 16)
    names = [script_aliases[code] for code in fields[1].split()]
    scx_entries.append((start, end, names))
scx_domain = merge([(start, end) for start, end, _ in scx_entries])

scx_ranges = {}
for name in script_names:
    default_part = subtract(script_ranges.get(name, []), scx_domain)
    explicit_part = merge([(start, end) for start, end, names in scx_entries if name in names])
    scx_ranges[name] = merge(default_part + explicit_part)

scripts_flat = []
for name, ranges in script_ranges.items():
    for start, end in ranges:
        scripts_flat.append((start, end, name))
scripts_flat.sort()

# --- ECMA-262 binary Unicode properties ---------------------------------
# The exact set from ECMA-262 (2025) table "Binary Unicode property aliases
# and their canonical property names" (22.2.2.9), excluding ASCII/Any/
# Assigned, which are computed rather than sourced from a single UCD file.
BINARY_SOURCES = {
    'ASCII_Hex_Digit': ('PropList.txt', 'ASCII_Hex_Digit'),
    'Alphabetic': ('DerivedCoreProperties.txt', 'Alphabetic'),
    'Bidi_Control': ('PropList.txt', 'Bidi_Control'),
    'Case_Ignorable': ('DerivedCoreProperties.txt', 'Case_Ignorable'),
    'Cased': ('DerivedCoreProperties.txt', 'Cased'),
    'Changes_When_Casefolded': ('DerivedCoreProperties.txt', 'Changes_When_Casefolded'),
    'Changes_When_Casemapped': ('DerivedCoreProperties.txt', 'Changes_When_Casemapped'),
    'Changes_When_Lowercased': ('DerivedCoreProperties.txt', 'Changes_When_Lowercased'),
    'Changes_When_NFKC_Casefolded': ('DerivedNormalizationProps.txt', 'Changes_When_NFKC_Casefolded'),
    'Changes_When_Titlecased': ('DerivedCoreProperties.txt', 'Changes_When_Titlecased'),
    'Changes_When_Uppercased': ('DerivedCoreProperties.txt', 'Changes_When_Uppercased'),
    'Dash': ('PropList.txt', 'Dash'),
    'Default_Ignorable_Code_Point': ('DerivedCoreProperties.txt', 'Default_Ignorable_Code_Point'),
    'Deprecated': ('PropList.txt', 'Deprecated'),
    'Diacritic': ('PropList.txt', 'Diacritic'),
    'Emoji': ('emoji-data.txt', 'Emoji'),
    'Emoji_Component': ('emoji-data.txt', 'Emoji_Component'),
    'Emoji_Modifier': ('emoji-data.txt', 'Emoji_Modifier'),
    'Emoji_Modifier_Base': ('emoji-data.txt', 'Emoji_Modifier_Base'),
    'Emoji_Presentation': ('emoji-data.txt', 'Emoji_Presentation'),
    'Extended_Pictographic': ('emoji-data.txt', 'Extended_Pictographic'),
    'Extender': ('PropList.txt', 'Extender'),
    'Grapheme_Base': ('DerivedCoreProperties.txt', 'Grapheme_Base'),
    'Grapheme_Extend': ('DerivedCoreProperties.txt', 'Grapheme_Extend'),
    'Hex_Digit': ('PropList.txt', 'Hex_Digit'),
    'IDS_Binary_Operator': ('PropList.txt', 'IDS_Binary_Operator'),
    'IDS_Trinary_Operator': ('PropList.txt', 'IDS_Trinary_Operator'),
    'ID_Continue': ('DerivedCoreProperties.txt', 'ID_Continue'),
    'ID_Start': ('DerivedCoreProperties.txt', 'ID_Start'),
    'Ideographic': ('PropList.txt', 'Ideographic'),
    'Join_Control': ('PropList.txt', 'Join_Control'),
    'Logical_Order_Exception': ('PropList.txt', 'Logical_Order_Exception'),
    'Lowercase': ('DerivedCoreProperties.txt', 'Lowercase'),
    'Math': ('DerivedCoreProperties.txt', 'Math'),
    'Noncharacter_Code_Point': ('PropList.txt', 'Noncharacter_Code_Point'),
    'Pattern_Syntax': ('PropList.txt', 'Pattern_Syntax'),
    'Pattern_White_Space': ('PropList.txt', 'Pattern_White_Space'),
    'Quotation_Mark': ('PropList.txt', 'Quotation_Mark'),
    'Radical': ('PropList.txt', 'Radical'),
    'Regional_Indicator': ('PropList.txt', 'Regional_Indicator'),
    'Sentence_Terminal': ('PropList.txt', 'Sentence_Terminal'),
    'Soft_Dotted': ('PropList.txt', 'Soft_Dotted'),
    'Terminal_Punctuation': ('PropList.txt', 'Terminal_Punctuation'),
    'Unified_Ideograph': ('PropList.txt', 'Unified_Ideograph'),
    'Uppercase': ('DerivedCoreProperties.txt', 'Uppercase'),
    'Variation_Selector': ('PropList.txt', 'Variation_Selector'),
    'White_Space': ('PropList.txt', 'White_Space'),
    'XID_Continue': ('DerivedCoreProperties.txt', 'XID_Continue'),
    'XID_Start': ('DerivedCoreProperties.txt', 'XID_Start'),
}
by_file = {}
for prop, (file_name, source_name) in BINARY_SOURCES.items():
    by_file.setdefault(file_name, set()).add(source_name)
parsed_by_file = {file_name: parse_ranges(file_name, wanted) for file_name, wanted in by_file.items()}
binary_properties = {
    prop: parsed_by_file[file_name].get(source_name, [])
    for prop, (file_name, source_name) in BINARY_SOURCES.items()
}
binary_properties['ASCII'] = [(0x0, 0x7F)]
binary_properties['Any'] = [(0x0, 0x10FFFF)]
binary_properties['Assigned'] = assigned_ranges
binary_properties['Bidi_Mirrored'] = bidi_mirrored_ranges
if set(binary_properties) - {'ASCII', 'Any', 'Assigned', 'Bidi_Mirrored'} != set(BINARY_SOURCES) - {'Bidi_Mirrored'}:
    raise SystemExit('binary property source table is inconsistent')
for prop, (file_name, source_name) in BINARY_SOURCES.items():
    if prop == 'Bidi_Mirrored':
        continue
    if not binary_properties[prop]:
        raise SystemExit(f'{prop}: no ranges found in {file_name} for {source_name}')

binary_aliases = {}
for fields, _ in rows('PropertyAliases.txt'):
    if len(fields) < 2:
        continue
    short, long = fields[0], fields[1]
    if long in binary_properties:
        binary_aliases[short] = long
for name in binary_properties:
    binary_aliases.setdefault(name, name)

# --- IDNA / UTS #46 mapping table (consumed by the hostname validators) -------------------
idna_rows = []
for fields, _ in rows('IdnaMappingTable.txt'):
    bounds = fields[0].split('..')
    start, end = int(bounds[0], 16), int(bounds[-1], 16)
    status = fields[1]
    mapping = [int(point, 16) for point in fields[2].split()] if len(fields) > 2 and fields[2] else []
    idna_rows.append((start, end, status, mapping))

# --- Bidi_Class (RFC 5893 Bidi rule) and Joining_Type (RFC 5892 Appendix A
# CONTEXTJ rule), consumed by the idn-hostname format validator. Both
# extracted-property files list only assigned exceptions; unlisted code
# points take their value from one or more `@missing` lines (general default
# first, more specific overrides later in file order) -- expand those into
# one explicit, gap-free partition of the whole scalar range so the runtime
# table needs no default-value fallback logic.


# `@missing` default lines spell values by their long name; explicit data
# rows in the same files spell the same values by their short alias
# (`PropertyValueAliases.txt` ties the two together for every property this
# generator reads, but wiring that lookup for just two default lines is not
# worth it: both files' long-name defaults are enumerated here once).
MISSING_VALUE_ALIASES = {
    'Left_To_Right': 'L', 'Right_To_Left': 'R', 'Arabic_Letter': 'AL',
    'European_Terminator': 'ET', 'Non_Joining': 'U',
}


def expand_with_missing_defaults(name):
    """`(fields, comment)` rows plus `# @missing: RANGE; VALUE` defaults ->
    a sorted, gap-free `[(start, end, value), ...]` covering 0..10FFFF."""
    defaults = []
    for line in (cache / name).read_text().splitlines():
        if not line.startswith('# @missing:'):
            continue
        spec = line[len('# @missing:'):].strip()
        code, value = (part.strip() for part in spec.split(';'))
        value = MISSING_VALUE_ALIASES.get(value, value)
        bounds = code.split('..')
        defaults.append((int(bounds[0], 16), int(bounds[-1], 16), value))
    table = {}
    for start, end, value in defaults:
        for code in range(start, end + 1):
            table[code] = value
    for fields, _ in rows(name):
        bounds = fields[0].split('..')
        start, end = int(bounds[0], 16), int(bounds[-1], 16)
        for code in range(start, end + 1):
            table[code] = fields[1]
    ranges = []
    for code in sorted(table):
        value = table[code]
        if ranges and ranges[-1][1] + 1 == code and ranges[-1][2] == value:
            ranges[-1] = (ranges[-1][0], code, value)
        else:
            ranges.append((code, code, value))
    return ranges


bidi_class_ranges = expand_with_missing_defaults('DerivedBidiClass.txt')
joining_type_ranges = expand_with_missing_defaults('DerivedJoiningType.txt')

out = ['// SPDX-FileCopyrightText: 2025 Unicode, Inc.',
       '// SPDX-License-Identifier: Unicode-3.0',
       '// Generated by scripts/generate-unicode.py; inputs pinned in vendor/unicode/manifest.json.',
       '#![allow(clippy::unreadable_literal, reason = "generated Unicode code point tables")]',
       'pub(super) const DECOMPOSITION: &[(u32, bool, &[u32])] = &[']
for code, compatibility, points in decompositions:
    out.append(f'    (0x{code:x}, {str(compatibility).lower()}, &[{", ".join(f"0x{p:x}" for p in points)}]),')
out += ['];', 'pub(super) const COMBINING: &[(u32, u8)] = &[']
out += [f'    (0x{code:x}, {value}),' for code, value in combining]
out += ['];', 'pub(super) const COMPOSITION: &[(u32, u32, u32)] = &[']
out += [f'    (0x{a:x}, 0x{b:x}, 0x{c:x}),' for a, b, c in sorted(compositions)]
out += ['];', 'pub(super) const CASE_FOLD: &[(u32, &[u32])] = &[']
for code, points in sorted(folds.items()):
    out.append(f'    (0x{code:x}, &[{", ".join(f"0x{p:x}" for p in points)}]),')
out += ['];', 'pub(super) const CATEGORIES: &[(u32, u32, u8)] = &[']
out += [f'    (0x{start:x}, 0x{end:x}, {mask}),' for start, end, mask in merged]
out += ['];', 'pub(super) const GENERAL_CATEGORIES: &[(u32, u32, &str)] = &[']
out += [f'    (0x{start:x}, 0x{end:x}, "{category}"),' for start, end, category in general_merged]
out += ['];', 'pub(super) const CONFUSABLES: &[(u32, &[u32])] = &[']
out += [f'    (0x{source:x}, &[{", ".join(f"0x{p:x}" for p in targets)}]),' for source, targets in confusables]
out += ['];', '']
text_output = ROOT / 'crates/core/src/text/unicode_data.rs'
text_output.write_text('\n'.join(out))

pattern_out = ['// SPDX-FileCopyrightText: 2025 Unicode, Inc.',
               '// SPDX-License-Identifier: Unicode-3.0',
               '// Generated by scripts/generate-unicode.py; inputs pinned in vendor/unicode/manifest.json.',
               '#![allow(clippy::unreadable_literal, reason = "generated Unicode code point tables")]',
               'pub(super) const SCRIPTS: &[(u32, u32, &str)] = &[']
pattern_out += [f'    (0x{start:x}, 0x{end:x}, "{name}"),' for start, end, name in scripts_flat]
pattern_out += ['];', 'pub(super) const SCRIPT_ALIASES: &[(&str, &str)] = &[']
pattern_out += [f'    ("{alias}", "{canonical}"),' for alias, canonical in sorted(script_aliases.items())]
pattern_out += ['];', 'pub(super) const SCRIPT_EXTENSIONS: &[(&str, &[(u32, u32)])] = &[']
for name in script_names:
    ranges = ', '.join(f'(0x{start:x}, 0x{end:x})' for start, end in scx_ranges[name])
    pattern_out.append(f'    ("{name}", &[{ranges}]),')
pattern_out += ['];', 'pub(super) const BINARY_PROPERTIES: &[(&str, &[(u32, u32)])] = &[']
for name in sorted(binary_properties):
    ranges = ', '.join(f'(0x{start:x}, 0x{end:x})' for start, end in binary_properties[name])
    pattern_out.append(f'    ("{name}", &[{ranges}]),')
pattern_out += ['];', 'pub(super) const BINARY_PROPERTY_ALIASES: &[(&str, &str)] = &[']
pattern_out += [f'    ("{alias}", "{canonical}"),' for alias, canonical in sorted(binary_aliases.items())]
pattern_out += ['];', '']
pattern_output = ROOT / 'crates/core/src/pattern/unicode/unicode_data.rs'
pattern_output.write_text('\n'.join(pattern_out))

idna_out = ['// SPDX-FileCopyrightText: 2025 Unicode, Inc.',
            '// SPDX-License-Identifier: Unicode-3.0',
            '// Generated by scripts/generate-unicode.py; inputs pinned in vendor/unicode/manifest.json.',
            '// Consumed by the hostname/idn-hostname format validators (IDNA/UTS-46).',
            '#![allow(clippy::unreadable_literal, reason = "generated Unicode code point tables")]',
            '#![allow(dead_code, reason = "consumed by the hostname/idn-hostname format validators")]',
            'pub(super) const IDNA_MAPPING: &[(u32, u32, &str, &[u32])] = &[']
for start, end, status, mapping in idna_rows:
    points = ', '.join(f'0x{p:x}' for p in mapping)
    idna_out.append(f'    (0x{start:x}, 0x{end:x}, "{status}", &[{points}]),')
idna_out += ['];', 'pub(super) const BIDI_CLASS: &[(u32, u32, &str)] = &[']
idna_out += [f'    (0x{start:x}, 0x{end:x}, "{value}"),' for start, end, value in bidi_class_ranges]
idna_out += ['];', 'pub(super) const JOINING_TYPE: &[(u32, u32, &str)] = &[']
idna_out += [f'    (0x{start:x}, 0x{end:x}, "{value}"),' for start, end, value in joining_type_ranges]
idna_out += ['];', '']
idna_output = ROOT / 'crates/core/src/text/idna_data.rs'
idna_output.write_text('\n'.join(idna_out))

for path in (text_output, pattern_output, idna_output):
    subprocess.run(['rustfmt', '--edition', '2024', str(path)], check=True, cwd=ROOT)
(ROOT / 'vendor/unicode/generated.json').write_text(json.dumps({
    str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
    for path in (text_output, pattern_output, idna_output)
}, indent=2) + '\n')
print(
    f'generated {len(decompositions)} decompositions, {len(compositions)} compositions, '
    f'{len(folds)} folds, {len(confusables)} confusables, {len(script_names)} scripts, '
    f'{len(binary_properties)} binary properties, {len(idna_rows)} IDNA mapping rows, '
    f'{len(bidi_class_ranges)} Bidi_Class ranges, {len(joining_type_ranges)} Joining_Type ranges'
)
