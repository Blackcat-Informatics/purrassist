// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared embedding selections retain input and independent guess ledgers.
use purrassist::{
    digest::Digest,
    json::{self, Value},
};
use purrassist_host::tools::Tools;
#[test]
fn embedding_tools_preserve_vectors_and_independent_alternative_ledgers() {
    let root =
        std::env::temp_dir().join(format!("purrassist-embedding-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let args = json::parse(r#"{"space":"caller-v1","query":[1,0],"candidates":[{"id":"a","vector":[1,0]},{"id":"b","vector":[2,0]}]}"#).unwrap();
    let result = tools.call("match_embeddings", &args).unwrap();
    assert_eq!(
        result.pointer("/status").unwrap().as_str(),
        Some("ambiguous")
    );
    assert_eq!(
        result.pointer("/receipt/sources/0/text").unwrap().as_str(),
        Some(args.to_json().as_str())
    );
    let Value::Array(values) = &result.get("alternatives").unwrap().value else {
        panic!("alternatives")
    };
    let Value::Array(ledgers) = &result.pointer("/receipt/alternatives").unwrap().value else {
        panic!("ledgers")
    };
    assert_eq!(values.len(), 2);
    for (value, ledger) in values.iter().zip(ledgers) {
        assert_eq!(
            ledger.get("output").unwrap().as_str(),
            Some(Digest::of(value.to_json().as_bytes()).to_string().as_str())
        );
        assert_eq!(
            ledger.pointer("/inferences/1/value").unwrap().as_str(),
            value.get("id").unwrap().as_str()
        );
    }
    for (query, vector) in [
        ("[1]", "[0]"),
        ("[1]", "[1e9999]"),
        ("[1]", "[1e-9999]"),
        ("[1,0]", "[1]"),
    ] {
        let args = json::parse(&format!("{{\"space\":\"space\",\"query\":{query},\"candidates\":[{{\"id\":\"bad\",\"vector\":{vector}}}]}}")).unwrap();
        let result = tools.call("match_embeddings", &args).unwrap();
        assert_eq!(
            result.pointer("/status").unwrap().as_str(),
            Some("unresolved")
        );
        assert_eq!(
            result
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("invalid_input")
        );
    }
    for (extra, status, reason) in [
        ("", "ambiguous", "completed"),
        (",\"effort\":\"exact\"", "unresolved", "budget_exceeded"),
    ] {
        let args = json::parse(&format!(r#"{{"space":"space","query":[1],"candidates":[{{"id":"a","vector":[1]}},{{"id":"b","vector":[2]}}]{extra}}}"#)).unwrap();
        let value = tools.call("match_embeddings", &args).unwrap();
        assert_eq!(value.get("status").unwrap().as_str(), Some(status));
        assert_eq!(
            value.pointer("/receipt/termination/kind").unwrap().as_str(),
            Some(reason)
        );
    }
    let winner = tools
        .call(
            "match_embeddings",
            &json::parse(
                r#"{"space":"space","query":[1],"candidates":[{"id":"tea","vector":[1]}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(winner.get("status").unwrap().as_str(), Some("best_guess"));
    assert_eq!(
        winner
            .pointer("/receipt/inferences/1/kind")
            .unwrap()
            .as_str(),
        Some("guess")
    );
    std::fs::remove_dir_all(root).unwrap();
}
