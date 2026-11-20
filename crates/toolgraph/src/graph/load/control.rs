// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Adapters from the caller's [`Governor`] to `PurRDF`'s admission and progress
//! authorities, and the mapping of its refusals to [`LoadError`].
//!
//! Storage admission charges the governor's own scratch meter, so the governor
//! stays the single accounting authority. Work and cancellation are polled
//! through the governor at each native counting point.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use purrassist_core::{
    Governor, ScratchMeter, StopReason,
    contract::{Resource, saturating_size},
};
use purrdf_core::{QuadIds, TermId, TermRef, TermValue, dataset_view::DatasetView};
use purrdf_lex::{
    allocation::{Admission, Memory, OwnedAdmission, StorageError},
    control::{ControlledError, Progress},
};
use purrdf_rdf::{
    BorrowedSource, NativeParseError, NativeRdfFormat, ParseRefusal, RetainedParse, SourceOriginId,
    SourceOriginKind, SourceRole, SourceSemanticReference, SourceStatementKind,
    parse_dataset_controlled,
};

use super::{Reifier, Row, stop, work};
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

/// The original account of a temporary native builder and its frozen view.
pub(super) struct ControlledStorage<'c, 'g> {
    storage: Storage,
    work: Work<'c, 'g>,
}

impl<'c, 'g> ControlledStorage<'c, 'g> {
    /// Keep the grant alive until both the builder and its frozen view die.
    pub(super) fn owned(governor: &'c Shared<'g>) -> (OwnedAdmission<Self>, Rejection) {
        let (storage, rejection) = Storage::new(governor);
        let account = Self {
            storage,
            work: Work::new(governor),
        };
        (OwnedAdmission::new(account), rejection)
    }
}

impl Admission for ControlledStorage<'_, '_> {
    fn resize(&mut self, live_bytes: usize) -> Result<(), StorageError> {
        self.storage.resize(live_bytes)
    }

    fn progress_enabled(&self) -> bool {
        true
    }

    fn checkpoint(&mut self, units: u64) -> Result<(), StorageError> {
        self.work.charge(units).map_err(|reason| {
            self.storage.rejection.record(reason);
            StorageError::AdmissionFailed
        })
    }
}

/// Native work units charged as one governor work unit. The governed parser and
/// `FnO` reader count one native unit per lexical or structural step, which is
/// much finer than this crate's row accounting; charging each step as a work unit
/// would limit accepted input under the Standard budget.
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
    accounted_storage_stop(governor, rejection, cause)
        .unwrap_or_else(|| read_failure("storage", cause))
}

