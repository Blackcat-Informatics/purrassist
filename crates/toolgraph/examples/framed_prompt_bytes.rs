// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Prepare real wire declarations separately, then admit their complete bytes.

use purrassist_core::{
    Budget, Governor, json,
    tool_call::{DeclarationForm, ToolSet},
};
use purrassist_toolgraph::{FramedPrompt, PromptByteFailure, PromptBytePart};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"[{"name":"read","description":"Read Ω files","inputSchema":{"type":"object"}},{"name":"write","description":"Write \"quoted\" text","inputSchema":{"type":"object"}}]"#;
    let tools = ToolSet::from_mcp(source, Budget::default())?;
    let mut preparation = Governor::new(Budget::default());
    let read = tools
        .tool("read")
        .expect("prepared read declaration")
        .render_declaration_governed(DeclarationForm::OpenAi, &mut preparation)?;
    let write = tools
        .tool("write")
        .expect("prepared write declaration")
        .render_declaration_governed(DeclarationForm::OpenAi, &mut preparation)?;
    let parts = [
        PromptBytePart {
            identity: "read",
            text: &read,
        },
        PromptBytePart {
            identity: "write",
            text: &write,
        },
    ];
    let input = FramedPrompt {
        source_identity: "wire-catalog",
        prefix: "[",
        separator: ",\n",
        suffix: "]",
        parts: &parts,
    };
    let expected = format!("[{read},\n{write}]");
    let exact = u64::try_from(expected.len())?;
    let output = input
        .assemble_bytes(exact, &mut Governor::new(Budget::default()))
        .expect("the exact prepared byte ceiling fits");
    assert_eq!(output.text, expected);
    assert_eq!(output.exact_bytes, exact);
    assert_eq!(output.input, input);
    let parsed = json::parse(&output.text)?;
    assert_eq!(
        parsed
            .pointer("/0/function/name")
            .and_then(json::Node::as_str),
        Some("read")
    );
    assert_eq!(
        parsed
            .pointer("/1/function/name")
            .and_then(json::Node::as_str),
        Some("write")
    );
    let error = input
        .assemble_bytes(exact - 1, &mut Governor::new(Budget::default()))
        .unwrap_err();
    assert_eq!(error.input, input);
    assert_eq!(
        error.reason,
        PromptByteFailure::ByteCeiling {
            required_bytes: exact,
            max_bytes: exact - 1
        }
    );
    Ok(())
}
