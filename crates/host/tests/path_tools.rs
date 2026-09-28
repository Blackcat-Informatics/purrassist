// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared path-tool outcomes and schema boundaries; candidates need not exist.
use purrassist::{json, json::Value};
use purrassist_host::tools::Tools;

#[test]
fn paths_preserve_status_evidence_and_never_read_candidate_files() {
    let root = std::env::temp_dir().join(format!("purrassist-path-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    for (input, status) in [
        (
            r#"{"query":"src/reader.rs","paths":["src/reader.rs"]}"#,
            "resolved",
        ),
        (
            r#"{"query":"raeder.rs","context":"src","paths":["src/reader.rs","tests/reader.rs"]}"#,
            "best_guess",
        ),
        (
            r#"{"query":"README.md","paths":["docs/README.md","other/README.md"]}"#,
            "ambiguous",
        ),
        (
            r#"{"query":"unrelated.xyz","paths":["src/reader.rs"]}"#,
            "unresolved",
        ),
    ] {
        let result = tools
            .call("match_paths", &json::parse(input).unwrap())
            .unwrap();
        assert_eq!(result.get("status").unwrap().as_str(), Some(status));
        assert!(result.get("receipt").is_some());
        if status == "best_guess" {
            assert_eq!(
                result.pointer("/value/path").unwrap().as_str(),
                Some("src/reader.rs")
            );
            let Value::Array(inferences) = &result.pointer("/receipt/inferences").unwrap().value
            else {
                panic!("missing inferences")
            };
            assert_eq!(inferences.len(), 1);
        }
    }
    let exact = tools.call("match_paths", &json::parse(r#"{"query":"src/reader.rs","paths":["src/reader.rs","tests/reader.rs"],"effort":"exact"}"#).unwrap()).unwrap();
    assert_eq!(exact.get("status").unwrap().as_str(), Some("resolved"));
    for invalid in [
        r#"{"query":"x","paths":[12]}"#,
        r#"{"query":"x","paths":[],"minimum_score":10001}"#,
        r#"{"query":"x","paths":[],"context":null}"#,
    ] {
        assert!(
            tools
                .call("match_paths", &json::parse(invalid).unwrap())
                .is_err()
        );
    }
    for invalid in [
        r#"{"query":"x","paths":["../outside"]}"#,
        r#"{"query":"x","paths":["x","x"]}"#,
    ] {
        let result = tools
            .call("match_paths", &json::parse(invalid).unwrap())
            .unwrap();
        assert_eq!(
            result
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("invalid_input")
        );
    }
    assert!(!root.join("src").exists());
    std::fs::remove_dir_all(root).unwrap();
}
