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
    Recognizer, ToolDefinition, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{Budget, Effort, Governor, Outcome, StopReason, contract::Resource};
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
        ("GROUP_SPLIT", cost::GROUP_SPLIT),
        ("CLOSER_MISSPELLED", cost::CLOSER_MISSPELLED),
        ("TOKEN_MISSPELLED", cost::TOKEN_MISSPELLED),
        ("CLOSER_IN_VALUE", cost::CLOSER_IN_VALUE),
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
        ("UNDECLARED_NAME", cost::UNDECLARED_NAME),
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

mod tool_call_cost_support;
use tool_call_cost_support::{self as support, hermes, qwen, tools};

/// The work bound and linear scaling, from 64 KiB to 256 KiB (`make
/// corpus` runs 64 KiB to 1 MiB in `tool_call_scale`).
#[test]
fn argument_work_bound() {
    support::argument_work_bound(1 << 16, 1 << 18);
}

/// T6: closer text inside a value costs work linear in the output. A
/// Qwen3-Coder XML value holding one closer per 256 bytes (`</parameter>`,
/// the call's `</function>`, a `</function></tool_call>` pair, or a whole
/// `</parameter></function></tool_call>` sequence) recovers under
/// `ToolRecoveryRequest::sized` with the whole value (the reading that holds
/// the closers), or, for the whole sequence, `Ambiguous` between the two
/// equally clean readings; from 32 KiB (128 closers) to 256 KiB (1,024
/// closers) the work ratio is within 1.05x of the byte ratio, so no reading
/// is read once per closer, and the work per byte stays within
/// `RECOVER_WORK_PER_BYTE` per reading reported.
#[test]
fn closer_text_in_values_is_linear() {
    let tools = tools();
    for token in [
        "</parameter>",
        "</function>",
        "</function></tool_call>",
        "</parameter></function></tool_call>",
    ] {
        let mut measured = Vec::new();
        for size in [1 << 15, 1 << 18] {
            let segment = format!("{}{token}", "a".repeat(256 - token.len()));
            let content = format!("{}z", segment.repeat(size / 256));
            let input = qwen(&content);
            let result = tools.recover(
                input.as_bytes(),
                &ToolRecoveryRequest::sized(Effort::Standard, input.as_bytes()),
            );
            match (
                &result.outcome,
                token.starts_with("</parameter></function>"),
            ) {
                (Outcome::Resolved(calls), false) => {
                    let call = &calls.calls[0];
                    assert_eq!(call.outcome, CallOutcome::Resolved, "{token} {size}");
                    assert_eq!(
                        call.arguments.get("content").and_then(Node::as_str),
                        Some(content.as_str()),
                        "{token} {size}"
                    );
                }
                (Outcome::Ambiguous(alternatives), true) => {
                    assert_eq!(alternatives.len(), 2, "{token} {size}");
                }
                (other, _) => panic!("{token} {size}: {other:?} {:?}", result.receipt.stopped),
            }
            let work = result.receipt.usage.work;
            println!("{token} {} bytes: work {work}", input.len());
            // A value read once costs no more per byte than the sized
            // budget grants; one reported twice (Ambiguous), twice that.
            let readings = if token.starts_with("</parameter></function>") {
                2
            } else {
                1
            };
            assert!(
                work <= readings * cost::RECOVER_WORK_PER_BYTE * input.len() as u64,
                "{token} {size}: work {work} exceeds {readings} x RECOVER_WORK_PER_BYTE per byte"
            );
            measured.push((input.len() as u128, u128::from(work)));
        }
        let ((small_bytes, small_work), (large_bytes, large_work)) = (measured[0], measured[1]);
        let work_ratio = large_work * small_bytes;
        let byte_ratio = small_work * large_bytes;
        assert!(
            work_ratio * 100 <= byte_ratio * 105 && byte_ratio * 100 <= work_ratio * 105,
            "{token}: 32 KiB -> 256 KiB work ratio is not within 1.05x of the byte ratio"
        );
    }
}

