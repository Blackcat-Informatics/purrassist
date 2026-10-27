// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! First-party lowering ownership and aggregate reference safety.

use purrassist_core::grammar::{Expr, Grammar, GrammarError};
use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use std::fmt::Write as _;

#[test]
fn lowering_resource_refusals_release_original_declarations_and_partial_expressions() {
    let source = r#"grammar "lowering";
reasoning "<t>" "</t>";
confusable "Ω" "O";
closer end = "</c>" | "</call>";
tool = "read";
call = "<c>" name:tool arguments:slot(json, "</c>") end;
result = "<r>" content:until("</r>") "</r>";
declaration = "<d>";
unused = mystery;"#;
    let mut measured = Governor::new(Budget::default());
    let error = Grammar::compile_governed(source, &mut measured).unwrap_err();
    assert_eq!(
        error,
        GrammarError::UnknownName {
            offset: source.find("mystery").unwrap(),
            name: "mystery".into()
        }
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
        (
            Budget {
                input_bytes: u64::try_from(source.len()).unwrap() - 1,
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
    ] {
        let mut g = Governor::new(budget);
        let cause = StopReason::BudgetExceeded(resource);
        assert_eq!(
            Grammar::compile_governed(source, &mut g).unwrap_err(),
            GrammarError::Resource(cause.clone())
        );
        assert_eq!(g.check(), Err(cause));
        assert_eq!(g.scratch_meter().live(), 0);
    }
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut g = Governor::with_cancellation(Budget::default(), cancellation);
    assert_eq!(
        Grammar::compile_governed(source, &mut g).unwrap_err(),
        GrammarError::Resource(StopReason::Cancelled)
    );
    assert_eq!(g.scratch_meter().live(), 0);
    let valid = source.replace("unused = mystery;", "unused = tool;");
    let mut g = Governor::new(Budget::default());
    let artifact = Grammar::compile_governed(&valid, &mut g).unwrap();
    let live = g.scratch_meter().live();
    assert!(artifact.production("tool").is_some());
    drop(artifact);
    // Backend tables retain their existing coarser charges, but new source
    // and expression owners must release at their actual artifact lifetime.
    assert!(g.scratch_meter().live() < live);
}

#[test]
fn cyclic_layouts_and_finished_dag_tails_are_checked_before_reference_traversal() {
    let source = r#"grammar "cycle"; layout a "x"; a = b; b = a; call = "<c>"; result = "<r>"; declaration = "<d>";"#;
    let mut g = Governor::new(Budget::default());
    assert_eq!(
        Grammar::compile_governed(source, &mut g).unwrap_err(),
        GrammarError::Recursive {
            offset: source.find("a =").unwrap(),
            name: "a".into()
        }
    );
    let valid = r#"grammar "layout"; atom = /x/; a = atom; layout a "x"; call = "<c>" a; result = "<r>"; declaration = "<d>";"#;
    let grammar = Grammar::compile_governed(valid, &mut Governor::new(Budget::default())).unwrap();
    let (a, _) = grammar.production("a").unwrap();
    assert_eq!(grammar.layout(a), Some(&b"x"[..]));
    assert!(matches!(grammar.call(), Expr::Sequence(items) if items.len() == 2));
    let large = Budget {
        depth: 4096,
        ..Budget::default()
    };
    let accepted = reference_source(47);
    Grammar::compile_governed(
        &accepted,
        &mut Governor::new(Budget {
            depth: 128,
            ..large
        }),
    )
    .unwrap();
    let mut caller = Governor::new(Budget {
        depth: 127,
        ..large
    });
    assert_eq!(
        Grammar::compile_governed(&accepted, &mut caller).unwrap_err(),
        GrammarError::Resource(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert_eq!(
        caller.check(),
        Err(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert_eq!(caller.scratch_meter().live(), 0);
    let refused = reference_source(48);
    let mut g = Governor::new(large);
    assert_eq!(
        Grammar::compile_governed(&refused, &mut g).unwrap_err(),
        GrammarError::Resource(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert_eq!(g.check(), Err(StopReason::BudgetExceeded(Resource::Depth)));
    assert_eq!(g.scratch_meter().live(), 0);
}

fn reference_source(prefix: usize) -> String {
    // The shared tail finishes first. A stack-length-only check misses its
    // contribution when a later root reuses it.
    let mut source =
        String::from("grammar \"dag\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";");
    for index in 0..80 {
        write!(source, "t{index} = t{};", index + 1).unwrap();
    }
    source.push_str("t80 = \"tail\";");
    for index in 0..prefix {
        if index + 1 == prefix {
            write!(source, "p{index} = t0;").unwrap();
        } else {
            write!(source, "p{index} = p{};", index + 1).unwrap();
        }
    }
    source
}
