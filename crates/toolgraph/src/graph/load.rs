// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The loading algorithm: materialise rows, own every row, validate, then type.

mod classification;
mod control;
mod profile;
mod roles;

use std::cell::RefCell;

use std::convert::Infallible;
use std::ops::ControlFlow;

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, ScratchLease, saturating_size},
    output::FieldMask,
};
use purrdf_core::{
    NativeBuildError, RdfDataset, RdfDatasetBuilder, TermId, TermRef, TermValue,
    blank_label::{LabelAlphabet, encode_blank_label_with_memory},
    dataset_from_view,
    dataset_view::DatasetView,
    fno::{FnBlank, FnoCatalog, FnoFieldKind, FnoReadError, FnoReadFailure},
    loss::{LossEntry, LossLedger, LossSite, LossSubject},
};
use purrdf_lex::{
    allocation::{Admission, Memory, OrderedTable, OwnedAdmission, StorageError},
    control::{ControlledError, Progress, compare_bytes},
    walk::{try_equal_range_by, try_sort_unstable_by},
};
use purrdf_rdf::{BorrowedSource, SourceStatementKind};

use super::{
    Coercion, Construct, Hint, LoadError, ProfileRule, SourceExtent, Tool, ToolAnnotations,
    ToolExecutionProfile, ToolGraph, ToolImplementation, ToolOutput, ToolParameter, Violation,
    Waiver,
};
use crate::{
    CallableBinding, ContextManagement, FilePermissions, FileRuleError, LoadingStrategy,
    OutputByteLimit, ResourceLimits, Vocabulary, codec::mcp::ExecutionMode,
    vocab::FNOM_MAPPING_NORMALIZATION,
};
use classification::{Nodes, RoleNode};
use control::{ControlledStorage, Shared, Storage, Work};
pub(super) use profile::BuildStorage;
use profile::{Profiles, matches_predicate};
use roles::{Kind, Role, Spec};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const RDF_PROPERTY: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#Property";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";
const RDFS_CLASS: &str = "http://www.w3.org/2000/01/rdf-schema#Class";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const OWL_EQUIVALENT_CLASS: &str = "http://www.w3.org/2002/07/owl#equivalentClass";
const DCTERMS_IS_PART_OF: &str = "http://purl.org/dc/terms/isPartOf";
const SH: &str = "http://www.w3.org/ns/shacl#";
const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";
const SH_PROPERTY: &str = "http://www.w3.org/ns/shacl#property";
const SH_NODE_SHAPE: &str = "http://www.w3.org/ns/shacl#NodeShape";
const SH_PROPERTY_SHAPE: &str = "http://www.w3.org/ns/shacl#PropertyShape";
const FNO: &str = "https://w3id.org/function/ontology#";
const FNOM: &str = "https://w3id.org/function/vocabulary/mapping#";
const FNO_FUNCTION: &str = "https://w3id.org/function/ontology#Function";
const FNO_MAPPED_FUNCTION: &str = "https://w3id.org/function/ontology#function";
const FNO_MAPPING: &str = "https://w3id.org/function/ontology#Mapping";
const FNOM_MAPPING: &str = "https://w3id.org/function/vocabulary/mapping#Mapping";
const FNO_EXPECTS: &str = "https://w3id.org/function/ontology#expects";
const FNO_RETURNS: &str = "https://w3id.org/function/ontology#returns";

#[derive(Debug)]
struct Row {
    subject_id: TermId,
    object_id: TermId,
    layer: SourceStatementKind,
    s: TermValue,
    p: String,
    o: TermValue,
}

impl Row {
    fn declares_mapping_alias(&self) -> bool {
        self.layer == SourceStatementKind::Quad
            && self.p == OWL_EQUIVALENT_CLASS
            && iri_of(&self.s) == Some(FNOM_MAPPING)
            && iri_of(&self.o) == Some(FNO_MAPPING)
    }

    fn uses_mapping_alias(&self) -> bool {
        self.layer == SourceStatementKind::Quad
            && self.p == RDF_TYPE
            && iri_of(&self.o) == Some(FNOM_MAPPING)
    }
}

#[derive(Clone, Copy)]
struct Node<'a> {
    id: TermId,
    term: &'a TermValue,
}

impl<'a> Node<'a> {
    const fn object(row: &'a Row) -> Self {
        Self {
            id: row.object_id,
            term: &row.o,
        }
    }
}

#[derive(Debug)]
struct Reifier {
    term_id: TermId,
    target: (TermId, TermId),
    id: TermValue,
    s: TermValue,
    p: String,
    o: TermValue,
}

/// Temporary lookup and consumption state for parameter-path reifiers.
struct HintBindings {
    by_path: Vec<usize>,
    consumed: Vec<bool>,
}

impl HintBindings {
    fn new(
        reifiers: &[Reifier],
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<Self, LoadError> {
        let mut by_path = Vec::new();
        for (index, reifier) in reifiers.iter().enumerate() {
            storage.charge(1)?;
            if reifier.p == SH_PATH && iri_of(&reifier.o).is_some() {
                storage.push(&mut by_path, index)?;
            }
        }
        // Keep native row order within each path for the duplicate diagnostic.
        try_sort_unstable_by(&mut by_path, |&left, &right| {
            storage.charge(1)?;
            let left_key = (reifiers[left].target, left);
            let right_key = (reifiers[right].target, right);
            Ok::<_, LoadError>(left_key.cmp(&right_key))
        })?;
        let mut consumed = Vec::new();
        storage.fill(&mut consumed, reifiers.len(), false)?;
        Ok(Self { by_path, consumed })
    }
}

/// Owned materialisation of a parsed dataset, indexed by subject.
struct Doc<'v> {
    vocabulary: &'v Vocabulary,
    namespace: &'v str,
    names: Vec<(&'v str, &'static str)>,
    rows: Vec<Row>,
    by_subject: Vec<usize>,
    reifiers: Vec<Reifier>,
    mapping_alias_declared: bool,
}

/// The typed tools and `FnO` ledger drop before their leases and native account.
#[derive(Debug)]
pub(super) struct RetainedTools {
    tools: Vec<Tool>,
    masks: Vec<ScratchLease>,
    fno_view_losses: LossLedger,
    storage: OwnedAdmission<Storage>,
}

impl RetainedTools {
    pub(super) fn as_slice(&self) -> &[Tool] {
        &self.tools
    }

    pub(super) const fn fno_view_losses(&self) -> &LossLedger {
        &self.fno_view_losses
    }
}

pub(super) fn show(term: &TermValue) -> String {
    match term {
        TermValue::Iri(iri) => iri.clone(),
        TermValue::Blank { label, .. } => format!("_:{label}"),
        TermValue::Literal { lexical_form, .. } => format!("{lexical_form:?}"),
        TermValue::Triple { .. } => "<<triple>>".to_owned(),
    }
}

fn stop(reason: StopReason) -> LoadError {
    match reason {
        StopReason::Cancelled => LoadError::Cancelled,
        other => LoadError::Exhausted(other),
    }
}

fn work(governor: &mut Governor, amount: u64) -> Result<(), LoadError> {
    governor.charge(Resource::Work, amount).map_err(stop)
}

