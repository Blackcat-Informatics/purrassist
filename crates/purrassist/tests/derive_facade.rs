// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The facade flavor of the derives: `#[derive(purrassist::ToolDefinition)]`
//! with no `crate =` attribute, generating `::purrassist::__core` paths; and
//! `#[tool(crate = ..)]` replacing that prefix. The facade re-exports core's
//! `tool_call` and `json` modules at its root, so `::purrassist` is itself a
//! valid `crate =` path, as is a module that re-exports the facade.
#![allow(clippy::redundant_pub_crate)] // with `unreachable_pub`, `pub(crate)` wins

use purrassist::json::{self, Node};
use purrassist::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, FromNode, FromNodeError, Param, ToolArguments,
    ToolDefinition as Definition, ToolParameter, ToolRecoveryRequest, ToolSet,
};
use purrassist::{Budget, Governor, Outcome};

#[derive(purrassist::ToolEnum, Debug, PartialEq)]
enum Unit {
    Px,
    #[param(name = "em")]
    Em,
}

/// Reads a file.
#[derive(purrassist::ToolDefinition, Debug, PartialEq)]
#[tool(effects = read_only)]
struct ReadFile {
    /// Where to read.
    path: String,
    limit: Option<u32>,
    unit: Unit,
    tags: Vec<String>,
}

fn hand_built() -> Definition {
    Definition::new("read_file")
        .description("Reads a file.")
        .effects(Effects::ReadOnly)
        .param(
            "path",
            <String as ToolParameter>::parameter()
                .required()
                .description("Where to read."),
        )
        .param("limit", <Option<u32> as ToolParameter>::parameter())
        .param("unit", <Unit as ToolParameter>::parameter().required())
        .param(
            "tags",
            <Vec<String> as ToolParameter>::parameter().required(),
        )
}

#[test]
fn derived_equals_hand_built() {
    assert_eq!(ReadFile::tool_definition(), hand_built());
    assert_eq!(Unit::parameter(), Param::enumeration(&["Px", "em"]));
}

#[test]
fn compile_recover_and_convert_through_the_facade() {
    let tools = ToolSet::compile([ReadFile::tool_definition()], Budget::default()).unwrap();
    let dialects =
        DialectSet::new([DialectId::Hermes], &mut Governor::new(Budget::default())).unwrap();
    let request = ToolRecoveryRequest::default().dialects(dialects);
    let turn = "<tool_call>\n{\"name\": \"read_file\", \"arguments\": \
                {\"path\": \"a.txt\", \"unit\": \"em\", \"tags\": [\"x\"]}}\n</tool_call>";
    let result = tools.recover(turn.as_bytes(), &request);
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("not resolved: {:#?}", result.outcome);
    };
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        ReadFile::from_node(&calls.calls[0].arguments),
        Ok(ReadFile {
            path: "a.txt".into(),
            limit: None,
            unit: Unit::Em,
            tags: vec!["x".into()],
        })
    );
}

#[test]
fn conversion_errors_name_the_pointer() {
    let node: Node = json::parse(r#"{"path":"a","unit":"mm","tags":[]}"#).unwrap();
    let error = ReadFile::from_node(&node).unwrap_err();
    assert_eq!(
        error,
        FromNodeError {
            pointer: "/unit".into(),
            expected: "one of \"Px\", \"em\"",
            found: "string".into(),
        }
    );
}

mod reexports {
    pub(crate) use purrassist as pa;
}

#[derive(purrassist::ToolEnum, Debug, PartialEq)]
#[tool(crate = ::purrassist)]
enum Root {
    A,
}

#[derive(purrassist::ToolDefinition, Debug, PartialEq)]
#[tool(crate = ::purrassist)]
struct ViaRoot {
    root: Root,
}

#[derive(purrassist::ToolDefinition, Debug, PartialEq)]
#[tool(crate = ::purrassist::__core)]
struct ViaHiddenCore {
    path: String,
}

#[derive(purrassist::ToolDefinition, Debug, PartialEq)]
#[tool(crate = crate::reexports::pa)]
struct ViaReexport {
    path: String,
    unit: Unit,
}

#[test]
fn the_override_accepts_the_facade_root_a_hidden_core_and_a_reexport() {
    assert_eq!(Root::parameter(), Param::enumeration(&["A"]));
    assert_eq!(
        ViaRoot::from_node(&json::parse(r#"{"root": "A"}"#).unwrap()),
        Ok(ViaRoot { root: Root::A })
    );
    assert_eq!(
        ViaHiddenCore::from_node(&json::parse(r#"{"path": "p"}"#).unwrap()),
        Ok(ViaHiddenCore { path: "p".into() })
    );
    assert_eq!(
        ViaReexport::from_node(&json::parse(r#"{"path": "p", "unit": "em"}"#).unwrap()),
        Ok(ViaReexport {
            path: "p".into(),
            unit: Unit::Em
        })
    );
    assert_eq!(ViaReexport::tool_definition().name(), "via_reexport");
}
