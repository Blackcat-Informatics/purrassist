// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The tool-call recovery operation `tool_call.recover.v1`: outcomes,
//! termination, tool choice, ids, policies, keywords, receipts, budgets,
//! and determinism.
use purrassist_core::contract::{InferenceKind, Resource};
use purrassist_core::digest::Digest;
use purrassist_core::grammar::edit::LossKind;
use purrassist_core::grammar::{Grammar, cost};
use purrassist_core::json::{self, Node, Value};
use purrassist_core::tool_call::emit;
use purrassist_core::tool_call::{
    AmbiguityPolicy, CallOutcome, DialectId, DialectSet, Effects, Finish, FromNode, FromNodeError,
    GenerationStop, IdGenerator, IdPolicy, Param, PolicySelection, ReasoningPolicy, Rejection,
    RepairPolicy, StrictForced, ToolCall, ToolCalls, ToolChoice, ToolDefinition,
    ToolRecoveryRequest, ToolSet, UnresolvedReason, turn_id_prefix,
};
use purrassist_core::wire::WireValue;
use purrassist_core::{
    AssistResult, Budget, CancellationToken, Effort, Governor, Outcome, StopReason,
};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

fn compile(definitions: impl IntoIterator<Item = ToolDefinition>) -> ToolSet {
    ToolSet::compile(definitions, Budget::default()).expect("tools compile")
}

fn set(dialects: &[DialectId]) -> DialectSet {
    DialectSet::new(
        dialects.iter().cloned(),
        &mut Governor::new(Budget::default()),
    )
    .expect("dialect set")
}

fn request(dialect: DialectId) -> (ToolRecoveryRequest, DialectSet) {
    (ToolRecoveryRequest::default(), set(&[dialect]))
}

/// The single value of a `Resolved` or `BestGuess` outcome.
fn value(result: &AssistResult<ToolCalls>) -> &ToolCalls {
    match &result.outcome {
        Outcome::Resolved(value) | Outcome::BestGuess(value) => value,
        other => panic!("expected one value: {other:#?}\n{:#?}", result.receipt),
    }
}

fn hermes(name: &str, arguments: &str) -> String {
    format!("<tool_call>\n{{\"name\": \"{name}\", \"arguments\": {arguments}}}\n</tool_call>")
}

fn rules(call: &ToolCall) -> Vec<&str> {
    call.repairs
        .evidence
        .iter()
        .map(|evidence| evidence.rule.as_str())
        .collect()
}

fn string_member(node: &Node, key: &str) -> Option<String> {
    node.get(key).and_then(Node::as_str).map(str::to_owned)
}

#[test]
fn every_anchor_fixture_resolves() {
    assert_anchor_fixtures_resolve(Effort::Standard, &DialectId::BUILT_IN, false);
}

/// Every anchor fixture of each of `dialects` is `Resolved`, top-level and
/// per call, at `effort`: with that dialect alone, or with every built-in
/// dialect allowed (the default request) when `all` is set.
fn assert_anchor_fixtures_resolve(effort: Effort, dialects: &[DialectId], all: bool) {
    for dialect in dialects {
        let dialect = dialect.clone();
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/tool-calls/conformance")
            .join(dialect.id());
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
            .expect("fixture directory")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();
        assert!(paths.len() >= 3, "{}", dialect.id());
        for path in paths {
            let file = path.display().to_string();
            let document = json::parse(&std::fs::read_to_string(&path).expect("fixture"))
                .unwrap_or_else(|error| panic!("{file}: {error:?}"));
            let markup = string_member(&document, "markup").expect("markup");
            let tools = ToolSet::from_openai(
                &document.get("tools").expect("tools").to_json(),
                Budget::default(),
            )
            .expect("fixture tools");
            let Some(Value::Array(expected)) =
                document.get("expected_calls").map(|node| &node.value)
            else {
                panic!("{file}: expected_calls");
            };
            let pinned;
            let dialects = if all {
                DialectSet::built_in()
            } else {
                pinned = set(core::slice::from_ref(&dialect));
                &pinned
            };
            let result = tools.recover(
                markup.as_bytes(),
                &ToolRecoveryRequest::new(effort),
                dialects,
            );
            assert_eq!(result.receipt.stopped, StopReason::Completed, "{file}");
            for evidence in &result.receipt.evidence {
                let rule = purrassist_core::tool_call::rules::rule(&evidence.rule).expect("rule");
                assert!(
                    effort != Effort::Exact || !rule.is_repair(),
                    "{file}: {rule:?}"
                );
            }
            let Outcome::Resolved(calls) = &result.outcome else {
                panic!("{file}: {:#?}", result.outcome);
            };
            assert_eq!(calls.calls.len(), expected.len(), "{file}");
            assert_eq!(calls.rejected.len(), 0, "{file}");
            assert_eq!(calls.finish, Finish::Calls, "{file}");
            assert_eq!(calls.usable().len(), expected.len(), "{file}");
            for (call, expected) in calls.calls.iter().zip(expected) {
                assert_eq!(call.outcome, CallOutcome::Resolved, "{file}: {call:#?}");
                assert_eq!(
                    call.tool.as_deref(),
                    string_member(expected, "name").as_deref(),
                    "{file}"
                );
                assert_eq!(
                    call.arguments.to_json(),
                    expected.get("arguments").expect("arguments").to_json(),
                    "{file}"
                );
                assert_eq!(&call.dialect, &dialect, "{file}");
            }
            assert_eq!(result.receipt.stopped, StopReason::Completed, "{file}");
        }
    }
}

#[test]
fn operation_id_is_tool_call_recover_v1() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = hermes("ls", "{}");
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    assert_eq!(result.receipt.operation, "tool_call.recover.v1");
    assert_eq!(
        result.receipt.operation,
        purrassist_core::tool_call::RECOVER_OPERATION
    );
    assert_eq!(result.receipt.source, Digest::of(output.as_bytes()));
    assert_eq!(result.receipt.sources[0].bytes, output.as_bytes());
    assert_eq!(result.receipt.effort, Effort::Standard);
    let node = result.to_node();
    assert_eq!(
        node.pointer("/receipt/operation").and_then(Node::as_str),
        Some("tool_call.recover.v1")
    );
    assert_eq!(
        node.pointer("/value/calls/0/tool").and_then(Node::as_str),
        Some("ls")
    );
}

fn read_tool() -> ToolSet {
    compile([ToolDefinition::new("read")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())])
}

#[test]
fn length_vs_stop_truncation() {
    let tools = read_tool();
    let complete = hermes("read", r#"{"path": "a.rs"}"#);
    let partial = "<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"b";
    let dialects = set(&[DialectId::Hermes]);
    let length = ToolRecoveryRequest::default().stop(GenerationStop::Length);
    // A length stop with only a cut-off call.
    let result = tools.recover(partial.as_bytes(), &length, &dialects);
    let calls = value(&result);
    assert_eq!(calls.finish, Finish::Truncated);
    assert_eq!(calls.openai_finish_reason(), "length");
    assert_eq!(
        calls.calls[0].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::Truncated
        }
    );
    assert!(rules(&calls.calls[0]).contains(&"tool_call.truncated"));
    assert!(calls.calls[0].repairs.data_loss());
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
    assert_eq!(calls.usable().len(), 0);
    // A length stop after a complete call.
    let both = format!("{complete}\n{partial}");
    let result = tools.recover(both.as_bytes(), &length, &dialects);
    let calls = value(&result);
    assert_eq!(calls.finish, Finish::TruncatedWithCalls);
    assert_eq!(calls.usable().len(), 1);
    assert_eq!(calls.calls.len(), 2);
    // A length stop with no call at all is still not a clean finish.
    let result = tools.recover(b"Hello", &length, &dialects);
    assert_eq!(value(&result).finish, Finish::Truncated);
    // The same text under a natural stop is malformed, never truncated.
    // `finish_reason` names the emitted calls: nothing for the cut call
    // alone, the complete call beside the rejected attempt.
    for (text, reason) in [(partial.to_owned(), "stop"), (both, "tool_calls")] {
        let result = tools.recover(
            text.as_bytes(),
            &ToolRecoveryRequest::default(),
            &set(&[DialectId::Hermes]),
        );
        let calls = value(&result);
        assert_eq!(calls.finish, Finish::Unresolved, "{text}");
        assert_eq!(calls.openai_finish_reason(), reason, "{text}");
        assert!(
            calls.calls.iter().all(|call| call.outcome
                != CallOutcome::Unresolved {
                    reason: UnresolvedReason::Truncated
                }),
            "{text}"
        );
        assert!(
            !result
                .receipt
                .evidence
                .iter()
                .any(|evidence| evidence.rule == "tool_call.truncated")
        );
        // The partial call is rejected or malformed; only the complete call is usable.
        let malformed = calls.rejected.len()
            + calls
                .calls
                .iter()
                .filter(|call| {
                    matches!(
                        call.outcome,
                        CallOutcome::Unresolved {
                            reason: UnresolvedReason::Malformed { .. }
                        }
                    )
                })
                .count();
        assert_eq!(malformed, 1, "{text}: {calls:#?}");
    }
}

#[test]
fn stop_sequence_consumed_closer() {
    let tools = read_tool();
    let text = "<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"a.rs\"}}\n";
    let result = tools.recover(
        text.as_bytes(),
        &ToolRecoveryRequest::default()
            .stop(GenerationStop::StopSequence(b"</tool_call>".to_vec())),
        &set(&[DialectId::Hermes]),
    );
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("{:#?}", result.outcome)
    };
    let call = &calls.calls[0];
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert!(rules(call).contains(&"tool_call.stop.consumed-closer"));
    assert!(call.repairs.inferences.is_empty() && !call.repairs.data_loss());
    assert_eq!(calls.finish, Finish::Calls);
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "tool_call.stop.consumed-closer")
    );
}

#[test]
fn unknown_stop_is_conservative() {
    let tools = read_tool();
    let partial = "<tool_call>\n{\"name\": \"read\", \"arguments\": {\"path\": \"b";
    for stop in [
        GenerationStop::Unknown,
        GenerationStop::Stop,
        GenerationStop::ContentFilter,
        GenerationStop::ToolCalls,
    ] {
        let result = tools.recover(
            partial.as_bytes(),
            &ToolRecoveryRequest::default().stop(stop.clone()),
            &set(&[DialectId::Hermes]),
        );
        let calls = value(&result);
        assert_eq!(calls.finish, Finish::Unresolved, "{stop:?}");
        assert_eq!(calls.usable().len(), 0, "{stop:?}");
        assert_eq!(calls.usable_accepting_guesses().len(), 0, "{stop:?}");
        assert!(
            !result
                .receipt
                .evidence
                .iter()
                .any(|evidence| evidence.rule == "tool_call.truncated"),
            "{stop:?}"
        );
    }
}

