// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The tool-call rule registry and call resolution: policy admission, cost
//! grounding, loss kinds, the `Mutating` guard, name and parameter
//! resolution, typing, repeats, unknown parameters, examples, confusable
//! terminals, the required missing-`</parameter>` repair, end-of-output
//! completeness, stray closers, `OpenAI` member order, schema validation,
//! request policies, and order independence. Every case runs grammar parses
//! of built-in (or test) dialects through `resolve_forest`. The published
//! rule table (`docs/tool-calls/RULES.md`) and custom-dialect walkthrough
//! (`docs/tool-calls/GRAMMAR.md`) are checked against the code here too.
use purrassist_core::contract::{Evidence, Inference, InferenceKind, Loss};
use purrassist_core::grammar::Grammar;
use purrassist_core::grammar::edit::{EditOperator, Grounding, LossKind, OPERATORS, Readings};
use purrassist_core::tool_call::rules::{self, DISABLED, RULES};
use purrassist_core::tool_call::{
    CallFacts, CallLedger, CallLoss, CallOutcome, DialectId, DialectSet, Effects, GenerationStop,
    ObjectSpec, Param, Policies, PolicySelection, ReasoningPolicy, RecognizedTurn, Recognizer,
    Rejection, RepairPolicy, ResolvedCall, ResolvedSegment, ResolvedTurn, ToolDefinition, ToolSet,
    UnknownParameters, UnresolvedReason, resolve_forest,
};
use purrassist_core::{Budget, Governor, StopReason};

fn set(dialects: &[DialectId]) -> DialectSet {
    DialectSet::new(
        dialects.iter().cloned(),
        &mut Governor::new(Budget::default()),
    )
    .expect("dialect set")
}

fn recognize(
    set: &DialectSet,
    tools: &ToolSet,
    text: &str,
    stop: GenerationStop,
) -> RecognizedTurn {
    let mut governor = Governor::new(Budget::default());
    let mut recognizer =
        Recognizer::with_tools(set, tools, ReasoningPolicy::default(), &mut governor)
            .expect("recognizer");
    recognizer
        .push(text.as_bytes(), &mut governor)
        .expect("push");
    recognizer.finish(stop, &mut governor).expect("finish")
}

fn resolve(tools: &ToolSet, turn: &RecognizedTurn, policies: &Policies) -> ResolvedTurn {
    resolve_forest(tools, turn, policies, &mut Governor::new(Budget::default())).expect("resolved")
}

/// Recognize `text` with `dialect` under `stop` and resolve it.
fn run_with(
    dialect: DialectId,
    tools: &ToolSet,
    text: &str,
    stop: GenerationStop,
    policies: &Policies,
) -> ResolvedTurn {
    let turn = recognize(&set(&[dialect]), tools, text, stop);
    resolve(tools, &turn, policies)
}

fn run(dialect: DialectId, tools: &ToolSet, text: &str) -> ResolvedTurn {
    run_with(
        dialect,
        tools,
        text,
        GenerationStop::ToolCalls,
        &Policies::default(),
    )
}

fn one(turn: &ResolvedTurn) -> &ResolvedCall {
    let calls = turn.calls();
    assert_eq!(calls.len(), 1, "{:#?}", turn.pieces);
    calls[0]
}

fn qwen(tool: &str, body: &str) -> String {
    format!("<tool_call>\n<function={tool}>\n{body}\n</function>\n</tool_call>")
}

fn rule_ids(call: &ResolvedCall) -> Vec<&str> {
    call.repairs()
        .evidence
        .iter()
        .map(|evidence| evidence.rule.as_str())
        .collect()
}

fn count(call: &ResolvedCall, rule: &str) -> usize {
    rule_ids(call).iter().filter(|id| **id == rule).count()
}

fn compile(definitions: impl IntoIterator<Item = ToolDefinition>) -> ToolSet {
    ToolSet::compile(definitions, Budget::default()).expect("tools compile")
}

fn evidence(rule: &str) -> Evidence {
    Evidence {
        rule: rule.into(),
        explanation: String::new(),
        span: None,
        destination: String::new(),
    }
}

const fn is_unresolved(outcome: &CallOutcome) -> bool {
    matches!(outcome, CallOutcome::Unresolved { .. })
}

#[test]
fn structural_admits_only_single_reading_rules() {
    for rule in RULES {
        assert_eq!(
            rule.admits(RepairPolicy::Structural),
            rule.readings == Readings::Single,
            "{}",
            rule.id
        );
        assert!(rule.admits(RepairPolicy::Tolerant), "{}", rule.id);
        assert_eq!(
            rule.guess,
            rule.readings == Readings::Multiple,
            "{}",
            rule.id
        );
    }
    // End to end: a guessed repair (a near-miss name) is refused under
    // Structural and admitted under Tolerant, for the same tool.
    let tools = compile([ToolDefinition::new("read_file").param("path", Param::string())]);
    let text = r#"<tool_call>{"name": "read_fiel", "arguments": {"path": "a"}}</tool_call>"#;
    for (policy, admitted) in [
        (RepairPolicy::Structural, false),
        (RepairPolicy::Tolerant, true),
    ] {
        let turn = run_with(
            DialectId::Hermes,
            &tools,
            text,
            GenerationStop::ToolCalls,
            &Policies::all(policy),
        );
        let call = one(&turn);
        assert_eq!(
            !is_unresolved(&call.outcome),
            admitted,
            "{policy:?}: {call:#?}"
        );
    }
    // Single-reading markup repairs stay admitted under Structural.
    let text = qwen("read_file", "<parameter=path>\na\n</parameters>");
    let turn = run_with(
        DialectId::Qwen3CoderXml,
        &tools,
        &text,
        GenerationStop::ToolCalls,
        &Policies::all(RepairPolicy::Structural),
    );
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(count(call, "tool_call.markup.closer-variant"), 1);
    // Reasoning guesses (D11) are multiple-reading rules: a call that
    // depends on closing an unclosed block at its opener, and a call inside
    // a closed block, are best guesses under Tolerant and never usable
    // under Structural.
    let call_text = qwen("read_file", "<parameter=path>\na\n</parameter>");
    for policy in [RepairPolicy::Structural, RepairPolicy::Tolerant] {
        let policies = Policies::all(policy);
        let unclosed = format!("<think>planning\n{call_text}");
        let turn = run_with(
            DialectId::Qwen3CoderXml,
            &tools,
            &unclosed,
            GenerationStop::ToolCalls,
            &policies,
        );
        let call = one(&turn);
        assert_eq!(count(call, "tool_call.reasoning.unclosed"), 1, "{call:#?}");
        let closed = format!("<think>maybe {call_text}</think>done");
        let turn = run_with(
            DialectId::Qwen3CoderXml,
            &tools,
            &closed,
            GenerationStop::Stop,
            &policies,
        );
        assert!(turn.calls().is_empty(), "an embedded call is not a call");
        let Some(ResolvedSegment::Reasoning { embedded, .. }) = turn.selected().first().copied()
        else {
            panic!("a reasoning segment: {:#?}", turn.pieces);
        };
        for outcome in [&call.outcome, &embedded[0].outcome] {
            match policy {
                RepairPolicy::Structural => assert!(is_unresolved(outcome), "{outcome:?}"),
                RepairPolicy::Tolerant => assert_eq!(outcome, &CallOutcome::BestGuess),
            }
        }
    }
}