pub(super) fn with_build_storage<T>(
    governor: &mut Governor,
    run: impl FnOnce(&mut BuildStorage<'_, '_, '_>) -> T,
) -> T {
    let governor = RefCell::new(governor);
    let (account, rejection) = Storage::new(&governor);
    let mut account = OwnedAdmission::new(account);
    run(&mut BuildStorage::new(&mut account, &governor, rejection))
}

fn iri_of(term: &TermValue) -> Option<&str> {
    match term {
        TermValue::Iri(iri) => Some(iri),
        _ => None,
    }
}

pub(super) fn load(
    bytes: &[u8],
    media_type: &str,
    base: Option<&str>,
    vocabulary: &Vocabulary,
    governor: &mut Governor,
) -> Result<ToolGraph, LoadError> {
    governor
        .charge(
            Resource::InputBytes,
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        )
        .map_err(stop)?;
    let format = purrdf_rdf::classify(media_type)
        .map_err(|found| control::unsupported_format(&found.message))?;
    let governor = RefCell::new(governor);
    let parse = control::parse(bytes, format, base, &governor)?;
    let view = parse
        .dataset()
        // INVARIANT: a parse that was not refused retains its dataset.
        .expect("a successful governed parse retains its dataset");
    let source = parse.source();
    let (tools, catalog) = analyse(&view, &source, vocabulary, &governor)?;
    // `PurRDF` exposes only a borrowed retained parse. The published dataset
    // copy is outside the governor's storage account.
    let dataset = dataset_from_view(&view)
        .map_err(|cause| control::read_failure("dataset-materialisation", cause))?;
    governor.borrow().check().map_err(stop)?;
    Ok(ToolGraph {
        tools,
        dataset,
        catalog,
    })
}

/// Run every stage over the parsed rows; nothing escapes unless all stages pass.
fn analyse<V: DatasetView<Id = TermId>>(
    view: &V,
    source: &BorrowedSource<'_>,
    vocabulary: &Vocabulary,
    governor: &Shared<'_>,
) -> Result<(RetainedTools, Option<FnoCatalog>), LoadError>
where
    V::ReadError: std::fmt::Display,
{
    let locations = Locations { view, source };
    with_doc(view, source, vocabulary, governor, |doc| {
        doc.scan_vocabulary(&locations, governor)?;
        doc.with_classification(&locations, governor, |nodes| {
            let used_alias = doc.own_every_row(nodes, &locations, governor)?;
            doc.validate_nodes(nodes, &locations, governor)?;
            let catalog = doc.read_catalog(nodes, view, source, governor)?;
            let tools =
                doc.build_tools(nodes, catalog.as_ref(), used_alias, &locations, governor)?;
            Ok((tools, catalog))
        })
    })
}

/// Retain the original row and index account through all consumers of the document.
fn with_doc<V: DatasetView<Id = TermId>, T>(
    view: &V,
    source: &BorrowedSource<'_>,
    vocabulary: &Vocabulary,
    governor: &Shared<'_>,
    read: impl FnOnce(&Doc<'_>) -> Result<T, LoadError>,
) -> Result<T, LoadError> {
    let (mut account, rejection) = ControlledStorage::owned(governor);
    let doc = Doc::collect(
        view,
        source,
        vocabulary,
        governor,
        &mut Memory::new(&mut account),
        &|cause| control::storage_stop(governor, &rejection, &cause),
    )?;
    read(&doc)
}

// Each subject's rows keep their original encounter order.
fn subject_index<S: Admission>(
    rows: &[Row],
    memory: &mut Memory<'_, S>,
    progress: &mut impl FnMut(u64) -> Result<(), LoadError>,
    storage_error: &impl Fn(StorageError) -> LoadError,
) -> Result<Vec<usize>, LoadError> {
    let mut by_subject = Vec::new();
    memory
        .reserve(&mut by_subject, rows.len())
        .map_err(storage_error)?;
    memory
        .extend(&mut by_subject, 0..rows.len())
        .map_err(storage_error)?;
    try_sort_unstable_by(&mut by_subject, |&left, &right| {
        rows[left]
            .s
            .try_cmp_with_control(&rows[right].s, memory, progress)
            .map(|order| order.then(left.cmp(&right)))
            .map_err(|failure| match failure {
                ControlledError::Native(cause) => storage_error(cause),
                ControlledError::Control(error) => error,
            })
    })?;
    Ok(by_subject)
}

fn vocabulary_index<'v, S: Admission>(
    vocabulary: &'v Vocabulary,
    governor: &Shared<'_>,
    memory: &mut Memory<'_, S>,
    progress: &mut impl FnMut(u64) -> Result<(), LoadError>,
    storage_error: &impl Fn(StorageError) -> LoadError,
) -> Result<Vec<(&'v str, &'static str)>, LoadError> {
    let mut names = Vec::new();
    let bindings = &vocabulary.terms().local_names;
    memory
        .reserve(&mut names, bindings.len())
        .map_err(storage_error)?;
    for (name, iri) in bindings {
        work(&mut governor.borrow_mut(), 1)?;
        if let Ok(at) = crate::TERM_NAMES.binary_search(&name.as_str()) {
            memory
                .push(&mut names, (iri.as_str(), crate::TERM_NAMES[at]))
                .map_err(storage_error)?;
        }
    }
    try_sort_unstable_by(&mut names, |(left, _), (right, _)| {
        compare_bytes(left.as_bytes(), right.as_bytes(), progress)
    })?;
    Ok(names)
}

impl<'v> Doc<'v> {
    fn collect<V: DatasetView<Id = TermId>, S: Admission>(
        view: &V,
        source: &BorrowedSource<'_>,
        vocabulary: &'v Vocabulary,
        governor: &Shared<'_>,
        memory: &mut Memory<'_, S>,
        storage_error: &impl Fn(StorageError) -> LoadError,
    ) -> Result<Self, LoadError> {
        let mut native_work = Work::new(governor);
        let mut progress = |units| native_work.charge(units).map_err(stop);
        let names = vocabulary_index(vocabulary, governor, memory, &mut progress, storage_error)?;
        let value = |id, memory: &mut Memory<'_, S>| {
            view.term_value_with_memory(
                id,
                memory,
                |cause| control::read_failure("dataset-read", cause),
                |cause, _| storage_error(cause),
            )
        };
        let mut rows = Vec::new();
        let mut mapping_alias_declared = false;
        // Statement rows, then annotation side-table rows: both describe a subject.
        let statements = view
            .quads()
            .map(|quad| (quad, SourceStatementKind::Quad))
            .chain(
                view.annotation_quads()
                    .map(|quad| (quad, SourceStatementKind::Annotation)),
            );
        for (quad, layer) in statements {
            work(&mut governor.borrow_mut(), 1)?;
            let s = value(quad.s, memory)?;
            let TermValue::Iri(p) = value(quad.p, memory)? else {
                return Err(unsupported(&s, "-", Construct::UnownedStatement, None));
            };
            if quad.g.is_some() {
                let at = control::quad_extent(source, quad, layer, &mut governor.borrow_mut())?;
                return Err(unsupported(&s, &p, Construct::NamedGraph, at));
            }
            let o = value(quad.o, memory)?;
            let row = Row {
                subject_id: quad.s,
                object_id: quad.o,
                layer,
                s,
                p,
                o,
            };
            mapping_alias_declared |= row.declares_mapping_alias();
            memory.push(&mut rows, row).map_err(storage_error)?;
        }
        let mut reifiers = Vec::new();
        for quad in view.reifier_quads() {
            work(&mut governor.borrow_mut(), 1)?;
            let id = value(quad.s, memory)?;
            if quad.g.is_some() {
                // A reifier row in a named graph would need a graph on its hint.
                let at = control::quad_extent(
                    source,
                    quad,
                    SourceStatementKind::Reifier,
                    &mut governor.borrow_mut(),
                )?;
                return Err(unsupported(&id, "-", Construct::NamedGraph, at));
            }
            let Some((s, p, o)) = view
                .with_term(quad.o, |term| match term {
                    TermRef::Triple { s, p, o } => Some((s, p, o)),
                    _ => None,
                })
                .map_err(|cause| control::read_failure("dataset-read", cause))?
            else {
                return Err(unsupported(&id, "-", Construct::Reifier, None));
            };
            let TermValue::Iri(p) = value(p, memory)? else {
                return Err(unsupported(&id, "-", Construct::Reifier, None));
            };
            let target = (s, o);
            let s = value(s, memory)?;
            let o = value(o, memory)?;
            let reifier = Reifier {
                term_id: quad.s,
                target,
                id,
                s,
                p,
                o,
            };
            memory.push(&mut reifiers, reifier).map_err(storage_error)?;
        }
        // A declared named graph with no statements has no quad to refuse above, so
        // read the view's declared graphs and refuse any that remain.
        if let Some(graph) = view.named_graphs().next() {
            work(&mut governor.borrow_mut(), 1)?;
            let at = control::graph_extent(source, graph, &mut governor.borrow_mut())?;
            return Err(unsupported(
                &value(graph, memory)?,
                "-",
                Construct::NamedGraph,
                at,
            ));
        }
        let by_subject = subject_index(&rows, memory, &mut progress, storage_error)?;
        Ok(Self {
            vocabulary,
            namespace: vocabulary.terms().namespace_iri.as_str(),
            names,
            rows,
            by_subject,
            reifiers,
            mapping_alias_declared,
        })
    }

    /// The vocabulary IRI of a required term name.
    fn term(&self, name: &'static str) -> &str {
        self.vocabulary
            .term(name)
            // INVARIANT: `Vocabulary::new` admits only complete term tables.
            .expect("vocabulary binds every TERM_NAMES entry")
    }

    fn find_bound<E>(
        &self,
        mut compare: impl FnMut(&str) -> Result<std::cmp::Ordering, E>,
    ) -> Result<Option<&'static str>, E> {
        use std::cmp::Ordering;

        let mut names = self.names.as_slice();
        while !names.is_empty() {
            let middle = names.len() / 2;
            let (key, name) = names[middle];
            match compare(key)? {
                Ordering::Less => names = &names[middle + 1..],
                Ordering::Equal => return Ok(Some(name)),
                Ordering::Greater => names = &names[..middle],
            }
        }
        Ok(None)
    }

    fn has_namespace<C: Progress + ?Sized>(
        &self,
        iri: &str,
        progress: &mut C,
    ) -> Result<bool, C::Error> {
        let namespace = self.namespace.as_bytes();
        let Some(prefix) = iri.as_bytes().get(..namespace.len()) else {
            return Ok(false);
        };
        compare_bytes(namespace, prefix, progress).map(std::cmp::Ordering::is_eq)
    }

    fn is_vocabulary_row<C: Progress + ?Sized>(
        &self,
        row: &Row,
        progress: &mut C,
    ) -> Result<bool, C::Error> {
        if self.has_namespace(&row.p, progress)? {
            return Ok(true);
        }
        match (row.p.as_str(), iri_of(&row.o)) {
            (RDF_TYPE, Some(iri)) => self.has_namespace(iri, progress),
            _ => Ok(false),
        }
    }

    /// Refuse unbound names throughout each written RDF statement.
    fn scan_vocabulary<V: DatasetView<Id = TermId>>(
        &self,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
    ) -> Result<(), LoadError> {
        let (mut account, rejection) = ControlledStorage::owned(governor);
        let mut memory = Memory::new(&mut account);
        let refusal = |cause| control::storage_stop(governor, &rejection, &cause);
        for row in &self.rows {
            if let Some(term) = self
                .inspect_vocabulary_statement(&row.s, &row.p, &row.o, &mut memory)
                .map_err(refusal)?
            {
                let at = locations.row(row, &mut governor.borrow_mut())?;
                return Err(LoadError::UnknownTerm {
                    subject: show(&row.s),
                    predicate: row.p.clone(),
                    term: term.to_owned(),
                    at,
                });
            }
        }
        for reifier in &self.reifiers {
            let unknown = self
                .inspect_vocabulary_statement(&reifier.s, &reifier.p, &reifier.o, &mut memory)
                .map_err(refusal)?;
            let unknown = match unknown {
                Some(term) => Some(term),
                None => self
                    .unknown_value_iri(&reifier.id, &mut memory)
                    .map_err(refusal)?,
            };
            if let Some(term) = unknown {
                let at = control::default_reifier_extent(
                    locations.view,
                    locations.source,
                    reifier,
                    &mut governor.borrow_mut(),
                )?;
                return Err(LoadError::UnknownTerm {
                    subject: show(&reifier.id),
                    predicate: reifier.p.clone(),
                    term: term.to_owned(),
                    at,
                });
            }
        }
        Ok(())
    }

    fn inspect_vocabulary_statement<'a, S: Admission>(
        &self,
        subject: &'a TermValue,
        predicate: &'a str,
        object: &'a TermValue,
        memory: &mut Memory<'_, S>,
    ) -> Result<Option<&'a str>, StorageError> {
        memory.charge(control::NATIVE_UNITS_PER_WORK)?;
        // Keep the existing direct-term refusals ahead of nested-value checks.
        for iri in [iri_of(subject), Some(predicate), iri_of(object)]
            .into_iter()
            .flatten()
        {
            if self.is_unbound_iri(iri, memory)? {
                return Ok(Some(iri));
            }
        }
        for value in [subject, object] {
            if matches!(value, TermValue::Literal { .. } | TermValue::Triple { .. })
                && let Some(iri) = self.unknown_value_iri(value, memory)?
            {
                return Ok(Some(iri));
            }
        }
        Ok(None)
    }

    fn unknown_value_iri<'a, S: Admission>(
        &self,
        value: &'a TermValue,
        memory: &mut Memory<'_, S>,
    ) -> Result<Option<&'a str>, StorageError> {
        let found = value.visit_terms_with_memory(
            |term, memory| {
                memory.charge(1)?;
                if let TermValue::Iri(iri) | TermValue::Literal { datatype: iri, .. } = term
                    && self.is_unbound_iri(iri, memory)?
                {
                    return Ok(ControlFlow::Break(iri.as_str()));
                }
                Ok(ControlFlow::Continue(()))
            },
            memory,
        )?;
        Ok(found.break_value())
    }

    fn is_unbound_iri<C: Progress + ?Sized>(
        &self,
        iri: &str,
        progress: &mut C,
    ) -> Result<bool, C::Error> {
        // The bare namespace IRI names the vocabulary itself.
        Ok(iri.len() > self.namespace.len()
            && self.has_namespace(iri, progress)?
            && self
                .find_bound(|known| compare_bytes(known.as_bytes(), iri.as_bytes(), progress))?
                .is_none())
    }
}

