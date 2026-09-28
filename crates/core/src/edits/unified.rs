// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Single-buffer unified diffs: strict grammar, relocatable context, exact undo.
use super::{EditPlan, TextEdit, size};
use crate::contract::{Evidence, Resource, SourceSpan};
use crate::{AssistResult, Budget, Effort, Governor, Outcome, Receipt, StopReason};

#[derive(Clone, Debug)]
struct Line<'a> {
    kind: u8,
    text: &'a str,
    terminated: bool,
}
#[derive(Debug)]
struct Hunk<'a> {
    start: usize,
    old_count: usize,
    lines: Vec<Line<'a>>,
}
#[derive(Debug)]
struct SourceLine<'a> {
    start: usize,
    text: &'a str,
    ending: &'a str,
}

/// Plan a single-file unified diff against a supplied UTF-8 buffer.
///
/// File headers are labels, never filesystem authority. Hunk counts and order
/// must be valid. Context/removal lines must match exactly, excluding LF/CRLF
/// framing; existing context bytes are preserved. Added lines use the matched
/// region's line ending. Explicit no-newline markers are honored.
///
/// Hunks with old context can move to any matching line boundary. Only globally
/// ordered, nonoverlapping combinations survive; multiple combinations remain
/// ambiguous. Context-free insertions use their declared original line position.
/// Resource exhaustion or malformed input returns `Unresolved` with a reason.
/// Binary patches, renames, and multiple file sections are rejected.
///
/// ```
/// use purrassist_core::{Budget, Outcome, edits::plan_unified};
/// let source = "one\ntwo\n";
/// let patch = "--- a/example\n+++ b/example\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n";
/// let Outcome::Resolved(plan) = plan_unified(source, patch, Budget::default()).outcome
///     else { panic!("expected a unique plan") };
/// let Outcome::Resolved(edit) = plan.apply(source, Budget::default()).outcome
///     else { panic!("expected application") };
/// assert_eq!(edit.text, "one\nthree\n");
/// ```
#[must_use]
pub fn plan_unified(source: &str, patch: &str, budget: Budget) -> AssistResult<EditPlan> {
    plan_unified_governed(source, patch, &mut Governor::new(budget))
}

