// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Single-source acceptance transcript shared through the harness Cargo library.
//! Native, WASI and browser-WASM callers exercise the actual core engines.
use purrassist_core::{
    Budget, CancellationToken, Effort, Governor, Outcome, StopReason,
    contract::Resource,
    digest::Digest,
    edits::EditPlan,
    embeddings::{Embedding, EmbeddingIndex, EmbeddingMatch, EmbeddingPolicy},
    json::Node,
    markdown::{
        Alignment, Dialect, Document, ListItemEdit, References, TableColumnEdit, TableRowEdit,
    },
    matching::{self, Candidate, MatchRequest, RankedMatch},
    paths::{PathIndex, PathMatch},
    schema::{Schema, recovery::RecoveryRequest},
    text::{self, Normalization},
    tool_call::{
        DialectId, DialectSet, Effects, Emission, EmitTarget, ObjectSpec, Param, ToolDefinition,
        ToolRecoveryRequest, ToolSet,
    },
    wire::{WireValue, array, integer, object, string},
};

/// Build the exact JSON transcript exercised by every portable target.
///
/// ```
/// let json = wasm_harness::transcript_json();
/// assert!(purrassist_core::json::parse(&json).is_ok());
/// ```
///
/// # Panics
/// Panics if a fixture's internal invariant fails. This is a qualification
/// fixture, not a production recovery operation.
#[must_use]
pub fn transcript_json() -> String {
    let cancellation = CancellationToken::new();
    let cancelled = Governor::with_cancellation(Budget::default(), cancellation.clone());
    cancellation.cancel();
    assert_eq!(cancelled.check(), Err(StopReason::Cancelled));
    let schema = Schema::compile(r#"{"type":"object","required":["temperature","drink"],"properties":{"temperature":{"type":"number"},"drink":{"enum":["tea","coffee"]}},"additionalProperties":false}"#).expect("schema");
    let mut transcript = Vec::new();
    for source in [
        "```json\n[[{temperatur: '42', drink: 'Tea',}]]\n```",
        r#"{"temperature":42,"drink":"tea"}"#,
        r#"{"temperature":1,"temperature":2,"drink":"tea"}"#,
    ] {
        let result = schema.recover(source, RecoveryRequest::default());
        let (status, values) = match result.outcome {
            Outcome::Resolved(node) => ("resolved", vec![node]),
            Outcome::BestGuess(node) => ("best_guess", vec![node]),
            Outcome::Ambiguous(nodes) => ("ambiguous", nodes),
            Outcome::Unresolved => ("unresolved", Vec::new()),
        };
        transcript.push(object([
            ("source", string(result.receipt.source.to_string())),
            ("status", string(status)),
            ("values", array(values)),
            (
                "rules",
                array(
                    result
                        .receipt
                        .evidence
                        .iter()
                        .map(|entry| string(&entry.rule)),
                ),
            ),
            ("termination", string(result.receipt.stopped.to_string())),
        ]));
    }
    let mut governor = Governor::new(Effort::Standard.budget());
    let distance = matching::damerau_levenshtein("CA", "ABC", &mut governor).expect("distance");
    assert_eq!(distance, 2);
    assert_eq!(text::comparison_key("Straße"), "strasse");
    transcript.push(string(text::normalize("\u{212b}각ﬃ", Normalization::Nfkc)));
    transcript.push(matching_transcript());
    transcript.push(string_selection_transcript());
    transcript.push(embedding_transcript());
    transcript.push(path_transcript());
    transcript.push(markdown_transcript());
    transcript.push(list_transcript());
    transcript.push(reference_transcript());
    transcript.push(tool_call_transcript());
    array(transcript).to_json()
}

/// Tool-call recovery over every built-in dialect: a near-miss name, a
/// stray closer, reasoning, and invalid UTF-8, with the complete result and
/// receipt (whose charges must be identical on every target), then, for
/// each shipped dialect alone, an anchor turn (canonical markup with typed,
/// nested, and verbatim arguments) and a repair turn, each recovered with
/// emission to the `OpenAI` `tool_calls` array. Every emitted array's digest
/// is pinned, so a target that recovers or prints differently fails here
/// before the transcripts are even compared.
fn tool_call_transcript() -> Node {
    let tools = transcript_tools();
    let mut output = b"<think>plan</think>Reading \xff.\n".to_vec();
    output.extend_from_slice(
        b"<tool_call>\n{\"name\": \"read_fiel\", \"arguments\": {\"path\": \"a.rs\", \"limit\": 5}}\n</tool_call>\n\
          <tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</parameter>\n</function>\n</tool_call>",
    );
    let result = tools.recover(&output, &ToolRecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
    let mut dialects = Vec::new();
    for case in DIALECT_CASES {
        let set = DialectSet::new(
            [case.dialect.clone()],
            &mut Governor::new(Budget::default()),
        )
        .expect("one built-in dialect");
        let mut turns = Vec::new();
        for (kind, turn, status, pinned) in [
            ("anchor", case.anchor, "resolved", case.anchor_digest),
            (
                "repair",
                case.repair,
                case.repair_status,
                case.repair_digest,
            ),
        ] {
            let request = ToolRecoveryRequest::default()
                .dialects(set.clone())
                .emit(EmitTarget::OpenAiToolCalls);
            let result = tools.recover(turn.as_bytes(), &request);
            let (actual, calls) = match &result.outcome {
                Outcome::Resolved(calls) => ("resolved", calls),
                Outcome::BestGuess(calls) => ("best_guess", calls),
                other => panic!("{} {kind}: no single reading: {other:?}", case.dialect.id()),
            };
            assert_eq!(actual, status, "{} {kind}", case.dialect.id());
            let usable = calls.usable_accepting_guesses().len();
            assert_eq!(usable, calls.calls.len(), "{} {kind}", case.dialect.id());
            let Some(Emission::OpenAiToolCalls(emitted)) = &calls.emitted else {
                panic!("{} {kind}: no OpenAI emission", case.dialect.id())
            };
            assert_eq!(emitted.calls.len(), usable, "{} {kind}", case.dialect.id());
            let array = emitted.output.to_json();
            let digest = Digest::of(array.as_bytes()).to_string();
            assert_eq!(digest, pinned, "{} {kind}: {array}", case.dialect.id());
            turns.push(object([
                ("kind", string(kind)),
                ("status", string(status)),
                ("openai_tool_calls", string(array)),
                ("openai_tool_calls_digest", string(digest)),
                ("result", result.to_node()),
            ]));
        }
        dialects.push(object([
            ("dialect", string(case.dialect.id())),
            ("turns", array(turns)),
        ]));
    }
    object([
        ("operation", string("tool_call.recover")),
        ("result", result.to_node()),
        ("dialects", array(dialects)),
    ])
}

/// Read-only and mutating tools with string, integer, number, boolean,
/// enumeration, array, and nested object parameters.
fn transcript_tools() -> ToolSet {
    let owner = ObjectSpec::new()
        .param("name", Param::string())
        .param("uid", Param::integer());
    ToolSet::compile(
        [
            ToolDefinition::new("read_file")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required())
                .param("limit", Param::integer()),
            ToolDefinition::new("list_dir")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required()),
            ToolDefinition::new("get_weather")
                .effects(Effects::ReadOnly)
                .param("city", Param::string().required())
                .param("unit", Param::enumeration(&["celsius", "fahrenheit"])),
            ToolDefinition::new("write_file")
                .effects(Effects::Mutating)
                .param("path", Param::string().required())
                .param("content", Param::string().required())
                .param("mode", Param::enumeration(&["create", "overwrite"]))
                .param("ratio", Param::number())
                .param("count", Param::integer())
                .param("tags", Param::array(Param::string()))
                .param(
                    "options",
                    Param::object(
                        ObjectSpec::new()
                            .param("backup", Param::boolean())
                            .param("retries", Param::integer())
                            .param("owner", Param::object(owner)),
                    ),
                ),
        ],
        Budget::default(),
    )
    .expect("tools")
}

