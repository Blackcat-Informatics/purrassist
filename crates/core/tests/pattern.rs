// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Pattern engine conformance: the vendored test262 `RegExp`
//! `property-escapes/generated` census, the derivative-vs-backtracking
//! differential fuzz, engine selection, and `generate`'s witness/emptiness
//! contract.
use purrassist_core::pattern::{Engine, GenerateOptions, GenerateOutcome, Pattern};

#[test]
fn anchored_derivative_transitions_distinguish_the_first_position() {
    for source in ["^a$", "(^a|b)$", "^a?$", "(^a)+$"] {
        let pattern = Pattern::compile(source).unwrap();
        for subject in ["a", "", "a a", "a\na", "a!a", "a\u{1e}a", "aaa", "ba"] {
            let mut derivative = Governor::new(Budget::default());
            let mut backtracking = Governor::new(Budget::default());
            assert_eq!(
                pattern.is_match(subject, &mut derivative),
                pattern.is_match_backtracking(subject, &mut backtracking),
                "{source:?} / {subject:?}"
            );
        }
    }
    assert_eq!(
        Pattern::compile("^a$")
            .unwrap()
            .is_match("a a", &mut Governor::new(Budget::default())),
        Ok(false)
    );
}
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

#[test]
fn pathological_nested_quantifier_completes_under_standard_budget() {
    let pattern = Pattern::compile("(a+)+b").expect("pattern");
    assert_eq!(pattern.engine(), Engine::Derivative);
    let text = "a".repeat(10_000);
    let mut governor = Governor::new(Budget::default());
    assert_eq!(pattern.is_match(&text, &mut governor), Ok(false));
}

#[test]
fn named_backreferences_resolve_and_duplicate_names_follow_the_ecma_rule() {
    // `\k<x>` resolves forward or backward to its named group's index.
    let mut governor = Governor::new(Budget::default());
    let forward_named = Pattern::compile(r"^(?<x>a)\k<x>$").expect("pattern");
    assert_eq!(forward_named.is_match("aa", &mut governor), Ok(true));
    assert_eq!(forward_named.is_match("ab", &mut governor), Ok(false));
    // A backreference to a group not yet (successfully) matched on this
    // attempt matches the empty string (ECMA-262 semantics), so this
    // pattern matches exactly one `a`, not two.
    let backward_named = Pattern::compile(r"^\k<x>(?<x>a)$").expect("pattern");
    assert_eq!(backward_named.is_match("a", &mut governor), Ok(true));
    assert_eq!(backward_named.is_match("aa", &mut governor), Ok(false));

    // A duplicate group name is valid only across mutually exclusive
    // branches of a shared alternation.
    assert!(Pattern::compile(r"(?<a>x)|(?<a>y)").is_ok());
    assert!(Pattern::compile(r"(?:(?<a>x)|(?<a>y))|(?<a>z)").is_ok());
    assert!(Pattern::compile(r"(?<a>x)(?<a>y)").is_err());
    assert!(Pattern::compile(r"(?:(?<a>x)(?<a>y))|z").is_err());

    // An unresolvable/nonexistent backreference is a compile-time error,
    // whether numeric (forward-referencing a group index that is never
    // defined) or named.
    assert!(Pattern::compile(r"\1").is_err());
    assert!(Pattern::compile(r"\k<missing>").is_err());

    // Duplicate names in the same alternative (not separated by a shared
    // disjunction) are early errors, exactly like the un-named case above.
    assert!(Pattern::compile(r"(?<a>x)(?<a>y)").is_err());
    assert!(Pattern::compile(r"(?:(?<a>x)(?<a>y))|z").is_err());
    assert!(Pattern::compile(r"((?<a>x))((?<a>y))").is_err());
}

