// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The tool-call cost scale checks shared by `tool_call_cost.rs` (`make
//! check`, small arguments) and `tool_call_scale.rs` (`make corpus`, the
//! 1 MiB arguments `docs/RESOURCES.md` quotes): each check takes its sizes,
//! so both lanes run the same assertions.
#![allow(unreachable_pub)]
use purrassist_core::grammar::cost;
use purrassist_core::json::Node;
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, Param, ToolCalls, ToolDefinition,
    ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{AssistResult, Budget, Effort, Governor, Outcome};

pub fn tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("write")
            .effects(Effects::ReadOnly)
            .param("content", Param::string().required())],
        Budget::default(),
    )
    .expect("tools compile")
}

pub fn qwen(content: &str) -> String {
    format!(
        "<tool_call>\n<function=write>\n<parameter=content>\n{content}\n</parameter>\n</function>\n</tool_call>"
    )
}

pub fn openai(content: &str) -> String {
    let arguments = format!("{{\"content\": \"{content}\"}}");
    format!(
        "{{\"id\": \"call_1\", \"type\": \"function\", \"function\": {{\"name\": \"write\", \"arguments\": {}}}}}",
        purrassist_core::json::quote(&arguments)
    )
}

pub fn hermes(content: &str) -> String {
    format!(
        "<tool_call>\n{{\"name\": \"write\", \"arguments\": {{\"content\": \"{content}\"}}}}\n</tool_call>"
    )
}

pub fn llama3(content: &str) -> String {
    format!("{{\"name\": \"write\", \"parameters\": {{\"content\": \"{content}\"}}}}")
}

pub fn mistral(content: &str) -> String {
    format!(
        "[TOOL_CALLS] [{{\"name\": \"write\", \"arguments\": {{\"content\": \"{content}\"}}, \"id\": \"a1B2c3D4e\"}}]"
    )
}

pub fn mistral11(content: &str) -> String {
    format!("[TOOL_CALLS]write[CALL_ID]a1B2c3D4e[ARGS]{{\"content\": \"{content}\"}}")
}

pub fn deepseek31(content: &str) -> String {
    format!(
        "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>write<｜tool▁sep｜>{{\"content\": \"{content}\"}}<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
    )
}

pub fn anthropic(content: &str) -> String {
    format!(
        "{{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"write\",\"input\":{{\"content\":\"{content}\"}}}}"
    )
}

pub fn harmony(content: &str) -> String {
    format!(
        "<|start|>assistant<|channel|>commentary to=functions.write <|constrain|>json<|message|>{{\"content\": \"{content}\"}}<|call|>"
    )
}

pub fn bare(content: &str) -> String {
    format!("{{\"name\": \"write\", \"arguments\": {{\"content\": \"{content}\"}}}}")
}

pub fn kimi(content: &str) -> String {
    format!(
        "<|tool_calls_section_begin|><|tool_call_begin|>functions.write:0<|tool_call_argument_begin|>{{\"content\": \"{content}\"}}<|tool_call_end|><|tool_calls_section_end|>"
    )
}

pub fn deepseek3(content: &str) -> String {
    format!(
        "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>write\n```json\n{{\"content\": \"{content}\"}}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
    )
}

/// The published bound for `n` bytes over `set` with no error
/// configuration explored (`E = 0`).
pub fn bound(set: &DialectSet, n: usize) -> u64 {
    let per_dialect: u64 = set
        .dialects()
        .iter()
        .map(|dialect| cost::K_THREAD * dialect.grammar().configuration_bound())
        .sum();
    (2 + cost::K_LEX + per_dialect) * n as u64
}

/// Recover `input` and require the single call `Resolved` with `content`.
pub fn resolved(
    tools: &ToolSet,
    input: &str,
    request: &ToolRecoveryRequest,
    content: &str,
) -> AssistResult<ToolCalls> {
    let result = tools.recover(input.as_bytes(), request);
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("{:?}", result.receipt.stopped)
    };
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[0]
            .arguments
            .get("content")
            .and_then(Node::as_str),
        Some(content)
    );
    result
}

