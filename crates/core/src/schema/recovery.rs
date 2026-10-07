// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Composed, best-first recovery with whole-schema validation and explicit guesses.
mod proposals;
use super::Schema;
use crate::contract::saturating_size as size;
use crate::contract::{
    AssistResult, Budget, Effort, Evidence, Governor, Outcome, Receipt, Resource, ScratchVec,
    SourceSpan, StopReason,
};
use crate::digest::Digest;
use crate::json::{Node, ParseFailure, Value};
use crate::scan;
use crate::search::{Acceptance, Change, Ledger, Proposals, Search, SearchState};
pub(crate) use proposals::{
    EDITED_NAME_COST, FOLDED_NAME_COST, KEY_RELOCATE_COST, KEY_REMOVE_COST, name_cost,
};
use std::collections::{BTreeMap, BTreeSet};

/// Recovery policy; effort changes search bounds, never the schema's constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryRequest {
    /// Search preset. Exact performs no transformations.
    pub effort: Effort,
    /// Explicit resource override; otherwise use the effort preset.
    pub budget: Option<Budget>,
    /// Permit invented values, always returned as `BestGuess` or ambiguity.
    pub allow_guesses: bool,
}
impl Default for RecoveryRequest {
    fn default() -> Self {
        Self {
            effort: Effort::Standard,
            budget: None,
            allow_guesses: true,
        }
    }
}
/// Search-wide recovery state shared by every [`Repair`] candidate.
struct Recovery<'a> {
    schema: &'a Schema,
    request: RecoveryRequest,
    receipt: Receipt,
    /// Task 6 wiring: deduplicated by destination (one distinct compiled
    /// pattern), across every candidate this search actually validated.
    pattern_evidence: BTreeMap<String, Evidence>,
}
/// One candidate JSON document; its deduplication key and output identity
/// are its compact JSON bytes, and equal-cost candidates keep enqueue order.
#[derive(Clone)]
struct Repair(Node);
/// One expansion of a rejected [`Repair`]: the schema walk in `proposals`
/// proposes local changes to `parent` through [`Walk::change`].
struct Walk<'w, 'p> {
    schema: &'w Schema,
    request: RecoveryRequest,
    source: Digest,
    governor: &'w mut Governor,
    proposals: &'w mut Proposals<'p, Repair>,
    parent: &'w Node,
}
impl SearchState for Repair {
    type Tie = ();
    type Context<'c> = Recovery<'c>;
    fn tie(&self) {}
    fn write_key(&self, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
        write_node_key(&self.0, key)
    }
    fn accept(
        &self,
        recovery: &mut Recovery<'_>,
        governor: &mut Governor,
    ) -> Result<Acceptance, StopReason> {
        recovery.validate(&self.0, governor)
    }
    fn expand(
        &self,
        recovery: &mut Recovery<'_>,
        proposals: &mut Proposals<'_, Self>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if recovery.request.effort == Effort::Exact {
            return Ok(());
        }
        let mut walk = Walk {
            schema: recovery.schema,
            request: recovery.request,
            source: recovery.receipt.source,
            governor,
            proposals,
            parent: &self.0,
        };
        walk.propose(
            &self.0,
            recovery.schema.root,
            (&[], ""),
            0,
            &mut BTreeSet::new(),
            &mut Vec::new(),
        )
    }
}
impl Schema {
    /// Recover a schema-valid interpretation of imperfect UTF-8 JSON input.
    ///
    /// Every accepted answer is validated against this complete schema. Equal-cost
    /// valid interpretations remain ambiguous. Resource exhaustion returns
    /// `Unresolved` with an explicit termination reason, even if an unranked valid
    /// candidate has been encountered. Invented values are never `Resolved`.
    /// Ambiguous answers have separate `receipt.alternatives` ledgers in output
    /// order, with output digests and indices into the receipt's validation log.
    ///
    /// ```
    /// use purrassist_core::{Outcome, schema::{Schema, recovery::RecoveryRequest}};
    /// let schema = Schema::compile(r#"{"type":"object"}"#).unwrap();
    /// let result = schema.recover(r#"{"x":1,"x":2}"#, RecoveryRequest::default());
    /// let Outcome::Ambiguous(choices) = result.outcome else { panic!("expected alternatives") };
    /// assert_eq!(choices.len(), result.receipt.alternatives.len());
    /// for ledger in &result.receipt.alternatives {
    ///     assert!(result.receipt.validations[ledger.validations[0]].passed);
    /// }
    /// ```
    #[must_use]
    pub fn recover(&self, input: &str, request: RecoveryRequest) -> AssistResult<Node> {
        let budget = request.budget.unwrap_or_else(|| request.effort.budget());
        self.recover_governed(input, request, &mut Governor::new(budget))
    }

