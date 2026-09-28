// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Curated contrast set for automatic corrections and conservative abstention.
use purrassist_core::{
    Effort, Outcome,
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
};

#[derive(Clone, Copy, Debug)]
enum Expected {
    Resolved(&'static str),
    Guess(&'static str),
    Ambiguous,
    Unresolved,
}
struct Case {
    name: &'static str,
    query: &'static str,
    choices: &'static [(&'static str, &'static str)],
    aliases: &'static [(&'static str, &'static str)],
    expected: Expected,
    truth: Option<&'static str>,
    effort: Effort,
    minimum_score: u32,
}
const fn case(
    name: &'static str,
    query: &'static str,
    choices: &'static [(&'static str, &'static str)],
    expected: Expected,
    truth: Option<&'static str>,
) -> Case {
    Case {
        name,
        query,
        choices,
        aliases: &[],
        expected,
        truth,
        effort: Effort::Standard,
        minimum_score: 7500,
    }
}

/// Lattice-level cases: byte identity, NFC, case-fold, width/compatibility
/// fold, diacritic-strip, UTS-39 confusable skeleton, and depunctuation.
fn lattice_cases() -> Vec<Case> {
    vec![
        case(
            "exact with distractor",
            "reader",
            &[("right", "reader"), ("near", "readers")],
            Expected::Resolved("right"),
            Some("right"),
        ),
        case(
            "case fold",
            "ReadMe",
            &[("right", "README")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "canonical accent",
            "cafe\u{301}",
            &[("right", "café")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "missing accent",
            "cafe",
            &[("right", "café"), ("wrong", "caff")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "accent collision",
            "cafe",
            &[("one", "café"), ("two", "cafè")],
            Expected::Ambiguous,
            None,
        ),
        case(
            "compatibility width",
            "ＡＢＣ",
            &[("right", "ABC")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "full case fold",
            "Straße",
            &[("right", "strasse")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "punctuation collision",
            "user_name",
            &[("one", "user-name"), ("two", "username")],
            Expected::Ambiguous,
            None,
        ),
        case(
            "transposition",
            "raeder",
            &[("right", "reader"), ("wrong", "writer")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "duplicate exact",
            "reader",
            &[("one", "reader"), ("two", "reader")],
            Expected::Ambiguous,
            None,
        ),
        // Lattice level: NFC (canonical composition, before case or width folding).
        case(
            "NFC canonical equivalence",
            "e\u{301}lan",
            &[("right", "élan")],
            Expected::Guess("right"),
            Some("right"),
        ),
        // Lattice level: UTS-39 confusable skeleton, after case/width/diacritic
        // folding. Cyrillic е (U+0435) is confusable with Latin e (U+0065).
        case(
            "confusable skeleton",
            "custom\u{435}r",
            &[("right", "customer"), ("wrong", "customers")],
            Expected::Guess("right"),
            Some("right"),
        ),
    ]
}

/// Edit distance, lexical, and phonetic cases: transposition, short/generic
/// abstention, and a phonetic match below the default acceptance threshold.
fn distance_cases() -> Vec<Case> {
    let mut cases = vec![
        case(
            "Unicode short transposition",
            "猫狗",
            &[("right", "狗猫")],
            Expected::Unresolved,
            Some("right"),
        ),
        case(
            "short substitution",
            "a",
            &[("wrong", "b")],
            Expected::Unresolved,
            None,
        ),
        case(
            "generic phrase extension",
            "tea",
            &[("wrong", "tea pot")],
            Expected::Unresolved,
            None,
        ),
    ];
    let mut phonetic = case(
        "phonetic requires lower threshold",
        "Robert",
        &[("right", "Rupert")],
        Expected::Guess("right"),
        Some("right"),
    );
    phonetic.minimum_score = 6000;
    cases.push(phonetic);
    cases
}

/// Protected-class cases: a difference in a protected token class excludes
/// the candidate even when the surrounding text is otherwise a strong match,
/// plus a caller-alias bypass and unrelated-string abstention.
fn protected_class_cases() -> Vec<Case> {
    let cases = vec![
        case(
            "numeric collision",
            "invoice-1001",
            &[("one", "invoice-1002"), ("two", "invoice-1003")],
            Expected::Unresolved,
            None,
        ),
        case(
            "numeric false correction",
            "invoice-1001",
            &[("wrong", "invoice-1002")],
            Expected::Unresolved,
            None,
        ),
        case(
            "numeric typo with correct identity",
            "invocie-1001",
            &[("right", "invoice-1001"), ("wrong", "invoice-1002")],
            Expected::Guess("right"),
            Some("right"),
        ),
        case(
            "version component mismatch",
            "release-v1.2.3",
            &[("wrong", "release-v1.2.4")],
            Expected::Unresolved,
            None,
        ),
        case(
            "unrelated",
            "unrelated",
            &[("one", "tea"), ("two", "coffee")],
            Expected::Unresolved,
            None,
        ),
        case(
            "negation changes intent",
            "not ready",
            &[("wrong", "ready")],
            Expected::Unresolved,
            None,
        ),
        // Protected class: negation. Without the guard, removing the "un"
        // prefix is a small enough edit to cross the acceptance threshold.
        case(
            "protected class: negation",
            "task unavailable",
            &[("wrong", "task available")],
            Expected::Unresolved,
            None,
        ),
        // Protected class: ISO date. A one-digit day change is a tiny edit
        // relative to the whole string.
        case(
            "protected class: ISO date",
            "log-2024-01-15-error",
            &[("wrong", "log-2024-01-16-error")],
            Expected::Unresolved,
            None,
        ),
        // Protected class: hex digest. Neither string contains a decimal
        // digit, so only the dedicated hex-digest class catches this.
        case(
            "protected class: hex digest",
            "build-deadbeef",
            &[("wrong", "build-deadbeee")],
            Expected::Unresolved,
            None,
        ),
        // Protected class: unit. The shared digit run "5" is identical; only
        // the unit differs.
        case(
            "protected class: unit",
            "timeout 5ms",
            &[("wrong", "timeout 5s")],
            Expected::Unresolved,
            None,
        ),
    ];
    cases
}

/// Caller-declared alias cases: a domain alias resolving a query no textual
/// signal would otherwise match, competing aliases producing ambiguity, an
/// alias bypassing the digit-run protected class, and `Effort::Exact`
/// ignoring aliases entirely.
fn alias_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut alias = case(
        "declared domain alias",
        "iced tea",
        &[("right", "cold tea"), ("wrong", "coffee")],
        Expected::Guess("right"),
        Some("right"),
    );
    alias.aliases = &[("right", "iced tea")];
    cases.push(alias);
    let mut numeric_alias = case(
        "declared numeric alias",
        "invoice-1001",
        &[("right", "invoice-1002")],
        Expected::Guess("right"),
        Some("right"),
    );
    numeric_alias.aliases = &[("right", "invoice-1001")];
    cases.push(numeric_alias);
    let mut alias_tie = case(
        "competing domain aliases",
        "iced tea",
        &[("one", "cold tea"), ("two", "chilled tea")],
        Expected::Ambiguous,
        None,
    );
    alias_tie.aliases = &[("one", "iced tea"), ("two", "iced tea")];
    cases.push(alias_tie);
    let mut exact_effort = case(
        "exact effort ignores aliases",
        "iced tea",
        &[("right", "cold tea")],
        Expected::Unresolved,
        Some("right"),
    );
    exact_effort.aliases = &[("right", "iced tea")];
    exact_effort.effort = Effort::Exact;
    cases.push(exact_effort);
    cases
}

fn curated_cases() -> Vec<Case> {
    let mut cases = lattice_cases();
    cases.extend(distance_cases());
    cases.extend(protected_class_cases());
    cases.extend(alias_cases());
    cases
}

#[test]
fn curated_string_selection_quality() {
    let cases = curated_cases();
    let mut correct_auto = 0;
    let mut incorrect_auto = 0;
    let mut abstained = 0;
    for example in &cases {
        let mut choices: Vec<_> = example
            .choices
            .iter()
            .map(|(id, text)| Candidate::new(*id, *text))
            .collect();
        for (id, alias) in example.aliases {
            choices
                .iter_mut()
                .find(|choice| choice.id == *id)
                .unwrap()
                .aliases
                .push((*alias).into());
        }
        let request = MatchRequest {
            effort: example.effort,
            ..MatchRequest::default()
        };
        let answer = matching::resolve(
            example.query,
            &choices,
            &request,
            SelectionPolicy {
                minimum_score: example.minimum_score,
                ..SelectionPolicy::default()
            },
        );
        match (&answer.outcome, example.expected) {
            (Outcome::Resolved(value), Expected::Resolved(id))
            | (Outcome::BestGuess(value), Expected::Guess(id)) => {
                assert_eq!(value.candidate.id, id, "{}: {answer:?}", example.name);
            }
            (Outcome::Ambiguous(values), Expected::Ambiguous) => {
                assert!(values.len() >= 2, "{}: {answer:?}", example.name);
            }
            (Outcome::Unresolved, Expected::Unresolved) => {}
            _ => panic!("{}: {answer:?}", example.name),
        }
        match &answer.outcome {
            Outcome::Resolved(value) | Outcome::BestGuess(value) => {
                if Some(value.candidate.id.as_str()) == example.truth {
                    correct_auto += 1;
                } else {
                    incorrect_auto += 1;
                }
            }
            Outcome::Ambiguous(_) | Outcome::Unresolved => abstained += 1,
        }
    }
    // docs/MATCHING-QUALITY.md quotes this exact case count; keep both in sync.
    assert_eq!(cases.len(), 30);
    assert_eq!(incorrect_auto, 0);
    eprintln!(
        "curated string corpus: {correct_auto} correct automatic, {incorrect_auto} incorrect automatic, {abstained} abstained"
    );
}
