// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Syntax adapters for intent-aware source editing.
//!
//! Parser success proves grammar acceptance, not type checking or program behavior.
//! Tree-sitter allocations are external to the core governor; input admission,
//! progress callbacks, tree traversal, and candidate counts are bounded here.
use purrassist_core::{
    AssistResult, Budget, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Resource, Validation},
    digest::Digest,
    edits::{AppliedEdit, EditPlan, TextEdit},
    provenance::{Derivation, Forest},
};
use std::ops::{ControlFlow, Range};

/// The isolated, pinned grammar family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    /// Python grammar.
    Python,
    /// Rust grammar.
    Rust,
    /// JavaScript including JSX.
    JavaScript,
    /// TypeScript.
    TypeScript,
    /// TypeScript including JSX.
    Tsx,
}
impl Language {
    /// Exact parser and grammar identities included in validation receipts.
    #[must_use]
    pub const fn identity(self) -> &'static str {
        match self {
            Self::Python => "tree-sitter@0.27.0/python@0.25.0",
            Self::Rust => "tree-sitter@0.27.0/rust@0.24.2",
            Self::JavaScript => "tree-sitter@0.27.0/javascript@0.25.0",
            Self::TypeScript => "tree-sitter@0.27.0/typescript@0.23.2",
            Self::Tsx => "tree-sitter@0.27.0/tsx@0.23.2",
        }
    }
    fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}