/// One shipped dialect's transcript turns and their pinned emissions.
struct DialectCase {
    dialect: DialectId,
    /// Canonical markup, as in the dialect's conformance anchors.
    anchor: &'static str,
    anchor_digest: &'static str,
    /// Markup that needs a repair.
    repair: &'static str,
    repair_status: &'static str,
    repair_digest: &'static str,
}

/// One case per entry of `DialectId::BUILT_IN` (checked by the test below).
const DIALECT_CASES: [DialectCase; 5] = [
    DialectCase {
        dialect: DialectId::Hermes,
        anchor: "<think>\nWrite the file.\n</think>\n\n<tool_call>\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"src/main.rs\", \"content\": \"fn main() {\\n    println!(\\\"hi\\\");\\n\\n\\tlet  x = 1;  \\n}\\n\", \"mode\": \"create\", \"ratio\": 2.50, \"count\": 1e3, \"tags\": [], \"options\": {\"backup\": false, \"retries\": 0, \"owner\": {\"name\": \"ops\", \"uid\": -7}}}}\n</tool_call>\n<tool_call>\n{\"arguments\": {\"path\": \"notes.txt\", \"limit\": 10}, \"name\": \"read_file\"}\n</tool_call>",
        anchor_digest: "sha256:c7f907179005d6a43506b78ea504a6eaa0c929332c9acdf90dec10c6a4ad2efb",
        repair: "Sure, here it is: <tool_call>Calling now: {\"name\": \"get_wether\", \"arguments\": {\"city\": \"Oslo\", \"unit\": \"Celsius\"}}</tool_call></tool_call>",
        repair_status: "best_guess",
        repair_digest: "sha256:81c28b0d74c3bcec95b1bf93265af004a5cc5d06b73784badf28689b815f03fb",
    },
    DialectCase {
        dialect: DialectId::Llama3Json,
        anchor: "<|python_tag|>{\n    \"type\": \"function\",\n    \"name\": \"write_file\",\n    \"parameters\": {\"path\": \"src/main.rs\", \"content\": \"fn main() {\\n    println!(\\\"hi\\\");\\n\\n\\tlet  x = 1;  \\n}\\n\", \"mode\": \"create\", \"ratio\": 2.50, \"count\": 1e3, \"tags\": [], \"options\": {\"backup\": false, \"retries\": 0, \"owner\": {\"name\": \"ops\", \"uid\": -7}}}\n}<|eom_id|>",
        anchor_digest: "sha256:75fba62908be6c306288dd21581b4eed33b1a518efbc445c8788647bff527e4a",
        repair: "{\"name\": \"read_file\", \"parameters\": {\"path\": \"src/lib.rs\", \"limit\": 40}\n{\"name\": \"get_weather\", \"parameters\": {\"city\": \"Oslo\", \"unit\": \"celsius\"}}}",
        repair_status: "resolved",
        repair_digest: "sha256:cf64b7392f46122a2ad98a32099dff59275c4aea1410b023f9446709a0729b6b",
    },
    DialectCase {
        dialect: DialectId::MistralV3,
        anchor: "[TOOL_CALLS] [{\"name\": \"write_file\", \"arguments\": {\"path\": \"src/main.rs\", \"content\": \"fn main() {\\n    println!(\\\"hi\\\");\\n\\n\\tlet  x = 1;  \\n}\\n\", \"mode\": \"create\", \"ratio\": 2.50, \"count\": 1e3, \"tags\": [], \"options\": {\"backup\": false, \"retries\": 0, \"owner\": {\"name\": \"ops\", \"uid\": -7}}}, \"id\": \"a1B2c3D4e\"}, {\"name\": \"read_file\", \"arguments\": {\"path\": \"notes.txt\", \"limit\": 10}, \"id\": \"f5G6h7J8k\"}]</s>",
        anchor_digest: "sha256:30b33a48978dbd2ed81606045cc3fd6d1edd77cff066419fc73415b0118805b1",
        repair: "[TOOL_CALLS] [{\"name\": \"read_file\", \"arguments\": {\"path\": \"src/lib.rs\", \"limit\": 40}, \"id\": \"Wq7kP2xZr\"}, {\"name\": \"get_weather\", \"arguments\": {\"city\": \"Oslo\", \"unit\": \"celsius\"}",
        repair_status: "resolved",
        repair_digest: "sha256:33cfb6f0a503abbf4bc0e6fb3ec96c7bc7a284327bde9a8374e49e1170990c10",
    },
    DialectCase {
        dialect: DialectId::OpenAiToolCalls,
        anchor: "{\"role\": \"assistant\", \"content\": null, \"tool_calls\": [{\"id\": \"call_1\", \"type\": \"function\", \"function\": {\"name\": \"get_weather\", \"arguments\": \"{\\\"city\\\":\\\"Lima\\\",\\\"unit\\\":\\\"fahrenheit\\\"}\"}}, {\"id\": \"call_2\", \"type\": \"function\", \"function\": {\"name\": \"read_file\", \"arguments\": \"{\\\"path\\\":\\\"src\\\",\\\"limit\\\":3}\"}}]}",
        anchor_digest: "sha256:9a7f6e37d469b42c1c8c56eae3c6bbc43538a7c468e0f8665d05e2b1a6e9b2be",
        repair: "[{\"id\": \"call_9\", \"type\": \"function\", \"function\": {\"name\": \"list_dri\", \"arguments\": \"{\\\"path\\\": \\\"src\\\"}\"}}]",
        repair_status: "best_guess",
        repair_digest: "sha256:206148a554d9576b8d4d22226de181a2c38077d9220b8ab8fec32b99ee363a48",
    },
    DialectCase {
        dialect: DialectId::Qwen3CoderXml,
        anchor: "<think>\nThe file needs a main function.\n</think>\n\n<tool_call>\n<function=write_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n<parameter=content>\nfn main() {\n    println!(\"hi\");\n\n\tlet  x = 1;  \n}\n\n</parameter>\n<parameter=mode>\noverwrite\n</parameter>\n<parameter=ratio>\n1.50\n</parameter>\n<parameter=count>\n-0\n</parameter>\n<parameter=tags>\n[\"a\", \"b c\"]\n</parameter>\n<parameter=options>\n<parameter=backup>\ntrue\n</parameter>\n<parameter=retries>\n3\n</parameter>\n<parameter=owner>\n{\"name\": \"ci\", \"uid\": 1000}\n</parameter>\n</parameter>\n</function>\n</tool_call>",
        anchor_digest: "sha256:1ab02681843a1102e50b307617d017f8fd0a4340bfeff52e561b512a38ea81c3",
        repair: "<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/lib.rs\n<parameter=limit>\n40\n</parameters>\n</parameters>\n</function>\n<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</function>",
        repair_status: "resolved",
        repair_digest: "sha256:474d182638a95680a7a182ef47a1ccd610af7404896b99feee3f4cee996ea9bb",
    },
];

