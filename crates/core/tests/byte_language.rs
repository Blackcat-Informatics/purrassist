// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Direct byte programs, exact inclusion and explicit resource outcomes.

use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    pattern::{ByteCompileError, ByteInclusion, ByteInstruction as I, ByteLanguage, Pattern},
};

fn governor() -> Governor {
    Governor::new(Budget::default())
}
fn language(code: &[I]) -> ByteLanguage {
    ByteLanguage::compile(code, &mut governor()).expect("valid byte fixture")
}

#[test]
fn byte_programs_match_complete_binary_strings_without_changing_unicode_search() {
    let byte = language(&[I::Literal(0xff)]);
    assert!(byte.accepts(&[0xff], &mut governor()).expect("raw byte"));
    for input in [&[][..], &[0xff, 0][..], &[0, 0xff][..], &[0xc3, 0xbf][..]] {
        assert!(!byte.accepts(input, &mut governor()).expect("full matching"));
    }
    let nul = language(&[I::Literal(0)]);
    assert!(
        nul.accepts(&[0], &mut governor())
            .expect("core alphabet includes zero")
    );
    let all = language(&[I::Class([u64::MAX; 4])]);
    for byte in [0, 1, 63, 64, 127, 128, 191, 192, 254, 255] {
        assert!(
            all.accepts(&[byte], &mut governor())
                .expect("bit-set boundaries")
        );
    }
    let compound = language(&[
        I::Literal(b'a'),
        I::Literal(b'b'),
        I::Alternate(2),
        I::Star,
        I::Literal(255),
        I::Concat(2),
    ]);
    assert!(
        compound
            .accepts(b"abba\xff", &mut governor())
            .expect("union/star/concat")
    );
    assert!(
        !compound
            .accepts(b"abba\xffx", &mut governor())
            .expect("prefix is not acceptance")
    );
    let empty = language(&[I::Empty]);
    let dead = language(&[I::Dead]);
    assert!(empty.accepts(b"", &mut governor()).expect("nullable root"));
    assert!(!dead.accepts(b"", &mut governor()).expect("dead root"));
    assert_eq!(
        empty.included_in(&dead, &mut governor()),
        Ok(ByteInclusion::Counterexample(vec![]))
    );
    assert_eq!(
        dead.included_in(&empty, &mut governor()),
        Ok(ByteInclusion::Contained)
    );
    let unicode = Pattern::compile("é").expect("existing Unicode pattern");
    assert!(
        unicode
            .is_match("xéy", &mut governor())
            .expect("existing search semantics")
    );
    let mut g = governor();
    let mut dfa = unicode
        .byte_dfa(&mut g)
        .expect("existing UTF-8 driver")
        .expect("regular");
    let invalid = dfa
        .step(dfa.start(), 255, &mut g)
        .expect("invalid byte state");
    assert!(
        !dfa.accepting(invalid, &mut g)
            .expect("Unicode still rejects malformed UTF-8")
    );
}

#[test]
fn postfix_errors_and_logical_depth_are_independent_of_operand_width() {
    for (code, expected) in [
        (vec![], ByteCompileError::Roots { remaining: 0 }),
        (
            vec![I::Empty, I::Dead],
            ByteCompileError::Roots { remaining: 2 },
        ),
        (
            vec![I::Star],
            ByteCompileError::Arity {
                instruction: 0,
                required: 1,
                available: 0,
            },
        ),
        (
            vec![I::Empty, I::Concat(2)],
            ByteCompileError::Arity {
                instruction: 1,
                required: 2,
                available: 1,
            },
        ),
        (
            vec![I::Alternate(0)],
            ByteCompileError::Arity {
                instruction: 0,
                required: 0,
                available: 0,
            },
        ),
    ] {
        assert_eq!(
            ByteLanguage::compile(&code, &mut governor()).err(),
            Some(expected)
        );
    }
    let mut wide = vec![I::Literal(b'a'); 64];
    wide.push(I::Concat(64));
    let compiled = ByteLanguage::compile(
        &wide,
        &mut Governor::new(Budget {
            depth: 1,
            ..Budget::default()
        }),
    )
    .expect("one flat sequence level");
    assert!(
        compiled
            .accepts(
                &[b'a'; 64],
                &mut Governor::new(Budget {
                    work: 50_000_000,
                    ..Budget::default()
                })
            )
            .expect("wide sequence full match")
    );
    let nested = [I::Literal(b'a'), I::Star, I::Star, I::Star];
    assert_eq!(
        ByteLanguage::compile(
            &nested,
            &mut Governor::new(Budget {
                depth: 2,
                ..Budget::default()
            })
        )
        .err(),
        Some(ByteCompileError::Resource(StopReason::BudgetExceeded(
            Resource::Depth
        )))
    );
    ByteLanguage::compile(
        &nested,
        &mut Governor::new(Budget {
            depth: 3,
            ..Budget::default()
        }),
    )
    .expect("exact logical depth neighbor");
}