/// Every resource a recovery charges, with the budget that exhausts it.
#[test]
fn exhaustion_is_explicit_every_resource() {
    let built_in = DialectSet::built_in();
    let tools = compile([ToolDefinition::new("read_file")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    // A near-miss name and a stray closer: work, candidates, scratch and
    // evidence are all charged.
    let output = format!("{}</tool_call>", hermes("read_fiel", r#"{"path": "a.rs"}"#));
    let output = output.as_bytes();
    let baseline = tools.recover(output, &ToolRecoveryRequest::default(), built_in);
    assert!(matches!(baseline.outcome, Outcome::BestGuess(_)));
    let usage = baseline.receipt.usage;
    assert!(usage.work > 0 && usage.candidates > 0 && usage.evidence_entries > 0);
    let defaults = Budget::default();
    let budgets = [
        (
            Resource::Work,
            Budget {
                work: usage.work - 1,
                ..defaults
            },
        ),
        (
            Resource::Candidates,
            Budget {
                candidates: usage.candidates - 1,
                ..defaults
            },
        ),
        (
            Resource::InputBytes,
            Budget {
                input_bytes: output.len() as u64 - 1,
                ..defaults
            },
        ),
        (
            Resource::ScratchBytes,
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..defaults
            },
        ),
        (
            Resource::Depth,
            Budget {
                depth: 0,
                ..defaults
            },
        ),
        (
            Resource::EvidenceEntries,
            Budget {
                evidence_entries: usage.evidence_entries - 1,
                ..defaults
            },
        ),
    ];
    for (resource, budget) in budgets {
        let result = tools.recover(
            output,
            &ToolRecoveryRequest::default().budget(budget),
            built_in,
        );
        assert_eq!(result.outcome, Outcome::Unresolved, "{resource:?}");
        assert_eq!(
            result.receipt.stopped,
            StopReason::BudgetExceeded(resource),
            "{resource:?}"
        );
        assert_eq!(result.receipt.source, Digest::of(output), "{resource:?}");
        assert_eq!(result.receipt.evidence.len(), 0, "{resource:?}");
        assert_eq!(result.receipt.losses.len(), 0, "{resource:?}");
        assert_eq!(result.receipt.inferences.len(), 0, "{resource:?}");
        assert_eq!(result.receipt.validations.len(), 0, "{resource:?}");
        assert_eq!(result.receipt.alternatives.len(), 0, "{resource:?}");
        assert_eq!(result.receipt.budget, budget, "{resource:?}");
        let node = result.to_node();
        assert_eq!(node.get("value").unwrap().to_json(), "null");
        assert_eq!(
            node.pointer("/receipt/termination/kind")
                .and_then(Node::as_str),
            Some("budget_exceeded")
        );
    }
    assert_source_retention_limits(&tools, output, built_in);
}

fn assert_source_retention_limits(tools: &ToolSet, output: &[u8], built_in: &DialectSet) {
    let defaults = Budget::default();
    // Retention runs first: an input over the byte limit is identified by
    // digest but not retained.
    let result = tools.recover(
        output,
        &ToolRecoveryRequest::default().budget(Budget {
            input_bytes: 1,
            ..defaults
        }),
        built_in,
    );
    assert_eq!(result.receipt.sources.len(), 0);
    // A budget that only retention fits is reported as the search's
    // exhaustion, with the source retained.
    let retained = output.len() as u64 + 1;
    let result = tools.recover(
        output,
        &ToolRecoveryRequest::default().budget(Budget {
            work: retained,
            ..defaults
        }),
        built_in,
    );
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Work)
    );
    assert_eq!(result.receipt.sources[0].bytes, output);
}

/// G16 (security review A6 and P1, historical-fit risk 5): the `Depth`
/// charge of a nested object frame counts the frame being pushed, the
/// arguments object being level 1, so with `Budget { depth: d, .. }` the
/// `xml-params` codec holds objects down to level `d` (`d - 1` nested array
/// items) and refuses the next with an explicit `BudgetExceeded(Depth)`: no
/// panic, no string fallback, no partial value. Whole-call recovery charges
/// the same resource again where it reads the nested value back (JSON tree,
/// exact-schema validation), so it fits fewer levels under the same budget,
/// and refuses the first level that does not fit the same way.
#[test]
fn nested_item_depth_is_a_stack_guard() {
    use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, XmlParamsCodec};
    let built_in = DialectSet::built_in();
    // A recursive `$ref` schema: its compile cost does not grow with the
    // output's nesting.
    let tools = ToolSet::from_openai(
        r##"[{"type":"function","function":{"name":"tree","parameters":{"type":"object","properties":{"children":{"type":"array","items":{"$ref":"#/$defs/node"}}},"required":["children"],"$defs":{"node":{"type":"object","properties":{"name":{"type":"string"},"children":{"type":"array","items":{"$ref":"#/$defs/node"}}},"required":["name"]}}}}}]"##,
        Budget::default(),
    )
    .expect("tools compile");
    // `levels` nested items, the innermost at object level `levels + 1`.
    let body = |levels: usize| -> String {
        let mut body = String::new();
        for level in 0..levels {
            write!(
                body,
                "<parameter=children><parameter=name>n{level}</parameter>"
            )
            .expect("writing to a String");
        }
        body.push_str(&"</parameter>".repeat(levels));
        body
    };
    let turn = |levels: usize| {
        format!(
            "<tool_call>\n<function=tree>\n{}\n</function>\n</tool_call>",
            body(levels)
        )
    };
    let expected = |levels: usize| -> String {
        let mut json = format!(r#"{{"name":"n{}"}}"#, levels - 1);
        for level in (0..levels - 1).rev() {
            json = format!(r#"{{"name":"n{level}","children":[{json}]}}"#);
        }
        format!(r#"{{"children":[{json}]}}"#)
    };
    let read = |levels: usize, depth: u32| {
        let text = body(levels);
        let bytes = text.as_bytes();
        let context = CodecContext {
            shape: tools.argument_shape("tree"),
            ..CodecContext::default()
        };
        let mut governor = Governor::new(Budget {
            depth,
            ..Budget::default()
        });
        XmlParamsCodec::begin(0, &mut governor)
            .expect("begin")
            .finish(
                bytes,
                0..bytes.len(),
                CodecEnd::Delimited,
                context,
                &mut governor,
            )
    };
    for d in [2_u32, 3, 8, 40, 127] {
        // The codec: `d - 1` nested items fit, `d` do not.
        let levels = d as usize - 1;
        let value = read(levels, d).expect("within the depth").expect("read");
        assert_eq!(value.tree.root.to_json(), expected(levels), "depth {d}");
        assert_eq!(
            read(levels + 1, d).err(),
            Some(StopReason::BudgetExceeded(Resource::Depth)),
            "depth {d}: {} nested items",
            levels + 1
        );
        // Whole-call recovery: a level fits when the budget covers all three
        // layers' charges (`3 * levels + 1`); the first that does not is an
        // explicit exhaustion.
        let fits = (d as usize - 1) / 3;
        let request = ToolRecoveryRequest::default().budget(Budget {
            depth: d,
            ..Budget::default()
        });
        if fits > 0 {
            let result = tools.recover(turn(fits).as_bytes(), &request, built_in);
            let Outcome::Resolved(calls) = &result.outcome else {
                panic!("depth {d}: {fits} nested items: {:#?}", result.outcome);
            };
            assert_eq!(
                calls.calls[0].arguments.to_json(),
                expected(fits),
                "depth {d}"
            );
            assert_eq!(result.receipt.stopped, StopReason::Completed, "depth {d}");
        }
        let result = tools.recover(turn(fits + 1).as_bytes(), &request, built_in);
        assert_eq!(result.outcome, Outcome::Unresolved, "depth {d}");
        assert_eq!(
            result.receipt.stopped,
            StopReason::BudgetExceeded(Resource::Depth),
            "depth {d}: {} nested items",
            fits + 1
        );
        assert_eq!(
            result.to_node().get("value").unwrap().to_json(),
            "null",
            "depth {d}: no partial value"
        );
    }
    // The CLI and MCP fix the budget at the effort presets' depth, 128.
    assert_eq!(Budget::default().depth, 128);
    assert!(read(127, 128).is_ok_and(|value| value.is_ok()));
    assert_eq!(
        read(128, 128).err(),
        Some(StopReason::BudgetExceeded(Resource::Depth))
    );
}

#[test]
fn cancelled_before_recover() {
    let tools = read_tool();
    let output = hermes("read", r#"{"path": "a.rs"}"#);
    let token = CancellationToken::new();
    let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
    token.cancel();
    let result = tools.recover_governed(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
        &mut governor,
    );
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(result.receipt.stopped, StopReason::Cancelled);
    assert_eq!(result.receipt.source, Digest::of(output.as_bytes()));
    assert_eq!(result.receipt.sources.len(), 0);
    assert_eq!(result.receipt.usage, governor.usage());
    assert_eq!(result.receipt.usage.work, 0);
}

const MINI: &str = include_str!("data/grammar/mini.grammar");
const OVERLAP: &str = include_str!("data/grammar/overlap.grammar");

fn custom(text: &str) -> DialectId {
    DialectId::custom(
        Grammar::compile_governed(text, &mut Governor::new(Budget::default())).expect("grammar"),
    )
}

#[test]
fn strict_ambiguity_rejects() {
    // Per-call ambiguity: a repeated parameter has a first and a last reading.
    let tools = read_tool();
    let output = hermes("read", r#"{"path": "a.rs", "path": "b.rs"}"#);
    let report = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        &set(&[DialectId::Hermes]),
    );
    let calls = value(&report);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Ambiguous);
    assert_ne!(calls.calls[0].distinguishing.len(), 0);
    assert_eq!(calls.finish, Finish::Unresolved);
    assert_eq!(report.receipt.alternatives.len(), 0);
    let reject = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().ambiguity(AmbiguityPolicy::Reject),
        &set(&[DialectId::Hermes]),
    );
    assert_eq!(reject.outcome, Outcome::Unresolved);
    assert_eq!(reject.receipt.stopped, StopReason::NoCandidate);
    let ambiguity = reject
        .receipt
        .validations
        .iter()
        .find(|validation| validation.constraint == "tool_call.ambiguity")
        .expect("ambiguity validation");
    assert!(!ambiguity.passed);
    assert_eq!(ambiguity.validator, "purrassist.tool_call.v1");
    assert_eq!(reject.receipt.alternatives.len(), 2);
    assert_eq!(
        reject
            .receipt
            .to_node()
            .pointer("/ambiguity/derivations")
            .unwrap()
            .to_json(),
        "2"
    );
    // No ambiguity under Reject: the check passes.
    let clean = tools.recover(
        hermes("read", r#"{"path": "a.rs"}"#).as_bytes(),
        &ToolRecoveryRequest::default().ambiguity(AmbiguityPolicy::Reject),
        &set(&[DialectId::Hermes]),
    );
    assert!(matches!(clean.outcome, Outcome::Resolved(_)));
    assert!(
        clean
            .receipt
            .validations
            .iter()
            .any(|validation| validation.constraint == "tool_call.ambiguity" && validation.passed)
    );
    // Whole-message ambiguity: two dialects read the same span.
    let tools = compile([ToolDefinition::new("f").effects(Effects::ReadOnly)]);
    let both = set(&[custom(MINI), custom(OVERLAP)]);
    let output = b"say <tool><call=f></call></tool> done";
    let report = tools.recover(output, &ToolRecoveryRequest::default(), &both);
    let Outcome::Ambiguous(values) = &report.outcome else {
        panic!("{:#?}", report.outcome)
    };
    assert_eq!(values.len(), 2);
    assert_eq!(report.receipt.alternatives.len(), 2);
    let dialects: Vec<&str> = values
        .iter()
        .map(|value| value.calls[0].dialect.name())
        .collect();
    assert!(dialects.contains(&"mini.v1") && dialects.contains(&"overlap.v1"));
    for (value, alternative) in values.iter().zip(&report.receipt.alternatives) {
        assert_eq!(
            alternative.output,
            Digest::of(value.to_node().to_json().as_bytes())
        );
    }
    let reject = tools.recover(
        output,
        &ToolRecoveryRequest::default().ambiguity(AmbiguityPolicy::Reject),
        &both,
    );
    assert_eq!(reject.outcome, Outcome::Unresolved);
    assert_eq!(reject.receipt.stopped, StopReason::NoCandidate);
    assert_eq!(reject.receipt.alternatives.len(), 2);
    // Competing readings are counted, never truncated: 2^20 readings
    // exceed every preset's candidate budget.
    let many: Vec<u8> = (0..20)
        .flat_map(|index| format!("call {index} <tool><call=f></call></tool>\n").into_bytes())
        .collect();
    let result = tools.recover(&many, &ToolRecoveryRequest::new(Effort::Maximum), &both);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Candidates)
    );
}

#[test]
fn per_call_repairs_itemized() {
    let tools = compile([
        ToolDefinition::new("read_file")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required()),
        ToolDefinition::new("list_dir")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required()),
    ]);
    let output = format!(
        "{}\n<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</parameter>\n</function>\n</tool_call>",
        hermes("read_fiel", r#"{"path": "a.rs"}"#)
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("{:#?}", result.outcome)
    };
    let [first, second] = &calls.calls[..] else {
        panic!("{calls:#?}")
    };
    assert_eq!(first.outcome, CallOutcome::BestGuess);
    assert_eq!(rules(first), ["tool_call.name.match"]);
    assert_eq!(first.repairs.inferences[0].kind, InferenceKind::Guess);
    assert_eq!(first.repairs.inferences[0].value, "read_file");
    assert_eq!(second.outcome, CallOutcome::Resolved);
    assert!(rules(second).contains(&"tool_call.markup.stray-closer"));
    assert!(
        second
            .repairs
            .losses
            .iter()
            .all(|loss| loss.kind == LossKind::Markup)
    );
    // Every per-call entry reaches the receipt, in output order.
    let receipt: Vec<&str> = result
        .receipt
        .evidence
        .iter()
        .map(|evidence| evidence.rule.as_str())
        .collect();
    let expected: Vec<&str> = rules(first).into_iter().chain(rules(second)).collect();
    assert_eq!(receipt, expected);
    assert_eq!(result.receipt.inferences, first.repairs.inferences);
    // And the wire value itemizes them per call.
    let node = WireValue::to_node(calls);
    assert_eq!(
        node.pointer("/calls/0/repairs/inferences/0/kind")
            .and_then(Node::as_str),
        Some("guess")
    );
    assert_eq!(
        node.pointer("/calls/1/repairs/losses/0/kind")
            .and_then(Node::as_str),
        Some("markup")
    );
}

#[test]
fn single_policy_override_applies_to_all() {
    let tools = compile([
        ToolDefinition::new("read_file").effects(Effects::ReadOnly),
        ToolDefinition::new("delete_record").effects(Effects::Mutating),
    ]);
    let output = format!(
        "{}\n{}",
        hermes("read_fiel", "{}"),
        hermes("delete_recrod", "{}")
    );
    let per_tool = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let calls = value(&per_tool);
    assert_eq!(calls.calls[0].outcome, CallOutcome::BestGuess);
    assert!(matches!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved { .. }
    ));
    let tolerant = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().policy(PolicySelection::All(RepairPolicy::Tolerant)),
        DialectSet::built_in(),
    );
    for call in &value(&tolerant).calls {
        assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
        assert_eq!(call.policy, Some(RepairPolicy::Tolerant));
    }
    let structural = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().policy(PolicySelection::All(RepairPolicy::Structural)),
        DialectSet::built_in(),
    );
    for call in &value(&structural).calls {
        assert!(
            matches!(
                &call.outcome,
                CallOutcome::Unresolved {
                    reason: UnresolvedReason::RuleNotAdmitted { .. }
                }
            ),
            "{call:#?}"
        );
    }
}

/// The digest of a result's complete wire form.
fn wire_digest(result: &AssistResult<ToolCalls>) -> String {
    Digest::of(result.to_node().to_json().as_bytes()).to_string()
}

/// The dialects the pinned digests were recorded with (permuted by the
/// test below).
const PINNED_DIALECTS: [DialectId; 3] = [
    DialectId::Hermes,
    DialectId::OpenAiToolCalls,
    DialectId::Qwen3CoderXml,
];

#[test]
fn deterministic_pinned_digest() {
    let definitions = || {
        vec![
            ToolDefinition::new("read_file")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required())
                .param("limit", Param::integer()),
            ToolDefinition::new("list_dir")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required()),
            ToolDefinition::new("write_file")
                .param("path", Param::string().required())
                .param("content", Param::string().required()),
        ]
    };
    let output = format!(
        "<think>plan</think>Reading.\n{}\n<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</parameter>\n</function>\n</tool_call>\n{}",
        hermes("read_fiel", r#"{"path": "a.rs", "limit": 5}"#),
        hermes("write_file", r#"{"path": "b.rs", "content": "x"}"#)
    );
    let orders: [[usize; 3]; 3] = [[0, 1, 2], [2, 0, 1], [1, 2, 0]];
    let mut digests = Vec::new();
    for order in orders {
        let all = definitions();
        let tools = compile(order.iter().map(|&index| all[index].clone()));
        let dialects: Vec<DialectId> = order
            .iter()
            .map(|&index| PINNED_DIALECTS[index].clone())
            .collect();
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default(),
            &set(&dialects),
        );
        assert!(matches!(result.outcome, Outcome::BestGuess(_)));
        digests.push(wire_digest(&result));
    }
    assert!(
        digests.windows(2).all(|pair| pair[0] == pair[1]),
        "{digests:?}"
    );
    assert_eq!(digests[0], PINNED_DIGEST);
    // The explicit `Prefix` policy (`call_0`, `call_1`, ...) is pinned too:
    // its calls and ids are exactly the ones recovery published before ids
    // were numbered from the turn's digest by default (the digest also
    // covers the receipt's charged usage).
    let all = definitions();
    let prefixed = compile(all).recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().ids(IdPolicy::Prefix {
            prefix: "call_".into(),
            start: 0,
        }),
        &set(&PINNED_DIALECTS),
    );
    assert_eq!(wire_digest(&prefixed), PINNED_PREFIX_DIGEST);
    // The Ambiguous wire form is pinned too.
    let tools = compile([ToolDefinition::new("f").effects(Effects::ReadOnly)]);
    let mut ambiguous = Vec::new();
    for pair in [[MINI, OVERLAP], [OVERLAP, MINI]] {
        let result = tools.recover(
            b"say <tool><call=f></call></tool> done",
            &ToolRecoveryRequest::default(),
            &set(&[custom(pair[0]), custom(pair[1])]),
        );
        ambiguous.push(wire_digest(&result));
    }
    assert_eq!(ambiguous[0], ambiguous[1]);
    assert_eq!(ambiguous[0], PINNED_AMBIGUOUS_DIGEST);
    let prefixed = tools.recover(
        b"say <tool><call=f></call></tool> done",
        &ToolRecoveryRequest::default().ids(IdPolicy::Prefix {
            prefix: "call_".into(),
            start: 0,
        }),
        &set(&[custom(MINI), custom(OVERLAP)]),
    );
    assert_eq!(wire_digest(&prefixed), PINNED_PREFIX_AMBIGUOUS_DIGEST);
}

