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
    fno::{FnoCatalog, FnoReadError, FnoReadFailure},
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
    CallableBinding, ContextManagement, LoadingStrategy, OutputByteLimit, ResourceLimits,
    Vocabulary, codec::mcp::ExecutionMode,
};
use classification::{Nodes, RoleNode};
use control::{ControlledStorage, Shared, Storage, Work};
use profile::{BuildStorage, Profiles, matches_predicate};
use roles::{Kind, Role, Spec};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const RDF_PROPERTY: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#Property";
const RDFS_LABEL: &str = "http://www.w3.org/2000/01/rdf-schema#label";
const RDFS_COMMENT: &str = "http://www.w3.org/2000/01/rdf-schema#comment";
const RDFS_CLASS: &str = "http://www.w3.org/2000/01/rdf-schema#Class";
const OWL_CLASS: &str = "http://www.w3.org/2002/07/owl#Class";
const DCTERMS_IS_PART_OF: &str = "http://purl.org/dc/terms/isPartOf";
const SH: &str = "http://www.w3.org/ns/shacl#";
const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";
const SH_PROPERTY: &str = "http://www.w3.org/ns/shacl#property";
const SH_NODE_SHAPE: &str = "http://www.w3.org/ns/shacl#NodeShape";
const SH_PROPERTY_SHAPE: &str = "http://www.w3.org/ns/shacl#PropertyShape";
const FNO: &str = "https://w3id.org/function/ontology#";
const FNOM: &str = "https://w3id.org/function/vocabulary/mapping#";
const FNO_FUNCTION: &str = "https://w3id.org/function/ontology#Function";
const FNO_EXPECTS: &str = "https://w3id.org/function/ontology#expects";
const FNO_RETURNS: &str = "https://w3id.org/function/ontology#returns";

#[derive(Debug)]
struct Row {
    subject_id: TermId,
    object_id: TermId,
    s: TermValue,
    p: String,
    o: TermValue,
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
    target_subject: TermId,
    id: TermValue,
    s: TermValue,
    p: String,
    o: TermValue,
}

/// Owned materialisation of a parsed dataset, indexed by subject.
struct Doc<'v> {
    vocabulary: &'v Vocabulary,
    namespace: &'v str,
    names: Vec<(&'v str, &'static str)>,
    rows: Vec<Row>,
    by_subject: Vec<usize>,
    reifiers: Vec<Reifier>,
}

/// The typed payload drops before its field-mask leases and native allocation account.
#[derive(Debug)]
pub(super) struct RetainedTools {
    tools: Vec<Tool>,
    masks: Vec<ScratchLease>,
    storage: OwnedAdmission<Storage>,
}

