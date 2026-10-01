// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Grammar engine, IR and recognizer core: the DSL, compilation budgets,
//! the linear fast path, edit operators, the packed forest, segmentation,
//! chunking invariance, dialect-order independence, reasoning policies, and
//! end-of-output semantics.
use purrassist_core::contract::{CancellationToken, InferenceKind, Resource, Usage};
use purrassist_core::grammar::codec::CodecKind;
use purrassist_core::grammar::cost;
use purrassist_core::grammar::edit::{EditOperator, LossKind, MarkupEdit};
use purrassist_core::grammar::{CaptureKind, Expr, Grammar, GrammarError};
use purrassist_core::provenance::{NodeId, ProvenanceNode};
use purrassist_core::tool_call::{
    CallSegment, Closure, DialectId, DialectSet, DialectSetError, GenerationStop, Piece, RawCall,
    ReasoningClosure, ReasoningPolicy, ReasoningSegment, RecognizedTurn, Recognizer, Rejection,
    Segment,
};
use purrassist_core::{Budget, Effort, Governor, StopReason};

const MINI: &str = include_str!("data/grammar/mini.grammar");
const OVERLAP: &str = include_str!("data/grammar/overlap.grammar");
const AMBIGUOUS: &str = include_str!("data/grammar/ambiguous.grammar");

fn dialect(text: &str) -> DialectId {
    let mut governor = Governor::new(Budget::default());
    DialectId::custom(Grammar::compile_governed(text, &mut governor).expect("grammar compiles"))
}

fn set(texts: &[&str]) -> DialectSet {
    let mut governor = Governor::new(Budget::default());
    DialectSet::new(texts.iter().map(|text| dialect(text)), &mut governor).expect("dialect set")
}

fn run(
    set: &DialectSet,
    chunks: &[&[u8]],
    stop: GenerationStop,
    policy: ReasoningPolicy,
    governor: &mut Governor,
) -> Result<RecognizedTurn, StopReason> {
    let mut recognizer = Recognizer::new(set, policy, governor)?;
    for chunk in chunks {
        recognizer.push(chunk, governor)?;
    }
    recognizer.finish(stop, governor)
}

fn recognize_with(
    set: &DialectSet,
    input: &[u8],
    stop: GenerationStop,
    policy: ReasoningPolicy,
) -> RecognizedTurn {
    let mut governor = Governor::new(Budget::default());
    run(set, &[input], stop, policy, &mut governor).expect("recognized")
}

fn recognize(set: &DialectSet, input: &[u8], stop: GenerationStop) -> RecognizedTurn {
    recognize_with(set, input, stop, ReasoningPolicy::default())
}

/// The selected readings of the turn's call segments.
fn calls(turn: &RecognizedTurn) -> Vec<&RawCall> {
    turn.calls()
        .into_iter()
        .map(|call| &call.readings[0])
        .collect()
}

fn one_call(turn: &RecognizedTurn) -> &RawCall {
    let calls = calls(turn);
    assert_eq!(calls.len(), 1, "{:#?}", turn.pieces());
    calls[0]
}

fn operators(call: &RawCall) -> Vec<EditOperator> {
    call.edits.iter().map(|edit| edit.operator).collect()
}

/// Every whole-turn reading, materialized (only for small turns).
fn readings(turn: &RecognizedTurn) -> Vec<Vec<Segment>> {
    let mut all: Vec<Vec<Segment>> = vec![Vec::new()];
    for piece in turn.pieces() {
        match piece {
            Piece::Segment(segment) => {
                for reading in &mut all {
                    reading.push(segment.clone());
                }
            }
            Piece::Choice(choice) => {
                all = all
                    .iter()
                    .flat_map(|prefix| {
                        choice.readings.iter().map(move |alternative| {
                            let mut reading = prefix.clone();
                            reading.extend(alternative.iter().cloned());
                            reading
                        })
                    })
                    .collect();
            }
        }
    }
    all
}

/// Inputs covering every segment kind, repair, and end-of-output path.
const CORPUS: &[&[u8]] = &[
    b"plain content only",
    b"hi <tool><call=read id=c1><arg=path>a.rs</arg><arg=n>2</arg></call></tool> bye",
    b"<tool><call=f></args></args></call></tool>",
    b"<tool><call=f><arg=a>1<arg=b>2</arg></call></tool> tail",
    b"<tool><call=f><arg=a>1</args></call></tool>",
    b"<tool><call=f><arg=a>1</arg></calls></tool>",
    b"x <|result|>ok<|/result|> y",
    b"<tool><call=f <tool><call=g></call></tool>",
    b"<tool><tool><call=f></call></tool>",
    b"<tool><call=f><arg=a>1</arg></call><tool><call=g></call></tool>",
    b"<think>plan <tool><call=f></call></tool></think> done",
    b"<think>plan <tool><call=f></call></tool> more <tool><call=g></call></tool>",
    b"<think>only thinking",
    b"a <c>x</c> <tool><call=f></call></tool>",
    b"<tool><call=f><arg=a>1</arg></call>",
    b"<tool><call=f><arg=a>partial",
    "<tool><call=f><arg=a>caf\u{e9}</arg></call></tool>".as_bytes(),
];