/// Preserve a caller stop or an account failure without choosing a native error category.
pub(super) fn accounted_storage_stop(
    governor: &Shared<'_>,
    rejection: &Rejection,
    cause: &StorageError,
) -> Option<LoadError> {
    if let Err(reason) = governor.borrow().check() {
        return Some(stop(reason));
    }
    rejection.take().map(|reason| {
        read_failure(
            "storage-accounting",
            format!("scratch accounting refused a native release ({reason:?}): {cause}"),
        )
    })
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

/// Match the reifier and the complete embedded triple behind its written occurrence.
pub(super) fn default_reifier_extent<V: DatasetView<Id = TermId>>(
    view: &V,
    source: &BorrowedSource<'_>,
    reifier: &Reifier,
    governor: &mut Governor,
) -> Result<Option<SourceExtent>, LoadError> {
    find_statement(source, governor, |statement, [s, _, o], graph, governor| {
        if graph.is_some()
            || source.statement_kind(statement) != Some(SourceStatementKind::Reifier)
            || !matches_value(view, source, s, &reifier.id, governor)?
        {
            return Ok(false);
        }
        let Some(triple) = term_id(source, o, governor)? else {
            return Ok(false);
        };
        let terms = view
            .with_term(triple, |term| match term {
                TermRef::Triple { s, p, o } => Some((s, p, o)),
                _ => None,
            })
            .map_err(|cause| read_failure("dataset-read", cause))?;
        let Some((s, p, o)) = terms else {
            return Ok(false);
        };
        Ok(matches_term(view, s, &reifier.s, governor)?
            && matches_iri(view, p, &reifier.p, governor)?
            && matches_term(view, o, &reifier.o, governor)?)
    })
}

/// Locate the written occurrence of the selected classified default-graph row.
pub(super) fn classified_row_extent<V: DatasetView<Id = TermId>>(
    view: &V,
    source: &BorrowedSource<'_>,
    row: &Row,
    governor: &mut Governor,
) -> Result<Option<SourceExtent>, LoadError> {
    find_statement(source, governor, |statement, [s, p, o], graph, governor| {
        if graph.is_some()
            || source.statement_kind(statement) != Some(row.layer)
            || term_id(source, s, governor)? != Some(row.subject_id)
            || term_id(source, o, governor)? != Some(row.object_id)
        {
            return Ok(false);
        }
        let Some(predicate) = term_id(source, p, governor)? else {
            return Ok(false);
        };
        matches_iri(view, predicate, &row.p, governor)
    })
}

/// Locate a native diagnostic's complete default-graph statement by RDF value.
pub(super) fn default_row_extent<V: DatasetView<Id = TermId>>(
    view: &V,
    source: &BorrowedSource<'_>,
    subject: &TermValue,
    predicate: &str,
    object: &TermValue,
    governor: &mut Governor,
) -> Result<Option<SourceExtent>, LoadError> {
    default_row_extent_or_iri(
        view,
        source,
        subject,
        predicate,
        Some(object),
        None,
        governor,
    )
}

/// Locate a statement that may have an explicitly normalized asserted object.
pub(super) fn default_row_extent_or_iri<V: DatasetView<Id = TermId>>(
    view: &V,
    source: &BorrowedSource<'_>,
    subject: &TermValue,
    predicate: &str,
    object: Option<&TermValue>,
    alias: Option<&str>,
    governor: &mut Governor,
) -> Result<Option<SourceExtent>, LoadError> {
    find_statement(source, governor, |statement, [s, p, o], graph, governor| {
        let Some(p) = term_id(source, p, governor)? else {
            return Ok(false);
        };
        if graph.is_some()
            || !matches_iri(view, p, predicate, governor)?
            || !matches_value(view, source, s, subject, governor)?
        {
            return Ok(false);
        }
        let Some(o) = term_id(source, o, governor)? else {
            return Ok(false);
        };
        if let Some(object) = object
            && matches_term(view, o, object, governor)?
        {
            return Ok(true);
        }
        if let Some(iri) = alias
            && source.statement_kind(statement) == Some(SourceStatementKind::Quad)
        {
            return matches_iri(view, o, iri, governor);
        }
        Ok(false)
    })
}

fn matches_value<V: DatasetView<Id = TermId>>(
    view: &V,
    source: &BorrowedSource<'_>,
    origin: SourceOriginId,
    expected: &TermValue,
    governor: &mut Governor,
) -> Result<bool, LoadError> {
    let Some(id) = term_id(source, origin, governor)? else {
        return Ok(false);
    };
    matches_term(view, id, expected, governor)
}

fn matches_iri<V: DatasetView<Id = TermId>>(
    view: &V,
    id: TermId,
    expected: &str,
    governor: &mut Governor,
) -> Result<bool, LoadError> {
    view.with_term(id, |actual| match actual {
        TermRef::Iri(iri) => matches_text(iri, expected, governor),
        _ => Ok(false),
    })
    .map_err(|cause| read_failure("dataset-read", cause))?
}

fn matches_term<V: DatasetView<Id = TermId>>(
    view: &V,
    id: TermId,
    expected: &TermValue,
    governor: &mut Governor,
) -> Result<bool, LoadError> {
    // Most matching candidates are named resources or list cells. Compare
    // those borrowed terms before allocating storage for richer RDF values.
    if matches!(expected, TermValue::Iri(_) | TermValue::Blank { .. }) {
        return view
            .with_term(id, |actual| match (actual, expected) {
                (TermRef::Iri(actual), TermValue::Iri(expected)) => {
                    matches_text(actual, expected, governor)
                }
                (
                    TermRef::Blank { label, scope },
                    TermValue::Blank {
                        label: expected,
                        scope: wanted,
                    },
                ) => Ok(scope == *wanted && matches_text(label, expected, governor)?),
                _ => Ok(false),
            })
            .map_err(|cause| read_failure("dataset-read", cause))?;
    }
    let shared = RefCell::new(governor);
    let (mut account, rejection) = ControlledStorage::owned(&shared);
    let mut memory = Memory::new(&mut account);
    let actual = view.term_value_with_memory(
        id,
        &mut memory,
        |cause| read_failure("dataset-read", cause),
        |cause, _| storage_stop(&shared, &rejection, &cause),
    )?;
    let mut progress = Work::new(&shared);
    actual
        .try_cmp_with_control(expected, &mut memory, &mut |units| progress.charge(units))
        .map(std::cmp::Ordering::is_eq)
        .map_err(|failure| match failure {
            ControlledError::Control(reason) => stop(reason),
            ControlledError::Native(cause) => storage_stop(&shared, &rejection, &cause),
        })
}

/// Locate the exact refused RDF statement, including its graph and native layer.
pub(super) fn quad_extent(
    source: &BorrowedSource<'_>,
    quad: QuadIds,
    layer: SourceStatementKind,
    governor: &mut Governor,
) -> Result<Option<SourceExtent>, LoadError> {
    find_statement(source, governor, |statement, [s, p, o], graph, governor| {
        let (Some(graph), Some(graph_id)) = (graph, quad.g) else {
            return Ok(false);
        };
        Ok(source.statement_kind(statement) == Some(layer)
            && term_id(source, graph, governor)? == Some(graph_id)
            && term_id(source, s, governor)? == Some(quad.s)
            && term_id(source, p, governor)? == Some(quad.p)
            && term_id(source, o, governor)? == Some(quad.o))
    })
}

/// Locate a graph declaration even when the named graph contains no statements.
pub(super) fn graph_extent(
    source: &BorrowedSource<'_>,
    graph: TermId,
    governor: &mut Governor,
) -> Result<Option<SourceExtent>, LoadError> {
    for occurrence in source.occurrences() {
        work(governor, 1)?;
        if occurrence.role() == SourceRole::GraphDeclaration
            && term_id(source, occurrence.origin(), governor)? == Some(graph)
            && let Some(range) = origin_range(source, occurrence.origin(), governor)?
        {
            return extent(source, range, governor).map(Some);
        }
    }
    Ok(None)
}

fn matches_text(left: &str, right: &str, governor: &mut Governor) -> Result<bool, LoadError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    work(governor, saturating_size(left.len()))?;
    Ok(left == right)
}