/// A string argument of `small` and of `large` bytes, printed in every
/// dialect but `OpenAI`, recovers on the fast path (no repair recorded)
/// within the published work bound, and the work ratio between the two
/// sizes is within 1.05x of the byte ratio.
pub fn argument_work_bound(small: usize, large: usize) {
    let tools = tools();
    // One budget sized for twice the large scale serves both scales.
    let request = ToolRecoveryRequest::new(Effort::Standard).budget(
        ToolRecoveryRequest::sized_budget(Effort::Standard, 2 * large, 2),
    );
    for (label, print) in [
        ("qwen", qwen as fn(&str) -> String),
        ("hermes", hermes),
        ("llama3", llama3),
        ("mistral", mistral),
        ("mistral11", mistral11),
        ("deepseek31", deepseek31),
        ("deepseek3", deepseek3),
        ("kimi", kimi),
        ("anthropic", anthropic),
        ("harmony", harmony),
        ("bare", bare),
    ] {
        let mut measured = Vec::new();
        for size in [small, large] {
            let content = "a".repeat(size);
            let input = print(&content);
            let result = resolved(&tools, &input, &request, &content);
            // The fast path explores no error configuration (E = 0): no
            // repair is recorded.
            assert!(
                result
                    .receipt
                    .evidence
                    .iter()
                    .all(|evidence| evidence.rule == "tool_call.xml.framing-newline"),
                "{label}: {:?}",
                result.receipt.evidence
            );
            let work = result.receipt.usage.work;
            let limit = bound(&request.dialects, input.len());
            println!(
                "{label} {} bytes: work {work}, published bound {limit}",
                input.len()
            );
            assert!(work <= limit, "{label}: work {work} exceeds {limit}");
            measured.push((input.len() as u128, u128::from(work)));
        }
        // (work ratio) / (byte ratio) within [1/1.05, 1.05], exactly.
        let ((small_bytes, small_work), (large_bytes, large_work)) = (measured[0], measured[1]);
        let work_ratio = large_work * small_bytes;
        let byte_ratio = small_work * large_bytes;
        assert!(
            work_ratio * 100 <= byte_ratio * 105 && byte_ratio * 100 <= work_ratio * 105,
            "{label}: {small} -> {large} byte work ratio is not within 1.05x of the byte ratio"
        );
    }
}

