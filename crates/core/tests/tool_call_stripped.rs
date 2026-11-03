// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Marker projections retain source evidence, ambiguity and native class semantics.
use purrassist_core::grammar::edit::EditOperator;
use purrassist_core::grammar::{CaptureKind, GrammarRecipe};
use purrassist_core::tool_call::{
    DialectId, DialectSet, Effects, GenerationStop, MarkupFamily, Param, ReasoningPolicy,
    RecognizedTurn, Recognizer, ToolCalls, ToolDefinition, ToolRecoveryRequest, ToolSet,
};
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
    recognize_with_tools(input, dialect, &tools())
}
fn recognize_with_tools(input: &str, dialect: DialectId, tools: &ToolSet) -> (RecognizedTurn, u64) {
    let set = set([dialect]);
    let mut governor = Governor::new(Budget::default());
    let mut recognizer =
        Recognizer::with_tools(&set, tools, ReasoningPolicy::default(), &mut governor)
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
    assert_long_initial_names_retain_calls();
}

fn assert_chunk_parity(input: &str, dialect: DialectId, splits: &[usize]) {
    assert_chunk_parity_with_tools(input, dialect, splits, &tools());
}

fn assert_chunk_parity_with_tools(
    input: &str,
    dialect: DialectId,
    splits: &[usize],
    tools: &ToolSet,
) -> RecognizedTurn {
    let (whole, work) = recognize_with_tools(input, dialect.clone(), tools);
    let set = set([dialect]);
    for &split in splits {
        let mut governor = Governor::new(Budget::default());
        let mut recognizer =
            Recognizer::with_tools(&set, tools, ReasoningPolicy::default(), &mut governor)
                .expect("recognizer");
        for chunk in [&input.as_bytes()[..split], &input.as_bytes()[split..]] {
            recognizer.push(chunk, &mut governor).expect("chunk");
        }
        let turn = recognizer
            .finish(GenerationStop::Stop, &mut governor)
            .expect("finish");
        assert_eq!(turn, whole, "split at {split}: {input:?}");
        assert_eq!(governor.usage().work, work, "split at {split}: work");
    }
    whole
}

/// Long initial names retain their source and admit only native JSON evidence.
fn assert_long_initial_names_retain_calls() {
    let name = "a".repeat(64);
    let tools = ToolSet::compile(
        [ToolDefinition::new(&name)
            .effects(Effects::ReadOnly)
            .param("text", Param::string().required())],
        Budget::default(),
    )
    .expect("long name tool");
    let arguments = r#"{"text":"retained"}"#;
    for (dialect, input) in [
        (
            DialectId::DeepSeekV3Stripped,
            format!("{name}\n```json\n{arguments}\n```"),
        ),
        (DialectId::DeepSeekV31Stripped, format!("{name}{arguments}")),
        (DialectId::KimiK2Stripped, format!("{name}{arguments}")),
        (DialectId::MistralV11Stripped, format!("{name}{arguments}")),
    ] {
        let result = tools.recover(
            input.as_bytes(),
            &ToolRecoveryRequest::default(),
            &set([dialect.clone()]),
        );
        assert_eq!(result.receipt.sources[0].bytes, input.as_bytes());
        let (Outcome::Resolved(calls) | Outcome::BestGuess(calls)) = &result.outcome else {
            panic!("{}: {:?}", dialect.id(), result.outcome)
        };
        assert_eq!(calls.calls.len(), 1, "{}", dialect.id());
        assert_eq!(calls.calls[0].tool.as_deref(), Some(name.as_str()));
        assert_eq!(
            calls.calls[0]
                .arguments
                .get("text")
                .and_then(purrassist_core::json::Node::as_str),
            Some("retained")
        );
        let turn = assert_chunk_parity_with_tools(&input, dialect, &[32, 64, input.len()], &tools);
        assert!(
            turn.calls()
                .iter()
                .flat_map(|call| &call.readings)
                .any(|reading| {
                    reading.name.as_ref().is_some_and(|captured| {
                        captured.value == name && input[captured.span.clone()] == name
                    }) && reading.captures.iter().any(|capture| {
                        capture.kind == CaptureKind::Arguments
                            && &input[capture.span.clone()] == arguments
                    })
                })
        );
    }
}

