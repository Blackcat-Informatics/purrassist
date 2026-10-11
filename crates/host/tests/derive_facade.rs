// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The facade derives from a crate whose only engine dependency is
//! `purrassist` (it has no `purrassist-core` dependency), which proves the
//! generated `::purrassist::__core` paths resolve.

use purrassist::Budget;
use purrassist::json;
use purrassist::tool_call::{FromNode, ToolArguments, ToolSet};

#[derive(purrassist::ToolEnum, Debug, PartialEq)]
enum Mode {
    Fast,
    Careful,
}

#[derive(purrassist::ToolDefinition, Debug, PartialEq)]
struct Run {
    command: String,
    mode: Mode,
    retries: Option<u8>,
}

#[test]
fn derives_resolve_without_a_core_dependency() {
    let tools = ToolSet::compile([Run::tool_definition()], Budget::default()).unwrap();
    assert!(tools.tool("run").is_some());
    let node = json::parse(r#"{"command":"ls","mode":"Careful"}"#).unwrap();
    assert_eq!(
        Run::from_node(&node),
        Ok(Run {
            command: "ls".into(),
            mode: Mode::Careful,
            retries: None
        })
    );
}
