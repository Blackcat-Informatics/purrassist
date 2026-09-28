// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Candidate path spelling, context, ambiguity, and traversal tests.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason, digest::Digest, matching::MatchRequest, paths::PathIndex,
};

fn index() -> PathIndex {
    PathIndex::new(
        [
            "src/reader.rs",
            "tests/reader.rs",
            "docs/README.md",
            "other/README.md",
            "src/writer.rs",
        ]
        .into_iter()
        .map(str::to_string),
        &mut Governor::new(Budget::default()),
    )
    .unwrap()
}
#[test]
fn parent_context_disambiguates_preserving_original_paths() {
    let result = index().resolve("raeder.rs", "src", &MatchRequest::default());
    let Outcome::BestGuess(answer) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(answer.path, "src/reader.rs");
    assert!(result.receipt.contains_guesses());
    let result = index().resolve("README.md", "", &MatchRequest::default());
    let Outcome::Ambiguous(answers) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(answers.len(), 2);
    assert!(
        answers
            .iter()
            .all(|answer| answer.path.ends_with("README.md"))
    );
    assert_eq!(result.receipt.evidence, Vec::new());
    assert_eq!(result.receipt.inferences, Vec::new());
    assert_eq!(result.receipt.alternatives.len(), answers.len());
    for (answer, ledger) in answers.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(answer.to_node().to_json().as_bytes())
        );
        assert_eq!(ledger.evidence.len(), 1);
        assert_eq!(ledger.evidence[0].destination, answer.path);
        assert_eq!(ledger.inferences.len(), 1);
        assert_eq!(ledger.inferences[0].value, answer.path);
        assert_eq!(ledger.losses, Vec::new());
        assert_eq!(ledger.validations, Vec::new());
    }
}
#[test]
fn weak_similarity_does_not_silently_choose_a_file() {
    let result = index().resolve("unrelated.xyz", "src", &MatchRequest::default());
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(matches!(
        index()
            .resolve("src/reader.rs", "other", &MatchRequest::default())
            .outcome,
        Outcome::Resolved(_)
    ));
}
#[test]
fn traversal_and_exhaustion_never_become_a_selected_path() {
    for invalid in [
        "../secrets",
        "/absolute",
        "a/../b",
        "a//b",
        "C:/file",
        "a\\b",
    ] {
        assert!(PathIndex::new([invalid.into()], &mut Governor::new(Budget::default())).is_err());
        assert_eq!(
            index()
                .resolve(invalid, "", &MatchRequest::default())
                .outcome,
            Outcome::Unresolved
        );
    }
    let request = MatchRequest {
        budget: Budget {
            candidates: 1,
            ..Budget::default()
        },
        ..MatchRequest::default()
    };
    let result = index().resolve("reader.rs", "src", &request);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(matches!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(_)
    ));
}

#[test]
fn shared_governor_reports_construction_and_sticky_exhaustion() {
    let request = MatchRequest::default();
    let mut governor = Governor::new(request.budget);
    let index = PathIndex::new(["src/reader.rs".into()], &mut governor).unwrap();
    let construction = governor.usage();
    let result = index.resolve_governed(
        "src/reader.rs",
        "",
        &request,
        purrassist_core::paths::SelectionPolicy::default(),
        &mut governor,
    );
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    assert_eq!(result.receipt.usage.candidates, construction.candidates + 1);
    assert!(result.receipt.usage.work > construction.work);
    governor.cancel();
    let cancelled = index.resolve_governed(
        "src/reader.rs",
        "",
        &request,
        purrassist_core::paths::SelectionPolicy::default(),
        &mut governor,
    );
    assert_eq!(cancelled.outcome, Outcome::Unresolved);
    assert_eq!(cancelled.receipt.stopped, StopReason::Cancelled);
}

#[test]
fn path_ledger_exhaustion_does_not_return_partial_alternatives() {
    let paths = index();
    let baseline = paths.resolve("README.md", "", &MatchRequest::default());
    assert!(matches!(baseline.outcome, Outcome::Ambiguous(_)));
    let request = MatchRequest {
        budget: Budget {
            evidence_entries: baseline.receipt.usage.evidence_entries - 1,
            ..Budget::default()
        },
        ..MatchRequest::default()
    };
    let result = paths.resolve("README.md", "", &request);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(purrassist_core::contract::Resource::EvidenceEntries)
    );
    assert_eq!(result.receipt.alternatives, Vec::new());
    assert_eq!(result.receipt.evidence, Vec::new());
    assert_eq!(result.receipt.inferences, Vec::new());
}