/// Run the same planning operation with cumulative caller-owned limits and cancellation.
///
/// Returns `Unresolved` on cancellation or exhaustion; the receipt records the
/// governor's effective limits and cumulative usage. Completed plans remain
/// proposals and perform no filesystem effects.
///
/// ```
/// use purrassist_core::{Budget, Governor, Outcome, StopReason, edits};
/// let mut governor = Governor::new(Budget::default());
/// governor.cancel();
/// let result = edits::plan_unified_governed("tea", "", &mut governor);
/// assert!(matches!(result.outcome, Outcome::Unresolved));
/// assert_eq!(result.receipt.stopped, StopReason::Cancelled);
/// ```
#[must_use]
pub fn plan_unified_governed(
    source: &str,
    patch: &str,
    governor: &mut Governor,
) -> AssistResult<EditPlan> {
    let budget = *governor.limits();
    let mut receipt = Receipt::new(
        source.as_bytes(),
        "text.unified-diff.v1",
        Effort::Standard,
        budget,
    );
    let result = plan(source, patch, governor, &mut receipt);
    let outcome = match result {
        Ok(mut plans) if plans.len() == 1 => {
            let (plan, evidence) = plans.remove(0);
            receipt.evidence.extend(evidence);
            Outcome::Resolved(plan)
        }
        // Every surviving combination matched its context at a complete
        // line boundary; leave that per-plan context evidence out of the
        // top-level ledger and publish it through the derivation forest
        // instead, promoting whatever context happens to be identical
        // across every combination and keeping the rest alternative-specific.
        Ok(plans) if !plans.is_empty() => {
            match populate_unified_alternatives(plans, governor, &mut receipt) {
                Ok(plans) => Outcome::Ambiguous(plans),
                Err(reason) => {
                    receipt.stopped = reason;
                    Outcome::Unresolved
                }
            }
        }
        Ok(_) => {
            receipt.stopped = StopReason::NoCandidate;
            Outcome::Unresolved
        }
        Err(reason) => {
            receipt.stopped = reason;
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    AssistResult { outcome, receipt }
}
/// Build one derivation per surviving hunk-placement combination and publish
/// shared vs. per-combination context evidence through the derivation forest.
///
/// # Errors
/// Returns `BudgetExceeded` before any ledger is committed to `receipt`.
fn populate_unified_alternatives(
    plans: Vec<(EditPlan, Vec<Evidence>)>,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Vec<EditPlan>, StopReason> {
    let mut forest = crate::provenance::Forest::new(governor);
    let mut derivations = Vec::with_capacity(plans.len());
    let mut outputs = Vec::with_capacity(plans.len());
    for (plan, evidence) in plans {
        let output = crate::digest::Digest::of(plan.to_node().to_json().as_bytes());
        let mut derivation = crate::provenance::Derivation::new(output, 0);
        for entry in evidence {
            derivation.push(forest.evidence(governor, entry)?);
        }
        derivations.push(derivation);
        outputs.push(plan);
    }
    let published = forest.publish(&derivations, governor)?;
    receipt.evidence = published.evidence;
    receipt.alternatives = published.alternatives;
    Ok(outputs)
}

fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}

fn plan(
    source: &str,
    patch: &str,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Vec<(EditPlan, Vec<Evidence>)>, StopReason> {
    receipt.retain_source(source, governor)?;
    receipt.retain_source(patch, governor)?;
    governor.charge(
        Resource::InputBytes,
        size(source.len()).saturating_add(size(patch.len())),
    )?;
    governor.charge(
        Resource::Work,
        size(source.len()).saturating_add(size(patch.len())),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        size(source.len())
            .saturating_add(size(patch.len()))
            .saturating_mul(128),
    )?;
    let hunks = parse(patch)?;
    let lines = source_lines(source);
    let mut partials = vec![(
        EditPlan {
            source: receipt.source,
            edits: Vec::new(),
        },
        0_usize,
    )];
    for hunk in hunks {
        let mut next = Vec::new();
        let candidates: Box<dyn Iterator<Item = usize>> = if hunk.old_count == 0 {
            Box::new(std::iter::once(hunk.start))
        } else {
            Box::new(0..=lines.len().saturating_sub(hunk.old_count))
        };
        for position in candidates {
            governor.charge(
                Resource::Work,
                size(hunk.lines.len()).saturating_add(size(patch.len())),
            )?;
            if !matches_at(&lines, &hunk, position) {
                continue;
            }
            for (previous, end) in &partials {
                if position < *end {
                    continue;
                }
                governor.charge(Resource::Candidates, 1)?;
                governor.charge(
                    Resource::ScratchBytes,
                    size(source.len())
                        .saturating_add(size(patch.len()))
                        .saturating_mul(2)
                        .saturating_add(size(previous.edits.len()).saturating_mul(128)),
                )?;
                let mut candidate = previous.clone();
                append_edits(source, &lines, &hunk, position, &mut candidate);
                if candidate.edits.windows(2).any(|pair| {
                    pair[0].range.end > pair[1].range.start
                        || pair[0].range.start == pair[1].range.start
                }) {
                    continue;
                }
                next.push((candidate, position + hunk.old_count));
            }
        }
        partials = next;
        if partials.is_empty() {
            break;
        }
    }
    let mut plans = Vec::new();
    for (plan, _) in partials {
        if plans.iter().any(|(existing, _)| existing == &plan) {
            continue;
        }
        let mut evidence = Vec::with_capacity(plan.edits.len());
        for edit in &plan.edits {
            governor.charge(Resource::EvidenceEntries, 1)?;
            evidence.push(Evidence {
                rule: "text.unified-context.v1".into(),
                explanation: "old-side content matched at a complete line boundary".into(),
                span: Some(SourceSpan {
                    source: receipt.source,
                    bytes: edit.range.clone(),
                }),
                destination: format!("bytes:{}..{}", edit.range.start, edit.range.end),
            });
        }
        plans.push((plan, evidence));
    }
    Ok(plans)
}

fn parse(patch: &str) -> Result<Vec<Hunk<'_>>, StopReason> {
    let mut lines = patch.split_inclusive('\n').peekable();
    for prefix in ["--- ", "+++ "] {
        if !lines.next().is_some_and(|line| {
            line.starts_with(prefix) && line.ends_with('\n') && line.trim_end().len() > prefix.len()
        }) {
            return Err(invalid("expected exactly one pair of unified file headers"));
        }
    }
    let mut hunks = Vec::new();
    let mut old_end = 0;
    let mut new_end = 0;
    while let Some(header) = lines.next() {
        let (old, new) = header
            .strip_prefix("@@ -")
            .and_then(|line| line.split_once(" +"))
            .ok_or_else(|| invalid("expected unified hunk header"))?;
        let (new, _) = new
            .split_once(" @@")
            .ok_or_else(|| invalid("unterminated unified hunk header"))?;
        let (old_start, old_count) = coordinate(old)?;
        let (new_start, new_count) = coordinate(new)?;
        if old_start < old_end || new_start < new_end {
            return Err(invalid("unordered or overlapping declared hunks"));
        }
        if old_start - old_end != new_start - new_end {
            return Err(invalid(
                "inconsistent unchanged gap between hunk coordinates",
            ));
        }
        old_end = old_start
            .checked_add(old_count)
            .ok_or_else(|| invalid("hunk coordinate overflow"))?;
        new_end = new_start
            .checked_add(new_count)
            .ok_or_else(|| invalid("hunk coordinate overflow"))?;
        let mut hunk = Hunk {
            start: old_start,
            old_count,
            lines: Vec::new(),
        };
        let (mut old_seen, mut new_seen) = (0, 0);
        while old_seen < old_count || new_seen < new_count {
            let raw = lines
                .next()
                .ok_or_else(|| invalid("truncated unified hunk"))?;
            let kind = *raw
                .as_bytes()
                .first()
                .ok_or_else(|| invalid("empty hunk line"))?;
            if !matches!(kind, b' ' | b'+' | b'-') || !raw.ends_with('\n') {
                return Err(invalid("invalid unified body line"));
            }
            old_seen += usize::from(kind != b'+');
            new_seen += usize::from(kind != b'-');
            if old_seen > old_count || new_seen > new_count {
                return Err(invalid("unified hunk count mismatch"));
            }
            // CR before the diff's LF is transport framing, just as in the source.
            let text = raw[1..]
                .strip_suffix('\n')
                .unwrap_or_default()
                .strip_suffix('\r')
                .unwrap_or_else(|| &raw[1..raw.len() - 1]);
            let terminated = if lines.peek().is_some_and(|line| {
                line.trim_end_matches(['\r', '\n']) == "\\ No newline at end of file"
            }) {
                lines.next();
                false
            } else {
                true
            };
            hunk.lines.push(Line {
                kind,
                text,
                terminated,
            });
        }
        if old_count == 0 && new_count == 0 {
            return Err(invalid("empty unified hunk"));
        }
        for excluded in *b"+-" {
            let mut side = hunk
                .lines
                .iter()
                .filter(|line| line.kind != excluded)
                .peekable();
            while let Some(line) = side.next() {
                if !line.terminated && (side.peek().is_some() || lines.peek().is_some()) {
                    return Err(invalid(
                        "no-newline marker is not on the final line of a side",
                    ));
                }
            }
        }
        hunks.push(hunk);
    }
    if hunks.is_empty() {
        return Err(invalid("diff contains no hunks"));
    }
    Ok(hunks)
}

fn coordinate(input: &str) -> Result<(usize, usize), StopReason> {
    let (start, count) = input.split_once(',').unwrap_or((input, "1"));
    if !start.bytes().all(|b| b.is_ascii_digit()) || !count.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid("invalid hunk coordinate"));
    }
    let start = start
        .parse::<usize>()
        .map_err(|_| invalid("invalid hunk start"))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| invalid("invalid hunk count"))?;
    Ok((
        if count == 0 {
            start
        } else {
            start
                .checked_sub(1)
                .ok_or_else(|| invalid("nonempty hunk starts at zero"))?
        },
        count,
    ))
}