/// A concrete parser error or missing grammar token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxIssue {
    /// Original byte range, empty for a missing token.
    pub span: Range<usize>,
    /// Grammar token or error node kind.
    pub kind: String,
    /// Whether the parser inserted this token during error recovery.
    pub missing: bool,
}
/// A named declaration with exact source selection boundaries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// Grammar node kind.
    pub kind: String,
    /// Declared spelling.
    pub name: String,
    /// Complete declaration source.
    pub span: Range<usize>,
    /// Body source when the grammar identifies one.
    pub body: Option<Range<usize>>,
}
/// Source-bound parser evidence and declaration selections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// Input source identity.
    pub source: Digest,
    /// Pinned parser and grammar identity.
    pub parser: &'static str,
    /// Whether there are no error or missing-token nodes.
    pub valid: bool,
    /// Parser errors, retaining their exact ranges.
    pub issues: Vec<SyntaxIssue>,
    /// Named declarations in source traversal order.
    pub symbols: Vec<Symbol>,
}
/// Parse a supported language without executing it.
///
/// # Errors
/// Reports incompatible grammar, cancelled/exhausted parsing, or traversal limits.
pub fn inspect(
    source: &str,
    language: Language,
    governor: &mut Governor,
) -> Result<Report, StopReason> {
    governor.charge(Resource::InputBytes, size(source.len()))?;
    governor.charge(Resource::Work, size(source.len()))?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.grammar())
        .map_err(|e| StopReason::InvalidInput(e.to_string()))?;
    let mut stopped = None;
    let mut progress = |_: &tree_sitter::ParseState| match governor.charge(Resource::Work, 100) {
        Ok(()) => ControlFlow::Continue(()),
        Err(reason) => {
            stopped = Some(reason);
            ControlFlow::Break(())
        }
    };
    let options = tree_sitter::ParseOptions::new().progress_callback(&mut progress);
    let tree = parser.parse_with_options(
        &mut |offset, _| source.as_bytes().get(offset..).unwrap_or(&[]),
        None,
        Some(options),
    );
    if let Some(reason) = stopped {
        return Err(reason);
    }
    let tree =
        tree.ok_or_else(|| StopReason::InvalidInput("syntax parser returned no tree".into()))?;
    let root = tree.root_node();
    let mut report = Report {
        source: Digest::of(source.as_bytes()),
        parser: language.identity(),
        valid: !root.has_error(),
        issues: Vec::new(),
        symbols: Vec::new(),
    };
    let mut stack = vec![(root, 0_u32)];
    while let Some((node, depth)) = stack.pop() {
        governor.charge(Resource::Work, 1)?;
        governor.charge(Resource::Depth, u64::from(depth))?;
        if node.is_error() || node.is_missing() {
            governor.charge(Resource::EvidenceEntries, 1)?;
            report.issues.push(SyntaxIssue {
                span: node.byte_range(),
                kind: node.kind().into(),
                missing: node.is_missing(),
            });
            report.valid = false;
        }
        if let Some(span) = mismatched_jsx(node, source) {
            governor.charge(Resource::EvidenceEntries, 1)?;
            report.issues.push(SyntaxIssue {
                span,
                kind: "jsx.mismatched-tags.v1".into(),
                missing: false,
            });
            report.valid = false;
        }
        if let Some(name) = node.child_by_field_name("name")
            && declaration(node.kind())
        {
            governor.charge(Resource::EvidenceEntries, 1)?;
            report.symbols.push(Symbol {
                kind: node.kind().into(),
                name: source[name.byte_range()].into(),
                span: node.byte_range(),
                body: node
                    .child_by_field_name("body")
                    .map(|body| body.byte_range()),
            });
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push((child, depth + 1));
            }
        }
    }
    Ok(report)
}
fn mismatched_jsx(node: tree_sitter::Node<'_>, source: &str) -> Option<Range<usize>> {
    if node.kind() != "jsx_element" {
        return None;
    }
    let opening = node.child_by_field_name("open_tag")?;
    let closing = node.child_by_field_name("close_tag")?;
    let name = |tag: tree_sitter::Node<'_>| {
        tag.child_by_field_name("name")
            .map(|name| &source[name.byte_range()])
    };
    let same = match (name(opening), name(closing)) {
        (None, None) => true, // Fragment <>...</>.
        (Some(left), Some(right)) => left
            .chars()
            .filter(|character| !character.is_whitespace())
            .eq(right.chars().filter(|character| !character.is_whitespace())),
        _ => false,
    };
    (!same).then(|| closing.byte_range())
}
fn declaration(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "class_definition"
            | "function_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "mod_item"
            | "function_declaration"
            | "class_declaration"
            | "method_definition"
            | "interface_declaration"
            | "type_alias_declaration"
    )
}
/// Apply a source-bound edit, then validate its grammar.
///
/// A single parser-indicated
/// missing punctuation token inside the changed region may be repaired, but only
/// when reparsing accepts the complete source. Multiple valid repairs stay ambiguous.
///
/// This performs no disk writes. The returned inverse restores the exact original.
#[must_use]
pub fn apply_checked(
    source: &str,
    plan: &EditPlan,
    language: Language,
    budget: Budget,
) -> AssistResult<AppliedEdit> {
    apply_checked_governed(source, plan, language, &mut Governor::new(budget))
}

