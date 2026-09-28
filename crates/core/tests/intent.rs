// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Explicit semantic domain rules and caller-owned embedding regression cases.
use purrassist_core::{
    Budget, Governor, Outcome,
    intent::{Concept, Domain, Rule, cosine_similarity},
};
use std::collections::BTreeMap;

fn attributes(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}
fn drinks() -> (Domain, Vec<Concept>) {
    let mut domain = Domain {
        id: "example.drinks.v1".into(),
        ..Domain::default()
    };
    for (phrase, key, value) in [
        ("tea", "drink", "tea"),
        ("hot", "temperature", "hot"),
        ("cooled", "temperature", "cold"),
        ("iced", "temperature", "cold"),
        ("glass", "vessel", "drinking-vessel"),
        ("cup", "vessel", "drinking-vessel"),
    ] {
        domain.rules.push(Rule {
            id: phrase.into(),
            phrase: phrase.into(),
            assignments: attributes(&[(key, value)]),
        });
    }
    domain.ignored_words = ["i", "want", "a", "of"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let concepts = ["hot", "cold"]
        .into_iter()
        .map(|temperature| Concept {
            id: format!("{temperature}-tea"),
            label: if temperature == "cold" {
                "cup of iced tea".into()
            } else {
                "cup of hot tea".into()
            },
            attributes: attributes(&[
                ("drink", "tea"),
                ("temperature", temperature),
                ("vessel", "drinking-vessel"),
            ]),
        })
        .collect();
    (domain, concepts)
}
#[test]
fn later_modifier_refines_intent_with_auditable_domain_rules() {
    let (domain, concepts) = drinks();
    let result = domain.resolve(
        "I want a glass of hot tea, cooled",
        &concepts,
        Budget::default(),
    );
    let Outcome::Resolved(answer) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(answer.concept.id, "cold-tea");
    assert_eq!(answer.concept.label, "cup of iced tea");
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|entry| entry.rule.ends_with("::cooled") && entry.explanation.contains("hot"))
    );
}
#[test]
fn unknown_negation_and_underspecification_do_not_silently_resolve() {
    let (domain, concepts) = drinks();
    assert!(matches!(
        domain.resolve("tea", &concepts, Budget::default()).outcome,
        Outcome::Ambiguous(_)
    ));
    assert_eq!(
        domain
            .resolve("not hot tea", &concepts, Budget::default())
            .outcome,
        Outcome::Unresolved
    );
}
#[test]
fn scaled_cosine_handles_extreme_values_and_rejects_invalid_vectors() {
    let mut governor = Governor::new(Budget::default());
    assert!(
        (cosine_similarity(&[f64::MAX, f64::MAX], &[1.0, 1.0], &mut governor).unwrap() - 1.0).abs()
            < 1e-14
    );
    assert!(cosine_similarity(&[f64::NAN], &[1.0], &mut governor).is_err());
    assert!(cosine_similarity(&[0.0], &[1.0], &mut governor).is_err());
    assert!(cosine_similarity(&[1.0], &[1.0, 2.0], &mut governor).is_err());
}

#[test]
fn shared_governor_retains_source_and_preserves_cancellation() {
    let (domain, concepts) = drinks();
    let mut governor = Governor::new(Budget::default());
    let result = domain.resolve_governed("hot tea", &concepts, &mut governor);
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    assert_eq!(result.receipt.sources[0].text, "hot tea");
    assert_eq!(result.receipt.usage, governor.usage());
    governor.cancel();
    let result = domain.resolve_governed("hot tea", &concepts, &mut governor);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        purrassist_core::StopReason::Cancelled
    );
    assert_eq!(result.receipt.sources, Vec::new());
}