#[test]
fn every_rule_has_cost_and_policy() {
    let mut ids: Vec<&str> = RULES.iter().map(|rule| rule.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), RULES.len(), "one rule per id");
    assert_eq!(RULES.len(), rules::RULE_COUNT);
    for rule in RULES {
        assert!(!rule.id.is_empty(), "every rule is named");
        assert!(!rule.policies.is_empty(), "{} has no policy", rule.id);
        assert_eq!(
            rule.cost % rules::multiplier(rule.grounding),
            0,
            "{}",
            rule.id
        );
        assert_eq!(rules::rule(rule.id), Some(rule));
    }
    // Every markup repair costs something; reading the dialect's own forms
    // costs nothing.
    for operator in OPERATORS {
        let rule = rules::of_operator(operator.operator);
        if operator.repair {
            assert!(rule.cost > 0, "{}", rule.id);
        }
    }
    for id in [
        "tool_call.xml.framing-newline",
        "tool_call.markup.nested-parameter",
        "tool_call.markup.json-body",
        "tool_call.arguments.decode-string",
    ] {
        assert_eq!(rules::rule(id).expect(id).cost, 0, "{id}");
    }
    // The required rule list is registered.
    for id in [
        "tool_call.markup.closer-variant",
        "tool_call.markup.closers-only-empty",
        "tool_call.markup.missing-parameter-close",
        "tool_call.markup.missing-function-close",
        "tool_call.markup.missing-wrapper-close",
        "tool_call.markup.missing-wrapper-close.before-text",
        "tool_call.markup.missing-closers-at-end",
        "tool_call.markup.nested-parameter",
        "tool_call.markup.json-body",
        "tool_call.markup.stray-closer",
        "tool_call.extract.prose",
        "tool_call.arguments.decode-string",
        "tool_call.markup.dangling-opener",
        "tool_call.markup.terminal-confusable",
        "tool_call.stop.consumed-closer",
        "tool_call.reasoning.unclosed",
        "tool_call.reasoning.embedded-call",
        "tool_call.context.fenced-example",
        "tool_call.xml.framing-newline",
        "tool_call.value.trim-nonverbatim",
        "tool_call.id.dedup",
        "tool_call.name.match",
        "json.key.match",
        "json.key.relocate",
        "json.enum.match",
    ] {
        assert!(rules::rule(id).is_some(), "{id} is registered");
    }
    // Missing-`</parameter>` is required under every policy.
    let close = rules::rule("tool_call.markup.missing-parameter-close").expect("rule");
    assert_eq!(
        (close.readings, close.guess, close.grounding),
        (Readings::Single, false, Grounding::Definition)
    );
    assert!(close.admits(RepairPolicy::Structural));
    // A wrapper closer inserted before text is a guess; before another
    // call's opener or the end of output it is the only reading.
    let wrapper = rules::rule("tool_call.markup.missing-wrapper-close").expect("rule");
    let before_text =
        rules::rule("tool_call.markup.missing-wrapper-close.before-text").expect("rule");
    assert!(!wrapper.guess && wrapper.admits(RepairPolicy::Structural));
    assert!(before_text.guess && !before_text.admits(RepairPolicy::Structural));
    assert_eq!(before_text.readings, Readings::Multiple);
}

#[test]
fn every_rule_declares_loss_kind() {
    let markup: Vec<&str> = RULES
        .iter()
        .filter(|rule| rule.loss == Some(LossKind::Markup))
        .map(|rule| rule.id)
        .collect();
    let data: Vec<&str> = RULES
        .iter()
        .filter(|rule| rule.loss == Some(LossKind::Data))
        .map(|rule| rule.id)
        .collect();
    assert_eq!(
        markup,
        [
            "tool_call.markup.stray-closer",
            "tool_call.markup.closer-variant",
            "tool_call.markup.terminal-confusable",
            "tool_call.markup.closers-only-empty",
            "tool_call.xml.framing-newline",
            "tool_call.extract.fence",
            "json.syntax.comment",
            "json.syntax.literal-case",
            "json.syntax.string",
            "json.syntax.trailing-comma",
        ]
    );
    assert_eq!(
        data,
        [
            "tool_call.reasoning.unterminated",
            "tool_call.extract.prose",
            "tool_call.parameter.drop-unknown",
            "tool_call.parameter.repeated",
            "tool_call.truncated",
            "tool_call.markup.dangling-opener",
            "tool_call.input.invalid-utf8",
        ]
    );
    // Rules that discard nothing declare no loss: insertions, readings,
    // name and value resolution.
    for id in [
        "tool_call.markup.missing-parameter-close",
        "tool_call.markup.missing-closers-at-end",
        "tool_call.value.trim-nonverbatim",
        "tool_call.name.match",
        "json.key.match",
        "json.enum.match",
    ] {
        assert_eq!(rules::rule(id).expect(id).loss, None, "{id}");
    }
}

#[test]
fn rules_table_matches_engine_operators() {
    assert!(RULES.len() > OPERATORS.len());
    for (index, operator) in OPERATORS.iter().enumerate() {
        let rule = &RULES[index];
        assert_eq!(rule.operator, Some(operator.operator));
        assert_eq!(rules::of_operator(operator.operator), rule);
        assert_eq!(rule.id, operator.id);
        assert_eq!(
            rule.cost,
            operator.cost * rules::multiplier(operator.grounding)
        );
        assert_eq!(
            (rule.guess, rule.readings, rule.grounding, rule.loss),
            (
                operator.guess,
                operator.readings,
                operator.grounding,
                operator.loss
            ),
            "{}",
            rule.id
        );
    }
    for rule in &RULES[OPERATORS.len()..] {
        assert_eq!(rule.operator, None, "{}", rule.id);
        assert!(OPERATORS.iter().all(|operator| operator.id != rule.id));
    }
    for id in DISABLED {
        assert!(rules::rule(id).is_none(), "{id} is disabled");
    }
}

#[test]
fn mutating_guess_never_resolved() {
    for effects in [Effects::ReadOnly, Effects::Mutating] {
        for policy in [RepairPolicy::Structural, RepairPolicy::Tolerant] {
            for guess in [false, true] {
                for loss in [None, Some(LossKind::Markup), Some(LossKind::Data)] {
                    let mut ledger = CallLedger::default();
                    ledger
                        .evidence
                        .push(evidence("tool_call.markup.closers-only-empty"));
                    if guess {
                        ledger.evidence.push(evidence("tool_call.name.match"));
                        ledger.inferences.push(Inference {
                            evidence: evidence("tool_call.name.match"),
                            kind: InferenceKind::Guess,
                            value: "f".into(),
                        });
                    }
                    if let Some(kind) = loss {
                        let rule = match kind {
                            LossKind::Markup => "tool_call.markup.closers-only-empty",
                            LossKind::Data => "tool_call.parameter.drop-unknown",
                        };
                        ledger.evidence.push(evidence(rule));
                        ledger.losses.push(CallLoss {
                            kind,
                            loss: Loss {
                                evidence: evidence(rule),
                                original: "x".into(),
                            },
                        });
                    }
                    let outcome = CallOutcome::classify(CallFacts {
                        effects,
                        policy,
                        ledger: &ledger,
                        competing: 1,
                        blocker: None,
                    });
                    let context = format!("{effects:?} {policy:?} guess={guess} {loss:?}");
                    if guess || loss == Some(LossKind::Data) {
                        assert_ne!(outcome, CallOutcome::Resolved, "{context}");
                    }
                    if effects == Effects::Mutating && guess {
                        assert_ne!(outcome, CallOutcome::Resolved, "{context}");
                    }
                    let expected = match (guess, loss, policy) {
                        (true, _, RepairPolicy::Structural) => "unresolved",
                        (true, _, RepairPolicy::Tolerant) | (false, Some(LossKind::Data), _) => {
                            "best-guess"
                        }
                        (false, _, _) => "resolved",
                    };
                    let actual = match outcome {
                        CallOutcome::Resolved => "resolved",
                        CallOutcome::BestGuess => "best-guess",
                        CallOutcome::Ambiguous => "ambiguous",
                        CallOutcome::Unresolved { .. } => "unresolved",
                    };
                    assert_eq!(actual, expected, "{context}");
                }
            }
        }
    }
    // End to end: a near-miss name on a mutating tool under a tolerant
    // policy is a best guess, never resolved.
    let tools = compile([ToolDefinition::new("write_file").param("path", Param::string())]);
    let text = r#"<tool_call>{"name": "write_fil", "arguments": {"path": "a"}}</tool_call>"#;
    let turn = run_with(
        DialectId::Hermes,
        &tools,
        text,
        GenerationStop::ToolCalls,
        &Policies::all(RepairPolicy::Tolerant),
    );
    let call = one(&turn);
    assert_eq!(call.reading.effects, Some(Effects::Mutating));
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
}

fn two_tools(effects: Effects) -> ToolSet {
    compile([
        ToolDefinition::new("read_file")
            .effects(effects)
            .param("path", Param::string().required()),
        ToolDefinition::new("get_weather")
            .effects(effects)
            .param("city", Param::string().required()),
    ])
}

#[test]
fn hallucinated_name_unresolved_both_policies() {
    let tools = two_tools(Effects::ReadOnly);
    let text = r#"<tool_call>{"name": "send_email", "arguments": {"to": "a@b.c"}}</tool_call>"#;
    for policy in [RepairPolicy::Structural, RepairPolicy::Tolerant] {
        let turn = run_with(
            DialectId::Hermes,
            &tools,
            text,
            GenerationStop::ToolCalls,
            &Policies::all(policy),
        );
        let call = one(&turn);
        assert_eq!(
            call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::UnknownTool {
                    name: "send_email".into()
                }
            },
            "{policy:?}"
        );
        assert_eq!(call.tool(), None, "never coerced into a real tool");
    }
}

