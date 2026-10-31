// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Marker projections retain source evidence, ambiguity and native class semantics.
use purrassist_core::grammar::GrammarRecipe;
use purrassist_core::grammar::edit::EditOperator;
use purrassist_core::tool_call::{
    DialectId, DialectSet, GenerationStop, MarkupFamily, ReasoningPolicy, RecognizedTurn,
    Recognizer, ToolCalls, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::wire::WireValue;
use purrassist_core::{Budget, Governor, Outcome};

fn tools() -> ToolSet {
    ToolSet::from_openai(
        r#"[{"type":"function","function":{"name":"f","parameters":{"type":"object","properties":{"text":{"type":"string"}}}}}]"#,
        Budget::default(),
    ).expect("tools")
}
fn set(dialects: impl IntoIterator<Item = DialectId>) -> DialectSet {
    DialectSet::new(dialects, &mut Governor::new(Budget::default())).expect("dialects")
}
fn recognize(input: &str, dialect: DialectId) -> (RecognizedTurn, u64) {
    let set = set([dialect]);
    let tools = tools();
    let mut governor = Governor::new(Budget::default());
    let mut recognizer =
        Recognizer::with_tools(&set, &tools, ReasoningPolicy::default(), &mut governor)
            .expect("recognizer");
    recognizer
        .push(input.as_bytes(), &mut governor)
        .expect("push");
    let turn = recognizer
        .finish(GenerationStop::Stop, &mut governor)
        .expect("finish");
    (turn, governor.usage().work)
}
fn recovered(input: &str, set: &DialectSet) -> ToolCalls {
    let result = tools().recover(input.as_bytes(), &ToolRecoveryRequest::default(), set);
    assert_eq!(result.receipt.sources[0].bytes, input.as_bytes());
    match result.outcome {
        Outcome::Resolved(calls) | Outcome::BestGuess(calls) => calls,
        other => panic!("{input:?}: {other:?}"),
    }
}

#[test]
fn stripped_variants_parse() {
    for (dialect, input) in [
        (
            DialectId::DeepSeekV3Stripped,
            "functionf\n```json\n{\"text\":\"deepseek\"}\n```",
        ),
        (
            DialectId::DeepSeekV31Stripped,
            "f{\"text\":\"name-arguments\"}",
        ),
        (
            DialectId::HarmonyStripped,
            "commentary to=functions.f json{\"text\":\"harmony\"}",
        ),
        (
            DialectId::HarmonyStripped,
            "assistant\tto=functions.f\tfinaljson\t{\"text\":\"second\"}",
        ),
        (
            DialectId::HermesStripped,
            "{\"name\":\"f\",\"arguments\":{\"text\":\"hermes\"}}",
        ),
        (
            DialectId::KimiK2Stripped,
            "functions.f:0{\"text\":\"kimi\"}",
        ),
        (
            DialectId::Llama3JsonStripped,
            "{\"name\":\"f\",\"parameters\":{\"text\":\"llama\"}}",
        ),
        (DialectId::MistralV11Stripped, "f{\"text\":\"mistral\"}"),
        (
            DialectId::MistralV3Stripped,
            "[{\"name\":\"f\",\"arguments\":{\"text\":\"array\"},\"id\":\"c1\"}]",
        ),
    ] {
        let calls = recovered(input, &set([dialect.clone()]));
        assert_eq!(calls.calls.len(), 1, "{}: {calls:?}", dialect.id());
        assert_eq!(calls.calls[0].tool.as_deref(), Some("f"));
        assert_eq!(calls.calls[0].dialect, dialect);
    }
}

#[test]
fn weak_names_without_retained_json_evidence_stay_content() {
    for body in [
        r#""\u007b\"text\":\"encoded\"}""#,
        r#""\u0020\n\u007b\"text\":\"encoded\"}""#,
    ] {
        for (dialect, input) in [
            (DialectId::DeepSeekV31Stripped, format!("f{body}")),
            (
                DialectId::BareJson,
                format!(r#"{{"name":"f","arguments":{body}}}"#),
            ),
        ] {
            let calls = recovered(&input, &set([dialect.clone()]));
            assert_eq!(calls.calls.len(), 1, "{}: {calls:?}", dialect.id());
            assert_eq!(
                calls.calls[0]
                    .arguments
                    .get("text")
                    .and_then(purrassist_core::json::Node::as_str),
                Some("encoded")
            );
        }
    }
    for dialect in [
        DialectId::DeepSeekV31Stripped,
        DialectId::KimiK2Stripped,
        DialectId::MistralV11Stripped,
    ] {
        for input in [
            "f",
            "f\"",
            r#"f"\u0020not an object""#,
            "f is an ordinary word.",
            "  f is an ordinary word.\n",
            "functions.f:0 is prose.",
        ] {
            let calls = recovered(input, &set([dialect.clone()]));
            assert!(
                calls.calls.is_empty(),
                "{}: {input:?}: {calls:?}",
                dialect.id()
            );
            assert!(
                calls.rejected.is_empty(),
                "{}: {input:?}: {calls:?}",
                dialect.id()
            );
        }
    }
}

#[test]
fn joint_name_id_preserves_every_admitted_source_boundary_as_a_guess() {
    let input = "ab12{}";
    let (turn, _) = recognize(input, DialectId::MistralV11Stripped);
    let segments = turn.calls();
    assert_eq!(segments.len(), 1);
    let readings = &segments[0].readings;
    assert_eq!(
        readings.len(),
        4,
        "whole name and three nonempty partitions: {readings:?}"
    );
    let mut splits = Vec::new();
    for reading in readings {
        let name = reading.name.as_ref().expect("name");
        assert_eq!(&input[name.span.clone()], name.value);
        if let Some(id) = &reading.id {
            assert_eq!(&input[id.span.clone()], id.value);
            assert_eq!(name.span.end, id.span.start);
            assert_eq!(format!("{}{}", name.value, id.value), "ab12");
            let boundary = reading
                .edits
                .iter()
                .find(|edit| edit.operator == EditOperator::StrippedNameIdBoundary)
                .expect("explicit boundary inference");
            assert!(boundary.rule().guess);
            assert_eq!(boundary.span, id.span.start..id.span.start);
            splits.push((name.value.clone(), id.value.clone()));
        } else {
            assert_eq!(name.value, "ab12");
        }
    }
    splits.sort();
    assert_eq!(
        splits,
        [
            ("a".into(), "b12".into()),
            ("ab".into(), "12".into()),
            ("ab1".into(), "2".into())
        ]
    );
}

#[test]
fn indentation_and_invalid_id_suffixes_have_linear_recognizer_work() {
    for dialect in [
        DialectId::DeepSeekV31Stripped,
        DialectId::MistralV11Stripped,
    ] {
        let mut previous = None;
        for length in [512, 1024, 2048] {
            let input = format!("{}f{{}}", " ".repeat(length));
            let (turn, work) = recognize(&input, dialect.clone());
            assert_eq!(turn.calls().len(), 1, "{} at {length}", dialect.id());
            if let Some(previous) = previous {
                assert!(
                    work < previous * 3,
                    "{}: {previous} -> {work}",
                    dialect.id()
                );
            }
            previous = Some(work);
        }
    }
}

#[test]
fn disallowed_name_arguments_retains_both_distinct_dialect_candidates() {
    assert_eq!(MarkupFamily::NameArgumentsStripped.dialect(), None);
    assert_eq!(
        MarkupFamily::NameArgumentsStripped.dialects(),
        [
            DialectId::DeepSeekV31Stripped,
            DialectId::MistralV11Stripped
        ]
    );
    let calls = recovered("f{}", &set([DialectId::Qwen3CoderXml]));
    assert_eq!(calls.rejected.len(), 1);
    assert_eq!(calls.rejected[0].dialect, None);
    assert_eq!(
        calls.rejected[0].candidate_dialects,
        MarkupFamily::NameArgumentsStripped.dialects()
    );
    for dialect in MarkupFamily::NameArgumentsStripped.dialects() {
        let calls = recovered("f{}", &set([dialect.clone()]));
        assert_eq!(calls.calls.len(), 1);
        assert!(calls.rejected.is_empty());
    }
}

#[test]
fn exact_bare_json_label_precedes_derived_labels_for_the_same_call() {
    let input = "{\"name\":\"f\",\"arguments\":{\"text\":\"retained\"}}";
    let calls = recovered(input, &DialectSet::built_in());
    assert_eq!(calls.calls.len(), 1);
    let call = &calls.calls[0];
    assert!(!call.dialect.grammar().is_derived(), "{call:?}");
    assert!(call.candidate_dialects.contains(&DialectId::HermesStripped));
    let array = format!("[{input}]");
    let result = tools().recover(
        array.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let Outcome::Ambiguous(readings) = result.outcome else {
        panic!("retain differing array ledgers: {result:?}")
    };
    assert!(
        !readings[0].calls[0].dialect.grammar().is_derived(),
        "{readings:?}"
    );
    assert_eq!(readings[0].calls[0].span, 1..array.len() - 1);
    assert_eq!(
        readings[0]
            .content
            .iter()
            .map(|span| &array[span.clone()])
            .collect::<Vec<_>>(),
        ["[", "]"]
    );
    assert!(readings.iter().any(|reading| {
        reading
            .calls
            .iter()
            .any(|call| call.dialect == DialectId::MistralV3Stripped)
    }));
    assert_eq!(result.receipt.alternatives.len(), readings.len());
    for (reading, receipt) in readings.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            receipt.output,
            purrassist_core::digest::Digest::of(reading.to_node().to_json().as_bytes())
        );
    }

    // Retained exact-looking objects inside arguments are data of the outer
    // weak call, even though a separate exact grammar knows their shape.
    let nested = r#"f{"data":{"name":"f","arguments":{"text":"nested"}}}"#;
    let calls = recovered(
        nested,
        &set([DialectId::DeepSeekV31Stripped, DialectId::BareJson]),
    );
    assert_eq!(calls.calls.len(), 1, "{calls:?}");
    assert_eq!(calls.calls[0].dialect, DialectId::DeepSeekV31Stripped);
    assert!(calls.calls[0].arguments.get("data").is_some(), "{calls:?}");
    for input in [
        r#"{"parameters":{"data":{"name":"f","arguments":{"text":"nested"}}},"name":"f"}"#,
        r#"{"parameters":"{\"data\":{\"name\":\"f\",\"arguments\":{\"text\":\"nested\"}}}","name":"f"}"#,
        r#"{"parameters":{"data":{"name":"f","arguments":{"text":"nested"}}},"name":"f""#,
    ] {
        let calls = recovered(
            input,
            &set([DialectId::Llama3JsonStripped, DialectId::BareJson]),
        );
        assert_eq!(calls.calls.len(), 1, "{input}: {calls:?}");
        assert_eq!(calls.calls[0].dialect, DialectId::Llama3JsonStripped);
        assert!(
            calls.calls[0].arguments.get("data").is_some(),
            "{input}: {calls:?}"
        );
    }
    let reasoning = format!("<think>{array}</think>");
    let result = tools().recover(
        reasoning.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let readings = match result.outcome {
        Outcome::Resolved(reading) | Outcome::BestGuess(reading) => vec![reading],
        Outcome::Ambiguous(readings) => readings,
        other => panic!("reasoning evidence: {other:?}"),
    };
    assert!(readings.iter().any(|reading| !reading.embedded.is_empty()));
    for reading in readings {
        assert!(reading.calls.is_empty(), "{reading:?}");
        assert!(
            reading
                .embedded
                .iter()
                .all(|call| call.outcome != purrassist_core::tool_call::CallOutcome::Resolved)
        );
    }
    let fenced = format!("```json\n{array}\n```");
    let result = tools().recover(
        fenced.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let readings = match result.outcome {
        Outcome::Resolved(reading) | Outcome::BestGuess(reading) => vec![reading],
        Outcome::Ambiguous(readings) => readings,
        other => panic!("fenced example evidence: {other:?}"),
    };
    assert!(readings.iter().any(|reading| !reading.calls.is_empty()));
    for reading in readings {
        assert!(
            reading
                .calls
                .iter()
                .all(|call| call.outcome != purrassist_core::tool_call::CallOutcome::Resolved)
        );
        assert!(reading.usable_accepting_guesses().is_empty());
    }
}

#[test]
fn llama_retains_name_key_commit_evidence_after_removing_its_marker() {
    let uncommitted = r#"{"parameters":{"name":"f","arguments":{"text":"inner"}}}"#;
    let mut native = recovered(
        uncommitted,
        &set([DialectId::Llama3Json, DialectId::BareJson]),
    );
    let mut projected = recovered(
        uncommitted,
        &set([DialectId::Llama3JsonStripped, DialectId::BareJson]),
    );
    // A missing outer name is not permission to invent an opaque tool frame.
    native
        .calls
        .iter_mut()
        .for_each(|call| call.candidate_dialects.clear());
    projected
        .calls
        .iter_mut()
        .for_each(|call| call.candidate_dialects.clear());
    assert_eq!(native.calls, projected.calls);
    for dialect in [DialectId::Llama3Json, DialectId::Llama3JsonStripped] {
        for input in [
            r#"{"parameters":{}}"#,
            r#"{"arguments":{}}"#,
            r#"{"name":"f"}"#,
        ] {
            let calls = recovered(input, &set([dialect.clone()]));
            assert!(calls.calls.is_empty(), "{}: {input}", dialect.id());
            assert!(calls.rejected.is_empty(), "{}: {input}", dialect.id());
        }
    }
}

#[test]
fn derived_presets_expose_the_metadata_used_for_recompilation() {
    for dialect in DialectId::BUILT_IN {
        let Some(recipe @ GrammarRecipe::Derived(metadata)) = dialect.source() else {
            continue;
        };
        assert_eq!(metadata.id(), dialect.id());
        assert_eq!(metadata.source(), recipe.base_text());
        assert!(!metadata.markers().is_empty());
        assert!(metadata.markers().iter().all(|marker| !marker.is_empty()));
        let grammar = recipe
            .compile_governed(&mut Governor::new(Budget::default()))
            .expect("preset compiles");
        assert!(grammar.is_derived());
        assert_eq!(grammar.id(), dialect.id());
        assert_eq!(grammar.digest(), dialect.grammar().digest());
    }
}

#[test]
fn weak_spaced_names_require_declared_evidence_but_contiguous_names_do_not() {
    use purrassist_core::tool_call::ToolDefinition;
    let tools = ToolSet::compile([ToolDefinition::new("f").alias("alias")], Budget::default())
        .expect("tools");
    for dialect in [
        DialectId::DeepSeekV31Stripped,
        DialectId::MistralV11Stripped,
    ] {
        let set = set([dialect.clone()]);
        for (input, expected) in [
            (r#"Saved {"name":"Ada","age":36}."#, None),
            ("unknown {}", None),
            ("unknown\t{}", None),
            ("f {}", Some("f")),
            ("f{}\nSaved {\"name\":\"Ada\",\"age\":36}.", Some("f")),
            ("alias {}", Some("alias")),
            ("unknown{}", Some("unknown")),
            ("f12{}", Some("f12")),
            (
                "f12 {}",
                (dialect == DialectId::MistralV11Stripped).then_some("f12"),
            ),
        ] {
            let mut governor = Governor::new(Budget::default());
            let mut recognizer =
                Recognizer::with_tools(&set, &tools, ReasoningPolicy::default(), &mut governor)
                    .expect("recognizer");
            recognizer
                .push(input.as_bytes(), &mut governor)
                .expect("push");
            let turn = recognizer
                .finish(GenerationStop::Stop, &mut governor)
                .expect("finish");
            let calls = turn.calls();
            assert_eq!(
                calls.len(),
                usize::from(expected.is_some()),
                "{} {input}: {turn:?}",
                dialect.id()
            );
            if let Some(expected) = expected {
                assert_eq!(
                    calls[0].readings[0].name.as_ref().expect("name").value,
                    expected
                );
                if let Some(saved) = input.find("Saved") {
                    use purrassist_core::tool_call::Segment;
                    assert!(turn.rejected_attempts().is_empty(), "{input}: {turn:?}");
                    assert!(turn.selected().iter().any(|segment| matches!(segment, Segment::Content(span) if span.start <= saved && span.end == input.len())), "prose bytes retained as content: {turn:?}");
                }
                if dialect == DialectId::MistralV11Stripped && input.starts_with("f12") {
                    assert!(calls[0].readings.iter().any(|reading| {
                        reading.name.as_ref().is_some_and(|name| name.value == "f")
                            && reading.id.as_ref().is_some_and(|id| id.value == "12")
                    }));
                }
            } else {
                assert!(turn.rejected_attempts().is_empty(), "{input}: {turn:?}");
            }
        }
    }
    let (turn, _) = recognize(
        "functionunknown\n```json\n{}\n```",
        DialectId::DeepSeekV3Stripped,
    );
    assert_eq!(
        turn.calls().len(),
        1,
        "retained function evidence: {turn:?}"
    );
    assert_eq!(
        turn.calls()[0].readings[0]
            .name
            .as_ref()
            .expect("name")
            .value,
        "unknown"
    );
}

#[test]
fn harmony_complete_calls_rank_before_preserved_lossy_tail_alternatives() {
    let input = "commentary to=functions.f json{\"text\":\"first\"}\ncommentary to=functions.f json{\"text\":\"second\"}";
    let result = tools().recover(
        input.as_bytes(),
        &ToolRecoveryRequest::default(),
        &set([DialectId::HarmonyStripped]),
    );
    let Outcome::Ambiguous(readings) = &result.outcome else {
        panic!("preserve trailing-prose ambiguity: {result:?}")
    };
    assert_eq!(readings.len(), result.receipt.alternatives.len());
    assert_eq!(
        readings[0].calls.len(),
        2,
        "complete least-cost reading: {result:?}"
    );
    let first_cost = result.receipt.alternatives[0].cost;
    assert!(
        readings
            .iter()
            .zip(&result.receipt.alternatives)
            .any(|(reading, receipt)| {
                reading.calls.len() == 1
                    && receipt.cost > first_cost
                    && reading.calls[0].repairs.losses.iter().any(|loss| {
                        loss.loss.evidence.rule == "tool_call.extract.prose"
                            && loss.loss.original.contains("\"text\":\"second\"")
                    })
            }),
        "lossy competitor keeps source, ledger and higher rank cost: {result:?}"
    );
}
