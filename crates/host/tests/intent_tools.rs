// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Semantic tool outcomes, exact source retention, and preparation exhaustion.
use purrassist::{
    json::{self, Node, Value},
    wire::{object, string},
};
use purrassist_host::tools::Tools;

fn request(input: &str, effort: &str) -> Node {
    object([
        ("input", string(input)),
        ("effort", string(effort)),
        ("domain", json::parse(r#"{"id":"drinks.v1","ignored_words":["i","want","a","of"],"rules":[{"id":"tea","phrase":"tea","assignments":{"drink":"tea"}},{"id":"hot","phrase":"hot","assignments":{"temperature":"hot"}},{"id":"cooled","phrase":"cooled","assignments":{"temperature":"cold"}},{"id":"glass","phrase":"glass","assignments":{"vessel":"drinking-vessel"}}]}"#).unwrap()),
        ("concepts", json::parse(r#"[{"id":"hot-tea","label":"cup of hot tea","attributes":{"drink":"tea","temperature":"hot","vessel":"drinking-vessel"}},{"id":"cold-tea","label":"cup of iced tea","attributes":{"drink":"tea","temperature":"cold","vessel":"drinking-vessel"}}]"#).unwrap()),
    ])
}
#[test]
fn semantic_tools_preserve_refinement_ambiguity_and_unknown_words() {
    let root = std::env::temp_dir().join(format!("purrassist-intent-tool-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let tools = Tools::new(&root).unwrap();
    let input = "I want a glass of hot tea, cooled";
    let result = tools
        .call("resolve_intent", &request(input, "standard"))
        .unwrap();
    assert_eq!(result.get("status").unwrap().as_str(), Some("resolved"));
    assert_eq!(
        result.pointer("/value/concept/label").unwrap().as_str(),
        Some("cup of iced tea")
    );
    assert_eq!(
        result
            .pointer("/value/desired/temperature")
            .unwrap()
            .as_str(),
        Some("cold")
    );
    assert_eq!(
        result.pointer("/receipt/sources/0/text").unwrap().as_str(),
        Some(input)
    );
    let Value::Array(evidence) = &result.pointer("/receipt/evidence").unwrap().value else {
        panic!("missing evidence")
    };
    assert!(evidence.iter().any(|entry| {
        entry.get("rule").unwrap().as_str() == Some("drinks.v1::cooled")
            && entry
                .get("explanation")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("hot")
    }));
    let ambiguous = tools
        .call("resolve_intent", &request("tea", "standard"))
        .unwrap();
    assert_eq!(ambiguous.get("status").unwrap().as_str(), Some("ambiguous"));
    let Value::Array(alternatives) = &ambiguous.get("alternatives").unwrap().value else {
        panic!("missing alternatives")
    };
    assert_eq!(alternatives.len(), 2);
    let Value::Array(ledgers) = &ambiguous.pointer("/receipt/alternatives").unwrap().value else {
        panic!("missing alternative ledgers")
    };
    for (value, ledger) in alternatives.iter().zip(ledgers) {
        assert_eq!(
            ledger.get("output").unwrap().as_str(),
            Some(
                purrassist::digest::Digest::of(value.to_json().as_bytes())
                    .to_string()
                    .as_str()
            )
        );
    }
    assert_eq!(ledgers.len(), alternatives.len());
    let unknown = tools
        .call("resolve_intent", &request("not hot tea", "standard"))
        .unwrap();
    assert_eq!(unknown.get("status").unwrap().as_str(), Some("unresolved"));
    assert_eq!(
        unknown
            .pointer("/receipt/termination/kind")
            .unwrap()
            .as_str(),
        Some("invalid_input")
    );
    for input in [input.to_owned(), "a".repeat(100_001)] {
        let exhausted = tools
            .call("resolve_intent", &request(&input, "exact"))
            .unwrap();
        assert_eq!(
            exhausted.get("status").unwrap().as_str(),
            Some("unresolved")
        );
        assert_eq!(
            exhausted
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("budget_exceeded")
        );
    }
    assert!(tools.call("resolve_intent", &json::parse(r#"{"input":"tea","domain":{"id":"x","rules":[{"id":"x","phrase":"tea","assignments":{"drink":12}}]},"concepts":[]}"#).unwrap()).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