#[test]
fn near_miss_best_guess_tolerant_unresolved_structural() {
    let text = r#"<tool_call>{"name": "read_fiel", "arguments": {"path": "a.rs"}}</tool_call>"#;
    // A read-only tool resolves under Tolerant: a labeled best guess.
    let tools = two_tools(Effects::ReadOnly);
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
    assert_eq!(call.tool(), Some("read_file"));
    assert_eq!(call.arguments().to_json(), r#"{"path":"a.rs"}"#);
    let guess = &call.repairs().inferences[0];
    assert_eq!(
        (
            guess.evidence.rule.as_str(),
            guess.kind,
            guess.value.as_str()
        ),
        ("tool_call.name.match", InferenceKind::Guess, "read_file")
    );
    // A mutating tool is Structural: exact names or aliases only.
    let tools = two_tools(Effects::Mutating);
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert!(
        matches!(&call.outcome, CallOutcome::Unresolved { reason: UnresolvedReason::RuleNotAdmitted { rule, policy: RepairPolicy::Structural } } if rule == "tool_call.name.match"),
        "{call:#?}"
    );
    // An explicit alias is exact under Structural.
    let tools = compile([ToolDefinition::new("read_file")
        .alias("cat")
        .param("path", Param::string())]);
    let text = r#"<tool_call>{"name": "cat", "arguments": {"path": "a"}}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(call.tool(), Some("read_file"));
    assert_eq!(rule_ids(call), ["tool_call.name.alias"]);
}

#[test]
fn definition_grounded_beats_syntax_only() {
    // Equal base cost, lower ranking cost when grounded in the definition.
    let definition = rules::of_operator(EditOperator::MissingParameterClose);
    let syntax = rules::of_operator(EditOperator::UndeclaredParameterBoundary);
    assert_eq!(
        EditOperator::MissingParameterClose.rule().cost,
        EditOperator::UndeclaredParameterBoundary.rule().cost
    );
    assert!(definition.cost < syntax.cost);
    for rule in RULES {
        let base = rule.cost / rules::multiplier(rule.grounding);
        assert!(
            base * rules::DEFINITION_COST_MULTIPLIER <= base * rules::SYNTAX_COST_MULTIPLIER,
            "{}",
            rule.id
        );
    }
    // End to end: the same missing `</parameter>`, ended by a declared
    // parameter's opener (the definition is known to the codec) or by any
    // opener (no tool known while reading).
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("a", Param::string())
        .param("b", Param::string())]);
    let text = qwen("f", "<parameter=a>\nx\n<parameter=b>\ny\n</parameter>");
    let grounded = run(DialectId::Qwen3CoderXml, &tools, &text);
    let grounded = one(&grounded);
    assert_eq!(grounded.outcome, CallOutcome::Resolved);
    assert_eq!(
        count(grounded, "tool_call.markup.missing-parameter-close"),
        1
    );
    let dialects = set(&[DialectId::Qwen3CoderXml]);
    let mut governor = Governor::new(Budget::default());
    let mut recognizer =
        Recognizer::new(&dialects, ReasoningPolicy::default(), &mut governor).expect("recognizer");
    recognizer
        .push(text.as_bytes(), &mut governor)
        .expect("push");
    let blind = recognizer
        .finish(GenerationStop::ToolCalls, &mut governor)
        .expect("finish");
    let blind = resolve(&tools, &blind, &Policies::default());
    let blind = one(&blind);
    assert_eq!(
        count(blind, "tool_call.markup.undeclared-parameter-boundary"),
        1
    );
    assert_eq!(blind.arguments().to_json(), grounded.arguments().to_json());
    assert!(grounded.reading.cost < blind.reading.cost);
    assert_eq!(blind.outcome, CallOutcome::BestGuess);
}

fn string_tool(effects: Effects) -> ToolSet {
    compile([ToolDefinition::new("f")
        .effects(effects)
        .param("code", Param::string().required())])
}

#[test]
fn declared_string_never_parsed() {
    let tools = string_tool(Effects::Mutating);
    for literal in ["12345", "true", "null", "{\"a\": 1}", "1.50"] {
        let text = qwen("f", &format!("<parameter=code>\n{literal}\n</parameter>"));
        let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{literal}");
        assert_eq!(
            call.arguments().get("code").and_then(|node| node.as_str()),
            Some(literal)
        );
    }
    // A JSON number for a string parameter is never stringified.
    let text = r#"<tool_call>{"name": "f", "arguments": {"code": 12345}}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::SchemaValidation
        }
    );
    assert_eq!(call.arguments().to_json(), r#"{"code":12345}"#);
    assert!(!call.reading.validations[0].passed);
    // Numbers keep their exact spelling.
    let tools = compile([ToolDefinition::new("g").param("ratio", Param::number())]);
    let text = qwen("g", "<parameter=ratio>\n2.50\n</parameter>");
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    assert_eq!(one(&turn).arguments().to_json(), r#"{"ratio":2.50}"#);
}

#[test]
fn repeated_scalar_reports_both_readings() {
    for effects in [Effects::ReadOnly, Effects::Mutating] {
        let tools = compile([ToolDefinition::new("f")
            .effects(effects)
            .param("path", Param::string())]);
        let text = qwen(
            "f",
            "<parameter=path>\na\n</parameter>\n<parameter=path>\nb\n</parameter>",
        );
        let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
        let call = one(&turn);
        assert_eq!(
            call.outcome,
            CallOutcome::Ambiguous,
            "{effects:?}: {call:#?}"
        );
        // First-wins and last-wins, in the total (content-digest) order.
        let mut readings = [
            call.arguments().to_json(),
            call.alternatives[0].arguments.to_json(),
        ];
        readings.sort();
        assert_eq!(readings, [r#"{"path":"a"}"#, r#"{"path":"b"}"#]);
        assert_eq!(call.alternatives.len(), 1);
        for reading in [&call.reading, &call.alternatives[0]] {
            assert_eq!(
                reading
                    .repairs
                    .evidence
                    .iter()
                    .filter(|evidence| evidence.rule == "tool_call.parameter.repeated")
                    .count(),
                1
            );
            assert!(
                reading.repairs.data_loss(),
                "the dropped occurrence is data"
            );
        }
        assert!(!call.distinguishing.is_empty(), "{call:#?}");
    }
}

#[test]
fn repeated_array_param_is_array() {
    let tools = compile([ToolDefinition::new("f")
        .param("tags", Param::array(Param::string()))
        .param("ids", Param::integer().repeatable())]);
    let text = qwen(
        "f",
        "<parameter=tags>\na\n</parameter>\n<parameter=ids>\n1\n</parameter>\n<parameter=tags>\nb\n</parameter>\n<parameter=ids>\n2\n</parameter>",
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments().to_json(),
        r#"{"tags":["a","b"],"ids":[1,2]}"#
    );
    assert_eq!(count(call, "tool_call.parameter.collect"), 2);
    // A JSON array value and a repeat combine; a single repeatable
    // occurrence is a one-element array.
    let text = qwen(
        "f",
        "<parameter=tags>\n[\"a\", \"b\"]\n</parameter>\n<parameter=tags>\nc\n</parameter>\n<parameter=ids>\n7\n</parameter>",
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(
        call.arguments().to_json(),
        r#"{"tags":["a","b","c"],"ids":[7]}"#
    );
    assert_eq!(call.outcome, CallOutcome::Resolved);
}

#[test]
fn positional_text_never_named() {
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("a", Param::string())]);
    // Text before, between or after parameters is never a parameter.
    for body in [
        "hello\n<parameter=a>\nx\n</parameter>",
        "<parameter=a>\nx\n</parameter>\ntrailing text",
    ] {
        let turn = run(DialectId::Qwen3CoderXml, &tools, &qwen("f", body));
        assert!(turn.calls().is_empty(), "{body}");
        assert!(
            turn.selected().iter().any(|segment| matches!(
                segment,
                ResolvedSegment::Rejected {
                    reason: Rejection::Malformed { .. },
                    ..
                }
            )),
            "{body}"
        );
    }
    // Positional arguments are not an object of named parameters.
    let text = r#"<tool_call>{"name": "f", "arguments": ["x"]}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    assert_eq!(
        one(&turn).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::ArgumentsNotObject
        }
    );
}

