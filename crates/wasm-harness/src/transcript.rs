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
    tool_call::{Effects, Param, ToolDefinition, ToolRecoveryRequest, ToolSet},
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
/// receipt (whose charges must be identical on every target).
fn tool_call_transcript() -> Node {
    let tools = ToolSet::compile(
        [
            ToolDefinition::new("read_file")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required())
                .param("limit", Param::integer()),
            ToolDefinition::new("list_dir")
                .effects(Effects::ReadOnly)
                .param("path", Param::string().required()),
        ],
        Budget::default(),
    )
    .expect("tools");
    let mut output = b"<think>plan</think>Reading \xff.\n".to_vec();
    output.extend_from_slice(
        b"<tool_call>\n{\"name\": \"read_fiel\", \"arguments\": {\"path\": \"a.rs\", \"limit\": 5}}\n</tool_call>\n\
          <tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</parameter>\n</function>\n</tool_call>",
    );
    let result = tools.recover(&output, &ToolRecoveryRequest::default());
    assert!(matches!(result.outcome, Outcome::BestGuess(_)));
    object([
        ("operation", string("tool_call.recover")),
        ("result", result.to_node()),
    ])
}

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
