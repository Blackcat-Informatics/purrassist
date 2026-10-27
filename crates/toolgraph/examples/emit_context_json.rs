// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Serialize a caller-prepared context-shaped object; this is not a context codec.
//! Run `cargo run -p purrassist-toolgraph --example emit_context_json`.

use purrassist_core::{Budget, Governor, json};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let expected = r#"{"outputProjectionJq":".orders","maxOutputBytes":4096,"outputEnforcement":"exact-bytes","historyRetentionTurns":2,"intentTags":["customer","quoted \"tag\"\u0000"],"loadingStrategy":"on_demand"}"#;
    // Parsing/source ownership is outside this emission operation. The future
    // context codec must separately validate fields, units, modes and version.
    let context = json::parse(expected)?;
    let mut governor = Governor::new(Budget::default());
    let encoded = context.to_json_governed(&mut governor)?;
    assert_eq!(encoded, expected);
    assert_eq!(
        json::quote_governed("customer", &mut governor)?,
        "\"customer\""
    );
    Ok(())
}
