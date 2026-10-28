// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Bounded stdin envelope: format, context, outputSchema and result. Stdout is
//! only the complete checked JSON Lines artifact; failures go to stderr.

use std::io::{Read, Write};

use purrassist_core::{Budget, Governor, json, schema::Schema};
use purrassist_toolgraph::ContextManagement;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const MAX_INPUT: u64 = 1_048_576;
    let mut source = String::new();
    std::io::stdin()
        .lock()
        .take(MAX_INPUT + 1)
        .read_to_string(&mut source)?;
    if u64::try_from(source.len())? > MAX_INPUT {
        return Err("input envelope exceeds 1048576 bytes".into());
    }
    let mut governor = Governor::new(Budget::default());
    let envelope = json::parse_with(&source, &mut governor)?;
    let json::Value::Object(members) = &envelope.value else {
        return Err("input envelope must be an object".into());
    };
    for member in members {
        if !["format", "context", "outputSchema", "result"].contains(&member.key.as_str()) {
            return Err(format!("unknown envelope field {:?}", member.key).into());
        }
    }
    let field = |name| {
        envelope
            .get(name)
            .ok_or_else(|| format!("envelope requires exactly one {name} member"))
    };
    let format = field("format")?.as_str().ok_or("format must be a string")?;
    let context = ContextManagement::decode_mcp(Some(format), field("context")?, &mut governor)?;
    let schema_text = field("outputSchema")?.to_json_governed(&mut governor)?;
    let schema = Schema::compile_governed(&schema_text, &mut governor)?;
    let artifact = context
        .project_result(&schema, field("result")?, &mut governor)
        .map_err(|error| error.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(artifact.bytes.as_str().as_bytes())?;
    Ok(())
}