#[test]
fn duplicate_named_group_backreference_resolves_to_the_participating_group() {
    // ECMA-262 (2025) duplicate named capturing groups: a backreference to
    // a name shared by groups in different (mutually exclusive)
    // alternatives resolves, at match time, to whichever one actually
    // participated — at most one can, since they are mutually exclusive.
    let mut governor = Governor::new(Budget::default());
    let pattern = Pattern::compile(r"^(?:(?<a>x)|(?<a>y))\k<a>$").expect("pattern");
    assert_eq!(pattern.engine(), Engine::Backtrack);
    assert_eq!(pattern.is_match("xx", &mut governor), Ok(true));
    assert_eq!(pattern.is_match("yy", &mut governor), Ok(true));
    assert_eq!(pattern.is_match("xy", &mut governor), Ok(false));
    assert_eq!(pattern.is_match("yx", &mut governor), Ok(false));

    // Neither alternative participates (the group is optional and did not
    // run): the backreference matches the empty string, per ECMA-262,
    // exactly like a numeric backreference to a group that never matched —
    // so this pattern matches only "" (group skipped, backref empty) or
    // "xx"/"yy" (group taken, backref repeats whichever one participated);
    // "x"/"y" alone are one character short (the group consumed it, and a
    // *participating* group's backreference is not empty), matching
    // ordinary same-index backreference behavior (`(a)\1` doesn't match
    // "a" either).
    let optional = Pattern::compile(r"^(?:(?<a>x)|(?<a>y))?\k<a>$").expect("pattern");
    assert_eq!(optional.is_match("", &mut governor), Ok(true));
    assert_eq!(optional.is_match("xx", &mut governor), Ok(true));
    assert_eq!(optional.is_match("yy", &mut governor), Ok(true));
    assert_eq!(optional.is_match("x", &mut governor), Ok(false));
    assert_eq!(optional.is_match("y", &mut governor), Ok(false));
    assert_eq!(optional.is_match("xy", &mut governor), Ok(false));

    // `generate`'s backtracking candidate path resolves the same way: a
    // witness for the duplicate-name pattern above must actually satisfy
    // the participating-group constraint it verifies with `is_match`.
    let outcome = pattern
        .generate(
            &mut governor,
            GenerateOptions {
                min_len: 0,
                max_len: Some(4),
                avoid: &[],
            },
        )
        .expect("governed search must complete");
    let GenerateOutcome::Witness(text) = outcome else {
        panic!("expected a witness for {pattern:?}");
    };
    assert!(text == "xx" || text == "yy", "unexpected witness {text:?}");
}

