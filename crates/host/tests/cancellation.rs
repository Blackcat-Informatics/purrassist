// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Cancelled calls do not enter filesystem mutation; ordinary calls still work.
use purrassist::{
    CancellationToken,
    digest::Digest,
    json,
    wire::{object, string},
};
use purrassist_host::tools::Tools;

#[test]
fn cancelled_tool_calls_preserve_files_and_new_tokens_can_retry() {
    let root = std::env::temp_dir().join(format!("purrassist-cancel-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("tea.txt"), "tea").unwrap();
    let tools = Tools::new(&root).unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let args = object([
        ("path", string("tea.txt")),
        ("expected_digest", string(Digest::of(b"tea").to_string())),
        ("replacement", string("coffee")),
    ]);
    assert_eq!(
        tools.call_with_cancellation("apply_file", &args, &token),
        Err("cancelled".into())
    );
    assert_eq!(
        std::fs::read_to_string(root.join("tea.txt")).unwrap(),
        "tea"
    );
    let query = json::parse(r#"{"input":"[[42]]","schema":{"type":"integer"}}"#).unwrap();
    assert_eq!(
        tools.call_with_cancellation("decode_json", &query, &token),
        Err("cancelled".into())
    );
    let retry = CancellationToken::new();
    let result = tools
        .call_with_cancellation("decode_json", &query, &retry)
        .unwrap();
    assert_eq!(
        result.pointer("/status").unwrap().as_str(),
        Some("resolved")
    );
    let result = tools
        .call_with_cancellation("apply_file", &args, &retry)
        .unwrap();
    assert!(result.get("commit").is_some());
    retry.cancel();
    assert_eq!(
        std::fs::read_to_string(root.join("tea.txt")).unwrap(),
        "coffee"
    );
    std::fs::remove_dir_all(root).unwrap();
}
