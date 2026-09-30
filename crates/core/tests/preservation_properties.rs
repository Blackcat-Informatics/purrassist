// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Adversarial preservation/replay/ledger property campaign (Task 11).
//!
//! A deterministic in-test LCG (fixed seed per test, not a fuzzing harness)
//! builds Markdown documents whose filler text is tokenized directly from the
//! `CommonMark` 0.31.2 and GFM 0.29.0 example corpora
//! (`tests/data/markdown/*.json`), with injected CRLF/CR line endings, tabs
//! (container continuation indentation), NBSP (word joints), a lazy quote
//! continuation, and nested containers (a list item containing a block
//! quote containing a nested list: "lists in quotes in lists").
//!
//! For every Task 9 Markdown structural op, plus the pre-existing section
//! body replace, list item before/after/replace/remove, task set, table
//! cell/row/column, and reference-definition ops, at every applicable site
//! discovered in each generated document (never a hand-picked shape), and
//! for exact/fuzzy/unified text-edit plans generated from random
//! substrings/diffs, [`verify_patch_properties`] asserts:
//!
//! - **byte preservation**: applying the plan reproduces exactly what an
//!   independent reference splice (sort the edits, concatenate the
//!   untouched gaps and the replacements) would produce -- i.e. every byte
//!   outside a changed range is byte-identical, checked against an oracle
//!   that does not share `Patch::apply`'s own implementation.
//! - **exact undo**: applying the plan's `inverse` to the applied text
//!   restores the original source byte-for-byte (`invert ∘ apply` is the
//!   identity).
//! - **the patch algebra's second law**: composing the plan with its own
//!   freshly computed inverse against the original source yields the
//!   identity patch (`compose(p, invert p)` has zero edits).
//! - **ledger coverage**: applying the plan through a governor produces a
//!   receipt whose `losses` cover *exactly* the plan's changed byte ranges
//!   (sorted, one loss per edit, `loss.original` equal to the exact source
//!   bytes at that range) -- no changed range is unaccounted for and no
//!   loss claims a range outside the edit list.
//!
//! JSON recovery is covered separately in
//! [`json_recovery_retains_input_and_covers_dropped_bytes`]: the exact
//! original (corrupted) input is retained verbatim in the receipt, and
//! every non-whitespace byte range dropped by prose-wrapping extraction is
//! covered by a `json.extract`-tagged loss whose `original` matches those
//! exact source bytes.
//!
//! [`adversarial_inputs_refuse_explicitly_and_never_panic`] additionally
//! drives every Document op and every text-edit free function with
//! deliberately invalid indices, markers, and exhausted budgets, wrapped in
//! `catch_unwind`: every outcome is an explicit `InvalidInput`,
//! `BudgetExceeded`, or (for the `AssistResult`-returning free functions)
//! `NoCandidate` refusal -- never a panic.
//!
//! Every `#[test]` prints its seed and case count (`--nocapture`) and
//! asserts a documented minimum; on any property failure the panic message
//! names the seed and the failing case index.
use purrassist_core::{
    AssistResult, Budget, Governor, Outcome, StopReason,
    contract::Loss,
    edits::{self, EditPlan, TextEdit},
    json::{self, Member, Node, Number, Value},
    markdown::{
        Alignment, Block, BlockKind, Document, ListItemEdit, SectionInsertion, TableColumnEdit,
        TableRowEdit,
    },
    schema::{Schema, recovery::RecoveryRequest},
};
use std::fmt::Write as _;
use std::ops::Range;
use std::panic::{self, UnwindSafe};

// ---------------------------------------------------------------------------
// Deterministic generator
// ---------------------------------------------------------------------------

/// A small, deterministic linear congruential generator (Knuth's MMIX
/// constants). Not cryptographic; matches this crate's other in-test-only
/// generators (see `tests/patch_laws.rs`).
struct Lcg(u64);
impl Lcg {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }
    const fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            usize::try_from(self.next_u64() % u64::try_from(bound).unwrap_or(u64::MAX)).unwrap_or(0)
        }
    }
}

const COMMONMARK_CORPUS: &str = include_str!("data/markdown/commonmark-0.31.2.json");
const GFM_CORPUS: &str = include_str!("data/markdown/gfm-0.29.0.gfm.13.json");
const ASCII_LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// Tokenize the CommonMark/GFM official example corpora into a deterministic
/// pool of plain lowercase words safe to embed as Markdown body text: pure
/// ASCII alphabetic, 3-10 bytes, excluding the JSON literals `true`/`false`/
/// `null` (so JSON-recovery filler never accidentally spells a JSON token).
fn vocabulary() -> Vec<String> {
    let mut words = Vec::new();
    for corpus in [COMMONMARK_CORPUS, GFM_CORPUS] {
        let Value::Array(cases) = json::parse(corpus).expect("corpus must parse").value else {
            panic!("corpus must be a JSON array")
        };
        for case in cases {
            let Some(text) = case.get("markdown").and_then(Node::as_str) else {
                continue;
            };
            for token in text.split(|c: char| !c.is_ascii_alphabetic()) {
                let lower = token.to_ascii_lowercase();
                if (3..=10).contains(&token.len())
                    && !matches!(lower.as_str(), "true" | "false" | "null")
                {
                    words.push(lower);
                }
            }
        }
    }
    words.sort();
    words.dedup();
    assert!(
        words.len() > 150,
        "expected a rich vocabulary seeded from the corpora, got {}",
        words.len()
    );
    words
}

/// Join `min_words..=max_words` vocabulary words with plain spaces, with
/// roughly one joint in six using U+00A0 NO-BREAK SPACE instead: injected
/// NBSP, seeded from the corpora's own words.
fn phrase(rng: &mut Lcg, words: &[String], min_words: usize, max_words: usize) -> String {
    let count = min_words + rng.below(max_words - min_words + 1);
    let mut out = String::new();
    for index in 0..count.max(1) {
        if index > 0 {
            out.push(if rng.below(6) == 0 { '\u{A0}' } else { ' ' });
        }
        out.push_str(&words[rng.below(words.len())]);
    }
    out
}