const PINNED_DIGEST: &str =
    "sha256:c28566579dbbdbae1864ecf260ff42e37bf3f16d6e5e03975736f4a2a02932b4";
const PINNED_PREFIX_DIGEST: &str =
    "sha256:836e51c26fed0803d19d515ca032de950356545e3f7f1d29099805514565086c";
const PINNED_AMBIGUOUS_DIGEST: &str =
    "sha256:f02fa417a5ef9c48a3319278e8062ad00ac1714781f5e41e66022633ffca9940";
const PINNED_PREFIX_AMBIGUOUS_DIGEST: &str =
    "sha256:7e1535246a60b2895d0e4bcbd52a907a3734df7f5e7758bf7288952b643661ce";

#[test]
fn invalid_utf8_is_loss_and_bytes_retained() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let mut output = b"hi \xff\xfe there ".to_vec();
    let at = output.len();
    output.extend_from_slice(hermes("ls", "{}").as_bytes());
    output.extend_from_slice(b"\n\xc3");
    let result = tools.recover(
        &output,
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("{:#?}", result.outcome)
    };
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(calls.calls[0].span.start, at);
    let losses: Vec<_> = result
        .receipt
        .losses
        .iter()
        .filter(|loss| loss.evidence.rule == "tool_call.input.invalid-utf8")
        .collect();
    assert_eq!(losses.len(), 2);
    assert_eq!(losses[0].evidence.span.as_ref().unwrap().bytes, 3..5);
    assert_eq!(losses[0].original, "\\xFF\\xFE");
    let end = output.len();
    assert_eq!(
        losses[1].evidence.span.as_ref().unwrap().bytes,
        end - 1..end
    );
    // Never replaced: the bytes are retained and the content ranges cover them.
    assert_eq!(result.receipt.sources[0].bytes, output);
    assert_eq!(result.receipt.sources[0].text(), None);
    assert!(calls.content.iter().any(|range| range.contains(&3)));
    let node = result.receipt.to_node();
    assert!(node.pointer("/sources/0/bytes_hex").is_some());
    assert!(!result.to_node().to_json().contains('\u{fffd}'));
    // `receipt_wire.rs` pins this receipt's exact wire form
    // (`tests/data/receipts/v3-tool-call-invalid-utf8.json`).
}

#[test]
fn forced_tool_choice_violation_recorded() {
    let tools = compile([
        ToolDefinition::new("read")
            .effects(Effects::ReadOnly)
            .param("path", Param::string()),
        ToolDefinition::new("list").effects(Effects::ReadOnly),
    ]);
    let output = format!(
        "{}\n{}",
        hermes("read", r#"{"path": "a"}"#),
        hermes("list", "{}")
    );
    let forced = ToolRecoveryRequest::default().tool_choice(ToolChoice::Forced("read".into()));
    let result = tools.recover(output.as_bytes(), &forced, DialectSet::built_in());
    let calls = value(&result);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceForced {
                expected: "read".into()
            }
        }
    );
    assert!(!calls.tool_choice.passed);
    assert_eq!(calls.finish, Finish::Unresolved);
    let validation = result
        .receipt
        .validations
        .iter()
        .find(|validation| validation.constraint == "tool_choice=forced:read")
        .expect("tool choice validation");
    assert!(!validation.passed);
    assert_eq!(validation.validator, "purrassist.tool_call.v1");
    assert_eq!(validation.diagnostics.len(), 1);
    // No call at all also violates a forced choice; a forced undeclared tool
    // is a request error.
    let result = tools.recover(b"no calls", &forced, DialectSet::built_in());
    assert!(!value(&result).tool_choice.passed);
    assert_eq!(value(&result).finish, Finish::Unresolved);
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().tool_choice(ToolChoice::Forced("write".into())),
        DialectSet::built_in(),
    );
    assert!(
        matches!(result.receipt.stopped, StopReason::InvalidInput(ref detail) if detail.contains("`write`"))
    );
    assert_eq!(result.outcome, Outcome::Unresolved);
    // Satisfied: only the forced tool is called.
    let result = tools.recover(
        hermes("read", "{}").as_bytes(),
        &forced,
        DialectSet::built_in(),
    );
    assert!(value(&result).tool_choice.passed);
    assert_eq!(value(&result).finish, Finish::Calls);
    // Required.
    let required = ToolRecoveryRequest::default().tool_choice(ToolChoice::Required);
    let result = tools.recover(b"no calls", &required, DialectSet::built_in());
    assert!(!value(&result).tool_choice.passed);
    assert_eq!(value(&result).finish, Finish::Unresolved);
    let result = tools.recover(output.as_bytes(), &required, DialectSet::built_in());
    assert!(value(&result).tool_choice.passed);
    assert_eq!(value(&result).finish, Finish::Calls);
}

#[test]
fn tool_choice_none_with_calls() {
    let tools = compile([ToolDefinition::new("list").effects(Effects::ReadOnly)]);
    let output = format!("Text.\n{}", hermes("list", "{}"));
    let none = ToolRecoveryRequest::default().tool_choice(ToolChoice::None);
    let result = tools.recover(output.as_bytes(), &none, DialectSet::built_in());
    let calls = value(&result);
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].tool.as_deref(), Some("list"));
    assert_eq!(
        calls.calls[0].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceNone
        }
    );
    assert_eq!(calls.usable().len(), 0);
    assert_eq!(calls.finish, Finish::Unresolved);
    assert!(!calls.tool_choice.passed);
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|validation| validation.constraint == "tool_choice=none" && !validation.passed)
    );
    let result = tools.recover(b"Only text.", &none, DialectSet::built_in());
    assert!(value(&result).tool_choice.passed);
    assert_eq!(value(&result).finish, Finish::ContentOnly);
}

struct Constant;

impl IdGenerator for Constant {
    fn id(&self, _: usize) -> String {
        "same".into()
    }
}

struct Numbered;

impl IdGenerator for Numbered {
    fn id(&self, call_index: usize) -> String {
        format!("toolu_{call_index}")
    }
}

