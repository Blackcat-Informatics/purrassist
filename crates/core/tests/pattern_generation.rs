// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Assertion-sensitive pattern generation, checked against independent
//! backtracking evaluation of an exhaustive finite subject alphabet.
use purrassist_core::contract::Resource;
use purrassist_core::pattern::{GenerateOptions, GenerateOutcome, Pattern};
use purrassist_core::{Budget, Governor, StopReason};

fn verify_witness(target: &Pattern, avoid: &[Pattern], text: &str) {
    for (pattern, expected) in
        std::iter::once((target, true)).chain(avoid.iter().map(|forbidden| (forbidden, false)))
    {
        assert_eq!(
            pattern.is_match(text, &mut Governor::new(Budget::default())),
            Ok(expected),
            "derivative evaluation of witness {text:?}"
        );
        assert_eq!(
            pattern.is_match_backtracking(text, &mut Governor::new(Budget::default())),
            Ok(expected),
            "independent backtracking evaluation of witness {text:?}"
        );
    }
}

fn generated(source: &str, exclusions: &[&str], min: usize, max: Option<usize>) -> String {
    let target = Pattern::compile(source).expect("target compiles");
    let avoid: Vec<_> = exclusions
        .iter()
        .map(|source| Pattern::compile(source).expect("exclusion compiles"))
        .collect();
    let outcome = target
        .generate(
            &mut Governor::new(Budget::default()),
            GenerateOptions {
                min_len: min,
                max_len: max,
                avoid: &avoid,
            },
        )
        .expect("generation completes within its budget");
    let GenerateOutcome::Witness(text) = outcome else {
        panic!("expected witness for {source:?}, excluding {exclusions:?}; got {outcome:?}")
    };
    assert!(text.chars().count() >= min);
    assert!(max.is_none_or(|maximum| text.chars().count() <= maximum));
    verify_witness(&target, &avoid, &text);
    text
}

#[test]
fn end_anchored_exclusions_do_not_forbid_repaired_extensions() {
    assert_eq!(generated("^a+$", &["^a$"], 0, None), "aa");
    assert_eq!(generated("^a+$", &["^$"], 0, None), "a");
    assert_eq!(generated("^a$", &["^$"], 0, Some(1)), "a");
    assert_eq!(generated("^[ab]+$", &["a$"], 2, Some(2)), "ab");
}

#[test]
fn acceptance_and_frontier_identity_preserve_word_assertion_context() {
    assert_eq!(generated(r"^a\b$", &[], 0, None), "a");
    assert_eq!(generated(r"^[a-]\b$", &[], 1, Some(1)), "a");
    assert_eq!(generated(r"^[a-]\B$", &[], 1, Some(1)), "-");
    assert_eq!(generated(r"^a\b$", &[r"^a\B$"], 0, None), "a");
}

#[test]
fn unanchored_matches_survive_later_characters_and_need_real_neighbours() {
    for source in [r"\Ba", r"a\B", r"\B_\B"] {
        let text = generated(source, &[], 0, Some(3));
        let pattern = Pattern::compile(source).expect("pattern compiles");
        assert_eq!(
            pattern.is_match_backtracking(&text, &mut Governor::new(Budget::default())),
            Ok(true)
        );
    }
    assert_eq!(generated("a", &[], 3, Some(3)).chars().count(), 3);
    assert_eq!(generated(r"^a\b", &[], 2, Some(2)).chars().count(), 2);
}

fn subjects(alphabet: &[char], maximum: usize) -> Vec<String> {
    let mut all = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..maximum {
        let mut next = Vec::new();
        for prefix in &frontier {
            for &character in alphabet {
                let mut subject = prefix.clone();
                subject.push(character);
                next.push(subject);
            }
        }
        all.extend(next.iter().cloned());
        frontier = next;
    }
    all
}

fn oracle_matches(pattern: &Pattern, subjects: &[String]) -> Vec<bool> {
    subjects
        .iter()
        .map(|text| {
            pattern
                .is_match_backtracking(text, &mut Governor::new(Budget::default()))
                .expect("finite independent evaluation completes")
        })
        .collect()
}