/// One reserved label per call, monotonically unique within a whole test
/// process, for `add_reference`/`relabel_reference` targets that must not
/// collide with a document's own generated labels.
fn fresh_label(rng: &mut Lcg, prefix: &str) -> String {
    format!("{prefix}{:x}", rng.next_u64())
}

/// Build one richly nested Markdown document: two root sections (the first
/// containing a nested subsection), a bullet list with a task item and one
/// item containing a block quote with a lazy paragraph continuation and its
/// own nested list ("lists in quotes in lists"), a GFM table with a
/// deliberately ragged final row, and a reference definition used in
/// shortcut, collapsed, and full form. Line endings (LF/CRLF/CR) and the
/// nested quote's continuation indentation (two spaces or one tab) vary
/// per call.
fn build_rich_document(rng: &mut Lcg, words: &[String]) -> String {
    let title1 = phrase(rng, words, 1, 3);
    let title2 = phrase(rng, words, 1, 3);
    let para1 = phrase(rng, words, 4, 8);
    let para2 = phrase(rng, words, 4, 8);
    let lazy1 = phrase(rng, words, 2, 4);
    let lazy2 = phrase(rng, words, 2, 4);
    let bullet = b"-+*"[rng.below(3)] as char;
    let indent = if rng.below(3) == 0 { "\t" } else { "  " };
    let n_items = 3 + rng.below(2);
    let mut list = String::new();
    for item in 0..n_items {
        let body = phrase(rng, words, 2, 5);
        if item == 1 {
            let qphrase = phrase(rng, words, 2, 4);
            let n1 = phrase(rng, words, 1, 3);
            let n2 = phrase(rng, words, 1, 3);
            let _ = write!(
                list,
                "{bullet} {body}\n{indent}> {qphrase}\n{indent}lazy continuation: {lazy1}\n{indent}>\n{indent}> - {n1}\n{indent}> - {n2}\n"
            );
        } else if item == n_items - 1 {
            let _ = writeln!(list, "{bullet} [ ] {body}");
        } else {
            let _ = writeln!(list, "{bullet} {body}");
        }
    }
    let cols = 2 + rng.below(2);
    let full_rows = 2 + rng.below(2);
    let mut table = String::new();
    table.push('|');
    for _ in 0..cols {
        let _ = write!(table, " {} |", phrase(rng, words, 1, 2));
    }
    table.push('\n');
    table.push('|');
    for _ in 0..cols {
        table.push_str(" --- |");
    }
    table.push('\n');
    for _ in 0..full_rows {
        table.push('|');
        for _ in 0..cols {
            let _ = write!(table, " {} |", phrase(rng, words, 1, 2));
        }
        table.push('\n');
    }
    // A deliberately ragged final row (fewer cells than the header),
    // exercising Document::replace_table_cell's implicit-cell materialization.
    let _ = writeln!(table, "| {} |", phrase(rng, words, 1, 2));

    let ref_label = fresh_label(rng, "doc-ref-");
    let source = format!(
        "# {title1}\n{para1}\n\n> {lazy1}\n{lazy2}\n\n## Items\n{list}\n{table}\nSee [{ref_label}] and [{ref_label}][] and [x][{ref_label}] things.\n\n# {title2}\n{para2}\n\n[{ref_label}]: /{ref_label} \"{title2}\"\n"
    );
    match rng.below(4) {
        1 => source.replace('\n', "\r\n"),
        2 => source.replace('\n', "\r"),
        _ => source,
    }
}

/// A flat ordered list, deliberately non-sequential, for
/// [`Document::renumber_list`] and ordered-item [`ListItemEdit::Move`].
fn build_ordered_document(rng: &mut Lcg, words: &[String]) -> String {
    let n = 3 + rng.below(4);
    let mut list = String::new();
    for item in 0..n {
        let number = item as u64 * 3 + 1;
        let body = phrase(rng, words, 1, 3);
        let _ = writeln!(list, "{number}. {body}");
    }
    match rng.below(3) {
        1 => list.replace('\n', "\r\n"),
        2 => list.replace('\n', "\r"),
        _ => list,
    }
}

// ---------------------------------------------------------------------------
// Structural site discovery (generic: never a hard-coded index)
// ---------------------------------------------------------------------------

fn collect_list_item_paths(blocks: &[Block], prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (index, block) in blocks.iter().enumerate() {
        prefix.push(index);
        if matches!(block.kind, BlockKind::ListItem { .. }) {
            out.push(prefix.clone());
        }
        collect_list_item_paths(&block.children, prefix, out);
        prefix.pop();
    }
}

fn collect_table_paths(blocks: &[Block], prefix: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
    for (index, block) in blocks.iter().enumerate() {
        prefix.push(index);
        if matches!(block.kind, BlockKind::Table { .. }) {
            out.push(prefix.clone());
        }
        collect_table_paths(&block.children, prefix, out);
        prefix.pop();
    }
}

fn sibling_count(doc: &Document, path: &[usize]) -> usize {
    if path.len() == 1 {
        doc.blocks().len()
    } else {
        doc.block_at(&path[..path.len() - 1])
            .expect("path was discovered from this document")
            .children
            .len()
    }
}

// ---------------------------------------------------------------------------
// Independent oracle and generic patch-law/ledger verification
// ---------------------------------------------------------------------------

/// Reference splice, independent of `Patch::apply`'s own implementation:
/// sort the edits, concatenate every untouched gap with each replacement.
fn naive_apply(source: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|edit| edit.range.start);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for edit in sorted {
        out.push_str(&source[cursor..edit.range.start]);
        out.push_str(&edit.replacement);
        cursor = edit.range.end;
    }
    out.push_str(&source[cursor..]);
    out
}

