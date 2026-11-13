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
use std::sync::{Arc, Mutex, PoisonError};

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

/// The refusal the storage adapter recorded for a release it could not honour.
///
/// The governor records a charge that exceeds the scratch limit, but an
/// unbalanced release leaves no trace in it, so the adapter keeps that reason
/// here for [`storage_stop`] to read.
#[derive(Clone, Debug, Default)]
pub(super) struct Rejection(Arc<Mutex<Option<StopReason>>>);

impl Rejection {
    fn record(&self, reason: StopReason) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(reason);
    }

    fn take(&self) -> Option<StopReason> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// Storage admission: the live bytes of one native account, charged to the
/// governor's scratch meter and refunded when the account is released.
#[derive(Debug)]
pub(super) struct Storage {
    meter: ScratchMeter,
    live: usize,
    rejection: Rejection,
}

impl Storage {
    /// A fresh account with no live bytes, and the handle that reports why it
    /// refused a release.
    pub(super) fn new(governor: &Shared<'_>) -> (Self, Rejection) {
        let rejection = Rejection::default();
        let account = Self {
            meter: governor.borrow().scratch_meter(),
            live: 0,
            rejection: rejection.clone(),
        };
        (account, rejection)
    }
}

impl Admission for Storage {
    fn resize(&mut self, live_bytes: usize) -> Result<(), StorageError> {
        let growing = live_bytes >= self.live;
        let delta = u64::try_from(live_bytes.abs_diff(self.live))
            .map_err(|_| StorageError::SizeOverflow)?;
        // The meter keeps an exhausted charge (it is sticky) and the governor
        // restores it; any other refusal is kept in the rejection.
        let result = if growing {
            self.meter.charge(delta)
        } else {
            self.meter.release(delta)
        };
        result.map_err(|reason| {
            self.rejection.record(reason);
            StorageError::AdmissionFailed
        })?;
        self.live = live_bytes;
        Ok(())
    }
}

/// Native work units charged as one governor work unit. The governed parser and
/// `FnO` reader count one native unit per lexical or structural step, which is
/// much finer than this crate's row accounting; charging each step as a work unit
/// would admit only a few kilobytes under the Standard budget. The admitted sizes
/// at 64 native units per work unit are measured in the `graph` module's
/// governance notes.
pub(super) const NATIVE_UNITS_PER_WORK: u64 = 64;

/// Work and stop authority: one governor work unit per
/// [`NATIVE_UNITS_PER_WORK`] native units.
#[derive(Debug)]
pub(super) struct Work<'c, 'g> {
    governor: &'c Shared<'g>,
    /// Native units counted but not yet charged; always below the divisor.
    carried: u64,
}

impl<'c, 'g> Work<'c, 'g> {
    /// A progress authority with no carried native units.
    pub(super) const fn new(governor: &'c Shared<'g>) -> Self {
        Self {
            governor,
            carried: 0,
        }
    }
}

impl Progress for Work<'_, '_> {
    type Error = StopReason;

    fn charge(&mut self, units: u64) -> Result<(), StopReason> {
        // The running total is charged by floor; the remainder is carried so that
        // small native steps are not rounded up one by one. A final remainder
        // below the divisor is never charged.
        let total = self.carried.saturating_add(units);
        self.carried = total % NATIVE_UNITS_PER_WORK;
        // A zero charge still polls cancellation and any recorded stop.
        // INVARIANT: native code runs only while no loader stage holds the borrow.
        self.governor
            .borrow_mut()
            .charge(Resource::Work, total / NATIVE_UNITS_PER_WORK)
    }
}

