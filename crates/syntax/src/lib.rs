// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Syntax adapters for intent-aware source editing.
//!
//! Parser success proves grammar acceptance, not type checking or program behavior.
//! Tree-sitter's own C allocations are bounded by the same governor as everything
//! else: `alloc::install` (called once, before any tree-sitter object exists;
//! see the [`alloc`] module) routes every `malloc`/`calloc`/`realloc`/`free`
//! tree-sitter's C library performs through wrappers that attribute each block to
//! the parse that requested it. A progress callback charges the delta since its
//! last check to [`Resource::ScratchBytes`] and breaks the parse with
//! `Cancelled`/`BudgetExceeded` on failure — never a partial tree. The real
//! allocator can still outrun the last charge between two progress checks; that
//! bound is the *overshoot* discussed in [`ParsedTree::allocation`] and
//! `docs/RESOURCES.md`. Input admission, progress callbacks, tree traversal, and
//! candidate counts are all bounded here.
use purrassist_core::contract::saturating_size as size;
use purrassist_core::{
    AssistResult, Budget, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Resource, ScratchLease, Validation},
    digest::Digest,
    edits::{AppliedEdit, EditPlan, TextEdit},
    provenance::{Derivation, Forest},
};
use std::ops::{ControlFlow, Range};

pub mod alloc;

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
    /// For `jsx.mismatched-tags.v1`, the opening and closing tag's name
    /// spans, when both are named (neither is a `<>...</>` fragment). Lets
    /// `checked` propose renaming either side to match the other as two
    /// distinct, competing repairs. `None` for every other issue kind, and
    /// for a fragment/named mismatch (nothing to rename).
    pub rename: Option<JsxRename>,
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
/// Allocator accounting for one governed parse, beyond [`Report`]'s content.
///
/// Fields are observed call and byte counts, not underlying memory; nothing
/// here is charged separately from the [`ScratchLease`] a [`ParsedTree`]
/// already holds, or from the charge [`inspect`] releases before it returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocationReport {
    /// Successful tree-sitter malloc, calloc, and realloc calls attributed
    /// to this parse; excludes ordinary Rust allocations and allocator headers.
    pub allocation_calls: u64,
    /// Bytes the produced tree retains: what [`ParsedTree`] charges to the
    /// governor and releases when dropped (or, for [`inspect`], what is
    /// charged and released again before [`inspect`] itself returns).
    pub retained_bytes: u64,
    /// High-water live bytes this parse's allocator account ever held
    /// (parser scratch plus tree, before the parser's own buffers were
    /// freed at the end of parsing).
    pub peak_bytes: u64,
    /// Largest single amount tree-sitter allocated between two consecutive
    /// progress-callback checks. Real allocation can momentarily exceed the
    /// governor's last charge by up to this many bytes before the next
    /// check catches up and either charges the difference or breaks the
    /// parse; see `docs/RESOURCES.md` for measured, per-grammar bounds on
    /// this quantity, and [`alloc`]'s module docs for why budget enforcement
    /// never relies on the allocator itself refusing a request.
    pub max_overshoot_bytes: u64,
}

/// A retained parse tree, independent of [`Report`]'s traversal.
///
/// `Send`: tree-sitter's own C library permits freeing memory from any
/// thread, and this crate's allocator wrappers (see [`alloc`]) account for
/// that correctly regardless of which thread drops this value.
#[derive(Debug)]
pub struct ParsedTree {
    tree: tree_sitter::Tree,
    lease: ScratchLease,
    allocation_calls: u64,
    peak_bytes: u64,
    max_overshoot_bytes: u64,
}

impl ParsedTree {
    /// This parse's allocator accounting. `retained_bytes` reads the
    /// governor-charged lease directly, so it always matches what dropping
    /// this value releases.
    #[must_use]
    pub const fn allocation(&self) -> AllocationReport {
        AllocationReport {
            allocation_calls: self.allocation_calls,
            retained_bytes: self.lease.bytes(),
            peak_bytes: self.peak_bytes,
            max_overshoot_bytes: self.max_overshoot_bytes,
        }
    }