#[test]
fn ids_never_collide_each_policy() {
    let tools = compile([ToolDefinition::new("list").effects(Effects::ReadOnly)]);
    let two = format!("{}\n{}", hermes("list", "{}"), hermes("list", "{}"));
    // Prefix, chained through `next_id_start` across three rounds.
    let mut start = 0;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..3 {
        let request = ToolRecoveryRequest::default().ids(IdPolicy::Prefix {
            prefix: "call_".into(),
            start,
        });
        let result = tools.recover(two.as_bytes(), &request, DialectSet::built_in());
        let calls = value(&result);
        for call in &calls.calls {
            assert!(seen.insert(call.id.clone()), "{} repeats", call.id);
        }
        assert_eq!(calls.next_id_start, start + 2);
        start = calls.next_id_start;
    }
    assert_eq!(seen.len(), 6);
    assert!(seen.contains("call_5"));
    // A generator: distinct ids are used as given; a duplicate is a request error.
    let result = tools.recover(
        two.as_bytes(),
        &ToolRecoveryRequest::default().ids(IdPolicy::Generator(Arc::new(Numbered))),
        DialectSet::built_in(),
    );
    let ids: Vec<&str> = value(&result)
        .calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(ids, ["toolu_0", "toolu_1"]);
    let result = tools.recover(
        two.as_bytes(),
        &ToolRecoveryRequest::default().ids(IdPolicy::Generator(Arc::new(Constant))),
        DialectSet::built_in(),
    );
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(
        matches!(&result.receipt.stopped, StopReason::InvalidInput(detail) if detail.contains("`same`")),
        "{:?}",
        result.receipt.stopped
    );
    // Source ids: duplicates and missing ids are numbered and recorded.
    let tools = compile([ToolDefinition::new("get_weather")
        .effects(Effects::ReadOnly)
        .param("city", Param::string().required())]);
    let element = |id: Option<&str>| {
        let id = id.map_or_else(String::new, |id| format!(r#""id": "{id}", "#));
        format!(
            r#"{{{id}"type": "function", "function": {{"name": "get_weather", "arguments": "{{\"city\": \"Lima\"}}"}}}}"#
        )
    };
    let output = format!(
        "[{}, {}, {}, {}]",
        element(Some("c1")),
        element(Some("c1")),
        element(None),
        element(Some("c1"))
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().ids(IdPolicy::Source),
        &set(&[DialectId::OpenAiToolCalls]),
    );
    let calls = value(&result);
    let ids: Vec<&str> = calls.calls.iter().map(|call| call.id.as_str()).collect();
    assert_eq!(ids, ["c1", "c1#1", "call#2", "c1#3"]);
    assert_eq!(calls.next_id_start, 4);
    for (index, call) in calls.calls.iter().enumerate() {
        let dedup: Vec<_> = call
            .repairs
            .inferences
            .iter()
            .filter(|inference| inference.evidence.rule == "tool_call.id.dedup")
            .collect();
        if index > 0 {
            assert_eq!(dedup.len(), 1, "{call:#?}");
            assert_eq!(dedup[0].kind, InferenceKind::Derived);
            assert_eq!(dedup[0].value, call.id);
            // A derived id never demotes the call.
            assert_eq!(call.outcome, CallOutcome::Resolved);
        } else {
            assert_eq!(dedup.len(), 0);
        }
    }
    assert_eq!(
        result
            .receipt
            .inferences
            .iter()
            .filter(|inference| inference.evidence.rule == "tool_call.id.dedup")
            .count(),
        3
    );
}

/// The ids of a result's calls, in order.
fn ids(result: &AssistResult<ToolCalls>) -> Vec<String> {
    value(result)
        .calls
        .iter()
        .map(|call| call.id.clone())
        .collect()
}

/// The `tool_call.id.dedup` inferences a call records.
fn id_inferences(call: &ToolCall) -> Vec<&str> {
    call.repairs
        .inferences
        .iter()
        .filter(|inference| inference.evidence.rule == "tool_call.id.dedup")
        .map(|inference| inference.value.as_str())
        .collect()
}

#[test]
fn default_ids_are_disjoint_across_turns() {
    let tools = compile([ToolDefinition::new("list")
        .effects(Effects::ReadOnly)
        .param("path", Param::string())]);
    // Two rounds of one conversation, recovered with zero configuration and
    // no state threaded between them.
    let first = format!("{}\n{}", hermes("list", "{}"), hermes("list", "{}"));
    let second = format!(
        "{}\n{}",
        hermes("list", r#"{"path": "a"}"#),
        hermes("list", r#"{"path": "b"}"#)
    );
    let mut seen = std::collections::BTreeSet::new();
    for output in [&first, &second] {
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        let calls = value(&result);
        let tag = turn_id_prefix(&calls.turn);
        assert_eq!(calls.turn, result.receipt.source);
        assert_eq!(tag.len(), "call_".len() + 12 + 1);
        assert_eq!(ids(&result), [format!("{tag}0"), format!("{tag}1")]);
        assert_eq!(calls.next_id_start, 2);
        for call in &calls.calls {
            assert!(seen.insert(call.id.clone()), "{} repeats", call.id);
            // Hermes markup carries no id: numbering it is the policy, not
            // a repair.
            assert_eq!(id_inferences(call), Vec::<&str>::new(), "{call:#?}");
            assert_eq!(call.source_id, None);
        }
    }
    assert_eq!(seen.len(), 4);
}

#[test]
fn default_ids_are_deterministic_per_turn() {
    let tools = compile([ToolDefinition::new("list").effects(Effects::ReadOnly)]);
    let output = format!("Listing.\n{}", hermes("list", "{}"));
    let once = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let twice = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    assert_eq!(ids(&once), ids(&twice));
    assert_eq!(once.to_node().to_json(), twice.to_node().to_json());
    // The tag is the output digest's leading 12 hex digits.
    let digest = Digest::of(output.as_bytes()).to_string();
    assert_eq!(
        ids(&once),
        [format!(
            "call_{}_0",
            &digest["sha256:".len().."sha256:".len() + 12]
        )]
    );
}

#[test]
fn explicit_prefix_is_unchanged() {
    let tools = compile([ToolDefinition::new("get_weather")
        .effects(Effects::ReadOnly)
        .param("city", Param::string().required())]);
    let hermes_two = format!(
        "{}\n{}",
        hermes("get_weather", r#"{"city": "Lima"}"#),
        hermes("get_weather", r#"{"city": "Oslo"}"#)
    );
    let prefix = IdPolicy::Prefix {
        prefix: "call_".into(),
        start: 0,
    };
    let result = tools.recover(
        hermes_two.as_bytes(),
        &ToolRecoveryRequest::default().ids(prefix),
        DialectSet::built_in(),
    );
    assert_eq!(ids(&result), ["call_0", "call_1"]);
    assert_eq!(value(&result).next_id_start, 2);
    // `Prefix` numbers every call, whatever id the model wrote, and records
    // no inference.
    let openai = r#"[{"id": "m1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"Lima\"}"}}]"#;
    let result = tools.recover(
        openai.as_bytes(),
        &ToolRecoveryRequest::default().ids(IdPolicy::Prefix {
            prefix: "t".into(),
            start: 7,
        }),
        &set(&[DialectId::OpenAiToolCalls]),
    );
    let calls = value(&result);
    assert_eq!(ids(&result), ["t7"]);
    assert_eq!(calls.calls[0].source_id.as_deref(), Some("m1"));
    assert_eq!(id_inferences(&calls.calls[0]), Vec::<&str>::new());
    assert_eq!(calls.next_id_start, 8);
}

#[test]
fn default_ids_preserve_model_ids_and_repair_duplicates() {
    let dialects = set(&[DialectId::OpenAiToolCalls]);
    let tools = compile([ToolDefinition::new("get_weather")
        .effects(Effects::ReadOnly)
        .param("city", Param::string().required())]);
    let element = |id: Option<&str>, city: &str| {
        let id = id.map_or_else(String::new, |id| format!(r#""id": "{id}", "#));
        format!(
            r#"{{{id}"type": "function", "function": {{"name": "get_weather", "arguments": "{{\"city\": \"{city}\"}}"}}}}"#
        )
    };
    // Distinct model ids are kept as written and reported.
    let output = format!(
        "[{}, {}]",
        element(Some("call_lima"), "Lima"),
        element(Some("call_oslo"), "Oslo")
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        &dialects,
    );
    let calls = value(&result);
    assert_eq!(ids(&result), ["call_lima", "call_oslo"]);
    for call in &calls.calls {
        assert_eq!(call.source_id.as_deref(), Some(call.id.as_str()));
        assert_eq!(id_inferences(call), Vec::<&str>::new());
        assert_eq!(call.outcome, CallOutcome::Resolved);
    }
    assert!(
        result
            .to_node()
            .to_json()
            .contains(r#""source_id":"call_lima""#)
    );
    // A repeated model id and a missing one are numbered from the turn's
    // tag, each recorded as a derived inference that does not demote.
    let output = format!(
        "[{}, {}, {}]",
        element(Some("c1"), "Lima"),
        element(Some("c1"), "Oslo"),
        element(None, "Rome")
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        &dialects,
    );
    let calls = value(&result);
    let tag = turn_id_prefix(&calls.turn);
    assert_eq!(
        ids(&result),
        ["c1".to_owned(), format!("{tag}1"), format!("{tag}2")]
    );
    assert_eq!(calls.next_id_start, 3);
    assert_eq!(id_inferences(&calls.calls[0]), Vec::<&str>::new());
    for call in &calls.calls[1..] {
        assert_eq!(id_inferences(call), [call.id.as_str()], "{call:#?}");
        assert_eq!(call.outcome, CallOutcome::Resolved);
        let inference = call
            .repairs
            .inferences
            .iter()
            .find(|inference| inference.evidence.rule == "tool_call.id.dedup")
            .expect("id inference");
        assert_eq!(inference.kind, InferenceKind::Derived);
    }
    let explanations: Vec<&str> = calls.calls[1..]
        .iter()
        .flat_map(|call| &call.repairs.evidence)
        .filter(|evidence| evidence.rule == "tool_call.id.dedup")
        .map(|evidence| evidence.explanation.as_str())
        .collect();
    assert!(
        explanations[0].contains("source call id `c1` repeats"),
        "{explanations:?}"
    );
    assert!(
        explanations[1].contains("carries no id"),
        "{explanations:?}"
    );
    assert_eq!(
        result
            .receipt
            .inferences
            .iter()
            .filter(|inference| inference.evidence.rule == "tool_call.id.dedup")
            .count(),
        2
    );
    // A model id equal to the id the turn would assign another call is
    // kept; the assigned id moves aside to the least free `_{k}` suffix, so
    // ids stay distinct. (A model cannot write its own turn's digest, so
    // the carried id is set on recovered calls and numbered by emission,
    // which shares recovery's assignment.)
    let output = format!("[{}, {}]", element(None, "Lima"), element(None, "Oslo"));
    assert_emitted_id_collision(&tools, output.as_bytes(), &dialects);
}

fn assert_emitted_id_collision(tools: &ToolSet, output: &[u8], dialects: &DialectSet) {
    let result = tools.recover(output, &ToolRecoveryRequest::default(), dialects);
    let mut calls = value(&result).clone();
    let tag = turn_id_prefix(&calls.turn);
    calls.calls[1].source_id = Some(format!("{tag}0"));
    let array = emit::openai_tool_calls(&calls, &IdPolicy::Turn).expect("array");
    let emitted: Vec<String> = match &array.output.value {
        Value::Array(items) => items
            .iter()
            .map(|item| match item.get("id").map(|id| &id.value) {
                Some(Value::String(id)) => id.clone(),
                other => panic!("no id: {other:?}"),
            })
            .collect(),
        other => panic!("not an array: {other:?}"),
    };
    assert_eq!(emitted, [format!("{tag}0_1"), format!("{tag}0")]);
}

fn adapter_read_file() -> ToolSet {
    ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"read_file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]"#,
        Budget::default(),
    )
    .expect("adapter tools")
}

#[test]
fn effects_override_per_tool() {
    let tools = adapter_read_file();
    let output = hermes("read_fiel", r#"{"path": "a.rs"}"#);
    // An adapter tool is mutating, hence structural: a near-miss is refused.
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let call = &value(&result).calls[0];
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::RuleNotAdmitted {
                rule: "tool_call.name.match".into(),
                policy: RepairPolicy::Structural
            }
        }
    );
    let exact = tools.recover(
        hermes("read_file", r#"{"path": "a.rs"}"#).as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let call = &value(&exact).calls[0];
    assert_eq!(
        (call.effects, call.policy, &call.outcome),
        (
            Some(Effects::Mutating),
            Some(RepairPolicy::Structural),
            &CallOutcome::Resolved
        )
    );
    // Overridden to read-only, it is tolerant.
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().effects("read_file", Effects::ReadOnly),
        DialectSet::built_in(),
    );
    let call = &value(&result).calls[0];
    assert_eq!(
        (call.effects, call.policy),
        (Some(Effects::ReadOnly), Some(RepairPolicy::Tolerant))
    );
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert_eq!(value(&result).usable_accepting_guesses().len(), 1);
    // An override naming an undeclared tool is a request error.
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().effects("read", Effects::ReadOnly),
        DialectSet::built_in(),
    );
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(
        matches!(&result.receipt.stopped, StopReason::InvalidInput(detail) if detail.contains("`read`"))
    );
    assert_eq!(result.receipt.sources.len(), 1);
}

#[test]
fn tool_choice_from_openai_and_anthropic_json() {
    let openai = |text: &str| ToolChoice::from_openai(&json::parse(text).expect("json"));
    let anthropic = |text: &str| ToolChoice::from_anthropic(&json::parse(text).expect("json"));
    assert_eq!(openai(r#""auto""#), Ok(ToolChoice::Auto));
    assert_eq!(openai(r#""none""#), Ok(ToolChoice::None));
    assert_eq!(openai(r#""required""#), Ok(ToolChoice::Required));
    assert_eq!(
        openai(r#"{"type":"function","function":{"name":"read"}}"#),
        Ok(ToolChoice::Forced("read".into()))
    );
    for malformed in [
        r#""any""#,
        "1",
        "null",
        "[]",
        r#"{"type":"function"}"#,
        r#"{"type":"tool","function":{"name":"read"}}"#,
        r#"{"type":"function","function":{"name":1}}"#,
        r#"{"type":"function","function":{"name":"read","strict":true}}"#,
        r#"{"type":"function","function":{"name":"read"},"extra":1}"#,
        r#"{"type":"function","type":"function","function":{"name":"read"}}"#,
    ] {
        assert!(openai(malformed).is_err(), "{malformed}");
    }
    assert_eq!(anthropic(r#"{"type":"auto"}"#), Ok(ToolChoice::Auto));
    assert_eq!(anthropic(r#"{"type":"any"}"#), Ok(ToolChoice::Required));
    assert_eq!(anthropic(r#"{"type":"none"}"#), Ok(ToolChoice::None));
    assert_eq!(
        anthropic(r#"{"type":"tool","name":"read"}"#),
        Ok(ToolChoice::Forced("read".into()))
    );
    assert_eq!(
        anthropic(r#"{"type":"tool","name":"read","disable_parallel_tool_use":true}"#),
        Ok(ToolChoice::ForcedStrict(StrictForced {
            name: "read".into(),
            exactly_one: true,
            allow_content: true,
            allow_reasoning: true,
        }))
    );
    assert_eq!(
        anthropic(r#"{"type":"tool","name":"read","disable_parallel_tool_use":false}"#),
        Ok(ToolChoice::Forced("read".into()))
    );
    for malformed in [
        r#"{"type":"tool","name":"read","disable_parallel_tool_use":"yes"}"#,
        r#"{"type":"tool","name":"read","disable_parallel_tool_use":null}"#,
        r#"{"type":"tool","name":"read","disable_parallel_tool_use":true,"disable_parallel_tool_use":true}"#,
        r#"{"type":"auto","disable_parallel_tool_use":false}"#,
        r#"{"type":"none","disable_parallel_tool_use":true}"#,
        r#""auto""#,
        r#"{"type":"tool"}"#,
        r#"{"type":"auto","name":"read"}"#,
        r#"{"type":"required"}"#,
        r#"{"name":"read"}"#,
        r#"{"type":"any","disable_parallel_tool_use":true}"#,
        r#"{"type":"tool","name":"a","name":"b"}"#,
    ] {
        let error = anthropic(malformed).expect_err(malformed);
        assert_ne!(error.reason.len(), 0);
    }
}

#[test]
fn strict_forced_constraint_strings() {
    let constraint = |strict: StrictForced| ToolChoice::ForcedStrict(strict).constraint();
    let base = StrictForced::exactly_one("read");
    assert_eq!(
        constraint(base.clone()),
        "tool_choice=forced[exactly-one,no-content]:read"
    );
    assert_eq!(
        constraint(base.clone().allow_reasoning(false)),
        "tool_choice=forced[exactly-one,no-content,no-reasoning]:read"
    );
    assert_eq!(
        constraint(base.clone().allow_content(true)),
        "tool_choice=forced[exactly-one]:read"
    );
    assert_eq!(
        constraint(base.clone().allow_content(true).allow_reasoning(false)),
        "tool_choice=forced[exactly-one,no-reasoning]:read"
    );
    assert_eq!(
        constraint(base.clone().with_exactly_one(false)),
        "tool_choice=forced[no-content]:read"
    );
    assert_eq!(
        constraint(base.clone().with_exactly_one(false).allow_reasoning(false)),
        "tool_choice=forced[no-content,no-reasoning]:read"
    );
    // The degenerate form is the lenient one, exactly.
    let lenient = base.with_exactly_one(false).allow_content(true);
    assert_eq!(constraint(lenient), "tool_choice=forced:read");
    assert_eq!(
        ToolChoice::Forced("read".into()).constraint(),
        "tool_choice=forced:read"
    );
    // The name comes last, so punctuation in it stays unambiguous.
    assert_eq!(
        constraint(StrictForced::exactly_one("a:b;c,d]")),
        "tool_choice=forced[exactly-one,no-content]:a:b;c,d]"
    );
}

#[test]
fn strict_forced_cardinality_content_and_reasoning() {
    let built_in = DialectSet::built_in();
    let tools = compile([
        ToolDefinition::new("read")
            .effects(Effects::ReadOnly)
            .alias("cat")
            .param("path", Param::string()),
        ToolDefinition::new("list").effects(Effects::ReadOnly),
    ]);
    let request = |strict: StrictForced| {
        ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(strict))
    };
    let exactly_one = || request(StrictForced::exactly_one("read"));
    let one = hermes("read", r#"{"path": "a"}"#);
    let two = format!("{one}\n{}", hermes("cat", r#"{"path": "b"}"#));
    // Exactly one: the second call (an alias of the forced tool) is the
    // cardinality violation, the first stays Resolved.
    let result = tools.recover(two.as_bytes(), &exactly_one(), built_in);
    let calls = value(&result);
    assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
    assert_eq!(
        calls.calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceCardinality
        }
    );
    assert_eq!(
        UnresolvedReason::ToolChoiceCardinality.code(),
        "tool_choice_cardinality"
    );
    assert!(!calls.tool_choice.passed);
    assert_eq!(calls.finish, Finish::Unresolved);
    assert_eq!(calls.verdict().code(), "nothing_usable");
    assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
    let wire = result.to_node();
    assert_eq!(
        wire.pointer("/value/calls/1/outcome/reason/code")
            .and_then(Node::as_str),
        Some("tool_choice_cardinality")
    );
    // Without exactly_one, two calls to the forced tool pass.
    let lenient = StrictForced::exactly_one("read")
        .with_exactly_one(false)
        .allow_content(true);
    let result = tools.recover(two.as_bytes(), &request(lenient), built_in);
    assert!(value(&result).tool_choice.passed);
    // A call to another tool is the Forced violation, not a cardinality one.
    let mixed = format!("{one}\n{}", hermes("list", "{}"));
    let result = tools.recover(mixed.as_bytes(), &exactly_one(), built_in);
    assert_eq!(
        value(&result).calls[1].outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceForced {
                expected: "read".into()
            }
        }
    );
    // No call at all fails, as under Forced.
    let result = tools.recover(b"nothing", &exactly_one(), built_in);
    assert!(!value(&result).tool_choice.passed);
    // An undeclared name is a request error for the strict form too.
    let result = tools.recover(
        one.as_bytes(),
        &request(StrictForced::exactly_one("write")),
        built_in,
    );
    assert!(
        matches!(result.receipt.stopped, StopReason::InvalidInput(ref detail) if detail.contains("`write`"))
    );
    assert_eq!(result.outcome, Outcome::Unresolved);
    // Content: ASCII whitespace frames, anything else (U+00A0 included) fails.
    let framed = format!(" \t\r\n\x0c{one}\n");
    let result = tools.recover(framed.as_bytes(), &exactly_one(), built_in);
    assert!(value(&result).tool_choice.passed);
    for prose in ["Sure.\n", "\u{a0}", "\u{2003}"] {
        let output = format!("{prose}{one}");
        let result = tools.recover(output.as_bytes(), &exactly_one(), built_in);
        assert!(!value(&result).tool_choice.passed, "{prose:?}");
        let allowed = StrictForced::exactly_one("read").allow_content(true);
        let result = tools.recover(output.as_bytes(), &request(allowed), built_in);
        assert!(value(&result).tool_choice.passed, "{prose:?}");
    }
    // Reasoning: an embedded call is not counted; the block is what fails.
    let thought = format!("<think>maybe {}</think>\n{one}", hermes("read", "{}"));
    let result = tools.recover(thought.as_bytes(), &exactly_one(), built_in);
    assert!(value(&result).tool_choice.passed);
    assert_eq!(value(&result).embedded.len(), 1);
    let result = tools.recover(
        thought.as_bytes(),
        &request(StrictForced::exactly_one("read").allow_reasoning(false)),
        built_in,
    );
    assert!(!value(&result).tool_choice.passed);
    assert!(
        value(&result).tool_choice.diagnostics[0].contains("reasoning"),
        "{:?}",
        value(&result).tool_choice.diagnostics
    );
}

/// A second call attempt that is not a call (R4, G3): a rejected attempt of
/// any reason is a call beyond the one call (`exactly_one`) and bytes
/// outside it (`no-content`), so the strict choice fails and nothing of the
/// turn runs, although the first call is valid on its own.
#[test]
fn strict_forced_counts_rejected_attempts_of_any_reason() {
    let tools = compile([ToolDefinition::new("read_file")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    let first = hermes("read_file", r#"{"path": "a"}"#);
    let junk = "<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"b\"}} junk junk {{{\n</tool_call>";
    let truncated = "<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": ";
    let unsupported = "[read_file(path=\"b\")]";
    let hermes_only = set(&[DialectId::Hermes]);
    for second in [junk, truncated] {
        let output = format!("{first}\n{second}");
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(
                StrictForced::exactly_one("read_file").allow_reasoning(true),
            )),
            &hermes_only,
        );
        let calls = value(&result);
        assert_eq!(calls.rejected.len(), 1, "{second:?}");
        assert!(!calls.tool_choice.passed, "{second:?}");
        // Both options name it: the attempt count and the attempt's bytes.
        assert!(
            calls
                .tool_choice
                .diagnostics
                .iter()
                .any(|d| d.starts_with("1 rejected call attempt beyond")),
            "{:?}",
            calls.tool_choice.diagnostics
        );
        assert!(
            calls
                .tool_choice
                .diagnostics
                .iter()
                .any(|d| d.starts_with("rejected call attempt at bytes")),
            "{:?}",
            calls.tool_choice.diagnostics
        );
        assert_eq!(calls.calls[0].outcome, CallOutcome::Resolved);
        assert_eq!(calls.verdict().code(), "nothing_usable");
        assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
        // Each option fails on its own.
        let only_cardinality = StrictForced::exactly_one("read_file").allow_content(true);
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(only_cardinality)),
            &hermes_only,
        );
        assert!(
            !value(&result).tool_choice.passed,
            "exactly-one alone: {second:?}"
        );
        let only_content = StrictForced::exactly_one("read_file").with_exactly_one(false);
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default()
                .tool_choice(ToolChoice::ForcedStrict(only_content.clone())),
            &hermes_only,
        );
        assert!(
            !value(&result).tool_choice.passed,
            "no-content alone: {second:?}"
        );
        // With both lenient, the attempt is not a strict matter.
        let lenient = only_content.allow_content(true);
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(lenient)),
            &hermes_only,
        );
        assert!(value(&result).tool_choice.passed, "lenient: {second:?}");
    }
    // An unsupported dialect's attempt: the Anthropic form (exactly one,
    // content allowed) fails on it, where it used to pass because the bytes
    // were only content.
    let output = format!("{first}\n{unsupported}");
    let anthropic = ToolChoice::from_anthropic(
        &json::parse(r#"{"type":"tool","name":"read_file","disable_parallel_tool_use":true}"#)
            .unwrap(),
    )
    .unwrap();
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().tool_choice(anthropic),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    assert!(matches!(
        calls.rejected[0].reason,
        Rejection::UnsupportedDialect { .. }
    ));
    assert!(!calls.tool_choice.passed, "{:?}", calls.tool_choice);
    assert_eq!(calls.verdict().code(), "nothing_usable");
    assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
    // The same turn without the attempt passes.
    let result = tools.recover(
        format!("Sure.\n{first}").as_bytes(),
        &ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(
            StrictForced::exactly_one("read_file").allow_content(true),
        )),
        DialectSet::built_in(),
    );
    assert!(value(&result).tool_choice.passed);
}

/// The strict result does not depend on the order of the readings (R4,
/// F5): two dialects contest the second `<tool_call>` body (Qwen3-Coder XML
/// rejects the opener alone and leaves the rest as content, Hermes rejects
/// the whole body), and the check fails, with identical diagnostics, whether
/// Hermes or Qwen is listed first. (The two cannot disagree on whether the
/// body holds an attempt: every dialect that contests a body rejects it, so
/// the merge-level fail-closed rule, which handles a member that read it as
/// content, is covered by the unit test `merge_fails_closed_in_either_order`
/// in `recover.rs`.)
#[test]
fn strict_forced_contested_second_body_is_order_independent() {
    let tools = compile([ToolDefinition::new("read_file")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    let output = format!(
        "{}\n<tool_call>\n{{\"name\": \"read_file\", \"arguments\": {{\"path\": \"b\"}}}} junk junk {{{{{{\n</tool_call>",
        hermes("read_file", r#"{"path": "a"}"#)
    );
    for strict in [
        StrictForced::exactly_one("read_file").allow_content(true),
        StrictForced::exactly_one("read_file"),
    ] {
        let mut seen = Vec::new();
        for dialects in [
            [DialectId::Hermes, DialectId::Qwen3CoderXml],
            [DialectId::Qwen3CoderXml, DialectId::Hermes],
        ] {
            let result = tools.recover(
                output.as_bytes(),
                &ToolRecoveryRequest::default()
                    .tool_choice(ToolChoice::ForcedStrict(strict.clone())),
                &set(&dialects),
            );
            let calls = value(&result);
            assert_eq!(calls.rejected.len(), 1, "{calls:#?}");
            assert_eq!(calls.rejected[0].candidate_dialects.len(), 2);
            assert!(!calls.tool_choice.passed, "{strict:?} {dialects:?}");
            assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
            seen.push(calls.tool_choice.diagnostics.clone());
        }
        assert_eq!(seen[0], seen[1], "{strict:?}");
    }
}

/// The diagnostics of a strict choice are capped (R4, G17): one cardinality
/// diagnostic stating the count of extra calls and attempts, one for the
/// content, one for the attempts' bytes and one for the reasoning, however
/// many calls, attempts, content ranges and blocks there are.
#[test]
fn strict_forced_diagnostics_are_capped() {
    let tools = compile([ToolDefinition::new("read_file")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    let call = hermes("read_file", r#"{"path": "a"}"#);
    let attempt = "<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": ";
    let mut output = String::new();
    for _ in 0..40 {
        writeln!(output, "<think>t</think>prose {call}").unwrap();
    }
    for _ in 0..7 {
        write!(output, "{attempt}\n</tool_call>\n").unwrap();
    }
    let strict = StrictForced::exactly_one("read_file").allow_reasoning(false);
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(strict)),
        &set(&[DialectId::Hermes]),
    );
    let calls = value(&result);
    assert_eq!(calls.calls.len(), 40);
    assert!(calls.rejected.len() >= 7, "{}", calls.rejected.len());
    let diagnostics = &calls.tool_choice.diagnostics;
    assert!(!calls.tool_choice.passed);
    assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
    assert!(
        diagnostics[0].starts_with(&format!(
            "39 extra calls and {} rejected call attempts beyond",
            calls.rejected.len()
        )),
        "{diagnostics:#?}"
    );
    assert!(diagnostics[1].starts_with("content at byte") && diagnostics[1].contains("(and "));
    assert!(
        diagnostics[2].starts_with("rejected call attempt at bytes")
            && diagnostics[2].contains("(and ")
    );
    assert!(diagnostics[3].starts_with("reasoning at bytes"));
    // Not one per entry: the wire form carries no more.
    assert!(diagnostics.iter().all(|d| d.len() < 200));
    // Every call after the first is still marked individually.
    assert!(calls.calls[1..].iter().all(|call| matches!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ToolChoiceCardinality
        }
    )));
}

/// A strict choice that forbids reasoning cannot be enforced when reasoning
/// markers are content (R4, G15): the request is an `invalid_input` error,
/// not a check that passes vacuously.
#[test]
fn strict_no_reasoning_under_reasoning_as_content_is_invalid_input() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = format!("<think>plan</think>\n{}", hermes("ls", "{}"));
    let strict = StrictForced::exactly_one("ls").allow_reasoning(false);
    let request = ToolRecoveryRequest::default()
        .reasoning(ReasoningPolicy::AsContent)
        .tool_choice(ToolChoice::ForcedStrict(strict.clone()));
    let result = tools.recover(output.as_bytes(), &request, DialectSet::built_in());
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert!(
        matches!(result.receipt.stopped, StopReason::InvalidInput(ref detail) if detail.contains("allow_reasoning")),
        "{:?}",
        result.receipt.stopped
    );
    // Reasoning allowed under AsContent, and no-reasoning under Separate,
    // are fine.
    let allowed = ToolRecoveryRequest::default()
        .reasoning(ReasoningPolicy::AsContent)
        .tool_choice(ToolChoice::ForcedStrict(
            strict.clone().allow_reasoning(true).allow_content(true),
        ));
    assert!(matches!(
        tools
            .recover(output.as_bytes(), &allowed, DialectSet::built_in())
            .outcome,
        Outcome::Resolved(_)
    ));
    let separate = ToolRecoveryRequest::default().tool_choice(ToolChoice::ForcedStrict(strict));
    assert!(
        !value(&tools.recover(output.as_bytes(), &separate, DialectSet::built_in()))
            .tool_choice
            .passed
    );
}

/// A failed tool choice leaves no usable call (R4, A3): both counts are zero
/// and agree with the empty emission, whatever the choice.
#[test]
fn failed_tool_choice_has_no_usable_calls() {
    let tools = compile([
        ToolDefinition::new("ls").effects(Effects::ReadOnly),
        ToolDefinition::new("cat").effects(Effects::ReadOnly),
    ]);
    let output = format!("{}\n{}", hermes("ls", "{}"), hermes("ls", "{}"));
    for choice in [
        ToolChoice::Auto,
        ToolChoice::Forced("cat".into()),
        ToolChoice::ForcedStrict(StrictForced::exactly_one("ls")),
        ToolChoice::None,
    ] {
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default().tool_choice(choice.clone()),
            DialectSet::built_in(),
        );
        let calls = value(&result);
        let wire = result.to_node();
        let count = |key: &str| {
            wire.pointer(&format!("/value/{key}"))
                .and_then(|node| match &node.value {
                    Value::Number(number) => Some(number.as_str().to_owned()),
                    _ => None,
                })
        };
        if calls.tool_choice.passed {
            assert_eq!(calls.usable().len(), 2, "{choice:?}");
            assert_eq!(count("usable_calls").as_deref(), Some("2"));
        } else {
            assert!(
                calls.usable().is_empty() && calls.usable_accepting_guesses().is_empty(),
                "{choice:?}"
            );
            assert_eq!(count("usable_calls").as_deref(), Some("0"), "{choice:?}");
            assert_eq!(
                count("usable_calls_accepting_guesses").as_deref(),
                Some("0"),
                "{choice:?}"
            );
            assert!(calls.emitted_calls().is_empty(), "{calls:#?}");
        }
    }
}

/// Fuzz regression (`scripts/fuzz.py --target tool-call`): an attempt that
/// reads several calls (Mistral v11 `[TOOL_CALLS]` elements) can begin
/// inside a reasoning block and end after its closer. The calls it read
/// inside the block were dropped: they completed only when the attempt
/// ended, after the block's segment was stored. Each is now offered as an
/// embedded call of the block it ends inside, as when the attempt ends in
/// the block, and the call after the closer is still a call. A call two
/// attempts read (a call of the attempt reading several, and the attempt
/// its own opener began) is offered once, and the embedded calls are in
/// span order (F5): the exact list is asserted, tool names included. The
/// block's closing `[/THINK]` belongs to the block (T2): a call that ran
/// into it is cut at it (`tool_call.reasoning.marker-cut`), so `s`, whose
/// only arguments were the marker's bytes, ends at 26 and is unresolved.
#[test]
fn calls_an_attempt_reads_inside_a_closed_block_stay_embedded() {
    let tools = compile([
        ToolDefinition::new("t").effects(Effects::ReadOnly),
        ToolDefinition::new("s").effects(Effects::ReadOnly),
        ToolDefinition::new("g").effects(Effects::ReadOnly),
    ]);
    for (output, reasoning, embedded, after) in [
        (
            "[TOOL_CALLS]t[TOOL_CALLS]s[/THINK]",
            0..34,
            vec![(0..13, "t"), (13..26, "s")],
            None,
        ),
        (
            "[TOOL_CALLS]t[TOOL_CALLS]s[/THINK][TOOL_CALLS]g",
            0..34,
            vec![(0..13, "t"), (13..26, "s")],
            Some(34..47),
        ),
        (
            "[TOOL_CALLS]t{\"x\":1}[TOOL_CALLS]s{\"y\":2}[/THINK]Answer.",
            0..48,
            vec![(0..20, "t"), (20..40, "s")],
            None,
        ),
    ] {
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default()
                .reasoning(ReasoningPolicy::Separate { starts_open: true }),
            &set(&[DialectId::MistralV11]),
        );
        let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = &result.outcome else {
            panic!("{output:?}: {:#?}", result.outcome)
        };
        assert_eq!(calls.reasoning, vec![reasoning], "{output:?}");
        let found: Vec<(core::ops::Range<usize>, &str)> = calls
            .embedded
            .iter()
            .map(|call| (call.span.clone(), call.tool.as_deref().unwrap_or("")))
            .collect();
        assert_eq!(found, embedded, "{output:?}");
        // No embedded call reaches into the block's closing marker; `s`
        // was cut at it exactly when it had read the marker's bytes (it
        // then has no arguments of its own and is unresolved).
        let marker = output.find("[/THINK]").expect("a closing marker");
        let cut = !output[..marker].ends_with('}');
        for call in &calls.embedded {
            assert!(call.span.end <= marker, "{output:?}: {call:?}");
            let was_cut = rules(call).contains(&"tool_call.reasoning.marker-cut");
            assert_eq!(
                was_cut,
                cut && call.tool.as_deref() == Some("s"),
                "{output:?}: {call:?}"
            );
            if was_cut {
                assert!(
                    matches!(call.outcome, CallOutcome::Unresolved { .. }),
                    "{output:?}: {call:?}"
                );
            }
        }
        let spans: Vec<_> = calls.calls.iter().map(|call| call.span.clone()).collect();
        assert_eq!(spans, after.into_iter().collect::<Vec<_>>(), "{output:?}");
    }
}

#[test]
fn starts_open_reasoning_separated() {
    let tools = compile([ToolDefinition::new("f").effects(Effects::ReadOnly)]);
    let reasoning = "I should call f.</think>";
    let content = "Calling it.";
    let call = "<tool_call>\n<function=f>\n</function>\n</tool_call>";
    let output = format!("{reasoning}{content}{call}");
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().reasoning(ReasoningPolicy::Separate { starts_open: true }),
        &set(&[DialectId::Qwen3CoderXml]),
    );
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("{:#?}", result.outcome)
    };
    assert_eq!(calls.reasoning.len(), 1);
    assert_eq!(calls.reasoning[0], 0..reasoning.len());
    assert_eq!(calls.content.len(), 1);
    assert_eq!(
        calls.content[0],
        reasoning.len()..reasoning.len() + content.len()
    );
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(
        calls.calls[0].span,
        reasoning.len() + content.len()..output.len()
    );
    assert!(
        !result
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule.contains("stray")),
        "{:#?}",
        result.receipt.evidence
    );
    assert_eq!(calls.finish, Finish::Calls);
}

/// Every reasoning block of `calls` as `(span, text)`, with the text bytes.
fn blocks<'a>(
    output: &'a [u8],
    calls: &ToolCalls,
) -> Vec<(core::ops::Range<usize>, core::ops::Range<usize>, &'a [u8])> {
    calls
        .reasoning_blocks
        .iter()
        .map(|block| {
            (
                block.span.clone(),
                block.text.clone(),
                &output[block.text.clone()],
            )
        })
        .collect()
}

#[test]
fn reasoning_blocks_two_adjacent_blocks_are_never_merged() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = format!("<think>a</think><think>b</think>\n{}", hermes("ls", "{}"));
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    // The outer ranges merge, the blocks do not.
    assert_eq!(calls.reasoning, vec![0..32]);
    assert_eq!(
        blocks(output.as_bytes(), calls),
        vec![(0..16, 7..8, &b"a"[..]), (16..32, 23..24, &b"b"[..])]
    );
    assert_eq!(calls.calls.len(), 1);
    let wire = result.to_node();
    assert_eq!(
        wire.pointer("/value/reasoning_blocks")
            .expect("reasoning_blocks on the wire")
            .to_json(),
        r#"[{"span":{"start":0,"end":16},"text":{"start":7,"end":8}},{"span":{"start":16,"end":32},"text":{"start":23,"end":24}}]"#
    );
}

#[test]
fn reasoning_blocks_closed_block_before_a_call() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = format!("<think>abc</think>\n{}", hermes("ls", "{}"));
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    assert_eq!(calls.reasoning, vec![0..18]);
    assert_eq!(
        blocks(output.as_bytes(), calls),
        vec![(0..18, 7..10, &b"abc"[..])]
    );
    assert_eq!(calls.calls.len(), 1);
}

#[test]
fn reasoning_blocks_starts_open_text_begins_at_zero() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = format!("abc</think>\n{}", hermes("ls", "{}"));
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().reasoning(ReasoningPolicy::Separate { starts_open: true }),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    // No opener is stripped: the text starts with the block, and ends at the
    // closer.
    assert_eq!(calls.reasoning, vec![0..11]);
    assert_eq!(
        blocks(output.as_bytes(), calls),
        vec![(0..11, 0..3, &b"abc"[..])]
    );
    assert_eq!(calls.calls.len(), 1);
}

#[test]
fn reasoning_blocks_closed_at_a_call_opener() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = format!("<think>abc{}", hermes("ls", "{}"));
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    // No closer: the block ends at the opener, so the text ends with the
    // span.
    assert_eq!(calls.reasoning, vec![0..10]);
    assert_eq!(
        blocks(output.as_bytes(), calls),
        vec![(0..10, 7..10, &b"abc"[..])]
    );
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "tool_call.reasoning.unclosed"),
        "{:#?}",
        result.receipt.evidence
    );
}

#[test]
fn reasoning_blocks_unterminated_runs_to_the_end_of_output() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let output = "<think>abc";
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    assert_eq!(calls.reasoning, vec![0..10]);
    assert_eq!(
        blocks(output.as_bytes(), calls),
        vec![(0..10, 7..10, &b"abc"[..])]
    );
    assert!(
        result
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "tool_call.reasoning.unterminated"),
        "{:#?}",
        result.receipt.evidence
    );
}

#[test]
fn reasoning_blocks_harmony_analysis_channel() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    // `<|channel|>analysis<|message|>` is 30 bytes, `<|end|>` 7.
    let output = "<|channel|>analysis<|message|>think hard<|end|>done";
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        &set(&[DialectId::Harmony]),
    );
    let calls = value(&result);
    assert_eq!(calls.reasoning, vec![0..47]);
    assert_eq!(
        blocks(output.as_bytes(), calls),
        vec![(0..47, 30..40, &b"think hard"[..])]
    );
    assert_eq!(calls.calls.len(), 0);
}

#[test]
fn reasoning_blocks_other_pairs_and_policies() {
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    // Mistral's `[THINK]` pair.
    let output = "[THINK]x[/THINK]";
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        &set(&[DialectId::MistralV11]),
    );
    assert_eq!(
        blocks(output.as_bytes(), value(&result)),
        vec![(0..16, 7..8, &b"x"[..])]
    );
    // A second opener inside a block is text; only the stripped edges are
    // markers.
    let output = "<think><think>x</think>";
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    assert_eq!(
        blocks(output.as_bytes(), value(&result)),
        vec![(0..23, 7..15, &b"<think>x"[..])]
    );
    // Reasoning as content has no blocks, and neither does an empty output
    // that starts open.
    let result = tools.recover(
        b"<think>a</think>",
        &ToolRecoveryRequest::default().reasoning(ReasoningPolicy::AsContent),
        DialectSet::built_in(),
    );
    assert_eq!(value(&result).reasoning_blocks.len(), 0);
    let result = tools.recover(
        b"",
        &ToolRecoveryRequest::default().reasoning(ReasoningPolicy::Separate { starts_open: true }),
        DialectSet::built_in(),
    );
    assert_eq!(value(&result).reasoning_blocks.len(), 0);
}

fn mode_tool() -> ToolSet {
    ToolSet::from_openai(
        r##"[{"type":"function","function":{"name":"set_mode","parameters":{"type":"object","properties":{"mode":{"anyOf":[{"type":"string","enum":["fast","safe"]},{"$ref":"#/$defs/level"}]}},"required":["mode"],"$defs":{"level":{"type":"integer","minimum":1}}}}}]"##,
        Budget::default(),
    )
    .expect("adapter tool")
}

#[test]
fn anyof_ref_enforced_on_recovered_call() {
    let tools = mode_tool();
    for (arguments, valid) in [
        (r#"{"mode": "fast"}"#, true),
        (r#"{"mode": 3}"#, true),
        (r#"{"mode": "slow"}"#, false),
        (r#"{"mode": 0}"#, false),
    ] {
        let result = tools.recover(
            hermes("set_mode", arguments).as_bytes(),
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        let call = &value(&result).calls[0];
        let schema = result
            .receipt
            .validations
            .iter()
            .find(|validation| validation.validator.starts_with("purrassist.schema"))
            .expect("schema validation");
        assert_eq!(schema.passed, valid, "{arguments}: {schema:#?}");
        if valid {
            assert_eq!(call.outcome, CallOutcome::Resolved, "{arguments}");
        } else {
            assert_eq!(
                call.outcome,
                CallOutcome::Unresolved {
                    reason: UnresolvedReason::SchemaValidation
                },
                "{arguments}"
            );
            assert_ne!(schema.diagnostics.len(), 0);
        }
        // The keyword is reported; JSON arguments arrive typed, so no
        // repair consulted it: it is enforced by validation alone.
        assert!(
            call.keywords
                .iter()
                .any(|keyword| keyword.keyword == "anyOf" && !keyword.used),
            "{:#?}",
            call.keywords
        );
    }
    // In Qwen3-Coder XML the value is text: the exact schema types it
    // (`tool_call.value.schema-typed`), and the keyword drove that typing.
    for (text, expected, used) in [("3", "3", true), ("fast", "\"fast\"", false)] {
        let output = format!(
            "<tool_call>\n<function=set_mode>\n<parameter=mode>\n{text}\n</parameter>\n</function>\n</tool_call>"
        );
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        let call = &value(&result).calls[0];
        assert_eq!(call.outcome, CallOutcome::Resolved, "{text}");
        assert_eq!(
            call.arguments.get("mode").map(Node::to_json).as_deref(),
            Some(expected)
        );
        assert!(
            call.keywords
                .iter()
                .any(|keyword| keyword.keyword == "anyOf" && keyword.used == used),
            "{text}: {:#?}",
            call.keywords
        );
    }
}

#[derive(Debug, PartialEq)]
struct Read {
    path: String,
    limit: Option<u32>,
}

impl FromNode for Read {
    fn from_node(node: &Node) -> Result<Self, FromNodeError> {
        let null = Node::new(Value::Null);
        Ok(Self {
            path: String::from_node(node.get("path").unwrap_or(&null))?,
            limit: Option::from_node(node.get("limit").unwrap_or(&null))?,
        })
    }
}

#[test]
fn arguments_as_typed() {
    let tools = compile([ToolDefinition::new("read")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())
        .param("limit", Param::integer())]);
    let output = format!(
        "{}\n{}",
        hermes("read", r#"{"path": "a.rs", "limit": 1e1}"#),
        hermes("read", r#"{"path": "b.rs"}"#)
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    assert_eq!(
        calls.calls[0].arguments_as::<Read>(),
        Ok(Read {
            path: "a.rs".into(),
            limit: Some(10)
        })
    );
    assert_eq!(
        calls.calls[1].arguments_as::<Read>(),
        Ok(Read {
            path: "b.rs".into(),
            limit: None
        })
    );
    let error = calls.calls[0].arguments_as::<Vec<u8>>().unwrap_err();
    assert_eq!(error.pointer, "");
}

#[test]
fn keywords_reported_used_or_ignored() {
    let tools = ToolSet::from_mcp(
        r#"{"name":"tag","inputSchema":{"type":"object","x-vendor":true,"patternProperties":{"^x-":{"type":"string"}},"properties":{"url":{"type":"string","format":"uri"},"note":{"type":"string","minLength":2}},"required":["url"]}}"#,
        Budget::default(),
    )
    .expect("mcp tool");
    let result = tools.recover(
        hermes("tag", r#"{"url": "https://example.com"}"#).as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let call = &value(&result).calls[0];
    assert_eq!(call.outcome, CallOutcome::Resolved);
    let reported: Vec<(&str, &str, bool)> = call
        .keywords
        .iter()
        .map(|keyword| {
            (
                keyword.pointer.as_str(),
                keyword.keyword.as_str(),
                keyword.used,
            )
        })
        .collect();
    assert_eq!(
        reported,
        [
            ("", "x-vendor", false),
            ("", "patternProperties", true),
            ("/properties/url", "format", false),
        ]
    );
    let keywords: Vec<(&str, &str, bool)> = result
        .receipt
        .validations
        .iter()
        .filter(|validation| validation.constraint.starts_with("keyword "))
        .map(|validation| {
            (
                validation.constraint.as_str(),
                validation.diagnostics[0].as_str(),
                validation.passed,
            )
        })
        .collect();
    assert_eq!(
        keywords,
        [
            (
                "keyword /x-vendor",
                "ignored-by-repair; enforced-by-validation",
                true
            ),
            ("keyword /patternProperties", "used", true),
            (
                "keyword /properties/url/format",
                "ignored-by-repair; enforced-by-validation",
                true
            ),
        ]
    );
    // A builder tool has no unmodeled keyword.
    let tools = compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)]);
    let result = tools.recover(
        hermes("ls", "{}").as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    assert_eq!(value(&result).calls[0].keywords.len(), 0);
}

#[test]
fn usable_never_yields_mutating_guess() {
    let tools = compile([
        ToolDefinition::new("read_file").effects(Effects::ReadOnly),
        ToolDefinition::new("delete_record").effects(Effects::Mutating),
        ToolDefinition::new("list_dir").effects(Effects::Mutating),
    ]);
    let output = format!(
        "{}\n{}\n{}",
        hermes("read_fiel", "{}"),
        hermes("delete_recrod", "{}"),
        hermes("list_dir", "{}")
    );
    let result = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::default().policy(PolicySelection::All(RepairPolicy::Tolerant)),
        DialectSet::built_in(),
    );
    let calls = value(&result);
    assert_eq!(calls.calls[1].outcome, CallOutcome::BestGuess);
    assert_eq!(calls.calls[1].effects, Some(Effects::Mutating));
    let usable: Vec<&str> = calls.usable().iter().map(|call| call.id.as_str()).collect();
    assert_eq!(usable, [format!("{}2", turn_id_prefix(&calls.turn))]);
    let accepting: Vec<&str> = calls
        .usable_accepting_guesses()
        .iter()
        .map(|call| call.tool.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(accepting, ["read_file", "list_dir"]);
    for call in calls
        .usable()
        .into_iter()
        .chain(calls.usable_accepting_guesses())
    {
        assert!(
            !(call.effects == Some(Effects::Mutating) && call.repairs.guessed()),
            "{call:#?}"
        );
    }
}

#[test]
fn motivating_case_closers_only() {
    let output = b"<tool_call><function=f></parameters></parameters></function></tool_call>";
    let tools = compile([ToolDefinition::new("f").param("path", Param::string())]);
    let result = tools.recover(
        output,
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("{:#?}", result.outcome)
    };
    let call = &calls.calls[0];
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.tool.as_deref(), Some("f"));
    assert_eq!(call.arguments.to_json(), "{}");
    assert_eq!(call.repairs.losses.len(), 1);
    assert_eq!(call.repairs.losses[0].kind, LossKind::Markup);
    assert_eq!(result.receipt.losses.len(), 1);
    assert_eq!(calls.finish, Finish::Calls);
    let tools = compile([ToolDefinition::new("f").param("path", Param::string().required())]);
    let result = tools.recover(
        output,
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let call = &value(&result).calls[0];
    assert!(
        matches!(call.outcome, CallOutcome::Unresolved { .. }),
        "{call:#?}"
    );
    assert_eq!(value(&result).usable().len(), 0);
    assert_eq!(value(&result).finish, Finish::Unresolved);
}

// The 1 MiB budget assertions (C4) live in `tool_call_cost.rs`
// (`one_mib_argument_fits_published_budget`), with the published cost bound.

#[test]
fn exact_effort_accepts_canonical_markup() {
    assert_anchor_fixtures_resolve(Effort::Exact, &DialectId::BUILT_IN, false);
}

/// The default request allows every built-in dialect: under the exact
/// preset (one candidate), dialect attempts that die on their own (a Hermes
/// attempt over a Qwen `<tool_call>` opener) must spend no candidate.
#[test]
fn exact_effort_all_dialects_qwen_resolves() {
    assert_anchor_fixtures_resolve(
        Effort::Exact,
        &[DialectId::Qwen3CoderXml, DialectId::Hermes],
        true,
    );
    // The genuine competition of 2^20 readings is still explicit exhaustion.
    let tools = compile([ToolDefinition::new("f").effects(Effects::ReadOnly)]);
    let many: Vec<u8> = (0..20)
        .flat_map(|index| format!("call {index} <tool><call=f></call></tool>\n").into_bytes())
        .collect();
    let both = set(&[custom(MINI), custom(OVERLAP)]);
    for effort in [Effort::Exact, Effort::Maximum] {
        let result = tools.recover(&many, &ToolRecoveryRequest::new(effort), &both);
        assert_eq!(
            result.receipt.stopped,
            StopReason::BudgetExceeded(Resource::Candidates),
            "{effort:?}"
        );
    }
}

#[test]
fn exact_effort_never_repairs() {
    let built_in = DialectSet::built_in();
    let tools = compile([
        ToolDefinition::new("read_file")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required()),
        ToolDefinition::new("list_dir")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required())
            .param("depth", Param::integer()),
    ]);
    let cases = exact_repair_cases();
    // With the model's one dialect, and with every built-in dialect allowed.
    let requests = |dialect: &DialectId| {
        [
            (
                ToolRecoveryRequest::new(Effort::Exact),
                set(core::slice::from_ref(dialect)),
            ),
            (
                ToolRecoveryRequest::new(Effort::Exact),
                set(&DialectId::BUILT_IN),
            ),
        ]
    };
    for ((output, rule, _), (exact, dialects)) in cases
        .iter()
        .flat_map(|case| requests(&case.2).map(|request| (case, request)))
    {
        // Every other effort repairs it.
        let quick = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::new(Effort::Quick),
            built_in,
        );
        let repaired = &value(&quick).calls[0];
        assert!(
            matches!(
                repaired.outcome,
                CallOutcome::Resolved | CallOutcome::BestGuess
            ),
            "{rule}: {repaired:#?}"
        );
        assert!(
            rules(repaired).contains(rule),
            "{rule}: {:?}",
            rules(repaired)
        );
        // Exact does not.
        let result = tools.recover(output.as_bytes(), &exact, &dialects);
        let calls = value(&result);
        let call = &calls.calls[0];
        assert_eq!(
            call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::RepairNotPermitted {
                    rule: (*rule).to_owned()
                }
            },
            "{rule}"
        );
        assert_eq!(call.span, repaired.span, "{rule}: the raw span is kept");
        // Repaired markup yields no value; a name that is not exact keeps
        // its exactly parsed arguments, unresolved.
        let arguments = if *rule == "tool_call.name.match" {
            r#"{"path":"a.rs"}"#
        } else {
            "null"
        };
        assert_eq!(call.arguments.to_json(), arguments, "{rule}");
        assert_eq!(calls.usable().len(), 0);
        assert_eq!(calls.usable_accepting_guesses().len(), 0);
        assert_eq!(calls.finish, Finish::Unresolved);
        for evidence in result
            .receipt
            .evidence
            .iter()
            .chain(call.repairs.evidence.iter())
            .chain(
                call.alternatives
                    .iter()
                    .flat_map(|reading| &reading.repairs.evidence),
            )
        {
            let found = purrassist_core::tool_call::rules::rule(&evidence.rule).expect("rule");
            assert!(!found.is_repair(), "{rule}: {found:?}");
        }
        assert_eq!(result.receipt.inferences.len(), 0, "{rule}");
        assert_eq!(
            WireValue::to_node(calls)
                .pointer("/calls/0/outcome/reason/code")
                .and_then(Node::as_str),
            Some("repair_not_permitted")
        );
    }
}

fn exact_repair_cases() -> [(String, &'static str, DialectId); 4] {
    [
        (
            "<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</parameter>\n</function>\n</tool_call>".to_owned(),
            "tool_call.markup.stray-closer",
            DialectId::Qwen3CoderXml,
        ),
        (
            "<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n<parameter=depth>\n2\n</parameter>\n</function>\n</tool_call>".to_owned(),
            "tool_call.markup.missing-parameter-close",
            DialectId::Qwen3CoderXml,
        ),
        (
            "<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</function>\n</tool_call>".to_owned(),
            "tool_call.markup.parameter-closed-by-element",
            DialectId::Qwen3CoderXml,
        ),
        (
            hermes("read_fiel", r#"{"path": "a.rs"}"#),
            "tool_call.name.match",
            DialectId::Hermes,
        ),
    ]
}

/// A segmentation that itself needs a repair (a reasoning block closed at a
/// call opener) has no exact reading at all.
#[test]
fn exact_effort_rejects_segmentation_repair() {
    let tools = compile([ToolDefinition::new("read_file")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    let output = format!("<think>plan{}", hermes("read_file", r#"{"path": "a.rs"}"#));
    let quick = tools.recover(
        output.as_bytes(),
        &ToolRecoveryRequest::new(Effort::Quick),
        DialectSet::built_in(),
    );
    assert!(
        quick
            .receipt
            .evidence
            .iter()
            .any(|evidence| evidence.rule == "tool_call.reasoning.unclosed")
    );
    for (exact, dialects) in [
        (
            ToolRecoveryRequest::new(Effort::Exact),
            set(&[DialectId::Hermes]),
        ),
        (
            ToolRecoveryRequest::new(Effort::Exact),
            set(&DialectId::BUILT_IN),
        ),
    ] {
        let result = tools.recover(output.as_bytes(), &exact, &dialects);
        assert_eq!(result.outcome, Outcome::Unresolved);
        assert_eq!(result.receipt.stopped, StopReason::NoCandidate);
        assert_eq!(result.receipt.evidence.len(), 0);
        let validation = &result.receipt.validations[0];
        assert_eq!(validation.constraint, "effort=exact");
        assert!(!validation.passed);
        assert!(validation.diagnostics[0].contains("tool_call.reasoning.unclosed"));
    }
}

/// Reports queued behind a call whose inserted wrapper closer is still
/// being decided are charged as scratch (`cost::QUEUED_REPORT` each) until
/// processed: the recognizer's peak reflects the queue, a scratch budget the
/// same turn fits in when its wrapper closes (so its reports never queue)
/// is too small for the held queue and ends in an explicit
/// `BudgetExceeded(ScratchBytes)`, never a panic or a partial value, and
/// the charges are identical however the output is chunked.
#[test]
fn held_reports_are_charged_and_exhaust_explicitly() {
    const BLOCKS: usize = 4000;
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("a", Param::string())]);
    let call = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>";
    let thoughts = "<think>t</think>".repeat(BLOCKS);
    // The wrapper closer never comes: every reasoning marker after the
    // call waits for the end of output.
    let held = format!("{call}\nDone.{thoughts}");
    // The same turn with its closer: markers are processed as they come.
    let closed = format!("{call}\n</tool_call>\nDone.{thoughts}");
    let dialects = set(&[DialectId::Qwen3CoderXml]);
    let recognize = |output: &str, chunk: usize, scratch: u64| {
        let mut governor = Governor::new(Budget {
            scratch_bytes: scratch,
            ..Effort::Standard.budget()
        });
        let mut recognizer = purrassist_core::tool_call::Recognizer::with_tools(
            &dialects,
            &tools,
            ReasoningPolicy::default(),
            &mut governor,
        )
        .expect("recognizer");
        let turn = output
            .as_bytes()
            .chunks(chunk)
            .try_for_each(|piece| recognizer.push(piece, &mut governor))
            .and_then(|()| recognizer.finish(GenerationStop::Stop, &mut governor));
        (turn.map(|turn| format!("{turn:?}")), governor.usage())
    };
    let ample = Effort::Standard.budget().scratch_bytes;
    let (held_turn, held_usage) = recognize(&held, held.len(), ample);
    let (closed_turn, closed_usage) = recognize(&closed, closed.len(), ample);
    assert!(held_turn.is_ok() && closed_turn.is_ok());
    // Two markers per block were queued at once.
    let queue = 2 * BLOCKS as u64 * cost::QUEUED_REPORT;
    assert!(
        held_usage.peak_scratch_bytes >= queue
            && held_usage.peak_scratch_bytes > closed_usage.peak_scratch_bytes,
        "{held_usage:?} vs {closed_usage:?}"
    );
    // A budget the closed turn fits in exactly is too small for the queue.
    let tight = closed_usage.peak_scratch_bytes;
    assert!(recognize(&closed, closed.len(), tight).0.is_ok());
    let (exhausted, _) = recognize(&held, held.len(), tight);
    assert_eq!(
        exhausted,
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    // Through the operation: an explicit exhaustion, no value.
    let mut request = ToolRecoveryRequest::sized_for(Effort::Standard, held.as_bytes(), &dialects);
    request.budget.scratch_bytes = tight;
    let result = tools.recover(held.as_bytes(), &request, &dialects);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::ScratchBytes)
    );
    // With enough scratch the operation completes: the call is the
    // before-text guess and every block is reasoning (merged ranges).
    let request = ToolRecoveryRequest::sized_for(Effort::Standard, held.as_bytes(), &dialects);
    let result = tools.recover(held.as_bytes(), &request, &dialects);
    let Outcome::BestGuess(calls) = &result.outcome else {
        panic!("{:?}", result.receipt.stopped)
    };
    assert_eq!(calls.calls[0].outcome, CallOutcome::BestGuess);
    assert!(rules(&calls.calls[0]).contains(&"tool_call.markup.missing-wrapper-close.before-text"));
    assert_eq!(calls.reasoning.len(), 1);
    assert_eq!(calls.reasoning[0], held.len() - thoughts.len()..held.len());
    assert!(result.receipt.usage.peak_scratch_bytes >= queue);
    // The same turn and charges for every chunking.
    for chunk in [1, 2, 3, 7, 64] {
        assert_eq!(
            recognize(&held, chunk, ample),
            (held_turn.clone(), held_usage),
            "chunk {chunk}"
        );
    }
}

/// Fuzz regression (`scripts/fuzz.py --target tool-call`): a class run that
/// dies on a multi-byte character fails at that character's lead byte, so a
/// rejected span never splits a well-formed UTF-8 character. Before the fix
/// the name class consumed `0xC3` and failed on `0xA9`, ending the rejected
/// span between the two bytes of `é`. Without its closing brace the
/// attempt keeps the span its grammar read and the rest stays content (F4);
/// with it, the attempt runs to that brace and no markup is content (S3).
#[test]
fn rejected_span_never_splits_a_character_a_class_cannot_consume() {
    let tools = ToolSet::compile(
        [ToolDefinition::new("get_weather")
            .effects(Effects::ReadOnly)
            .param("city", Param::string())],
        Budget::default(),
    )
    .expect("tools");
    for (input, at, closed) in [
        (
            "{\"id\":\"a\",\"type\":\"function\",\"function\":{\"name\"\u{e9}",
            46,
            false,
        ),
        (
            "{\"id\":\"a\",\"type\":\"function\",\"function\":{\"name\": \"get_weathe\u{e9}r\"}}",
            59,
            true,
        ),
    ] {
        let result = tools.recover(
            input.as_bytes(),
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = result.outcome else {
            panic!("{input:?}: one reading expected");
        };
        assert_eq!(calls.rejected.len(), 1, "{input:?}");
        let rejected = &calls.rejected[0];
        assert_eq!(rejected.reason, Rejection::Malformed { at }, "{input:?}");
        assert!(input.is_char_boundary(at), "{input:?}");
        let end = if closed { input.len() } else { at };
        assert_eq!(rejected.span, 0..end, "{input:?}");
        assert_eq!(rejected.text(), Some(&input[..end]));
        if closed {
            assert_eq!(calls.content.len(), 0, "{input:?}");
        } else {
            assert_eq!(calls.content.len(), 1, "{input:?}");
            assert_eq!(calls.content[0], at..input.len(), "{input:?}");
        }
    }
}

/// S3: a call recovered inside a broken wrapper is kept, and the wrapper's
/// rejected attempt covers it: its raw text is the whole wrapper, closer
/// included, and no wrapper token is left as content, in every reading.
#[test]
fn rejected_wrapper_covers_the_call_recovered_inside_it() {
    let tools = ToolSet::compile(
        [ToolDefinition::new("f")
            .effects(Effects::ReadOnly)
            .param("a", Param::string())],
        Budget::default(),
    )
    .expect("tools");
    let input = "<tool_call>{\"name\": \"f\", \"arguments\": {\"a\": \"b\"}} junk junk</tool_call>";
    let result = tools.recover(
        input.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let readings: Vec<&ToolCalls> = match &result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => vec![calls],
        Outcome::Ambiguous(alternatives) => alternatives.iter().collect(),
        Outcome::Unresolved => panic!("unexpected outcome {:?}", result.outcome),
    };
    let mut inner = 0;
    for calls in readings {
        assert_eq!(calls.rejected.len(), 1, "{calls:?}");
        let rejected = &calls.rejected[0];
        assert_eq!(rejected.span, 0..input.len());
        assert_eq!(rejected.text(), Some(input));
        assert!(calls.content.is_empty(), "{calls:?}");
        for call in &calls.calls {
            assert_eq!(call.span, 11..49);
            inner += 1;
        }
    }
    assert_eq!(inner, 1, "the bare JSON reading keeps its call");
}

/// G1: under the default allow-list the body is the evidence that ranks
/// genuinely different readings of a shared `<tool_call>` opener through
/// the least-cost segmentation. A `<function=` body is a Qwen3-Coder XML
/// call and a JSON object body a Hermes call, whole or repaired: the other
/// dialect's attempt is rejected at the body's first byte, which costs more
/// than any repair a call needs, so no reading competes.
#[test]
fn body_evidence_selects_the_dialect_of_a_shared_opener() {
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("p", Param::string().required())]);
    for (output, dialect, rules_expected) in [
        (
            "<tool_call>\n<function=f>\n<parameter=p>\nx\n</parameter>\n</function>\n</tool_call>",
            DialectId::Qwen3CoderXml,
            &[
                "tool_call.xml.framing-newline",
                "tool_call.xml.framing-newline",
            ][..],
        ),
        (
            "<tool_call>\n<function=f>\n<parameter=p>\nx\n</parameter>\n</tool_call>",
            DialectId::Qwen3CoderXml,
            &[
                "tool_call.xml.framing-newline",
                "tool_call.xml.framing-newline",
                "tool_call.markup.missing-function-close",
            ][..],
        ),
        (
            "<tool_call>\n{\"name\": \"f\", \"arguments\": {\"p\": \"x\"}}\n</tool_call>",
            DialectId::Hermes,
            &[][..],
        ),
        (
            "<tool_call>\n{\"name\": \"f\", \"arguments\": {\"p\": \"x\",}}\n</tool_call>",
            DialectId::Hermes,
            &["json.syntax.trailing-comma"][..],
        ),
    ] {
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        let Outcome::Resolved(calls) = &result.outcome else {
            panic!("{output:?}: {:#?}", result.outcome);
        };
        assert_eq!(calls.calls.len(), 1, "{output:?}");
        let call = &calls.calls[0];
        assert_eq!(call.dialect, dialect, "{output:?}");
        assert_eq!(call.arguments.to_json(), r#"{"p":"x"}"#, "{output:?}");
        let mut recorded = rules(call);
        recorded.sort_unstable();
        let mut expected = rules_expected.to_vec();
        expected.sort_unstable();
        assert_eq!(recorded, expected, "{output:?}");
        assert!(
            calls.rejected.is_empty(),
            "{output:?}: {:#?}",
            calls.rejected
        );
        assert!(result.receipt.alternatives.is_empty(), "{output:?}");
    }
}

/// G1: a dangling attempt over a shared opener is reported as the attempt
/// of the dialect whose grammar read the body furthest, naming every
/// dialect that attempted it; the readings that differ only in which
/// dialect's attempt was rejected are one reading, never `Ambiguous`.
#[test]
fn rejected_attempts_over_a_shared_opener_are_one_reading() {
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("p", Param::string().required())]);
    for (output, dialect) in [
        (
            "<tool_call>\n<function=f>\n<parameter=p>\nx",
            DialectId::Qwen3CoderXml,
        ),
        ("<tool_call>\n{\"name\": \"f\", \"argu", DialectId::Hermes),
        (
            "<tool_call>\n<function=f>\nprose\n</tool_call>",
            DialectId::Qwen3CoderXml,
        ),
    ] {
        let result = tools.recover(
            output.as_bytes(),
            &ToolRecoveryRequest::default(),
            DialectSet::built_in(),
        );
        assert!(
            result.receipt.alternatives.is_empty(),
            "{output:?}: {:#?}",
            result.outcome
        );
        let calls = value(&result);
        assert!(calls.calls.is_empty(), "{output:?}");
        assert_eq!(calls.rejected.len(), 1, "{output:?}: {:#?}", calls.rejected);
        let rejected = &calls.rejected[0];
        assert_eq!(rejected.dialect, Some(dialect), "{output:?}");
        assert_eq!(rejected.span.start, 0, "{output:?}");
        assert_eq!(
            rejected.candidate_dialects,
            [DialectId::Hermes, DialectId::Qwen3CoderXml],
            "{output:?}"
        );
        let wire = calls.to_node();
        assert_eq!(
            wire.pointer("/rejected/0/candidate_dialects")
                .map(Node::to_json),
            Some(r#"["hermes.v1","qwen3-coder-xml.v1"]"#.to_owned()),
            "{output:?}"
        );
        assert_eq!(calls.finish, Finish::Unresolved, "{output:?}");
    }
    // A pinned dialect attempts alone.
    let result = tools.recover(
        b"<tool_call>\n<function=f>\n<parameter=p>\nx",
        &ToolRecoveryRequest::default(),
        &set(&[DialectId::Qwen3CoderXml]),
    );
    assert_eq!(
        value(&result).rejected[0].candidate_dialects,
        [DialectId::Qwen3CoderXml]
    );
}

/// G1: two dialects that read the same bytes into the same call are one
/// reading, never an ambiguity, and the call names both dialects in
/// `candidate_dialects` (on the value and on the wire).
#[test]
fn same_call_from_two_dialects_names_both() {
    let tools = compile([ToolDefinition::new("f").effects(Effects::ReadOnly)]);
    let twin = OVERLAP.replace("\"overlap.v1\"", "\"overlap-twin.v1\"");
    let (first, second) = (custom(OVERLAP), custom(&twin));
    let output = b"say <tool><call=f></call></tool> done";
    for pair in [
        [first.clone(), second.clone()],
        [second.clone(), first.clone()],
    ] {
        let result = tools.recover(output, &ToolRecoveryRequest::default(), &set(&pair));
        let calls = value(&result);
        assert_eq!(calls.calls.len(), 1);
        let call = &calls.calls[0];
        assert_eq!(call.tool.as_deref(), Some("f"));
        let mut expected = vec![first.clone(), second.clone()];
        expected.sort();
        assert_eq!(call.candidate_dialects, expected);
        assert!(call.candidate_dialects.contains(&call.dialect));
        let mut names: Vec<String> = calls
            .to_node()
            .pointer("/calls/0/candidate_dialects")
            .map(|node| match &node.value {
                Value::Array(items) => items
                    .iter()
                    .map(|item| item.as_str().expect("an id").to_owned())
                    .collect(),
                other => panic!("{other:?}"),
            })
            .expect("candidate_dialects on the wire");
        names.sort();
        assert_eq!(names.len(), 2);
        assert!(names[0].starts_with("overlap-twin.v1") && names[1].starts_with("overlap.v1"));
        assert_eq!(result.receipt.alternatives.len(), 0);
        assert_eq!(
            result
                .receipt
                .to_node()
                .pointer("/ambiguity")
                .map(Node::to_json),
            Some("null".to_owned())
        );
    }
    // One dialect reads the call alone.
    let result = tools.recover(
        output,
        &ToolRecoveryRequest::default(),
        &set(std::slice::from_ref(&first)),
    );
    assert_eq!(value(&result).calls[0].candidate_dialects, [first]);
}

/// G1: two dialects that read the same bytes into different calls with
/// identical ledgers are a genuine ambiguity, and the dialect that read each
/// call is its distinguishing evidence: a passing `dialect=ID` validation
/// of the call, so `distinguishing` is never empty.
#[test]
fn different_calls_from_two_dialects_are_distinguished_by_dialect() {
    let tools = compile([
        ToolDefinition::new("fg").effects(Effects::ReadOnly),
        ToolDefinition::new("g").effects(Effects::ReadOnly),
    ]);
    let whole = custom(
        "grammar \"whole.v1\"; ident = /[a-z]+/; call = \"<x>\" name:ident \"</x>\"; result = \"<r>\"; declaration = \"<d>\";",
    );
    let tail = custom(
        "grammar \"tail.v1\"; ident = /[a-z]+/; call = \"<x>f\" name:ident \"</x>\"; result = \"<r>\"; declaration = \"<d>\";",
    );
    let result = tools.recover(
        b"<x>fg</x>",
        &ToolRecoveryRequest::default(),
        &set(&[whole, tail]),
    );
    let Outcome::Ambiguous(values) = &result.outcome else {
        panic!("{:#?}", result.outcome);
    };
    let mut tools_read: Vec<&str> = values
        .iter()
        .map(|value| value.calls[0].tool.as_deref().expect("a tool"))
        .collect();
    tools_read.sort_unstable();
    assert_eq!(tools_read, ["fg", "g"]);
    let wire = result.receipt.to_node();
    let pairs = wire
        .pointer("/ambiguity/distinguishing")
        .expect("an annotation");
    assert_eq!(
        pairs.to_json(),
        r#"[[{"kind":"validation","rule":"purrassist.tool_call.v1"},{"kind":"validation","rule":"purrassist.tool_call.v1"}]]"#
    );
    let attested: Vec<&str> = result
        .receipt
        .validations
        .iter()
        .filter(|validation| validation.constraint.starts_with("dialect="))
        .map(|validation| validation.constraint.as_str())
        .collect();
    let mut attested = attested;
    attested.sort_unstable();
    assert_eq!(attested.len(), 2, "{:#?}", result.receipt.validations);
    assert!(attested[0].starts_with("dialect=tail.v1"), "{attested:?}");
    assert!(attested[1].starts_with("dialect=whole.v1"), "{attested:?}");
}

/// T4: a resynchronization that gave up (another attempt opened after the
/// failure) is never started again from the same failure: the attempt's
/// failure stands. Restarting it at every byte would rescan the skipped
/// call each time, quadratic work that exhausts the turn's sized budget.
#[test]
fn resync_that_gave_up_is_not_restarted() {
    let tools = ToolSet::compile(
        [ToolDefinition::new("f")
            .effects(Effects::ReadOnly)
            .param("a", Param::string())],
        Budget::default(),
    )
    .expect("tools");
    let input = format!(
        "<|tool_calls_section_begin|><|tool_call_begin|>functions.f:0 oops <tool_call>{}</tool_call> {}",
        "{\"name\": \"f\", \"arguments\": {\"a\": \"x\"}}",
        "lorem ipsum dolor sit amet ".repeat(150)
    );
    let result = tools.recover(
        input.as_bytes(),
        &ToolRecoveryRequest::sized(Effort::Standard, input.as_bytes()),
        DialectSet::built_in(),
    );
    let Outcome::Resolved(calls) = &result.outcome else {
        panic!("unexpected outcome {:?}", result.outcome);
    };
    assert_eq!(calls.calls.len(), 1, "{calls:?}");
    assert_eq!(calls.rejected.len(), 1, "{calls:?}");
    assert!(
        result.receipt.usage.work < 200_000,
        "work {}",
        result.receipt.usage.work
    );
}

/// F9 (priority rule): a repair never overrides a reading that needs none.
/// A canonical call whose string value holds text near a closer of its
/// dialect (a case variant, one slip, an over-long spelling, a declared
/// variant, the closer itself inside a JSON string) keeps its whole value,
/// `Resolved` with no rejection and no content, pinned, under the default
/// allow-list, and under `Effort::Exact`, for every closer of Qwen3-Coder
/// XML, Hermes, Kimi-K2 and `DeepSeek` v3/v3.1.
#[test]
fn canonical_values_with_near_closer_text_resolve() {
    let tools = compile([ToolDefinition::new("write_file")
        .param("path", Param::string().required())
        .param("content", Param::string().required())]);
    let cases: [(DialectId, &[&str], bool); 5] = [
        (
            DialectId::Qwen3CoderXml,
            &[
                "</parameter>",
                "</function>",
                "</tool_call>",
                "</parameters>",
                "</functions>",
                "</tool_calls>",
            ],
            false,
        ),
        (DialectId::Hermes, &["</tool_call>", "</tool_calls>"], true),
        (
            DialectId::KimiK2,
            &["<|tool_call_end|>", "<|tool_calls_section_end|>"],
            true,
        ),
        (
            DialectId::DeepSeekV31,
            &["<｜tool▁call▁end｜>", "<｜tool▁calls▁end｜>"],
            true,
        ),
        (
            DialectId::DeepSeekV3,
            &["<｜tool▁call▁end｜>", "<｜tool▁calls▁end｜>"],
            true,
        ),
    ];
    for (dialect, closers, exact_too) in cases {
        for closer in closers {
            let mut variants = near_closer(closer);
            // A canonical closer inside a JSON string is the string's text;
            // inside an XML parameter value it ends the value.
            if exact_too {
                variants.push((*closer).to_owned());
            }
            for variant in variants {
                let content = format!("x {variant} y");
                let input = near_closer_input(&dialect, &content);
                for (request, dialects) in [
                    request(dialect.clone()),
                    (ToolRecoveryRequest::default(), set(&DialectId::BUILT_IN)),
                    (
                        ToolRecoveryRequest::sized(Effort::Exact, input.as_bytes()),
                        set(std::slice::from_ref(&dialect)),
                    ),
                ] {
                    let result = tools.recover(input.as_bytes(), &request, &dialects);
                    let Outcome::Resolved(calls) = &result.outcome else {
                        panic!("{input:?}: {:#?}", result.outcome);
                    };
                    assert_eq!(calls.calls.len(), 1, "{input:?}: {calls:#?}");
                    let call = &calls.calls[0];
                    assert_eq!(call.outcome, CallOutcome::Resolved, "{input:?}");
                    assert_eq!(
                        call.arguments.pointer("/content").and_then(Node::as_str),
                        Some(content.as_str()),
                        "{input:?}"
                    );
                    assert!(calls.rejected.is_empty(), "{input:?}: {calls:#?}");
                    assert!(calls.content.is_empty(), "{input:?}: {calls:#?}");
                }
            }
        }
    }
}

/// Text near `closer`: a case variant, one slip, two over-long spellings.
fn near_closer(closer: &str) -> Vec<String> {
    let letters: Vec<usize> = closer
        .char_indices()
        .filter(|(_, character)| character.is_ascii_alphanumeric())
        .map(|(at, _)| at)
        .collect();
    let (first, last) = (letters[0], letters[letters.len() - 1] + 1);
    let slip = letters[letters.len() - 2];
    vec![
        format!(
            "{}{}{}",
            &closer[..first],
            closer[first..=first].to_ascii_uppercase(),
            &closer[first + 1..]
        ),
        format!("{}{}", &closer[..slip], &closer[slip + 1..]),
        format!("{}x{}", &closer[..last], &closer[last..]),
        format!("{}_{}", &closer[..last], &closer[last..]),
    ]
}

/// A canonical `write_file` call in `dialect` whose `content` is `content`.
fn near_closer_input(dialect: &DialectId, content: &str) -> String {
    let json = format!(
        "{{\"path\":\"a\",\"content\":\"{}\"}}",
        content.replace('"', "\\\"")
    );
    match dialect {
        DialectId::Qwen3CoderXml => format!(
            "<tool_call>\n<function=write_file>\n<parameter=path>\na\n</parameter>\n<parameter=content>\n{content}\n</parameter>\n</function>\n</tool_call>"
        ),
        DialectId::Hermes => {
            format!("<tool_call>{{\"name\":\"write_file\",\"arguments\":{json}}}</tool_call>")
        }
        DialectId::KimiK2 => format!(
            "<|tool_calls_section_begin|><|tool_call_begin|>functions.write_file:0<|tool_call_argument_begin|>{json}<|tool_call_end|><|tool_calls_section_end|>"
        ),
        DialectId::DeepSeekV31 => format!(
            "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>write_file<｜tool▁sep｜>{json}<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
        ),
        _ => format!(
            "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>write_file\n```json\n{json}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>"
        ),
    }
}