#[test]
fn bounded_generation_agrees_with_exhaustive_assertion_oracle() {
    // These anchored target classes admit exactly this subject alphabet.
    // Enumerating all subjects through the largest bound is therefore a
    // complete existence/shortest-length oracle, independent of derivatives,
    // minterm representatives, frontier deduplication, and cycle detection.
    let subjects = subjects(&['a', 'b', '_', '-'], 3);
    assert_eq!(subjects.len(), 85);
    let targets = [
        "^[ab_-]{0,3}$",
        "^[ab_-]{1,3}$",
        "^a+$",
        r"^[a-]{1,3}\b$",
        r"^[a-]{1,3}\B$",
        r"^\b[ab_-]{1,3}\b$",
        r"^\B[ab_-]{1,3}\B$",
        r"^[a-]?\ba?[a-]?$",
        r"^a\b$",
        r"^-\B$",
        r"^\b$",
        r"^\B$",
    ];
    let exclusions = [
        "",
        "^$",
        "^a$",
        "a$",
        "^a",
        "a",
        r"^a\b$",
        r"a\b",
        r"\ba$",
        r"\b",
        r"\B",
        "^",
        "$",
        "^[ab_-]{0,1}$",
    ];
    let forbidden: Vec<_> = exclusions
        .iter()
        .map(|source| Pattern::compile(source).expect("exclusion compiles"))
        .collect();
    let forbidden_matches: Vec<_> = forbidden
        .iter()
        .map(|pattern| oracle_matches(pattern, &subjects))
        .collect();
    let mut cases = 0;
    for source in targets {
        let target = Pattern::compile(source).expect("target compiles");
        let target_matches = oracle_matches(&target, &subjects);
        for excluded in std::iter::once(Vec::new())
            .chain((0..forbidden.len()).map(|index| vec![index]))
            .chain([vec![2, 3], vec![1, 6], vec![4, 10]])
        {
            let avoid: Vec<_> = excluded
                .iter()
                .map(|&index| forbidden[index].clone())
                .collect();
            for maximum in 0..=3 {
                for minimum in 0..=maximum + 1 {
                    let expected = subjects
                        .iter()
                        .enumerate()
                        .find(|(index, text)| {
                            (minimum..=maximum).contains(&text.chars().count())
                                && target_matches[*index]
                                && excluded
                                    .iter()
                                    .all(|&avoid| !forbidden_matches[avoid][*index])
                        })
                        .map(|(_, text)| text.chars().count());
                    let outcome = target
                        .generate(
                            &mut Governor::new(Budget::default()),
                            GenerateOptions {
                                min_len: minimum,
                                max_len: Some(maximum),
                                avoid: &avoid,
                            },
                        )
                        .expect("finite generation completes");
                    match outcome {
                        GenerateOutcome::Witness(text) => {
                            assert_eq!(
                                Some(text.chars().count()),
                                expected,
                                "{source:?}, exclusions {excluded:?}, bounds {minimum}..={maximum}, witness {text:?}"
                            );
                            verify_witness(&target, &avoid, &text);
                        }
                        GenerateOutcome::NoCandidate(_) => assert_eq!(
                            expected, None,
                            "false absence for {source:?}, exclusions {excluded:?}, bounds {minimum}..={maximum}"
                        ),
                    }
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 3_024);
}

#[test]
fn generation_exhaustion_stays_an_explicit_resource_error() {
    let target = Pattern::compile("^a+$").expect("target compiles");
    let avoid = Pattern::compile("^a$").expect("avoid compiles");
    let options = GenerateOptions {
        min_len: 0,
        max_len: None,
        avoid: &[avoid],
    };
    let mut governor = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        target.generate(&mut governor, options),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let mut governor = Governor::new(Budget {
        scratch_bytes: 0,
        ..Budget::default()
    });
    assert_eq!(
        target.generate(&mut governor, options),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
}
