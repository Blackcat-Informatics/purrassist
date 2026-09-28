// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Numerical scaling, epistemic selection, and all-or-nothing search budgets.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    embeddings::{Embedding, EmbeddingIndex, EmbeddingPolicy},
};
fn entry(id: &str, vector: &[f64]) -> Embedding {
    Embedding {
        id: id.into(),
        vector: vector.into(),
    }
}
#[test]
fn scale_extremes_ties_and_weak_matches() {
    let mut g = Governor::new(Budget::default());
    let index = EmbeddingIndex::new(
        "space",
        &[
            entry("huge", &[1e308, 1e308]),
            entry("tiny", &[1e-308, 1e-308]),
            entry("other", &[-1.0, -1.0]),
        ],
        &mut g,
    )
    .unwrap();
    let Outcome::Ambiguous(values) = index
        .select(&[1.0, 1.0], EmbeddingPolicy::default(), &mut g)
        .unwrap()
    else {
        panic!("tie")
    };
    assert_eq!(
        values.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
        ["huge", "tiny"]
    );
    assert!(values.iter().all(|v| v.score > 0.999_999_999));
    let Outcome::BestGuess(value) = index
        .select(&[-1.0, -1.0], EmbeddingPolicy::default(), &mut g)
        .unwrap()
    else {
        panic!("guess")
    };
    assert_eq!(value.id, "other");
    assert_eq!(
        index
            .select(&[1.0, -1.0], EmbeddingPolicy::default(), &mut g)
            .unwrap(),
        Outcome::Unresolved
    );
    let near = EmbeddingIndex::new(
        "space",
        &[entry("runner", &[1.0, 0.1]), entry("best", &[1.0, 0.0])],
        &mut g,
    )
    .unwrap();
    assert!(matches!(
        near.select(&[1.0, 0.0], EmbeddingPolicy::default(), &mut g)
            .unwrap(),
        Outcome::Ambiguous(_)
    ));
    let Outcome::BestGuess(best) = near
        .select(
            &[1.0, 0.0],
            EmbeddingPolicy {
                minimum_margin: 0.001,
                ..EmbeddingPolicy::default()
            },
            &mut g,
        )
        .unwrap()
    else {
        panic!("margin")
    };
    assert_eq!(best.id, "best");
}
#[test]
fn invalid_vectors_and_exhaustion_never_return_partial_winners() {
    for candidates in [
        vec![entry("a", &[])],
        vec![entry("a", &[0.0])],
        vec![entry("a", &[f64::NAN])],
        vec![entry("a", &[f64::INFINITY])],
        vec![entry("a", &[1.0]), entry("a", &[2.0])],
        vec![entry("a", &[1.0]), entry("b", &[1.0, 2.0])],
    ] {
        assert!(matches!(
            EmbeddingIndex::new("space", &candidates, &mut Governor::new(Budget::default())),
            Err(StopReason::InvalidInput(_))
        ));
    }
    let index = EmbeddingIndex::new(
        "space",
        &[entry("a", &[1.0]), entry("b", &[-1.0])],
        &mut Governor::new(Budget::default()),
    )
    .unwrap();
    let mut tiny = Governor::new(Budget {
        candidates: 1,
        ..Budget::default()
    });
    assert_eq!(
        index.select(&[1.0], EmbeddingPolicy::default(), &mut tiny),
        Err(StopReason::BudgetExceeded(Resource::Candidates))
    );
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert_eq!(
        index.select(&[1.0], EmbeddingPolicy::default(), &mut cancelled),
        Err(StopReason::Cancelled)
    );
}