    /// Whether the tree has no error or missing-token nodes.
    #[must_use]
    pub fn valid(&self) -> bool {
        !self.tree.root_node().has_error()
    }
}

/// Parse `source` under `language`, retaining the tree without a `Report`.
///
/// Charges `InputBytes`/`Work` like [`inspect`]; on success, the tree's
/// allocator-tracked bytes are charged to [`Resource::ScratchBytes`] and
/// released exactly once, when the returned [`ParsedTree`] is dropped — even
/// from a different thread than the one that parsed it (`Tree` is `Send`;
/// see [`alloc`]).
///
/// # Errors
/// Reports incompatible grammar, or cancelled/exhausted parsing — including
/// [`Resource::ScratchBytes`] exhaustion measured by the real allocator, not
/// an estimate. A break during parsing never returns a partial tree: this
/// discards whatever tree-sitter had built so far and releases every byte
/// charged during the attempt.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_syntax::{Language, parse_governed};
/// let mut governor = Governor::new(Budget::default());
/// let parsed = parse_governed("fn f() {}", Language::Rust, &mut governor).unwrap();
/// assert!(parsed.valid());
/// assert!(parsed.allocation().retained_bytes > 0);
/// ```
pub fn parse_governed(
    source: &str,
    language: Language,
    governor: &mut Governor,
) -> Result<ParsedTree, StopReason> {
    let accounted = parse_accounted(source, language, governor)?;
    let lease = governor.scratch_lease_for(accounted.retained_bytes);
    Ok(ParsedTree {
        allocation_calls: accounted.allocation_calls,
        tree: accounted.tree,
        lease,
        peak_bytes: accounted.peak_bytes,
        max_overshoot_bytes: accounted.max_overshoot_bytes,
    })
}

/// One accounted parse: the tree plus enough allocator bookkeeping for
/// [`inspect`]/[`parse_governed`] to charge and release it correctly.
struct Accounted {
    tree: tree_sitter::Tree,
    retained_bytes: u64,
    allocation_calls: u64,
    peak_bytes: u64,
    max_overshoot_bytes: u64,
}

/// Parse `source` under `language` with every tree-sitter allocation
/// attributed to a fresh [`alloc::Account`] (via [`alloc::scoped`]) and
/// charged to `governor`'s `ScratchBytes` at each progress-callback check.
///
/// On success, `retained_bytes` reflects exactly the tree's own footprint:
/// the local `Parser` (and its internal scratch) has already been dropped
/// and reconciled away by the time this returns, inside the same scoped
/// account, before the caller ever sees the tree. On failure — cancellation,
/// any budget exhaustion, or an incompatible grammar — every byte charged
/// during the attempt is released before returning; there is no partial
/// success.
fn parse_accounted(
    source: &str,
    language: Language,
    governor: &mut Governor,
) -> Result<Accounted, StopReason> {
    governor.charge(Resource::InputBytes, size(source.len()))?;
    governor.charge(Resource::Work, size(source.len()))?;
    alloc::install();
    let account = alloc::Account::new();
    let mut charged = 0_u64;
    let mut max_overshoot = 0_u64;
    let mut stopped: Option<StopReason> = None;
    let tree = alloc::scoped(&account, || {
        let mut parser = tree_sitter::Parser::new();
        if let Err(error) = parser.set_language(&language.grammar()) {
            stopped = Some(StopReason::InvalidInput(error.to_string()));
            return None;
        }
        let mut progress = |_: &tree_sitter::ParseState| {
            if let Err(reason) = governor.charge(Resource::Work, 100) {
                stopped = Some(reason);
                return ControlFlow::Break(());
            }
            let live = account.live();
            if live > charged {
                let delta = live - charged;
                max_overshoot = max_overshoot.max(delta);
                if let Err(reason) = governor.charge(Resource::ScratchBytes, delta) {
                    stopped = Some(reason);
                    return ControlFlow::Break(());
                }
                charged = live;
            } else if live < charged {
                let _ = governor.release_scratch(charged - live);
                charged = live;
            }
            ControlFlow::Continue(())
        };
        let options = tree_sitter::ParseOptions::new().progress_callback(&mut progress);
        // The `Parser` drops here, inside the scoped account, freeing its
        // own internal buffers; the `Tree` it produced is refcounted
        // separately by tree-sitter and survives this scope.
        parser.parse_with_options(
            &mut |offset, _| source.as_bytes().get(offset..).unwrap_or(&[]),
            None,
            Some(options),
        )
    });
    // Reconcile whatever the `Parser`'s own drop, above, just released.
    let live = account.live();
    if live < charged {
        let _ = governor.release_scratch(charged - live);
        charged = live;
    } else if live > charged {
        if let Err(reason) = governor.charge(Resource::ScratchBytes, live - charged) {
            stopped = Some(stopped.unwrap_or(reason));
        }
        charged = live;
    }
    if let Some(reason) = stopped {
        let _ = governor.release_scratch(charged);
        return Err(reason);
    }
    let Some(tree) = tree else {
        let _ = governor.release_scratch(charged);
        return Err(StopReason::InvalidInput(
            "syntax parser returned no tree".into(),
        ));
    };
    Ok(Accounted {
        allocation_calls: account.calls(),
        tree,
        retained_bytes: charged,
        peak_bytes: account.peak(),
        max_overshoot_bytes: max_overshoot,
    })
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
    let accounted = parse_accounted(source, language, governor)?;
    let lease = governor.scratch_lease_for(accounted.retained_bytes);
    let report = traverse(&accounted.tree, source, language, governor)?;
    // `accounted.tree` and `lease` drop here (in this order): dropping the
    // tree frees its real memory (decrementing the account tracked inside
    // `lease`'s charge), and dropping the lease releases exactly the bytes
    // charged for it from the governor's ledger. `inspect` never retains the
    // tree past its own return, so its net scratch charge is always zero by
    // the time it returns, on every path (including an early `?` return
    // inside `traverse`).
    drop(accounted.tree);
    drop(lease);
    Ok(report)
}

