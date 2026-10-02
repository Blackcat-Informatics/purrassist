// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The published tool-call recovery cost bound (A10, C4): the constants of
//! `grammar::cost` and the built-in dialects' configuration bounds equal the
//! values `docs/RESOURCES.md` ("Tool-call recovery cost bound") publishes;
//! 1 MiB arguments satisfy the published work bound, scale linearly, and
//! recover under the published budgets; recognition charges the same for
//! every way the output is chunked; and adding a dialect adds at most its
//! published per-dialect term per byte.
use purrassist_core::grammar::cost;
use purrassist_core::json::Node;
use purrassist_core::tool_call::{
    CallOutcome, DialectId, DialectSet, Effects, GenerationStop, Param, ReasoningPolicy,
    Recognizer, ToolCalls, ToolDefinition, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{
    AssistResult, Budget, Effort, Governor, Outcome, StopReason, contract::Resource,
};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const RESOURCES: &str = include_str!("../../../docs/RESOURCES.md");
const COST_SOURCE: &str = include_str!("../src/grammar/cost.rs");
const SECTION: &str = "## Tool-call recovery cost bound";
/// The published bound, exactly as `docs/RESOURCES.md` states it.
const BOUND: &str = "work ≤ (2 + K_LEX + Σ_d K_d)·n + K_ERR·E,   K_d = K_THREAD·B_d";

/// Every `grammar::cost` constant and its value.
fn constants() -> Vec<(&'static str, u64)> {
    vec![
        ("K_LEX", cost::K_LEX),
        ("K_THREAD", cost::K_THREAD),
        ("K_EVENT", cost::K_EVENT),
        ("K_CAPTURE", cost::K_CAPTURE),
        ("K_ERR", cost::K_ERR),
        ("K_COMPILE", cost::K_COMPILE),
        ("K_PRINT", cost::K_PRINT),
        ("K_WITNESS", cost::K_WITNESS),
        ("K_CODEC", cost::K_CODEC),
        ("JSON_CODEC_STATE", cost::JSON_CODEC_STATE),
        ("CLOSER_VARIANT", cost::CLOSER_VARIANT),
        ("TERMINAL_CONFUSABLE", cost::TERMINAL_CONFUSABLE),
        ("CONSUMED_CLOSER", cost::CONSUMED_CLOSER),
        ("INSERT_CLOSER", cost::INSERT_CLOSER),
        ("DELETE_STRAY_CLOSER", cost::DELETE_STRAY_CLOSER),
        ("REASONING_UNCLOSED", cost::REASONING_UNCLOSED),
        ("REJECT", cost::REJECT),
        ("MISSING_PARAMETER_CLOSE", cost::MISSING_PARAMETER_CLOSE),
        ("FRAMING_NEWLINE", cost::FRAMING_NEWLINE),
        ("STRINGIFIED_ARGUMENTS", cost::STRINGIFIED_ARGUMENTS),
        ("JSON_REPAIR", cost::JSON_REPAIR),
        ("JSON_GUESS", cost::JSON_GUESS),
        (
            "STRAY_CLOSER_GAP",
            u64::try_from(cost::STRAY_CLOSER_GAP).expect("a small gap"),
        ),
        ("QUEUED_REPORT", cost::QUEUED_REPORT),
        ("EXTRACT_PROSE", cost::EXTRACT_PROSE),
        ("EXTRACT_FENCE", cost::EXTRACT_FENCE),
        ("RECOVER_WORK_PER_BYTE", cost::RECOVER_WORK_PER_BYTE),
        ("RECOVER_SCRATCH_PER_BYTE", cost::RECOVER_SCRATCH_PER_BYTE),
        (
            "RECOVER_OPENER_CANDIDATE_DIVISOR",
            cost::RECOVER_OPENER_CANDIDATE_DIVISOR,
        ),
    ]
}

/// The `| `name` | value | ...` rows of the published section.
fn published() -> BTreeMap<String, u64> {
    let start = RESOURCES.find(SECTION).expect("the cost bound section");
    let section = &RESOURCES[start + SECTION.len()..];
    let section = &section[..section.find("\n## ").unwrap_or(section.len())];
    assert!(
        section.contains(BOUND),
        "the section states the bound `{BOUND}`"
    );
    let mut rows = BTreeMap::new();
    for line in section.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once("` | ") else {
            continue;
        };
        let value = rest.split(" |").next().unwrap_or_default().replace(',', "");
        if let Ok(value) = value.parse::<u64>() {
            assert!(
                rows.insert(name.to_owned(), value).is_none(),
                "`{name}` published twice"
            );
        }
    }
    rows
}

