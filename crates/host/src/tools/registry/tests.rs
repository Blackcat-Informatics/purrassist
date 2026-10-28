// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The actual entry invocation boundary, including real journaled effects.
use super::{ENTRIES, Entry};
use crate::tools::Tools;
use purrassist::{
    Budget, CancellationToken, Governor,
    contract::Resource,
    digest::Digest,
    json::{self, Node},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "purrassist-output-boundary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("tea.txt"), "tea").unwrap();
        Self(path)
    }
    fn tools(&self) -> Tools {
        Tools::new(&self.0).unwrap()
    }
    fn text(&self) -> String {
        std::fs::read_to_string(self.0.join("tea.txt")).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn entry(name: &str) -> &Entry {
    ENTRIES.iter().find(|entry| entry.name == name).unwrap()
}
fn replacement() -> Node {
    json::parse(&format!(
        r#"{{"path":"tea.txt","expected_digest":"{}","replacement":"coffee"}}"#,
        Digest::of(b"tea")
    ))
    .unwrap()
}
#[expect(
    clippy::unnecessary_wraps,
    reason = "The controlled producer must have the actual production Handler function-pointer signature."
)]
fn echo(_: &Tools, _: &str, args: &Node, _: Option<&CancellationToken>) -> Result<Node, String> {
    Ok(args.clone())
}
fn apply_then_cancel(
    tools: &Tools,
    _: &str,
    args: &Node,
    token: Option<&CancellationToken>,
) -> Result<Node, String> {
    let result = tools.apply(args, token);
    if result.is_ok() {
        token.unwrap().cancel();
    }
    result
}