/// The refusal for a storage failure.
///
/// The governor's first typed reason wins. Otherwise the refusal is the native
/// storage cause itself (a physical or logical limit the governor did not
/// record) or the adapter's rejected release; neither is claimed as a budget.
pub(super) fn storage_stop(
    governor: &Shared<'_>,
    rejection: &Rejection,
    cause: &StorageError,
) -> LoadError {
    if let Err(reason) = governor.borrow().check() {
        return stop(reason);
    }
    rejection.take().map_or_else(
        || read_failure("storage", cause),
        |reason| {
            read_failure(
                "storage-accounting",
                format!("scratch accounting refused a native release ({reason:?}): {cause}"),
            )
        },
    )
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

/// The refusal of a media type the loader does not parse.
pub(super) fn unsupported_format(cause: impl std::fmt::Display) -> LoadError {
    read_failure("unsupported-format", cause)
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
fn refusal(
    refusal: &ParseRefusal<'_, Storage, Work<'_, '_>>,
    governor: &Shared<'_>,
    rejection: &Rejection,
) -> LoadError {
    let native = match refusal.cause() {
        ControlledError::Control(reason) => return stop(reason.clone()),
        ControlledError::Native(native) => native,
    };
    match native {
        NativeParseError::Storage(cause) => storage_stop(governor, rejection, cause),
        NativeParseError::Diagnostic(found) if found.storage_error().is_some() => {
            // INVARIANT: the guard established the storage cause.
            found.storage_error().map_or_else(
                || diagnostic(found),
                |cause| storage_stop(governor, rejection, &cause),
            )
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
        NativeParseError::UnsupportedFormat(_) => unsupported_format(native),
    }
}

/// Parse Turtle or `TriG` under the governor's storage, work and stop authority,
/// retaining the exact ordered source occurrences.
///
/// # Errors
/// A caller stop or exhausted budget the governor recorded is
/// [`LoadError::Exhausted`] or [`LoadError::Cancelled`]; every native failure,
/// including a storage failure the governor did not record, is
/// [`LoadError::Parse`].
pub(super) fn parse<'b, 'c, 'g>(
    bytes: &'b [u8],
    format: NativeRdfFormat,
    base: Option<&str>,
    governor: &'c Shared<'g>,
) -> Result<RetainedParse<'b, Storage, Work<'c, 'g>>, LoadError> {
    let (storage, rejection) = Storage::new(governor);
    parse_dataset_controlled(bytes, format, base, storage, Work::new(governor))
        .map_err(|failed| refusal(&failed, governor, &rejection))
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

    // Rationale: a storage failure the governor did not record must not be claimed as
    // a budget; it keeps its native cause as a parse error. A governor-recorded
    // exhaustion still wins, and a rejected release is its own accounting error.
    // Native work below the divisor is carried, and a zero charge still observes a stop.
    #[test]
    fn unrecorded_storage_failures_keep_their_cause_and_work_is_carried() {
        let mut governor = Governor::new(Budget::default());
        let shared = RefCell::new(&mut governor);
        let (_, rejection) = Storage::new(&shared);
        let refused = storage_stop(&shared, &rejection, &StorageError::AllocationFailed);
        let LoadError::Parse { code, message, .. } = refused else {
            panic!("an unrecorded failure is not a budget claim");
        };
        assert_eq!(code, "storage");
        assert!(message.contains("allocation failed"), "{message}");
        rejection.record(StopReason::InvalidInput("x".into()));
        let refused = storage_stop(&shared, &rejection, &StorageError::AdmissionFailed);
        assert!(matches!(&refused, LoadError::Parse { code, .. } if code == "storage-accounting"));

        let mut work = Work::new(&shared);
        for _ in 0..NATIVE_UNITS_PER_WORK - 1 {
            work.charge(1).expect("below the divisor");
        }
        assert_eq!(shared.borrow().usage().work, 0);
        let mut work = Work::new(&shared);
        work.charge(NATIVE_UNITS_PER_WORK * 2 + 1).expect("charged");
        work.charge(NATIVE_UNITS_PER_WORK - 1).expect("carried");
        assert_eq!(shared.borrow().usage().work, 3);
    }
}