#[test]
fn resources_doc_matches_cost_constants() {
    let rows = published();
    let constants = constants();
    // Every `pub const` of `grammar::cost` is listed here, and so published.
    let declared: Vec<&str> = COST_SOURCE
        .lines()
        .filter_map(|line| line.strip_prefix("pub const "))
        .map(|rest| rest.split(':').next().expect("a constant name"))
        .collect();
    let listed: Vec<&str> = constants.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        declared, listed,
        "every grammar::cost constant is checked, in source order"
    );
    for (name, value) in &constants {
        assert_eq!(
            rows.get(*name),
            Some(value),
            "docs/RESOURCES.md publishes `{name}` = {value}"
        );
    }
    let mut dialects = 0;
    for dialect in DialectId::BUILT_IN {
        let bound = dialect.grammar().configuration_bound();
        assert_eq!(
            rows.get(dialect.id()),
            Some(&bound),
            "docs/RESOURCES.md publishes B_d of `{}`",
            dialect.id()
        );
        dialects += 1;
    }
    assert_eq!(
        rows.len(),
        constants.len() + dialects,
        "no unchecked published value: {rows:?}"
    );
}

fn tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("write")
            .effects(Effects::ReadOnly)
            .param("content", Param::string().required())],
        Budget::default(),
    )
    .expect("tools compile")
}

fn qwen(content: &str) -> String {
    format!(
        "<tool_call>\n<function=write>\n<parameter=content>\n{content}\n</parameter>\n</function>\n</tool_call>"
    )
}

fn openai(content: &str) -> String {
    let arguments = format!("{{\"content\": \"{content}\"}}");
    format!(
        "{{\"id\": \"call_1\", \"type\": \"function\", \"function\": {{\"name\": \"write\", \"arguments\": {}}}}}",
        purrassist_core::json::quote(&arguments)
    )
}

fn hermes(content: &str) -> String {
    format!(
        "<tool_call>\n{{\"name\": \"write\", \"arguments\": {{\"content\": \"{content}\"}}}}\n</tool_call>"
    )
}

fn llama3(content: &str) -> String {
    format!("{{\"name\": \"write\", \"parameters\": {{\"content\": \"{content}\"}}}}")
}

fn mistral(content: &str) -> String {
    format!(
        "[TOOL_CALLS] [{{\"name\": \"write\", \"arguments\": {{\"content\": \"{content}\"}}, \"id\": \"a1B2c3D4e\"}}]"
    )
}

fn mistral11(content: &str) -> String {
    format!("[TOOL_CALLS]write[CALL_ID]a1B2c3D4e[ARGS]{{\"content\": \"{content}\"}}")
}

/// The published bound for `n` bytes over `set` with no error
/// configuration explored (`E = 0`).
fn bound(set: &DialectSet, n: usize) -> u64 {
    let per_dialect: u64 = set
        .dialects()
        .iter()
        .map(|dialect| cost::K_THREAD * dialect.grammar().configuration_bound())
        .sum();
    (2 + cost::K_LEX + per_dialect) * n as u64
}