#[test]
fn unknown_parameter_policy_each_mode() {
    let text = r#"<tool_call>{"name": "f", "arguments": {"path": "a", "zzzzzz": 1}}</tool_call>"#;
    for (handling, expected, arguments) in [
        (
            UnknownParameters::Error,
            "unresolved",
            r#"{"path":"a","zzzzzz":1}"#,
        ),
        (
            UnknownParameters::DropWithLoss,
            "best-guess",
            r#"{"path":"a"}"#,
        ),
        (
            UnknownParameters::PassThrough,
            "resolved",
            r#"{"path":"a","zzzzzz":1}"#,
        ),
    ] {
        for policy in [RepairPolicy::Structural, RepairPolicy::Tolerant] {
            let tools = compile([ToolDefinition::new("f")
                .unknown_parameters(handling)
                .param("path", Param::string())]);
            let turn = run_with(
                DialectId::Hermes,
                &tools,
                text,
                GenerationStop::ToolCalls,
                &Policies::all(policy),
            );
            let call = one(&turn);
            let actual = match &call.outcome {
                CallOutcome::Resolved => "resolved",
                CallOutcome::BestGuess => "best-guess",
                CallOutcome::Ambiguous => "ambiguous",
                CallOutcome::Unresolved { reason } => {
                    assert_eq!(
                        reason,
                        &UnresolvedReason::UnknownParameter {
                            pointer: "/zzzzzz".into()
                        }
                    );
                    "unresolved"
                }
            };
            assert_eq!(actual, expected, "{handling:?} {policy:?}");
            assert_eq!(
                call.arguments().to_json(),
                arguments,
                "{handling:?} {policy:?}"
            );
        }
    }
    // Under Tolerant a near-miss name is renamed rather than dropped.
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    let text = r#"<tool_call>{"name": "f", "arguments": {"pth": "a"}}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.arguments().to_json(), r#"{"path":"a"}"#);
    assert_eq!(rule_ids(call), ["json.key.match"]);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
}

/// Under `Structural` a near-miss parameter name is never dropped in place
/// of the rename the evidence favors: dropping `mdoe` would recover a
/// different call (found by the tool-call mutation corpus). An undeclared
/// name that is no near miss is still dropped with a loss.
#[test]
fn structural_never_drops_a_near_miss_parameter() {
    let text =
        r#"<tool_call>{"name": "f", "arguments": {"query": "a", "mdoe": "regex"}}</tool_call>"#;
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .unknown_parameters(UnknownParameters::DropWithLoss)
        .param("query", Param::string().required())
        .param("mode", Param::enumeration(&["literal", "regex"]))]);
    let structural = run_with(
        DialectId::Hermes,
        &tools,
        text,
        GenerationStop::ToolCalls,
        &Policies::all(RepairPolicy::Structural),
    );
    assert_eq!(
        one(&structural).outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::UnknownParameter {
                pointer: "/mdoe".into()
            }
        }
    );
    let tolerant = run(DialectId::Hermes, &tools, text);
    let call = one(&tolerant);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert_eq!(
        call.arguments().to_json(),
        r#"{"query":"a","mode":"regex"}"#
    );
    let text = r#"<tool_call>{"name": "f", "arguments": {"query": "a", "zzzzzz": 1}}</tool_call>"#;
    let turn = run_with(
        DialectId::Hermes,
        &tools,
        text,
        GenerationStop::ToolCalls,
        &Policies::all(RepairPolicy::Structural),
    );
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::BestGuess);
    assert_eq!(call.arguments().to_json(), r#"{"query":"a"}"#);
    assert_eq!(rule_ids(call), ["tool_call.parameter.drop-unknown"]);
}

#[test]
fn fenced_example_is_guess() {
    let call_text = qwen("f", "<parameter=a>\nx\n</parameter>");
    for text in [
        format!("For example:\n```xml\n{call_text}\n```\n"),
        format!("Use `{call_text}` to call it."),
        format!("> {call_text}\n"),
    ] {
        for (effects, expected) in [
            (Effects::ReadOnly, "best-guess"),
            (Effects::Mutating, "unresolved"),
        ] {
            let tools = compile([ToolDefinition::new("f")
                .effects(effects)
                .param("a", Param::string())]);
            let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
            let call = one(&turn);
            assert_eq!(count(call, "tool_call.context.fenced-example"), 1, "{text}");
            let actual = match call.outcome {
                CallOutcome::BestGuess => "best-guess",
                CallOutcome::Unresolved { .. } => "unresolved",
                _ => "other",
            };
            assert_eq!(actual, expected, "{effects:?} {text}");
            assert!(call.repairs().guessed());
        }
    }
    // The same call outside any fence is resolved, and a fence inside an
    // argument value never makes later calls examples.
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("a", Param::string())]);
    let text = format!(
        "{}\n{call_text}",
        qwen("f", "<parameter=a>\n```python\nunclosed\n</parameter>")
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    assert_eq!(turn.calls().len(), 2);
    for call in turn.calls() {
        assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    }
}

#[test]
fn confusable_terminal_repaired_with_cost() {
    let mut governor = Governor::new(Budget::default());
    let grammar = Grammar::compile_governed(
        r#"grammar "confusable-test.v1";
           confusable "｜" "|";
           call = "<c>" name:/[a-z_]+/ "<｜end｜>";
           result = "<r>" content:until("</r>") "</r>";
           declaration = "<d>";"#,
        &mut governor,
    )
    .expect("grammar");
    let dialect = DialectId::custom(grammar);
    let tools = compile([ToolDefinition::new("ls")]);
    let turn = run(dialect, &tools, "<c>ls<|end|>");
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(rule_ids(call), ["tool_call.markup.terminal-confusable"]);
    let rule = rules::of_operator(EditOperator::TerminalConfusable);
    assert_eq!(call.reading.cost, rule.cost);
    assert!(rule.cost > 0);
    assert_eq!(call.repairs().losses[0].kind, LossKind::Markup);
    assert_eq!(call.repairs().losses[0].loss.original, "<|end|>");
}

#[test]
fn per_call_alternatives_exposed() {
    let tools = compile([
        ToolDefinition::new("get_data").effects(Effects::ReadOnly),
        ToolDefinition::new("get_date").effects(Effects::ReadOnly),
    ]);
    let text = r#"<tool_call>{"name": "get_dat", "arguments": {}}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Ambiguous, "{call:#?}");
    let mut tools_named: Vec<&str> = core::iter::once(call.tool())
        .chain(
            call.alternatives
                .iter()
                .map(|reading| reading.tool.as_deref()),
        )
        .flatten()
        .collect();
    tools_named.sort_unstable();
    assert_eq!(tools_named, ["get_data", "get_date"]);
    assert!(!call.distinguishing.is_empty(), "{call:#?}");
    // Independent ambiguous calls stay per call: two calls, one turn reading.
    let doubled = format!("{text}\n{text}");
    let turn = run(DialectId::Hermes, &tools, &doubled);
    assert_eq!(turn.calls().len(), 2);
    assert_eq!(turn.pieces.len(), 3);
}

fn data_mode_tool(data: purrassist_core::tool_call::ParameterSpec) -> ToolSet {
    compile([ToolDefinition::new("store")
        .param("data", data.required())
        .param("mode", Param::string())])
}

#[test]
fn missing_parameter_close_typed_structural() {
    let tools = data_mode_tool(Param::object(
        ObjectSpec::new().param("a", Param::integer()),
    ));
    let text = qwen(
        "store",
        "<parameter=data>{\"a\":1}<parameter=mode>fast</parameter>",
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(call.reading.effects, Some(Effects::Mutating));
    assert_eq!(call.reading.policy, Some(RepairPolicy::Structural));
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments().to_json(),
        r#"{"data":{"a":1},"mode":"fast"}"#
    );
    assert_eq!(count(call, "tool_call.markup.missing-parameter-close"), 1);
}

