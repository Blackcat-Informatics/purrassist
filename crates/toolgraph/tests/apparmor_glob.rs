// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Parsed byte semantics, safe policy words and bounded nonexpanding storage.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    AppArmorGlob, GlobBindingProblem, GlobError, GlobTokenKind, LiteralPathBinding,
    literal_path_word,
};

fn governor() -> Governor {
    Governor::new(Budget::default())
}

#[test]
fn semantic_arena_preserves_bytes_escapes_classes_and_nested_empty_alternatives() {
    let source = r#"@{ROOT}/é/{,a{b,c}}/*/**/?/[^\-a-z0-9!é\]]/\*\?\{\}\[\]\\\""#;
    let glob = AppArmorGlob::parse(source, &["ROOT", "UNUSED"], &mut governor())
        .expect("admitted full syntax subset");
    assert_eq!(glob.source(), source);
    assert_eq!(
        glob.to_policy_word(&mut governor()).expect("quoted word"),
        format!("\"{source}\"")
    );
    assert_eq!(glob.declaration(0), Some("ROOT"));
    assert!(
        glob.tokens()
            .iter()
            .any(|token| matches!(token.kind, GlobTokenKind::Variable { declaration: 0 }))
    );
    assert!(
        glob.tokens()
            .iter()
            .any(|token| matches!(token.kind, GlobTokenKind::Star))
    );
    assert!(
        glob.tokens()
            .iter()
            .any(|token| matches!(token.kind, GlobTokenKind::RecursiveStar))
    );
    assert!(
        glob.tokens()
            .iter()
            .any(|token| matches!(token.kind, GlobTokenKind::AnyByte))
    );
    let class = glob
        .tokens()
        .iter()
        .position(|token| matches!(token.kind, GlobTokenKind::ClassStart { .. }))
        .expect("class");
    let GlobTokenKind::ClassStart { negated: true, end } = glob.tokens()[class].kind else {
        panic!("caret class negation must be semantic");
    };
    let atoms: Vec<_> = glob.tokens()[class + 1..end]
        .iter()
        .map(|token| token.kind.clone())
        .collect();
    assert_eq!(
        atoms,
        [
            GlobTokenKind::Byte(b'-'),
            GlobTokenKind::ByteRange {
                first: b'a',
                last: b'z'
            },
            GlobTokenKind::ByteRange {
                first: b'0',
                last: b'9'
            },
            GlobTokenKind::Byte(b'!'),
            GlobTokenKind::Byte(0xc3),
            GlobTokenKind::Byte(0xa9),
            GlobTokenKind::Byte(b']'),
        ]
    );
    assert_eq!(
        glob.tokens()[end].kind,
        GlobTokenKind::ClassEnd { start: class }
    );
    for (index, token) in glob.tokens().iter().enumerate() {
        if let GlobTokenKind::AlternationStart { end } = token.kind {
            assert_eq!(
                glob.tokens()[end].kind,
                GlobTokenKind::AlternationEnd { start: index }
            );
            assert!(
                glob.tokens()[index + 1..end]
                    .iter()
                    .any(|token| token.kind == GlobTokenKind::Branch { start: index })
            );
        }
    }
}

