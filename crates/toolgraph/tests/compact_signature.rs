// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The compact signature lane over the worked example in `docs/tool-graph/TOOLS.md`.

use purrassist_core::{Budget, Governor, StopReason};
use purrassist_toolgraph::{
    PrefixTable, ProjectionTarget, SOURCE_REPRESENTATION, TERM_NAMES, TermBindings, ToolGraph,
    Vocabulary,
    codec::mcp::{self, CodecError},
    lanes, register_loss_profile,
};
use purrdf_core::loss::{LossLedger, LossProfile, LossSubject, LossTerm, check_ledger_sound_in};

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

fn load(turtle: &str) -> (Vocabulary, ToolGraph) {
    let vocabulary = vocabulary();
    let graph = ToolGraph::load(
        turtle.as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut Governor::new(Budget::default()),
    )
    .expect("graph loads");
    (vocabulary, graph)
}

fn project(vocabulary: &Vocabulary, graph: &ToolGraph, depth: u64) -> (String, LossLedger) {
    let (signature, ledger) = lanes::compact(
        graph,
        vocabulary,
        depth,
        &mut Governor::new(Budget::default()),
    )
    .expect("compact lane");
    check_ledger_sound_in(
        vocabulary.loss_profile(),
        &ledger,
        SOURCE_REPRESENTATION,
        ProjectionTarget::CompactSignature.representation(),
    )
    .expect("every entry is registered for compact-signature-v1");
    (signature.into_string(), ledger)
}

// Invariant: the signature for the worked example is byte-for-byte the document's
// golden (tools in name order, one LF per line) and every omitted source node of the
// documented loss table is ledgered with its exact source location.
#[test]
fn the_worked_example_signature_and_ledger_match_the_specification() {
    let (vocabulary, graph) = load(&worked());
    let (signature, ledger) = project(&vocabulary, &graph, 2);
    assert_eq!(signature, golden());
    let ex = |name: &str| Some(format!("https://example.org/tools/{name}"));
    let rows: Vec<(&str, Option<String>)> = ledger
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.code.as_ref(),
                match &entry.subject {
                    LossSubject::Term(LossTerm::Iri(iri)) => Some(iri.clone()),
                    _ => None,
                },
            )
        })
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

#[test]
fn compact_and_gbnf_are_identical_after_reordering_tool_declarations() {
    let source = worked();
    // Move the complete Turtle statements, including the anonymous FnO lists
    // and the multiline policy literals within them.
    let (header, query) = source
        .split_once("\nex:query_customer_orders a pa:Tool")
        .expect("first tool declaration");
    let (query, edit) = query
        .split_once("\nex:edit_file a pa:Tool")
        .expect("second tool declaration");
    let (edit, tail) = edit
        .split_once("\nex:customerParam a fno:Parameter")
        .expect("declarations end before the parameter statements");
    let reordered = format!(
        "{header}\nex:edit_file a pa:Tool{edit}\nex:query_customer_orders a pa:Tool{query}\nex:customerParam a fno:Parameter{tail}"
    );
    let (vocabulary, graph) = load(&source);
    let (reordered_vocabulary, reordered_graph) = load(&reordered);
    assert_eq!(
        project(&vocabulary, &graph, 2),
        project(&reordered_vocabulary, &reordered_graph, 2)
    );
    let project_grammars = |graph: &ToolGraph, vocabulary: &Vocabulary| {
        lanes::gbnf(graph, vocabulary, &mut Governor::new(Budget::default())).expect("GBNF lane")
    };
    assert_eq!(
        project_grammars(&graph, &vocabulary),
        project_grammars(&reordered_graph, &reordered_vocabulary)
    );
}

#[test]
fn compact_and_gbnf_read_only_the_projected_declarations() {
    let (vocabulary, graph) = load(&worked());
    let ample = || Governor::new(Budget::default());
    let (manifest, _) = mcp::encode(&graph, &vocabulary, &mut ample()).expect("manifest");
    let budget = Budget {
        input_bytes: u64::try_from(manifest.as_str().len() - 1).expect("test manifest size"),
        ..Budget::default()
    };
    let compact = lanes::compact(&graph, &vocabulary, 2, &mut ample()).expect("compact lane");
    let grammar = lanes::gbnf(&graph, &vocabulary, &mut ample()).expect("GBNF lane");
    assert_eq!(
        lanes::compact(&graph, &vocabulary, 2, &mut Governor::new(budget))
            .expect("declaration budget admits compact projection"),
        compact
    );
    assert_eq!(
        lanes::gbnf(&graph, &vocabulary, &mut Governor::new(budget))
            .expect("declaration budget admits GBNF projection"),
        grammar
    );
}

#[test]
fn cancelled_projection_refuses_even_without_tools() {
    let (vocabulary, graph) = load("");
    assert_eq!(graph.tools(), []);
    let mut governor = Governor::new(Budget::default());
    governor.cancel();
    assert!(matches!(
        lanes::compact(&graph, &vocabulary, 2, &mut governor),
        Err(lanes::CompactError::Manifest(cause))
            if matches!(*cause, CodecError::Resource(StopReason::Cancelled))
    ));
    assert!(matches!(
        lanes::gbnf(&graph, &vocabulary, &mut governor),
        Err(lanes::GbnfError::Manifest(cause))
            if matches!(*cause, CodecError::Resource(StopReason::Cancelled))
    ));
}

// Invariant: a depth too small to expand the result object renders `object` and
// ledgers the cut, so the signature never silently loses nested detail.
#[test]
fn a_result_object_beyond_the_depth_is_ledgered() {
    let (vocabulary, graph) = load(&worked());
    let (signature, ledger) = project(&vocabulary, &graph, 0);
    assert!(signature.contains("->object\n"));
    assert!(
        ledger
            .entries()
            .iter()
            .any(|entry| entry.code == "schema-keyword-not-expressible"
                && entry.subject
                    == LossSubject::Term(LossTerm::Iri(
                        "https://example.org/tools/QueryOutput".to_owned()
                    )))
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
