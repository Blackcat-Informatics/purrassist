// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Validate a caller-prepared result with an admitted exact MCP output contract.
use purrassist::{Budget, Governor, json, tool_call::ToolSet};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let declaration = r#"{"name":"count","inputSchema":{},"outputSchema":{"type":"object","properties":{"count":{"type":"integer"}},"required":["count"]}}"#;
    let mut governor = Governor::new(Budget::default());
    let tools = ToolSet::from_mcp_governed(declaration, &mut governor)?;
    let schema = tools
        .tool("count")
        .expect("declared count")
        .output_schema()
        .expect("declared output");
    let result = json::parse_with(r#"{"count":3}"#, &mut governor)?;
    assert!(schema.validate_with(&result, &mut governor)?.valid);
    Ok(())
}
