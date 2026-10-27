// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Actual AARE lowering, literal binding and byte-language witnesses.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    AppArmorGlob, GlobBindingProblem, GlobError, GlobInclusion, GlobLanguage, GlobLanguageError,
    GlobLanguageInput, LiteralPathBinding,
};

fn governor() -> Governor {
    Governor::new(Budget {
        work: 10_000_000,
        ..Budget::default()
    })
}
fn compile(sources: &[&str]) -> GlobLanguage {
    let globs: Vec<_> = sources
        .iter()
        .map(|source| AppArmorGlob::parse(source, &[], &mut governor()).expect("admitted source"))
        .collect();
    let inputs: Vec<_> = globs
        .iter()
        .map(|glob| GlobLanguageInput {
            glob,
            bindings: &[],
        })
        .collect();
    GlobLanguage::compile(&inputs, &mut governor()).expect("actual lowering")
}

#[test]
fn semantic_glob_lowering_preserves_byte_operators_classes_branches_and_empty_union() {
    for (source, yes, no) in [
        ("/?", &b"/\xff"[..], &b"/\xc3\xa9"[..]),
        ("/[é]", &b"/\xc3"[..], &b"/\xc3\xa9"[..]),
        ("/a*b", &b"/axxb"[..], &b"/a/b"[..]),
        ("/a**b", &b"/a/b"[..], &b"/a/bx"[..]),
        ("/[/]", &b"//"[..], &b"/x"[..]),
        ("/[^a]", &b"/\xff"[..], &b"/\0"[..]),
        ("/{,a{b,c}}", &b"/ab"[..], &b"/a"[..]),
        (r"/\*", &b"/*"[..], &b"/x"[..]),
        (r"/[\-a-z]", &b"/-"[..], &b"/A"[..]),
    ] {
        let language = compile(&[source]);
        assert!(
            language
                .accepts(yes, &mut governor())
                .expect("admitted path"),
            "{source}"
        );
        assert!(
            !language
                .accepts(no, &mut governor())
                .expect("neighbor refused"),
            "{source}"
        );
    }
    let nullable = compile(&["{,}"]);
    assert!(
        nullable
            .accepts(b"", &mut governor())
            .expect("empty brace alternative")
    );
    let union = compile(&["/x", "/y"]);
    assert!(
        union
            .accepts(b"/x", &mut governor())
            .expect("first alternative")
    );
    assert!(
        union
            .accepts(b"/y", &mut governor())
            .expect("second alternative")
    );
    assert!(
        !compile(&[])
            .accepts(b"", &mut governor())
            .expect("empty set is dead")
    );
}

#[test]
fn inclusion_and_literal_bindings_use_shared_validation_and_return_exact_witnesses() {
    let child = compile(&["/a/*", "/b/{x,y}"]);
    let parent = compile(&["/**"]);
    assert_eq!(
        child.included_in(&parent, &mut governor()),
        Ok(GlobInclusion::Contained)
    );
    let left = compile(&["/[ÿ]"]);
    let right = compile(&["/[Ã]"]);
    let GlobInclusion::Counterexample(witness) = left
        .included_in(&right, &mut governor())
        .expect("binary product search")
    else {
        panic!("uncovered byte");
    };
    assert_eq!(witness, b"/\xbf");
    assert!(std::str::from_utf8(&witness).is_err());
    assert!(
        left.accepts(&witness, &mut governor())
            .expect("left witness")
    );
    assert!(
        !right
            .accepts(&witness, &mut governor())
            .expect("right witness")
    );
    let glob =
        AppArmorGlob::parse("@{ROOT}/{,a}*", &["ROOT"], &mut governor()).expect("declared source");
    let valid = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/é work",
    }];
    let language = GlobLanguage::compile(
        &[GlobLanguageInput {
            glob: &glob,
            bindings: &valid,
        }],
        &mut governor(),
    )
    .expect("shared safe binding");
    assert!(
        language
            .accepts("/srv/é work/abc".as_bytes(), &mut governor())
            .expect("literal UTF-8 binding bytes")
    );
    assert_eq!(
        glob.bind_literal_paths(&valid, &mut governor())
            .expect("same printer validation"),
        "\"/srv/é work/{,a}*\""
    );
    for (bindings, expected) in [
        (vec![], GlobError::MissingBinding { offset: 0 }),
        (
            vec![LiteralPathBinding {
                name: "NO",
                path: "/x",
            }],
            GlobError::Binding {
                index: 0,
                offset: 0,
                problem: GlobBindingProblem::Undeclared,
            },
        ),
        (
            vec![valid[0], valid[0]],
            GlobError::Binding {
                index: 1,
                offset: 0,
                problem: GlobBindingProblem::Duplicate,
            },
        ),
    ] {
        assert_eq!(
            GlobLanguage::compile(
                &[GlobLanguageInput {
                    glob: &glob,
                    bindings: &bindings
                }],
                &mut governor()
            )
            .err(),
            Some(GlobLanguageError::Glob(expected.clone()))
        );
        assert_eq!(
            glob.bind_literal_paths(&bindings, &mut governor()).err(),
            Some(expected)
        );
    }
}

#[test]
fn lowering_resources_and_product_exhaustion_are_explicit_with_valid_neighbors() {
    let source = format!("/{}", "{a,b}".repeat(64));
    let glob = AppArmorGlob::parse(&source, &[], &mut governor()).expect("no eager expansion");
    let inputs = [GlobLanguageInput {
        glob: &glob,
        bindings: &[],
    }];
    let mut measured = governor();
    GlobLanguage::compile(&inputs, &mut measured).expect("measure lowering");
    let usage = measured.usage();
    for budget in [
        Budget {
            work: usage.work - 1,
            ..Budget::default()
        },
        Budget {
            scratch_bytes: usage.peak_scratch_bytes - 1,
            ..Budget::default()
        },
    ] {
        assert!(GlobLanguage::compile(&inputs, &mut Governor::new(budget)).is_err());
    }
    GlobLanguage::compile(
        &inputs,
        &mut Governor::new(Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            ..Budget::default()
        }),
    )
    .expect("exact lowering neighbor");
    let left = compile(&["/**"]);
    let right = compile(&["/*"]);
    assert_eq!(
        left.included_in(
            &right,
            &mut Governor::new(Budget {
                work: 0,
                ..Budget::default()
            })
        ),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        left.included_in(
            &right,
            &mut Governor::with_cancellation(Budget::default(), token)
        ),
        Err(StopReason::Cancelled)
    );
    let GlobInclusion::Counterexample(witness) = left
        .included_in(&right, &mut governor())
        .expect("valid search neighbor")
    else {
        panic!("recursive star is broader");
    };
    assert_eq!(witness, b"//");
    assert!(
        left.accepts(&witness, &mut governor())
            .expect("left acceptance")
    );
    assert!(
        !right
            .accepts(&witness, &mut governor())
            .expect("right refusal")
    );
}
