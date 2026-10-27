// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Govern existing wire declaration output separately from tool preparation.

use purrassist_core::{
    Budget, Governor, json,
    tool_call::{DeclarationForm, ToolSet},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"{ "name": "read", "description": "Read files", "inputSchema": {"type":"object"}, "_meta": {"example":true} }"#;
    let tools = ToolSet::from_mcp(source, Budget::default())?;
    let tool = tools
        .tool("read")
        .expect("the admitted declaration names read");
    for form in [
        DeclarationForm::Mcp,
        DeclarationForm::OpenAi,
        DeclarationForm::Anthropic,
    ] {
        let mut output_governor = Governor::new(Budget::default());
        let output = tool.render_declaration_governed(form, &mut output_governor)?;
        let declaration = json::parse(&output)?;
        let record = declaration.get("function").unwrap_or(&declaration);
        assert_eq!(
            record.get("name").and_then(|name| name.as_str()),
            Some("read")
        );
        if form == DeclarationForm::Mcp {
            assert_eq!(output, source);
        }
        assert!(output_governor.usage().work >= u64::try_from(output.len())?);
    }
    Ok(())
}
