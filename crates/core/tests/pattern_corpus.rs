// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Pattern engine corpus suites (`make corpus`): the vendored test262
//! `RegExp` `property-escapes/generated` census and fixtures, and the
//! derivative-vs-backtracking differential over ten thousand generated
//! pattern/input pairs. `pattern.rs` holds the focused engine tests that
//! `make check` runs.
use purrassist_core::pattern::{Engine, Pattern};
use purrassist_core::{Budget, Governor, StopReason};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn test262_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/test262/generated")
}

/// The property a test262 `property-escapes/generated/<name>.js` file
/// exercises, derived from its filename (`General_Category_-_X.js`,
/// `Script_-_X.js`, `Script_Extensions_-_X.js`, or `X.js` for a binary
/// property), and the `\p{...}` expression(s) that name it.
enum Property {
    Binary(String),
    GeneralCategory(String),
    Script(String),
    ScriptExtensions(String),
}
impl Property {
    // A four-way filename-prefix dispatch reads far more clearly as
    // `if`/`else if` than as nested `Option::map_or_else` closures.
    #[allow(clippy::option_if_let_else)]
    fn from_filename(stem: &str) -> Self {
        if let Some(value) = stem.strip_prefix("General_Category_-_") {
            Self::GeneralCategory(value.to_string())
        } else if let Some(value) = stem.strip_prefix("Script_Extensions_-_") {
            Self::ScriptExtensions(value.to_string())
        } else if let Some(value) = stem.strip_prefix("Script_-_") {
            Self::Script(value.to_string())
        } else {
            Self::Binary(stem.to_string())
        }
    }
    fn expression(&self) -> String {
        match self {
            Self::Binary(name) => name.clone(),
            Self::GeneralCategory(value) => format!("General_Category={value}"),
            Self::Script(value) => format!("Script={value}"),
            Self::ScriptExtensions(value) => format!("Script_Extensions={value}"),
        }
    }
}

/// The exact ECMA-262 (2025) binary Unicode property list this engine
/// supports (`22.2.2.9`), by canonical name (not alias): `ASCII`, `Any`, and
/// `Assigned` are computed rather than sourced from one Unicode data file;
/// every other name is sourced by `scripts/generate-unicode.py`. Used only to
/// assert the test262 census is exactly this set, not to compile patterns
/// (which exercises the parser's own alias resolution instead).
const BINARY_PROPERTIES: &[&str] = &[
    "ASCII",
    "ASCII_Hex_Digit",
    "Alphabetic",
    "Any",
    "Assigned",
    "Bidi_Control",
    "Bidi_Mirrored",
    "Case_Ignorable",
    "Cased",
    "Changes_When_Casefolded",
    "Changes_When_Casemapped",
    "Changes_When_Lowercased",
    "Changes_When_NFKC_Casefolded",
    "Changes_When_Titlecased",
    "Changes_When_Uppercased",
    "Dash",
    "Default_Ignorable_Code_Point",
    "Deprecated",
    "Diacritic",
    "Emoji",
    "Emoji_Component",
    "Emoji_Modifier",
    "Emoji_Modifier_Base",
    "Emoji_Presentation",
    "Extended_Pictographic",
    "Extender",
    "Grapheme_Base",
    "Grapheme_Extend",
    "Hex_Digit",
    "IDS_Binary_Operator",
    "IDS_Trinary_Operator",
    "ID_Continue",
    "ID_Start",
    "Ideographic",
    "Join_Control",
    "Logical_Order_Exception",
    "Lowercase",
    "Math",
    "Noncharacter_Code_Point",
    "Pattern_Syntax",
    "Pattern_White_Space",
    "Quotation_Mark",
    "Radical",
    "Regional_Indicator",
    "Sentence_Terminal",
    "Soft_Dotted",
    "Terminal_Punctuation",
    "Unified_Ideograph",
    "Uppercase",
    "Variation_Selector",
    "White_Space",
    "XID_Continue",
    "XID_Start",
];