fn unsupported(
    subject: &TermValue,
    predicate: &str,
    construct: Construct,
    at: Option<SourceExtent>,
) -> LoadError {
    LoadError::UnsupportedConstruct {
        subject: show(subject),
        predicate: predicate.to_owned(),
        construct,
        at,
    }
}

/// The retained native dataset and source used only when attaching a refusal.
struct Locations<'a, V> {
    view: &'a V,
    source: &'a BorrowedSource<'a>,
}

impl<V: DatasetView<Id = TermId>> Locations<'_, V> {
    fn row(&self, row: &Row, governor: &mut Governor) -> Result<Option<SourceExtent>, LoadError> {
        control::classified_row_extent(self.view, self.source, row, governor)
    }

    fn reifier(
        &self,
        reifier: &Reifier,
        governor: &mut Governor,
    ) -> Result<Option<SourceExtent>, LoadError> {
        control::default_reifier_extent(self.view, self.source, reifier, governor)
    }

    fn unsupported_row(
        &self,
        row: &Row,
        construct: Construct,
        governor: &mut Governor,
    ) -> Result<LoadError, LoadError> {
        let at = self.row(row, governor)?;
        Ok(unsupported(&row.s, &row.p, construct, at))
    }

    fn invalid_shape_row(
        &self,
        row: &Row,
        violation: Violation,
        governor: &mut Governor,
    ) -> Result<LoadError, LoadError> {
        let at = self.row(row, governor)?;
        Ok(LoadError::InvalidShape {
            subject: show(&row.s),
            predicate: row.p.clone(),
            at: at.map(Box::new),
            violation,
        })
    }
}

impl Doc<'_> {
    fn owns_mapping_alias_declaration(&self, row: &Row) -> bool {
        self.mapping_alias_declared
            && (row.declares_mapping_alias()
                || (row.layer == SourceStatementKind::Quad
                    && row.p == RDFS_COMMENT
                    && iri_of(&row.s) == Some(FNOM_MAPPING)
                    && matches!(row.o, TermValue::Literal { .. })))
    }

    /// Every row has exactly one owning view; an unowned row is a refusal.
    fn own_every_row<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
    ) -> Result<bool, LoadError> {
        let mut progress = Work::new(governor);
        let mut first_alias = None;
        for row in &self.rows {
            work(&mut governor.borrow_mut(), 1)?;
            if self.owns_mapping_alias_declaration(row) {
                continue;
            }
            if row.uses_mapping_alias() {
                first_alias.get_or_insert(row);
            }
            let vocabulary = self.is_vocabulary_row(row, &mut progress).map_err(stop)?;
            let declaration = row.p == RDF_TYPE
                && matches!(iri_of(&row.o), Some(RDFS_CLASS | OWL_CLASS | RDF_PROPERTY));
            let annotation = matches!(row.p.as_str(), RDFS_LABEL | RDFS_COMMENT);
            let owned = if nodes.owns_fno(row.subject_id) {
                !vocabulary
                    || nodes
                        .role_of(row.subject_id)
                        .is_some_and(|mask| mask & Role::TOOL != 0)
            } else if vocabulary {
                nodes.role_of(row.subject_id).is_some()
            } else if nodes.owns_shape(row.subject_id) {
                row.p.starts_with(SH) || row.p == RDF_TYPE || annotation
            } else if nodes.owns_shape_cell(row.subject_id) {
                matches!(row.p.as_str(), RDF_FIRST | RDF_REST)
            } else if nodes.role_of(row.subject_id).is_some() {
                annotation || row.p == RDF_TYPE
            } else {
                declaration
            };
            if !owned {
                return Err(locations.unsupported_row(
                    row,
                    Construct::UnownedStatement,
                    &mut governor.borrow_mut(),
                )?);
            }
        }
        if let Some(row) = first_alias {
            let detail = if !self.mapping_alias_declared {
                "fnom:Mapping requires its owl:equivalentClass declaration for fno:Mapping"
            } else if !self.vocabulary.admits_fno_mapping_normalization() {
                "fnom:Mapping requires the registered FnO view normalization"
            } else {
                return Ok(true);
            };
            let at = locations.row(row, &mut governor.borrow_mut())?;
            return Err(LoadError::InvalidFnoBinding {
                subject: show(&row.s),
                predicate: Some(RDF_TYPE.to_owned()),
                detail: detail.to_owned(),
                at,
            });
        }
        Ok(false)
    }
}

fn is_literal(value: &TermValue, datatypes: &[&str]) -> bool {
    matches!(
        value,
        TermValue::Literal { datatype, language: None, direction: None, .. }
            if datatypes.contains(&datatype.as_str())
    )
}

const fn lexical_str(value: &TermValue) -> &str {
    match value {
        TermValue::Literal { lexical_form, .. } => lexical_form.as_str(),
        _ => "",
    }
}

const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
const XSD_BOOLEAN: &str = "http://www.w3.org/2001/XMLSchema#boolean";
const XSD_ANY_URI: &str = "http://www.w3.org/2001/XMLSchema#anyURI";
const XSD_INTEGERS: [&str; 3] = [
    "http://www.w3.org/2001/XMLSchema#integer",
    "http://www.w3.org/2001/XMLSchema#positiveInteger",
    "http://www.w3.org/2001/XMLSchema#nonNegativeInteger",
];

const fn wrong(expected: &'static str) -> Violation {
    Violation::WrongKind { expected }
}

const fn range(expected: &'static str) -> Violation {
    Violation::OutOfRange { expected }
}

fn check_integer(kind: Kind, value: &TermValue) -> Result<(), Violation> {
    if !is_literal(value, &XSD_INTEGERS) {
        return Err(wrong("xsd:integer literal"));
    }
    let positive = kind == Kind::Positive;
    match parse_u64(lexical_str(value)) {
        Some(0) | None if positive => Err(range("a positive integer that fits u64")),
        None => Err(range("a non-negative integer that fits u64")),
        Some(_) => Ok(()),
    }
}

/// Check one value against the vocabulary's shape table entry.
fn check_value(kind: Kind, value: &TermValue) -> Result<(), Violation> {
    let text = lexical_str(value);
    match kind {
        Kind::Str | Kind::FilePermissions if is_literal(value, &[XSD_STRING]) => Ok(()),
        Kind::Str | Kind::FilePermissions => Err(wrong("xsd:string literal")),
        Kind::NonEmptyStr => {
            check_value(Kind::Str, value)?;
            if text.is_empty() {
                Err(range("a nonempty string"))
            } else {
                Ok(())
            }
        }
        Kind::Name => {
            check_value(Kind::Str, value)?;
            let valid = !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'));
            valid
                .then_some(())
                .ok_or(range("a nonempty name of [A-Za-z0-9_.:-]"))
        }
        Kind::Bool if is_literal(value, &[XSD_BOOLEAN]) => {
            matches!(text, "true" | "false" | "1" | "0")
                .then_some(())
                .ok_or(range("true or false"))
        }
        Kind::Bool => Err(wrong("xsd:boolean literal")),
        Kind::Positive | Kind::NonNegative => check_integer(kind, value),
        Kind::Node if matches!(value, TermValue::Iri(_) | TermValue::Blank { .. }) => Ok(()),
        Kind::Node => Err(wrong("IRI or blank node")),
        Kind::Iri if matches!(value, TermValue::Iri(_)) => Ok(()),
        Kind::Iri => Err(wrong("IRI")),
        Kind::AnyUri if is_literal(value, &[XSD_ANY_URI]) => Ok(()),
        Kind::AnyUri => Err(wrong("xsd:anyURI literal")),
        Kind::Choice(allowed) => {
            check_value(Kind::Str, value)?;
            allowed
                .contains(&text)
                .then_some(())
                .ok_or(range("one of the vocabulary's listed values"))
        }
    }
}

fn property_violation(
    kind: Kind,
    value: &TermValue,
    governor: &Shared<'_>,
) -> Result<Option<Violation>, LoadError> {
    let structural = check_value(kind, value).err();
    if kind != Kind::FilePermissions || structural.is_some() {
        return Ok(structural);
    }
    match FilePermissions::parse(lexical_str(value), &mut governor.borrow_mut()) {
        Ok(_) => Ok(None),
        Err(FileRuleError::Resource { reason, .. }) => Err(stop(reason)),
        Err(FileRuleError::Permission { .. }) => Ok(Some(range(
            "file permissions drawn from r, w, a, k, l and m",
        ))),
        Err(FileRuleError::DuplicatePermission { .. }) => Ok(Some(Violation::Duplicate)),
        Err(FileRuleError::WriteAppend { .. }) => Ok(Some(range(
            "file permissions without both write and append",
        ))),
        Err(_) => Ok(Some(range("a valid file permission set"))),
    }
}

