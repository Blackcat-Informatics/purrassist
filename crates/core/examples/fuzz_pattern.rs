// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-isolated regex compile/match/generate probe for the source-region
//! fuzz runner (`scripts/fuzz.py --target pattern`).
//!
//! Bounded stdin is split on the ASCII record-separator control byte
//! (`\u{1e}`) into two UTF-8 fields: a pattern source and a subject text.
//! Checks: compilation never panics; the derivative and backtracking
//! engines agree whenever both can complete on the same (pattern, subject)
//! pair; any [`Pattern::generate`] witness matches its own pattern. Emits
//! one differential record per admitted, successfully matched execution for
//! `scripts/fuzz.py` to check against Node's `RegExp` (`u` mode), restricted
//! to the subset both implement (`scripts/fuzz.py` skips a construct Node
//! itself refuses to compile).
use purrassist_core::{
    Budget, Governor,
    json::quote,
    pattern::{GenerateOptions, GenerateOutcome, Pattern},
};
use std::io::{self, Read};

fn fields(input: &str) -> (&str, &str) {
    match input.split_once('\u{1e}') {
        Some((a, b)) => (a, b),
        None => (input, ""),
    }
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().take(4097).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096, "probe input exceeds 4096 bytes");
    let Ok(input) = std::str::from_utf8(&bytes) else {
        println!("invalid-utf8");
        return;
    };
    let (source, subject) = fields(input);
    let budget = Budget::default();
    let mut compile_governor = Governor::new(budget);
    let Ok(pattern) = Pattern::compile_governed(source, &mut compile_governor) else {
        println!("ok");
        return;
    };

    let mut derivative_governor = Governor::new(budget);
    let derivative = pattern.is_match(subject, &mut derivative_governor);
    let mut backtrack_governor = Governor::new(budget);
    let backtrack = pattern.is_match_backtracking(subject, &mut backtrack_governor);
    if let (Ok(a), Ok(b)) = (&derivative, &backtrack) {
        assert_eq!(
            a, b,
            "derivative and backtracking engines disagree on {source:?} / {subject:?}"
        );
    }

    let mut generate_governor = Governor::new(budget);
    if let Ok(GenerateOutcome::Witness(text)) = pattern.generate(
        &mut generate_governor,
        GenerateOptions {
            min_len: 0,
            max_len: Some(subject.chars().count().max(4)),
            avoid: &[],
        },
    ) {
        let mut check_governor = Governor::new(budget);
        assert_eq!(
            pattern.is_match(&text, &mut check_governor),
            Ok(true),
            "generated witness {text:?} does not match its own pattern {source:?}"
        );
    }

    println!("ok");
    if let Ok(expected) = &derivative {
        println!(
            "{{\"kind\":\"pattern\",\"pattern\":{},\"subject\":{},\"expected\":{expected}}}",
            quote(source),
            quote(subject),
        );
    }
}