/// Every `General_Category` value name this engine's `category_alias`
/// recognizes as a long-form value (the same set test262 generates a file
/// for), including the derived `Cased_Letter` grouping.
const GENERAL_CATEGORY_VALUES: &[&str] = &[
    "Cased_Letter",
    "Close_Punctuation",
    "Connector_Punctuation",
    "Control",
    "Currency_Symbol",
    "Dash_Punctuation",
    "Decimal_Number",
    "Enclosing_Mark",
    "Final_Punctuation",
    "Format",
    "Initial_Punctuation",
    "Letter",
    "Letter_Number",
    "Line_Separator",
    "Lowercase_Letter",
    "Mark",
    "Math_Symbol",
    "Modifier_Letter",
    "Modifier_Symbol",
    "Nonspacing_Mark",
    "Number",
    "Open_Punctuation",
    "Other",
    "Other_Letter",
    "Other_Number",
    "Other_Punctuation",
    "Other_Symbol",
    "Paragraph_Separator",
    "Private_Use",
    "Punctuation",
    "Separator",
    "Space_Separator",
    "Spacing_Mark",
    "Surrogate",
    "Symbol",
    "Titlecase_Letter",
    "Unassigned",
    "Uppercase_Letter",
];

struct Extracted {
    lone: Vec<u32>,
    ranges: Vec<(u32, u32)>,
}
impl Extracted {
    /// Every codepoint this `buildString` argument names, filtering out the
    /// UTF-16 surrogate range: JavaScript strings are UTF-16 and can hold a
    /// lone surrogate, but a Rust `char`/`str` (UTF-8, scalar values only)
    /// never can, so no text this engine matches against could ever contain
    /// one. `General_Category_-_Surrogate.js`'s `matchSymbols` is entirely
    /// surrogates and becomes empty after this filter (its `nonMatchSymbols`
    /// is not, and is still exercised).
    fn scalars(&self) -> Vec<char> {
        let mut out = Vec::new();
        for &point in &self.lone {
            if let Some(c) = char::from_u32(point) {
                out.push(c);
            }
        }
        for &(start, end) in &self.ranges {
            for point in start..=end {
                if let Some(c) = char::from_u32(point) {
                    out.push(c);
                }
            }
        }
        out
    }
}

/// Extract the `loneCodePoints`/`ranges` array literals from one
/// `const {name} = buildString({...});` call in a vendored test262 fixture.
/// The generator (`harness/regExpUtils.js`) always emits exactly this shape
/// (see `docs/DEPENDENCIES.md`), so a small hand-rolled scan suffices; no
/// regex or JS-parser dependency is needed for this one fixed structure.
fn extract(source: &str, name: &str) -> Option<Extracted> {
    let marker = format!("const {name} = buildString({{");
    let start = source.find(&marker)? + marker.len();
    let end = start + source[start..].find("});")?;
    let body = &source[start..end];
    let lone = extract_array(body, "loneCodePoints:")
        .map(parse_hex_list)
        .unwrap_or_default();
    let ranges = extract_array(body, "ranges:")
        .map(parse_ranges)
        .unwrap_or_default();
    Some(Extracted { lone, ranges })
}
fn extract_array<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let after_key = body[body.find(key)? + key.len()..].trim_start();
    let after_bracket = after_key.strip_prefix('[')?;
    let mut depth = 1usize;
    for (index, character) in after_bracket.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&after_bracket[..index]);
                }
            }
            _ => {}
        }
    }
    None
}
fn parse_hex_list(text: &str) -> Vec<u32> {
    text.split(',')
        .filter_map(|token| {
            let token = token.trim().strip_prefix("0x")?;
            u32::from_str_radix(token, 16).ok()
        })
        .collect()
}
fn parse_ranges(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']') else {
            break;
        };
        let pair = &rest[open + 1..open + close];
        let values = parse_hex_list(pair);
        if let [start, end] = values[..] {
            out.push((start, end));
        }
        rest = &rest[open + close + 1..];
    }
    out
}

fn census() -> Vec<(String, Property)> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(test262_dir()).expect("vendored test262 fixtures present") {
        let entry = entry.expect("readable directory entry");
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "js") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 filename")
            .to_string();
        entries.push((stem.clone(), Property::from_filename(&stem)));
    }
    entries
}