#[test]
fn missing_parameter_close_string_structural() {
    let tools = data_mode_tool(Param::string());
    let text = qwen(
        "store",
        "<parameter=data>{\"a\":1}<parameter=mode>fast</parameter>",
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(
        call.arguments().get("data").and_then(|node| node.as_str()),
        Some("{\"a\":1}")
    );
    let close: Vec<&Evidence> = call
        .repairs()
        .evidence
        .iter()
        .filter(|evidence| evidence.rule == "tool_call.markup.missing-parameter-close")
        .collect();
    assert_eq!(close.len(), 1);
    assert!(
        close[0].explanation.contains("alternative reading"),
        "{}",
        close[0].explanation
    );
    assert!(
        call.alternatives.is_empty(),
        "the alternative is never offered"
    );
}

#[test]
fn undeclared_opener_is_not_boundary() {
    let tools = data_mode_tool(Param::string());
    let text = qwen("store", "<parameter=data>x<parameter=zzz>y</parameter>");
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert_eq!(
        call.arguments().to_json(),
        r#"{"data":"x<parameter=zzz>y"}"#
    );
}

#[test]
fn incomplete_typed_value_at_boundary_unresolved() {
    let tools = data_mode_tool(Param::object(
        ObjectSpec::new().param("a", Param::integer()),
    ));
    let text = qwen(
        "store",
        "<parameter=data>{\"a\":<parameter=mode>fast</parameter>",
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    assert!(turn.calls().is_empty(), "{:#?}", turn.pieces);
    assert!(turn.selected().iter().any(|segment| matches!(
        segment,
        ResolvedSegment::Rejected {
            reason: Rejection::Malformed { .. },
            ..
        }
    )));
}

fn required_tool() -> ToolSet {
    compile([ToolDefinition::new("f")
        .param("a", Param::string().required())
        .param("b", Param::string())])
}

#[test]
fn missing_closers_at_end_complete_call_under_stop() {
    let tools = required_tool();
    for stop in [GenerationStop::Stop, GenerationStop::Unknown] {
        let text = "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n";
        let turn = run_with(
            DialectId::Qwen3CoderXml,
            &tools,
            text,
            stop.clone(),
            &Policies::default(),
        );
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{stop:?}: {call:#?}");
        assert_eq!(count(call, "tool_call.markup.missing-closers-at-end"), 2);
        assert_eq!(call.arguments().to_json(), r#"{"a":"x"}"#);
    }
    // A typed value complete at the end is complete.
    let tools = compile([ToolDefinition::new("g").param("n", Param::integer().required())]);
    let text = "<tool_call>\n<function=g>\n<parameter=n>\n42";
    let turn = run_with(
        DialectId::Qwen3CoderXml,
        &tools,
        text,
        GenerationStop::Stop,
        &Policies::default(),
    );
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(count(call, "tool_call.markup.missing-closers-at-end"), 3);
}

#[test]
fn incomplete_call_under_stop_is_malformed() {
    let tools = required_tool();
    // A required parameter missing at the end of output.
    let text = "<tool_call>\n<function=f>\n<parameter=b>\nx\n</parameter>\n";
    let turn = run_with(
        DialectId::Qwen3CoderXml,
        &tools,
        text,
        GenerationStop::Stop,
        &Policies::default(),
    );
    assert!(
        matches!(&one(&turn).outcome, CallOutcome::Unresolved { reason: UnresolvedReason::Malformed { detail } } if detail.contains("`/a`")),
        "{:#?}",
        turn.pieces
    );
    // A string value still open, and a name never finished: malformed
    // under a stop, truncation under a length stop.
    for text in [
        "<tool_call>\n<function=f>\n<parameter=a>\nhello",
        "<tool_call>\n<function=re",
    ] {
        let turn = run_with(
            DialectId::Qwen3CoderXml,
            &tools,
            text,
            GenerationStop::Stop,
            &Policies::default(),
        );
        assert!(turn.calls().is_empty(), "{text}: {:#?}", turn.pieces);
        let Some(ResolvedSegment::Rejected {
            reason, repairs, ..
        }) = turn.selected().last().copied()
        else {
            panic!("{text}: a rejected attempt: {:#?}", turn.pieces);
        };
        assert!(
            matches!(
                reason,
                Rejection::Malformed { .. } | Rejection::Unterminated
            ),
            "{text}: {reason:?}"
        );
        if *reason == Rejection::Unterminated {
            assert_eq!(repairs.losses[0].kind, LossKind::Data);
            assert_eq!(repairs.evidence[0].rule, "tool_call.markup.dangling-opener");
        }
        let turn = run_with(
            DialectId::Qwen3CoderXml,
            &tools,
            text,
            GenerationStop::Length,
            &Policies::default(),
        );
        let call = one(&turn);
        assert_eq!(
            call.outcome,
            CallOutcome::Unresolved {
                reason: UnresolvedReason::Truncated
            },
            "{text}"
        );
        assert_eq!(count(call, "tool_call.truncated"), 1);
        assert!(call.repairs().data_loss());
    }
}

#[test]
fn data_loss_demotes_to_best_guess() {
    for effects in [Effects::ReadOnly, Effects::Mutating] {
        let tools = compile([ToolDefinition::new("f")
            .effects(effects)
            .unknown_parameters(UnknownParameters::DropWithLoss)
            .param("path", Param::string())]);
        let text =
            r#"<tool_call>{"name": "f", "arguments": {"path": "a", "verbose": true}}</tool_call>"#;
        let turn = run(DialectId::Hermes, &tools, text);
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::BestGuess, "{effects:?}");
        assert_eq!(call.arguments().to_json(), r#"{"path":"a"}"#);
        let loss = &call.repairs().losses[0];
        assert_eq!(loss.kind, LossKind::Data);
        assert_eq!(loss.loss.original, r#""verbose":true"#);
    }
    // Dropped prose inside a call's markup is data too; a fence marker is
    // markup.
    let tools = compile([ToolDefinition::new("f")
        .effects(Effects::ReadOnly)
        .param("path", Param::string())]);
    let text = "<tool_call>\n```json\n{\"name\": \"f\", \"arguments\": {\"path\": \"a\"}}\n```\n</tool_call>";
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(count(call, "tool_call.extract.fence"), 2);
    let text = "<tool_call>Sure, here it is: {\"name\": \"f\", \"arguments\": {\"path\": \"a\"}}</tool_call>";
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::BestGuess, "{call:#?}");
    assert_eq!(count(call, "tool_call.extract.prose"), 1);
    assert_eq!(call.repairs().losses[0].loss.original, "Sure, here it is: ");
}

#[test]
fn markup_loss_does_not_demote() {
    let text = "<function=f></parameters></parameters></function>";
    let text = format!("<tool_call>{text}</tool_call>");
    let tools = compile([ToolDefinition::new("f").param("verbose", Param::boolean())]);
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(call.reading.policy, Some(RepairPolicy::Structural));
    assert_eq!(call.reading.effects, Some(Effects::Mutating));
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.tool(), Some("f"));
    assert_eq!(call.arguments().to_json(), "{}");
    assert_eq!(call.repairs().losses.len(), 1);
    let loss = &call.repairs().losses[0];
    assert_eq!(
        (
            loss.kind,
            loss.loss.evidence.rule.as_str(),
            loss.loss.original.as_str()
        ),
        (
            LossKind::Markup,
            "tool_call.markup.closers-only-empty",
            "</parameters></parameters>"
        )
    );
    // With a required parameter the same body is not resolved, and the loss
    // is still recorded.
    let tools = compile([ToolDefinition::new("f").param("path", Param::string().required())]);
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::MissingRequired {
                pointer: "/path".into()
            }
        }
    );
    assert_eq!(count(call, "tool_call.markup.closers-only-empty"), 1);
}

#[test]
fn nonverbatim_value_trim_recorded() {
    let text = "<tool_call><function=f><parameter=n>\n  5\n</parameter></function></tool_call>";
    let tools = compile([ToolDefinition::new("f").param("n", Param::integer())]);
    let turn = run(DialectId::Qwen3CoderXml, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.arguments().to_json(), r#"{"n":5}"#);
    let trims: Vec<&Evidence> = call
        .repairs()
        .evidence
        .iter()
        .filter(|evidence| evidence.rule == "tool_call.value.trim-nonverbatim")
        .collect();
    assert_eq!(trims.len(), 1);
    let at = text.find("\n  5\n").expect("value");
    assert_eq!(
        trims[0].span.as_ref().expect("span").bytes,
        at..at + "\n  5\n".len()
    );
    // A declared string keeps its spaces after the framing newlines.
    let tools = compile([ToolDefinition::new("f").param("n", Param::string())]);
    let turn = run(DialectId::Qwen3CoderXml, &tools, text);
    let call = one(&turn);
    assert_eq!(call.arguments().to_json(), r#"{"n":"  5"}"#);
    assert_eq!(count(call, "tool_call.value.trim-nonverbatim"), 0);
    assert_eq!(count(call, "tool_call.xml.framing-newline"), 2);
    // Enumeration values are trimmed and normalized, under Structural too.
    let tools = compile([
        ToolDefinition::new("f").param("unit", Param::enumeration(&["celsius", "fahrenheit"]))
    ]);
    let text = qwen("f", "<parameter=unit>\n Celsius \n</parameter>");
    let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved, "{call:#?}");
    assert_eq!(call.arguments().to_json(), r#"{"unit":"celsius"}"#);
    assert_eq!(count(call, "tool_call.value.trim-nonverbatim"), 1);
    assert_eq!(count(call, "tool_call.value.enum-normalize"), 1);
}

#[test]
fn repeated_closer_after_call_is_markup_not_content() {
    let tools = compile([ToolDefinition::new("f").param("a", Param::string())]);
    for text in [
        "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function></function></tool_call>",
        "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call></tool_call>",
        "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call></tool_call></tool_calls>",
    ] {
        let turn = run(DialectId::Qwen3CoderXml, &tools, text);
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{text}: {call:#?}");
        assert!(count(call, "tool_call.markup.stray-closer") >= 1, "{text}");
        assert!(
            call.repairs()
                .losses
                .iter()
                .all(|loss| loss.kind == LossKind::Markup)
        );
        for segment in turn.selected() {
            assert!(
                matches!(segment, ResolvedSegment::Call(_)),
                "{text}: no closer leaks into content: {segment:?}"
            );
        }
        assert_eq!(call.span, 0..text.len());
    }
    // Hermes too.
    let text = r#"<tool_call>{"name": "f", "arguments": {"a": "x"}}</tool_call></tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.span, 0..text.len());
    assert_eq!(count(call, "tool_call.markup.stray-closer"), 1);
}

fn openai_tools() -> ToolSet {
    compile([ToolDefinition::new("get_weather")
        .effects(Effects::ReadOnly)
        .param("city", Param::string().required())])
}

#[test]
fn openai_member_order_independent() {
    let tools = openai_tools();
    let arguments = r#""{\"city\": \"Lima\"}""#;
    let elements = [
        format!(
            r#"{{"id": "c1", "type": "function", "function": {{"name": "get_weather", "arguments": {arguments}}}}}"#
        ),
        format!(
            r#"{{"type": "function", "id": "c1", "function": {{"arguments": {arguments}, "name": "get_weather"}}}}"#
        ),
        format!(
            r#"{{"function": {{"name": "get_weather", "arguments": {arguments}}}, "type": "function", "id": "c1"}}"#
        ),
        format!(
            r#"{{"function": {{"arguments": {arguments}, "name": "get_weather"}}, "id": "c1"}}"#
        ),
        format!(
            r#"{{"id": "c1", "function": {{"name": "get_weather", "arguments": {arguments}}}, "type":"function"}}"#
        ),
        format!(
            "{{\n  \"type\": \"function\",\n  \"function\": {{\n    \"name\": \"get_weather\",\n    \"arguments\": {arguments}\n  }},\n  \"id\": \"c1\"\n}}"
        ),
    ];
    for element in &elements {
        let turn = run(DialectId::OpenAiToolCalls, &tools, element);
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{element}: {call:#?}");
        assert_eq!(call.tool(), Some("get_weather"));
        assert_eq!(call.arguments().to_json(), r#"{"city":"Lima"}"#);
        assert_eq!(call.id.as_ref().map(|id| id.value.as_str()), Some("c1"));
        assert_eq!(call.span, 0..element.len());
    }
    // A `type` other than "function" never opens a call.
    let text = r#"{"type": "text", "function": {"name": "get_weather", "arguments": "{}"}}"#;
    let turn = run(DialectId::OpenAiToolCalls, &tools, text);
    assert!(turn.calls().is_empty(), "{:#?}", turn.pieces);
    assert!(
        turn.selected()
            .iter()
            .all(|segment| matches!(segment, ResolvedSegment::Content(_)))
    );
}

#[test]
fn openai_missing_id_still_a_call() {
    let tools = openai_tools();
    for text in [
        r#"{"type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"Oslo\"}"}}"#,
        r#"{"function": {"name": "get_weather", "arguments": "{\"city\": \"Oslo\"}"}}"#,
        r#"[{"function": {"arguments": "{\"city\": \"Oslo\"}", "name": "get_weather"}, "type": "function"}]"#,
    ] {
        let turn = run(DialectId::OpenAiToolCalls, &tools, text);
        let call = one(&turn);
        assert_eq!(call.id, None, "{text}");
        assert_eq!(call.outcome, CallOutcome::Resolved, "{text}: {call:#?}");
        assert_eq!(call.arguments().to_json(), r#"{"city":"Oslo"}"#);
    }
}

#[test]
fn schema_validation_failure_unresolved_with_diagnostics() {
    let tools = ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"set_level","parameters":{"type":"object","properties":{"level":{"type":"integer","minimum":10}},"required":["level"]}}}]"#,
        Budget::default(),
    )
    .expect("adapter tools");
    let text = r#"<tool_call>{"name": "set_level", "arguments": {"level": 5}}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(
        call.outcome,
        CallOutcome::Unresolved {
            reason: UnresolvedReason::SchemaValidation
        }
    );
    let validation = &call.reading.validations[0];
    assert!(!validation.passed);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("/level")),
        "{validation:#?}"
    );
    // No schema-guided invention: the value is unchanged.
    assert_eq!(call.arguments().to_json(), r#"{"level":5}"#);
    // A valid value passes the same exact schema.
    let text = r#"<tool_call>{"name": "set_level", "arguments": {"level": 12}}</tool_call>"#;
    let turn = run(DialectId::Hermes, &tools, text);
    let call = one(&turn);
    assert_eq!(call.outcome, CallOutcome::Resolved);
    assert!(call.reading.validations[0].passed);
}

#[test]
fn policy_selection_all_overrides_per_tool() {
    let text = r#"<tool_call>{"name": "read_fiel", "arguments": {"path": "a"}}</tool_call>"#;
    let read_only = two_tools(Effects::ReadOnly);
    let mutating = two_tools(Effects::Mutating);
    let outcome = |tools: &ToolSet, policies: &Policies| {
        let turn = run_with(
            DialectId::Hermes,
            tools,
            text,
            GenerationStop::ToolCalls,
            policies,
        );
        one(&turn).outcome.clone()
    };
    assert_eq!(
        outcome(&read_only, &Policies::default()),
        CallOutcome::BestGuess
    );
    assert!(is_unresolved(&outcome(
        &read_only,
        &Policies::all(RepairPolicy::Structural)
    )));
    assert!(is_unresolved(&outcome(&mutating, &Policies::default())));
    assert_eq!(
        outcome(&mutating, &Policies::all(RepairPolicy::Tolerant)),
        CallOutcome::BestGuess
    );
    // A per-tool effects override selects that tool's policy; an explicit
    // single policy still wins.
    let mut policies = Policies::default();
    policies
        .effects
        .insert("read_file".into(), Effects::ReadOnly);
    policies
        .effects
        .insert("get_weather".into(), Effects::ReadOnly);
    assert_eq!(outcome(&mutating, &policies), CallOutcome::BestGuess);
    policies.selection = PolicySelection::All(RepairPolicy::Structural);
    assert!(is_unresolved(&outcome(&mutating, &policies)));
    // Adapter tools default to Mutating and so Structural.
    let adapted = ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"read_file","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}]"#,
        Budget::default(),
    )
    .expect("adapter tools");
    assert!(is_unresolved(&outcome(&adapted, &Policies::default())));
    let mut policies = Policies::default();
    policies
        .effects
        .insert("read_file".into(), Effects::ReadOnly);
    assert_eq!(outcome(&adapted, &policies), CallOutcome::BestGuess);
}