/// [`inspect`], but also returning the parse's [`AllocationReport`].
///
/// Reports peak, retained, and overshoot byte counts instead of discarding
/// them, for hosts that must surface allocator accounting alongside grammar
/// issues and declarations from the same governed parse — see
/// `purrassist_host`'s `inspect_code` tool.
///
/// # Errors
/// Same as [`inspect`].
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_syntax::{Language, inspect_with_allocation};
/// let mut governor = Governor::new(Budget::default());
/// let (report, allocation) = inspect_with_allocation("fn f() {}", Language::Rust, &mut governor)?;
/// assert!(report.valid);
/// assert!(allocation.retained_bytes > 0);
/// assert!(allocation.allocation_calls > 0);
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
pub fn inspect_with_allocation(
    source: &str,
    language: Language,
    governor: &mut Governor,
) -> Result<(Report, AllocationReport), StopReason> {
    let parsed = parse_governed(source, language, governor)?;
    let report = traverse(&parsed.tree, source, language, governor)?;
    Ok((report, parsed.allocation()))
}

/// Shared traversal for [`inspect`]/[`inspect_with_allocation`]: walk every
/// node of an already-parsed tree, collecting parser errors/missing tokens,
/// mismatched JSX/TSX tag pairs, and named declarations.
fn traverse(
    tree: &tree_sitter::Tree,
    source: &str,
    language: Language,
    governor: &mut Governor,
) -> Result<Report, StopReason> {
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
                rename: None,
            });
            report.valid = false;
        }
        if let Some((span, rename)) = mismatched_jsx(node, source) {
            governor.charge(Resource::EvidenceEntries, 1)?;
            report.issues.push(SyntaxIssue {
                span,
                kind: "jsx.mismatched-tags.v1".into(),
                missing: false,
                rename,
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
/// A mismatched JSX/TSX tag pair's opening/closing name spans, used to offer
/// a rename repair on either side (see [`SyntaxIssue::rename`]).
type JsxRename = (Range<usize>, Range<usize>);

/// The issue span (the closing tag's whole range) plus [`JsxRename`], when
/// both tags are named.
fn mismatched_jsx(
    node: tree_sitter::Node<'_>,
    source: &str,
) -> Option<(Range<usize>, Option<JsxRename>)> {
    if node.kind() != "jsx_element" {
        return None;
    }
    let opening = node.child_by_field_name("open_tag")?;
    let closing = node.child_by_field_name("close_tag")?;
    let opening_name = opening.child_by_field_name("name");
    let closing_name = closing.child_by_field_name("name");
    let text = |name: tree_sitter::Node<'_>| {
        source[name.byte_range()]
            .chars()
            .filter(|character| !character.is_whitespace())
    };
    let same = match (opening_name, closing_name) {
        (None, None) => true, // Fragment <>...</>.
        (Some(left), Some(right)) => text(left).eq(text(right)),
        _ => false,
    };
    if same {
        return None;
    }
    let rename = match (opening_name, closing_name) {
        (Some(left), Some(right)) => Some((left.byte_range(), right.byte_range())),
        _ => None, // A fragment mismatched with a named tag has nothing to rename.
    };
    Some((closing.byte_range(), rename))
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
/// A single parser-indicated missing punctuation token inside the changed
/// region may be repaired, and a mismatched JSX/TSX closing tag name may be
/// renamed to match either side, but only when reparsing accepts the
/// complete source. Multiple valid repairs stay ambiguous — for example, a
/// mismatched `<div></span>` accepts renaming either tag to match the other,
/// which are two distinct, competing, individually valid repairs.
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
        if !changed
            .iter()
            .any(|range| range.start <= issue.span.start && issue.span.start <= range.end)
        {
            continue;
        }
        if issue.missing && matches!(issue.kind.as_str(), ";" | ":" | ")" | "]" | "}" | ",") {
            if let Some(candidate) = try_missing_punctuation(
                source,
                &applied.text,
                report.source,
                issue,
                language,
                governor,
                receipt,
            )? {
                accepted.push(candidate);
            }
        } else if issue.kind == "jsx.mismatched-tags.v1"
            && let Some(rename) = issue.rename.clone()
        {
            accepted.extend(try_jsx_renames(
                source,
                &applied.text,
                report.source,
                rename,
                language,
                governor,
                receipt,
            )?);
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
        // Every accepted repair changed a different byte range (a different
        // missing token's position, or one specific tag's name), but
        // publish through the derivation forest anyway so any evidence that
        // happens to be identical across repairs is promoted once instead
        // of duplicated into every alternative.
        _ => populate_repair_alternatives(accepted, governor, receipt)?,
    })
}
/// Try inserting `issue`'s parser-indicated missing token at its reported
/// position, the single candidate a missing-punctuation issue offers.
///
/// # Errors
/// Propagates cancellation/exhaustion from applying or re-inspecting the
/// candidate.
fn try_missing_punctuation(
    source: &str,
    applied_text: &str,
    report_source: Digest,
    issue: &SyntaxIssue,
    language: Language,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Option<(AppliedEdit, Evidence)>, StopReason> {
    let evidence = Evidence {
        rule: "syntax.missing-punctuation.v1".into(),
        explanation: format!(
            "insert parser-indicated {:?} at byte {} and revalidate",
            issue.kind, issue.span.start
        ),
        span: None,
        destination: format!("bytes:{}", issue.span.start),
    };
    let edit = TextEdit {
        range: issue.span.clone(),
        replacement: issue.kind.clone(),
    };
    try_candidate(
        source,
        applied_text,
        report_source,
        edit,
        evidence,
        language,
        governor,
        receipt,
    )
}
/// Try both directions of a JSX/TSX tag-name mismatch repair: renaming the
/// closing tag to match the opening one, and renaming the opening tag to
/// match the closing one. Each candidate edits the *original* malformed
/// text independently (not the other candidate's result), so both are
/// genuinely competing, distinct-output repairs when both reparse validly.
///
/// # Errors
/// Propagates cancellation/exhaustion from applying or re-inspecting either
/// candidate.
fn try_jsx_renames(
    source: &str,
    applied_text: &str,
    report_source: Digest,
    rename: JsxRename,
    language: Language,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Vec<(AppliedEdit, Evidence)>, StopReason> {
    let (opening_name, closing_name) = rename;
    let opening_text = applied_text[opening_name.clone()].to_string();
    let closing_text = applied_text[closing_name.clone()].to_string();
    let attempts = [
        (
            closing_name.clone(),
            opening_text.clone(),
            format!(
                "rename closing JSX tag to {opening_text:?} at byte {} to match its opening tag and revalidate",
                closing_name.start
            ),
            closing_name.start,
        ),
        (
            opening_name.clone(),
            closing_text.clone(),
            format!(
                "rename opening JSX tag to {closing_text:?} at byte {} to match its closing tag and revalidate",
                opening_name.start
            ),
            opening_name.start,
        ),
    ];
    let mut accepted = Vec::new();
    for (range, replacement, explanation, destination_start) in attempts {
        let evidence = Evidence {
            rule: "syntax.jsx-tag-rename.v1".into(),
            explanation,
            span: None,
            destination: format!("bytes:{destination_start}"),
        };
        if let Some(candidate) = try_candidate(
            source,
            applied_text,
            report_source,
            TextEdit { range, replacement },
            evidence,
            language,
            governor,
            receipt,
        )? {
            accepted.push(candidate);
        }
    }
    Ok(accepted)
}
/// Apply one candidate edit against the original malformed `applied_text`,
/// then re-`inspect` the whole result. Returns `None` (not an error) when
/// the candidate reparses but is still globally invalid — that is a
/// rejected candidate, not a failure of `checked` itself.
///
/// # Errors
/// Propagates cancellation/exhaustion from applying or re-inspecting the
/// candidate.
#[expect(
    clippy::too_many_arguments,
    reason = "one candidate attempt, not a public API; splitting it would only move these same parameters into another struct with no callers"
)]
fn try_candidate(
    source: &str,
    applied_text: &str,
    report_source: Digest,
    edit: TextEdit,
    evidence: Evidence,
    language: Language,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Option<(AppliedEdit, Evidence)>, StopReason> {
    governor.charge(Resource::Candidates, 1)?;
    let repair = EditPlan {
        source: report_source,
        edits: vec![edit],
    };
    let repaired = repair.apply_governed(applied_text, governor);
    let Outcome::Resolved(repaired) = repaired.outcome else {
        return Err(repaired.receipt.stopped);
    };
    let validation_report = inspect(&repaired.text, language, governor)?;
    validation(receipt, &validation_report);
    if !validation_report.valid {
        return Ok(None);
    }
    let inverse = EditPlan {
        source: Digest::of(repaired.text.as_bytes()),
        edits: vec![TextEdit {
            range: 0..repaired.text.len(),
            replacement: source.into(),
        }],
    };
    Ok(Some((
        AppliedEdit {
            text: repaired.text,
            inverse,
        },
        evidence,
    )))
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
    Ok(Outcome::Ambiguous(purrassist_core::provenance::permute(
        &outputs,
        &published.order,
    )))
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
    /// two genuinely *independent* missing-token defects can never both be
    /// individually sufficient (fixing one always leaves the other's error
    /// in place) — confirmed by direct experimentation against tree-sitter's
    /// JavaScript and Rust grammars (compound unclosed-block recovery even
    /// reports the same byte offset twice for two still-independently-needed
    /// closers). A *single* defect can still admit two competing, globally
    /// valid repairs, though: a mismatched JSX/TSX closing tag name accepts
    /// renaming either side to match the other (see `checked`'s
    /// `jsx.mismatched-tags.v1` branch, and the public-API test
    /// `mismatched_jsx_tag_yields_two_ambiguous_rename_repairs` in
    /// `tests/syntax.rs`, which reaches `populate_repair_alternatives`
    /// through `apply_checked` with a real `<div></span>` input). This unit
    /// test instead exercises exactly the code path `checked` hands off to
    /// once it *has* collected two or more accepted repairs
    /// (`populate_repair_alternatives`), with two realistic, independently
    /// valid `AppliedEdit`s standing in for that scenario, without incurring
    /// the cost of a full parse per candidate.
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