#[test]
fn malformed_unsupported_and_undeclared_sources_report_original_byte_offsets() {
    for (source, expected) in [
        ("", GlobError::Syntax { offset: 0 }),
        ("/tmp/[", GlobError::Syntax { offset: 6 }),
        ("/tmp/[]", GlobError::Syntax { offset: 6 }),
        ("/tmp/[z-a]", GlobError::Syntax { offset: 6 }),
        ("/tmp/[a-]", GlobError::Syntax { offset: 7 }),
        ("/tmp/[-a]", GlobError::Unsupported { offset: 6 }),
        (r"/tmp/[\^a]", GlobError::Unsupported { offset: 6 }),
        (r"/tmp/[a\-z]", GlobError::Unsupported { offset: 7 }),
        (r"/tmp/[a\-]", GlobError::Unsupported { offset: 7 }),
        (r"/tmp/[\--z]", GlobError::Unsupported { offset: 6 }),
        ("/tmp/[@{ROOT}]", GlobError::Unsupported { offset: 6 }),
        (r"/tmp/\x41", GlobError::Unsupported { offset: 5 }),
        (r"/tmp/\1", GlobError::Unsupported { offset: 5 }),
        ("/tmp/\\", GlobError::Syntax { offset: 6 }),
        ("/tmp/{a}", GlobError::Syntax { offset: 7 }),
        ("/tmp/{a,b", GlobError::Syntax { offset: 9 }),
        ("/tmp/a,b", GlobError::Syntax { offset: 6 }),
        ("/tmp/a}", GlobError::Syntax { offset: 6 }),
        ("/tmp/a]", GlobError::Syntax { offset: 6 }),
        ("/tmp/\" r, /etc/** rw,", GlobError::Syntax { offset: 5 }),
        ("/tmp/a\n", GlobError::Syntax { offset: 6 }),
        ("/tmp/é/@{NO}", GlobError::UndeclaredVariable { offset: 8 }),
        ("/tmp/@{1BAD}", GlobError::Syntax { offset: 7 }),
    ] {
        assert_eq!(
            AppArmorGlob::parse(source, &["ROOT"], &mut governor()).err(),
            Some(expected),
            "source {source:?}"
        );
    }
    for source in [
        r"/tmp/\*",
        r"/tmp/[\-a]",
        "/tmp/[a-z]",
        "/tmp/[!a]",
        "/tmp/{,a}",
        "/tmp/{a,{b,c}}",
        "@{ROOT}/é",
    ] {
        AppArmorGlob::parse(source, &["ROOT"], &mut governor()).expect("valid syntax neighbor");
    }
    for (names, index, offset) in [
        (&["ROOT", "ROOT"][..], 1, 0),
        (&["1ROOT"][..], 0, 0),
        (&["ROOT-OTHER"][..], 0, 4),
    ] {
        assert_eq!(
            AppArmorGlob::parse("/tmp/*", names, &mut governor()).err(),
            Some(GlobError::Declaration { index, offset })
        );
    }
}

#[test]
fn literal_bindings_refuse_pattern_injection_and_incomplete_or_extraneous_sets() {
    let glob = AppArmorGlob::parse(
        "@{ROOT}/{,cache/}**/@{ROOT}",
        &["ROOT", "UNUSED"],
        &mut governor(),
    )
    .expect("declared glob");
    let binding = |name, path| LiteralPathBinding { name, path };
    assert_eq!(
        glob.bind_literal_paths(&[binding("ROOT", "/srv/é work")], &mut governor())
            .expect("safe literal substitution"),
        "\"/srv/é work/{,cache/}**//srv/é work\""
    );
    assert_eq!(
        literal_path_word("/srv/é work", &mut governor()).expect("same literal validator"),
        "\"/srv/é work\""
    );
    for (bindings, problem, index, offset) in [
        (
            vec![binding("1BAD", "/x")],
            GlobBindingProblem::InvalidName,
            0,
            0,
        ),
        (
            vec![binding("NO", "/x")],
            GlobBindingProblem::Undeclared,
            0,
            0,
        ),
        (
            vec![binding("ROOT", "/x"), binding("ROOT", "/y")],
            GlobBindingProblem::Duplicate,
            1,
            0,
        ),
        (
            vec![binding("ROOT", "/x"), binding("UNUSED", "/y")],
            GlobBindingProblem::Unused,
            1,
            0,
        ),
        (
            vec![binding("ROOT", "relative")],
            GlobBindingProblem::NotAbsolute,
            0,
            0,
        ),
        (
            vec![binding("ROOT", "/x/*")],
            GlobBindingProblem::UnsafePath,
            0,
            3,
        ),
        (
            vec![binding("ROOT", "/x/{a,b}")],
            GlobBindingProblem::UnsafePath,
            0,
            3,
        ),
        (
            vec![binding("ROOT", "/x/@{HOME}")],
            GlobBindingProblem::UnsafePath,
            0,
            3,
        ),
        (
            vec![binding("ROOT", "/x/\" r, /etc/** rw,")],
            GlobBindingProblem::UnsafePath,
            0,
            3,
        ),
        (
            vec![binding("ROOT", "/x/\n")],
            GlobBindingProblem::UnsafePath,
            0,
            3,
        ),
    ] {
        assert_eq!(
            glob.bind_literal_paths(&bindings, &mut governor()),
            Err(GlobError::Binding {
                index,
                offset,
                problem
            })
        );
    }
    assert_eq!(
        glob.bind_literal_paths(&[], &mut governor()),
        Err(GlobError::MissingBinding { offset: 0 })
    );
    for source in ["/x/{", "/x/*", "/x/@{", "/x/[", "/x/?", "/x/\\"] {
        assert!(
            literal_path_word(source, &mut governor()).is_err(),
            "unsafe literal {source:?}"
        );
    }
}