#[test]
fn effects_override_unknown_tool_is_error() {
    let tools = two_tools(Effects::Mutating);
    let turn = recognize(
        &set(&[DialectId::Hermes]),
        &tools,
        "plain text",
        GenerationStop::Stop,
    );
    let mut policies = Policies::default();
    policies
        .effects
        .insert("delete_everything".into(), Effects::ReadOnly);
    let result = resolve_forest(
        &tools,
        &turn,
        &policies,
        &mut Governor::new(Budget::default()),
    );
    assert!(
        matches!(&result, Err(StopReason::InvalidInput(reason)) if reason.contains("`delete_everything`")),
        "{result:?}"
    );
    // An alias is not a canonical tool name.
    let tools = compile([ToolDefinition::new("read_file").alias("cat")]);
    let mut policies = Policies::default();
    policies.effects.insert("cat".into(), Effects::ReadOnly);
    assert!(policies.validate(&tools).is_err());
    policies.effects.clear();
    policies
        .effects
        .insert("read_file".into(), Effects::ReadOnly);
    assert!(policies.validate(&tools).is_ok());
}

#[test]
fn resolution_independent_of_tool_and_dialect_order() {
    let definitions = || {
        vec![
            ToolDefinition::new("read_file")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required())
                .param("limit", Param::integer()),
            ToolDefinition::new("get_weather")
                .effects(Effects::ReadOnly)
                .param("city", Param::string().required())
                .param("unit", Param::enumeration(&["celsius", "fahrenheit"])),
            ToolDefinition::new("get_date").effects(Effects::ReadOnly),
            ToolDefinition::new("get_data").effects(Effects::ReadOnly),
        ]
    };
    let forward = compile(definitions());
    let backward = compile(definitions().into_iter().rev());
    let text = concat!(
        "Two calls.\n<tool_call>\n{\"name\": \"get_wether\", \"arguments\": {\"city\": \"Oslo\", \"unit\": \"Celsius\"}}\n</tool_call>\n",
        "<tool_call>\n<function=read_file>\n<parameter=path>\na\n</parameter>\n<parameter=limit>\n 3 \n</parameter>\n</function>\n</tool_call>\n",
        "<tool_call>{\"name\": \"get_dat\", \"arguments\": {}}</tool_call>",
    );
    let mut results = Vec::new();
    for tools in [&forward, &backward] {
        for dialects in [
            [DialectId::Hermes, DialectId::Qwen3CoderXml],
            [DialectId::Qwen3CoderXml, DialectId::Hermes],
        ] {
            let dialects = set(&dialects);
            let turn = recognize(&dialects, tools, text, GenerationStop::ToolCalls);
            let mut governor = Governor::new(Budget::default());
            let resolved = resolve_forest(tools, &turn, &Policies::default(), &mut governor)
                .expect("resolved");
            results.push((resolved, governor.usage()));
        }
    }
    for other in &results[1..] {
        assert_eq!(other, &results[0]);
    }
    let calls = results[0].0.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].outcome, CallOutcome::BestGuess);
    assert_eq!(
        calls[0].arguments().to_json(),
        r#"{"city":"Oslo","unit":"celsius"}"#
    );
    assert_eq!(calls[1].outcome, CallOutcome::Resolved);
    assert_eq!(calls[1].arguments().to_json(), r#"{"path":"a","limit":3}"#);
    assert_eq!(calls[2].outcome, CallOutcome::Ambiguous);
}