fn parse_u64(text: &str) -> Option<u64> {
    let digits = text.strip_prefix('+').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn boolean(value: &TermValue) -> bool {
    matches!(value, TermValue::Literal { lexical_form, .. } if matches!(lexical_form.as_str(), "true" | "1"))
}

impl Doc<'_> {
    /// The concrete roles of a node: its recognised bits, with rule markers resolved.
    fn resolve_concrete(
        &self,
        node: &TermValue,
        mask: u16,
        rule_domain: impl FnOnce() -> u16,
    ) -> Result<u16, LoadError> {
        let mut mask = mask & !Role::SHAPE;
        if mask & Role::RULE_ANY != 0 {
            let concrete = mask & Role::RULES;
            let concrete = if concrete == 0 || concrete == Role::RULES {
                // A predicate specific to a rule can supply an omitted class.
                rule_domain()
            } else {
                concrete
            };
            if concrete.count_ones() != 1 {
                return Err(LoadError::InvalidExecutionProfile {
                    subject: show(node),
                    predicate: self.term("rule").to_owned(),
                    at: None,
                    violation: Violation::Detail(
                        "a rule needs exactly one concrete rule class".to_owned(),
                    ),
                });
            }
            mask = (mask & !Role::RULES & !Role::RULE_ANY) | concrete;
        }
        Ok(mask)
    }

    fn wrap(
        role: u16,
        subject: &TermValue,
        predicate: &str,
        violation: Violation,
        at: Option<SourceExtent>,
    ) -> LoadError {
        let (subject, predicate) = (show(subject), predicate.to_owned());
        let at = at.map(Box::new);
        if role & (Role::TOOL | Role::CONTEXT) != 0 {
            LoadError::InvalidShape {
                subject,
                predicate,
                violation,
                at,
            }
        } else if role == Role::HINT {
            LoadError::InvalidHint {
                subject,
                predicate,
                violation,
                at,
            }
        } else {
            LoadError::InvalidExecutionProfile {
                subject,
                predicate,
                violation,
                at,
            }
        }
    }

    fn validate_nodes<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
    ) -> Result<(), LoadError> {
        let mut progress = Work::new(governor);
        for classified in nodes.roles() {
            let node = classified.identity.term;
            let mask = self.resolve_concrete(node, classified.mask, || classified.rule_domain)?;
            for &index in classified.rows {
                let row = &self.rows[index];
                work(&mut governor.borrow_mut(), 1)?;
                if mask == 0 {
                    // An abstract role or bare shape has no property table.
                    if self.is_vocabulary_row(row, &mut progress).map_err(stop)? {
                        return Err(locations.unsupported_row(
                            row,
                            Construct::UnownedStatement,
                            &mut governor.borrow_mut(),
                        )?);
                    }
                } else if let Some(name) = self.find_bound(|key| {
                    compare_bytes(key.as_bytes(), row.p.as_bytes(), &mut progress).map_err(stop)
                })? && Role::domain(name) & mask == 0
                {
                    let at = locations.row(row, &mut governor.borrow_mut())?;
                    return Err(Self::wrap(
                        Role::first(mask),
                        node,
                        &row.p,
                        Violation::Domain {
                            subject_role: Role::label(Role::first(mask)),
                        },
                        at,
                    ));
                }
            }
            for role in Role::bits(mask) {
                self.validate_role(classified, role, locations, governor, &mut progress)?;
            }
        }
        Ok(())
    }

    fn validate_role<V: DatasetView<Id = TermId>>(
        &self,
        node: RoleNode<'_>,
        role: u16,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        let mut socket_type = false;
        let mut protocol = None;
        let mut mount_allow = None;
        for spec in Spec::table(role) {
            work(&mut governor.borrow_mut(), 1)?;
            let predicate = self.term(spec.name);
            let (found, excess) =
                self.count_role_values(node.rows, predicate, spec.max, progress)?;
            if found < usize::from(spec.min) {
                return Err(Self::wrap(
                    role,
                    node.identity.term,
                    predicate,
                    Violation::Missing,
                    None,
                ));
            }
            if let Some(row) = excess {
                let at = locations.row(row, &mut governor.borrow_mut())?;
                return Err(Self::wrap(
                    role,
                    node.identity.term,
                    predicate,
                    Violation::TooMany { found },
                    at,
                ));
            }
            if found == 0 {
                continue;
            }
            for &index in node.rows {
                let row = &self.rows[index];
                if !matches_predicate(row, predicate, progress)? {
                    continue;
                }
                let value = &row.o;
                if matches!(spec.kind, Kind::Name | Kind::Positive | Kind::NonNegative) {
                    progress
                        .charge(saturating_size(lexical_str(value).len()))
                        .map_err(stop)?;
                }
                if let Some(violation) = property_violation(spec.kind, value, governor)? {
                    let at = locations.row(row, &mut governor.borrow_mut())?;
                    return Err(Self::wrap(
                        role,
                        node.identity.term,
                        predicate,
                        violation,
                        at,
                    ));
                }
                match (role, spec.name) {
                    (Role::NETWORK, "socketType") => socket_type = true,
                    (Role::NETWORK, "protocol") => protocol = Some(row),
                    (Role::MOUNT, "deny") if !boolean(value) => mount_allow = Some(row),
                    _ => {}
                }
            }
        }
        if role == Role::NETWORK && socket_type == protocol.is_some() {
            let at = protocol
                .map(|row| locations.row(row, &mut governor.borrow_mut()))
                .transpose()?
                .flatten();
            return Err(Self::wrap(
                role,
                node.identity.term,
                self.term("protocol"),
                Violation::Exclusive,
                at,
            ));
        }
        if let Some(row) = mount_allow {
            let predicate = self.term("deny");
            let at = locations.row(row, &mut governor.borrow_mut())?;
            return Err(Self::wrap(
                role,
                node.identity.term,
                predicate,
                Violation::OutOfRange {
                    expected: "true: a mount rule is always a denial",
                },
                at,
            ));
        }
        Ok(())
    }

    /// Count every match and retain the first row beyond the maximum.
    fn count_role_values(
        &self,
        indices: &[usize],
        predicate: &str,
        maximum: Option<u8>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(usize, Option<&Row>), LoadError> {
        let first_excess = maximum.map(|max| usize::from(max) + 1);
        let mut found = 0;
        let mut excess = None;
        for &index in indices {
            let row = &self.rows[index];
            if matches_predicate(row, predicate, progress)? {
                found += 1;
                if Some(found) == first_excess {
                    excess = Some(row);
                }
            }
        }
        Ok((found, excess))
    }
}

impl Doc<'_> {
    /// Read the strict `FnO` catalog from the rows the `FnO` view owns, under the
    /// governor's storage, work and stop authority.
    fn read_catalog<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        original: &V,
        source: &BorrowedSource<'_>,
        governor: &Shared<'_>,
    ) -> Result<Option<FnoCatalog>, LoadError> {
        if !nodes.has_fno {
            return Ok(None);
        }
        self.with_fno_view(nodes, governor, |view| {
            let (storage, rejection) = Storage::new(governor);
            FnoCatalog::from_dataset_with_control(
                view,
                self.namespace,
                storage,
                Work::new(governor),
            )
            .map(Some)
            .map_err(|failure| match failure {
                ControlledError::Control(reason) => stop(reason),
                ControlledError::Native(purrdf_core::fno::FnoConstructionFailure::Storage(
                    cause,
                )) => control::storage_stop(governor, &rejection, &cause),
                ControlledError::Native(native) => {
                    self.fno_error(&native, original, source, &mut governor.borrow_mut())
                }
            })
        })
    }

    /// Keep the frozen view charged until the reader returns and the view drops.
    fn with_fno_view<T>(
        &self,
        nodes: &Nodes<'_>,
        governor: &Shared<'_>,
        read: impl FnOnce(&RdfDataset) -> Result<T, LoadError>,
    ) -> Result<T, LoadError> {
        let (mut view_account, view_rejection) = ControlledStorage::owned(governor);
        let view = self
            .fno_view(nodes, &mut Memory::new(&mut view_account))
            .map_err(|failure| {
                if let NativeBuildError::Storage(cause) = &failure
                    && let Some(refused) =
                        control::accounted_storage_stop(governor, &view_rejection, cause)
                {
                    return refused;
                }
                LoadError::InvalidFnoBinding {
                    subject: self.namespace.to_owned(),
                    predicate: None,
                    detail: format!("FnO view construction was refused: {failure}"),
                    at: None,
                }
            })?;
        read(&view)
    }

    /// The default-graph rows the `FnO` reader owns, frozen under the temporary account.
    fn fno_view(
        &self,
        nodes: &Nodes<'_>,
        memory: &mut Memory<'_, impl Admission>,
    ) -> Result<RdfDataset, NativeBuildError> {
        let mut builder = RdfDatasetBuilder::new();
        for row in &self.rows {
            memory
                .admission_mut()
                .checkpoint(control::NATIVE_UNITS_PER_WORK)?;
            if !nodes.owns_fno(row.subject_id)
                || self.owns_mapping_alias_declaration(row)
                || self.is_vocabulary_row(row, memory)?
            {
                continue;
            }
            let s = builder.intern_value_with_memory(&row.s, memory)?;
            let p = builder.intern_iri_with_memory(&row.p, memory)?;
            let o = if row.uses_mapping_alias() {
                builder.intern_iri_with_memory(FNO_MAPPING, memory)?
            } else {
                builder.intern_value_with_memory(&row.o, memory)?
            };
            builder.push_quad_with_memory(s, p, o, None, memory)?;
        }
        builder.freeze_with_memory(memory)
    }

    fn fno_error<V: DatasetView<Id = TermId>>(
        &self,
        failure: &FnoReadFailure<Infallible>,
        original: &V,
        source: &BorrowedSource<'_>,
        governor: &mut Governor,
    ) -> LoadError {
        let (subject, predicate, object, alias_only) = match failure {
            purrdf_core::fno::FnoConstructionFailure::Semantic(shared) => match &**shared {
                FnoReadError::Field {
                    subject,
                    predicate,
                    objects,
                    expected: FnoFieldKind::Blank,
                } if self.mapping_alias_declared
                    && predicate == RDF_TYPE
                    && matches!(objects.as_slice(), [value] if iri_of(value) == Some(FNO_MAPPING)) =>
                {
                    (
                        Some(subject),
                        Some(predicate.clone()),
                        objects.first(),
                        true,
                    )
                }
                FnoReadError::Field {
                    subject, predicate, ..
                }
                | FnoReadError::List {
                    subject, predicate, ..
                } => (Some(subject), Some(predicate.clone()), None, false),
                FnoReadError::Returns {
                    subject,
                    predicate,
                    head,
                    ..
                } => (Some(subject), Some(predicate.clone()), Some(head), false),
                FnoReadError::Statement { row, .. } => (
                    Some(&row.s),
                    Some(show(&row.p)),
                    row.g.is_none().then_some(&row.o),
                    false,
                ),
                _ => (None, None, None, false),
            },
            _ => (None, None, None, false),
        };
        let at = match (subject, predicate.as_deref(), object) {
            (Some(subject), Some(predicate), Some(object)) => {
                let alias = (self.mapping_alias_declared
                    && predicate == RDF_TYPE
                    && iri_of(object) == Some(FNO_MAPPING))
                .then_some(FNOM_MAPPING);
                match control::default_row_extent_or_iri(
                    original,
                    source,
                    subject,
                    predicate,
                    (!alias_only).then_some(object),
                    alias,
                    governor,
                ) {
                    Ok(at) => at,
                    Err(error) => return error,
                }
            }
            _ => None,
        };
        LoadError::InvalidFnoBinding {
            subject: subject.map_or_else(|| self.namespace.to_owned(), show),
            predicate,
            detail: failure.to_string(),
            at,
        }
    }
}

