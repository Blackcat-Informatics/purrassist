// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! `ToolGraph::tool_set` over the worked example in `docs/tool-graph/TOOLS.md`.

use purrassist_core::{
    Budget, Governor, Outcome, json,
    tool_call::{
        CallOutcome, DeclarationForm, DialectSet, Effects, RepairPolicy, ToolRecoveryRequest,
        ToolSet, ToolSetError, UnresolvedReason,
    },
};
use purrassist_toolgraph::{
    PrefixTable, TERM_NAMES, TermBindings, ToolGraph, ToolSetBridgeError, Vocabulary,
    register_loss_profile,
};
use purrdf_core::loss::LossProfile;

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../docs/tool-graph/TOOLS.md");
/// The encoder's pinned manifest for the worked example (the byte source of `tool_set`).
const MANIFEST: &str = include_str!("data/worked_manifest.json");

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

fn edited(from: &str, to: &str) -> String {
    let source = worked();
    assert!(source.contains(from), "{from}");
    source.replacen(from, to, 1)
}

fn tool_set(source: &str) -> Result<ToolSet, ToolSetBridgeError> {
    let vocabulary = vocabulary();
    let mut governor = Governor::new(Budget::default());
    let graph = ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut governor,
    )
    .expect("graph loads");
    graph
        .tool_set(&vocabulary, &mut governor)
        .map(|(tools, _)| tools)
}

// Rationale: a reified alias must resolve to its canonical parameter through real
// recovery, and a value the unchanged schema forbids must still be refused after
// the rename. Both run against the exact schema, not a rebuilt subset.
#[test]
fn a_reified_alias_recovers_and_the_exact_schema_still_validates() {
    let tools = tool_set(&worked()).expect("tool set");
    let output = br#"<tool_call>{"name":"query_customer_orders","arguments":{"customer":"customer-17","limit":5}}</tool_call>"#;
    let result = tools.recover(
        output,
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let Outcome::Resolved(calls) = result.outcome else {
        panic!("{:?}", result.outcome);
    };
    let call = &calls.calls[0];
    assert_eq!(
        call.arguments.to_json(),
        r#"{"customer_id":"customer-17","limit":5}"#
    );

    let forbidden = br#"<tool_call>{"name":"query_customer_orders","arguments":{"customer":"someone-else","limit":5}}</tool_call>"#;
    let refused = tools.recover(
        forbidden,
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let Outcome::Resolved(refused_calls) = refused.outcome else {
        panic!("{:?}", refused.outcome);
    };
    assert!(
        matches!(
            refused_calls.calls[0].outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::SchemaValidation
            }
        ),
        "the pattern constraint must hold through the alias: {:?}",
        refused_calls.calls[0].outcome
    );

    let schema = tools.tool("query_customer_orders").expect("tool").schema();
    let check = |arguments: &str| {
        schema
            .validate(&json::parse(arguments).expect("json"))
            .expect("validation runs")
            .valid
    };
    assert!(check(r#"{"customer_id":"customer-17","limit":5}"#));
    assert!(
        !check(r#"{"customer_id":"someone-else","limit":5}"#),
        "pattern"
    );
    assert!(
        !check(r#"{"customer_id":"customer-17","limit":0}"#),
        "minimum"
    );
}

// Rationale: the tool's source declaration is the manifest bytes, so render-back
// returns the original text, not a re-serialisation of a typed model.
#[test]
fn the_original_declaration_renders_back_byte_for_byte() {
    let tools = tool_set(&worked()).expect("tool set");
    for name in ["edit_file", "query_customer_orders"] {
        let rendered = tools
            .render_declaration(name, DeclarationForm::Mcp)
            .expect("render");
        assert!(
            MANIFEST.contains(rendered.as_str()),
            "{name}: the rendered declaration is not a verbatim slice of the encoded manifest"
        );
        assert!(rendered.starts_with(&format!(r#"{{"name":"{name}","#)));
    }
}

// Rationale: the hint alone never grants ReadOnly, and a tolerant hint selects the
// tolerant policy. The worked query is read-only with a coercion hint; the edit
// tool is mutating with none; a write grant on the query profile revokes ReadOnly
// although its readOnlyHint stays true.
#[test]
fn effects_and_policy_follow_the_hint_and_the_proven_capabilities() {
    let tools = tool_set(&worked()).expect("tool set");
    let query = tools.tool("query_customer_orders").expect("query");
    let edit = tools.tool("edit_file").expect("edit");
    assert_eq!(
        (query.effects(), query.policy()),
        (Effects::ReadOnly, RepairPolicy::Tolerant)
    );
    assert_eq!(
        (edit.effects(), edit.policy()),
        (Effects::Mutating, RepairPolicy::Structural)
    );

    let granted = edited(
        r#"ex:TemporaryRead a pa:FileRule ; pa:pathGlob "@{TOOL_TMP}/**" ; pa:filePermissions "r""#,
        r#"ex:TemporaryRead a pa:FileRule ; pa:pathGlob "@{TOOL_TMP}/**" ; pa:filePermissions "rw""#,
    );
    let tools = tool_set(&granted).expect("tool set");
    let query = tools.tool("query_customer_orders").expect("query");
    assert_eq!(query.effects(), Effects::Mutating);
}

// Rationale: an alias equal to a sibling parameter is ambiguous and must be a typed
// collision from the core adapter, not a silent override.
#[test]
fn an_alias_that_collides_with_a_sibling_parameter_is_refused() {
    let source = format!(
        "{}\n<< ex:textShape sh:path ex:text ~ ex:textClaim >> .\nex:textClaim a pa:Hint ; \
         pa:maxEditDistance 0 ; pa:alias \"path\" ; pa:typeCoercion \"none\" ; \
         pa:caseFold false ; pa:unicodeFold false .\n",
        worked()
    );
    let refused = tool_set(&source).expect_err("collision");
    assert!(
        matches!(
            refused,
            ToolSetBridgeError::ToolSet(ToolSetError::Collision { .. })
        ),
        "{refused:?}"
    );
}

// Rationale: ReadOnly covers everything a tool can call. A read-only tool whose
// subtool grants a write is Mutating, though its own profile is write-free.
#[test]
fn a_subtool_that_can_write_revokes_read_only() {
    let nested = edited(
        " pa:inputShape ex:QueryInput ;",
        " pa:subtool ex:edit_file ; pa:inputShape ex:QueryInput ;",
    );
    let tools = tool_set(&nested).expect("tool set");
    let query = tools.tool("query_customer_orders").expect("query");
    assert_eq!(query.effects(), Effects::Mutating);
}