/// Recover `input` and require the single call `Resolved` with `content`.
fn resolved(
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

#[test]
fn one_mib_argument_work_bound() {
    let tools = tools();
    // One budget sized for 2 MiB serves both scales.
    let request = ToolRecoveryRequest::new(Effort::Standard).budget(
        ToolRecoveryRequest::sized_budget(Effort::Standard, 2 << 20, 2),
    );
    for (label, print) in [
        ("qwen", qwen as fn(&str) -> String),
        ("hermes", hermes),
        ("llama3", llama3),
        ("mistral", mistral),
        ("mistral11", mistral11),
    ] {
        let mut measured = Vec::new();
        for size in [1 << 16, 1 << 20] {
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
            "{label}: 64 KiB -> 1 MiB work ratio is not within 1.05x of the byte ratio"
        );
    }
}

/// C4: a 1 MiB string argument recovers under the unmodified
/// `Effort::Maximum` preset and under `ToolRecoveryRequest::sized` for every
/// effort from `Quick` to `Maximum`, with identical charges in every case
/// and peak scratch at most 16 bytes per input byte for Qwen3-Coder XML,
/// Hermes, Llama 3 JSON, Mistral v3 and Mistral v11 arguments (`docs/RESOURCES.md` quotes these
/// measurements). `OpenAI`
/// stringified arguments decode their escaped string again, so they are
/// held to `RECOVER_SCRATCH_PER_BYTE` instead, under `sized` budgets.
#[test]
fn one_mib_argument_fits_published_budget() {
    let tools = tools();
    let content = "a".repeat(1 << 20);
    for (label, input, sixteen) in [
        ("qwen", qwen(&content), true),
        ("hermes", hermes(&content), true),
        ("llama3", llama3(&content), true),
        ("mistral", mistral(&content), true),
        ("mistral11", mistral11(&content), true),
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
        if sixteen {
            assert!(
                usage.peak_scratch_bytes <= 16 * len as u64,
                "{label}: peak scratch {} over 16 bytes per input byte",
                usage.peak_scratch_bytes
            );
        }
    }
}

/// A representative turn: reasoning, prose, a Qwen3-Coder XML call with a
/// repair, and a Hermes call.
fn turn() -> String {
    let body = "line of text with <angle> brackets and \"quotes\"\n".repeat(64);
    format!(
        "<think>plan the edit</think>Writing now.\n{}\n<tool_call>\n<function=write>\n<parameter=content>\nshort\n</parameter>\n</tool_call>\n{}",
        qwen(&body),
        hermes("done")
    )
}

#[test]
fn push_chunking_identical_work() {
    let tools = tools();
    let set = DialectSet::built_in();
    let input = turn();
    let mut usages = Vec::new();
    for chunk in [1, 16, 4096] {
        let mut governor = Governor::new(Budget::default());
        let mut recognizer =
            Recognizer::with_tools(&set, &tools, ReasoningPolicy::default(), &mut governor)
                .expect("recognizer");
        for piece in input.as_bytes().chunks(chunk) {
            recognizer.push(piece, &mut governor).expect("push");
        }
        let turn = recognizer
            .finish(GenerationStop::Stop, &mut governor)
            .expect("finish");
        assert_eq!(turn.calls().len(), 3, "chunk {chunk}");
        usages.push((chunk, governor.usage(), turn));
    }
    for (chunk, usage, turn) in &usages[1..] {
        assert_eq!(*usage, usages[0].1, "chunk {chunk}: usage");
        assert_eq!(*turn, usages[0].2, "chunk {chunk}: recognized turn");
    }
    assert!(usages[0].1.work > 0);
}

#[test]
fn work_independent_of_dialect_count_beyond_bound() {
    let tools = tools();
    let content = "a".repeat(1 << 16);
    for input in [
        qwen(&content),
        hermes(&content),
        llama3(&content),
        mistral(&content),
        mistral11(&content),
    ] {
        let n = input.len() as u64;
        let mut previous: Option<(u64, Vec<DialectId>)> = None;
        for dialects in [
            vec![DialectId::Qwen3CoderXml],
            vec![DialectId::Qwen3CoderXml, DialectId::Hermes],
            vec![
                DialectId::Qwen3CoderXml,
                DialectId::Hermes,
                DialectId::OpenAiToolCalls,
            ],
            vec![
                DialectId::Qwen3CoderXml,
                DialectId::Hermes,
                DialectId::OpenAiToolCalls,
                DialectId::Llama3Json,
            ],
            vec![
                DialectId::Qwen3CoderXml,
                DialectId::Hermes,
                DialectId::OpenAiToolCalls,
                DialectId::Llama3Json,
                DialectId::MistralV3,
            ],
            vec![
                DialectId::Qwen3CoderXml,
                DialectId::Hermes,
                DialectId::OpenAiToolCalls,
                DialectId::Llama3Json,
                DialectId::MistralV3,
                DialectId::MistralV11,
            ],
        ] {
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

/// Declarations of a directory listing tool, as an integrator passes them:
/// adapter tools are `Mutating`, so every call runs under `Structural`.
const LIST_DIR: &str = r#"[{"type":"function","function":{"name":"list_dir","parameters":{"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]}}}]"#;

/// A busy turn: `calls` Qwen3-Coder XML calls, each missing a
/// `</parameter>` (the `tool_call.markup.missing-parameter-close` repair),
/// each followed by a stray `<tool_call>` line that both Qwen3-Coder XML
/// and Hermes attempt and reject, so every attempt is a contested span.
fn busy_turn(calls: usize) -> String {
    let mut turn = String::new();
    for index in 0..calls {
        let _ = write!(
            turn,
            "<tool_call>\n<function=list_dir>\n<parameter=depth>\n{index}\n<parameter=path>\nsrc/{index}\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n"
        );
    }
    turn
}

/// G7: the candidate budget is sized to the turn. A 64-call turn whose
/// every call needs a markup repair, with a contested stray opener after
/// each, exhausts the fixed `Effort::Standard` preset explicitly, and is
/// recovered with every call `Resolved` under `ToolRecoveryRequest::sized`:
/// `⌈128 / 8⌉ = 16` candidates for each of its 256 openers (64 calls and 64
/// stray lines, each opened by two dialects). Competing segmentations of
/// the stray lines that recover the same calls are folded per span, so the
/// readings charged grow with the calls instead of doubling per call.
#[test]
fn busy_turn_resolves_every_call_under_sized() {
    let tools = ToolSet::from_openai(LIST_DIR, Budget::default()).expect("declarations");
    let output = busy_turn(64);
    let preset = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::new(Effort::Standard),
    );
    assert_eq!(preset.outcome, Outcome::Unresolved);
    assert_eq!(
        preset.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Candidates)
    );
    assert!(preset.receipt.evidence.is_empty() && preset.receipt.alternatives.is_empty());

    let openers = DialectSet::built_in().call_openers(output.as_bytes());
    assert_eq!(openers, 256);
    let request = ToolRecoveryRequest::sized(Effort::Standard, output.as_bytes());
    assert_eq!(request.budget.candidates, 128 + 16 * 256);
    let result = tools.recover(output.as_bytes(), &request);
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("{:?}", result.receipt.stopped)
    };
    assert_eq!(result.receipt.stopped, StopReason::Completed);
    assert_eq!(calls.calls.len(), 64);
    for (index, call) in calls.calls.iter().enumerate() {
        assert_eq!(call.outcome, CallOutcome::Resolved, "call {index}");
        assert_eq!(
            call.arguments.to_json(),
            format!(r#"{{"depth":{index},"path":"src/{index}"}}"#)
        );
        assert!(
            call.repairs
                .evidence
                .iter()
                .any(|evidence| evidence.rule == "tool_call.markup.missing-parameter-close"),
            "call {index}"
        );
    }
    assert_eq!(calls.usable().len(), 64);
    // Every stray opener is a recorded rejection naming both dialects.
    assert_eq!(calls.rejected.len(), 64);
    for rejected in &calls.rejected {
        assert_eq!(
            rejected.candidate_dialects,
            [DialectId::Hermes, DialectId::Qwen3CoderXml]
        );
    }
    // One reading per contested span plus the selected reading: linear.
    assert_eq!(result.receipt.usage.candidates, 64 * 2 + 1);
    assert_eq!(result.receipt.alternatives.len(), 0);
}

/// Competing readings that genuinely differ per call (a near-miss tool
/// name and a repeated parameter on a read-only tool) are charged per call;
/// the sized budget grows with the calls, so 64 of them recover (each call
/// `Ambiguous` between its two readings) where the fixed preset exhausts.
#[test]
fn per_call_competition_scales_with_calls() {
    let tools = ToolSet::compile(
        [ToolDefinition::new("write_file")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required())
            .param("content", Param::string().required())],
        Budget::default(),
    )
    .expect("tools");
    let output = "<tool_call>\n<function=write_fiel>\n<parameter=path>\na\n</parameter>\n<parameter=path>\nc\n</parameter>\n<parameter=content>\nb\n</parameter>\n</function>\n</tool_call>\n".repeat(64);
    let preset = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::new(Effort::Standard),
    );
    assert_eq!(
        preset.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Candidates)
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::sized(Effort::Standard, output.as_bytes()),
    );
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("{:?}", result.receipt.stopped)
    };
    assert_eq!(calls.calls.len(), 64);
    assert!(
        calls
            .calls
            .iter()
            .all(|call| call.outcome == CallOutcome::Ambiguous && call.alternatives.len() == 1)
    );
}