struct ToolLookup<'i, 'n, 'd> {
    mappings_by_function: &'i [usize],
    profiles: Profiles<'n, 'd>,
}

fn index_mappings(
    catalog: &FnoCatalog,
    storage: &mut BuildStorage<'_, '_, '_>,
    progress: &mut Work<'_, '_>,
) -> Result<Vec<usize>, LoadError> {
    let mut indices = Vec::new();
    let mappings = &catalog.as_input().mappings;
    for index in 0..mappings.len() {
        storage.charge(1)?;
        storage.push(&mut indices, index)?;
    }
    try_sort_unstable_by(&mut indices, |&left, &right| {
        compare_bytes(
            mappings[left].function.as_bytes(),
            mappings[right].function.as_bytes(),
            progress,
        )
        // Equal functions retain native catalog order for duplicate diagnostics.
        .map(|order| order.then(left.cmp(&right)))
        .map_err(stop)
    })?;
    Ok(indices)
}

impl Doc<'_> {
    fn build_tools<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        catalog: Option<&FnoCatalog>,
        used_alias: bool,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
    ) -> Result<RetainedTools, LoadError> {
        let (scratch, rejection) = Storage::new(governor);
        let mut scratch = OwnedAdmission::new(scratch);
        let mut scratch = BuildStorage::new(&mut scratch, governor, rejection);
        let mut progress = Work::new(governor);
        let mut iris = Vec::new();
        for role in nodes.roles() {
            let node = role.identity;
            scratch.charge(1)?;
            if role.mask & Role::TOOL != 0
                && let Some(iri) = iri_of(node.term)
            {
                scratch.push(&mut iris, iri)?;
            }
        }
        try_sort_unstable_by(&mut iris, |left, right| {
            compare_bytes(left.as_bytes(), right.as_bytes(), &mut progress).map_err(stop)
        })?;
        let mappings_by_function = if let Some(catalog) = catalog {
            self.check_catalog_tools(catalog, &iris, locations, governor, &mut progress)?;
            index_mappings(catalog, &mut scratch, &mut progress)?
        } else {
            Vec::new()
        };
        let lookup = ToolLookup {
            mappings_by_function: &mappings_by_function,
            profiles: Profiles { nodes },
        };
        let mut hints = HintBindings::new(&self.reifiers, &mut scratch)?;
        let (storage, rejection) = Storage::new(governor);
        let mut retained = RetainedTools {
            tools: Vec::new(),
            masks: Vec::new(),
            fno_view_losses: LossLedger::new(),
            storage: OwnedAdmission::new(storage),
        };
        let mut storage = BuildStorage::new(&mut retained.storage, governor, rejection);
        let mut seen_names = OrderedTable::default();
        for role in nodes.roles() {
            let node = role.identity;
            storage.charge(1)?;
            if role.mask & Role::TOOL == 0 {
                continue;
            }
            let catalog = match catalog {
                Some(catalog)
                    if self.has_type_id(nodes, node.id, FNO_FUNCTION, &mut progress)? =>
                {
                    catalog
                }
                _ => {
                    return Err(self.missing_function(
                        nodes,
                        node,
                        locations,
                        governor,
                        &mut progress,
                    )?);
                }
            };
            let candidate =
                self.build_tool(node, catalog, &lookup, &mut hints, locations, &mut storage)?;
            let name = self.required_property(nodes, node, "name", &mut progress)?;
            if scratch.record_name(&mut seen_names, lexical_str(&name.o), &mut progress)? {
                let at = locations.row(name, &mut governor.borrow_mut())?;
                return Err(LoadError::InvalidShape {
                    subject: show(node.term),
                    predicate: name.p.clone(),
                    at: at.map(Box::new),
                    violation: Violation::Duplicate,
                });
            }
            storage.reserve_next(&mut retained.tools)?;
            storage.reserve_next(&mut retained.masks)?;
            let (tool, mask) = candidate;
            retained.tools.push(tool);
            retained.masks.push(mask);
        }
        self.check_reifiers(nodes, &hints.consumed, locations, governor, &mut progress)?;
        if used_alias {
            self.record_mapping_normalizations(
                &mut retained.fno_view_losses,
                &mut storage,
                &mut progress,
            )?;
        }
        // The classified node iterator already visits the IRI subjects in lexical order.
        Ok(retained)
    }

    fn record_mapping_normalizations(
        &self,
        ledger: &mut LossLedger,
        storage: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        for row in &self.rows {
            storage.charge(1)?;
            if row.uses_mapping_alias() {
                let definition = FNOM_MAPPING_NORMALIZATION;
                let subject = storage.loss_term(&row.s)?;
                storage.record_loss(
                    ledger,
                    LossEntry {
                        code: definition.code,
                        from: definition.from,
                        to: definition.to,
                        note: definition.description,
                        site: LossSite::Unspecified,
                        subject: LossSubject::Absent,
                        detail: Vec::new(),
                    },
                    subject,
                    progress,
                )?;
            }
        }
        Ok(())
    }

    fn check_catalog_tools<V: DatasetView<Id = TermId>>(
        &self,
        catalog: &FnoCatalog,
        iris: &[&str],
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        for function in &catalog.as_input().functions {
            work(&mut governor.borrow_mut(), 1)?;
            let matches = try_equal_range_by(iris, |key| {
                compare_bytes(key.as_bytes(), function.iri.as_bytes(), progress).map_err(stop)
            })?;
            if !matches.is_empty() {
                continue;
            }
            let mut at = None;
            for row in &self.rows {
                if matches_predicate(row, RDF_TYPE, progress)?
                    && iri_of(&row.o) == Some(FNO_FUNCTION)
                    && let Some(subject) = iri_of(&row.s)
                    && compare_bytes(subject.as_bytes(), function.iri.as_bytes(), progress)
                        .map_err(stop)?
                        .is_eq()
                {
                    at = locations.row(row, &mut governor.borrow_mut())?;
                    break;
                }
            }
            return Err(LoadError::UnsupportedConstruct {
                subject: function.iri.clone(),
                predicate: RDF_TYPE.to_owned(),
                construct: Construct::FunctionWithoutTool,
                at,
            });
        }
        Ok(())
    }

    fn check_reifiers<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        consumed: &[bool],
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        for (used, reifier) in consumed.iter().copied().zip(&self.reifiers) {
            work(&mut governor.borrow_mut(), 1)?;
            if used {
                continue;
            }
            let at = locations.reifier(reifier, &mut governor.borrow_mut())?;
            return Err(
                if self.has_type_id(nodes, reifier.term_id, self.term("Hint"), progress)? {
                    LoadError::InvalidHint {
                        subject: show(&reifier.id),
                        predicate: reifier.p.clone(),
                        at: at.map(Box::new),
                        violation: Violation::Unresolved,
                    }
                } else {
                    unsupported(&reifier.id, &reifier.p, Construct::Reifier, at)
                },
            );
        }
        Ok(())
    }

    fn missing_function<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        node: Node<'_>,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<LoadError, LoadError> {
        let at = self
            .type_row(nodes, node.id, self.term("Tool"), progress)?
            .map(|row| locations.row(row, &mut governor.borrow_mut()))
            .transpose()?
            .flatten();
        Ok(LoadError::InvalidFnoBinding {
            subject: show(node.term),
            predicate: Some(RDF_TYPE.to_owned()),
            detail: "a tool must be typed as an fno:Function".to_owned(),
            at,
        })
    }

    fn first_property<'d>(
        &'d self,
        nodes: &Nodes<'_>,
        id: TermId,
        predicate: &str,
        progress: &mut Work<'_, '_>,
    ) -> Result<Option<&'d Row>, LoadError> {
        for &index in nodes.rows(id) {
            let row = &self.rows[index];
            if matches_predicate(row, predicate, progress)? {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    fn has_type_id(
        &self,
        nodes: &Nodes<'_>,
        id: TermId,
        class: &str,
        progress: &mut Work<'_, '_>,
    ) -> Result<bool, LoadError> {
        Ok(self.type_row(nodes, id, class, progress)?.is_some())
    }

    fn type_row<'d>(
        &'d self,
        nodes: &Nodes<'_>,
        id: TermId,
        class: &str,
        progress: &mut Work<'_, '_>,
    ) -> Result<Option<&'d Row>, LoadError> {
        for &index in nodes.rows(id) {
            let row = &self.rows[index];
            if matches_predicate(row, RDF_TYPE, progress)?
                && let Some(iri) = iri_of(&row.o)
                && compare_bytes(iri.as_bytes(), class.as_bytes(), progress)
                    .map_err(stop)?
                    .is_eq()
            {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    fn typed_value<'d>(
        &'d self,
        nodes: &Nodes<'_>,
        id: TermId,
        name: &'static str,
        progress: &mut Work<'_, '_>,
    ) -> Result<Option<&'d TermValue>, LoadError> {
        Ok(self
            .first_property(nodes, id, self.term(name), progress)?
            .map(|row| &row.o))
    }

    fn typed_number(
        &self,
        nodes: &Nodes<'_>,
        id: TermId,
        name: &'static str,
        progress: &mut Work<'_, '_>,
    ) -> Result<u64, LoadError> {
        let value = self
            .typed_value(nodes, id, name, progress)?
            .map_or("", lexical_str);
        progress
            .charge(purrassist_core::contract::saturating_size(value.len()))
            .map_err(stop)?;
        // INVARIANT: vocabulary validation admitted this unsigned integer.
        Ok(parse_u64(value).expect("validated integer"))
    }

    fn build_tool<V: DatasetView<Id = TermId>>(
        &self,
        node: Node<'_>,
        catalog: &FnoCatalog,
        lookup: &ToolLookup<'_, '_, '_>,
        hints: &mut HintBindings,
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<(Tool, ScratchLease), LoadError> {
        let mut progress = Work::new(storage.governor);
        let nodes = lookup.profiles.nodes;
        let TermValue::Iri(iri) = node.term else {
            return Err(LoadError::InvalidShape {
                subject: show(node.term),
                predicate: self.term("name").to_owned(),
                at: None,
                violation: Violation::WrongKind {
                    expected: "an IRI subject",
                },
            });
        };
        let subtools = self.retained_texts(nodes, node.id, "subtool", storage)?;
        let (parameters, output, implementation) =
            self.build_callable(node, lookup, catalog, hints, locations, storage)?;
        let flag = |name, progress: &mut Work<'_, '_>| {
            Ok::<_, LoadError>(
                self.typed_value(nodes, node.id, name, progress)?
                    .map(boolean),
            )
        };
        let annotations = ToolAnnotations {
            title: self
                .typed_value(nodes, node.id, "title", &mut progress)?
                .map(|term| storage.lexical(term))
                .transpose()?,
            read_only_hint: flag("readOnlyHint", &mut progress)?,
            destructive_hint: flag("destructiveHint", &mut progress)?,
            idempotent_hint: flag("idempotentHint", &mut progress)?,
            open_world_hint: flag("openWorldHint", &mut progress)?,
        };
        let profile = self.required_property(nodes, node, "executionProfile", &mut progress)?;
        let execution_profile = lookup.profiles.build(self, profile, storage)?;
        let context_node = Node::object(self.required_property(
            nodes,
            node,
            "contextManagement",
            &mut progress,
        )?);
        let context = self.build_context(nodes, context_node, locations, storage)?;
        let iri = storage.string(iri)?;
        let name = self.retained_text(nodes, node.id, "name", storage, &mut progress)?;
        let description =
            self.retained_text(nodes, node.id, "description", storage, &mut progress)?;
        let input_shape_row = self.required_property(nodes, node, "inputShape", &mut progress)?;
        let input_shape = storage.lexical(&input_shape_row.o)?;
        let input_shape_term = storage.loss_term(&input_shape_row.o)?;
        let output_shape_row = self.required_property(nodes, node, "outputShape", &mut progress)?;
        let output_shape = storage.lexical(&output_shape_row.o)?;
        let output_shape_term = storage.loss_term(&output_shape_row.o)?;
        let parameter_constraints = self.retained_text(
            nodes,
            node.id,
            "parameterConstraints",
            storage,
            &mut progress,
        )?;
        let (context, mask_lease) = context;
        Ok((
            Tool {
                iri,
                name,
                description,
                annotations,
                input_shape,
                input_shape_term,
                output_shape,
                output_shape_term,
                implementation,
                parameters,
                output,
                execution_profile,
                context,
                parameter_constraints,
                subtools,
            },
            mask_lease,
        ))
    }

    fn required_property<'d>(
        &'d self,
        nodes: &Nodes<'_>,
        node: Node<'_>,
        name: &'static str,
        progress: &mut Work<'_, '_>,
    ) -> Result<&'d Row, LoadError> {
        let predicate = self.term(name);
        self.first_property(nodes, node.id, predicate, progress)?
            .ok_or_else(|| LoadError::InvalidShape {
                subject: show(node.term),
                predicate: predicate.to_owned(),
                at: None,
                violation: Violation::Missing,
            })
    }

    fn retained_text(
        &self,
        nodes: &Nodes<'_>,
        id: TermId,
        name: &'static str,
        storage: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<String, LoadError> {
        self.typed_value(nodes, id, name, progress)?
            .map(|term| storage.lexical(term))
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn retained_texts(
        &self,
        nodes: &Nodes<'_>,
        id: TermId,
        name: &'static str,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<Vec<String>, LoadError> {
        let mut values = Vec::new();
        let predicate = self.term(name);
        let mut progress = Work::new(storage.governor);
        for &index in nodes.rows(id) {
            let row = &self.rows[index];
            if matches_predicate(row, predicate, &mut progress)? {
                let value = storage.lexical(&row.o)?;
                storage.push(&mut values, value)?;
            }
        }
        try_sort_unstable_by(&mut values, |left, right| {
            compare_bytes(left.as_bytes(), right.as_bytes(), &mut progress).map_err(stop)
        })?;
        Ok(values)
    }
}

struct ShapePath<'a> {
    iri: &'a str,
    path_id: TermId,
    reference: usize,
    used: bool,
}