#[test]
fn product_search_returns_shortest_binary_witnesses_and_checks_initial_acceptance() {
    let left = language(&[I::Literal(255), I::Literal(128), I::Alternate(2)]);
    let dead = language(&[I::Dead]);
    let right = language(&[I::Literal(128)]);
    for (parent, expected) in [(&dead, vec![128]), (&right, vec![255])] {
        let ByteInclusion::Counterexample(witness) = left
            .included_in(parent, &mut governor())
            .expect("completed product search")
        else {
            panic!("left has uncovered bytes");
        };
        assert_eq!(witness, expected);
        assert!(
            left.accepts(&witness, &mut governor())
                .expect("witness left acceptance")
        );
        assert!(
            !parent
                .accepts(&witness, &mut governor())
                .expect("witness right refusal")
        );
    }
    let repeated = language(&[I::Literal(128), I::Star]);
    assert_eq!(
        right.included_in(&repeated, &mut governor()),
        Ok(ByteInclusion::Contained)
    );
    assert_eq!(
        repeated.included_in(&right, &mut governor()),
        Ok(ByteInclusion::Counterexample(vec![]))
    );
}

fn check_operation_bounds(run: impl Fn(&mut Governor) -> Result<(), StopReason>) {
    let mut measured = governor();
    run(&mut measured).expect("measure successful operation");
    let usage = measured.usage();
    for (budget, resource) in [
        (
            Budget {
                work: usage.work - 1,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
    ] {
        assert_eq!(
            run(&mut Governor::new(budget)),
            Err(StopReason::BudgetExceeded(resource))
        );
    }
    run(&mut Governor::new(Budget {
        work: usage.work,
        scratch_bytes: usage.peak_scratch_bytes,
        ..Budget::default()
    }))
    .expect("exact resource neighbor");
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        run(&mut Governor::with_cancellation(Budget::default(), token)),
        Err(StopReason::Cancelled)
    );
}

#[test]
fn compile_matching_and_search_refuse_resource_exhaustion_without_success() {
    let code = [I::Literal(b'a'), I::Star];
    check_operation_bounds(|g| {
        ByteLanguage::compile(&code, g)
            .map(|_| ())
            .map_err(|error| match error {
                ByteCompileError::Resource(reason) => reason,
                _ => panic!("fixture syntax is valid"),
            })
    });
    let repeated = language(&code);
    let one = language(&[I::Literal(b'a')]);
    check_operation_bounds(|g| repeated.accepts(b"aaaa", g).map(|_| ()));
    check_operation_bounds(|g| one.included_in(&repeated, g).map(|_| ()));
    check_operation_bounds(|g| repeated.included_in(&one, g).map(|_| ()));
    let two = language(&[I::Literal(b'a'), I::Literal(b'b'), I::Concat(2)]);
    let dead = language(&[I::Dead]);
    assert_eq!(
        two.included_in(
            &dead,
            &mut Governor::new(Budget {
                depth: 1,
                ..Budget::default()
            })
        ),
        Err(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert_eq!(
        two.included_in(
            &dead,
            &mut Governor::new(Budget {
                depth: 2,
                ..Budget::default()
            })
        ),
        Ok(ByteInclusion::Counterexample(b"ab".to_vec()))
    );
}
