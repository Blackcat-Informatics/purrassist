// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The compact signature lane over the worked example in `docs/tool-graph/TOOLS.md`.

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::{
    PrefixTable, ProjectionTarget, SOURCE_REPRESENTATION, TERM_NAMES, TermBindings, ToolGraph,
    Vocabulary, lanes, register_loss_profile,
};
use purrdf_core::loss::{LossProfile, check_ledger_sound_in};

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../docs/tool-graph/TOOLS.md");

fn vocabulary() -> Vocabulary {
    Vocabulary::new(
        "pa",
        PrefixTable(vec![("pa".into(), NAMESPACE.into())]),
        TermBindings {
            namespace_iri: NAMESPACE.into(),
            local_names: TERM_NAMES
                .iter()
                .map(|name| ((*name).into(), format!("{NAMESPACE}{name}")))
                .collect(),
        },
        register_loss_profile(LossProfile::builtin()).expect("loss profile"),
    )
    .expect("worked vocabulary")
}

/// The Turtle fence of the "End-to-end example" section, read from the document itself.
fn worked() -> String {
    let section = TOOLS
        .split("## End-to-end example")
        .nth(1)
        .expect("worked example section");
    let fence = section.split("```turtle\n").nth(1).expect("turtle fence");
    fence
        .split("\n```")
        .next()
        .expect("closed fence")
        .to_owned()
}

/// The text fence that follows the compact lane's statement, read from the document.
fn golden() -> String {
    let section = TOOLS
        .split("`CompactSignature` for the example")
        .nth(1)
        .expect("compact lane statement");
    let fence = section.split("```text\n").nth(1).expect("text fence");
    let text = fence.split("```").next().expect("closed fence").to_owned();
    assert!(text.ends_with('\n'), "the golden ends in one LF");
    assert_eq!(text.lines().count(), 2);
    text
}

fn project(depth: u64) -> (String, Vec<(String, Option<String>)>) {
    let vocabulary = vocabulary();
    let mut governor = Governor::new(Budget::default());
    let graph = ToolGraph::load(
        worked().as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut governor,
    )
    .expect("graph loads");
    let (signature, ledger) =
        lanes::compact(&graph, &vocabulary, depth, &mut governor).expect("compact lane");
    check_ledger_sound_in(
        vocabulary.loss_profile(),
        &ledger,
        SOURCE_REPRESENTATION,
        ProjectionTarget::CompactSignature.representation(),
    )
    .expect("every entry is registered for compact-signature-v1");
    let entries = ledger
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.code.to_string(),
                entry.location.as_ref().and_then(|at| at.subject.clone()),
            )
        })
        .collect();
    (signature.into_string(), entries)
}

// Invariant: the signature for the worked example is byte-for-byte the document's
// golden (tools in name order, one LF per line) and every omitted source node of the
// documented loss table is ledgered with its exact source location.
#[test]
fn the_worked_example_signature_and_ledger_match_the_specification() {
    let (signature, entries) = project(2);
    assert_eq!(signature, golden());
    let ex = |name: &str| Some(format!("https://example.org/tools/{name}"));
    let rows: Vec<(&str, Option<String>)> = entries
        .iter()
        .map(|(code, subject)| (code.as_str(), subject.clone()))
        .collect();
    let expected = [
        ("authorization-not-expressible", ex("edit_file")),
        ("recovery-hints-not-expressible", ex("pathClaim")),
        ("taint-not-expressible", ex("pathClaim")),
    ];
    // Per tool, rows follow the documented order; check the edit_file prefix and the
    // query_customer_orders rows that carry hints.
    assert_eq!(rows[0], expected[0]);
    let codes: Vec<&str> = rows.iter().map(|(code, _)| *code).collect();
    for code in [
        "session-binding-not-expressible",
        "confinement-not-expressible",
        "resource-ceiling-not-expressible",
        "context-contract-not-expressible",
        "tool-annotations-not-expressible",
        "rdf-binding-not-expressible",
        "description-not-expressible",
        "schema-keyword-not-expressible",
    ] {
        assert!(codes.contains(&code), "{code}");
    }
    assert!(rows.contains(&("session-binding-not-expressible", ex("customerClaim"))));
    assert!(rows.contains(&("taint-not-expressible", ex("customerClaim"))));
    assert!(rows.contains(&expected[1]) && rows.contains(&expected[2]));
    assert!(rows.contains(&("schema-keyword-not-expressible", ex("QueryInput"))));
    assert!(rows.contains(&("schema-keyword-not-expressible", ex("EditOutput"))));
}

// Invariant: a depth too small to expand the result object renders `object` and
// ledgers the cut, so the signature never silently loses nested detail.
#[test]
fn a_result_object_beyond_the_depth_is_ledgered() {
    let (signature, entries) = project(0);
    assert!(signature.contains("->object\n"));
    assert!(
        entries
            .iter()
            .any(|(code, subject)| code == "schema-keyword-not-expressible"
                && subject.as_deref() == Some("https://example.org/tools/QueryOutput"))
    );
}

// Invariant: lines follow tool-name order even when the graph's IRI order is the
// reverse, and each line carries its own tool's schemas.
#[test]
fn signature_lines_follow_name_order_not_graph_order() {
    let renamed = worked().replace("pa:name \"edit_file\"", "pa:name \"zz_edit_file\"");
    assert_ne!(renamed, worked(), "the fixture renames edit_file");
    let vocabulary = vocabulary();
    let mut governor = Governor::new(Budget::default());
    let graph = ToolGraph::load(
        renamed.as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut governor,
    )
    .expect("graph loads");
    let (signature, _) =
        lanes::compact(&graph, &vocabulary, 2, &mut governor).expect("compact lane");
    let lines: Vec<&str> = signature.as_str().lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0].starts_with("query_customer_orders(customer_id:string,limit:integer)->")
            && lines[0].contains("orders:string"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("zz_edit_file(path:string,text:string)->")
            && lines[1].contains("version:string"),
        "{}",
        lines[1]
    );
}