impl Doc<'_> {
    fn check_shape_complete<V: DatasetView<Id = TermId>>(
        &self,
        paths: &[ShapePath<'_>],
        locations: &Locations<'_, V>,
        storage: &BuildStorage<'_, '_, '_>,
    ) -> Result<(), LoadError> {
        for path in paths {
            storage.charge(1)?;
            if !path.used {
                return Err(locations.invalid_shape_row(
                    &self.rows[path.reference],
                    Violation::Unresolved,
                    &mut storage.governor.borrow_mut(),
                )?);
            }
        }
        Ok(())
    }

    fn build_callable<V: DatasetView<Id = TermId>>(
        &self,
        node: Node<'_>,
        lookup: &ToolLookup<'_, '_, '_>,
        catalog: &FnoCatalog,
        hints: &mut HintBindings,
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<(Vec<ToolParameter>, ToolOutput, ToolImplementation), LoadError> {
        // INVARIANT: build_tool admitted an IRI before inspecting its binding.
        let iri = iri_of(node.term).expect("tool subject is an IRI");
        let nodes = lookup.profiles.nodes;
        let binding = self.admit_callable(
            catalog,
            lookup.mappings_by_function,
            iri,
            locations,
            storage,
        )?;
        let mut progress = Work::new(storage.governor);
        let input = self.required_property(nodes, node, "inputShape", &mut progress)?;
        let output = self.required_property(nodes, node, "outputShape", &mut progress)?;
        let input_shape = Node::object(input);
        let output_shape = Node::object(output);
        let (scratch, rejection) = Storage::new(storage.governor);
        let mut scratch = OwnedAdmission::new(scratch);
        let mut scratch = BuildStorage::new(&mut scratch, storage.governor, rejection);
        let mut input_paths =
            self.shape_paths(nodes, input, locations, &mut scratch, &mut progress)?;
        let mut output_paths =
            self.shape_paths(nodes, output, locations, &mut scratch, &mut progress)?;
        let mut parameters = Vec::new();
        for parameter in binding.parameters() {
            storage.charge(1)?;
            // INVARIANT: `CallableBinding::admit` refuses a parameter without a predicate.
            let predicate = parameter
                .parameter
                .predicate
                .as_deref()
                .expect("admitted predicate");
            let (shape, path_id) = self.path_shape(
                input_shape,
                &mut input_paths,
                predicate,
                locations,
                storage.governor,
                &mut progress,
            )?;
            let hint = self.build_hint(nodes, (shape.id, path_id), hints, locations, storage)?;
            let parameter = ToolParameter {
                iri: storage.string(&parameter.parameter.iri)?,
                predicate: storage.string(predicate)?,
                datatype: storage.string(&parameter.parameter.r#type)?,
                required: parameter.parameter.required,
                implementation_property: storage
                    .string(&parameter.mapping.implementation_property)?,
                hint,
            };
            storage.push(&mut parameters, parameter)?;
        }
        self.check_shape_complete(&input_paths, locations, storage)?;
        let returned = binding.output();
        // INVARIANT: `CallableBinding::admit` refuses an output without a predicate.
        let output_predicate = returned
            .output
            .predicate
            .as_deref()
            .expect("admitted predicate");
        self.path_shape(
            output_shape,
            &mut output_paths,
            output_predicate,
            locations,
            storage.governor,
            &mut progress,
        )?;
        self.check_shape_complete(&output_paths, locations, storage)?;
        let implementation = binding.implementation();
        Ok((
            parameters,
            ToolOutput {
                iri: storage.string(&returned.output.iri)?,
                predicate: storage.string(output_predicate)?,
                datatype: storage.string(&returned.output.r#type)?,
                implementation_property: storage
                    .string(&returned.mapping.implementation_property)?,
            },
            ToolImplementation {
                iri: storage.string(&implementation.iri)?,
                format: storage.string(&implementation.format)?,
                see_also: storage.string(&implementation.see_also)?,
            },
        ))
    }

    fn admit_callable<'a, V: DatasetView<Id = TermId>>(
        &self,
        catalog: &'a FnoCatalog,
        mapping_indices: &[usize],
        iri: &str,
        locations: &Locations<'_, V>,
        storage: &BuildStorage<'_, '_, '_>,
    ) -> Result<CallableBinding<'a>, LoadError> {
        let fno_error = |detail: String| LoadError::InvalidFnoBinding {
            subject: iri.to_owned(),
            predicate: None,
            detail,
            at: None,
        };
        let mut progress = Work::new(storage.governor);
        let mappings = &catalog.as_input().mappings;
        let range = try_equal_range_by(mapping_indices, |&index| {
            compare_bytes(
                mappings[index].function.as_bytes(),
                iri.as_bytes(),
                &mut progress,
            )
            .map_err(stop)
        })?;
        let candidates = &mapping_indices[range];
        let [mapping] = candidates else {
            let detail = format!(
                "a tool needs exactly one fno:Mapping, found {}",
                candidates.len(),
            );
            let Some(&duplicate) = candidates.get(1) else {
                return Err(fno_error(detail));
            };
            let at = self.mapping_function_extent(
                &mappings[duplicate].node,
                iri,
                locations,
                storage.governor,
                &mut progress,
            )?;
            return Err(LoadError::InvalidFnoBinding {
                subject: iri.to_owned(),
                predicate: Some(FNO_MAPPED_FUNCTION.to_owned()),
                detail,
                at,
            });
        };
        CallableBinding::admit(catalog, *mapping, &mut storage.governor.borrow_mut()).map_err(
            |error| match error {
                crate::BindingError::Resource(reason) => stop(reason),
                other => fno_error(other.to_string()),
            },
        )
    }

    fn mapping_function_extent<V: DatasetView<Id = TermId>>(
        &self,
        mapping: &FnBlank,
        function: &str,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<Option<SourceExtent>, LoadError> {
        for row in &self.rows {
            if matches_predicate(row, FNO_MAPPED_FUNCTION, progress)?
                && let Some((label, scope)) = row.s.as_blank()
                && scope == mapping.scope
                && let Some(object) = iri_of(&row.o)
                && compare_bytes(label.as_bytes(), mapping.label.as_bytes(), progress)
                    .map_err(stop)?
                    .is_eq()
                && compare_bytes(object.as_bytes(), function.as_bytes(), progress)
                    .map_err(stop)?
                    .is_eq()
            {
                return locations.row(row, &mut governor.borrow_mut());
            }
        }
        Ok(None)
    }

    /// Index simple paths in lexical order, preserving the graph nodes as borrows.
    fn shape_paths<'d, V: DatasetView<Id = TermId>>(
        &'d self,
        nodes: &Nodes<'_>,
        reference: &Row,
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<Vec<ShapePath<'d>>, LoadError> {
        let shape = Node::object(reference);
        if !self.has_type_id(nodes, shape.id, SH_NODE_SHAPE, progress)? {
            let at = locations.row(reference, &mut storage.governor.borrow_mut())?;
            return Err(LoadError::InvalidShape {
                subject: show(&reference.s),
                predicate: reference.p.clone(),
                at: at.map(Box::new),
                violation: Violation::Unresolved,
            });
        }
        let mut paths = Vec::new();
        for &index in nodes.rows(shape.id) {
            let row = &self.rows[index];
            if !matches_predicate(row, SH_PROPERTY, progress)? {
                continue;
            }
            let property = Node::object(row);
            let mut path = None;
            let mut found = 0;
            for &index in nodes.rows(property.id) {
                let candidate = &self.rows[index];
                if matches_predicate(candidate, SH_PATH, progress)? {
                    found += 1;
                    // Keep the sole value or the first excess value as the count grows.
                    if found <= 2 {
                        path = Some(candidate);
                    }
                }
            }
            let Some(path) = path else {
                let at = locations.row(row, &mut storage.governor.borrow_mut())?;
                return Err(LoadError::InvalidShape {
                    subject: show(property.term),
                    predicate: SH_PATH.to_owned(),
                    at: at.map(Box::new),
                    violation: Violation::Missing,
                });
            };
            if found > 1 {
                return Err(locations.invalid_shape_row(
                    path,
                    Violation::TooMany { found },
                    &mut storage.governor.borrow_mut(),
                )?);
            }
            let Some(iri) = iri_of(&path.o) else {
                let at = locations.row(path, &mut storage.governor.borrow_mut())?;
                return Err(unsupported(
                    property.term,
                    SH_PATH,
                    Construct::ComplexPath,
                    at,
                ));
            };
            storage.push(
                &mut paths,
                ShapePath {
                    iri,
                    path_id: path.object_id,
                    reference: index,
                    used: false,
                },
            )?;
        }
        try_sort_unstable_by(&mut paths, |left, right| {
            let order =
                compare_bytes(left.iri.as_bytes(), right.iri.as_bytes(), progress).map_err(stop)?;
            Ok::<_, LoadError>(order.then_with(|| left.reference.cmp(&right.reference)))
        })?;
        Ok(paths)
    }

    fn path_shape<'d, V: DatasetView<Id = TermId>>(
        &'d self,
        shape: Node<'_>,
        paths: &mut [ShapePath<'_>],
        predicate: &str,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(Node<'d>, TermId), LoadError> {
        let range = try_equal_range_by(paths, |path| {
            compare_bytes(path.iri.as_bytes(), predicate.as_bytes(), progress).map_err(stop)
        })?;
        match &mut paths[range] {
            [only] => {
                only.used = true;
                Ok((Node::object(&self.rows[only.reference]), only.path_id))
            }
            [_, duplicate, ..] => Err(locations.invalid_shape_row(
                &self.rows[duplicate.reference],
                Violation::Duplicate,
                &mut governor.borrow_mut(),
            )?),
            [] => Err(LoadError::InvalidShape {
                subject: show(shape.term),
                predicate: SH_PROPERTY.to_owned(),
                at: None,
                violation: Violation::Unresolved,
            }),
        }
    }

    fn build_hint<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        path: (TermId, TermId),
        hints: &mut HintBindings,
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<Option<Hint>, LoadError> {
        let mut progress = Work::new(storage.governor);
        // The shape row and reifier components share the same native term table.
        let range = try_equal_range_by(&hints.by_path, |&index| {
            storage.charge(1)?;
            Ok::<_, LoadError>(self.reifiers[index].target.cmp(&path))
        })?;
        let at = match hints.by_path[range] {
            [] => return Ok(None),
            [only] => only,
            [_, duplicate, ..] => {
                let reifier = &self.reifiers[duplicate];
                let at = locations.reifier(reifier, &mut storage.governor.borrow_mut())?;
                return Err(LoadError::InvalidHint {
                    subject: show(&reifier.id),
                    predicate: SH_PATH.to_owned(),
                    at: at.map(Box::new),
                    violation: Violation::Duplicate,
                });
            }
        };
        hints.consumed[at] = true;
        let reifier = &self.reifiers[at];
        let id = &reifier.id;
        let mut none = None;
        let mut integer_to_string = false;
        let mut string_to_integer = false;
        let coercion_predicate = self.term("typeCoercion");
        for &index in nodes.rows(reifier.term_id) {
            let row = &self.rows[index];
            if !matches_predicate(row, coercion_predicate, &mut progress)? {
                continue;
            }
            match lexical_str(&row.o) {
                "none" => none = Some(row),
                "integer-to-string" => integer_to_string = true,
                "string-to-integer" => string_to_integer = true,
                _ => unreachable!("validated type coercion"),
            }
        }
        if let Some(row) = none
            && (integer_to_string || string_to_integer)
        {
            let at = locations.row(row, &mut storage.governor.borrow_mut())?;
            return Err(LoadError::InvalidHint {
                subject: show(id),
                predicate: coercion_predicate.to_owned(),
                at: at.map(Box::new),
                violation: Violation::OutOfRange {
                    expected: "none alone, or conversions without none",
                },
            });
        }
        let mut coercions = Vec::new();
        for (enabled, coercion) in [
            (integer_to_string, Coercion::IntegerToString),
            (string_to_integer, Coercion::StringToInteger),
        ] {
            if enabled {
                storage.push(&mut coercions, coercion)?;
            }
        }
        Ok(Some(Hint {
            reifier: Self::scoped_reference(id, storage)?,
            reifier_term: storage.loss_term(id)?,
            shape: storage.lexical(&reifier.s)?,
            max_edit_distance: self.typed_number(
                nodes,
                reifier.term_id,
                "maxEditDistance",
                &mut progress,
            )?,
            aliases: self.retained_texts(nodes, reifier.term_id, "alias", storage)?,
            coercions,
            case_fold: self
                .typed_value(nodes, reifier.term_id, "caseFold", &mut progress)?
                .is_some_and(boolean),
            unicode_fold: self
                .typed_value(nodes, reifier.term_id, "unicodeFold", &mut progress)?
                .is_some_and(boolean),
            taints: self.retained_texts(nodes, reifier.term_id, "taint", storage)?,
            session_binding: self
                .typed_value(nodes, reifier.term_id, "sessionBinding", &mut progress)?
                .map(|value| storage.lexical(value))
                .transpose()?,
        }))
    }

    /// Use the same scoped blank label as the retained Turtle graph.
    fn scoped_reference(
        id: &TermValue,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<String, LoadError> {
        let Some((label, scope)) = id.as_blank() else {
            return storage.lexical(id);
        };
        let shared = storage.governor;
        let (mut account, rejection) = ControlledStorage::owned(shared);
        let mut memory = Memory::new(&mut account);
        let storage_error = |cause| control::storage_stop(shared, &rejection, &cause);
        let label = encode_blank_label_with_memory(
            label,
            scope,
            LabelAlphabet::BlankNodeLabel,
            &mut memory,
        )
        .map_err(storage_error)?;
        storage.blank_label(&label)
    }
}

impl Doc<'_> {
    fn build_context<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        node: Node<'_>,
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<(ContextManagement, ScratchLease), LoadError> {
        let predicate = self.term("outputProjectionJq");
        let mut progress = Work::new(storage.governor);
        let row = self.first_property(nodes, node.id, predicate, &mut progress)?;
        let projection = row.map_or("", |row| lexical_str(&row.o));
        let mask = FieldMask::parse_with_lease(projection, &mut storage.governor.borrow_mut());
        let mask = match mask {
            Ok(mask) => mask,
            Err(purrassist_core::output::FieldMaskError::Resource(reason)) => {
                return Err(stop(reason));
            }
            Err(error) => {
                let at = match row {
                    Some(row) => locations
                        .row(row, &mut storage.governor.borrow_mut())?
                        .map(Box::new),
                    None => None,
                };
                return Err(LoadError::UnsupportedProjection {
                    subject: show(node.term),
                    blank_scope: node.term.as_blank().map(|(_, scope)| scope),
                    predicate: predicate.to_owned(),
                    projection: projection.to_owned(),
                    error,
                    at,
                });
            }
        };
        let limit = OutputByteLimit::new(self.typed_number(
            nodes,
            node.id,
            "maxOutputBytes",
            &mut progress,
        )?)
        // INVARIANT: `validate_nodes` admitted a positive value.
        .expect("validated positive byte limit");
        let loading = if self
            .typed_value(nodes, node.id, "loadingStrategy", &mut progress)?
            .map(lexical_str)
            == Some("always")
        {
            LoadingStrategy::Always
        } else {
            LoadingStrategy::OnDemand
        };
        let tags = self.retained_texts(nodes, node.id, "intentTags", storage)?;
        let turns = self.typed_number(nodes, node.id, "historyRetentionTurns", &mut progress)?;
        let (mask, lease) = mask;
        Ok((
            ContextManagement::new(mask, limit, turns, tags, loading),
            lease,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PrefixTable, TERM_NAMES, TermBindings, register_loss_profile};
    use purrassist_core::Budget;
    use purrdf_core::loss::LossProfile;
    use purrdf_rdf::NativeRdfFormat;

    const NAMESPACE: &str = "urn:toolgraph:";

    fn fixture_vocabulary() -> Vocabulary {
        Vocabulary::new(
            "pa",
            PrefixTable(vec![("pa".into(), NAMESPACE.into())]),
            TermBindings {
                namespace_iri: NAMESPACE.into(),
                local_names: TERM_NAMES
                    .iter()
                    .map(|name| ((*name).into(), format!("{NAMESPACE}{name}")))
                    .collect(),
            },
            register_loss_profile(LossProfile::builtin()).unwrap(),
        )
        .unwrap()
    }

    const CATALOG_SOURCE: &[u8] = br#"
            @prefix owl: <http://www.w3.org/2002/07/owl#> .
            @prefix dcterms: <http://purl.org/dc/terms/> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            <urn:catalog> a owl:Ontology ;
              dcterms:isPartOf <urn:toolgraph:> ;
              rdfs:label "Loader test catalog" ;
              rdfs:comment "A catalog without functions." .
        "#;

    #[test]
    fn classification_refunds_its_scratch_storage_on_refusal() {
        let vocabulary = fixture_vocabulary();
        let mut fixture = Governor::new(Budget::default());
        let source_owner = RefCell::new(&mut fixture);
        let parsed = control::parse(
            b"<urn:context> a <urn:toolgraph:Context> . <urn:hint> a <urn:toolgraph:Hint> .",
            NativeRdfFormat::Turtle,
            None,
            &source_owner,
        )
        .unwrap();
        let view = parsed.dataset().unwrap();
        let source = parsed.source();
        let locations = Locations {
            view: &view,
            source: &source,
        };
        with_doc(&view, &source, &vocabulary, &source_owner, |doc| {
            let mut ample = Governor::new(Budget::default());
            let shared = RefCell::new(&mut ample);
            let meter = shared.borrow().scratch_meter();
            let baseline = 64;
            meter.charge(baseline).unwrap();
            doc.with_classification(&locations, &shared, |nodes| {
                assert_eq!(nodes.roles().count(), 2);
                assert!(meter.live() > baseline);
                Ok(())
            })?;
            let peak = shared.borrow().usage().peak_scratch_bytes;
            assert_eq!(meter.live(), baseline);

            let mut tight = Governor::new(Budget {
                scratch_bytes: peak - 1,
                ..Budget::default()
            });
            let shared = RefCell::new(&mut tight);
            let meter = shared.borrow().scratch_meter();
            meter.charge(baseline).unwrap();
            let refused =
                doc.with_classification(&locations, &shared, |_| -> Result<(), LoadError> {
                    panic!("a refused classification must not reach its consumer");
                });
            assert!(
                matches!(
                    refused,
                    Err(LoadError::Exhausted(StopReason::BudgetExceeded(
                        Resource::ScratchBytes
                    )))
                ),
                "{refused:?}"
            );
            assert_eq!(meter.live(), baseline);
            Ok(())
        })
        .unwrap();
    }

    /// The vocabulary scan charges the lookup work after source rows are admitted.
    #[test]
    fn vocabulary_prefix_and_binding_lookup_obey_the_work_limit() {
        let vocabulary = fixture_vocabulary();
        let mut fixture = Governor::new(Budget::default());
        let source_owner = RefCell::new(&mut fixture);
        let parsed = control::parse(
            br#"<urn:subject> <urn:toolgraph:profileName> "example" ."#,
            NativeRdfFormat::Turtle,
            None,
            &source_owner,
        )
        .unwrap();
        let view = parsed.dataset().unwrap();
        let source = parsed.source();
        let locations = Locations {
            view: &view,
            source: &source,
        };
        with_doc(&view, &source, &vocabulary, &source_owner, |doc| {
            // One unit covers visiting the row. It cannot cover a matching
            // namespace and the comparisons against the vocabulary index.
            let mut tight = Governor::new(Budget {
                work: 1,
                ..Budget::default()
            });
            assert_eq!(
                doc.scan_vocabulary(&locations, &RefCell::new(&mut tight)),
                Err(LoadError::Exhausted(StopReason::BudgetExceeded(
                    Resource::Work
                )))
            );
            let mut ample = Governor::new(Budget::default());
            doc.scan_vocabulary(&locations, &RefCell::new(&mut ample))
        })
        .unwrap();
    }

    /// The vocabulary validator must refuse an exhausted scan even after the
    /// parser and classifier have admitted a complete context.
    #[test]
    fn vocabulary_property_scans_obey_the_work_limit() {
        let vocabulary = fixture_vocabulary();
        let mut fixture = Governor::new(Budget::default());
        let source_owner = RefCell::new(&mut fixture);
        let parsed = control::parse(
            br#"@prefix pa: <urn:toolgraph:> .
                <urn:context> a pa:Context ;
                    pa:outputProjectionJq ".result" ;
                    pa:maxOutputBytes 256 ;
                    pa:historyRetentionTurns 1 ;
                    pa:intentTags "a", "b", "c" ;
                    pa:loadingStrategy "always" ."#,
            NativeRdfFormat::Turtle,
            None,
            &source_owner,
        )
        .unwrap();
        let view = parsed.dataset().unwrap();
        let source = parsed.source();
        let locations = Locations {
            view: &view,
            source: &source,
        };
        with_doc(&view, &source, &vocabulary, &source_owner, |doc| {
            doc.with_classification(&locations, &source_owner, |nodes| {
                // This budget covers the eight rows and five checks, but it
                // cannot also cover the property scans.
                let mut tight = Governor::new(Budget {
                    work: 16,
                    ..Budget::default()
                });
                assert_eq!(
                    doc.validate_nodes(nodes, &locations, &RefCell::new(&mut tight)),
                    Err(LoadError::Exhausted(StopReason::BudgetExceeded(
                        Resource::Work
                    )))
                );

                let mut ample = Governor::new(Budget::default());
                doc.validate_nodes(nodes, &locations, &RefCell::new(&mut ample))?;
                let mut exact = Governor::new(Budget {
                    work: ample.usage().work,
                    ..Budget::default()
                });
                doc.validate_nodes(nodes, &locations, &RefCell::new(&mut exact))
            })
        })
        .unwrap();
    }

    #[test]
    fn temporary_fno_storage_is_scoped_to_the_reader_and_refunds_on_refusal() {
        let vocabulary = fixture_vocabulary();
        let mut fixture = Governor::new(Budget::default());
        let source_owner = RefCell::new(&mut fixture);
        let parsed =
            control::parse(CATALOG_SOURCE, NativeRdfFormat::Turtle, None, &source_owner).unwrap();
        let view = parsed.dataset().unwrap();
        let source = parsed.source();
        let locations = Locations {
            view: &view,
            source: &source,
        };
        with_doc(&view, &source, &vocabulary, &source_owner, |doc| {
            doc.with_classification(&locations, &source_owner, |nodes| {
                check_fno_view_account(doc, nodes);
                Ok(())
            })
        })
        .unwrap();
    }

    fn check_fno_view_account(doc: &Doc<'_>, nodes: &Nodes<'_>) {
        let mut ample = Governor::new(Budget::default());
        let governor = RefCell::new(&mut ample);
        let meter = governor.borrow().scratch_meter();
        let (view_peak, catalog_bytes, catalog) = doc
            .with_fno_view(nodes, &governor, |view| {
                let view_bytes = meter.live();
                assert!(view_bytes > 0);
                let peak = governor.borrow().usage().peak_scratch_bytes;
                let (storage, _) = Storage::new(&governor);
                let catalog = FnoCatalog::from_dataset_with_control(
                    view,
                    NAMESPACE,
                    storage,
                    Work::new(&governor),
                )
                .unwrap();
                Ok((peak, meter.live() - view_bytes, catalog))
            })
            .unwrap();
        assert!(catalog_bytes > 0);
        assert_eq!(meter.live(), catalog_bytes);
        drop(catalog);
        assert!(meter.live() < catalog_bytes);

        let mut tight = Governor::new(Budget {
            scratch_bytes: view_peak - 1,
            ..Budget::default()
        });
        let governor = RefCell::new(&mut tight);
        let meter = governor.borrow().scratch_meter();
        let refused = doc.with_fno_view(nodes, &governor, |_| -> Result<(), LoadError> {
            panic!("a refused temporary view must not reach its reader");
        });
        assert_eq!(
            refused,
            Err(LoadError::Exhausted(StopReason::BudgetExceeded(
                Resource::ScratchBytes
            )))
        );
        assert_eq!(meter.live(), 0);
    }

    #[test]
    fn retained_source_rows_refund_their_account_on_completion_and_refusal() {
        let vocabulary = fixture_vocabulary();
        let mut fixture = Governor::new(Budget::default());
        let source_owner = RefCell::new(&mut fixture);
        let parsed =
            control::parse(CATALOG_SOURCE, NativeRdfFormat::Turtle, None, &source_owner).unwrap();
        let view = parsed.dataset().unwrap();
        let source = parsed.source();
        let empty = control::parse(b"", NativeRdfFormat::Turtle, None, &source_owner).unwrap();

        let mut ample = Governor::new(Budget::default());
        let governor = RefCell::new(&mut ample);
        let meter = governor.borrow().scratch_meter();
        let other_owner = governor.borrow_mut().lease_scratch(31).unwrap();
        let baseline = meter.live();
        let names_peak = with_doc(
            &empty.dataset().unwrap(),
            &empty.source(),
            &vocabulary,
            &governor,
            |doc| {
                assert_eq!(doc.rows.first().map(|row| row.p.as_str()), None);
                Ok(governor.borrow().usage().peak_scratch_bytes)
            },
        )
        .unwrap();
        assert_eq!(meter.live(), baseline);
        let peak = with_doc(&view, &source, &vocabulary, &governor, |doc| {
            assert_eq!(doc.rows.len(), 4);
            assert!(meter.live() > baseline);
            Ok(governor.borrow().usage().peak_scratch_bytes)
        })
        .unwrap();
        assert!(peak > names_peak);
        assert_eq!(meter.live(), baseline);
        drop(other_owner);
        assert_eq!(meter.live(), 0);

        let mut tight = Governor::new(Budget {
            scratch_bytes: peak - 1,
            ..Budget::default()
        });
        let governor = RefCell::new(&mut tight);
        let meter = governor.borrow().scratch_meter();
        let other_owner = governor.borrow_mut().lease_scratch(baseline).unwrap();
        let refusal = with_doc(
            &view,
            &source,
            &vocabulary,
            &governor,
            |_| -> Result<(), LoadError> {
                panic!("partial rows must not reach the loader's validation stages");
            },
        );
        assert_eq!(
            refusal,
            Err(LoadError::Exhausted(StopReason::BudgetExceeded(
                Resource::ScratchBytes
            )))
        );
        assert_eq!(meter.live(), baseline);
        drop(other_owner);
        assert_eq!(meter.live(), 0);
    }
}