/// Verify, for one generated `(source, plan)` pair: byte preservation
/// against the independent splice oracle, exact undo, the patch algebra's
/// `compose(p, invert p) == identity` law, and receipt-ledger coverage of
/// every changed byte range. See the module doc comment for the precise
/// definition of each property.
fn verify_patch_properties(source: &str, plan: &EditPlan, label: &str) {
    let mut governor = Governor::new(Budget::default());
    let result = plan.apply_governed(source, &mut governor);
    let Outcome::Resolved(applied) = result.outcome else {
        panic!(
            "{label}: apply must resolve for a freshly planned patch on {source:?}: {:?}",
            result.receipt.stopped
        );
    };
    let expected_text = naive_apply(source, &plan.edits);
    assert_eq!(
        applied.text, expected_text,
        "{label}: bytes outside changed ranges must be byte-identical to an independent splice"
    );

    let Outcome::Resolved(restored) = applied
        .inverse
        .apply(&applied.text, Budget::default())
        .outcome
    else {
        panic!("{label}: inverse must resolve against the applied output");
    };
    assert_eq!(
        restored.text, source,
        "{label}: inverse must restore the exact original (invert . apply = identity)"
    );

    let mut invert_governor = Governor::new(Budget::default());
    let inverse = plan
        .invert(source, &mut invert_governor)
        .unwrap_or_else(|error| panic!("{label}: invert must succeed: {error}"));
    let mut compose_governor = Governor::new(Budget::default());
    let identity = plan
        .compose(&inverse, source, &mut compose_governor)
        .unwrap_or_else(|error| panic!("{label}: compose(p, invert p) must succeed: {error}"));
    assert!(
        identity.edits.is_empty(),
        "{label}: compose(p, invert p) must be the identity patch, got {identity:?}"
    );

    let mut expected_ranges: Vec<Range<usize>> =
        plan.edits.iter().map(|edit| edit.range.clone()).collect();
    expected_ranges.sort_by_key(|range| range.start);
    let mut covered: Vec<Range<usize>> = result
        .receipt
        .losses
        .iter()
        .filter_map(|loss| loss.evidence.span.as_ref().map(|span| span.bytes.clone()))
        .collect();
    covered.sort_by_key(|range| range.start);
    assert_eq!(
        covered, expected_ranges,
        "{label}: receipt losses must cover exactly the changed byte ranges"
    );
    for loss in &result.receipt.losses {
        if let Some(span) = &loss.evidence.span {
            assert_eq!(
                loss.original,
                source[span.bytes.clone()],
                "{label}: loss.original must equal the exact source bytes it names"
            );
        }
    }
}

fn check_result(
    cases: &mut usize,
    source: &str,
    label: &str,
    result: Result<EditPlan, StopReason>,
) {
    match result {
        Ok(plan) => {
            verify_patch_properties(source, &plan, label);
            *cases += 1;
        }
        Err(reason) => panic!("{label} on {source:?} unexpectedly refused: {reason}"),
    }
}

/// Like [`check_result`], but for a reordering op (`Move`) whose target is
/// a structurally valid index yet not always a *representable* one: e.g.
/// relocating a root list item among the document's entire flat top-level
/// block sequence (headings, paragraphs, tables, ...) can place it adjacent
/// to content it would merge or split differently with once reparsed.
/// `Document::edit_list_item`/`edit_table_row`/`edit_table_column`/
/// `move_section` detect exactly that (by reparsing and comparing) and
/// refuse explicitly rather than emit a plan that would not reproduce the
/// intended arrangement -- itself a property this campaign cares about
/// (explicit `InvalidInput`, never a silent wrong answer or a panic), so an
/// explicit refusal here is recorded, not treated as a failure.
fn check_result_or_explicit_refusal(
    cases: &mut usize,
    skipped: &mut usize,
    source: &str,
    label: &str,
    result: Result<EditPlan, StopReason>,
) {
    match result {
        Ok(plan) => {
            verify_patch_properties(source, &plan, label);
            *cases += 1;
        }
        Err(StopReason::InvalidInput(_)) => *skipped += 1,
        Err(reason) => panic!("{label} on {source:?} unexpectedly refused: {reason}"),
    }
}

// ---------------------------------------------------------------------------
// Per-family site exercisers
// ---------------------------------------------------------------------------

fn exercise_sections(
    rng: &mut Lcg,
    words: &[String],
    source: &str,
    doc: &Document,
    tag: &str,
    cases: &mut usize,
    skipped: &mut usize,
) {
    let sections = doc.sections();
    for section in &sections {
        let heading = section.heading;
        check_result(
            cases,
            source,
            &format!("{tag}:rename_heading@{heading}"),
            doc.rename_heading(
                heading,
                &phrase(rng, words, 1, 3),
                &mut Governor::new(Budget::default()),
            ),
        );
        let body = format!("{}\n", phrase(rng, words, 2, 6));
        check_result(
            cases,
            source,
            &format!("{tag}:replace_section@{heading}"),
            doc.replace_section_governed(heading, &body, &mut Governor::new(Budget::default())),
        );
        let insertion = format!(
            "# {}\n{}\n",
            phrase(rng, words, 1, 2),
            phrase(rng, words, 1, 3)
        );
        check_result(
            cases,
            source,
            &format!("{tag}:insert_section_after@{heading}"),
            doc.insert_section(
                SectionInsertion::After(heading),
                &insertion,
                &mut Governor::new(Budget::default()),
            ),
        );
        for other in &sections {
            if other.heading == heading || section.span.contains(&other.span.start) {
                continue;
            }
            check_result_or_explicit_refusal(
                cases,
                skipped,
                source,
                &format!("{tag}:move_section@{heading}->{}", other.heading),
                doc.move_section(
                    heading,
                    SectionInsertion::After(other.heading),
                    None,
                    &mut Governor::new(Budget::default()),
                ),
            );
        }
        check_result_or_explicit_refusal(
            cases,
            skipped,
            source,
            &format!("{tag}:move_section_start@{heading}"),
            doc.move_section(
                heading,
                SectionInsertion::Start,
                None,
                &mut Governor::new(Budget::default()),
            ),
        );
        // remove_section last: it is the most destructive and does not
        // change what any earlier attempt (all against the same immutable
        // `doc`) observed.
        check_result(
            cases,
            source,
            &format!("{tag}:remove_section@{heading}"),
            doc.remove_section(heading, &mut Governor::new(Budget::default())),
        );
    }
    check_result(
        cases,
        source,
        &format!("{tag}:insert_section_start"),
        doc.insert_section(
            SectionInsertion::Start,
            &format!(
                "# {}\n{}\n",
                phrase(rng, words, 1, 2),
                phrase(rng, words, 1, 3)
            ),
            &mut Governor::new(Budget::default()),
        ),
    );
}