/// The sized budget stays finite: a crafted call whose readings multiply
/// (16 parameters each written twice: 2^16 readings) exhausts `Candidates`
/// explicitly at every effort, with no value and no ledger.
#[test]
fn crafted_reading_explosion_exhausts_explicitly() {
    let mut definition = ToolDefinition::new("f").effects(Effects::ReadOnly);
    let mut body = String::new();
    for index in 0..16 {
        definition = definition.param(format!("p{index}"), Param::string());
        let _ = write!(
            body,
            "<parameter=p{index}>\na\n</parameter>\n<parameter=p{index}>\nb\n</parameter>\n"
        );
    }
    let tools = ToolSet::compile([definition], Budget::default()).expect("tools");
    let output = format!("<tool_call>\n<function=f>\n{body}</function>\n</tool_call>");
    for effort in [
        Effort::Quick,
        Effort::Standard,
        Effort::Deep,
        Effort::Maximum,
    ] {
        let request = ToolRecoveryRequest::sized(effort, output.as_bytes());
        let result = tools.recover(output.as_bytes(), &request);
        assert_eq!(result.outcome, Outcome::Unresolved, "{effort:?}");
        assert_eq!(
            result.receipt.stopped,
            StopReason::BudgetExceeded(Resource::Candidates),
            "{effort:?}"
        );
        assert_eq!(result.receipt.usage.candidates, request.budget.candidates);
        assert!(result.receipt.evidence.is_empty() && result.receipt.losses.is_empty());
        assert_eq!(result.receipt.alternatives.len(), 0);
        assert_eq!(
            result.to_node().get("value").map(Node::to_json).as_deref(),
            Some("null")
        );
    }
}