fn assert_long_preamble_retains_source() {
    // A long dropped preamble crosses UTF-8 and ends at the JSON opener;
    // extraction preserves the captured name, arguments and original source.
    let preamble = format!("{}é ", "p".repeat(64));
    let input = format!(r#"{preamble}{{"name":"f","arguments":{{"text":"retained"}}}}"#);
    let (turn, _) = recognize(&input, DialectId::HermesStripped);
    let calls = turn.calls();
    assert_eq!(calls.len(), 1);
    let raw = &calls[0].readings[0];
    let name = raw.name.as_ref().expect("retained name");
    assert_eq!(name.value, "f");
    assert_eq!(&input[name.span.clone()], "f");
    assert_eq!(raw.span, 0..input.len());
    assert_eq!(
        raw.arguments
            .root
            .get("text")
            .and_then(purrassist_core::json::Node::as_str),
        Some("retained")
    );
    assert!(raw.captures.iter().any(|capture| {
        capture.kind == CaptureKind::Dropped && input[capture.span.clone()] == preamble
    }));
    assert!(raw.captures.iter().any(|capture| {
        capture.kind == CaptureKind::Arguments
            && &input[capture.span.clone()] == r#"{"text":"retained"}"#
    }));
    assert!(raw.edits.iter().any(|edit| {
        edit.operator == EditOperator::ExtractProse
            && edit.original == preamble.as_bytes()
            && edit.replacement.is_empty()
    }));
    assert_chunk_parity(
        &input,
        DialectId::HermesStripped,
        &[32, 64, 65, preamble.len(), preamble.len() + 1, input.len()],
    );
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
    let long_name = "f".repeat(64);
    let unicode_boundary = format!("{long_name}é");
    for dialect in [
        DialectId::DeepSeekV3Stripped,
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
            &long_name,
            &unicode_boundary,
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
            if input == long_name || input == unicode_boundary {
                assert_eq!(
                    calls.content.as_slice(),
                    core::slice::from_ref(&(0..input.len())),
                    "{}: {input:?}",
                    dialect.id()
                );
            }
        }
        for input in [&long_name, &unicode_boundary] {
            assert_chunk_parity(input, dialect.clone(), &[32, 64, input.len()]);
        }
    }

    assert_long_preamble_retains_source();
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
        assert_eq!(calls.rejected.len(), 0);
    }
}