fn source_lines(source: &str) -> Vec<SourceLine<'_>> {
    let mut start = 0;
    source
        .split_inclusive('\n')
        .map(|raw| {
            let ending = if raw.ends_with("\r\n") {
                "\r\n"
            } else if raw.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let line = SourceLine {
                start,
                text: &raw[..raw.len() - ending.len()],
                ending,
            };
            start += raw.len();
            line
        })
        .collect()
}

fn matches_at(lines: &[SourceLine<'_>], hunk: &Hunk<'_>, position: usize) -> bool {
    let Some(end) = position.checked_add(hunk.old_count) else {
        return false;
    };
    let Some(actual) = lines.get(position..end) else {
        return false;
    };
    if hunk
        .lines
        .iter()
        .any(|line| line.kind != b'-' && !line.terminated)
        && end != lines.len()
    {
        return false;
    }
    if hunk.old_count == 0 && position > 0 && lines[position - 1].ending.is_empty() {
        return false;
    }
    hunk.lines
        .iter()
        .filter(|line| line.kind != b'+')
        .zip(actual)
        .all(|(expected, actual)| {
            expected.text == actual.text && expected.terminated != actual.ending.is_empty()
        })
}

fn append_edits(
    source: &str,
    lines: &[SourceLine<'_>],
    hunk: &Hunk<'_>,
    position: usize,
    plan: &mut EditPlan,
) {
    let mut cursor = position;
    let ending = lines
        .get(position)
        .filter(|line| !line.ending.is_empty())
        .or_else(|| lines.iter().find(|line| !line.ending.is_empty()))
        .map_or("\n", |line| line.ending);
    let offset = |index: usize| lines.get(index).map_or(source.len(), |line| line.start);
    let mut pending: Option<TextEdit> = None;
    for line in &hunk.lines {
        if line.kind == b' ' {
            if let Some(edit) = pending.take() {
                plan.edits.push(edit);
            }
            cursor += 1;
            continue;
        }
        let edit = pending.get_or_insert_with(|| TextEdit {
            range: offset(cursor)..offset(cursor),
            replacement: String::new(),
        });
        if line.kind == b'-' {
            cursor += 1;
            edit.range.end = offset(cursor);
        } else {
            edit.replacement.push_str(line.text);
            if line.terminated {
                edit.replacement.push_str(ending);
            }
        }
    }
    if let Some(edit) = pending {
        plan.edits.push(edit);
    }
}