#[test]
fn test262_property_escapes_census_is_complete_and_bidirectional() {
    let entries = census();
    assert_eq!(
        entries.len(),
        441,
        "expected exactly the vendored file count"
    );

    let mut binary_seen = BTreeSet::new();
    let mut gc_seen = BTreeSet::new();
    let mut script_seen = BTreeSet::new();
    let mut scx_seen = BTreeSet::new();
    for (_, property) in &entries {
        match property {
            Property::Binary(name) => binary_seen.insert(name.clone()),
            Property::GeneralCategory(value) => gc_seen.insert(value.clone()),
            Property::Script(value) => script_seen.insert(value.clone()),
            Property::ScriptExtensions(value) => scx_seen.insert(value.clone()),
        };
    }

    // Every supported binary property has a file, and every binary-property
    // file names a property this engine supports (bidirectional).
    let expected_binary: BTreeSet<String> = BINARY_PROPERTIES
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert_eq!(binary_seen, expected_binary);

    // Same, for General_Category values.
    let expected_gc: BTreeSet<String> = GENERAL_CATEGORY_VALUES
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    assert_eq!(gc_seen, expected_gc);

    // Script and Script_Extensions cover the same 175 Unicode 17 script
    // values (every script has both a `Script` and a `Script_Extensions`
    // file; this engine's Script_Extensions table is defined for exactly
    // the scripts its Script table has, see `pattern/unicode.rs`).
    assert_eq!(script_seen, scx_seen);
    assert_eq!(script_seen.len(), 175);

    // No file fell through the filename classifier into the wrong bucket.
    assert_eq!(
        binary_seen.len() + gc_seen.len() + script_seen.len() + scx_seen.len(),
        entries.len()
    );

    for value in &script_seen {
        assert!(
            Pattern::compile(&format!("\\p{{Script={value}}}")).is_ok(),
            "Script={value} must compile"
        );
        assert!(
            Pattern::compile(&format!("\\p{{Script_Extensions={value}}}")).is_ok(),
            "Script_Extensions={value} must compile"
        );
    }
}

#[test]
fn test262_property_escapes_generated_fixtures() {
    // Some properties' `matchSymbols`/`nonMatchSymbols` cover most of the
    // Unicode scalar range (over a million codepoints, e.g. `Assigned`'s
    // `nonMatchSymbols`); a fresh, generously (but still finitely) budgeted
    // governor per fixture keeps that a matter of the derivative engine's
    // genuinely linear cost, not the default Standard effort level (which
    // is sized for typical schema patterns, not million-codepoint strings).
    let mut exercised = 0usize;
    for (stem, property) in census() {
        let source = std::fs::read_to_string(test262_dir().join(format!("{stem}.js")))
            .unwrap_or_else(|error| panic!("{stem}.js: {error}"));
        let expression = property.expression();
        let positive = Pattern::compile(&format!("^\\p{{{expression}}}+$"))
            .unwrap_or_else(|error| panic!("\\p{{{expression}}}: {error}"));
        let negative = Pattern::compile(&format!("^\\P{{{expression}}}+$"))
            .unwrap_or_else(|error| panic!("\\P{{{expression}}}: {error}"));

        if let Some(matches) = extract(&source, "matchSymbols") {
            let text: String = matches.scalars().into_iter().collect();
            if !text.is_empty() {
                let mut governor = Governor::new(purrassist_core::Effort::Maximum.budget());
                assert_eq!(
                    positive.is_match(&text, &mut governor),
                    Ok(true),
                    "{stem}: \\p{{{expression}}} must match every matchSymbols codepoint"
                );
                exercised += 1;
            }
        }
        if let Some(non_matches) = extract(&source, "nonMatchSymbols") {
            let text: String = non_matches.scalars().into_iter().collect();
            if !text.is_empty() {
                let mut governor = Governor::new(purrassist_core::Effort::Maximum.budget());
                assert_eq!(
                    negative.is_match(&text, &mut governor),
                    Ok(true),
                    "{stem}: \\P{{{expression}}} must match every nonMatchSymbols codepoint"
                );
                exercised += 1;
            }
        }
    }
    // Every one of the 441 files contributes at least a matchSymbols
    // assertion; most also contribute a nonMatchSymbols one (all but the
    // handful, like General_Category_-_Surrogate.js, whose matchSymbols or
    // nonMatchSymbols set is entirely unrepresentable surrogates).
    assert!(exercised > 800, "exercised only {exercised} assertions");
}

