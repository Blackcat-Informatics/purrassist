// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! String selection preserves uncertainty, identity, and alternative provenance.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    digest::Digest,
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    wire::WireValue,
};

fn candidates(values: &[(&str, &str)]) -> Vec<Candidate> {
    values
        .iter()
        .map(|(id, text)| Candidate::new(*id, *text))
        .collect()
}

#[test]
fn exact_takes_precedence_and_fuzzy_answer_is_a_guess() {
    let values = candidates(&[("right", "reader"), ("near", "readers")]);
    let exact = matching::resolve(
        "reader",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    let Outcome::Resolved(answer) = exact.outcome else {
        panic!("{exact:?}")
    };
    assert_eq!(answer.candidate.id, "right");
    assert!(!exact.receipt.contains_guesses());
    let fuzzy = matching::resolve(
        "raeder",
        &values[..1],
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert!(matches!(fuzzy.outcome, Outcome::BestGuess(_)));
    assert!(fuzzy.receipt.contains_guesses());
}

#[test]
fn ties_keep_separate_digest_bound_ledgers_even_at_zero_margin() {
    let values = candidates(&[("a", "reader"), ("b", "reader")]);
    let answer = matching::resolve(
        "reader",
        &values,
        &MatchRequest::default(),
        SelectionPolicy {
            minimum_score: 10_000,
            minimum_margin: 0,
        },
    );
    let Outcome::Ambiguous(matches) = answer.outcome else {
        panic!("{answer:?}")
    };
    assert_eq!(matches.len(), 2);
    assert_eq!(answer.receipt.alternatives.len(), 2);
    assert_eq!(answer.receipt.evidence, Vec::new());
    for (candidate, ledger) in matches.iter().zip(&answer.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(candidate.to_node().to_json().as_bytes())
        );
        assert_eq!(ledger.evidence[0].destination, candidate.candidate.id);
        assert_eq!(ledger.inferences, Vec::new());
    }
}

#[test]
fn alias_tie_and_weak_similarity_are_explicit() {
    let mut values = candidates(&[("tea", "cold tea"), ("drink", "beverage")]);
    values[0].aliases.push("iced tea".into());
    values[1].aliases.push("iced tea".into());
    let answer = matching::resolve(
        "iced tea",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    let Outcome::Ambiguous(matches) = answer.outcome else {
        panic!("{answer:?}")
    };
    assert_eq!(matches.len(), 2);
    assert!(
        answer
            .receipt
            .alternatives
            .iter()
            .all(|ledger| ledger.inferences.len() == 1)
    );
    let weak = matching::resolve(
        "unrelated",
        &candidates(&[("tea", "tea")]),
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert_eq!(weak.outcome, Outcome::Unresolved);
    assert_eq!(weak.receipt.stopped, StopReason::NoCandidate);
}

#[test]
fn selection_considers_candidates_beyond_the_ranking_display_limit() {
    let mut values: Vec<_> = (0..12)
        .map(|index| Candidate::new(format!("distractor-{index}"), "readers"))
        .collect();
    values.push(Candidate::new("answer", "reader"));
    let result = matching::resolve(
        "reader",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    let Outcome::Resolved(answer) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(answer.candidate.id, "answer");
}

#[test]
fn ledger_exhaustion_discards_all_alternatives() {
    let values = candidates(&[("a", "reader"), ("b", "reader")]);
    let baseline = matching::resolve(
        "reader",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    let request = MatchRequest {
        budget: Budget {
            evidence_entries: baseline.receipt.usage.evidence_entries - 1,
            ..Budget::default()
        },
        ..MatchRequest::default()
    };
    let result = matching::resolve("reader", &values, &request, SelectionPolicy::default());
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::EvidenceEntries)
    );
    assert_eq!(result.receipt.alternatives, Vec::new());
    assert_eq!(result.receipt.evidence, Vec::new());
}

#[test]
fn shared_governor_and_invalid_policy_fail_closed() {
    let values = candidates(&[("tea", "tea")]);
    let request = MatchRequest::default();
    let mut governor = Governor::new(request.budget);
    governor.cancel();
    let stopped = matching::resolve_governed(
        "tea",
        &values,
        &request,
        SelectionPolicy::default(),
        &mut governor,
    );
    assert_eq!(stopped.outcome, Outcome::Unresolved);
    assert_eq!(stopped.receipt.stopped, StopReason::Cancelled);
    let invalid = matching::resolve(
        "tea",
        &values,
        &request,
        SelectionPolicy {
            minimum_score: 10_001,
            minimum_margin: 0,
        },
    );
    assert!(matches!(
        invalid.receipt.stopped,
        StopReason::InvalidInput(_)
    ));
}

#[test]
fn numeric_identity_guard_is_explicit_and_budgeted() {
    // `rank` and `resolve` both apply the protected-class guard: a candidate
    // whose decimal digit run differs from the query is excluded from
    // ranking entirely, not merely demoted, so neither ever reports it as a
    // `Resolved`/ranked answer (see also `protected_class_digits_excludes_candidate`
    // below, which asserts this same non-resolution more directly).
    let values = candidates(&[("wrong", "invoice-1002")]);
    let rejected = matching::resolve(
        "invoice-1001",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert_eq!(rejected.outcome, Outcome::Unresolved);
    assert_eq!(rejected.receipt.stopped, StopReason::NoCandidate);
    assert_eq!(
        rejected.receipt.evidence[0].rule,
        "matching.numeric-identity-v1"
    );
    let ranked = matching::rank("invoice-1001", &values, &MatchRequest::default());
    assert!(
        !matches!(ranked.outcome, Outcome::Resolved(_)),
        "a digit-run mismatch must exclude the candidate from rank, not just from resolve: {ranked:?}"
    );
    assert_eq!(ranked.receipt.stopped, StopReason::NoCandidate);
    assert_eq!(
        ranked.receipt.evidence[0].rule,
        "matching.numeric-identity-v1"
    );
    // The protected-class check itself charges Work before comparing tokens;
    // a budget too small to afford that charge fails closed with no evidence,
    // rather than silently skipping the guard.
    let request = MatchRequest {
        budget: Budget {
            work: 1,
            ..Budget::default()
        },
        ..MatchRequest::default()
    };
    let exhausted = matching::resolve(
        "invoice-1001",
        &values,
        &request,
        SelectionPolicy::default(),
    );
    assert_eq!(exhausted.outcome, Outcome::Unresolved);
    assert_eq!(
        exhausted.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Work)
    );
    assert_eq!(exhausted.receipt.evidence, Vec::new());
}

#[test]
fn protected_class_digits_excludes_candidate() {
    let values = candidates(&[("wrong", "invoice-1002")]);
    let ranked = matching::rank("invoice-1001", &values, &MatchRequest::default());
    assert_eq!(ranked.outcome, Outcome::Unresolved);
    let resolved = matching::resolve(
        "invoice-1001",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert_eq!(resolved.outcome, Outcome::Unresolved);
}

#[test]
fn protected_class_negation_excludes_candidate() {
    let values = candidates(&[("wrong", "task available")]);
    let ranked = matching::rank("task unavailable", &values, &MatchRequest::default());
    assert_eq!(ranked.outcome, Outcome::Unresolved);
    assert!(
        ranked
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "matching.protected-negation-v1")
    );
}

#[test]
fn protected_class_semver_excludes_candidate() {
    let values = candidates(&[("wrong", "release-v1.2.4")]);
    let ranked = matching::rank("release-v1.2.3", &values, &MatchRequest::default());
    assert_eq!(ranked.outcome, Outcome::Unresolved);
    assert!(
        ranked
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "matching.protected-semver-v1")
    );
}

#[test]
fn protected_class_iso_date_excludes_candidate() {
    let values = candidates(&[("wrong", "log-2024-01-16")]);
    let ranked = matching::rank("log-2024-01-15", &values, &MatchRequest::default());
    assert_eq!(ranked.outcome, Outcome::Unresolved);
    assert!(
        ranked
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "matching.protected-iso-date-v1")
    );
}

#[test]
fn protected_class_hex_digest_excludes_candidate() {
    let values = candidates(&[("wrong", "build-deadbeee")]);
    let ranked = matching::rank("build-deadbeef", &values, &MatchRequest::default());
    assert_eq!(ranked.outcome, Outcome::Unresolved);
    assert!(
        ranked
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "matching.protected-hex-digest-v1")
    );
}

#[test]
fn protected_class_unit_excludes_candidate() {
    let values = candidates(&[("wrong", "response time: 5s")]);
    let ranked = matching::rank("response time: 5ms", &values, &MatchRequest::default());
    assert_eq!(ranked.outcome, Outcome::Unresolved);
    assert!(
        ranked
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "matching.protected-unit-v1")
    );
}

#[test]
fn protected_class_exact_alias_bypasses_the_guard() {
    let mut values = candidates(&[("right", "invoice-1002")]);
    values[0].aliases.push("invoice-1001".into());
    let resolved = matching::resolve(
        "invoice-1001",
        &values,
        &MatchRequest::default(),
        SelectionPolicy::default(),
    );
    assert!(matches!(resolved.outcome, Outcome::BestGuess(_)));
}