/// Qwen calls followed by closers separated by whitespace.
const SEPARATED: [&str; 3] = [
    "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>\n</tool_call>",
    "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</function>\n</tool_call>",
    "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>\r\n \t</tool_calls>\n\nThanks.\n",
];

#[test]
fn stray_closers_separated_by_newlines_absorbed() {
    let tools = compile([ToolDefinition::new("f").param("a", Param::string())]);
    for text in SEPARATED {
        let turn = run(DialectId::Qwen3CoderXml, &tools, text);
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{text}: {call:#?}");
        assert_eq!(call.arguments().to_json(), r#"{"a":"x"}"#);
        let strays: Vec<&CallLoss> = call
            .repairs()
            .losses
            .iter()
            .filter(|loss| loss.loss.evidence.rule == "tool_call.markup.stray-closer")
            .collect();
        assert_eq!(strays.len(), 1, "{text}: {call:#?}");
        assert_eq!(strays[0].kind, LossKind::Markup);
        // The deleted span is exact: any whitespace before the closer and
        // the closer itself.
        let span = strays[0]
            .loss
            .evidence
            .span
            .as_ref()
            .expect("span")
            .bytes
            .clone();
        assert_eq!(strays[0].loss.original, &text[span]);
        assert!(strays[0].loss.original.trim_start().starts_with("</"));
        assert!(!rule_ids(call).contains(&"tool_call.markup.missing-wrapper-close"));
        // The call runs through the last closer.
        let last = text.rfind("</tool_call").expect("closer");
        let end = last + text[last..].find('>').expect("closer end") + 1;
        assert_eq!(call.span.end, end, "{text}");
        for segment in turn.selected() {
            if let ResolvedSegment::Content(content) = segment {
                assert!(
                    !text[content.clone()].contains("</"),
                    "{text}: closer leaked"
                );
            }
        }
    }
    // A gap longer than the bound is content.
    let long = format!(
        "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>{}</tool_call>",
        " ".repeat(purrassist_core::grammar::cost::STRAY_CLOSER_GAP + 1)
    );
    let turn = run(DialectId::Qwen3CoderXml, &tools, &long);
    let call = one(&turn);
    assert_eq!(count(call, "tool_call.markup.stray-closer"), 0);
    assert!(matches!(
        turn.selected().last(),
        Some(ResolvedSegment::Content(_))
    ));
}

#[test]
fn whitespace_after_last_closer_is_content() {
    let tools = compile([ToolDefinition::new("f").param("a", Param::string())]);
    let call_text =
        "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>";
    for tail in [
        "\n",
        "\n</tool_call>\n  ",
        "\n</tool_call>\n<tool_call>",
        "\n\n</think>",
    ] {
        let text = format!("{call_text}{tail}");
        let turn = run(DialectId::Qwen3CoderXml, &tools, &text);
        let call = one(&turn);
        assert_eq!(call.outcome, CallOutcome::Resolved, "{text:?}");
        let absorbed = tail.matches("</tool_call>").count();
        assert_eq!(
            count(call, "tool_call.markup.stray-closer"),
            absorbed,
            "{text:?}"
        );
        let end = text.find(call_text).expect("call")
            + call_text.len()
            + if absorbed > 0 {
                "\n</tool_call>".len()
            } else {
                0
            };
        assert_eq!(call.span.end, end, "{text:?}");
        let rest: Vec<&ResolvedSegment> = turn
            .selected()
            .into_iter()
            .filter(|segment| segment.span().start >= end)
            .collect();
        assert!(!rest.is_empty(), "{text:?}");
        assert_eq!(
            rest[0].span().start,
            end,
            "{text:?}: trailing bytes are not absorbed"
        );
    }
}

#[test]
fn stray_closer_absorption_is_split_invariant() {
    let tools = compile([ToolDefinition::new("f").param("a", Param::string())]);
    let dialects = set(&[DialectId::Qwen3CoderXml, DialectId::Hermes]);
    let run_split = |text: &str, cuts: &[usize]| {
        let mut governor = Governor::new(Budget::default());
        let mut recognizer =
            Recognizer::with_tools(&dialects, &tools, ReasoningPolicy::default(), &mut governor)
                .expect("recognizer");
        let mut from = 0;
        for &cut in cuts.iter().chain(core::iter::once(&text.len())) {
            recognizer
                .push(&text.as_bytes()[from..cut], &mut governor)
                .expect("push");
            from = cut;
        }
        let turn = recognizer
            .finish(GenerationStop::Stop, &mut governor)
            .expect("finish");
        let resolved =
            resolve_forest(&tools, &turn, &Policies::default(), &mut governor).expect("resolved");
        (turn, resolved, governor.usage())
    };
    let mut inputs: Vec<String> = SEPARATED.iter().map(|text| (*text).to_owned()).collect();
    inputs.push(
        "<tool_call>\n<function=f>\n<parameter=a>\nx\n</parameter>\n</function>\n</tool_call>\n</tool_call>\n  "
            .into(),
    );
    for text in &inputs {
        let whole = run_split(text, &[]);
        for cut in 1..text.len() {
            let split = run_split(text, &[cut]);
            assert_eq!(split.0, whole.0, "{text:?} split at {cut}");
            assert_eq!(split.1, whole.1, "{text:?} split at {cut}");
            assert_eq!(split.2, whole.2, "{text:?} split at {cut}");
        }
        let bytes: Vec<usize> = (1..text.len()).collect();
        let bytewise = run_split(text, &bytes);
        assert_eq!(bytewise.1, whole.1, "{text:?} byte by byte");
        assert_eq!(bytewise.2, whole.2, "{text:?} byte by byte");
    }
}

/// The parsing/record/repair classification documented in `rules.rs`: a
/// rule is a repair exactly when it costs something or guesses, and these
/// are every other rule.
#[test]
fn repair_classification_matches_documentation() {
    let not_repairs: Vec<&str> = RULES
        .iter()
        .filter(|rule| !rule.is_repair())
        .map(|rule| rule.id)
        .collect();
    let mut documented = vec![
        // Parsing.
        "tool_call.markup.nested-parameter",
        "tool_call.markup.json-body",
        "tool_call.xml.array-item",
        "tool_call.value.trim-nonverbatim",
        "tool_call.xml.framing-newline",
        "tool_call.arguments.decode-string",
        "tool_call.name.alias",
        "tool_call.parameter.alias",
        "tool_call.parameter.pass-through",
        "tool_call.parameter.collect",
        "tool_call.value.decode-typed",
        // Records.
        "tool_call.reasoning.unterminated",
        "tool_call.truncated",
        "tool_call.markup.dangling-opener",
        "tool_call.id.dedup",
        "tool_call.input.invalid-utf8",
    ];
    let mut actual = not_repairs.clone();
    actual.sort_unstable();
    documented.sort_unstable();
    assert_eq!(actual, documented);
    for rule in RULES {
        assert_eq!(rule.is_repair(), rule.cost > 0 || rule.guess, "{}", rule.id);
    }
}

/// `docs/tool-calls/RULES.md`, whose tables `rules_doc_matches_registry`
/// checks against the registry.
const RULES_DOC: &str = include_str!("../../../docs/tool-calls/RULES.md");

/// `docs/tool-calls/GRAMMAR.md`, whose custom-dialect walkthrough
/// `grammar_doc_example_compiles` runs.
const GRAMMAR_DOC: &str = include_str!("../../../docs/tool-calls/GRAMMAR.md");

/// The table rows of the `heading` section of `doc` (every line whose first
/// cell is backticked), split into trimmed cells.
fn doc_rows<'d>(doc: &'d str, heading: &str) -> Vec<Vec<&'d str>> {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("the `{heading}` section"));
    let section = &doc[start + heading.len()..];
    let section = &section[..section.find("\n## ").unwrap_or(section.len())];
    section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            line.trim_start_matches('|')
                .trim_end_matches('|')
                .split(" | ")
                .map(str::trim)
                .collect()
        })
        .collect()
}