#[test]
fn admission_refuses_invalid_contracts_arguments_and_cancellation_before_effects() {
    let root = Root::new();
    let tools = root.tools();
    let original = entry("apply_file");
    let invalid = Entry {
        output_schema: "{",
        ..*original
    };
    let mut admission = Governor::new(Budget::default());
    let mut output = Governor::new(Budget::default());
    assert!(
        invalid
            .invoke(&tools, &replacement(), None, &mut admission, &mut output)
            .is_err()
    );
    assert_eq!(root.text(), "tea");
    assert_eq!(output.usage().work, 0);
    let token = CancellationToken::new();
    token.cancel();
    let mut admission = crate::tools::governor(Budget::default(), Some(&token));
    assert_eq!(
        original.invoke(
            &tools,
            &replacement(),
            Some(&token),
            &mut admission,
            &mut output
        ),
        Err("cancelled".into())
    );
    assert_eq!(root.text(), "tea");
    let mut admission = Governor::new(Budget::default());
    assert!(
        original
            .invoke(
                &tools,
                &json::parse("{}").unwrap(),
                None,
                &mut admission,
                &mut output
            )
            .unwrap_err()
            .contains("tool arguments violate schema")
    );
    assert_eq!(root.text(), "tea");
    let mut admission = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert!(
        original
            .invoke(&tools, &replacement(), None, &mut admission, &mut output)
            .is_err()
    );
    assert_eq!(
        admission.check(),
        Err(purrassist::StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(root.text(), "tea");
}

#[test]
fn whole_output_checks_preserve_valid_objects_arrays_and_native_refusal_originals() {
    let root = Root::new();
    let tools = root.tools();
    let checked = Entry {
        name: "echo",
        description: "test original",
        input_schema: "true",
        output_schema: r#"{"type":"object","required":["expected"]}"#,
        writes: false,
        handler: echo,
    };
    let original = json::parse(r#"{"n":1.20,"n":1.2}"#).unwrap();
    let mut admission = Governor::new(Budget::default());
    let mut output = Governor::new(Budget::default());
    let refusal = json::parse(
        &checked
            .invoke(&tools, &original, None, &mut admission, &mut output)
            .unwrap_err(),
    )
    .unwrap();
    assert_eq!(
        refusal.get("original_output").unwrap().to_json(),
        original.to_json()
    );
    assert_eq!(
        refusal.pointer("/reason/kind").unwrap().as_str(),
        Some("schema_violation")
    );
    assert_eq!(
        refusal.pointer("/validation/valid").unwrap().to_json(),
        "false"
    );
    assert_eq!(
        refusal.get("schema").unwrap().as_str(),
        Some(
            purrassist::Schema::compile(checked.output_schema)
                .unwrap()
                .digest()
                .to_string()
                .as_str()
        )
    );
    assert_eq!(
        refusal
            .pointer("/admission/termination/kind")
            .unwrap()
            .as_str(),
        Some("completed")
    );
    assert_eq!(
        refusal
            .pointer("/output_validation/termination/kind")
            .unwrap()
            .as_str(),
        Some("invalid_input")
    );
    for (schema, text) in [
        (r#"{"type":"object"}"#, r#"{"a":1.20,"b":1.2}"#),
        (r#"{"type":"array"}"#, "[1.20,1.2]"),
    ] {
        let valid = Entry {
            output_schema: schema,
            ..checked
        };
        let original = json::parse(text).unwrap();
        let mut admission = Governor::new(Budget::default());
        let mut output = Governor::new(Budget::default());
        assert_eq!(
            valid
                .invoke(&tools, &original, None, &mut admission, &mut output)
                .unwrap()
                .to_json(),
            original.to_json()
        );
    }
    assert_native_output_stops(&checked, &tools, &original);
    // The largest registered output schema is prepared through actual public
    // dispatch, with the real recovery handler and native returned envelope.
    let args=json::parse(r#"{"output":"<tool_call>{\"name\":\"ls\",\"arguments\":{}}</tool_call>","tools":[{"type":"function","function":{"name":"ls","parameters":{"type":"object","properties":{}}}}]}"#).unwrap();
    assert_eq!(
        tools
            .call("recover_tool_calls", &args)
            .unwrap()
            .get("status")
            .unwrap()
            .as_str(),
        Some("resolved")
    );
}

#[test]
fn completed_file_effects_survive_late_cancellation_and_invalid_output_release() {
    for valid in [true, false] {
        let root = Root::new();
        let tools = root.tools();
        let token = CancellationToken::new();
        let selected = entry("apply_file");
        let checked = Entry {
            handler: apply_then_cancel,
            output_schema: if valid {
                selected.output_schema
            } else {
                "false"
            },
            ..*selected
        };
        let mut admission = crate::tools::governor(Budget::default(), Some(&token));
        let mut output = Governor::new(Budget::default());
        let result = checked.invoke(
            &tools,
            &replacement(),
            Some(&token),
            &mut admission,
            &mut output,
        );
        assert!(token.is_cancelled());
        assert_eq!(root.text(), "coffee");
        let original = if valid {
            result.unwrap()
        } else {
            let error = result.unwrap_err();
            assert_ne!(error, "cancelled");
            let refusal = json::parse(&error).unwrap();
            assert_eq!(refusal.get("handler_returned").unwrap().to_json(), "true");
            assert_eq!(
                refusal.get("effects_may_have_occurred").unwrap().to_json(),
                "true"
            );
            assert_eq!(
                refusal.pointer("/reason/kind").unwrap().as_str(),
                Some("schema_violation")
            );
            refusal.get("original_output").unwrap().clone()
        };
        assert_eq!(
            original
                .pointer("/receipt/termination/kind")
                .unwrap()
                .as_str(),
            Some("completed")
        );
        let transaction = original
            .pointer("/commit/transaction")
            .unwrap()
            .as_str()
            .unwrap();
        let undo = json::parse(&format!(r#"{{"transaction":"{transaction}"}}"#)).unwrap();
        tools.call("undo_file", &undo).unwrap();
        assert_eq!(root.text(), "tea");
    }
}

fn assert_native_output_stops(checked: &Entry, tools: &Tools, original: &Node) {
    for resource in [Resource::Work, Resource::ScratchBytes] {
        let mut budget = Budget::default();
        match resource {
            Resource::Work => budget.work = 0,
            Resource::ScratchBytes => budget.scratch_bytes = 0,
            _ => unreachable!(),
        }
        let mut admission = Governor::new(Budget::default());
        let mut output = Governor::new(budget);
        let refusal = json::parse(
            &checked
                .invoke(tools, original, None, &mut admission, &mut output)
                .unwrap_err(),
        )
        .unwrap();
        assert_eq!(
            refusal.get("original_output").unwrap().to_json(),
            original.to_json()
        );
        assert_eq!(
            refusal.pointer("/reason/stop").unwrap().as_str(),
            Some("budget_exceeded")
        );
        assert_eq!(
            output.check(),
            Err(purrassist::StopReason::BudgetExceeded(resource))
        );
        assert_eq!(
            refusal
                .pointer("/output_validation/termination/kind")
                .unwrap()
                .as_str(),
            Some("budget_exceeded")
        );
    }
}
