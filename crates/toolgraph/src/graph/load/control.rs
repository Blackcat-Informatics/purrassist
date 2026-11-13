// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Adapters from the caller's [`Governor`] to `PurRDF`'s admission and progress
//! authorities, and the mapping of its refusals to [`LoadError`].
//!
//! Storage admission charges the governor's own scratch meter, so the governor
//! stays the single accounting authority. Work and cancellation are polled
//! through the governor at each native counting point.

use std::cell::RefCell;
use std::collections::BTreeMap;

use purrassist_core::{Governor, ScratchMeter, StopReason, contract::Resource};
use purrdf_core::{TermValue, dataset_view::DatasetView};
use purrdf_lex::{
    allocation::{Admission, StorageError},
    control::{ControlledError, Progress},
};
use purrdf_rdf::{
    NativeParseError, NativeRdfFormat, ParseRefusal, RetainedParse, SourceOriginId,
    SourceOriginKind, SourceRole, SourceSemanticReference, parse_dataset_controlled,
};

use super::{show, stop};
use crate::graph::{LoadError, SourceExtent};

/// The governor shared between the native parser's progress callback and the
/// loader's own stages. Native calls never run while a stage holds the borrow.
pub(super) type Shared<'g> = RefCell<&'g mut Governor>;

/// Storage admission: the live bytes of one native account, charged to the
/// governor's scratch meter and refunded when the account is released.
#[derive(Debug)]
pub(super) struct Storage {
    meter: ScratchMeter,
    live: usize,
}

impl Storage {
    /// A fresh account with no live bytes.
    pub(super) fn new(governor: &Shared<'_>) -> Self {
        Self {
            meter: governor.borrow().scratch_meter(),
            live: 0,
        }
    }
}

impl Admission for Storage {
    fn resize(&mut self, live_bytes: usize) -> Result<(), StorageError> {
        let growing = live_bytes >= self.live;
        let delta = u64::try_from(live_bytes.abs_diff(self.live))
            .map_err(|_| StorageError::SizeOverflow)?;
        // The meter keeps the typed reason (it is sticky); the governor restores it.
        let result = if growing {
            self.meter.charge(delta)
        } else {
            self.meter.release(delta)
        };
        result.map_err(|_| StorageError::AdmissionFailed)?;
        self.live = live_bytes;
        Ok(())
    }
}

/// Work and stop authority: one governor work unit per native step.
#[derive(Debug)]
pub(super) struct Work<'c, 'g>(pub(super) &'c Shared<'g>);

impl Progress for Work<'_, '_> {
    type Error = StopReason;

    fn charge(&mut self, units: u64) -> Result<(), StopReason> {
        // INVARIANT: native code runs only while no loader stage holds the borrow.
        self.0.borrow_mut().charge(Resource::Work, units)
    }
}

/// The refusal for a storage failure, with the governor's first typed reason.
pub(super) fn storage_stop(governor: &Shared<'_>) -> LoadError {
    match governor.borrow().check() {
        Err(reason) => stop(reason),
        // A physical or logical native limit the governor did not record.
        Ok(()) => LoadError::Exhausted(StopReason::BudgetExceeded(Resource::ScratchBytes)),
    }
}

/// A source-read failure with a stable code; the dataset is the parser's own.
pub(super) fn read_failure(code: &str, cause: impl std::fmt::Display) -> LoadError {
    LoadError::Parse {
        code: code.to_owned(),
        message: cause.to_string(),
        line: None,
        column: None,
    }
}

/// A native diagnostic as the typed parse error, keeping its code and position.
pub(super) fn diagnostic(diagnostic: &purrdf_core::diagnostic::RdfDiagnostic) -> LoadError {
    let location = diagnostic.location.as_deref();
    LoadError::Parse {
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
        line: location.and_then(|at| at.line),
        column: location.and_then(|at| at.column),
    }
}

fn positioned(code: &str, message: String, line: u64, column: u32) -> LoadError {
    LoadError::Parse {
        code: code.to_owned(),
        message,
        line: (line != 0).then_some(line),
        column: (column != 0).then_some(column),
    }
}

/// Map a native or caller failure of the governed parse. A caller stop or
/// storage refusal is never reported as invalid syntax.
fn refusal(refusal: &ParseRefusal<'_, Storage, Work<'_, '_>>, governor: &Shared<'_>) -> LoadError {
    let native = match refusal.cause() {
        ControlledError::Control(reason) => return stop(reason.clone()),
        ControlledError::Native(native) => native,
    };
    match native {
        NativeParseError::Storage(_) => storage_stop(governor),
        NativeParseError::Diagnostic(found) if found.storage_error().is_some() => {
            storage_stop(governor)
        }
        NativeParseError::Diagnostic(found) => diagnostic(found),
        NativeParseError::Lexical {
            cause,
            line,
            column,
        } => positioned("lexical", cause.to_string(), *line, *column),
        NativeParseError::Iri {
            cause,
            line,
            column,
        } => positioned("iri", cause.to_string(), *line, *column),
        NativeParseError::Position { cause, line } => {
            positioned("position", cause.to_string(), *line, 0)
        }
        NativeParseError::Utf8 { cause, offset } => {
            read_failure("utf8", format!("byte {offset}: {cause}"))
        }
        NativeParseError::Base { cause } => read_failure("base", cause),
        NativeParseError::Composite(cause) => read_failure("composite", cause),
        NativeParseError::SourceAssociation(cause) => read_failure("source-association", cause),
        NativeParseError::UnsupportedFormat(_) => read_failure("unsupported-format", native),
    }
}