fn matching_transcript() -> Node {
    let candidate = Candidate {
        id: "tea".into(),
        text: "cold tea".into(),
        aliases: vec!["iced tea".into()],
    };
    let result = matching::rank("iced tea", &[candidate], &MatchRequest::default());
    let Outcome::Resolved(values) = result.outcome else {
        panic!("expected alias ranking")
    };
    assert_eq!(values[0].candidate.id, "tea");
    assert!(
        values[0]
            .signals
            .iter()
            .any(|signal| signal.method == "caller-alias-v1")
    );
    object([
        ("operation", string("matching")),
        ("score", integer(u64::from(values[0].score))),
        ("receipt", result.receipt.to_node()),
    ])
}

fn string_selection_transcript() -> Node {
    let candidates = [
        Candidate::new("first", "reader"),
        Candidate::new("second", "reader"),
    ];
    let result = matching::resolve(
        "reader",
        &candidates,
        &MatchRequest::default(),
        matching::SelectionPolicy {
            minimum_score: 10_000,
            minimum_margin: 0,
        },
    );
    let Outcome::Ambiguous(values) = result.outcome else {
        panic!("expected tied exact strings")
    };
    for (value, ledger) in values.iter().zip(&result.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(value.to_node().to_json().as_bytes())
        );
    }
    object([
        ("operation", string("string-selection")),
        ("values", array(values.iter().map(RankedMatch::to_node))),
        ("receipt", result.receipt.to_node()),
    ])
}

