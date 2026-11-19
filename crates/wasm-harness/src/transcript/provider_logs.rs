// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Portable replay of byte-pinned provider responses and their truncated prefixes.

use purrassist_core::{
    Budget, Governor,
    digest::Digest,
    json::Node,
    tool_call::{
        ChatFaultKind, DialectSet, EmitTarget, EventFaultKind, Param, ProviderLogFormat,
        ProviderLogRecovery, ToolCall, ToolDefinition, ToolRecoveryRequest, ToolSet,
    },
    wire::{array, object, string},
};

const ANTHROPIC: &[u8] = include_bytes!("../provider_logs/anthropic-parallel-weather.sse");
const OPENAI: &[u8] = include_bytes!("../provider_logs/openai-weather.sse");

/// Replay the complete captures and their truncated terminal events.
pub(super) fn transcript() -> Node {
    let anthropic_digest = Digest::of(ANTHROPIC).to_string();
    let openai_digest = Digest::of(OPENAI).to_string();
    assert_eq!(
        anthropic_digest,
        "sha256:8c0641597662061a7516ebc2b3d574514b21856d70411761d4fb504d4b92455a"
    );
    assert_eq!(
        openai_digest,
        "sha256:c6aa7e397b7123c3501f25df3a05d4daf7e8ad6d61ffa406ab5361fe36a8d5b1"
    );
    let tools = ToolSet::compile(
        [
            ToolDefinition::new("get_weather").param("city", Param::string().required()),
            ToolDefinition::new("GetWeatherArgs")
                .param("city", Param::string().required())
                .param("country", Param::string().required())
                .param("units", Param::string().required()),
        ],
        Budget::default(),
    )
    .expect("weather tool declarations");
    let recover = |format, log| {
        let request = ToolRecoveryRequest::default().emit(EmitTarget::OpenAiToolCalls);
        let governor = Governor::new(request.budget);
        tools.recover_provider_log(format, log, request, DialectSet::built_in(), governor)
    };

    let anthropic = recover(ProviderLogFormat::Anthropic, ANTHROPIC);
    let ProviderLogRecovery::Anthropic(result) = &anthropic else {
        unreachable!("Anthropic response");
    };
    assert!(result.is_clean(), "{:?}", result.faults);
    assert_eq!(result.log, ANTHROPIC);
    let calls = result.usable();
    assert_eq!(calls.len(), 2);
    for (call, city) in calls.into_iter().zip(["Paris", "London"]) {
        assert_eq!(call.tool.as_deref(), Some("get_weather"));
        assert_eq!(
            call.arguments.get("city").and_then(Node::as_str),
            Some(city)
        );
    }

    let openai = recover(ProviderLogFormat::OpenAiChat, OPENAI);
    let ProviderLogRecovery::OpenAiChat(result) = &openai else {
        unreachable!("OpenAI response");
    };
    assert!(result.is_clean(), "{:?}", result.faults);
    assert_eq!(result.log, OPENAI);
    let calls = result.usable();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool.as_deref(), Some("GetWeatherArgs"));
    for (field, value) in [("city", "Edinburgh"), ("country", "UK"), ("units", "c")] {
        assert_eq!(
            calls[0].arguments.get(field).and_then(Node::as_str),
            Some(value)
        );
    }

    // Deleting the final LF cuts the complete terminal SSE event.
    let anthropic_prefix = &ANTHROPIC[..ANTHROPIC.len() - 1];
    let openai_prefix = &OPENAI[..OPENAI.len() - 1];
    let cut_anthropic = recover(ProviderLogFormat::Anthropic, anthropic_prefix);
    let ProviderLogRecovery::Anthropic(result) = &cut_anthropic else {
        unreachable!("Anthropic response");
    };
    assert_eq!(result.usable(), [] as [&ToolCall; 0]);
    assert_eq!(result.log, anthropic_prefix);
    assert!(
        result
            .faults
            .iter()
            .any(|fault| fault.kind == EventFaultKind::PartialEvent)
    );

    let cut_openai = recover(ProviderLogFormat::OpenAiChat, openai_prefix);
    let ProviderLogRecovery::OpenAiChat(result) = &cut_openai else {
        unreachable!("OpenAI response");
    };
    assert_eq!(result.usable(), [] as [&ToolCall; 0]);
    assert_eq!(result.log, openai_prefix);
    assert!(
        result
            .faults
            .iter()
            .any(|fault| fault.kind == ChatFaultKind::PartialEvent)
    );

    object([
        (
            "provider_log_digests",
            array([string(anthropic_digest), string(openai_digest)]),
        ),
        ("anthropic_weather", anthropic.to_node()),
        ("openai_weather", openai.to_node()),
        ("anthropic_truncated", withheld(&cut_anthropic)),
        ("openai_truncated", withheld(&cut_openai)),
    ])
}

fn withheld(result: &ProviderLogRecovery) -> Node {
    assert_eq!(result.exit_code(), 4);
    let wire = result.to_node();
    assert_eq!(wire.pointer("/value/usable_calls").unwrap().to_json(), "0");
    assert_eq!(
        wire.pointer("/value/emitted/output").unwrap().to_json(),
        "[]"
    );
    wire
}