    /// Recover using the same governor as schema compilation or caller preparation.
    ///
    /// The governor's limits are authoritative; `request.budget` is ignored here.
    /// Effort still selects allowed recovery rules. Cancellation and exhaustion
    /// return `Unresolved` with cumulative usage and an explicit termination reason.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, Outcome, schema::{Schema, recovery::RecoveryRequest}};
    /// let mut governor = Governor::new(Budget::default());
    /// let schema = Schema::compile_governed(r#"{"type":"integer"}"#, &mut governor)?;
    /// let result = schema.recover_governed("[[42]]", RecoveryRequest::default(), &mut governor);
    /// assert!(matches!(result.outcome, Outcome::Resolved(_)));
    /// assert_eq!(result.receipt.usage, governor.usage());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn recover_governed(
        &self,
        input: &str,
        request: RecoveryRequest,
        governor: &mut Governor,
    ) -> AssistResult<Node> {
        let budget = *governor.limits();
        let mut recovery = Recovery {
            schema: self,
            request,
            receipt: Receipt::new(input.as_bytes(), "json.recover.v1", request.effort, budget),
            pattern_evidence: BTreeMap::new(),
        };
        let mut search = Search::new(governor);
        let outcome = match recovery
            .unsupported_vocabulary_evidence(governor)
            .and_then(|()| recovery.run(&mut search, input, governor))
        {
            Ok(outcome) => outcome,
            Err(reason) => {
                recovery.receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        drop(search);
        // Task 6 wiring: every distinct pattern actually validated during
        // this search, regardless of which candidate (if any) was accepted.
        recovery
            .receipt
            .evidence
            .extend(recovery.pattern_evidence.into_values());
        recovery.receipt.usage = governor.usage();
        AssistResult {
            outcome,
            receipt: recovery.receipt,
        }
    }
}
impl Recovery<'_> {
    /// Requirement 5: every recovery receipt carries an
    /// `unsupported-vocabulary` evidence entry for each optional (`false`)
    /// `$vocabulary` URI this schema's dialects did not recognize, naming
    /// the declaring resource's own keywords left unevaluated.
    fn unsupported_vocabulary_evidence(
        &mut self,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for unsupported in self.schema.unsupported_vocabularies() {
            governor.charge(Resource::EvidenceEntries, 1)?;
            let explanation = format!(
                "vocabulary {} is not supported; keywords left unevaluated: {}",
                unsupported.uri,
                unsupported.keywords.join(", ")
            );
            governor.charge(
                Resource::ScratchBytes,
                size(unsupported.uri.len())
                    .saturating_add(size(explanation.len()))
                    .saturating_add(64),
            )?;
            self.receipt.evidence.push(Evidence {
                rule: "schema.vocabulary.unsupported-v1".into(),
                explanation,
                span: None,
                destination: unsupported.uri,
            });
        }
        Ok(())
    }
    fn run(
        &mut self,
        search: &mut Search<Repair>,
        input: &str,
        governor: &mut Governor,
    ) -> Result<Outcome<Node>, StopReason> {
        self.receipt.retain_source(input, governor)?;
        governor.charge(Resource::InputBytes, size(input.len()))?;
        match crate::json::parse_with(input, governor) {
            Ok(node) => search.seed(governor, Repair(node), Ledger::default())?,
            Err(ParseFailure::Resource(reason)) => return Err(reason),
            Err(ParseFailure::Syntax(error)) if self.request.effort == Effort::Exact => {
                return Err(StopReason::InvalidInput(error.to_string()));
            }
            Err(ParseFailure::Syntax(_)) => self.syntax(search, input, governor)?,
        }
        let accepted = search.run(self, governor)?;
        let outcome = search.finish(accepted, &mut self.receipt, governor)?;
        Ok(outcome.map(|repair| repair.0))
    }
    /// Validate one candidate against the whole schema, retaining its
    /// validation, format, and pattern evidence in the receipt.
    fn validate(&mut self, node: &Node, governor: &mut Governor) -> Result<Acceptance, StopReason> {
        let report = self.schema.validate_with(node, governor)?;
        governor.charge(Resource::EvidenceEntries, 1)?;
        // Every violation's diagnostic line is permanently retained in the
        // receipt (never released), so it is charged directly, before the
        // `format!` calls that build it, like `retain_source`.
        let diagnostic_bytes = report.violations.iter().fold(0_u64, |sum, violation| {
            sum.saturating_add(size(violation.instance_path.len()))
                .saturating_add(size(violation.message.len()))
                .saturating_add(size(violation.schema_path.len()))
                .saturating_add(8)
        });
        governor.charge(Resource::ScratchBytes, diagnostic_bytes)?;
        let validation = self.receipt.validations.len();
        self.receipt.validations.push(report.to_validation());
        // Requirement 1 / Task 6 wiring: format annotation evidence and
        // pattern-engine evidence are search-time signals, not tied to
        // any one alternative, so they are retained unconditionally
        // (after the index captured above) rather than routed through
        // the derivation forest's per-alternative ledgers.
        governor.charge(
            Resource::EvidenceEntries,
            size(report.format_evidence.len()),
        )?;
        let format_bytes = report.format_evidence.iter().fold(0_u64, |sum, entry| {
            sum.saturating_add(size(entry.validator.len()))
                .saturating_add(size(entry.constraint.len()))
                .saturating_add(
                    entry
                        .diagnostics
                        .iter()
                        .fold(0, |inner, line| inner.saturating_add(size(line.len()))),
                )
                .saturating_add(32)
        });
        governor.charge(Resource::ScratchBytes, format_bytes)?;
        self.receipt
            .validations
            .extend(report.format_evidence.iter().cloned());
        for evidence in &report.pattern_evidence {
            if !self.pattern_evidence.contains_key(&evidence.destination) {
                governor.charge(Resource::EvidenceEntries, 1)?;
                governor.charge(
                    Resource::ScratchBytes,
                    size(evidence.rule.len())
                        .saturating_add(size(evidence.explanation.len()))
                        .saturating_add(size(evidence.destination.len()))
                        .saturating_add(32),
                )?;
                self.pattern_evidence
                    .insert(evidence.destination.clone(), evidence.clone());
            }
        }
        Ok(if report.valid {
            Acceptance::Accepted { validation }
        } else {
            Acceptance::Rejected
        })
    }
    /// Seed one candidate per bracketed value of the input, plus the value
    /// at offset 0, in ascending start order (see [`EXTRACT_WORK_PER_BYTE`]).
    ///
    /// A recovery parse is deterministic in the input after its first byte,
    /// so a container that a parse starts at offset `p` is exactly the
    /// candidate a standalone recovery from `p` would produce. Discovery
    /// therefore parses each bracketed region once: a [`scan::Tracker`]
    /// finds the next opener outside earlier region parses, a tracked parse
    /// recovers the region, and every container nested in it is taken as a
    /// subtree. An opener such a parse consumed as text (inside a string,
    /// comment or bare token) is parsed on its own, unless an earlier such
    /// parse already covered it. The tracker reads each byte outside region
    /// parses once, region parses never overlap, opener parses never
    /// overlap, and the offset-0 value (when the input does not start with a
    /// bracket) is parsed once more, so every byte is read at most four times.
    fn syntax(
        &self,
        search: &mut Search<Repair>,
        input: &str,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let bytes = input.as_bytes();
        let mut parses = ScratchVec::new(governor);
        if !matches!(bytes.first(), Some(b'{' | b'[')) {
            parses.push(Extracted {
                region: crate::json::recover_region(input, 0, governor)?,
                brackets_before: 0,
                root_only: true,
            })?;
        }
        let mut swept = 0;
        let mut cursor = 0;
        let mut orphan_cursor = 0;
        while cursor < bytes.len() {
            let mut tracker = scan::Tracker::new(cursor);
            let mut opened = None;
            for &byte in &bytes[cursor..] {
                if let Some(scan::Event::Open(start)) = tracker.push(byte) {
                    opened = Some(start);
                    break;
                }
                swept += u64::from(matches!(byte, b'}' | b']'));
            }
            governor.charge(Resource::Work, size(opened.unwrap_or(bytes.len()) - cursor))?;
            let Some(start) = opened else { break };
            let mut region = crate::json::recover_region(input, start, governor)?;
            let brackets_before = swept;
            swept += region.brackets;
            cursor = region.stop.max(start + 1);
            let orphans = core::mem::take(&mut region.orphans);
            parses.push(Extracted {
                region,
                brackets_before,
                root_only: false,
            })?;
            for orphan in &orphans {
                if orphan.at < orphan_cursor {
                    continue;
                }
                let region = crate::json::recover_region(input, orphan.at, governor)?;
                orphan_cursor = region.stop.max(orphan.at + 1);
                parses.push(Extracted {
                    region,
                    brackets_before: brackets_before + orphan.brackets_before,
                    root_only: false,
                })?;
            }
        }
        let context = Extraction {
            input,
            total_brackets: swept,
            first_significant: input.len() - input.trim_start().len(),
            last_significant_end: input.trim_end().len(),
        };
        governor.charge(
            Resource::Work,
            size(context.first_significant)
                .saturating_add(size(input.len() - context.last_significant_end)),
        )?;
        let mut found = ScratchVec::new(governor);
        for extracted in parses.as_slice() {
            collect_candidates(extracted, &mut found, governor)?;
        }
        found
            .as_mut_slice()
            .sort_unstable_by_key(|candidate| (candidate.start, candidate.order));
        let mut previous = None;
        for candidate in found.as_slice() {
            // Equal starts are the same standalone candidate found by two parses.
            if previous != Some(candidate.start) {
                previous = Some(candidate.start);
                self.seed_candidate(search, &context, candidate, governor)?;
            }
        }
        Ok(())
    }
    /// Seed one candidate with its edits and the `json.extract` losses for
    /// the text around it, building its ledger only when it is admitted.
    fn seed_candidate(
        &self,
        search: &mut Search<Repair>,
        context: &Extraction<'_>,
        candidate: &Candidate<'_>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        let edits = candidate.edits;
        if !self.request.allow_guesses && edits.iter().any(|edit| edit.guessed) {
            return Ok(());
        }
        let input = context.input;
        let discarded = [
            (
                0..candidate.start,
                context.first_significant < candidate.start,
                candidate.brackets_before,
            ),
            (
                candidate.end..input.len(),
                context.last_significant_end > candidate.end,
                context.total_brackets - candidate.brackets_through,
            ),
        ];
        let mut cost = 0_u64;
        for edit in edits {
            cost = cost.saturating_add(if edit.guessed { 100 } else { 1 });
        }
        for (_, significant, brackets) in &discarded {
            if *significant {
                cost = cost.saturating_add(5 + 10 * brackets);
            }
        }
        let node = candidate.node;
        search.seed_with(
            governor,
            cost,
            |key| write_node_key(node, key),
            |governor| {
                governor.charge(Resource::ScratchBytes, node.estimated_bytes())?;
                let mut ledger = Ledger::default();
                for edit in edits {
                    let range = edit.span.clone();
                    charge_change(
                        governor,
                        edit.rule.len() + EDIT_EXPLANATION.len() + edit.replacement.len(),
                        range.len(),
                        edit.guessed.then_some(edit.replacement.len()),
                    )?;
                    let evidence = Evidence {
                        rule: edit.rule.into(),
                        explanation: format!("{EDIT_EXPLANATION}{}", edit.replacement),
                        span: Some(SourceSpan {
                            source: self.receipt.source,
                            bytes: range.clone(),
                        }),
                        destination: String::new(),
                    };
                    ledger.record(Change {
                        evidence,
                        cost: if edit.guessed { 100 } else { 1 },
                        original: (!range.is_empty()).then(|| input[range].into()),
                        guess: edit.guessed.then(|| edit.replacement.clone()),
                    });
                }
                for (range, significant, brackets) in discarded {
                    if !significant {
                        continue;
                    }
                    governor.charge(Resource::EvidenceEntries, 1)?;
                    charge_change(
                        governor,
                        EXTRACT_RULE.len() + EXTRACT_EXPLANATION.len(),
                        range.len(),
                        None,
                    )?;
                    let evidence = Evidence {
                        rule: EXTRACT_RULE.into(),
                        explanation: EXTRACT_EXPLANATION.into(),
                        span: Some(SourceSpan {
                            source: self.receipt.source,
                            bytes: range.clone(),
                        }),
                        destination: String::new(),
                    };
                    ledger.record(Change {
                        evidence,
                        cost: 5 + 10 * brackets,
                        original: Some(input[range].into()),
                        guess: None,
                    });
                }
                Ok((Repair(node.clone()), ledger))
            },
        )?;
        Ok(())
    }
}
/// Upper bound on the `Work` that `json.extract` candidate discovery charges
/// per input byte when no candidate container is nested inside another.
///
/// When strict parsing fails, recovery offers the value at offset 0 and every
/// bracketed value as a candidate, each priced with the `json.extract` loss
/// of the text around it. Discovery is linear: a `scan::Tracker` reads the
/// text between regions, one tracked recovery parse reads each region and
/// yields every container nested in it as a subtree (no re-parse), and an
/// opener that a parse consumed as text (inside a string, comment or bare
/// token) gets one parse of its own unless an earlier such parse covered it.
/// For an `n`-byte input (`n >= 1`), the charges are at most:
///
/// - `3n` for reading: the tracker and region parses never overlap, opener
///   parses never overlap, and the offset-0 value is parsed once more when
///   the input does not start with a bracket;
/// - `6n + n` for recovered values (at most two per consumed byte, plus one
///   root per parse) and `2n` for locating the leading and trailing text;
/// - `n` for candidates (one per distinct start) plus each candidate's
///   deduplication key, at most `6s + 8` bytes for a candidate spanning `s`
///   source bytes; candidates that are not nested in another span at most
///   `3n` bytes in total, so their keys cost at most `26n`.
///
/// That totals `39n`, within `EXTRACT_WORK_PER_BYTE * n`. A candidate nested
/// inside another adds its own key and charge once more per enclosing
/// candidate, so in general discovery Work is at most
/// `EXTRACT_WORK_PER_BYTE * (n + s)`, where `s` sums the source spans of
/// nested candidates (at most `n` times the nesting depth, itself bounded by
/// [`Budget::depth`]). Earlier recovery attempted a parse from every bracket,
/// which is quadratic in the number of brackets.
///
/// Brackets inside strings, comments and bare tokens of a region are
/// candidates only through the opener parses described above; one nested in
/// text that an opener parse itself consumed as text is not offered again.
///
/// ```
/// use purrassist_core::{Effort, Outcome};
/// use purrassist_core::schema::{Schema, recovery::{EXTRACT_WORK_PER_BYTE, RecoveryRequest}};
/// let schema = Schema::compile(r#"{"type":"object","required":["answer"]}"#).unwrap();
/// let input = "notes {} [1] ".repeat(1_000) + r#"{"answer": 42}"#;
/// let result = schema.recover(&input, RecoveryRequest { effort: Effort::Standard, ..RecoveryRequest::default() });
/// assert!(matches!(result.outcome, Outcome::Resolved(_)));
/// assert!(result.receipt.usage.work <= EXTRACT_WORK_PER_BYTE * input.len() as u64);
/// ```
pub const EXTRACT_WORK_PER_BYTE: u64 = 48;
/// Evidence text for a syntax edit, followed by its replacement.
const EDIT_EXPLANATION: &str = "replace source range with ";
/// Rule and evidence text for text discarded around a candidate.
const EXTRACT_RULE: &str = "json.extract";
const EXTRACT_EXPLANATION: &str = "select a JSON region from surrounding content";
/// Shared facts about the whole input for pricing discarded text.
struct Extraction<'a> {
    input: &'a str,
    /// Bracket characters (`[]{}`) in the whole input.
    total_brackets: u64,
    /// Offset of the first non-whitespace character (`input.len()` if none).
    first_significant: usize,
    /// End of the last non-whitespace character (`0` if none).
    last_significant_end: usize,
}
/// One parse performed by extraction and the brackets before its start.
struct Extracted {
    region: crate::json::Region,
    brackets_before: u64,
    /// Offer only the root value (the offset-0 value of an input that does
    /// not start with a bracket; its containers are found as regions).
    root_only: bool,
}
impl crate::contract::ScratchCharge for Extracted {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 224);
        224
    };
}
/// A borrowed node awaiting pre-order traversal.
struct NodeRef<'a>(&'a Node);
impl crate::contract::ScratchCharge for NodeRef<'_> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 8);
        8
    };
}
/// One standalone-equivalent candidate within a parse.
struct Candidate<'a> {
    node: &'a Node,
    start: usize,
    /// Discovery order, a deterministic tie-break among equal starts.
    order: usize,
    /// Consumed end: the value and its trailing whitespace/comments.
    end: usize,
    edits: &'a [crate::json::SyntaxEdit],
    /// Brackets before `start` in the whole input.
    brackets_before: u64,
    /// Brackets before `end` in the whole input.
    brackets_through: u64,
}
impl crate::contract::ScratchCharge for Candidate<'_> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 72);
        72
    };
}
/// Append `extracted`'s completed containers (or only its root value) to
/// `found`, pairing the parse's pre-order container records with the
/// tree's containers by an explicit-stack pre-order traversal.
fn collect_candidates<'a>(
    extracted: &'a Extracted,
    found: &mut ScratchVec<Candidate<'a>>,
    governor: &Governor,
) -> Result<(), StopReason> {
    let region = &extracted.region;
    let Some(root) = &region.root else {
        return Ok(());
    };
    if extracted.root_only {
        if region.completed {
            let order = found.len();
            found.push(Candidate {
                node: root,
                start: 0,
                order,
                end: region.stop,
                edits: &region.edits,
                brackets_before: 0,
                brackets_through: region.brackets,
            })?;
        }
        return Ok(());
    }
    let mut pending = ScratchVec::new(governor);
    pending.push(NodeRef(root))?;
    let mut containers = region.containers.iter();
    // Each visited node was charged one `Work` unit when its parse recovered it.
    while let Some(NodeRef(node)) = pending.pop() {
        let first = pending.len();
        match &node.value {
            Value::Array(items) => {
                for item in items {
                    pending.push(NodeRef(item))?;
                }
            }
            Value::Object(members) => {
                for member in members {
                    pending.push(NodeRef(&member.value))?;
                }
            }
            _ => continue,
        }
        pending.as_mut_slice()[first..].reverse();
        let container = containers
            .next()
            .expect("every started container is in the tree, in pre-order");
        if let Some(end) = container.end {
            let order = found.len();
            found.push(Candidate {
                node,
                start: container.start,
                order,
                end,
                edits: &region.edits[container.edits_before..container.edits_through],
                brackets_before: extracted.brackets_before + container.brackets_before,
                brackets_through: extracted.brackets_before + container.brackets_through,
            })?;
        }
    }
    Ok(())
}
/// Charge one ledger change's retained storage before it is built: its
/// evidence, plus an evidence copy and the original text for a non-empty
/// loss, plus an evidence copy and the value for a guess (256 bytes of
/// structure per entry, as [`Ledger::estimated_bytes`], plus `text` bytes of
/// rule and explanation per entry).
fn charge_change(
    governor: &mut Governor,
    text: usize,
    original: usize,
    guess: Option<usize>,
) -> Result<(), StopReason> {
    let entries = 1 + u64::from(original > 0) + u64::from(guess.is_some());
    governor.charge(
        Resource::ScratchBytes,
        entries
            .saturating_mul(size(text).saturating_add(256))
            .saturating_add(size(original))
            .saturating_add(size(guess.unwrap_or(0))),
    )
}
/// A candidate's canonical deduplication key: its compact JSON.
fn write_node_key(node: &Node, key: &mut ScratchVec<u8>) -> Result<(), StopReason> {
    let json = node.to_json();
    key.reserve(json.len())?;
    key.extend(json.bytes())
}
impl Walk<'_, '_> {
    /// Propose replacing the node at `route` (reported at `path`) in the
    /// parent candidate with `edit`'s `(replacement, rule, cost, guessed)`.
    fn change(
        &mut self,
        route: &[usize],
        path: &str,
        edit: (Node, &str, u64, bool),
    ) -> Result<(), StopReason> {
        let (replacement, rule, cost, guessed) = edit;
        if guessed && !self.request.allow_guesses {
            return Ok(());
        }
        self.governor.charge(Resource::Work, 1)?;
        self.governor.charge(Resource::EvidenceEntries, 1)?;
        let parent = self.parent;
        let original = at(parent, route);
        if original.equivalent(&replacement) {
            return Ok(());
        }
        // Cloning the parent duplicates the whole candidate (node tree and
        // every ledger so far); `original`/`replacement` are each serialized
        // at least once below. Charge a bound on all of it before any of
        // those allocations happen, and release exactly that bound once this
        // change either lands in the queue or is discarded as a duplicate.
        let clone_bytes = parent
            .estimated_bytes()
            .saturating_add(self.proposals.parent().estimated_bytes())
            .saturating_add(original.estimated_bytes())
            .saturating_add(replacement.estimated_bytes().saturating_mul(3))
            .saturating_add(512);
        let _lease = self.governor.lease_scratch(clone_bytes)?;
        let evidence = Evidence {
            rule: rule.into(),
            explanation: format!("replace with {}", replacement.to_json()),
            destination: path.into(),
            span: original.span.clone().map(|bytes| SourceSpan {
                source: self.source,
                bytes,
            }),
        };
        let change = Change {
            evidence,
            cost,
            original: Some(original.to_json()),
            guess: guessed.then(|| replacement.to_json()),
        };
        let mut node = parent.clone();
        *at_mut(&mut node, route) = replacement;
        self.proposals.commit(self.governor, Repair(node), change)
    }
}
fn at<'a>(mut node: &'a Node, route: &[usize]) -> &'a Node {
    for &index in route {
        node = match &node.value {
            Value::Array(items) => &items[index],
            Value::Object(members) => &members[index].value,
            _ => unreachable!("generated route"),
        };
    }
    node
}
fn at_mut<'a>(mut node: &'a mut Node, route: &[usize]) -> &'a mut Node {
    for &index in route {
        node = match &mut node.value {
            Value::Array(items) => &mut items[index],
            Value::Object(members) => &mut members[index].value,
            _ => unreachable!("generated route"),
        };
    }
    node
}
