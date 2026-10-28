// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Prepare real wire declarations separately, then admit their complete bytes.

use purrassist_core::{
    Budget, Governor, json,
    tool_call::{DeclarationForm, ToolSet},
};
use purrassist_toolgraph::{
    ByteLevelTokenContract, FramedPrompt, PromptByteFailure, PromptBytePart, PromptTokenAccounting,
    PromptTokenFailure, TokenCountError, TokenCounter,
};

/// An executable example encoding, not a tokenizer qualification claim.
#[derive(Debug)]
struct ScalarEncoding;

impl TokenCounter for ScalarEncoding {
    fn identity(&self) -> &'static str {
        "example-unicode-scalar.v1"
    }

    fn configuration(&self) -> &'static str {
        "one-scalar-per-token; no-preprocessing; no-special-tokens"
    }

    fn count(&self, bytes: &[u8], governor: &mut Governor) -> Result<u64, TokenCountError> {
        let bytes_len = purrassist_core::contract::saturating_size(bytes.len());
        governor.charge(
            purrassist_core::contract::Resource::Work,
            bytes_len.saturating_mul(2),
        )?;
        let text = core::str::from_utf8(bytes).map_err(|_| TokenCountError::Unavailable)?;
        u64::try_from(text.chars().count()).map_err(|_| TokenCountError::Unavailable)
    }
}

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
    token_admission(input, &expected, exact)?;
    Ok(())
}

fn token_admission(
    input: FramedPrompt<'_>,
    expected: &str,
    exact: u64,
) -> Result<(), core::num::TryFromIntError> {
    // Token admission sees these same wire bytes, including every framing byte.
    let token_limit = u64::try_from(expected.chars().count())?;
    assert!(
        token_limit < exact,
        "the declaration contains multibyte text"
    );
    let encoding = ScalarEncoding;
    let counted = input
        .assemble_accounted(
            exact,
            token_limit,
            PromptTokenAccounting::Exact(&encoding),
            &mut Governor::new(Budget::default()),
        )
        .expect("exact scalar count fits independently of UTF-8 bytes");
    assert_eq!(counted.bytes().text, expected);
    assert_eq!(counted.measurement().tokens, token_limit);
    let byte_refusal = input
        .assemble_accounted(
            token_limit,
            token_limit,
            PromptTokenAccounting::Exact(&encoding),
            &mut Governor::new(Budget::default()),
        )
        .unwrap_err();
    assert!(matches!(
        byte_refusal.reason,
        PromptTokenFailure::Bytes(PromptByteFailure::ByteCeiling { .. })
    ));
    let conservative = PromptTokenAccounting::ByteLevel(ByteLevelTokenContract {
        encoding_identity: "example-unicode-scalar.v1/no-special-tokens",
        admission_basis: "each UTF-8 scalar occupies at least one byte; no preprocessing or implicit tokens",
    });
    let token_refusal = input
        .assemble_accounted(
            exact,
            token_limit,
            conservative,
            &mut Governor::new(Budget::default()),
        )
        .unwrap_err();
    assert_eq!(
        token_refusal.reason,
        PromptTokenFailure::TokenCeiling {
            max_tokens: token_limit
        }
    );
    assert_eq!(
        token_refusal.measurement.expect("complete bound").tokens,
        exact
    );
    Ok(())
}
