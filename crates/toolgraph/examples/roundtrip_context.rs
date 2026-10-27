// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Execute the complete context-fragment round trip, with caller-owned DOM preparation.

use purrassist_core::{Budget, Governor, json};
use purrassist_toolgraph::{ContextManagement, MCP_CONTEXT_FORMAT, OutputEnforcement};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = json::parse(
        r#"{"outputProjectionJq":"{orders,total}","maxOutputBytes":1e2,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":["é","é","line\n"],"loadingStrategy":"on_demand"}"#,
    )?;
    let mut governor = Governor::new(Budget::default());
    let context = ContextManagement::decode_mcp(Some(MCP_CONTEXT_FORMAT), &source, &mut governor)?;
    assert_eq!(context.output_enforcement(), OutputEnforcement::ExactBytes);
    let wire = context.encode_json(Some(MCP_CONTEXT_FORMAT), &mut governor)?;
    assert_eq!(
        wire,
        r#"{"outputProjectionJq":"{orders, total}","maxOutputBytes":100,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":["é","é","line\n"],"loadingStrategy":"on_demand"}"#
    );
    let encoded = context.encode_mcp(Some(MCP_CONTEXT_FORMAT), &mut governor)?;
    let decoded = ContextManagement::decode_mcp(Some(MCP_CONTEXT_FORMAT), &encoded, &mut governor)?;
    assert_eq!(decoded, context);
    Ok(())
}