/// Parse Turtle or `TriG` under the governor's storage, work and stop authority,
/// retaining the exact ordered source occurrences.
///
/// # Errors
/// A caller stop or exhausted budget is [`LoadError::Exhausted`] or
/// [`LoadError::Cancelled`]; every native failure is [`LoadError::Parse`].
pub(super) fn parse<'b, 'c, 'g>(
    bytes: &'b [u8],
    format: NativeRdfFormat,
    base: Option<&str>,
    governor: &'c Shared<'g>,
) -> Result<RetainedParse<'b, Storage, Work<'c, 'g>>, LoadError> {
    parse_dataset_controlled(bytes, format, base, Storage::new(governor), Work(governor))
        .map_err(|failed| refusal(&failed, governor))
}

/// The statement a diagnostic refers to, as far as the error names it.
struct Probe<'a> {
    subject: &'a str,
    predicate: &'a str,
    term: Option<&'a str>,
}

/// Attach the exact source extent to a statement-level refusal.
///
/// The extent is that of the first statement in source order whose subject and
/// predicate (and, for an unknown term, whose subject, predicate or object) match
/// the error. Errors that name a node rather than a statement stay unlocated.
pub(super) fn locate(error: &mut LoadError, parse: &RetainedParse<'_, Storage, Work<'_, '_>>) {
    let (probe, slot) = match error {
        LoadError::UnknownTerm {
            subject,
            predicate,
            term,
            at,
        } => (
            Probe {
                subject,
                predicate,
                term: Some(term),
            },
            at,
        ),
        LoadError::UnsupportedConstruct {
            subject,
            predicate,
            at,
            ..
        } => (
            Probe {
                subject,
                predicate,
                term: None,
            },
            at,
        ),
        _ => return,
    };
    let found = extent(parse, &probe);
    *slot = found;
}

fn extent(
    parse: &RetainedParse<'_, Storage, Work<'_, '_>>,
    probe: &Probe<'_>,
) -> Option<SourceExtent> {
    let view = parse.dataset()?;
    let source = parse.source();
    let mut statements: BTreeMap<usize, [Option<SourceOriginId>; 3]> = BTreeMap::new();
    for occurrence in source.occurrences() {
        let (Some(statement), slot) = (
            occurrence.statement(),
            match occurrence.role() {
                SourceRole::Subject => 0,
                SourceRole::Predicate => 1,
                SourceRole::Object => 2,
                SourceRole::Graph | SourceRole::GraphDeclaration => continue,
            },
        ) else {
            continue;
        };
        statements.entry(statement).or_default()[slot] = Some(occurrence.origin());
    }
    let term = |origin: SourceOriginId| -> Option<TermValue> {
        let mut at = origin;
        loop {
            let found = source.origin(at)?;
            match (found.semantic(), found.kind()) {
                (Some(SourceSemanticReference::Term(id)), _) => return view.term_value(id).ok(),
                (Some(SourceSemanticReference::ConstantIri(iri)), _) => {
                    return Some(TermValue::Iri(iri.to_owned()));
                }
                (_, SourceOriginKind::Inherited { from }) => at = *from,
                _ => return None,
            }
        }
    };
    let range = |origin: SourceOriginId| -> Option<std::ops::Range<usize>> {
        let mut at = origin;
        loop {
            match source.origin(at)?.kind() {
                SourceOriginKind::Written(range) => return Some(range.clone()),
                SourceOriginKind::Generated { anchor, .. } => return Some(anchor.clone()),
                SourceOriginKind::Inherited { from } => at = *from,
            }
        }
    };
    for origins in statements.into_values() {
        let [Some(subject), Some(predicate), Some(object)] = origins else {
            continue;
        };
        let (Some(s), Some(p), Some(o)) = (term(subject), term(predicate), term(object)) else {
            continue;
        };
        let matches = show(&s) == probe.subject
            && show(&p) == probe.predicate
            && probe.term.is_none_or(|wanted| {
                [&s, &p, &o]
                    .iter()
                    .any(|candidate| show(candidate) == wanted)
            });
        if !matches {
            continue;
        }
        let (Some(p_range), Some(o_range)) = (range(predicate), range(object)) else {
            continue;
        };
        return Some(SourceExtent::new(
            source.bytes(),
            p_range.start.min(o_range.start)..p_range.end.max(o_range.end),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use purrassist_core::{Budget, CancellationToken};

    use super::*;

    // Rationale: the governed parse must poll the caller's stop authority, so a
    // cancelled governor refuses with `Cancelled` and yields no dataset. The
    // up-front input charge in `load` also checks cancellation, so this drives the
    // parse entry point directly; the uncancelled neighbour proves it is the
    // cancellation, not the input, that refuses.
    #[test]
    fn cancellation_stops_the_governed_parse_without_a_dataset() {
        let source = b"<urn:a> <urn:b> <urn:c> .";
        let token = CancellationToken::new();
        let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
        let shared = RefCell::new(&mut governor);
        let parsed = parse(source, NativeRdfFormat::Turtle, None, &shared).expect("not cancelled");
        assert!(parsed.dataset().is_some());
        drop(parsed);
        token.cancel();
        let refused = parse(source, NativeRdfFormat::Turtle, None, &shared);
        assert!(
            matches!(refused, Err(LoadError::Cancelled)),
            "{:?}",
            refused.map(|_| ())
        );
    }
}