/// The published sizing formula, for every effort.
#[test]
fn sized_budget_follows_the_published_formula() {
    let output = busy_turn(3);
    let openers = DialectSet::built_in().call_openers(output.as_bytes());
    assert_eq!(openers, 12);
    for effort in [
        Effort::Exact,
        Effort::Quick,
        Effort::Standard,
        Effort::Deep,
        Effort::Maximum,
    ] {
        let preset = effort.budget();
        let budget = ToolRecoveryRequest::sized(effort, output.as_bytes()).budget;
        let n = output.len() as u64;
        assert_eq!(budget.work, preset.work + cost::RECOVER_WORK_PER_BYTE * n);
        assert_eq!(
            budget.scratch_bytes,
            preset.scratch_bytes + cost::RECOVER_SCRATCH_PER_BYTE * n
        );
        assert_eq!(budget.input_bytes, preset.input_bytes.max(n));
        assert_eq!(
            budget.candidates,
            preset.candidates
                + preset
                    .candidates
                    .div_ceil(cost::RECOVER_OPENER_CANDIDATE_DIVISOR)
                    * openers
        );
        assert_eq!(
            (budget.depth, budget.evidence_entries, budget.helper_calls),
            (preset.depth, preset.evidence_entries, preset.helper_calls)
        );
    }
    // Restricting the allow-list counts only its dialects' openers.
    let hermes = DialectSet::new([DialectId::Hermes], &mut Governor::new(Budget::default()))
        .expect("dialects");
    assert_eq!(hermes.call_openers(output.as_bytes()), 6);
}