#[test]
fn backreference_patterns_route_to_backtracking() {
    for pattern in [r"(a)\1", r"(?<x>a)\k<x>", r"(?=a)a", r"(?<=a)a"] {
        let compiled = Pattern::compile(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"));
        assert_eq!(
            compiled.engine(),
            Engine::Backtrack,
            "{pattern} should route to backtracking"
        );
        assert_eq!(
            compiled.engine_evidence().rule,
            "pattern.engine.backtrack-v1"
        );
    }
    let plain = Pattern::compile("[a-z]+[0-9]*").unwrap();
    assert_eq!(plain.engine(), Engine::Derivative);
    assert_eq!(plain.engine_evidence().rule, "pattern.engine.derivative-v1");
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

#[test]
fn generate_witness_always_matches_and_never_matches_avoid() {
    let mut governor = Governor::new(Budget::default());
    let cases: &[(&str, &[&str])] = &[
        (r"^[a-c]{2,4}$", &["ab", "ac"]),
        (r"^\d{3}$", &["000", "999"]),
        (r"^[a-z]+@[a-z]+$", &[]),
    ];
    for (pattern_source, avoid_sources) in cases {
        let pattern = Pattern::compile(pattern_source).expect(pattern_source);
        let avoid: Vec<Pattern> = avoid_sources
            .iter()
            .map(|source| Pattern::compile(source).expect(source))
            .collect();
        let outcome = pattern
            .generate(
                &mut governor,
                GenerateOptions {
                    min_len: 0,
                    max_len: Some(8),
                    avoid: &avoid,
                },
            )
            .expect("generation should not fail the governor");
        let GenerateOutcome::Witness(text) = outcome else {
            panic!("{pattern_source}: expected a witness avoiding {avoid_sources:?}");
        };
        assert_eq!(
            pattern.is_match(&text, &mut governor),
            Ok(true),
            "witness {text:?} must match {pattern_source}"
        );
        for (avoided, forbidden) in avoid.iter().zip(*avoid_sources) {
            assert_eq!(
                avoided.is_match(&text, &mut governor),
                Ok(false),
                "witness {text:?} must not match avoided pattern {forbidden:?}"
            );
        }
    }
}

#[test]
fn generate_proves_emptiness_for_a_pattern_intersected_with_its_own_complement() {
    let mut governor = Governor::new(Budget::default());
    let pattern = Pattern::compile("abc").expect("pattern");
    let avoid = Pattern::compile("abc").expect("avoid");
    let outcome = pattern
        .generate(
            &mut governor,
            GenerateOptions {
                min_len: 0,
                max_len: Some(6),
                avoid: &[avoid],
            },
        )
        .expect("governed search must complete");
    match outcome {
        GenerateOutcome::NoCandidate(evidence) => {
            assert!(evidence.explanation.contains("empty"));
        }
        GenerateOutcome::Witness(text) => panic!("expected no witness, got {text:?}"),
    }
}

#[test]
fn generate_proves_emptiness_for_the_universally_empty_class() {
    let mut governor = Governor::new(Budget::default());
    let pattern = Pattern::compile(r"[^\s\S]").expect("pattern");
    let outcome = pattern
        .generate(
            &mut governor,
            GenerateOptions {
                min_len: 0,
                max_len: Some(4),
                avoid: &[],
            },
        )
        .expect("governed search must complete");
    assert!(
        matches!(outcome, GenerateOutcome::NoCandidate(_)),
        "[^\\s\\S] matches nothing and must never produce a witness"
    );
}

#[test]
fn generate_reports_bounded_exhaustion_not_emptiness_for_a_backtracking_pattern() {
    let mut governor = Governor::new(Budget::default());
    // `(a)\1` needs the backtracking engine (it has a backreference); its
    // language ("aa") is not empty, but let a length bound below any
    // possible witness show the explicit-exhaustion wording, distinct from
    // the derivative engine's proof-by-exhaustive-search wording.
    let pattern = Pattern::compile(r"^(a)\1$").expect("pattern");
    assert_eq!(pattern.engine(), Engine::Backtrack);
    let outcome = pattern
        .generate(
            &mut governor,
            GenerateOptions {
                min_len: 0,
                max_len: Some(0),
                avoid: &[],
            },
        )
        .expect("governed search must complete");
    match outcome {
        GenerateOutcome::NoCandidate(evidence) => {
            assert!(
                evidence.explanation.contains("exhaustion")
                    || evidence.explanation.contains("bounded")
            );
        }
        GenerateOutcome::Witness(text) => {
            panic!("length bound 0 cannot match \"aa\", got {text:?}")
        }
    }
    let unbounded = pattern
        .generate(
            &mut governor,
            GenerateOptions {
                min_len: 0,
                max_len: Some(4),
                avoid: &[],
            },
        )
        .expect("governed search must complete");
    assert_eq!(unbounded, GenerateOutcome::Witness("aa".into()));
}

#[test]
fn resource_exhaustion_during_matching_is_a_governor_error_not_a_false_match() {
    let pattern = Pattern::compile("(a+)+$").expect("pattern");
    let mut governor = Governor::new(Budget {
        work: 10,
        ..Budget::default()
    });
    let err = pattern
        .is_match("aaaaaaaaaaaa!", &mut governor)
        .expect_err("ten Work units cannot complete this search");
    assert!(matches!(err, StopReason::BudgetExceeded(_)));
}