impl RetainedTools {
    pub(super) fn as_slice(&self) -> &[Tool] {
        &self.tools
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
            doc.own_every_row(nodes, &locations, governor)?;
            doc.validate_nodes(nodes, &locations, governor)?;
            let catalog = doc.read_catalog(nodes, view, source, governor)?;
            let tools = doc.build_tools(nodes, catalog.as_ref(), &locations, governor)?;
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
                s,
                p,
                o,
            };
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
            let target_subject = s;
            let s = value(s, memory)?;
            let o = value(o, memory)?;
            let reifier = Reifier {
                term_id: quad.s,
                target_subject,
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
                let at = locations.statement(&row.s, &row.p, &row.o, &mut governor.borrow_mut())?;
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
    fn statement(
        &self,
        subject: &TermValue,
        predicate: &str,
        object: &TermValue,
        governor: &mut Governor,
    ) -> Result<Option<SourceExtent>, LoadError> {
        control::default_row_extent(self.view, self.source, subject, predicate, object, governor)
    }

    fn unsupported_row(
        &self,
        row: &Row,
        construct: Construct,
        governor: &mut Governor,
    ) -> Result<LoadError, LoadError> {
        let at = self.statement(&row.s, &row.p, &row.o, governor)?;
        Ok(unsupported(&row.s, &row.p, construct, at))
    }
}

fn unowned_row<V: DatasetView<Id = TermId>>(
    row: &Row,
    view: &V,
    source: &BorrowedSource<'_>,
    governor: &mut Governor,
) -> Result<LoadError, LoadError> {
    Ok(LoadError::UnsupportedConstruct {
        subject: show(&row.s),
        predicate: row.p.clone(),
        construct: Construct::UnownedStatement,
        at: control::default_row_extent(view, source, &row.s, &row.p, &row.o, governor)?,
    })
}

impl Doc<'_> {
    /// Every row has exactly one owning view; an unowned row is a refusal.
    fn own_every_row<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
    ) -> Result<(), LoadError> {
        let mut progress = Work::new(governor);
        for row in &self.rows {
            work(&mut governor.borrow_mut(), 1)?;
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
                return Err(unowned_row(
                    row,
                    locations.view,
                    locations.source,
                    &mut governor.borrow_mut(),
                )?);
            }
        }
        Ok(())
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
        Kind::Str if is_literal(value, &[XSD_STRING]) => Ok(()),
        Kind::Str => Err(wrong("xsd:string literal")),
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
                        return Err(unowned_row(
                            row,
                            locations.view,
                            locations.source,
                            &mut governor.borrow_mut(),
                        )?);
                    }
                } else if let Some(name) = self.find_bound(|key| {
                    compare_bytes(key.as_bytes(), row.p.as_bytes(), &mut progress).map_err(stop)
                })? && Role::domain(name) & mask == 0
                {
                    let at =
                        locations.statement(node, &row.p, &row.o, &mut governor.borrow_mut())?;
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
        let mut protocol = false;
        let mut mount_allow = None;
        for spec in Spec::table(role) {
            work(&mut governor.borrow_mut(), 1)?;
            let predicate = self.term(spec.name);
            let mut found = 0;
            for &index in node.rows {
                if matches_predicate(&self.rows[index], predicate, progress)? {
                    found += 1;
                }
            }
            let cardinality = if found < usize::from(spec.min) {
                Some(Violation::Missing)
            } else if found > usize::from(spec.max) {
                Some(Violation::TooMany { found })
            } else {
                None
            };
            if let Some(violation) = cardinality {
                return Err(Self::wrap(
                    role,
                    node.identity.term,
                    predicate,
                    violation,
                    None,
                ));
            }
            if found == 0 {
                continue;
            }
            if role == Role::NETWORK {
                socket_type |= spec.name == "socketType";
                protocol |= spec.name == "protocol";
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
                if let Err(violation) = check_value(spec.kind, value) {
                    let at = locations.statement(
                        node.identity.term,
                        predicate,
                        value,
                        &mut governor.borrow_mut(),
                    )?;
                    return Err(Self::wrap(
                        role,
                        node.identity.term,
                        predicate,
                        violation,
                        at,
                    ));
                }
                if role == Role::MOUNT && spec.name == "deny" && !boolean(value) {
                    mount_allow = Some(value);
                }
            }
        }
        if role == Role::NETWORK && socket_type == protocol {
            return Err(Self::wrap(
                role,
                node.identity.term,
                self.term("protocol"),
                Violation::Exclusive,
                None,
            ));
        }
        if let Some(value) = mount_allow {
            let predicate = self.term("deny");
            let at = locations.statement(
                node.identity.term,
                predicate,
                value,
                &mut governor.borrow_mut(),
            )?;
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
            if !nodes.owns_fno(row.subject_id) || self.is_vocabulary_row(row, memory)? {
                continue;
            }
            let s = builder.intern_value_with_memory(&row.s, memory)?;
            let p = builder.intern_iri_with_memory(&row.p, memory)?;
            let o = builder.intern_value_with_memory(&row.o, memory)?;
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
        let (subject, predicate, object) = match failure {
            purrdf_core::fno::FnoConstructionFailure::Semantic(shared) => match &**shared {
                FnoReadError::Field {
                    subject, predicate, ..
                }
                | FnoReadError::List {
                    subject, predicate, ..
                } => (Some(subject), Some(predicate.clone()), None),
                FnoReadError::Returns {
                    subject,
                    predicate,
                    head,
                    ..
                } => (Some(subject), Some(predicate.clone()), Some(head)),
                FnoReadError::Statement { row, .. } => (
                    Some(&row.s),
                    Some(show(&row.p)),
                    row.g.is_none().then_some(&row.o),
                ),
                _ => (None, None, None),
            },
            _ => (None, None, None),
        };
        let at = match (subject, predicate.as_deref(), object) {
            (Some(subject), Some(predicate), Some(object)) => {
                match control::default_row_extent(
                    original, source, subject, predicate, object, governor,
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
    iris: &'i [&'d str],
    profiles: Profiles<'n, 'd>,
}

impl ToolLookup<'_, '_, '_> {
    fn contains(&self, iri: &str, progress: &mut Work<'_, '_>) -> Result<bool, LoadError> {
        let found = try_equal_range_by(self.iris, |key| {
            compare_bytes(key.as_bytes(), iri.as_bytes(), progress).map_err(stop)
        })?;
        Ok(!found.is_empty())
    }
}

impl Doc<'_> {
    fn build_tools<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes<'_>,
        catalog: Option<&FnoCatalog>,
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
        let lookup = ToolLookup {
            iris: &iris,
            profiles: Profiles { nodes },
        };
        if let Some(catalog) = catalog {
            self.check_catalog_tools(catalog, &lookup, locations, governor, &mut progress)?;
        }
        let mut consumed = Vec::new();
        scratch.fill(&mut consumed, self.reifiers.len(), false)?;
        let (storage, rejection) = Storage::new(governor);
        let mut retained = RetainedTools {
            tools: Vec::new(),
            masks: Vec::new(),
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
            let Some(catalog) = catalog else {
                return Err(Self::missing_function(node.term));
            };
            if !self.has_type_id(nodes, node.id, FNO_FUNCTION, &mut progress)? {
                return Err(Self::missing_function(node.term));
            }
            let candidate = self.build_tool(
                node,
                catalog,
                &lookup,
                &mut consumed,
                locations,
                &mut storage,
            )?;
            let original_name = self
                .first_property(nodes, node.id, self.term("name"), &mut progress)?
                .map_or("", |row| lexical_str(&row.o));
            if scratch.record_name(&mut seen_names, original_name, &mut progress)? {
                return Err(LoadError::InvalidShape {
                    subject: show(node.term),
                    predicate: self.term("name").to_owned(),
                    at: None,
                    violation: Violation::Duplicate,
                });
            }
            storage.reserve_next(&mut retained.tools)?;
            storage.reserve_next(&mut retained.masks)?;
            let (tool, mask) = candidate;
            retained.tools.push(tool);
            retained.masks.push(mask);
        }
        self.check_reifiers(nodes, &consumed, locations, governor, &mut progress)?;
        // The classified node iterator already visits the IRI subjects in lexical order.
        Ok(retained)
    }

    fn check_catalog_tools<V: DatasetView<Id = TermId>>(
        &self,
        catalog: &FnoCatalog,
        lookup: &ToolLookup<'_, '_, '_>,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        for function in &catalog.as_input().functions {
            work(&mut governor.borrow_mut(), 1)?;
            if lookup.contains(function.iri.as_str(), progress)? {
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
                    at = locations.statement(&row.s, &row.p, &row.o, &mut governor.borrow_mut())?;
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
            let at = control::default_reifier_extent(
                locations.view,
                locations.source,
                reifier,
                &mut governor.borrow_mut(),
            )?;
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

    fn missing_function(node: &TermValue) -> LoadError {
        LoadError::InvalidFnoBinding {
            subject: show(node),
            predicate: Some(RDF_TYPE.to_owned()),
            detail: "a tool must be typed as an fno:Function".to_owned(),
            at: None,
        }
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
        for &index in nodes.rows(id) {
            let row = &self.rows[index];
            if matches_predicate(row, RDF_TYPE, progress)?
                && let Some(iri) = iri_of(&row.o)
                && compare_bytes(iri.as_bytes(), class.as_bytes(), progress)
                    .map_err(stop)?
                    .is_eq()
            {
                return Ok(true);
            }
        }
        Ok(false)
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
        consumed: &mut [bool],
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
        for subtool in &subtools {
            if !lookup.contains(subtool, &mut progress)? {
                return Err(LoadError::InvalidShape {
                    subject: iri.clone(),
                    predicate: self.term("subtool").to_owned(),
                    at: None,
                    violation: Violation::Unresolved,
                });
            }
        }
        let (parameters, output, implementation) =
            self.build_callable(node, nodes, catalog, consumed, locations, storage)?;
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
        let input_shape = storage.lexical(
            &self
                .required_property(nodes, node, "inputShape", &mut progress)?
                .o,
        )?;
        let output_shape = storage.lexical(
            &self
                .required_property(nodes, node, "outputShape", &mut progress)?
                .o,
        )?;
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
                output_shape,
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
    property: Node<'a>,
    used: bool,
}

fn check_shape_complete(
    shape: Node<'_>,
    paths: &[ShapePath<'_>],
    storage: &BuildStorage<'_, '_, '_>,
) -> Result<(), LoadError> {
    for path in paths {
        storage.charge(1)?;
        if !path.used {
            return Err(LoadError::InvalidShape {
                subject: show(shape.term),
                predicate: path.iri.to_owned(),
                at: None,
                violation: Violation::Unresolved,
            });
        }
    }
    Ok(())
}

impl Doc<'_> {
    fn build_callable<V: DatasetView<Id = TermId>>(
        &self,
        node: Node<'_>,
        nodes: &Nodes<'_>,
        catalog: &FnoCatalog,
        consumed: &mut [bool],
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<(Vec<ToolParameter>, ToolOutput, ToolImplementation), LoadError> {
        // INVARIANT: build_tool admitted an IRI before inspecting its binding.
        let iri = iri_of(node.term).expect("tool subject is an IRI");
        let binding = Self::admit_callable(catalog, iri, storage)?;
        let mut progress = Work::new(storage.governor);
        let input_shape =
            Node::object(self.required_property(nodes, node, "inputShape", &mut progress)?);
        let output_shape =
            Node::object(self.required_property(nodes, node, "outputShape", &mut progress)?);
        let (scratch, rejection) = Storage::new(storage.governor);
        let mut scratch = OwnedAdmission::new(scratch);
        let mut scratch = BuildStorage::new(&mut scratch, storage.governor, rejection);
        let mut input_paths =
            self.shape_paths(nodes, input_shape, locations, &mut scratch, &mut progress)?;
        let mut output_paths =
            self.shape_paths(nodes, output_shape, locations, &mut scratch, &mut progress)?;
        let mut parameters = Vec::new();
        for parameter in binding.parameters() {
            storage.charge(1)?;
            // INVARIANT: `CallableBinding::admit` refuses a parameter without a predicate.
            let predicate = parameter
                .parameter
                .predicate
                .as_deref()
                .expect("admitted predicate");
            let shape = Self::path_shape(input_shape, &mut input_paths, predicate, &mut progress)?;
            let hint = self.build_hint(nodes, shape, predicate, consumed, storage)?;
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
        check_shape_complete(input_shape, &input_paths, storage)?;
        let returned = binding.output();
        // INVARIANT: `CallableBinding::admit` refuses an output without a predicate.
        let output_predicate = returned
            .output
            .predicate
            .as_deref()
            .expect("admitted predicate");
        Self::path_shape(
            output_shape,
            &mut output_paths,
            output_predicate,
            &mut progress,
        )?;
        check_shape_complete(output_shape, &output_paths, storage)?;
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

    fn admit_callable<'a>(
        catalog: &'a FnoCatalog,
        iri: &str,
        storage: &BuildStorage<'_, '_, '_>,
    ) -> Result<CallableBinding<'a>, LoadError> {
        let fno_error = |detail: String| LoadError::InvalidFnoBinding {
            subject: iri.to_owned(),
            predicate: None,
            detail,
            at: None,
        };
        let mut found = 0;
        let mut mapping = 0;
        let mut progress = Work::new(storage.governor);
        for (at, row) in catalog.as_input().mappings.iter().enumerate() {
            storage.charge(1)?;
            if compare_bytes(row.function.as_bytes(), iri.as_bytes(), &mut progress)
                .map_err(stop)?
                .is_eq()
            {
                mapping = at;
                found += 1;
            }
        }
        if found != 1 {
            return Err(fno_error(format!(
                "a tool needs exactly one fno:Mapping, found {found}"
            )));
        }
        CallableBinding::admit(catalog, mapping, &mut storage.governor.borrow_mut()).map_err(
            |error| match error {
                crate::BindingError::Resource(reason) => stop(reason),
                other => fno_error(other.to_string()),
            },
        )
    }

    /// Index simple paths in lexical order, preserving the graph nodes as borrows.
    fn shape_paths<'d, V: DatasetView<Id = TermId>>(
        &'d self,
        nodes: &Nodes<'_>,
        shape: Node<'_>,
        locations: &Locations<'_, V>,
        storage: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<Vec<ShapePath<'d>>, LoadError> {
        if !self.has_type_id(nodes, shape.id, SH_NODE_SHAPE, progress)? {
            return Err(LoadError::InvalidShape {
                subject: show(shape.term),
                predicate: RDF_TYPE.to_owned(),
                at: None,
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
            for &index in nodes.rows(property.id) {
                let row = &self.rows[index];
                if matches_predicate(row, SH_PATH, progress)? && path.replace(row).is_some() {
                    path = None;
                    break;
                }
            }
            let Some(path) = path else {
                return Err(LoadError::InvalidShape {
                    subject: show(property.term),
                    predicate: SH_PATH.to_owned(),
                    at: None,
                    violation: Violation::Missing,
                });
            };
            let Some(iri) = iri_of(&path.o) else {
                let at = locations.statement(
                    property.term,
                    SH_PATH,
                    &path.o,
                    &mut storage.governor.borrow_mut(),
                )?;
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
                    property,
                    used: false,
                },
            )?;
        }
        try_sort_unstable_by(&mut paths, |left, right| {
            compare_bytes(left.iri.as_bytes(), right.iri.as_bytes(), progress).map_err(stop)
        })?;
        Ok(paths)
    }

    fn path_shape<'d>(
        shape: Node<'_>,
        paths: &mut [ShapePath<'d>],
        predicate: &str,
        progress: &mut Work<'_, '_>,
    ) -> Result<Node<'d>, LoadError> {
        let range = try_equal_range_by(paths, |path| {
            compare_bytes(path.iri.as_bytes(), predicate.as_bytes(), progress).map_err(stop)
        })?;
        let violation = match &mut paths[range] {
            [only] => {
                only.used = true;
                return Ok(only.property);
            }
            [] => Violation::Unresolved,
            _ => Violation::Duplicate,
        };
        Err(LoadError::InvalidShape {
            subject: show(shape.term),
            predicate: SH_PROPERTY.to_owned(),
            at: None,
            violation,
        })
    }

    fn build_hint(
        &self,
        nodes: &Nodes<'_>,
        shape: Node<'_>,
        predicate: &str,
        consumed: &mut [bool],
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<Option<Hint>, LoadError> {
        let mut matched = None;
        let mut progress = Work::new(storage.governor);
        for (at, reifier) in self.reifiers.iter().enumerate() {
            storage.charge(1)?;
            if reifier.target_subject != shape.id || reifier.p != SH_PATH {
                continue;
            }
            let Some(path) = iri_of(&reifier.o) else {
                continue;
            };
            if !compare_bytes(path.as_bytes(), predicate.as_bytes(), &mut progress)
                .map_err(stop)?
                .is_eq()
            {
                continue;
            }
            if matched.replace(at).is_some() {
                return Err(LoadError::InvalidHint {
                    subject: show(&reifier.id),
                    predicate: SH_PATH.to_owned(),
                    at: None,
                    violation: Violation::Duplicate,
                });
            }
        }
        let Some(at) = matched else { return Ok(None) };
        consumed[at] = true;
        let reifier = &self.reifiers[at];
        let id = &reifier.id;
        let mut none = false;
        let mut integer_to_string = false;
        let mut string_to_integer = false;
        let coercion_predicate = self.term("typeCoercion");
        for &index in nodes.rows(reifier.term_id) {
            let row = &self.rows[index];
            if !matches_predicate(row, coercion_predicate, &mut progress)? {
                continue;
            }
            match lexical_str(&row.o) {
                "none" => none = true,
                "integer-to-string" => integer_to_string = true,
                "string-to-integer" => string_to_integer = true,
                _ => unreachable!("validated type coercion"),
            }
        }
        if none && (integer_to_string || string_to_integer) {
            return Err(LoadError::InvalidHint {
                subject: show(id),
                predicate: self.term("typeCoercion").to_owned(),
                at: None,
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
            reifier: Self::reifier_reference(id, storage)?,
            shape: storage.lexical(shape.term)?,
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
    fn reifier_reference(
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
                        .statement(&row.s, &row.p, &row.o, &mut storage.governor.borrow_mut())?
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
