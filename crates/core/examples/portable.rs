// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Deterministic native/WASI acceptance transcript using the actual core engines.
use purrassist_core::{
    Budget, Effort, Governor, Outcome, StopReason,
    contract::Resource,
    digest::Digest,
    edits::EditPlan,
    embeddings::{Embedding, EmbeddingIndex, EmbeddingMatch, EmbeddingPolicy},
    json::Node,
    markdown::{Alignment, Dialect, Document, References, TableColumnEdit, TableRowEdit},
    matching,
    schema::{Schema, recovery::RecoveryRequest},
    text::{self, Normalization},
    wire::{array, integer, object, string},
};

fn main() {
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
    transcript.push(embedding_transcript());
    transcript.push(markdown_transcript());
    transcript.push(reference_transcript());
    let output = array(transcript).to_json();
    println!("{}", Digest::of(output.as_bytes()));
    println!("{output}");
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

fn markdown_transcript() -> Node {
    let mut results = Vec::new();
    for newline in ["\n", "\r\n", "\r"] {
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
