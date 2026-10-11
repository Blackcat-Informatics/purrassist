// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The facade flavor of the derives: `#[derive(purrassist::ToolDefinition)]`
//! with no `crate =` attribute, generating `::purrassist::__core` paths.

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
