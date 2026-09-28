// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared string matching keeps caller identities, aliases, signals, and failures.
use purrassist::{json, json::Value};
use purrassist_host::tools::Tools;

#[test]
fn string_tool_admits_legacy_and_identified_candidates() {
    let root = std::env::temp_dir().join(format!("purrassist-match-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let legacy = tools
        .call(
            "match_strings",
            &json::parse(r#"{"query":"raeder","candidates":["reader","writer"]}"#).unwrap(),
        )
        .unwrap();
    assert_eq!(legacy.pointer("/matches/0/id").unwrap().as_str(), Some("0"));
    assert_eq!(
        legacy.pointer("/matches/0/text").unwrap().as_str(),
        Some("reader")
    );
    assert!(legacy.pointer("/matches/0/signals/0/method").is_some());
    let request = r#"{"query":"iced tea","candidates":[{"id":"tea","text":"cold tea","aliases":["iced tea"]},{"id":"coffee","text":"coffee"}],"effort":"standard","english_phonetics":false,"limit":1}"#;
    let selected = tools
        .call("match_strings", &json::parse(request).unwrap())
        .unwrap();
    assert_eq!(
        selected.pointer("/matches/0/id").unwrap().as_str(),
        Some("tea")
    );
    assert_eq!(
        selected.pointer("/matches/0/aliases/0").unwrap().as_str(),
        Some("iced tea")
    );
    let Value::Array(signals) = &selected.pointer("/matches/0/signals").unwrap().value else {
        panic!("missing signals")
    };
    assert!(
        signals
            .iter()
            .any(|signal| signal.get("method").unwrap().as_str() == Some("caller-alias-v1"))
    );
    assert!(
        signals
            .iter()
            .all(|signal| signal.get("method").unwrap().as_str() != Some("soundex-en-v1"))
    );
    let exact = tools.call("match_strings", &json::parse(r#"{"query":"iced tea","candidates":[{"id":"tea","text":"cold tea","aliases":["iced tea"]}],"effort":"exact"}"#).unwrap()).unwrap();
    assert_eq!(
        exact.pointer("/receipt/termination/kind").unwrap().as_str(),
        Some("no_candidate")
    );
    let duplicate = tools.call("match_strings", &json::parse(r#"{"query":"tea","candidates":[{"id":"same","text":"tea"},{"id":"same","text":"coffee"}]}"#).unwrap()).unwrap();
    assert_eq!(
        duplicate
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("invalid_input")
    );
    assert!(duplicate.get("matches").is_some());
    assert!(
        tools
            .call(
                "match_strings",
                &json::parse(
                    r#"{"query":"tea","candidates":[{"id":"tea","text":"tea","aliases":[1]}]}"#
                )
                .unwrap()
            )
            .is_err()
    );
    let oversized = format!(
        r#"{{"query":"tea","candidates":[{{"id":"tea","text":"tea","aliases":["{}"]}}],"effort":"exact"}}"#,
        "x".repeat(100_000)
    );
    let exhausted = tools
        .call("match_strings", &json::parse(&oversized).unwrap())
        .unwrap();
    assert_eq!(
        exhausted
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("budget_exceeded")
    );
    assert_eq!(exhausted.pointer("/matches").unwrap().to_json(), "[]");
    std::fs::remove_dir_all(root).unwrap();
}