fn path_transcript() -> Node {
    let request = MatchRequest::default();
    let mut governor = Governor::new(request.budget);
    let index = PathIndex::new(
        ["docs/README.md".into(), "other/README.md".into()],
        &mut governor,
    )
    .expect("paths");
    let answer = index.resolve_governed(
        "README.md",
        "",
        &request,
        purrassist_core::paths::SelectionPolicy::default(),
        &mut governor,
    );
    let Outcome::Ambiguous(values) = answer.outcome else {
        panic!("expected ambiguous path alternatives")
    };
    assert_eq!(answer.receipt.alternatives.len(), 2);
    for (value, ledger) in values.iter().zip(&answer.receipt.alternatives) {
        assert_eq!(
            ledger.output,
            Digest::of(value.to_node().to_json().as_bytes())
        );
        assert_eq!(ledger.inferences[0].value, value.path);
    }
    object([
        ("operation", string("paths")),
        ("values", array(values.iter().map(PathMatch::to_node))),
        ("receipt", answer.receipt.to_node()),
    ])
}

fn embedding_transcript() -> Node {
    let mut g = Governor::new(Budget::default());
    let index = EmbeddingIndex::new(
        "portable-v1",
        &[
            Embedding {
                id: "large".into(),
                vector: vec![1e308, 5e307, -2e307],
            },
            Embedding {
                id: "small".into(),
                vector: vec![-1e-308, 2e-308, 3e-308],
            },
        ],
        &mut g,
    )
    .expect("index");
    let mut results = Vec::new();
    for (query, expected) in [([0.5, 0.25, -0.1], "large"), ([-0.1, 0.2, 0.3], "small")] {
        let result = index
            .select(&query, EmbeddingPolicy::default(), &mut g)
            .expect("query");
        assert!(matches!(&result, Outcome::BestGuess(value) if value.id == expected));
        results.push(embedding_outcome(result));
    }
    let tied = EmbeddingIndex::new(
        "ties-v1",
        &[
            Embedding {
                id: "first".into(),
                vector: vec![1.0, 0.0],
            },
            Embedding {
                id: "second".into(),
                vector: vec![2.0, 0.0],
            },
        ],
        &mut g,
    )
    .expect("tied index");
    let result = tied
        .select(&[1.0, 0.0], EmbeddingPolicy::default(), &mut g)
        .expect("ties");
    assert!(
        matches!(&result, Outcome::Ambiguous(values) if values.len() == 2 && values[0].id == "first" && values[1].id == "second")
    );
    results.push(embedding_outcome(result));
    let weak = tied
        .select(&[0.0, 1.0], EmbeddingPolicy::default(), &mut g)
        .expect("weak query");
    assert_eq!(weak, Outcome::Unresolved);
    results.push(embedding_outcome(weak));
    let mut exhausted = Governor::new(Budget {
        candidates: 1,
        ..Budget::default()
    });
    let failure = tied
        .select(&[1.0, 0.0], EmbeddingPolicy::default(), &mut exhausted)
        .expect_err("complete-search budget");
    assert_eq!(failure, StopReason::BudgetExceeded(Resource::Candidates));
    results.push(string(failure.to_string()));
    let invalid = tied
        .select(&[0.0, 0.0], EmbeddingPolicy::default(), &mut g)
        .expect_err("zero vector");
    assert!(matches!(invalid, StopReason::InvalidInput(_)));
    results.push(string(invalid.to_string()));
    object([
        ("operation", string("embeddings")),
        ("results", array(results)),
    ])
}

