// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! GLM boundaries preserve text and typed JSON without XML framing semantics.
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, ObjectSpec, Param, ToolCalls, ToolDefinition,
    ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{Budget, Governor, Outcome};

fn recover(body: &str) -> ToolCalls {
    let tools = ToolSet::compile(
        [ToolDefinition::new("f")
            .effects(Effects::ReadOnly)
            .param("text", Param::string())
            .param("items", Param::array(Param::integer()))
            .param(
                "object",
                Param::object(ObjectSpec::new().param("text", Param::string())),
            )],
        Budget::default(),
    )
    .expect("tools");
    let dialects = DialectSet::new([DialectId::Glm45], &mut Governor::new(Budget::default()))
        .expect("dialect");
    let input = format!("<tool_call>f\n{body}</tool_call>");
    match tools
        .recover(input.as_bytes(), &ToolRecoveryRequest::default(), &dialects)
        .outcome
    {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("{other:?}"),
    }
}

#[test]
fn verbatim_newlines_and_exact_closer_outrank_fuzzy_substrings() {
    let text = "\n  keep </arg_valu> this\n";
    let calls = recover(&format!(
        "<arg_key>text</arg_key><arg_value>{text}</arg_value>"
    ));
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[0]
            .arguments
            .get("text")
            .and_then(|node| node.as_str()),
        Some(text)
    );
}

#[test]
fn inline_json_strings_do_not_open_or_close_argument_elements() {
    let calls = recover(
        r#"<arg_key>object</arg_key><arg_value>{"text":"<arg_key>literal</arg_value>"}</arg_value>"#,
    );
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[0]
            .arguments
            .get("object")
            .unwrap()
            .get("text")
            .unwrap()
            .as_str(),
        Some("<arg_key>literal</arg_value>")
    );
}

#[test]
fn repeated_arrays_collect_and_repeated_scalars_remain_ambiguous() {
    let calls = recover(
        "<arg_key>items</arg_key><arg_value>[1]</arg_value><arg_key>items</arg_key><arg_value>[2]</arg_value>",
    );
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[0].arguments.get("items").unwrap().to_json(),
        "[1,2]"
    );
    let calls = recover(
        "<arg_key>text</arg_key><arg_value>a</arg_value><arg_key>text</arg_key><arg_value>b</arg_value>",
    );
    assert_ne!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.usable(),
        [] as [&purrassist_core::tool_call::ToolCall; 0]
    );
}

#[test]
fn malformed_alternating_pairs_never_become_usable_calls() {
    for body in [
        "<arg_value>orphan</arg_value>",
        "<arg_key></arg_key><arg_value>x</arg_value>",
        "unexpected positional text",
    ] {
        let calls = recover(body);
        assert!(calls.usable_accepting_guesses().is_empty(), "{body}");
    }
}

/// Exact schemas permit numeric text only when no string branch also accepts it.
#[test]
fn emission_uses_exact_schema_typing_for_refs_and_preserves_string_unions() {
    use purrassist_core::tool_call::{IdPolicy, emit};
    for (schema, representable) in [
        (
            r##"{"type":"object","properties":{"x":{"$ref":"#/$defs/int"}},"$defs":{"int":{"type":"integer"}}}"##,
            true,
        ),
        (
            r#"{"type":"object","properties":{"x":{"anyOf":[{"type":"integer"},{"type":"string"}]}}}"#,
            false,
        ),
    ] {
        let declarations =
            format!(r#"[{{"type":"function","function":{{"name":"f","parameters":{schema}}}}}]"#);
        let tools = ToolSet::from_openai(&declarations, Budget::default()).expect("tools");
        let recovered = tools.recover(
            br#"<tool_call>{"name":"f","arguments":{"x":42}}</tool_call>"#,
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        let Outcome::Resolved(calls) = recovered.outcome else {
            panic!("{recovered:?}")
        };
        assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
        let emitted =
            emit::emit(&tools, &calls, &DialectId::Glm45, &IdPolicy::default()).expect("emission");
        assert_eq!(emitted.skipped.is_empty(), representable);
        if representable {
            assert!(
                String::from_utf8(emitted.output)
                    .expect("UTF-8")
                    .contains("<arg_value>42</arg_value>")
            );
        }
    }
}