#[test]
fn dsl_parses_and_compiles() {
    let mut governor = Governor::new(Budget::default());
    let mini = Grammar::compile_governed(MINI, &mut governor).expect("mini compiles");
    assert_eq!(mini.id(), "mini.v1");
    assert_eq!(
        mini.digest(),
        purrassist_core::digest::Digest::of(MINI.as_bytes())
    );
    assert_eq!(mini.reasoning(), Some((&b"<think>"[..], &b"</think>"[..])));
    assert_eq!(
        mini.confusables(),
        [("｜".as_bytes().to_vec(), b"|".to_vec())]
    );
    assert!(mini.configuration_bound() > 0);
    assert_eq!(mini.longest_literal(), "<｜/result｜>".len());

    // The parameter production keeps literals, slot and capture placement.
    let (_, param) = mini.production("param").expect("param production");
    let Expr::Sequence(items) = param else {
        panic!("param is a sequence: {param:?}");
    };
    assert!(matches!(items[0], Expr::Literal(id) if mini.literal(id) == b"<arg="));
    let Expr::Capture {
        kind: CaptureKind::ParamName,
        inner,
    } = &items[1]
    else {
        panic!("param-name capture expected");
    };
    let Expr::Reference(ident) = **inner else {
        panic!("reference to ident expected");
    };
    assert_eq!(mini.production_name(ident), "ident");
    let Expr::Class(class) = mini.production_expr(ident) else {
        panic!("ident is a class");
    };
    assert_eq!(mini.class_source(*class), "[A-Za-z_][A-Za-z0-9_.:-]*");
    let Expr::Capture {
        kind: CaptureKind::ParamValue,
        inner,
    } = &items[3]
    else {
        panic!("param-value capture expected");
    };
    let Expr::Slot {
        codec: CodecKind::Raw,
        delimiters,
    } = **inner
    else {
        panic!("raw slot expected");
    };
    assert_eq!(mini.delimiters(delimiters).len(), 5);
    let Expr::Closer(arg_end) = items[4] else {
        panic!("closer expected");
    };
    assert_eq!(mini.closer_name(arg_end), "arg-end");
    assert_eq!(
        mini.closer_spellings(arg_end),
        [b"</arg>".to_vec(), b"</args>".to_vec()]
    );

    // Call, result and declaration are all retained for printing.
    let Expr::Sequence(call) = mini.call() else {
        panic!("call is a sequence");
    };
    assert!(matches!(
        &call[3],
        Expr::Repeat {
            min: 0,
            max: Some(1),
            ..
        }
    ));
    assert!(matches!(
        &call[5],
        Expr::Repeat {
            min: 0,
            max: None,
            ..
        }
    ));
    assert!(matches!(mini.result(), Expr::Sequence(items) if items.len() == 3));
    assert!(matches!(mini.declaration(), Expr::Sequence(items) if items.len() == 3));

    // The module documentation's worked example.
    let turn = recognize(
        &set(&[MINI]),
        b"<tool><call=read id=c1><arg=path>a.rs</arg></call></tool>",
        GenerationStop::Stop,
    );
    let call = one_call(&turn);
    assert_eq!(
        call.name.as_ref().map(|name| name.value.as_str()),
        Some("read")
    );
    assert_eq!(call.id.as_ref().map(|id| id.value.as_str()), Some("c1"));
    assert_eq!(call.arguments.root.to_json(), r#"{"path":"a.rs"}"#);
    assert_eq!(call.closure, Closure::Closed);

    for text in [OVERLAP, AMBIGUOUS] {
        let grammar = Grammar::compile_governed(text, &mut governor).expect("compiles");
        assert!(grammar.configuration_bound() > 0);
    }
}

type Expected = Box<dyn Fn(&str) -> GrammarError>;

#[test]
#[allow(clippy::too_many_lines)]
fn invalid_grammar_is_typed_error() {
    const TAIL: &str = " result = \"<r>\"; declaration = \"<d>\";";
    let at = |text: &str, needle: &str| text.find(needle).expect("needle in text");
    let cases: Vec<(String, Expected)> = vec![
        (
            "call = \"<c>\";".into(),
            Box::new(|_| GrammarError::Syntax {
                offset: 0,
                message: "a grammar starts with `grammar \"id\";`".into(),
            }),
        ),
        (
            "grammar \"g\"; call = \"<c>;".into(),
            Box::new(move |text| GrammarError::Syntax {
                offset: at(text, "\"<c>"),
                message: "unterminated string literal".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" missing;{TAIL}"),
            Box::new(move |text| GrammarError::UnknownName {
                offset: at(text, "missing"),
                name: "missing".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\"; call = \"<d>\";{TAIL}"),
            Box::new(move |text| GrammarError::DuplicateName {
                offset: text.rfind("call =").expect("second call"),
                name: "call".into(),
            }),
        ),
        (
            format!("grammar \"g\"; a = \"x\" b; b = \"y\" a; call = \"<c>\" a;{TAIL}"),
            Box::new(move |text| GrammarError::Recursive {
                offset: at(text, "a ="),
                name: "a".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" body:/[a-z]+/;{TAIL}"),
            Box::new(move |text| GrammarError::UnknownCapture {
                offset: at(text, "body:"),
                name: "body".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" arguments:slot(yaml, \"</c>\");{TAIL}"),
            Box::new(move |text| GrammarError::UnknownCodec {
                offset: at(text, "yaml"),
                name: "yaml".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" /(a)\\1/;{TAIL}"),
            Box::new(move |text| GrammarError::InvalidClass {
                offset: at(text, "/(a)"),
                message: "backreferences and lookaround are not finite-state".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" /[a-z]*/;{TAIL}"),
            Box::new(move |text| GrammarError::InvalidClass {
                offset: at(text, "/[a-z]*/"),
                message: "a class must not match the empty string; use `?` or `*`".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" \"\";{TAIL}"),
            Box::new(move |text| GrammarError::EmptyLiteral {
                offset: at(text, "\"\""),
            }),
        ),
        (
            "grammar \"g\"; call = \"<c>\"; declaration = \"<d>\";".into(),
            Box::new(|_| GrammarError::MissingProduction { name: "result" }),
        ),
        (
            format!("grammar \"g\"; call = /[a-z]+/ \"<c>\";{TAIL}"),
            Box::new(move |text| GrammarError::InvalidOpener {
                offset: at(text, "call"),
                production: "call".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\"?;{TAIL}"),
            Box::new(move |text| GrammarError::InvalidOpener {
                offset: at(text, "call"),
                production: "call".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" arguments:until(\"</c>\");{TAIL}"),
            Box::new(move |text| GrammarError::CaptureTarget {
                offset: at(text, "arguments"),
                capture: "arguments",
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" param-name:/[a-z]+/ \">\";{TAIL}"),
            Box::new(move |text| GrammarError::UnpairedParameter {
                offset: at(text, "param-name"),
            }),
        ),
        (
            format!(
                "grammar \"g\"; call = \"<c>\" arguments:slot(raw, \"|\") \"|\" param-name:/[a-z]+/ param-value:slot(raw, \"</c>\");{TAIL}"
            ),
            Box::new(move |text| GrammarError::ConflictingArguments {
                offset: at(text, "call"),
            }),
        ),
        (
            format!("grammar \"g\"; until = \"<c>\"; call = \"<c>\";{TAIL}"),
            Box::new(move |text| GrammarError::ReservedName {
                offset: at(text, "until"),
                name: "until".into(),
            }),
        ),
        (
            format!("grammar \"g\"; call = \"<c>\" \"\\q\";{TAIL}"),
            Box::new(move |text| GrammarError::Syntax {
                offset: at(text, "\\q"),
                message: "unknown escape \\q".into(),
            }),
        ),
    ];
    for (text, expected) in &cases {
        let mut governor = Governor::new(Budget::default());
        let error = Grammar::compile_governed(text, &mut governor).expect_err(text);
        assert_eq!(error, expected(text), "{text}");
        if let Some(offset) = error.offset() {
            assert!(offset < text.len(), "{text}");
        }
        assert_ne!(error.to_string(), "");
    }
}

#[test]
fn compile_budget_exhaustion_explicit() {
    // Every resource compilation charges is charged on the ordinary path.
    let mut governor = Governor::new(Budget::default());
    Grammar::compile_governed(MINI, &mut governor).expect("compiles");
    let usage = governor.usage();
    assert!(usage.work > 0 && usage.peak_scratch_bytes > 0);

    let starve = |resource: Resource| {
        let budget = match resource {
            Resource::Work => Budget {
                work: 0,
                ..Budget::default()
            },
            Resource::InputBytes => Budget {
                input_bytes: 0,
                ..Budget::default()
            },
            Resource::ScratchBytes => Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
            Resource::Depth => Budget {
                depth: 0,
                ..Budget::default()
            },
            other => panic!("compilation does not charge {other:?}"),
        };
        Governor::new(budget)
    };
    for resource in [
        Resource::Work,
        Resource::InputBytes,
        Resource::ScratchBytes,
        Resource::Depth,
    ] {
        let error = Grammar::compile_governed(MINI, &mut starve(resource)).expect_err("starved");
        assert_eq!(
            error,
            GrammarError::Resource(StopReason::BudgetExceeded(resource)),
            "{resource:?}"
        );
    }
    // A partially sufficient budget still fails explicitly, never partially.
    for work in [10, 1_000, usage.work - 1] {
        let mut governor = Governor::new(Budget {
            work,
            ..Budget::default()
        });
        assert_eq!(
            Grammar::compile_governed(MINI, &mut governor).unwrap_err(),
            GrammarError::Resource(StopReason::BudgetExceeded(Resource::Work))
        );
    }
    let token = CancellationToken::new();
    token.cancel();
    let mut cancelled = Governor::with_cancellation(Budget::default(), token);
    assert_eq!(
        Grammar::compile_governed(MINI, &mut cancelled).unwrap_err(),
        GrammarError::Resource(StopReason::Cancelled)
    );
    // Dialect sets are governed too.
    let mut starved = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        DialectSet::new([dialect(MINI)], &mut starved).unwrap_err(),
        DialectSetError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
}

fn large_budget() -> Budget {
    Budget {
        work: u64::MAX / 4,
        candidates: u64::MAX / 4,
        input_bytes: 1 << 30,
        scratch_bytes: 1 << 36,
        evidence_entries: u64::MAX / 4,
        ..Budget::default()
    }
}

#[test]
fn fast_path_is_deterministic_linear() {
    let mini = set(&[MINI]);
    let unit: &[u8] = b"text <tool><call=read_file id=c1><arg=path>src/main.rs</arg><arg=n>12</arg></call></tool> more text\n";
    let mut measured = Vec::new();
    for target in [64 * 1024, 256 * 1024, 1024 * 1024] {
        let size = target / unit.len() * unit.len();
        let input: Vec<u8> = unit.iter().copied().cycle().take(size).collect();
        let mut first = Governor::new(large_budget());
        let turn = run(
            &mini,
            &[&input],
            GenerationStop::Stop,
            ReasoningPolicy::default(),
            &mut first,
        )
        .expect("recognized");
        let mut second = Governor::new(large_budget());
        let again = run(
            &mini,
            &[&input],
            GenerationStop::Stop,
            ReasoningPolicy::default(),
            &mut second,
        )
        .expect("recognized");
        assert_eq!(turn, again, "identical results across runs");
        assert_eq!(first.usage(), second.usage(), "identical usage across runs");
        // Well-formed input takes the fast path: no repair, no ambiguity.
        assert!(!turn.is_ambiguous());
        assert_eq!(turn.forest().nodes(), []);
        assert_eq!(turn.calls().len(), size / unit.len());
        assert!(
            calls(&turn)
                .iter()
                .all(|call| call.closure == Closure::Closed)
        );
        let work = first.usage().work;
        // The published per-byte bound.
        let per_byte = cost::K_LEX + cost::K_THREAD * mini.configuration_bound();
        assert!(
            work <= per_byte * size as u64,
            "work {work} exceeds the published bound"
        );
        measured.push((size, work));
    }
    // (work / base work) / (bytes / base bytes) within [0.95, 1.05], in
    // exact integer arithmetic.
    let (base_bytes, base_work) = measured[0];
    for &(bytes, work) in &measured[1..] {
        let scaled = u128::from(work) * base_bytes as u128 * 100;
        let expected = u128::from(base_work) * bytes as u128;
        assert!(
            (expected * 95..=expected * 105).contains(&scaled),
            "work {work} for {bytes} bytes is not linear in {base_work} for {base_bytes} bytes"
        );
    }
}

fn edit_of(call: &RawCall, operator: EditOperator) -> &MarkupEdit {
    call.edits
        .iter()
        .find(|edit| edit.operator == operator)
        .unwrap_or_else(|| panic!("{operator:?} edit in {call:#?}"))
}

/// The declared rule, cost, guess flag and loss kind of `operator`, and
/// that the forest holds its evidence, loss and inference exactly as
/// declared.
fn assert_operator(
    turn: &RecognizedTurn,
    operator: EditOperator,
    declared: (&str, u64, bool, Option<LossKind>),
) {
    let rule = operator.rule();
    assert_eq!((rule.id, rule.cost, rule.guess, rule.loss), declared);
    let nodes = turn.forest().nodes();
    let count = |matches: &dyn Fn(&ProvenanceNode) -> bool| {
        nodes.iter().filter(|node| matches(node)).count()
    };
    let evidence = count(
        &|node| matches!(node, ProvenanceNode::Evidence(evidence) if evidence.rule == rule.id),
    );
    let losses =
        count(&|node| matches!(node, ProvenanceNode::Loss(loss) if loss.evidence.rule == rule.id));
    let guesses = count(&|node| {
        matches!(node, ProvenanceNode::Inference(inference)
            if inference.evidence.rule == rule.id && inference.kind == InferenceKind::Guess)
    });
    assert!(evidence > 0, "{} evidence in the forest", rule.id);
    assert_eq!(losses > 0, rule.loss.is_some(), "{} loss", rule.id);
    assert_eq!(guesses > 0, rule.guess, "{} guess", rule.id);
    for node in nodes {
        if let ProvenanceNode::Evidence(evidence) = node
            && evidence.rule == rule.id
        {
            let span = evidence.span.as_ref().expect("edits carry spans");
            assert_eq!(span.source, turn.source());
        }
    }
}

#[test]
fn edit_operators_repair_mini_grammar() {
    let mini = set(&[MINI]);

    // A missing `</arg>` before the next parameter: insert the closer.
    let turn = recognize(
        &mini,
        b"<tool><call=f><arg=a>1<arg=b>2</arg></call></tool>",
        GenerationStop::Stop,
    );
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), r#"{"a":"1","b":"2"}"#);
    assert_eq!(call.closure, Closure::Repaired);
    assert_eq!(operators(call), [EditOperator::MissingFunctionClose]);
    let edit = edit_of(call, EditOperator::MissingFunctionClose);
    assert_eq!(
        (edit.span.clone(), edit.replacement.as_slice()),
        (22..22, &b"</arg>"[..])
    );
    assert_eq!(call.cost, cost::INSERT_CLOSER);
    assert_operator(
        &turn,
        EditOperator::MissingFunctionClose,
        ("tool_call.markup.missing-function-close", 2, false, None),
    );

    // Closers-only body: both plural closers are stray and deleted.
    let turn = recognize(
        &mini,
        b"<tool><call=f></args></args></call></tool>",
        GenerationStop::Stop,
    );
    let call = one_call(&turn);
    assert_eq!(call.arguments.root.to_json(), "{}");
    assert_eq!(operators(call), [EditOperator::DeleteStrayCloser; 2]);
    assert_eq!(call.edits[0].original, b"</args>");
    assert_eq!(call.edits[1].span, 21..28);
    assert_eq!(call.cost, 2 * cost::DELETE_STRAY_CLOSER);
    assert_operator(
        &turn,
        EditOperator::DeleteStrayCloser,
        (
            "tool_call.markup.stray-closer",
            3,
            false,
            Some(LossKind::Markup),
        ),
    );

    // A declared closer variant.
    let turn = recognize(
        &mini,
        b"<tool><call=f><arg=a>1</args></call></tool>",
        GenerationStop::Stop,
    );
    let call = one_call(&turn);
    assert_eq!(operators(call), [EditOperator::CloserVariant]);
    let edit = edit_of(call, EditOperator::CloserVariant);
    assert_eq!(
        (edit.original.as_slice(), edit.replacement.as_slice()),
        (&b"</args>"[..], &b"</arg>"[..])
    );
    assert_eq!(call.cost, cost::CLOSER_VARIANT);
    assert_operator(
        &turn,
        EditOperator::CloserVariant,
        (
            "tool_call.markup.closer-variant",
            1,
            false,
            Some(LossKind::Markup),
        ),
    );

    // ASCII `|` for the full-width `｜` of the result markers.
    let input: &[u8] = b"x <|result|>ok<|/result|> y";
    let turn = recognize(&mini, input, GenerationStop::Stop);
    let Segment::Result(result) = turn.selected()[1] else {
        panic!("result segment expected: {:#?}", turn.pieces());
    };
    let reading = &result.readings[0];
    assert_eq!(
        reading.capture_text(CaptureKind::Content, input),
        Some("ok")
    );
    assert_eq!(operators(reading), [EditOperator::TerminalConfusable; 2]);
    assert_eq!(reading.edits[0].replacement, "<｜result｜>".as_bytes());
    assert_operator(
        &turn,
        EditOperator::TerminalConfusable,
        (
            "tool_call.markup.terminal-confusable",
            1,
            false,
            Some(LossKind::Markup),
        ),
    );

    // The stop sequence consumed the expected closer.
    let turn = recognize(
        &mini,
        b"<tool><call=f><arg=a>1</arg></call>",
        GenerationStop::StopSequence(b"</tool>".to_vec()),
    );
    let call = one_call(&turn);
    assert_eq!(operators(call), [EditOperator::ConsumedCloser]);
    assert_eq!(call.closure, Closure::Repaired);
    assert_operator(
        &turn,
        EditOperator::ConsumedCloser,
        ("tool_call.stop.consumed-closer", 1, false, None),
    );
}

#[test]
fn forest_factorizes_independent_choices() {
    // Twenty calls, each with two equal-cost readings (name or id): the
    // ambiguity stays local to each call and never multiplies out.
    let ambiguous = set(&[AMBIGUOUS]);
    let input: Vec<u8> = (b'a'..=b't')
        .enumerate()
        .flat_map(|(index, word)| {
            format!("step {index}: <c>{}</c>\n", char::from(word)).into_bytes()
        })
        .collect();
    let mut governor = Governor::new(Effort::Standard.budget());
    let turn = run(
        &ambiguous,
        &[&input],
        GenerationStop::Stop,
        ReasoningPolicy::default(),
        &mut governor,
    )
    .expect("no exhaustion at Effort::Standard");
    assert_eq!(turn.calls().len(), 20);
    assert!(!turn.is_ambiguous());
    assert_eq!(turn.readings(), 1);
    for call in turn.calls() {
        assert_eq!(call.readings.len(), 2, "per-call ambiguity");
        assert_eq!(call.derivations.len(), 2);
        let named = call
            .readings
            .iter()
            .filter(|reading| reading.name.is_some())
            .count();
        let identified = call
            .readings
            .iter()
            .filter(|reading| reading.id.is_some())
            .count();
        assert_eq!((named, identified), (1, 1));
        assert_eq!(call.derivations[0].cost(), call.derivations[1].cost());
        assert_ne!(call.derivations[0].output(), call.derivations[1].output());
    }

    // Twenty spans two dialects both recognize: twenty local choices, each
    // with two readings, and still no exhaustion.
    let both = set(&[MINI, OVERLAP]);
    let input: Vec<u8> = (0..20)
        .flat_map(|index| {
            format!("call {index} <tool><call=f{index}></call></tool>\n").into_bytes()
        })
        .collect();
    let mut governor = Governor::new(Effort::Standard.budget());
    let turn = run(
        &both,
        &[&input],
        GenerationStop::Stop,
        ReasoningPolicy::default(),
        &mut governor,
    )
    .expect("no exhaustion at Effort::Standard");
    let choices: Vec<_> = turn
        .pieces()
        .iter()
        .filter_map(|piece| match piece {
            Piece::Choice(choice) => Some(choice),
            Piece::Segment(_) => None,
        })
        .collect();
    assert_eq!(choices.len(), 20);
    assert_eq!(turn.readings(), 1 << 20);
    for choice in choices {
        assert_eq!(choice.readings.len(), 2);
        let dialects: Vec<&str> = choice
            .readings
            .iter()
            .map(|reading| match &reading[..] {
                [Segment::Call(call)] => call.readings[0].dialect.name(),
                other => panic!("one call per reading: {other:?}"),
            })
            .collect();
        assert!(dialects.contains(&"mini.v1") && dialects.contains(&"overlap.v1"));
        assert_eq!(choice.derivations[0].cost(), choice.derivations[1].cost());
    }
}

fn assert_partition(segments: &[Segment], len: usize, context: &str) {
    let mut at = 0;
    for segment in segments {
        let span = segment.span();
        assert_eq!(
            span.start, at,
            "{context}: contiguous at {at}: {segments:#?}"
        );
        // Only a reasoning block closed by guess at an opener at its own
        // start is zero-width (it still carries the guess).
        let zero_width = matches!(segment, Segment::Reasoning(block)
            if block.closure == ReasoningClosure::ClosedAtOpener);
        assert!(
            span.end > span.start || zero_width,
            "{context}: non-empty segment {span:?}"
        );
        if let Segment::Call(call) | Segment::Result(call) = segment {
            for reading in &call.readings {
                assert_eq!(reading.span, span, "{context}: readings share the span");
            }
        }
        at = span.end;
    }
    assert_eq!(at, len, "{context}: covers the whole input");
}

#[test]
fn segmentation_partitions_input() {
    // A rejected attempt ends where its failing token began, so an opener
    // starting inside that token still starts a call.
    let mini = set(&[MINI]);
    let turn = recognize(
        &mini,
        b"<tool><tool><call=f></call></tool>",
        GenerationStop::Stop,
    );
    let selected = turn.selected();
    assert!(matches!(
        selected[0],
        Segment::Rejected { span, reason: Rejection::Malformed { at: 7 }, .. } if *span == (0..6)
    ));
    assert!(matches!(selected[1], Segment::Call(call) if call.span == (6..34)));

    let all = set(&[MINI, OVERLAP, AMBIGUOUS]);
    for policy in [
        ReasoningPolicy::default(),
        ReasoningPolicy::Separate { starts_open: true },
        ReasoningPolicy::AsContent,
    ] {
        for &input in CORPUS {
            for stop in [GenerationStop::Stop, GenerationStop::Length] {
                let turn = recognize_with(&all, input, stop.clone(), policy);
                let context = format!("{:?} {policy:?} {stop:?}", String::from_utf8_lossy(input));
                for reading in readings(&turn) {
                    assert_partition(&reading, input.len(), &context);
                }
                assert_eq!(turn.len(), input.len());
            }
        }
    }
}

#[test]
fn push_split_equals_single_push() {
    let all = set(&[MINI, OVERLAP, AMBIGUOUS]);
    let stops = [
        GenerationStop::Stop,
        GenerationStop::Length,
        GenerationStop::StopSequence(b"</tool>".to_vec()),
    ];
    for &input in CORPUS {
        for stop in &stops {
            for policy in [
                ReasoningPolicy::default(),
                ReasoningPolicy::Separate { starts_open: true },
            ] {
                let mut whole = Governor::new(Budget::default());
                let expected =
                    run(&all, &[input], stop.clone(), policy, &mut whole).expect("recognized");
                for split in 0..=input.len() {
                    let mut governor = Governor::new(Budget::default());
                    let turn = run(
                        &all,
                        &[&input[..split], &input[split..]],
                        stop.clone(),
                        policy,
                        &mut governor,
                    )
                    .expect("recognized");
                    assert_eq!(
                        turn,
                        expected,
                        "{:?} split at {split}",
                        String::from_utf8_lossy(input)
                    );
                    assert_eq!(governor.usage(), whole.usage(), "usage at split {split}");
                }
                // Byte at a time as well.
                let mut governor = Governor::new(Budget::default());
                let bytes: Vec<&[u8]> = input.chunks(1).collect();
                let turn =
                    run(&all, &bytes, stop.clone(), policy, &mut governor).expect("recognized");
                assert_eq!(turn, expected);
                assert_eq!(governor.usage(), whole.usage());
            }
        }
    }
}

#[test]
fn dialect_set_order_independent() {
    let texts = [MINI, OVERLAP, AMBIGUOUS];
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let input: &[u8] = b"a <tool><call=f></call></tool> b <c>x</c> <|result|>r<|/result|> <tool><call=g></args></call></tool>";
    let mut expected: Option<(RecognizedTurn, Usage)> = None;
    for permutation in permutations {
        let mut governor = Governor::new(Budget::default());
        let dialects: Vec<DialectId> = permutation
            .iter()
            .map(|&index| dialect(texts[index]))
            .collect();
        let set = DialectSet::new(dialects, &mut governor).expect("set");
        let names: Vec<&str> = set.dialects().iter().map(DialectId::name).collect();
        assert_eq!(names, ["ambiguous.v1", "mini.v1", "overlap.v1"]);
        let mut governor = Governor::new(Budget::default());
        let turn = run(
            &set,
            &[input],
            GenerationStop::Stop,
            ReasoningPolicy::default(),
            &mut governor,
        )
        .expect("recognized");
        assert!(turn.is_ambiguous());
        match &expected {
            None => expected = Some((turn, governor.usage())),
            Some((first, usage)) => {
                assert_eq!(&turn, first, "{permutation:?}");
                assert_eq!(&governor.usage(), usage, "{permutation:?}");
            }
        }
    }
}

fn reasoning(segment: &Segment) -> &ReasoningSegment {
    match segment {
        Segment::Reasoning(reasoning) => reasoning,
        other => panic!("reasoning expected, got {other:?}"),
    }
}

#[test]
fn reasoning_policies() {
    let mini = set(&[MINI]);
    let input: &[u8] = b"<think>consider</think>answer <tool><call=f></call></tool>";

    // Separate: the block is its own segment.
    let turn = recognize(&mini, input, GenerationStop::Stop);
    let selected = turn.selected();
    let block = reasoning(selected[0]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (0..23, ReasoningClosure::Closed)
    );
    assert!(block.edits.is_empty() && block.embedded.is_empty());
    assert_eq!(selected[1], &Segment::Content(23..30));
    assert!(matches!(selected[2], Segment::Call(_)));

    // starts_open: everything before the first closer is reasoning, and the
    // lone closer is the reasoning closer, never stray markup or content.
    let opened = ReasoningPolicy::Separate { starts_open: true };
    let turn = recognize_with(
        &mini,
        b"consider</think>answer",
        GenerationStop::Stop,
        opened,
    );
    let selected = turn.selected();
    assert_eq!(selected.len(), 2);
    let block = reasoning(selected[0]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (0..16, ReasoningClosure::Closed)
    );
    assert_eq!(selected[1], &Segment::Content(16..22));
    // An empty output holds no reasoning.
    let turn = recognize_with(&mini, b"", GenerationStop::Stop, opened);
    assert!(turn.pieces().is_empty() && turn.is_empty());
    // Without a closer the unclosed-block rules apply from offset 0.
    let turn = recognize_with(&mini, b"still thinking", GenerationStop::Stop, opened);
    let block = reasoning(turn.selected()[0]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (0..14, ReasoningClosure::Unterminated)
    );
    let turn = recognize_with(
        &mini,
        b"plan <tool><call=f></call></tool>",
        GenerationStop::Stop,
        opened,
    );
    let selected = turn.selected();
    let block = reasoning(selected[0]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (0..5, ReasoningClosure::ClosedAtOpener)
    );
    assert!(matches!(selected[1], Segment::Call(_)));

    // AsContent: markers are content, and calls are still calls.
    let turn = recognize_with(
        &mini,
        input,
        GenerationStop::Stop,
        ReasoningPolicy::AsContent,
    );
    let selected = turn.selected();
    assert_eq!(selected[0], &Segment::Content(0..30));
    assert!(matches!(selected[1], Segment::Call(_)));
    let turn = recognize_with(
        &mini,
        b"<think>a <tool><call=f></call></tool></think>",
        GenerationStop::Stop,
        ReasoningPolicy::AsContent,
    );
    assert_eq!(turn.calls().len(), 1);
    assert!(
        turn.selected()
            .iter()
            .all(|segment| !matches!(segment, Segment::Reasoning(_)))
    );
}

/// The forest nodes of `rule`'s guess inferences that call derivations
/// depend on.
fn guess_nodes(turn: &RecognizedTurn, rule: &str) -> Vec<NodeId> {
    let mut ids = Vec::new();
    for call in turn.calls() {
        for derivation in &call.derivations {
            for &node in derivation.nodes() {
                if let ProvenanceNode::Inference(inference) = turn.forest().node(node)
                    && inference.evidence.rule == rule
                    && !ids.contains(&node)
                {
                    ids.push(node);
                }
            }
        }
    }
    ids
}

#[test]
fn unclosed_think_does_not_swallow_calls() {
    let mini = set(&[MINI]);
    let input: &[u8] =
        b"<think>plan <tool><call=f></call></tool> then <tool><call=g><arg=a>1</arg></call></tool>";
    let turn = recognize(&mini, input, GenerationStop::Stop);
    let selected = turn.selected();
    let block = reasoning(selected[0]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (0..12, ReasoningClosure::ClosedAtOpener)
    );
    assert_eq!(block.edits.len(), 1);
    assert_eq!(block.edits[0].operator, EditOperator::ReasoningUnclosed);
    assert_eq!(block.edits[0].span, 12..12);
    let names: Vec<&str> = calls(&turn)
        .iter()
        .map(|call| call.name.as_ref().expect("named").value.as_str())
        .collect();
    assert_eq!(names, ["f", "g"]);
    // Both later calls depend on the guess: at most a best guess.
    assert_operator(
        &turn,
        EditOperator::ReasoningUnclosed,
        ("tool_call.reasoning.unclosed", 4, true, None),
    );
    let guess = guess_nodes(&turn, "tool_call.reasoning.unclosed");
    assert_eq!(guess.len(), 1);
    for call in turn.calls() {
        assert!(call.derivations[0].contains(guess[0]));
    }
    assert_eq!(turn.cost(), cost::REASONING_UNCLOSED);

    // With no opener, the rest is reasoning and a data loss is recorded.
    let turn = recognize(&mini, b"answer <think>never closed", GenerationStop::Stop);
    let selected = turn.selected();
    assert_eq!(selected[0], &Segment::Content(0..7));
    let block = reasoning(selected[1]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (7..26, ReasoningClosure::Unterminated)
    );
    assert_operator(
        &turn,
        EditOperator::ReasoningUnterminated,
        (
            "tool_call.reasoning.unterminated",
            0,
            false,
            Some(LossKind::Data),
        ),
    );
}

#[test]
fn closed_think_embedded_call_is_guess_alternative() {
    let mini = set(&[MINI]);
    let input: &[u8] = b"<think>maybe <tool><call=f><arg=a>1</arg></call></tool></think>done";
    let turn = recognize(&mini, input, GenerationStop::Stop);
    let selected = turn.selected();
    assert_eq!(selected.len(), 2);
    let block = reasoning(selected[0]);
    assert_eq!(
        (block.span.clone(), block.closure),
        (0..63, ReasoningClosure::Closed)
    );
    // The call stays reasoning text ...
    assert_eq!(turn.calls().len(), 0, "no call segment");
    // ... and is offered as a guess-flagged alternative.
    assert_eq!(block.embedded.len(), 1);
    let embedded: &CallSegment = &block.embedded[0];
    let reading = &embedded.readings[0];
    assert_eq!(reading.name.as_ref().expect("named").value, "f");
    assert_eq!(reading.arguments.root.to_json(), r#"{"a":"1"}"#);
    assert_eq!(operators(reading), [EditOperator::ReasoningEmbeddedCall]);
    assert_eq!(embedded.derivations.len(), 1);
    assert_operator(
        &turn,
        EditOperator::ReasoningEmbeddedCall,
        ("tool_call.reasoning.embedded-call", 0, true, None),
    );
    assert_eq!(turn.cost(), 0);
}

#[test]
fn stop_sequence_reinserts_expected_closer() {
    let mini = set(&[MINI]);
    let input: &[u8] = b"<tool><call=f><arg=a>1</arg></call>";
    let turn = recognize(
        &mini,
        input,
        GenerationStop::StopSequence(b"</tool>".to_vec()),
    );
    let call = one_call(&turn);
    assert_eq!(call.closure, Closure::Repaired);
    let edit = edit_of(call, EditOperator::ConsumedCloser);
    assert_eq!(
        (edit.span.clone(), edit.replacement.as_slice()),
        (35..35, &b"</tool>"[..])
    );
    assert!(!edit.rule().guess);
    assert_eq!(call.cost, cost::CONSUMED_CLOSER);

    // Without the matching stop sequence the closer is a malformed-end
    // insertion, left to the caller's completeness rule.
    for stop in [
        GenerationStop::Stop,
        GenerationStop::Unknown,
        GenerationStop::ContentFilter,
        GenerationStop::ToolCalls,
        GenerationStop::StopSequence(b"</other>".to_vec()),
    ] {
        let turn = recognize(&mini, input, stop.clone());
        let call = one_call(&turn);
        assert_eq!(call.closure, Closure::Unterminated, "{stop:?}");
        assert_eq!(
            operators(call),
            [EditOperator::MissingClosersAtEnd],
            "{stop:?}"
        );
    }
}

#[test]
fn length_vs_stop_partial_call_distinguished() {
    let mini = set(&[MINI]);
    let input: &[u8] = b"text <tool><call=f><arg=a>partial";
    // A length stop: truncation, keeping what was captured.
    let turn = recognize(&mini, input, GenerationStop::Length);
    let call = one_call(&turn);
    assert_eq!(call.closure, Closure::Dangling);
    assert_eq!(call.arguments.root.to_json(), r#"{"a":"partial"}"#);
    assert_eq!(call.edits.len(), 0, "no edits");
    assert_eq!(turn.selected()[0], &Segment::Content(0..5));
    // Any other stop: malformed, completed only by end-of-output insertions.
    let turn = recognize(&mini, input, GenerationStop::Stop);
    let call = one_call(&turn);
    assert_eq!(call.closure, Closure::Unterminated);
    assert_eq!(operators(call), [EditOperator::MissingClosersAtEnd; 3]);

    // Truncation never rescues malformed markup, and an open call that no
    // closer can complete is rejected under a non-length stop.
    let input: &[u8] = b"text <tool><call=f";
    let turn = recognize(&mini, input, GenerationStop::Length);
    let call = one_call(&turn);
    assert_eq!(call.closure, Closure::Dangling);
    assert_eq!(call.name.as_ref().expect("partial name").value, "f");
    let turn = recognize(&mini, input, GenerationStop::Stop);
    assert_eq!(turn.calls().len(), 0, "no call segment");
    assert!(matches!(
        turn.selected()[1],
        Segment::Rejected { span, reason: Rejection::Unterminated, .. } if *span == (5..18)
    ));

    // A length stop inside a multi-byte character keeps the whole characters.
    let input = "<tool><call=f><arg=a>caf\u{e9}".as_bytes();
    let turn = recognize(&mini, &input[..input.len() - 1], GenerationStop::Length);
    assert_eq!(one_call(&turn).arguments.root.to_json(), r#"{"a":"caf"}"#);
    // Malformed UTF-8 in an argument rejects the call, keeping its span.
    let turn = recognize(
        &mini,
        b"<tool><call=f><arg=a>\xFF</arg></call></tool>",
        GenerationStop::Stop,
    );
    assert_eq!(turn.calls().len(), 0, "no call segment");
    assert!(matches!(
        turn.selected()[0],
        Segment::Rejected {
            reason: Rejection::InvalidUtf8 { at: 21 },
            ..
        }
    ));
}

#[test]
fn exhaustion_and_cancellation_explicit() {
    let mini = set(&[MINI]);
    let input: &[u8] = b"<tool><call=f></args></args></call></tool> and <tool><call=g><arg=a>1<arg=b>2</arg></call></tool>";
    let mut ordinary = Governor::new(Budget::default());
    run(
        &mini,
        &[input],
        GenerationStop::Stop,
        ReasoningPolicy::default(),
        &mut ordinary,
    )
    .expect("recognized");
    let usage = ordinary.usage();
    assert!(usage.candidates > 0 && usage.evidence_entries > 0);

    let starve = |budget: Budget, resource: Resource| {
        let mut governor = Governor::new(budget);
        let mut recognizer =
            Recognizer::new(&mini, ReasoningPolicy::default(), &mut governor).expect("recognizer");
        let result = match recognizer.push(input, &mut governor) {
            Ok(()) => recognizer
                .finish(GenerationStop::Stop, &mut governor)
                .map(|_| ()),
            Err(reason) => {
                // Errors are sticky.
                assert_eq!(recognizer.push(b"more", &mut governor), Err(reason.clone()));
                assert_eq!(
                    recognizer
                        .finish(GenerationStop::Stop, &mut governor)
                        .map(|_| ()),
                    Err(reason.clone())
                );
                Err(reason)
            }
        };
        assert_eq!(
            result,
            Err(StopReason::BudgetExceeded(resource)),
            "{resource:?}"
        );
    };
    starve(
        Budget {
            work: usage.work / 2,
            ..Budget::default()
        },
        Resource::Work,
    );
    starve(
        Budget {
            candidates: usage.candidates - 1,
            ..Budget::default()
        },
        Resource::Candidates,
    );
    starve(
        Budget {
            evidence_entries: usage.evidence_entries - 1,
            ..Budget::default()
        },
        Resource::EvidenceEntries,
    );
    starve(
        Budget {
            input_bytes: 20,
            ..Budget::default()
        },
        Resource::InputBytes,
    );
    starve(
        Budget {
            scratch_bytes: usage.peak_scratch_bytes / 2,
            ..Budget::default()
        },
        Resource::ScratchBytes,
    );

    // Cancellation between pushes ends the turn explicitly.
    let token = CancellationToken::new();
    let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
    let mut recognizer =
        Recognizer::new(&mini, ReasoningPolicy::default(), &mut governor).expect("recognizer");
    recognizer
        .push(&input[..10], &mut governor)
        .expect("first chunk");
    token.cancel();
    assert_eq!(
        recognizer.push(&input[10..], &mut governor),
        Err(StopReason::Cancelled)
    );
    assert_eq!(
        recognizer
            .finish(GenerationStop::Stop, &mut governor)
            .map(|_| ()),
        Err(StopReason::Cancelled)
    );
}