fn embedding_outcome(value: Outcome<EmbeddingMatch>) -> Node {
    let (status, values) = match value {
        Outcome::Resolved(_) => panic!("numerical matches cannot prove semantic intent"),
        Outcome::BestGuess(value) => ("best_guess", vec![value]),
        Outcome::Ambiguous(values) => ("ambiguous", values),
        Outcome::Unresolved => ("unresolved", Vec::new()),
    };
    object([
        ("status", string(status)),
        (
            "values",
            array(values.into_iter().map(|value| {
                object([
                    ("id", string(value.id)),
                    // No epsilon or rounding masks target-specific floating-point results.
                    ("score_bits", integer(value.score.to_bits())),
                ])
            })),
        ),
    ])
}

fn section_transcript(newline: &str, results: &mut Vec<Node>) {
    let nested_source =
        format!("> - # Tea{newline}>   Hot{newline}>   # Keep{newline}>   Tail{newline}");
    let mut nested_g = Governor::new(Budget::default());
    let nested = Document::parse(&nested_source, &mut nested_g).expect("nested section");
    let site = &nested.section_sites(&mut nested_g).expect("section sites")[0];
    assert_eq!(site.path, [0, 0, 0]);
    let nested_body = format!("Cold café.{newline}## Child{newline}Ready{newline}");
    let nested_plan = nested
        .replace_section_at(&site.path, &nested_body, &mut nested_g)
        .expect("nested plan");
    let nested_output = apply_edit(&nested_plan, &nested_source, results);
    assert_eq!(
        nested_output,
        format!(
            "> - # Tea{newline}>   Cold café.{newline}>   ## Child{newline}>   Ready{newline}>   # Keep{newline}>   Tail{newline}"
        )
    );
    let section_source = format!("# Tea{newline}Old{newline}# Keep{newline}Tail");
    let mut section_g = Governor::new(Budget::default());
    let section = Document::parse(&section_source, &mut section_g).expect("section");
    let body = format!("Cold café.{newline}## Child{newline}Ready{newline}");
    let section_plan = section
        .replace_section_governed(0, &body, &mut section_g)
        .expect("section plan");
    let edited = apply_edit(&section_plan, &section_source, results);
    assert_eq!(edited, format!("# Tea{newline}{body}# Keep{newline}Tail"));
    assert!(
        section
            .replace_section_governed(0, "```\n", &mut section_g)
            .is_err()
    );
}