/// C4's published per-byte bounds on a 16 KiB argument (`make corpus` runs
/// the 1 MiB argument, with its 16-bytes-per-byte scratch ceiling, in
/// `tool_call_scale`).
#[test]
fn argument_fits_published_budget() {
    support::argument_fits_published_budget(1 << 14);
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
    support::work_independent_of_dialect_count(1 << 14);
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
/// `⌈128 / 8⌉ = 16` candidates for each of its 320 openers (64 calls, each
/// opened at `<tool_call>` by two dialects and at `<function=` by one, and 64
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
    assert_eq!(openers, 320);
    let request = ToolRecoveryRequest::sized(Effort::Standard, output.as_bytes());
    assert_eq!(request.budget.candidates, 128 + 16 * 320);
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
    assert!(
        matches!(preset.receipt.stopped, StopReason::BudgetExceeded(_)),
        "{:?}",
        preset.receipt.stopped
    );
    // The readings are charged per call: with the preset's candidates
    // alone limiting it (its work budget lifted), the preset runs out of
    // candidates, not of anything else.
    let mut candidates_only = ToolRecoveryRequest::new(Effort::Standard);
    candidates_only.budget.work = u64::MAX / 4;
    let limited = tools.recover(output.as_bytes(), &candidates_only);
    assert_eq!(
        limited.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Candidates)
    );
    assert_eq!(
        limited.receipt.usage.candidates,
        candidates_only.budget.candidates
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
/// (16 parameters each written twice: 2^16 readings) exhausts explicitly at
/// every effort, with no value and no ledger: `Candidates`, except at
/// `Maximum`, whose larger per-opener candidate share (the call's
/// `<tool_call>` and `<function=` openers) lets the readings outgrow
/// `ScratchBytes` first.
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
        let exhausted = if effort == Effort::Maximum {
            Resource::ScratchBytes
        } else {
            Resource::Candidates
        };
        assert_eq!(
            result.receipt.stopped,
            StopReason::BudgetExceeded(exhausted),
            "{effort:?}"
        );
        if exhausted == Resource::Candidates {
            assert_eq!(result.receipt.usage.candidates, request.budget.candidates);
        }
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
    // Per call: `<tool_call>` (Qwen3-Coder XML and Hermes) and
    // `<function=` (a bare Qwen3-Coder XML element); per stray line:
    // `<tool_call>` twice.
    assert_eq!(openers, 15);
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

/// The unparsed-markup detector reads the content linearly: on prose and
/// on inputs dense with every family's anchor, its work grows with the
/// content within 1.05x of the byte ratio from 64 KiB to 1 MiB, at most
/// 32 units per byte (`docs/RESOURCES.md` quotes these measurements).
#[test]
fn unparsed_markup_scan_is_linear() {
    use purrassist_core::tool_call::detect_unparsed;
    let hermes = DialectSet::new([DialectId::Hermes], &mut Governor::new(Budget::default()))
        .expect("dialects");
    for unit in [
        "The quick (brown) fox [1] jumps over f(x) = 2x.\n",
        "\n[f(a=1",
        "{\"name\": \"x\", \"arguments\": {",
        "<tool_call>x<arg_key>",
        "to=functions.\n",
        "<|channel|>to=functions.\n",
        "[TOOL_CALLS]a\n",
        "<｜tool▁call▁begin｜>",
        "```tool_code\nf(",
        "functions.x:0{",
        "\nab{\"",
        "\"function\": {\"name\": \"x\", \"arguments\": ",
    ] {
        let mut measured = Vec::new();
        for size in [1 << 16, 1 << 20] {
            let content = unit.repeat(size / unit.len());
            let mut governor = Governor::new(Budget {
                work: u64::MAX / 4,
                ..Budget::default()
            });
            let whole = 0..content.len();
            detect_unparsed(
                content.as_bytes(),
                core::slice::from_ref(&whole),
                &hermes,
                &mut governor,
            )
            .expect("detected");
            let work = governor.usage().work;
            println!("{unit:?} {} bytes: work {work}", content.len());
            assert!(work <= 32 * content.len() as u64, "{unit:?}: {work}");
            measured.push((content.len() as u128, u128::from(work)));
        }
        let ((small_bytes, small_work), (large_bytes, large_work)) = (measured[0], measured[1]);
        let work_ratio = large_work * small_bytes;
        let byte_ratio = small_work * large_bytes;
        assert!(
            work_ratio * 100 <= byte_ratio * 105 && byte_ratio * 100 <= work_ratio * 105,
            "{unit:?}: work is not linear in the content"
        );
    }
}

/// S2: schema typing of unmodeled parameters has no count cap and stays
/// governed. A Qwen3-Coder XML call with 300 pydantic `Optional[int]`
/// parameters resolves fully at `Standard` under `ToolRecoveryRequest::sized`
/// (the values are decided in batched rounds, a handful of validations
/// whatever their number); the same call under a budget too small for it
/// is the explicit `BudgetExceeded`, never a quiet verbatim fallback; and
/// values that interact through a shared ancestor (an `anyOf` over a `$ref`
/// object written as nested parameters) are still typed.
#[test]
fn schema_typing_is_uncapped_and_governed() {
    let count = 300;
    let properties: Vec<String> = (0..count)
        .map(|index| format!(r#""p{index}":{{"anyOf":[{{"type":"integer"}},{{"type":"null"}}]}}"#))
        .collect();
    let declaration = format!(
        r#"[{{"type":"function","function":{{"name":"wide","parameters":{{"type":"object","properties":{{{}}}}}}}}}]"#,
        properties.join(",")
    );
    let tools = ToolSet::from_openai(&declaration, Budget::default()).expect("adapter tools");
    let mut body = String::new();
    for index in 0..count {
        write!(body, "<parameter=p{index}>\n{index}\n</parameter>\n").expect("write");
    }
    let output = format!("<tool_call>\n<function=wide>\n{body}</function>\n</tool_call>");
    let request = ToolRecoveryRequest::sized(Effort::Standard, output.as_bytes());
    let result = tools.recover(output.as_bytes(), &request);
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("{:?} {:?}", result.outcome, result.receipt.stopped)
    };
    let call = &calls.calls[0];
    assert_eq!(call.outcome, CallOutcome::Resolved);
    for index in 0..count {
        assert_eq!(
            call.arguments.get(&format!("p{index}")).map(Node::to_json),
            Some(index.to_string())
        );
    }
    let typed = call
        .repairs
        .evidence
        .iter()
        .filter(|evidence| evidence.rule == "tool_call.value.schema-typed")
        .count();
    assert_eq!(typed, count);

    // Starved: explicit exhaustion, never a quiet verbatim call.
    let work = result.receipt.usage.work;
    let starved = tools.recover(
        output.as_bytes(),
        &request.clone().budget(Budget {
            work: work * 9 / 10,
            ..request.budget
        }),
    );
    assert_eq!(starved.outcome, Outcome::Unresolved);
    assert!(
        matches!(starved.receipt.stopped, StopReason::BudgetExceeded(_)),
        "{:?}",
        starved.receipt.stopped
    );

    // Interacting values: an `anyOf` over a `$ref` object, nested, whose
    // integer member must be typed and whose numeric-looking string member
    // must not (flipping both fails the shared `anyOf`).
    let tools = ToolSet::from_openai(
        r##"[{"type":"function","function":{"name":"view","parameters":{"type":"object","properties":{"opts":{"anyOf":[{"$ref":"#/$defs/O"},{"type":"null"}]}},"$defs":{"O":{"type":"object","properties":{"depth":{"type":"integer"},"label":{"type":"string"}},"required":["depth","label"]}}}}}]"##,
        Budget::default(),
    )
    .expect("adapter tools");
    let output = "<tool_call>\n<function=view>\n<parameter=opts>\n<parameter=depth>\n2\n</parameter>\n<parameter=label>\n42\n</parameter>\n</parameter>\n</function>\n</tool_call>";
    let result = tools.recover(output.as_bytes(), &ToolRecoveryRequest::default());
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("{:?}", result.outcome)
    };
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[0].arguments.to_json(),
        r#"{"opts":{"depth":2,"label":"42"}}"#
    );
}