fn assert_word_limits(run: impl Fn(&mut Governor) -> Result<String, GlobError>) {
    let mut measured = governor();
    let output = run(&mut measured).expect("measure successful word");
    assert!(
        u64::try_from(output.capacity()).expect("capacity fits counter")
            <= measured.scratch_meter().live()
    );
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
            Err(GlobError::Resource(StopReason::BudgetExceeded(resource)))
        );
    }
    assert_eq!(
        run(&mut Governor::new(Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            ..Budget::default()
        }))
        .expect("exact word neighbor"),
        output
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        run(&mut Governor::with_cancellation(
            Budget::default(),
            cancelled
        )),
        Err(GlobError::Resource(StopReason::Cancelled))
    );
}

#[test]
fn resource_limits_and_cancellation_refuse_models_and_words_without_partial_success() {
    let source = "@{ROOT}/{a,{b,c}}/[^a-z]?";
    let mut measured = governor();
    let glob = AppArmorGlob::parse(source, &["ROOT"], &mut measured).expect("measure parser");
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
        (
            Budget {
                depth: 1,
                ..Budget::default()
            },
            Resource::Depth,
        ),
        (
            Budget {
                input_bytes: u64::try_from(source.len() - 1).expect("small input"),
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
    ] {
        assert_eq!(
            AppArmorGlob::parse(source, &["ROOT"], &mut Governor::new(budget)).err(),
            Some(GlobError::Resource(StopReason::BudgetExceeded(resource)))
        );
    }
    AppArmorGlob::parse(
        source,
        &["ROOT"],
        &mut Governor::new(Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            depth: 2,
            ..Budget::default()
        }),
    )
    .expect("exact successful parser neighbor");
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/work",
    }];
    for operation in [0, 1, 2] {
        let run = |governor: &mut Governor| match operation {
            0 => glob.to_policy_word(governor),
            1 => glob.bind_literal_paths(&bindings, governor),
            _ => literal_path_word("/srv/work", governor),
        };
        assert_word_limits(run);
    }
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        AppArmorGlob::parse(
            source,
            &["ROOT"],
            &mut Governor::with_cancellation(Budget::default(), cancelled)
        )
        .err(),
        Some(GlobError::Resource(StopReason::Cancelled))
    );
}

#[test]
fn deep_and_wide_alternatives_keep_linear_flat_storage_without_expansion() {
    for source in [
        format!("/{}x{}", "{a,".repeat(4096), "}".repeat(4096)),
        format!("/{}", "{a,b}".repeat(4096)),
    ] {
        let mut governor = Governor::new(Budget {
            depth: 4096,
            ..Budget::default()
        });
        let glob = AppArmorGlob::parse(&source, &[], &mut governor)
            .expect("iterative nonexpanding syntax");
        assert!(glob.tokens().len() <= source.len() * 2);
        assert!(governor.usage().work <= u64::try_from(source.len() * 20).expect("test work fits"));
        assert_eq!(
            glob.to_policy_word(&mut governor).expect("flat printer"),
            format!("\"{source}\"")
        );
        drop(glob); // Recursive ownership would also make destruction unsafe here.
    }
}