fn markdown_transcript() -> Node {
    let mut results = Vec::new();
    for newline in ["\n", "\r\n", "\r"] {
        section_transcript(newline, &mut results);
        let source = format!("> | Drink | State |{newline}> | --- | --- |{newline}> | tea | hot |");
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).expect("table");
        let plan = doc
            .replace_table_cell(&[0, 0], 1, 1, "**iced**", &mut g)
            .expect("cell");
        let updated = apply_edit(&plan, &source, &mut results);
        assert_eq!(updated, source.replace("hot", "**iced**"));
        let doc = Document::parse(&updated, &mut g).expect("updated table");
        let plan = doc
            .edit_table_row(
                &[0, 0],
                &TableRowEdit::Insert {
                    index: 1,
                    cells: vec!["café".into(), "a\\|b".into()],
                },
                &mut g,
            )
            .expect("insert row");
        let inserted = apply_edit(&plan, &updated, &mut results);
        assert_eq!(inserted, format!("{updated}{newline}> | café | a\\|b |"));
        let doc = Document::parse(&inserted, &mut g).expect("inserted table");
        results.push(string(
            doc.render_html(Dialect::Gfm, &mut g).expect("render"),
        ));
        let plan = doc
            .edit_table_row(&[0, 0], &TableRowEdit::Remove { index: 0 }, &mut g)
            .expect("remove row");
        let removed = apply_edit(&plan, &inserted, &mut results);
        assert!(!removed.contains("tea"));
        assert!(removed.contains("café"));
        let doc = Document::parse(&removed, &mut g).expect("remaining table");
        let plan = doc
            .edit_table_column(
                &[0, 0],
                &TableColumnEdit::Insert {
                    index: 0,
                    header: "Origin".into(),
                    alignment: Alignment::Left,
                    cells: vec!["caller".into()],
                },
                &mut g,
            )
            .expect("insert column");
        let widened = apply_edit(&plan, &removed, &mut results);
        assert!(widened.contains("| Origin | Drink | State |"));
        let doc = Document::parse(&widened, &mut g).expect("widened table");
        let plan = doc
            .edit_table_column(
                &[0, 0],
                &TableColumnEdit::Align {
                    index: 0,
                    alignment: Alignment::Right,
                },
                &mut g,
            )
            .expect("align column");
        let aligned = apply_edit(&plan, &widened, &mut results);
        let doc = Document::parse(&aligned, &mut g).expect("aligned table");
        let plan = doc
            .edit_table_column(&[0, 0], &TableColumnEdit::Remove { index: 0 }, &mut g)
            .expect("remove column");
        assert_eq!(apply_edit(&plan, &aligned, &mut results), removed);
    }
    let source = "> - [ ] café\r\n>   - [X] child\r\n";
    let mut g = Governor::new(Budget::default());
    let doc = Document::parse(source, &mut g).expect("tasks");
    let plan = doc
        .set_task_at(&[0, 0, 1], false, &mut g)
        .expect("nested task");
    assert_eq!(
        apply_edit(&plan, source, &mut results),
        source.replace("[X]", "[ ]")
    );
    object([
        ("operation", string("markdown")),
        ("results", array(results)),
    ])
}