#[test]
fn exact_bare_json_label_precedes_derived_labels_for_the_same_call() {
    let input = "{\"name\":\"f\",\"arguments\":{\"text\":\"retained\"}}";
    let calls = recovered(input, DialectSet::built_in());
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
    let Outcome::Resolved(calls) = result.outcome else {
        panic!("prefer the exact singleton object framing: {result:?}")
    };
    assert_eq!(calls.calls.len(), 1);
    assert_eq!(calls.calls[0].dialect, DialectId::BareJson);
    assert_eq!(calls.calls[0].span, 1..array.len() - 1);
    assert_eq!(
        calls
            .content
            .iter()
            .map(|span| &array[span.clone()])
            .collect::<Vec<_>>(),
        ["[", "]"]
    );
    assert!(
        calls.calls[0]
            .candidate_dialects
            .contains(&DialectId::MistralV3Stripped)
    );
    assert_eq!(result.receipt.alternatives.len(), 0);

    assert_nested_arguments_remain_data();
    assert_failed_native_frame_preserves_content();

    let reasoning = format!("<think>{array}</think>");
    let result = tools().recover(
        reasoning.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let readings = match result.outcome {
        Outcome::Resolved(reading) | Outcome::BestGuess(reading) => vec![reading],
        Outcome::Ambiguous(readings) => readings,
        other @ Outcome::Unresolved => panic!("reasoning evidence: {other:?}"),
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
    let prefaced = format!("\nFor example:\n```json\n{input}\n```\n");
    let (turn, _) = recognize(&prefaced, DialectId::HermesStripped);
    assert_ne!(turn.calls().len(), 0);
    for call in turn.calls() {
        for reading in &call.readings {
            assert!(
                reading
                    .edits
                    .iter()
                    .any(|edit| edit.operator == EditOperator::ContextFencedExample),
                "{reading:?}"
            );
        }
    }
    for fenced in [format!("```json\n{array}\n```"), prefaced] {
        assert_fenced_calls_not_usable(&fenced);
    }
}

/// Failed projection framing cannot swallow prose before a native marker.
fn assert_failed_native_frame_preserves_content() {
    use purrassist_core::tool_call::{Effects, Piece, Rejection, Segment};
    let input = "Checking. [TOOL_CALLS] [{\"name\":\"read_file\",\"arguments\":{\"path\":\"a.txt\" oops. Sorry, my answer is 42 (see {1,2} set).";
    let golden = purrassist_core::json::parse(include_str!(
        "data/tool-call-goldens/mistral3_prose_after_unclosed_arguments.json"
    ))
    .expect("Mistral prose golden");
    assert_eq!(
        golden
            .get("input")
            .and_then(purrassist_core::json::Node::as_str),
        Some(input)
    );
    let declaration = golden.get("tools_openai").expect("golden tools").to_json();
    let tools = ToolSet::from_openai(&declaration, Budget::default()).expect("golden tools");
    let effects = match golden
        .pointer("/effects/read_file")
        .and_then(purrassist_core::json::Node::as_str)
    {
        Some("read_only") => Effects::ReadOnly,
        other => panic!("unexpected golden read_file effects: {other:?}"),
    };
    let request = ToolRecoveryRequest::default().effects("read_file", effects);
    let result = tools.recover(input.as_bytes(), &request, DialectSet::built_in());
    assert_eq!(result.receipt.sources[0].bytes, input.as_bytes());
    let Outcome::Resolved(calls) = result.outcome else {
        panic!("native failed frame preserves exterior content: {result:?}");
    };
    assert_eq!(calls.calls.len(), 0);
    assert_eq!(calls.rejected.len(), 1);
    let rejection = &calls.rejected[0];
    assert_eq!(rejection.span, 10..72);
    assert_eq!(rejection.reason, Rejection::Malformed { at: 72 });
    assert_eq!(
        rejection.candidate_dialects,
        [DialectId::MistralV11, DialectId::MistralV3]
    );
    assert_eq!(
        calls
            .content
            .iter()
            .map(|span| &input[span.clone()])
            .collect::<Vec<_>>(),
        [
            "Checking. ",
            "oops. Sorry, my answer is 42 (see {1,2} set)."
        ]
    );

    let set = set([DialectId::HermesStripped, DialectId::MistralV3]);
    let mut turns = Vec::new();
    // Split inside the physically consumed marker, before either failure.
    for chunks in [
        vec![input.as_bytes()],
        vec![&input.as_bytes()[..16], &input.as_bytes()[16..]],
    ] {
        let mut governor = Governor::new(Budget::default());
        let mut recognizer = Recognizer::with_tools(&set, &tools, request.reasoning, &mut governor)
            .expect("recognizer");
        for chunk in chunks {
            recognizer.push(chunk, &mut governor).expect("chunk");
        }
        turns.push(
            recognizer
                .finish(GenerationStop::Stop, &mut governor)
                .expect("finish"),
        );
    }
    assert_eq!(turns[0], turns[1]);
    let mut segments = Vec::new();
    for piece in turns[0].pieces() {
        match piece {
            Piece::Segment(segment) => segments.push(segment),
            Piece::Choice(choice) => segments.extend(choice.readings.iter().flatten()),
        }
    }
    assert!(
        segments.iter().any(|segment| matches!(segment,
        Segment::Rejected { span,dialect,reason:Rejection::Malformed{at:72} }
        if *span==(0..72) && *dialect==DialectId::HermesStripped)),
        "the original weak rejection remains an alternative"
    );
    assert!(segments.iter().any(|segment| matches!(segment,
        Segment::Rejected { span,dialect,reason:Rejection::Malformed{at:72} }
        if *span==(10..72) && *dialect==DialectId::MistralV3)));
}

fn assert_nested_arguments_remain_data() {
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
    // The same-frame exception must not promote a native-shaped payload
    // nested inside the projected call's arguments in closed reasoning.
    // The projected call's opener must be at the start of a source line.
    let nested_reasoning = r#"<think>
hmm {"name":"f","input":{"data":{"name":"f","arguments":{"text":"nested"}}}} done</think>"#;
    let nested_calls = recovered(
        nested_reasoning,
        &set([DialectId::HermesStripped, DialectId::BareJson]),
    );
    assert_eq!(nested_calls.calls.len(), 0);
    assert_eq!(nested_calls.embedded.len(), 1, "{nested_calls:?}");
    assert!(
        nested_calls.embedded[0].arguments.get("data").is_some(),
        "{nested_calls:?}"
    );
}

/// Exterior framing cannot turn a possible example into a usable call.
fn assert_fenced_calls_not_usable(input: &str) {
    let result = tools().recover(
        input.as_bytes(),
        &ToolRecoveryRequest::default(),
        DialectSet::built_in(),
    );
    let readings = match result.outcome {
        Outcome::Resolved(reading) | Outcome::BestGuess(reading) => vec![reading],
        Outcome::Ambiguous(readings) => readings,
        other @ Outcome::Unresolved => panic!("fenced example evidence: {other:?}"),
    };
    assert!(readings.iter().any(|reading| !reading.calls.is_empty()));
    for reading in readings {
        assert!(
            reading
                .calls
                .iter()
                .all(|call| call.outcome != purrassist_core::tool_call::CallOutcome::Resolved)
        );
        assert_eq!(reading.usable_accepting_guesses().len(), 0);
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
        assert_ne!(metadata.markers().len(), 0);
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