/// Apply and syntax-validate using one caller-owned governor.
///
/// Cancellation and exhaustion return `Unresolved` with cumulative usage. No
/// disk writes occur; the caller decides whether to commit a resolved result.
///
/// ```
/// use purrassist_core::{Budget, Governor, Outcome, StopReason, edits::EditPlan, digest::Digest};
/// use purrassist_syntax::{apply_checked_governed, Language};
/// let mut governor = Governor::new(Budget::default());
/// governor.cancel();
/// let plan = EditPlan { source: Digest::of(b""), edits: vec![] };
/// let result = apply_checked_governed("", &plan, Language::Rust, &mut governor);
/// assert!(matches!(result.outcome, Outcome::Unresolved));
/// assert_eq!(result.receipt.stopped, StopReason::Cancelled);
/// ```
#[must_use]
pub fn apply_checked_governed(
    source: &str,
    plan: &EditPlan,
    language: Language,
    governor: &mut Governor,
) -> AssistResult<AppliedEdit> {
    let mut result = plan.apply_governed(source, governor);
    result.receipt.operation = "syntax.apply.v1".into();
    let Outcome::Resolved(applied) = &result.outcome else {
        return result;
    };
    let operation = checked(
        source,
        plan,
        applied,
        language,
        governor,
        &mut result.receipt,
    );
    result.outcome = match operation {
        Ok(outcome) => outcome,
        Err(reason) => {
            result.receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    result.receipt.usage = governor.usage();
    result
}
fn checked(
    source: &str,
    plan: &EditPlan,
    applied: &AppliedEdit,
    language: Language,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Outcome<AppliedEdit>, StopReason> {
    let report = inspect(&applied.text, language, governor)?;
    validation(receipt, &report);
    if report.valid {
        return Ok(Outcome::Resolved(applied.clone()));
    }
    let mut accepted: Vec<(AppliedEdit, Evidence)> = Vec::new();
    let changed = changed_ranges(plan);
    for issue in &report.issues {
        if !issue.missing
            || !matches!(issue.kind.as_str(), ";" | ":" | ")" | "]" | "}" | ",")
            || !changed
                .iter()
                .any(|range| range.start <= issue.span.start && issue.span.start <= range.end)
        {
            continue;
        }
        governor.charge(Resource::Candidates, 1)?;
        let repair = EditPlan {
            source: report.source,
            edits: vec![TextEdit {
                range: issue.span.clone(),
                replacement: issue.kind.clone(),
            }],
        };
        let repaired = repair.apply_governed(&applied.text, governor);
        let Outcome::Resolved(repaired) = repaired.outcome else {
            return Err(repaired.receipt.stopped);
        };
        let validation_report = inspect(&repaired.text, language, governor)?;
        validation(receipt, &validation_report);
        if validation_report.valid {
            let inverse = EditPlan {
                source: Digest::of(repaired.text.as_bytes()),
                edits: vec![TextEdit {
                    range: 0..repaired.text.len(),
                    replacement: source.into(),
                }],
            };
            let evidence = Evidence {
                rule: "syntax.missing-punctuation.v1".into(),
                explanation: format!(
                    "insert parser-indicated {:?} at byte {} and revalidate",
                    issue.kind, issue.span.start
                ),
                span: None,
                destination: format!("bytes:{}", issue.span.start),
            };
            accepted.push((
                AppliedEdit {
                    text: repaired.text,
                    inverse,
                },
                evidence,
            ));
        }
    }
    Ok(match accepted.len() {
        0 => {
            receipt.stopped = StopReason::NoCandidate;
            Outcome::Unresolved
        }
        1 => {
            let (answer, evidence) = accepted.remove(0);
            receipt.evidence.push(evidence);
            Outcome::Resolved(answer)
        }
        // Every accepted repair inserted a different parser-indicated token
        // at a different byte offset, but publish through the derivation
        // forest anyway so any evidence that happens to be identical across
        // repairs (rather than the ordinarily-distinguishing byte offset) is
        // promoted once instead of duplicated into every alternative.
        _ => populate_repair_alternatives(accepted, governor, receipt)?,
    })
}
/// Build one derivation per accepted repair and publish shared vs.
/// per-repair evidence through the derivation forest.
///
/// # Errors
/// Returns `BudgetExceeded` before any ledger is committed to `receipt`.
fn populate_repair_alternatives(
    accepted: Vec<(AppliedEdit, Evidence)>,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Outcome<AppliedEdit>, StopReason> {
    let mut forest = Forest::new(governor);
    let mut derivations = Vec::with_capacity(accepted.len());
    let mut outputs = Vec::with_capacity(accepted.len());
    for (answer, evidence) in accepted {
        let output = Digest::of(answer.to_node().to_json().as_bytes());
        let mut derivation = Derivation::new(output, 0);
        derivation.push(forest.evidence(governor, evidence)?);
        derivations.push(derivation);
        outputs.push(answer);
    }
    let published = forest.publish(&derivations, governor)?;
    // `receipt.evidence` already carries the original edit plan's own
    // evidence (from `EditPlan::apply_governed`, called before this repair
    // search); extend rather than replace it.
    receipt.evidence.extend(published.evidence);
    receipt.alternatives = published.alternatives;
    Ok(Outcome::Ambiguous(outputs))
}
fn validation(receipt: &mut Receipt, report: &Report) {
    receipt.validations.push(Validation {
        validator: report.parser.into(),
        constraint: report.source.to_string(),
        passed: report.valid,
        diagnostics: report
            .issues
            .iter()
            .map(|issue| {
                format!(
                    "{} at {:?}; missing={}",
                    issue.kind, issue.span, issue.missing
                )
            })
            .collect(),
    });
}
fn changed_ranges(plan: &EditPlan) -> Vec<Range<usize>> {
    let mut edits: Vec<_> = plan.edits.iter().collect();
    edits.sort_by_key(|edit| edit.range.start);
    let mut original = 0;
    let mut updated = 0;
    let mut ranges = Vec::new();
    for edit in edits {
        updated += edit.range.start - original;
        let start = updated;
        updated += edit.replacement.len();
        ranges.push(start..updated);
        original = edit.range.end;
    }
    ranges
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::populate_repair_alternatives;
    use purrassist_core::{
        Budget, Governor, Outcome,
        contract::Evidence,
        digest::Digest,
        edits::{AppliedEdit, EditPlan},
    };

    /// `checked` only accepts a repair when reparsing is *globally* valid, so
    /// two genuinely independent missing-token defects can never both be
    /// individually sufficient (fixing one always leaves the other's error
    /// in place) — confirmed by direct experimentation against tree-sitter's
    /// JavaScript and Rust grammars (compound unclosed-block recovery even
    /// reports the same byte offset twice for two still-independently-needed
    /// closers). This test instead exercises exactly the code path `checked`
    /// hands off to once it *has* collected two or more accepted repairs
    /// (`populate_repair_alternatives`), with two realistic, independently
    /// valid `AppliedEdit`s standing in for that scenario.
    #[test]
    fn ambiguous_repairs_publish_one_alternative_per_accepted_repair() {
        let mut governor = Governor::new(Budget::default());
        let shared = Evidence {
            rule: "syntax.missing-punctuation.v1".into(),
            explanation: "insert parser-indicated \";\" at byte 5 and revalidate".into(),
            span: None,
            destination: "bytes:5".into(),
        };
        let first = AppliedEdit {
            text: "fn f() { 1; }".into(),
            inverse: EditPlan {
                source: Digest::of(b"fn f() { 1; }"),
                edits: Vec::new(),
            },
        };
        let second = AppliedEdit {
            text: "fn f() { 2; }".into(),
            inverse: EditPlan {
                source: Digest::of(b"fn f() { 2; }"),
                edits: Vec::new(),
            },
        };
        let mut receipt = purrassist_core::Receipt::new(
            b"probe",
            "syntax.apply.v1",
            purrassist_core::Effort::default(),
            Budget::default(),
        );
        let outcome = populate_repair_alternatives(
            vec![(first, shared.clone()), (second, shared)],
            &mut governor,
            &mut receipt,
        )
        .unwrap();
        let Outcome::Ambiguous(answers) = outcome else {
            panic!("expected ambiguous outcome")
        };
        assert_eq!(answers.len(), 2);
        assert_eq!(receipt.alternatives.len(), answers.len());
        for (answer, ledger) in answers.iter().zip(&receipt.alternatives) {
            assert_eq!(
                ledger.output,
                Digest::of(answer.to_node().to_json().as_bytes())
            );
        }
        // The two repairs' evidence was byte-identical: it is shared by
        // every alternative and promoted to the top-level ledger, leaving
        // each alternative's own ledger empty.
        assert_eq!(receipt.evidence.len(), 1);
        assert!(
            receipt
                .alternatives
                .iter()
                .all(|ledger| ledger.evidence.is_empty())
        );
    }
}