/// C4: a `size`-byte string argument recovers under the unmodified
/// `Effort::Maximum` preset and under `ToolRecoveryRequest::sized` for every
/// effort from `Quick` to `Maximum`, with identical charges in every case
/// and work and peak scratch within `RECOVER_WORK_PER_BYTE` and
/// `RECOVER_SCRATCH_PER_BYTE`. From 1 MiB, where the fixed scratch of a
/// recovery no longer dominates, peak scratch is also at most 16 bytes per
/// input byte for Qwen3-Coder XML, Hermes, Llama 3 JSON, Mistral v3, Mistral
/// v11, DeepSeek-V3.1, DeepSeek-V3, Kimi-K2, Anthropic, Harmony and bare JSON
/// arguments (`docs/RESOURCES.md` quotes these measurements). `OpenAI`
/// stringified arguments decode their escaped string again, so they are held
/// to `RECOVER_SCRATCH_PER_BYTE` alone.
pub fn argument_fits_published_budget(size: usize) {
    let tools = tools();
    let content = "a".repeat(size);
    for (label, input, sixteen) in [
        ("qwen", qwen(&content), true),
        ("hermes", hermes(&content), true),
        ("llama3", llama3(&content), true),
        ("mistral", mistral(&content), true),
        ("mistral11", mistral11(&content), true),
        ("deepseek31", deepseek31(&content), true),
        ("deepseek3", deepseek3(&content), true),
        ("kimi", kimi(&content), true),
        ("anthropic", anthropic(&content), true),
        ("harmony", harmony(&content), true),
        ("bare", bare(&content), true),
        ("openai", openai(&content), false),
    ] {
        let len = input.len();
        let mut usages = Vec::new();
        if sixteen {
            let preset = resolved(
                &tools,
                &input,
                &ToolRecoveryRequest::new(Effort::Maximum),
                &content,
            );
            assert_eq!(preset.receipt.budget, Effort::Maximum.budget());
            usages.push(preset.receipt.usage);
        }
        for effort in [
            Effort::Quick,
            Effort::Standard,
            Effort::Deep,
            Effort::Maximum,
        ] {
            let sized = resolved(
                &tools,
                &input,
                &ToolRecoveryRequest::sized(effort, input.as_bytes()),
                &content,
            );
            usages.push(sized.receipt.usage);
        }
        let usage = usages[0];
        // The charges are a function of the input alone.
        assert!(usages.iter().all(|other| *other == usage), "{label}");
        println!(
            "{label}: {len} bytes, work {}, peak scratch {}",
            usage.work, usage.peak_scratch_bytes
        );
        assert!(usage.work <= cost::RECOVER_WORK_PER_BYTE * len as u64);
        assert!(usage.peak_scratch_bytes <= cost::RECOVER_SCRATCH_PER_BYTE * len as u64);
        if sixteen && size >= 1 << 20 {
            assert!(
                usage.peak_scratch_bytes <= 16 * len as u64,
                "{label}: peak scratch {} over 16 bytes per input byte",
                usage.peak_scratch_bytes
            );
        }
    }
}

/// Adding a dialect to the allow-list adds at most its published
/// per-byte term to the work of recovering a `size`-byte argument.
pub fn work_independent_of_dialect_count(size: usize) {
    let tools = tools();
    let content = "a".repeat(size);
    for input in [
        qwen(&content),
        hermes(&content),
        llama3(&content),
        mistral(&content),
        mistral11(&content),
        deepseek31(&content),
        deepseek3(&content),
        kimi(&content),
        anthropic(&content),
        harmony(&content),
        bare(&content),
    ] {
        let n = input.len() as u64;
        let mut previous: Option<(u64, Vec<DialectId>)> = None;
        let order = [
            DialectId::Qwen3CoderXml,
            DialectId::Hermes,
            DialectId::OpenAiToolCalls,
            DialectId::Llama3Json,
            DialectId::MistralV3,
            DialectId::MistralV11,
            DialectId::DeepSeekV31,
            DialectId::DeepSeekV3,
            DialectId::KimiK2,
            DialectId::AnthropicToolUse,
            DialectId::Harmony,
            DialectId::BareJson,
        ];
        for count in 1..=order.len() {
            let dialects = order[..count].to_vec();
            let set = DialectSet::new(dialects.clone(), &mut Governor::new(Budget::default()))
                .expect("dialects");
            let request = ToolRecoveryRequest::sized_for(Effort::Standard, input.as_bytes(), set);
            let result = tools.recover(input.as_bytes(), &request);
            assert!(
                matches!(result.outcome, Outcome::Resolved(_)),
                "{dialects:?}"
            );
            let work = result.receipt.usage.work;
            if let Some((before, known)) = &previous {
                let added: u64 = dialects
                    .iter()
                    .filter(|dialect| !known.contains(dialect))
                    .map(|dialect| cost::K_THREAD * dialect.grammar().configuration_bound())
                    .sum();
                println!(
                    "{} -> {} dialects: work {before} -> {work} (+{} per byte; published term {added})",
                    known.len(),
                    dialects.len(),
                    work.saturating_sub(*before) / n
                );
                assert!(
                    work <= before + added * n,
                    "adding dialects added more than their published per-byte term"
                );
            }
            previous = Some((work, dialects));
        }
    }
}