/// The checked columns of one registry rule, spelled as `RULES.md` spells
/// them: id, cost, guess, readings, grounding, loss, policies, `Exact`
/// treatment, and source.
fn registry_row(rule: &rules::Rule) -> Vec<String> {
    let readings = match rule.readings {
        Readings::Single => "single",
        Readings::Multiple => "multiple",
    };
    let grounding = match rule.grounding {
        Grounding::Definition => "definition",
        Grounding::Syntax => "syntax",
    };
    let loss = match rule.loss {
        None => "none",
        Some(LossKind::Markup) => "markup",
        Some(LossKind::Data) => "data",
    };
    let policies: Vec<String> = rule
        .policies
        .iter()
        .map(|policy| format!("`{policy:?}`"))
        .collect();
    vec![
        format!("`{}`", rule.id),
        rule.cost.to_string(),
        if rule.guess { "yes" } else { "no" }.to_owned(),
        readings.to_owned(),
        grounding.to_owned(),
        loss.to_owned(),
        policies.join(", "),
        if rule.is_repair() {
            "refused"
        } else {
            "applied"
        }
        .to_owned(),
        rule.operator.map_or_else(
            || "resolution".to_owned(),
            |operator| format!("`{operator:?}`"),
        ),
    ]
}

/// `docs/tool-calls/RULES.md` lists exactly the registry, in registry order,
/// with every field equal to the code's, plus a non-empty meaning per rule;
/// and exactly the disabled rules, none of which is in the registry.
#[test]
fn rules_doc_matches_registry() {
    let expected: Vec<Vec<String>> = RULES.iter().map(registry_row).collect();
    let documented: Vec<Vec<String>> = doc_rows(RULES_DOC, "## Registry")
        .into_iter()
        .map(|row| {
            assert_eq!(row.len(), 10, "ten cells per registry row: {row:?}");
            assert!(!row[9].is_empty(), "every rule has a meaning: {row:?}");
            row[..9].iter().map(|cell| (*cell).to_owned()).collect()
        })
        .collect();
    let regenerated: Vec<String> = expected
        .iter()
        .map(|row| format!("| {} | MEANING |", row.join(" | ")))
        .collect();
    assert_eq!(
        documented,
        expected,
        "docs/tool-calls/RULES.md must list every registry rule in order; expected rows:\n{}",
        regenerated.join("\n")
    );
    let disabled: Vec<&str> = doc_rows(RULES_DOC, "## Disabled rules")
        .into_iter()
        .map(|row| {
            assert_eq!(row.len(), 2, "id and reason per disabled row: {row:?}");
            assert!(!row[1].is_empty(), "every disabled rule has a reason");
            row[0]
        })
        .collect();
    let listed: Vec<String> = DISABLED.iter().map(|id| format!("`{id}`")).collect();
    assert_eq!(disabled, listed);
    for id in DISABLED {
        assert!(rules::rule(id).is_none(), "`{id}` is never a registry rule");
    }
}

/// The body of the first fenced block after `marker` in `doc`.
fn fenced_after<'d>(doc: &'d str, marker: &str) -> &'d str {
    let start = doc
        .find(marker)
        .unwrap_or_else(|| panic!("the `{marker}` marker"));
    let rest = &doc[start + marker.len()..];
    let rest = &rest[rest.find("```").expect("a fence after the marker")..];
    let body = &rest[rest.find('\n').expect("the opening fence line") + 1..];
    &body[..body.find("\n```").expect("the closing fence")]
}

/// The custom-dialect walkthrough of `docs/tool-calls/GRAMMAR.md` holds: its
/// grammar compiles, its output recovers to the documented call with the
/// documented repairs under every policy, and printing the call gives the
/// documented canonical form, which recognizes with no repair.
#[test]
fn grammar_doc_example_compiles() {
    use purrassist_core::tool_call::{Emission, EmitTarget, IdPolicy, ToolRecoveryRequest};
    use purrassist_core::{Effort, Outcome};

    let source = fenced_after(GRAMMAR_DOC, "<!-- custom-dialect-grammar -->");
    let output = fenced_after(GRAMMAR_DOC, "<!-- custom-dialect-output -->");
    let printed = fenced_after(GRAMMAR_DOC, "<!-- custom-dialect-emitted -->");
    let mut governor = Governor::new(Budget::default());
    let grammar = Grammar::compile_governed(source, &mut governor)
        .unwrap_or_else(|error| panic!("the documented grammar compiles: {error}"));
    assert_eq!(grammar.id(), "acme-call.v1");
    let acme = DialectId::custom(grammar);
    let dialects = DialectSet::new([acme.clone()], &mut governor).expect("dialect set");
    let tools = compile([ToolDefinition::new("read_file")
        .effects(Effects::ReadOnly)
        .param("path", Param::string().required())]);
    for policy in [
        PolicySelection::PerTool,
        PolicySelection::All(RepairPolicy::Structural),
        PolicySelection::All(RepairPolicy::Tolerant),
    ] {
        let request = ToolRecoveryRequest::new(Effort::Standard)
            .dialects(dialects.clone())
            .ids(IdPolicy::Source)
            .policy(policy)
            .emit(EmitTarget::Dialect(acme.clone()));
        let result = tools.recover(output.as_bytes(), &request);
        let Outcome::Resolved(calls) = &result.outcome else {
            panic!("{policy:?}: {:#?}", result.outcome)
        };
        assert_eq!(calls.calls.len(), 1, "{policy:?}");
        let call = &calls.calls[0];
        assert_eq!(call.outcome, CallOutcome::Resolved, "{policy:?}");
        assert_eq!(
            (
                call.id.as_str(),
                call.tool.as_deref(),
                call.arguments.to_json().as_str()
            ),
            ("r1", Some("read_file"), r#"{"path":"src/lib.rs"}"#)
        );
        let mut applied: Vec<&str> = call
            .repairs
            .evidence
            .iter()
            .map(|evidence| evidence.rule.as_str())
            .collect();
        applied.sort_unstable();
        assert_eq!(
            applied,
            [
                "tool_call.markup.closer-variant",
                "tool_call.markup.terminal-confusable",
                "tool_call.markup.terminal-confusable",
            ]
        );
        assert!(
            call.repairs
                .losses
                .iter()
                .all(|loss| loss.kind == LossKind::Markup)
        );
        let prose = "Let me read it.\n".len();
        assert_eq!(
            calls
                .content
                .iter()
                .map(|range| (range.start, range.end))
                .collect::<Vec<_>>(),
            [(0, prose)],
            "the text before the call is content"
        );
        let Some(Emission::Dialect { emitted, .. }) = &calls.emitted else {
            panic!("the call is printed in the custom dialect")
        };
        assert_eq!(emitted.output, printed.as_bytes(), "{policy:?}");

        let again = tools.recover(printed.as_bytes(), &request);
        let Outcome::Resolved(reread) = &again.outcome else {
            panic!("{:#?}", again.outcome)
        };
        assert_eq!(reread.calls.len(), 1);
        assert_eq!(
            reread.calls[0].arguments.to_json(),
            r#"{"path":"src/lib.rs"}"#
        );
        assert!(
            reread.calls[0].repairs.evidence.is_empty(),
            "the canonical form recognizes with no repair"
        );
    }
}
