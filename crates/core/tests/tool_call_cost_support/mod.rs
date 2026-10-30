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
    CallOutcome, DialectId, DialectSet, Effects, ObjectSpec, Param, ToolCalls, ToolDefinition,
    ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{AssistResult, Budget, Effort, Governor, Outcome, StopReason};

pub fn tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("write")
            .effects(Effects::ReadOnly)
            .param("content", Param::string().required())],
        Budget::default(),
    )
    .expect("tools compile")
}

pub fn glm(content: &str) -> String {
    format!(
        "<tool_call>write\n<arg_key>content</arg_key><arg_value>{content}</arg_value></tool_call>"
    )
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

pub fn gemma3(content: &str) -> String {
    format!("```tool_code\nwrite(content=\"{content}\")\n```\n")
}

pub fn pythonic(content: &str) -> String {
    format!("[write(content=\"{content}\")] ")
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
    let result = tools.recover(
        input.as_bytes(),
        request,
        purrassist_core::tool_call::DialectSet::built_in(),
    );
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
        ("glm45", glm as fn(&str) -> String),
        ("hermes", hermes),
        ("pythonic", pythonic),
        ("gemma3", gemma3),
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
            let limit = bound(DialectSet::built_in(), input.len());
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
        ("glm45", glm(&content), true),
        ("hermes", hermes(&content), true),
        ("pythonic", pythonic(&content), true),
        ("gemma3", gemma3(&content), true),
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
            let request = ToolRecoveryRequest::sized_for(Effort::Standard, input.as_bytes(), &set);
            let result = tools.recover(input.as_bytes(), &request, &set);
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

/// A read-only tool with an array of objects and an array of strings, to
/// repeat, and the `write` tool the dialect shapes call.
fn dense_tools() -> ToolSet {
    ToolSet::compile(
        [
            ToolDefinition::new("write")
                .effects(Effects::ReadOnly)
                .param("content", Param::string().required()),
            ToolDefinition::new("tree")
                .effects(Effects::ReadOnly)
                .param(
                    "children",
                    Param::array(Param::object(
                        ObjectSpec::new().param("name", Param::string().required()),
                    )),
                )
                .param("tags", Param::array(Param::string())),
        ],
        Budget::default(),
    )
    .expect("tools compile")
}

/// The dense-markup shapes `docs/RESOURCES.md` ("Dense markup") measures:
/// output that is nothing but one small unit repeated, in every built-in
/// dialect. Each is `(label, wrapper before, unit, wrapper after, what the
/// units become)`.
const DENSE: [(&str, &str, &str, &str, Dense); 20] = [
    (
        "gemma3-calls",
        "",
        "```tool_code\nwrite(content='a')\n```\n",
        "",
        Dense::Calls,
    ),
    (
        "glm45-calls",
        "",
        "<tool_call>write\n<arg_key>content</arg_key><arg_value>a</arg_value></tool_call>\n",
        "",
        Dense::Calls,
    ),
    (
        "pythonic-calls",
        "",
        "[write(content='a')]\n",
        "",
        Dense::Calls,
    ),
    (
        "qwen-nested-items",
        "<tool_call>\n<function=tree>\n",
        "<parameter=children><parameter=name>a</parameter></parameter>\n",
        "</function>\n</tool_call>",
        Dense::Items("children"),
    ),
    (
        "qwen-json-array-items",
        "<tool_call>\n<function=tree>\n",
        "<parameter=tags>[\"a\"]</parameter>\n",
        "</function>\n</tool_call>",
        Dense::Items("tags"),
    ),
    (
        "qwen-string-items",
        "<tool_call>\n<function=tree>\n",
        "<parameter=tags>a</parameter>\n",
        "</function>\n</tool_call>",
        Dense::Items("tags"),
    ),
    (
        "qwen-calls",
        "",
        "<tool_call>\n<function=write>\n<parameter=content>\na\n</parameter>\n</function>\n</tool_call>\n",
        "",
        Dense::Calls,
    ),
    (
        "hermes-calls",
        "",
        "<tool_call>\n{\"name\": \"write\", \"arguments\": {\"content\": \"a\"}}\n</tool_call>\n",
        "",
        Dense::Calls,
    ),
    (
        "openai-calls",
        "",
        "{\"id\": \"c\", \"type\": \"function\", \"function\": {\"name\": \"write\", \"arguments\": \"{\\\"content\\\": \\\"a\\\"}\"}}\n",
        "",
        Dense::Calls,
    ),
    (
        "mistral-v3-calls",
        "",
        "[TOOL_CALLS] [{\"name\": \"write\", \"arguments\": {\"content\": \"a\"}, \"id\": \"a1B2c3D4e\"}]\n",
        "",
        Dense::Calls,
    ),
    (
        "mistral-v3-array",
        "[TOOL_CALLS] [",
        "{\"name\": \"write\", \"arguments\": {\"content\": \"a\"}, \"id\": \"a1B2c3D4e\"}, ",
        "{\"name\": \"write\", \"arguments\": {\"content\": \"a\"}, \"id\": \"a1B2c3D4e\"}]",
        Dense::ArrayOfCalls,
    ),
    (
        "mistral-v11-calls",
        "",
        "[TOOL_CALLS]write[CALL_ID]a1B2c3D4e[ARGS]{\"content\": \"a\"}</s>\n",
        "",
        Dense::Calls,
    ),
    (
        "mistral-v11-chained-calls",
        "",
        "[TOOL_CALLS]write[CALL_ID]a1B2c3D4e[ARGS]{\"content\": \"a\"}\n",
        "",
        Dense::Calls,
    ),
    (
        "harmony-calls",
        "",
        "<|start|>assistant<|channel|>commentary to=functions.write <|constrain|>json<|message|>{\"content\": \"a\"}<|call|>",
        "",
        Dense::Calls,
    ),
    (
        "deepseek-v3.1-calls",
        "<｜tool▁calls▁begin｜>",
        "<｜tool▁call▁begin｜>write<｜tool▁sep｜>{\"content\": \"a\"}<｜tool▁call▁end｜>",
        "<｜tool▁calls▁end｜>",
        Dense::Calls,
    ),
    (
        "deepseek-v3-calls",
        "<｜tool▁calls▁begin｜>",
        "<｜tool▁call▁begin｜>function<｜tool▁sep｜>write\n```json\n{\"content\": \"a\"}\n```<｜tool▁call▁end｜>",
        "<｜tool▁calls▁end｜>",
        Dense::Calls,
    ),
    (
        "kimi-calls",
        "<|tool_calls_section_begin|>",
        "<|tool_call_begin|>functions.write:0<|tool_call_argument_begin|>{\"content\": \"a\"}<|tool_call_end|>",
        "<|tool_calls_section_end|>",
        Dense::Calls,
    ),
    (
        "llama3-calls",
        "",
        "{\"name\": \"write\", \"parameters\": {\"content\": \"a\"}}\n",
        "",
        Dense::Calls,
    ),
    (
        "anthropic-calls",
        "",
        "{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"write\",\"input\":{\"content\":\"a\"}}\n",
        "",
        Dense::Calls,
    ),
    (
        "bare-json-empty-calls",
        "",
        "{\"name\":\"tree\",\"arguments\":{}}",
        "",
        Dense::Calls,
    ),
];

/// What the repeated units of a dense shape become.
#[derive(Clone, Copy)]
enum Dense {
    /// One call each.
    Calls,
    /// One call each, and one more in the closing text: the elements of one
    /// JSON array.
    ArrayOfCalls,
    /// One item each of the named array parameter of the one call.
    Items(&'static str),
}

/// Dense markup of `bytes` bytes in the shape `label` recovers under plain
/// `ToolRecoveryRequest::sized(Effort::Standard, output)`, with no raised
/// limit: the stop is `Completed` and every unit (call or array item) is
/// recovered. Mistral v3 calls are measured both as one `[TOOL_CALLS]`
/// section each and as the elements of one JSON array; v11 calls both
/// separated by `</s>` and as one whitespace-separated chain.
fn dense_fits_published_budget(label: &str, bytes: usize) {
    let tools = dense_tools();
    let &(_, before, unit, after, expected) = DENSE
        .iter()
        .find(|shape| shape.0 == label)
        .expect("a dense shape");
    let units = (bytes - before.len() - after.len()) / unit.len();
    let input = format!("{before}{}{after}", unit.repeat(units));
    let request = ToolRecoveryRequest::sized(Effort::Standard, input.as_bytes());
    let result = tools.recover(input.as_bytes(), &request, DialectSet::built_in());
    let n = input.len() as u64;
    let usage = result.receipt.usage;
    // Per byte, in thousandths.
    let per_byte = |charged: u64| charged.saturating_mul(1000) / n;
    println!(
        "{label}: {n} bytes, {units} units, work {} / 1000 per byte, peak scratch {} / 1000, evidence {} / 1000 ({} entries), {:?}",
        per_byte(usage.work),
        per_byte(usage.peak_scratch_bytes),
        per_byte(usage.evidence_entries),
        usage.evidence_entries,
        result.receipt.stopped
    );
    assert_eq!(result.receipt.stopped, StopReason::Completed, "{label}");
    let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = &result.outcome else {
        panic!("{label}: {:?}", result.outcome)
    };
    match expected {
        Dense::Calls | Dense::ArrayOfCalls => {
            let expected_calls = units + usize::from(matches!(expected, Dense::ArrayOfCalls));
            assert_eq!(calls.calls.len(), expected_calls, "{label}");
            assert!(
                calls
                    .calls
                    .iter()
                    .all(|call| call.outcome == CallOutcome::Resolved),
                "{label}"
            );
        }
        Dense::Items(name) => {
            assert_eq!(calls.calls.len(), 1, "{label}");
            let items = calls.calls[0].arguments.get(name).expect("the parameter");
            let purrassist_core::json::Value::Array(items) = &items.value else {
                panic!("{label}: {items:?}")
            };
            assert_eq!(items.len(), units, "{label}");
        }
    }
}

/// [`dense_fits_published_budget`] for every dense shape, each on its own
/// thread (the shapes are independent recoveries).
pub fn dense_markup_fits_published_budget(bytes: usize) {
    std::thread::scope(|scope| {
        let handles: Vec<_> = DENSE
            .iter()
            .map(|shape| scope.spawn(move || dense_fits_published_budget(shape.0, bytes)))
            .collect();
        for handle in handles {
            if let Err(panic) = handle.join() {
                std::panic::resume_unwind(panic);
            }
        }
    });
}
