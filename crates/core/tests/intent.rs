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

#[test]
fn conflicting_rules_have_separate_output_bound_ledgers() {
    use purrassist_core::{contract::InferenceKind, digest::Digest};
    let (mut domain, concepts) = drinks();
    domain.rules.push(Rule {
        id: "other-tea".into(),
        phrase: "tea".into(),
        assignments: attributes(&[("drink", "coffee")]),
    });
    domain.rules.push(Rule {
        id: "warm".into(),
        phrase: "temperature".into(),
        assignments: attributes(&[("temperature", "hot")]),
    });
    domain.rules.push(Rule {
        id: "cold".into(),
        phrase: "temperature".into(),
        assignments: attributes(&[("temperature", "cold")]),
    });
    let result = domain.resolve("tea temperature", &concepts, Budget::default());
    let Outcome::Ambiguous(answers) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(answers.len(), 2);
    assert_eq!(result.receipt.alternatives.len(), answers.len());
    // Both surviving interpretations matched the *same* "tea" phrase rule
    // identically (same rule id, same prior-assignment context, same
    // assigned value): the derivation forest (crates/core/src/provenance.rs)
    // recognizes that shared evidence and its derived "drink" inference as
    // depended on by *every* alternative and promotes them once to the
    // top-level, selected-result ledgers rather than duplicating them into
    // both alternatives (crates/core/src/contract.rs `RECEIPT_VERSION` 2).
    // Only the temperature rule, which differs between the two
    // interpretations, remains alternative-specific below.
    assert_eq!(result.receipt.evidence.len(), 1);
    assert!(result.receipt.evidence[0].rule.ends_with("::tea"));
    assert_eq!(result.receipt.inferences.len(), 1);
    assert_eq!(
        result.receipt.inferences[0].evidence.destination,
        "/desired/drink"
    );
    assert_eq!(result.receipt.inferences[0].kind, InferenceKind::Derived);
    for (answer, ledger) in answers.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(answer.to_node().to_json().as_bytes())
        );
        assert!(result.receipt.validations[ledger.validations[0]].passed);
        assert!(
            !ledger
                .evidence
                .iter()
                .any(|entry| entry.rule.ends_with("::other-tea"))
        );
        let expected = if answer.concept.id == "cold-tea" {
            "::cold"
        } else {
            "::warm"
        };
        assert!(
            ledger
                .evidence
                .iter()
                .any(|entry| entry.rule.ends_with(expected))
        );
        assert_eq!(ledger.evidence.len(), 1);
        assert_eq!(ledger.inferences.len(), 1);
        assert!(
            ledger
                .inferences
                .iter()
                .all(|inference| inference.kind == InferenceKind::Derived)
        );
        assert!(
            ledger
                .inferences
                .iter()
                .any(
                    |inference| inference.evidence.destination == "/desired/temperature"
                        && inference.value == format!("{:?}", answer.desired["temperature"])
                )
        );
    }
    assert!(!result.receipt.contains_guesses());
}
#[test]
fn rejected_and_duplicate_derivations_do_not_pollute_selected_evidence() {
    let (mut domain, concepts) = drinks();
    domain.rules.push(Rule {
        id: "wrong".into(),
        phrase: "tea".into(),
        assignments: attributes(&[("drink", "coffee")]),
    });
    domain.rules.push(Rule {
        id: "duplicate".into(),
        phrase: "tea".into(),
        assignments: attributes(&[("drink", "tea")]),
    });
    let result = domain.resolve("hot tea", &concepts, Budget::default());
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    assert_eq!(result.receipt.evidence.len(), 2);
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .all(|entry| !entry.rule.ends_with("::wrong") && !entry.rule.ends_with("::duplicate"))
    );
    assert_eq!(result.receipt.validations.len(), 1);
    assert_eq!(result.receipt.alternatives, Vec::new());
}
#[test]
fn semantic_ledger_exhaustion_never_publishes_partial_alternatives() {
    use purrassist_core::{StopReason, contract::Resource};
    let (domain, concepts) = drinks();
    let complete = domain.resolve("tea", &concepts, Budget::default());
    let limit = complete.receipt.usage.evidence_entries - 1;
    assert!(matches!(complete.outcome, Outcome::Ambiguous(_)));
    let limited = domain.resolve(
        "tea",
        &concepts,
        Budget {
            evidence_entries: limit,
            ..Budget::default()
        },
    );
    assert_eq!(limited.outcome, Outcome::Unresolved);
    assert_eq!(
        limited.receipt.stopped,
        StopReason::BudgetExceeded(Resource::EvidenceEntries)
    );
    assert_eq!(limited.receipt.alternatives, Vec::new());
    assert_eq!(limited.receipt.validations, Vec::new());
    assert_eq!(limited.receipt.evidence, Vec::new());
}