fn find_statement(
    source: &BorrowedSource<'_>,
    governor: &mut Governor,
    mut matches: impl FnMut(
        usize,
        [SourceOriginId; 3],
        Option<SourceOriginId>,
        &mut Governor,
    ) -> Result<bool, LoadError>,
) -> Result<Option<SourceExtent>, LoadError> {
    let mut occurrences = source.occurrences().peekable();
    while let Some(first) = occurrences.next() {
        work(governor, 1)?;
        let Some(statement) = first.statement() else {
            continue;
        };
        let mut origins = [None; 4];
        let mut occurrence = first;
        loop {
            let slot = match occurrence.role() {
                SourceRole::Subject => 0,
                SourceRole::Predicate => 1,
                SourceRole::Object => 2,
                SourceRole::Graph | SourceRole::GraphDeclaration => 3,
            };
            origins[slot] = Some(occurrence.origin());
            let Some(next) = occurrences.next_if(|next| next.statement() == Some(statement)) else {
                break;
            };
            work(governor, 1)?;
            occurrence = next;
        }
        if let [Some(s), Some(p), Some(o), graph] = origins
            && matches(statement, [s, p, o], graph, governor)?
            && let (Some(p), Some(o)) = (
                origin_range(source, p, governor)?,
                origin_range(source, o, governor)?,
            )
        {
            return extent(source, p.start.min(o.start)..p.end.max(o.end), governor).map(Some);
        }
    }
    Ok(None)
}

/// Folded statement roots and graph declarations carry final dataset term IDs.
fn term_id(
    source: &BorrowedSource<'_>,
    origin: SourceOriginId,
    governor: &mut Governor,
) -> Result<Option<TermId>, LoadError> {
    work(governor, 1)?;
    Ok(
        match source
            .origin(origin)
            .and_then(purrdf_rdf::SourceTermOrigin::semantic)
        {
            Some(SourceSemanticReference::Term(id)) => Some(id),
            _ => None,
        },
    )
}

fn origin_range(
    source: &BorrowedSource<'_>,
    mut origin: SourceOriginId,
    governor: &mut Governor,
) -> Result<Option<Range<usize>>, LoadError> {
    while let Some(found) = source.origin(origin) {
        work(governor, 1)?;
        match found.kind() {
            SourceOriginKind::Written(range) => return Ok(Some(range.clone())),
            SourceOriginKind::Generated { anchor, .. } => return Ok(Some(anchor.clone())),
            SourceOriginKind::Inherited { from } => origin = *from,
        }
    }
    Ok(None)
}

fn extent(
    source: &BorrowedSource<'_>,
    range: Range<usize>,
    governor: &mut Governor,
) -> Result<SourceExtent, LoadError> {
    work(governor, saturating_size(range.start).saturating_mul(4))?;
    Ok(SourceExtent::new(source.bytes(), range))
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