fn apply_edit(plan: &EditPlan, source: &str, transcript: &mut Vec<Node>) -> String {
    let result = plan.apply(source, Budget::default());
    let Outcome::Resolved(value) = result.outcome else {
        panic!("apply failed")
    };
    let undone = value.inverse.apply(&value.text, Budget::default());
    let Outcome::Resolved(original) = undone.outcome else {
        panic!("undo failed")
    };
    assert_eq!(original.text, source);
    let stale = plan.apply(&format!("{source}changed"), Budget::default());
    assert_eq!(stale.outcome, Outcome::Unresolved);
    transcript.push(object([
        ("text", string(&value.text)),
        ("receipt", result.receipt.to_node()),
        ("inverse_receipt", undone.receipt.to_node()),
        ("stale_receipt", stale.receipt.to_node()),
    ]));
    value.text
}

fn reference_transcript() -> Node {
    let mut results = Vec::new();
    let many = "[tea]: /hot\n".repeat(1024);
    let mut budgeted = Governor::new(Budget::default());
    let indexed =
        References::parse_blocks(&[&many], &mut budgeted).expect("linear reference budget");
    assert_eq!(indexed.definitions().len(), 1024);
    results.push(object([(
        "many_reference_work",
        integer(budgeted.usage().work),
    )]));
    let mut g = Governor::new(Budget::default());
    let source = "> [Tea]: /hot\r\n> [tea]: /shadow\r\n>\r\n> [TEA]\r\n";
    let doc = Document::parse(source, &mut g).expect("references");
    let sites = doc.reference_sites(&mut g).expect("reference sites");
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].path, [0, 0]);
    assert_eq!(&source[sites[0].destination_span.clone()], "/hot");
    let plan = doc
        .replace_reference(&sites[0].path, 0, "/iced", Some("iced\ntea"), &mut g)
        .expect("reference edit");
    let output = apply_edit(&plan, source, &mut results);
    let updated = Document::parse(&output, &mut g).expect("updated references");
    let sites = updated.reference_sites(&mut g).expect("updated sites");
    assert_eq!(sites[0].title.as_deref(), Some("iced\ntea"));
    assert_eq!(sites[1].destination, "/shadow");
    results.push(string(
        updated
            .render_html(Dialect::Gfm, &mut g)
            .expect("reference rendering"),
    ));
    object([
        ("operation", string("reference definitions")),
        ("results", array(results)),
    ])
}

fn list_transcript() -> Node {
    let mut results = Vec::new();
    for newline in ["\n", "\r\n", "\r"] {
        let source = format!("> - hot{newline}> - keep{newline}");
        let mut g = Governor::new(Budget::default());
        let doc = Document::parse(&source, &mut g).expect("list");
        let plan = doc
            .edit_list_item(
                &[0, 0],
                &ListItemEdit::After {
                    marker: "-".into(),
                    body: "[x] iced café".into(),
                },
                &mut g,
            )
            .expect("insert item");
        let inserted = apply_edit(&plan, &source, &mut results);
        assert_eq!(
            inserted,
            format!("> - hot{newline}> - [x] iced café{newline}> - keep{newline}")
        );
        let doc = Document::parse(&inserted, &mut g).expect("inserted list");
        let removed = doc
            .edit_list_item(&[0, 1], &ListItemEdit::Remove, &mut g)
            .expect("remove item");
        assert_eq!(apply_edit(&removed, &inserted, &mut results), source);
    }
    object([
        ("operation", string("list items")),
        ("results", array(results)),
    ])
}

#[cfg(test)]
mod tests {
    use super::{DIALECT_CASES, DialectId};

    #[test]
    fn every_shipped_dialect_has_a_case() {
        let covered: Vec<_> = DIALECT_CASES
            .iter()
            .map(|case| case.dialect.clone())
            .collect();
        assert_eq!(covered, DialectId::BUILT_IN);
    }
}