/// A small, deterministic LCG (`xorshift`-free, textbook Lehmer generator);
/// no external `rand` dependency, and no reliance on OS randomness the core
/// forbids ambient access to.
struct Lcg(u64);
impl Lcg {
    const fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.0 >> 33
    }
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }
}

const ALPHABET: &[char] = &['a', 'b', 'c', '0', '1', ' '];

fn random_class(rng: &mut Lcg) -> String {
    match rng.below(6) {
        0 => "a".into(),
        1 => "[ab]".into(),
        2 => "[^a]".into(),
        3 => ".".into(),
        4 => "\\d".into(),
        _ => "\\w".into(),
    }
}
fn random_atom(rng: &mut Lcg, depth: u32) -> String {
    if depth == 0 || rng.below(4) != 0 {
        random_class(rng)
    } else {
        format!("(?:{})", random_pattern(rng, depth - 1))
    }
}
fn random_piece(rng: &mut Lcg, depth: u32) -> String {
    let atom = random_atom(rng, depth);
    match rng.below(5) {
        0 => format!("{atom}*"),
        1 => format!("{atom}+"),
        2 => format!("{atom}?"),
        3 => format!("{atom}{{1,3}}"),
        _ => atom,
    }
}
fn random_pattern(rng: &mut Lcg, depth: u32) -> String {
    let branches = 1 + rng.below(2);
    let mut alternatives = Vec::new();
    for _ in 0..branches {
        let pieces = 1 + rng.below(3);
        let mut sequence = String::new();
        for _ in 0..pieces {
            sequence.push_str(&random_piece(rng, depth));
        }
        alternatives.push(sequence);
    }
    alternatives.join("|")
}
fn random_input(rng: &mut Lcg) -> String {
    let length = rng.below(9);
    (0..length)
        .map(|_| ALPHABET[usize::try_from(rng.below(ALPHABET.len() as u64)).unwrap_or(0)])
        .collect()
}

#[test]
fn derivative_and_backtracking_engines_agree_on_ten_thousand_generated_pairs() {
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut checked = 0usize;
    while checked < 10_000 {
        let source = random_pattern(&mut rng, 2);
        let Ok(pattern) = Pattern::compile(&source) else {
            continue;
        };
        assert_eq!(
            pattern.engine(),
            Engine::Derivative,
            "generated pattern {source:?} has no backreference/lookaround and must use the derivative engine"
        );
        let input = random_input(&mut rng);
        let mut derivative_governor = Governor::new(Budget::default());
        // The backtracking evaluator is worst-case exponential in exactly
        // the ways the derivative engine (this test's whole point) is not;
        // a generated pattern with ambiguous nested quantifiers can need
        // far more than the default Standard budget to explore every
        // branch even on an 8-character input. A generous but still
        // bounded budget here tests engine *agreement*, not backtracking
        // performance (`pathological_nested_quantifier_completes_under_standard_budget`
        // is the dedicated test for the derivative engine's own bound).
        let mut backtrack_governor = Governor::new(Budget {
            work: 3_000_000,
            ..Budget::default()
        });
        let derivative = pattern.is_match(&input, &mut derivative_governor);
        let backtracking = pattern.is_match_backtracking(&input, &mut backtrack_governor);
        if matches!(backtracking, Err(StopReason::BudgetExceeded(_))) {
            // Both engines already agree the pattern is admissible (parsed,
            // routed to the derivative engine); a backtracking exhaustion
            // on a fuzz-generated worst case is not a disagreement to chase
            // here, just skip this pair and keep sampling.
            continue;
        }
        assert_eq!(
            derivative, backtracking,
            "pattern {source:?} on input {input:?}: derivative={derivative:?} backtracking={backtracking:?}"
        );
        checked += 1;
    }
}