fn exercise_list_items(
    rng: &mut Lcg,
    words: &[String],
    source: &str,
    doc: &Document,
    tag: &str,
    cases: &mut usize,
    skipped: &mut usize,
) {
    let mut paths = Vec::new();
    collect_list_item_paths(doc.blocks(), &mut Vec::new(), &mut paths);
    for path in &paths {
        let block = doc.block_at(path).expect("discovered path must resolve");
        let BlockKind::ListItem { checked, .. } = block.kind else {
            unreachable!("collect_list_item_paths only collects ListItem blocks")
        };
        let siblings = sibling_count(doc, path);
        let index = *path.last().expect("path is non-empty");
        let label = format!("{tag}:list@{path:?}");

        check_result(
            cases,
            source,
            &format!("{label}:before"),
            doc.edit_list_item(
                path,
                &ListItemEdit::Before {
                    marker: "-".into(),
                    body: phrase(rng, words, 1, 3),
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result(
            cases,
            source,
            &format!("{label}:after"),
            doc.edit_list_item(
                path,
                &ListItemEdit::After {
                    marker: "-".into(),
                    body: phrase(rng, words, 1, 3),
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result(
            cases,
            source,
            &format!("{label}:replace"),
            doc.edit_list_item(
                path,
                &ListItemEdit::Replace {
                    body: phrase(rng, words, 1, 3),
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        if siblings > 1 {
            let mut to = rng.below(siblings);
            if to == index {
                to = (to + 1) % siblings;
            }
            check_result_or_explicit_refusal(
                cases,
                skipped,
                source,
                &format!("{label}:move->{to}"),
                doc.edit_list_item(
                    path,
                    &ListItemEdit::Move { to },
                    &mut Governor::new(Budget::default()),
                ),
            );
        }
        if let Some(current) = checked {
            check_result(
                cases,
                source,
                &format!("{label}:task_toggle"),
                doc.set_task_at(path, !current, &mut Governor::new(Budget::default())),
            );
            check_result(
                cases,
                source,
                &format!("{label}:task_same"),
                doc.set_task_at(path, current, &mut Governor::new(Budget::default())),
            );
        }
        check_result(
            cases,
            source,
            &format!("{label}:remove"),
            doc.edit_list_item(
                path,
                &ListItemEdit::Remove,
                &mut Governor::new(Budget::default()),
            ),
        );
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one straight-line sequence exercising every table cell/row/column \
              op at one discovered site; splitting it would only move these \
              same sequential checks across an arbitrary function boundary"
)]
fn exercise_tables(
    rng: &mut Lcg,
    words: &[String],
    source: &str,
    doc: &Document,
    tag: &str,
    cases: &mut usize,
    skipped: &mut usize,
) {
    let mut paths = Vec::new();
    collect_table_paths(doc.blocks(), &mut Vec::new(), &mut paths);
    for path in &paths {
        let block = doc.block_at(path).expect("discovered path must resolve");
        let BlockKind::Table { header, rows, .. } = &block.kind else {
            unreachable!("collect_table_paths only collects Table blocks")
        };
        let width = header.len();
        let row_count = rows.len();
        let label = format!("{tag}:table@{path:?}");

        check_result(
            cases,
            source,
            &format!("{label}:cell_header"),
            doc.replace_table_cell(
                path,
                0,
                rng.below(width),
                &phrase(rng, words, 1, 2),
                &mut Governor::new(Budget::default()),
            ),
        );
        if row_count >= 2 {
            let row = 1 + rng.below(row_count - 1);
            let column = rng.below(width);
            check_result(
                cases,
                source,
                &format!("{label}:cell_body@{row},{column}"),
                doc.replace_table_cell(
                    path,
                    row,
                    column,
                    &phrase(rng, words, 1, 2),
                    &mut Governor::new(Budget::default()),
                ),
            );
        }
        // The final row is always deliberately ragged (see
        // build_rich_document): row = row_count selects it, materializing
        // every implicit cell up to and including the last column.
        check_result(
            cases,
            source,
            &format!("{label}:cell_materialize"),
            doc.replace_table_cell(
                path,
                row_count,
                width - 1,
                &phrase(rng, words, 1, 2),
                &mut Governor::new(Budget::default()),
            ),
        );

        let insert_index = rng.below(row_count + 1);
        check_result(
            cases,
            source,
            &format!("{label}:row_insert@{insert_index}"),
            doc.edit_table_row(
                path,
                &TableRowEdit::Insert {
                    index: insert_index,
                    cells: (0..width).map(|_| phrase(rng, words, 1, 2)).collect(),
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result(
            cases,
            source,
            &format!("{label}:row_remove"),
            doc.edit_table_row(
                path,
                &TableRowEdit::Remove {
                    index: row_count - 1,
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result_or_explicit_refusal(
            cases,
            skipped,
            source,
            &format!("{label}:row_move"),
            doc.edit_table_row(
                path,
                &TableRowEdit::Move {
                    index: 0,
                    to: row_count - 1,
                },
                &mut Governor::new(Budget::default()),
            ),
        );

        let column_insert_index = rng.below(width + 1);
        check_result(
            cases,
            source,
            &format!("{label}:column_insert@{column_insert_index}"),
            doc.edit_table_column(
                path,
                &TableColumnEdit::Insert {
                    index: column_insert_index,
                    header: phrase(rng, words, 1, 2),
                    alignment: Alignment::Left,
                    cells: (0..row_count).map(|_| phrase(rng, words, 1, 2)).collect(),
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        let alignment = [
            Alignment::Default,
            Alignment::Left,
            Alignment::Right,
            Alignment::Center,
        ][rng.below(4)];
        check_result(
            cases,
            source,
            &format!("{label}:column_align"),
            doc.edit_table_column(
                path,
                &TableColumnEdit::Align {
                    index: rng.below(width),
                    alignment,
                },
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result(
            cases,
            source,
            &format!("{label}:column_remove"),
            doc.edit_table_column(
                path,
                &TableColumnEdit::Remove { index: width - 1 },
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result_or_explicit_refusal(
            cases,
            skipped,
            source,
            &format!("{label}:column_move"),
            doc.edit_table_column(
                path,
                &TableColumnEdit::Move {
                    index: 0,
                    to: width - 1,
                },
                &mut Governor::new(Budget::default()),
            ),
        );
    }
}

fn exercise_references(
    rng: &mut Lcg,
    words: &[String],
    source: &str,
    doc: &Document,
    tag: &str,
    cases: &mut usize,
) {
    let sites = doc
        .reference_sites(&mut Governor::new(Budget::default()))
        .unwrap_or_else(|error| panic!("{tag}: reference_sites must succeed: {error}"));
    for site in &sites {
        let label = format!("{tag}:reference@{:?}#{}", site.path, site.occurrence);
        let destination = format!("/{}", fresh_label(rng, "dest-"));
        let title = phrase(rng, words, 1, 2);
        check_result(
            cases,
            source,
            &format!("{label}:replace"),
            doc.replace_reference(
                &site.path,
                site.occurrence,
                &destination,
                Some(&title),
                &mut Governor::new(Budget::default()),
            ),
        );
        let new_label = fresh_label(rng, "relabel-");
        check_result(
            cases,
            source,
            &format!("{label}:relabel"),
            doc.relabel_reference(
                &site.path,
                site.occurrence,
                &new_label,
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result(
            cases,
            source,
            &format!("{label}:remove"),
            doc.remove_reference(
                &site.path,
                site.occurrence,
                &mut Governor::new(Budget::default()),
            ),
        );
        check_result(
            cases,
            source,
            &format!("{label}:unlink"),
            doc.unlink_reference(
                &site.path,
                site.occurrence,
                &mut Governor::new(Budget::default()),
            ),
        );
    }
    let added_label = fresh_label(rng, "added-");
    let destination = format!("/{added_label}");
    check_result(
        cases,
        source,
        &format!("{tag}:add_reference"),
        doc.add_reference(
            &added_label,
            &destination,
            None,
            &mut Governor::new(Budget::default()),
        ),
    );
}

// ---------------------------------------------------------------------------
// Markdown structural op properties
// ---------------------------------------------------------------------------

const SEED_RICH_DOCS: u64 = 0xA11C_E5EE_D0C5_0001;
const RICH_DOC_COUNT: usize = 260;
const MINIMUM_RICH_CASES: usize = 8_000;

#[test]
fn markdown_structural_ops_preserve_bytes_and_ledgers() {
    let words = vocabulary();
    let mut rng = Lcg::new(SEED_RICH_DOCS);
    let mut cases = 0usize;
    let mut skipped = 0usize;
    let mut parsed = 0usize;
    for doc_index in 0..RICH_DOC_COUNT {
        let source = build_rich_document(&mut rng, &words);
        let mut governor = Governor::new(Budget::default());
        let Ok(doc) = Document::parse(&source, &mut governor) else {
            continue;
        };
        parsed += 1;
        let tag = format!("seed={SEED_RICH_DOCS:#x} doc#{doc_index}");
        exercise_sections(
            &mut rng,
            &words,
            &source,
            &doc,
            &tag,
            &mut cases,
            &mut skipped,
        );
        exercise_list_items(
            &mut rng,
            &words,
            &source,
            &doc,
            &tag,
            &mut cases,
            &mut skipped,
        );
        exercise_tables(
            &mut rng,
            &words,
            &source,
            &doc,
            &tag,
            &mut cases,
            &mut skipped,
        );
        exercise_references(&mut rng, &words, &source, &doc, &tag, &mut cases);
    }
    eprintln!(
        "markdown_structural_ops_preserve_bytes_and_ledgers: {parsed}/{RICH_DOC_COUNT} documents parsed, {cases} cases, {skipped} explicit move refusals (seed={SEED_RICH_DOCS:#x})"
    );
    assert!(
        cases >= MINIMUM_RICH_CASES,
        "seed={SEED_RICH_DOCS:#x}: only {cases} cases executed across {parsed} documents"
    );
}

// ---------------------------------------------------------------------------
// Ordered-list renumbering and move
// ---------------------------------------------------------------------------

const SEED_ORDERED_DOCS: u64 = 0x0EDE_0000_0000_0002;
const ORDERED_DOC_COUNT: usize = 400;
const MINIMUM_ORDERED_CASES: usize = 1_000;

#[test]
fn ordered_list_renumber_and_move_preserve_bytes_and_ledgers() {
    let words = vocabulary();
    let mut rng = Lcg::new(SEED_ORDERED_DOCS);
    let mut cases = 0usize;
    let mut skipped = 0usize;
    for doc_index in 0..ORDERED_DOC_COUNT {
        let source = build_ordered_document(&mut rng, &words);
        let mut governor = Governor::new(Budget::default());
        let Ok(doc) = Document::parse(&source, &mut governor) else {
            continue;
        };
        let mut paths = Vec::new();
        collect_list_item_paths(doc.blocks(), &mut Vec::new(), &mut paths);
        let tag = format!("seed={SEED_ORDERED_DOCS:#x} doc#{doc_index}");
        for path in &paths {
            let siblings = sibling_count(&doc, path);
            let index = *path.last().expect("path is non-empty");
            let start = 1 + rng.below(5) as u64;
            check_result(
                &mut cases,
                &source,
                &format!("{tag}:renumber@{path:?}"),
                doc.renumber_list(path, start, &mut Governor::new(Budget::default())),
            );
            if siblings > 1 {
                let mut to = rng.below(siblings);
                if to == index {
                    to = (to + 1) % siblings;
                }
                check_result_or_explicit_refusal(
                    &mut cases,
                    &mut skipped,
                    &source,
                    &format!("{tag}:move@{path:?}->{to}"),
                    doc.edit_list_item(
                        path,
                        &ListItemEdit::Move { to },
                        &mut Governor::new(Budget::default()),
                    ),
                );
            }
        }
    }
    eprintln!(
        "ordered_list_renumber_and_move_preserve_bytes_and_ledgers: {cases} cases, {skipped} explicit move refusals (seed={SEED_ORDERED_DOCS:#x})"
    );
    assert!(
        cases >= MINIMUM_ORDERED_CASES,
        "seed={SEED_ORDERED_DOCS:#x}: only {cases} cases executed"
    );
}

// ---------------------------------------------------------------------------
// Exact / fuzzy / unified text-edit plans
// ---------------------------------------------------------------------------

fn random_substring_range(
    rng: &mut Lcg,
    source: &str,
    min_chars: usize,
    max_chars: usize,
) -> Option<Range<usize>> {
    let boundaries: Vec<usize> = source
        .char_indices()
        .map(|(offset, _)| offset)
        .chain([source.len()])
        .collect();
    if boundaries.len() < 2 {
        return None;
    }
    let last_char = boundaries.len() - 1;
    let start_char = rng.below(last_char);
    let available = last_char - start_char;
    if available < min_chars {
        return None;
    }
    let length = min_chars + rng.below(max_chars.min(available) - min_chars + 1);
    Some(boundaries[start_char]..boundaries[start_char + length])
}

fn build_unified_case(rng: &mut Lcg, words: &[String]) -> (String, String) {
    let line_count = 4 + rng.below(6);
    let lines: Vec<String> = (0..line_count).map(|_| phrase(rng, words, 2, 6)).collect();
    let mut text = String::new();
    for line in &lines {
        let _ = writeln!(text, "{line}");
    }
    let target = rng.below(line_count);
    let new_line = phrase(rng, words, 1, 5);
    let patch = format!(
        "--- a/example\n+++ b/example\n@@ -{line},1 +{line},1 @@\n-{old}\n+{new_line}\n",
        line = target + 1,
        old = lines[target],
    );
    let source = match rng.below(3) {
        1 => text.replace('\n', "\r\n"),
        _ => text,
    };
    (source, patch)
}

const SEED_EDIT_PLANS: u64 = 0xED17_5EED_0000_0003;
const EXACT_CASE_COUNT: usize = 1500;
const FUZZY_CASE_COUNT: usize = 1500;
const UNIFIED_CASE_COUNT: usize = 1500;
const MINIMUM_EDIT_CASES: usize = 3_500;

#[test]
fn exact_fuzzy_and_unified_edit_plans_preserve_bytes_and_ledgers() {
    let words = vocabulary();
    let mut rng = Lcg::new(SEED_EDIT_PLANS);
    let mut cases = 0usize;
    let mut skipped = 0usize;

    for case in 0..EXACT_CASE_COUNT {
        let source = phrase(&mut rng, &words, 6, 20);
        let Some(range) = random_substring_range(&mut rng, &source, 1, 8) else {
            skipped += 1;
            continue;
        };
        let needle = source[range].to_string();
        let replacement = phrase(&mut rng, &words, 1, 4);
        let result = edits::locate(&source, &needle, &replacement, Budget::default());
        let label = format!("seed={SEED_EDIT_PLANS:#x} exact#{case}");
        match result.outcome {
            Outcome::Resolved(plan) => {
                verify_patch_properties(&source, &plan, &label);
                cases += 1;
            }
            Outcome::Ambiguous(plans) => {
                for (index, plan) in plans.iter().enumerate() {
                    verify_patch_properties(&source, plan, &format!("{label}.{index}"));
                    cases += 1;
                }
            }
            other => panic!(
                "{label}: expected a resolved or ambiguous plan, got {other:?}: {:?}",
                result.receipt.stopped
            ),
        }
    }

    for case in 0..FUZZY_CASE_COUNT {
        let source = phrase(&mut rng, &words, 6, 20);
        let Some(range) = random_substring_range(&mut rng, &source, 2, 8) else {
            skipped += 1;
            continue;
        };
        let mut chars: Vec<char> = source[range].chars().collect();
        let position = rng.below(chars.len());
        chars[position] = ASCII_LETTERS[rng.below(ASCII_LETTERS.len())] as char;
        let needle: String = chars.into_iter().collect();
        let replacement = phrase(&mut rng, &words, 1, 4);
        let result = edits::locate_fuzzy(&source, &needle, &replacement, 1, Budget::default());
        let label = format!("seed={SEED_EDIT_PLANS:#x} fuzzy#{case}");
        match result.outcome {
            Outcome::Resolved(plan) | Outcome::BestGuess(plan) => {
                verify_patch_properties(&source, &plan, &label);
                cases += 1;
            }
            Outcome::Ambiguous(plans) => {
                for (index, plan) in plans.iter().enumerate() {
                    verify_patch_properties(&source, plan, &format!("{label}.{index}"));
                    cases += 1;
                }
            }
            Outcome::Unresolved => skipped += 1,
        }
    }

    for case in 0..UNIFIED_CASE_COUNT {
        let (source, patch) = build_unified_case(&mut rng, &words);
        let result = edits::plan_unified(&source, &patch, Budget::default());
        let label = format!("seed={SEED_EDIT_PLANS:#x} unified#{case}");
        match result.outcome {
            Outcome::Resolved(plan) => {
                verify_patch_properties(&source, &plan, &label);
                cases += 1;
            }
            Outcome::Ambiguous(plans) => {
                for (index, plan) in plans.iter().enumerate() {
                    verify_patch_properties(&source, plan, &format!("{label}.{index}"));
                    cases += 1;
                }
            }
            other => panic!(
                "{label}: expected a resolved or ambiguous plan, got {other:?}: {:?}",
                result.receipt.stopped
            ),
        }
    }

    eprintln!(
        "exact_fuzzy_and_unified_edit_plans_preserve_bytes_and_ledgers: {cases} cases, {skipped} skipped (seed={SEED_EDIT_PLANS:#x})"
    );
    assert!(
        cases >= MINIMUM_EDIT_CASES,
        "seed={SEED_EDIT_PLANS:#x}: only {cases} cases executed ({skipped} skipped)"
    );
}

// ---------------------------------------------------------------------------
// JSON recovery: original retained, dropped bytes covered by losses
// ---------------------------------------------------------------------------

fn random_json_scalar(rng: &mut Lcg, words: &[String]) -> Node {
    match rng.below(4) {
        0 => Node::new(Value::Null),
        1 => Node::new(Value::Bool(rng.below(2) == 0)),
        2 => Node::new(Value::Number(Number::from_unsigned(rng.next_u64() % 1000))),
        _ => Node::new(Value::String(words[rng.below(words.len())].clone())),
    }
}

/// A flat array or object of scalars only: the JSON text this produces
/// contains exactly one bracket pair (its own), so wrapping it in sentinel
/// prose cannot create a second, ambiguous, syntactically valid JSON region.
fn random_json_root(rng: &mut Lcg, words: &[String]) -> Node {
    let count = 1 + rng.below(3);
    if rng.below(2) == 0 {
        Node::new(Value::Array(
            (0..count).map(|_| random_json_scalar(rng, words)).collect(),
        ))
    } else {
        Node::new(Value::Object(
            (0..count)
                .map(|index| Member {
                    key: format!("k{index}"),
                    key_span: None,
                    value: random_json_scalar(rng, words),
                })
                .collect(),
        ))
    }
}

/// Assert that every non-whitespace dropped byte range (the sentinel-wrapped
/// prose either side of the embedded JSON, which we placed at exactly
/// `prefix.len()..prefix.len() + json_text.len()` since we built the
/// corrupted input by concatenation) is covered by a `json.extract`-tagged
/// loss whose `original` matches those exact bytes.
///
/// `json::recover_syntax` consumes trailing whitespace immediately after
/// the value it parses (ordinary parser behavior, not extraction), so the
/// suffix range recovery actually reports as dropped starts just past that
/// whitespace, not at the literal end of `json_text`; this mirrors that by
/// skipping it before comparing, without weakening the "every non-whitespace
/// dropped byte is covered" property itself.
fn assert_extraction_losses_cover_dropped_bytes(
    corrupted: &str,
    prefix_len: usize,
    json_len: usize,
    losses: &[Loss],
    label: &str,
) {
    let suffix_unadjusted = prefix_len + json_len;
    let whitespace_skip = corrupted[suffix_unadjusted..]
        .find(|c: char| !c.is_whitespace())
        .unwrap_or(corrupted.len() - suffix_unadjusted);
    let suffix_start = suffix_unadjusted + whitespace_skip;
    for range in [0..prefix_len, suffix_start..corrupted.len()] {
        if corrupted[range.clone()].trim().is_empty() {
            continue;
        }
        let found = losses.iter().any(|loss| {
            loss.evidence.rule == "json.extract"
                && loss
                    .evidence
                    .span
                    .as_ref()
                    .is_some_and(|span| span.bytes == range)
                && loss.original == corrupted[range.clone()]
        });
        assert!(
            found,
            "{label}: expected a json.extract loss covering {range:?} of {corrupted:?}"
        );
    }
}

const SEED_JSON_RECOVERY: u64 = 0x9502_0000_0000_0004;
const JSON_CASE_COUNT: usize = 900;
const MINIMUM_JSON_CASES: usize = 600;

#[test]
fn json_recovery_retains_input_and_covers_dropped_bytes() {
    let words = vocabulary();
    let mut rng = Lcg::new(SEED_JSON_RECOVERY);
    let schema = Schema::compile("true").expect("the permissive boolean schema always compiles");
    let mut cases = 0usize;
    for case in 0..JSON_CASE_COUNT {
        let node = random_json_root(&mut rng, &words);
        let json_text = node.to_json();
        // A leading '@' cannot start any JSON token (object, array, string,
        // number, or the true/false/null literals), so recovery's own
        // multi-start extraction search cannot mistake the sentinel prose
        // for JSON content starting at offset 0.
        let prefix = format!("@@@ {} ", phrase(&mut rng, &words, 2, 4));
        let suffix = format!(" {} @@@", phrase(&mut rng, &words, 2, 4));
        let corrupted = format!("{prefix}{json_text}{suffix}");
        let result = schema.recover(&corrupted, RecoveryRequest::default());
        let label = format!("seed={SEED_JSON_RECOVERY:#x} json#{case}");
        assert!(
            result
                .receipt
                .sources
                .iter()
                .any(|document| document.text == corrupted),
            "{label}: the exact original (corrupted) input must be retained verbatim in the receipt"
        );
        match result.outcome {
            Outcome::Resolved(recovered) => {
                assert!(
                    recovered.equivalent(&node),
                    "{label}: recovered value must equal the embedded JSON"
                );
                assert_extraction_losses_cover_dropped_bytes(
                    &corrupted,
                    prefix.len(),
                    json_text.len(),
                    &result.receipt.losses,
                    &label,
                );
                cases += 1;
            }
            Outcome::Ambiguous(nodes) => {
                assert_eq!(nodes.len(), result.receipt.alternatives.len());
                for (index, (value, alternative)) in
                    nodes.iter().zip(&result.receipt.alternatives).enumerate()
                {
                    assert!(
                        value.equivalent(&node),
                        "{label}.{index}: ambiguous alternative must equal the embedded JSON"
                    );
                    let mut losses = result.receipt.losses.clone();
                    losses.extend(alternative.losses.clone());
                    assert_extraction_losses_cover_dropped_bytes(
                        &corrupted,
                        prefix.len(),
                        json_text.len(),
                        &losses,
                        &format!("{label}.{index}"),
                    );
                    cases += 1;
                }
            }
            other => panic!(
                "{label}: expected recovery to resolve the embedded JSON, got {other:?}: {:?}",
                result.receipt.stopped
            ),
        }
    }
    eprintln!(
        "json_recovery_retains_input_and_covers_dropped_bytes: {cases} cases (seed={SEED_JSON_RECOVERY:#x})"
    );
    assert!(
        cases >= MINIMUM_JSON_CASES,
        "seed={SEED_JSON_RECOVERY:#x}: only {cases} cases executed"
    );
}

// ---------------------------------------------------------------------------
// Refusals are explicit, never panics
// ---------------------------------------------------------------------------

#[expect(
    clippy::match_wild_err_arm,
    reason = "catch_unwind's Err carries an opaque `Box<dyn Any + Send>` panic \
              payload with no variant worth naming; every reachable case here is \
              already handled explicitly by the Ok(..) arms"
)]
fn check_refusal(
    label: &str,
    attempt: impl FnOnce() -> Result<EditPlan, StopReason> + UnwindSafe,
) -> usize {
    match panic::catch_unwind(attempt) {
        // An adversarial-looking input that happened to be legal, or an
        // explicit refusal, is not a failure.
        Ok(Ok(_) | Err(StopReason::InvalidInput(_) | StopReason::BudgetExceeded(_))) => 1,
        Ok(Err(other)) => panic!("{label}: unexpected refusal kind {other:?}"),
        Err(_) => panic!("{label}: operation panicked instead of returning an explicit refusal"),
    }
}

#[expect(
    clippy::match_wild_err_arm,
    reason = "catch_unwind's Err carries an opaque `Box<dyn Any + Send>` panic \
              payload with no variant worth naming; every reachable case here is \
              already handled explicitly by the Ok(..) arms"
)]
fn check_refusal_assist<T: std::fmt::Debug>(
    label: &str,
    attempt: impl FnOnce() -> AssistResult<T> + UnwindSafe,
) -> usize {
    match panic::catch_unwind(attempt) {
        Ok(result) => match result.outcome {
            Outcome::Unresolved => match result.receipt.stopped {
                StopReason::InvalidInput(_)
                | StopReason::BudgetExceeded(_)
                | StopReason::NoCandidate => 1,
                other => panic!("{label}: unexpected stop reason {other:?}"),
            },
            _ => 1,
        },
        Err(_) => panic!("{label}: operation panicked instead of returning an explicit refusal"),
    }
}

const SEED_ADVERSARIAL: u64 = 0xADCE_5EED_0000_0005;
const ADVERSARIAL_DOC_COUNT: usize = 300;
const ADVERSARIAL_FREE_FUNCTION_ROUNDS: usize = 300;
const MINIMUM_ADVERSARIAL_CASES: usize = 2_000;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line sequence of independent adversarial attempts, \
              each three lines; splitting it would only move these same \
              sequential checks across an arbitrary function boundary"
)]
fn adversarial_inputs_refuse_explicitly_and_never_panic() {
    let words = vocabulary();
    let mut rng = Lcg::new(SEED_ADVERSARIAL);
    let mut cases = 0usize;
    for doc_index in 0..ADVERSARIAL_DOC_COUNT {
        let source = build_rich_document(&mut rng, &words);
        let mut governor = Governor::new(Budget::default());
        let Ok(doc) = Document::parse(&source, &mut governor) else {
            continue;
        };
        let tag = format!("seed={SEED_ADVERSARIAL:#x} doc#{doc_index}");
        cases += check_refusal(&format!("{tag}:rename_heading_missing"), || {
            doc.rename_heading(usize::MAX, "x", &mut Governor::new(Budget::default()))
        });
        cases += check_refusal(&format!("{tag}:remove_section_missing"), || {
            doc.remove_section(usize::MAX, &mut Governor::new(Budget::default()))
        });
        cases += check_refusal(&format!("{tag}:insert_section_missing_target"), || {
            doc.insert_section(
                SectionInsertion::After(usize::MAX),
                "# x\ny\n",
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:edit_list_item_missing"), || {
            doc.edit_list_item(
                &[usize::MAX],
                &ListItemEdit::Remove,
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:edit_list_item_bad_marker"), || {
            doc.edit_list_item(
                &[0],
                &ListItemEdit::Before {
                    marker: "??".into(),
                    body: "x".into(),
                },
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:renumber_list_missing"), || {
            doc.renumber_list(&[usize::MAX], 1, &mut Governor::new(Budget::default()))
        });
        cases += check_refusal(&format!("{tag}:replace_table_cell_missing"), || {
            doc.replace_table_cell(
                &[usize::MAX],
                usize::MAX,
                usize::MAX,
                "x",
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:edit_table_row_missing"), || {
            doc.edit_table_row(
                &[usize::MAX],
                &TableRowEdit::Remove { index: usize::MAX },
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:edit_table_column_missing"), || {
            doc.edit_table_column(
                &[usize::MAX],
                &TableColumnEdit::Remove { index: usize::MAX },
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:relabel_reference_missing"), || {
            doc.relabel_reference(&[usize::MAX], 0, "x", &mut Governor::new(Budget::default()))
        });
        cases += check_refusal(&format!("{tag}:add_reference_bad_label"), || {
            doc.add_reference(
                "has[bracket]",
                "/x",
                None,
                &mut Governor::new(Budget::default()),
            )
        });
        cases += check_refusal(&format!("{tag}:rename_heading_zero_work"), || {
            doc.rename_heading(
                0,
                "x",
                &mut Governor::new(Budget {
                    work: 0,
                    ..Budget::default()
                }),
            )
        });
        cases += check_refusal(&format!("{tag}:remove_section_zero_scratch"), || {
            doc.remove_section(
                0,
                &mut Governor::new(Budget {
                    scratch_bytes: 0,
                    ..Budget::default()
                }),
            )
        });
        cases += check_refusal(&format!("{tag}:add_reference_zero_work"), || {
            doc.add_reference(
                "fresh",
                "/x",
                None,
                &mut Governor::new(Budget {
                    work: 0,
                    ..Budget::default()
                }),
            )
        });
    }
    for round in 0..ADVERSARIAL_FREE_FUNCTION_ROUNDS {
        let source = phrase(&mut rng, &words, 4, 10);
        let tag = format!("seed={SEED_ADVERSARIAL:#x} free#{round}");
        cases += check_refusal_assist(&format!("{tag}:locate_empty_needle"), || {
            edits::locate(&source, "", "x", Budget::default())
        });
        cases += check_refusal_assist(&format!("{tag}:locate_fuzzy_bad_bound"), || {
            edits::locate_fuzzy(&source, "ab", "x", 5, Budget::default())
        });
        cases += check_refusal_assist(&format!("{tag}:plan_unified_not_a_diff"), || {
            edits::plan_unified(&source, "not a diff", Budget::default())
        });
        cases += check_refusal_assist(&format!("{tag}:locate_zero_work"), || {
            edits::locate(
                &source,
                "a",
                "b",
                Budget {
                    work: 0,
                    ..Budget::default()
                },
            )
        });
    }
    eprintln!(
        "adversarial_inputs_refuse_explicitly_and_never_panic: {cases} cases (seed={SEED_ADVERSARIAL:#x})"
    );
    assert!(
        cases >= MINIMUM_ADVERSARIAL_CASES,
        "seed={SEED_ADVERSARIAL:#x}: only {cases} cases executed"
    );
}
