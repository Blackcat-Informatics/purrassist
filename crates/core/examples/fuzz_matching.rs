// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated string-selection probe for the source-region fuzz
//! runner (`scripts/fuzz.py --target matching`).
//!
//! Bounded stdin is a UTF-8 text split on the ASCII record-separator
//! control byte (`\u{1e}`): the first field is the query, every remaining
//! nonempty field is one candidate spelling (identified by its position).
//! Checks: ranking and selection never panic; every ranked or selected
//! match's identity is among the supplied candidates; an `Ambiguous`
//! outcome's alternative count matches the receipt's alternative ledger
//! count (alternatives/outcome correspondence).
use purrassist_core::{
    Budget, Governor, Outcome,
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
};
use std::io::{self, Read};

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let Ok(input) = std::str::from_utf8(&bytes) else {
        println!("invalid-utf8");
        return;
    };
    let mut parts = input.split('\u{1e}');
    let query = parts.next().unwrap_or("");
    let candidates: Vec<Candidate> = parts
        .enumerate()
        .filter(|(_, text)| !text.is_empty())
        .map(|(index, text)| Candidate::new(index.to_string(), text))
        .collect();
    let request = MatchRequest {
        budget: Budget::default(),
        ..MatchRequest::default()
    };

    let mut rank_governor = Governor::new(request.budget);
    let ranked = matching::rank_governed(query, &candidates, &request, &mut rank_governor);
    match ranked.outcome {
        Outcome::Resolved(matches) => {
            for entry in &matches {
                assert!(
                    candidates
                        .iter()
                        .any(|candidate| candidate.id == entry.candidate.id),
                    "ranked match {:?} is not among the supplied candidates",
                    entry.candidate.id
                );
            }
        }
        Outcome::BestGuess(_) | Outcome::Ambiguous(_) | Outcome::Unresolved => {}
    }

    let mut resolve_governor = Governor::new(request.budget);
    let selected = matching::resolve_governed(
        query,
        &candidates,
        &request,
        SelectionPolicy::default(),
        &mut resolve_governor,
    );
    match selected.outcome {
        Outcome::Resolved(answer) | Outcome::BestGuess(answer) => {
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.id == answer.candidate.id),
                "selected match {:?} is not among the supplied candidates",
                answer.candidate.id
            );
        }
        Outcome::Ambiguous(answers) => {
            assert_eq!(answers.len(), selected.receipt.alternatives.len());
            for answer in &answers {
                assert!(
                    candidates
                        .iter()
                        .any(|candidate| candidate.id == answer.candidate.id),
                    "ambiguous selection alternative {:?} is not among the supplied candidates",
                    answer.candidate.id
                );
            }
        }
        Outcome::Unresolved => {}
    }
    println!("ok");
}
