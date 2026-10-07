// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Pattern engine tests: property escapes of each kind, derivative and
//! backtracking agreement on anchors, engine selection, backreferences, and
//! `generate`'s witness/emptiness contract. The test262 property-escape
//! corpus and the generated differential run in `pattern_corpus.rs` (`make
//! corpus`).
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

/// Each kind of property escape the test262 corpus covers (binary property,
/// `General_Category` value and group, `Script`, `Script_Extensions`, and
/// their aliases) matches its members and `\P{...}` matches exactly the
/// rest, on both engines.
#[test]
fn property_escapes_match_their_sets_and_complements() {
    let cases: &[(&str, &[&str], &[&str])] = &[
        ("ASCII_Hex_Digit", &["0", "f", "F"], &["g", "\u{ff10}", " "]),
        (
            "Alphabetic",
            &["a", "\u{3b1}", "\u{4e00}"],
            &["1", "-", "\u{1f600}"],
        ),
        (
            "General_Category=Uppercase_Letter",
            &["A", "\u{391}"],
            &["a", "1"],
        ),
        ("Lu", &["Z", "\u{410}"], &["z", "_"]),
        ("gc=Nd", &["7", "\u{663}"], &["x", "\u{2167}"]),
        ("L", &["q", "\u{5d0}"], &["1", " "]),
        ("LC", &["a", "A", "\u{1c5}"], &["\u{2b0}", "1"]),
        ("Script=Greek", &["\u{3b1}", "\u{3a9}"], &["a", "\u{430}"]),
        ("sc=Cyrillic", &["\u{430}"], &["a", "\u{3b1}"]),
        (
            "Script_Extensions=Devanagari",
            &["\u{915}", "\u{964}"],
            &["a", "\u{995}"],
        ),
    ];
    for (expression, members, others) in cases {
        let positive = Pattern::compile(&format!("^\\p{{{expression}}}$")).unwrap();
        let negative = Pattern::compile(&format!("^\\P{{{expression}}}$")).unwrap();
        for (subject, member) in members
            .iter()
            .map(|text| (text, true))
            .chain(others.iter().map(|text| (text, false)))
        {
            for (pattern, expected) in [(&positive, member), (&negative, !member)] {
                let mut derivative = Governor::new(Budget::default());
                let mut backtracking = Governor::new(Budget::default());
                assert_eq!(
                    pattern.is_match(subject, &mut derivative),
                    Ok(expected),
                    "{expression} / {subject:?}"
                );
                assert_eq!(
                    pattern.is_match_backtracking(subject, &mut backtracking),
                    Ok(expected),
                    "{expression} / {subject:?} (backtracking)"
                );
            }
        }
    }
    assert!(Pattern::compile("\\p{Not_A_Property}").is_err());
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
