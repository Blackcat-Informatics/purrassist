// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The loading algorithm: materialise rows, own every row, validate, then type.

mod control;
mod roles;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;

use purrassist_core::{Governor, StopReason, contract::Resource, output::FieldMask};
use purrdf_core::{
    NativeBuildError, RdfDataset, RdfDatasetBuilder, TermId, TermRef, TermValue,
    blank_label::{LabelAlphabet, encode_blank_label_with_memory},
    dataset_from_view,
    dataset_view::DatasetView,
    fno::{FnoCatalog, FnoReadError, FnoReadFailure},
};
use purrdf_lex::{
    allocation::{Admission, Memory, Resident, StorageError},
    control::{ControlledError, Progress, compare_bytes},
    walk::try_sort_unstable_by,
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
use control::{ControlledStorage, Shared, Storage, Work};
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
    s: TermValue,
    p: String,
    o: TermValue,
}

#[derive(Debug)]
struct Reifier {
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
    let (tools, catalog) = match analyse(&view, &source, vocabulary, &governor) {
        Ok(analysed) => analysed,
        Err(mut error) => {
            control::locate(&mut error, &view, &source, &mut governor.borrow_mut())?;
            return Err(error);
        }
    };
    // `PurRDF` exposes only a borrowed retained parse. The published dataset
    // copy is outside the governor's storage account.
    let dataset = dataset_from_view(&view)
        .map_err(|cause| control::read_failure("dataset-materialisation", cause))?;
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
) -> Result<(Vec<Tool>, Option<FnoCatalog>), LoadError>
where
    V::ReadError: std::fmt::Display,
{
    with_doc(view, source, vocabulary, governor, |doc| {
        doc.scan_vocabulary(&mut governor.borrow_mut())?;
        let nodes = doc.classify(&mut governor.borrow_mut())?;
        doc.own_every_row(&nodes, &mut governor.borrow_mut())?;
        doc.validate_nodes(&nodes, view, source, &mut governor.borrow_mut())?;
        let catalog = doc.read_catalog(&nodes, view, source, governor)?;
        let tools = doc.build_tools(&nodes, catalog.as_ref(), &mut governor.borrow_mut())?;
        Ok((tools, catalog))
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

impl<'v> Doc<'v> {
    fn collect<V: DatasetView<Id = TermId>, S: Admission>(
        view: &V,
        source: &BorrowedSource<'_>,
        vocabulary: &'v Vocabulary,
        governor: &Shared<'_>,
        memory: &mut Memory<'_, S>,
        storage_error: &impl Fn(StorageError) -> LoadError,
    ) -> Result<Self, LoadError>
    where
        V::ReadError: std::fmt::Display,
    {
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
        let mut native_work = Work::new(governor);
        let mut progress = |units| native_work.charge(units).map_err(stop);
        try_sort_unstable_by(&mut names, |(left, _), (right, _)| {
            compare_bytes(left.as_bytes(), right.as_bytes(), &mut progress)
        })?;
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
                return Err(unsupported(&s, "-", Construct::UnownedStatement));
            };
            if quad.g.is_some() {
                let at = control::quad_extent(source, quad, layer, &mut governor.borrow_mut())?;
                return Err(named_graph(&s, &p, at));
            }
            let o = value(quad.o, memory)?;
            memory
                .push(&mut rows, Row { s, p, o })
                .map_err(storage_error)?;
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
                return Err(named_graph(&id, "-", at));
            }
            let Some((s, p, o)) = view
                .with_term(quad.o, |term| match term {
                    TermRef::Triple { s, p, o } => Some((s, p, o)),
                    _ => None,
                })
                .map_err(|cause| control::read_failure("dataset-read", cause))?
            else {
                return Err(unsupported(&id, "-", Construct::Reifier));
            };
            let TermValue::Iri(p) = value(p, memory)? else {
                return Err(unsupported(&id, "-", Construct::Reifier));
            };
            let s = value(s, memory)?;
            let o = value(o, memory)?;
            memory
                .push(&mut reifiers, Reifier { id, s, p, o })
                .map_err(storage_error)?;
        }
        // A declared named graph with no statements has no quad to refuse above, so
        // read the view's declared graphs and refuse any that remain.
        if let Some(graph) = view.named_graphs().next() {
            work(&mut governor.borrow_mut(), 1)?;
            let at = control::graph_extent(source, graph, &mut governor.borrow_mut())?;
            return Err(named_graph(&value(graph, memory)?, "-", at));
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

    fn bound(&self, iri: &str) -> Option<&'static str> {
        self.names
            .binary_search_by_key(&iri, |&(key, _)| key)
            .ok()
            .map(|position| self.names[position].1)
    }

    fn in_namespace(&self, iri: &str) -> bool {
        iri.starts_with(self.namespace)
    }

    fn rows_of<'a>(&'a self, subject: &TermValue) -> impl Iterator<Item = &'a Row> + 'a {
        let start = self
            .by_subject
            .partition_point(|&row| self.rows[row].s < *subject);
        let matching = &self.by_subject[start..];
        let length = matching.partition_point(|&row| self.rows[row].s == *subject);
        matching[..length].iter().map(|&row| &self.rows[row])
    }

    fn objects<'a>(&'a self, subject: &TermValue, predicate: &str) -> Vec<&'a TermValue> {
        self.rows_of(subject)
            .filter(|row| row.p == predicate)
            .map(|row| &row.o)
            .collect()
    }

    fn objects_of(&self, subject: &TermValue, name: &'static str) -> Vec<&TermValue> {
        self.objects(subject, self.term(name))
    }

    fn has_type(&self, subject: &TermValue, class: &str) -> bool {
        self.rows_of(subject)
            .any(|row| row.p == RDF_TYPE && iri_of(&row.o) == Some(class))
    }

    /// Refuse every caller-namespace term the vocabulary does not bind.
    fn scan_vocabulary(&self, governor: &mut Governor) -> Result<(), LoadError> {
        let check = |subject: &TermValue, predicate: &str, term: &TermValue| {
            match iri_of(term) {
                // The bare namespace IRI names the vocabulary itself, not a term in it.
                Some(iri)
                    if iri != self.namespace
                        && self.in_namespace(iri)
                        && self.bound(iri).is_none() =>
                {
                    Err(LoadError::UnknownTerm {
                        subject: show(subject),
                        predicate: predicate.to_owned(),
                        term: iri.to_owned(),
                        at: None,
                    })
                }
                _ => Ok(()),
            }
        };
        for row in &self.rows {
            work(governor, 1)?;
            check(&row.s, &row.p, &row.s)?;
            check(&row.s, &row.p, &TermValue::Iri(row.p.clone()))?;
            check(&row.s, &row.p, &row.o)?;
        }
        for reifier in &self.reifiers {
            work(governor, 1)?;
            check(&reifier.id, &reifier.p, &reifier.s)?;
            check(&reifier.id, &reifier.p, &TermValue::Iri(reifier.p.clone()))?;
            check(&reifier.id, &reifier.p, &reifier.o)?;
        }
        Ok(())
    }
}

fn unsupported(subject: &TermValue, predicate: &str, construct: Construct) -> LoadError {
    LoadError::UnsupportedConstruct {
        subject: show(subject),
        predicate: predicate.to_owned(),
        construct,
        at: None,
    }
}

fn named_graph(subject: &TermValue, predicate: &str, at: Option<super::SourceExtent>) -> LoadError {
    LoadError::UnsupportedConstruct {
        subject: show(subject),
        predicate: predicate.to_owned(),
        construct: Construct::NamedGraph,
        at,
    }
}

/// Role assignment for every node the vocabulary recognises.
struct Nodes {
    roles: BTreeMap<TermValue, u16>,
    fno: BTreeSet<TermValue>,
    shapes: BTreeSet<TermValue>,
    shape_cells: BTreeSet<TermValue>,
}

impl Doc<'_> {
    fn classify(&self, governor: &mut Governor) -> Result<Nodes, LoadError> {
        let mut typed: BTreeMap<TermValue, u16> = BTreeMap::new();
        let mut linked: BTreeMap<TermValue, u16> = BTreeMap::new();
        let mut inferred: BTreeMap<TermValue, u16> = BTreeMap::new();
        for row in &self.rows {
            work(governor, 1)?;
            let Some(name) = self.bound(&row.p) else {
                if row.p == RDF_TYPE
                    && let Some(class) = iri_of(&row.o).and_then(|iri| self.bound(iri))
                {
                    if Role::is_runtime(class) {
                        return Err(unsupported(&row.s, &row.p, Construct::RuntimeFact));
                    }
                    *typed.entry(row.s.clone()).or_default() |= Role::of_class(class);
                }
                continue;
            };
            if Role::is_runtime(name) {
                return Err(unsupported(&row.s, &row.p, Construct::RuntimeFact));
            }
            *inferred.entry(row.s.clone()).or_default() |= Role::domain(name);
            let link = Role::link(name);
            if link != 0 {
                *linked.entry(row.o.clone()).or_default() |= link;
            }
        }
        for reifier in &self.reifiers {
            *linked.entry(reifier.id.clone()).or_default() |= Role::HINT;
        }
        let mut roles: BTreeMap<TermValue, u16> = BTreeMap::new();
        for (node, mask) in &typed {
            *roles.entry(node.clone()).or_default() |= mask;
        }
        for (node, mask) in &linked {
            *roles.entry(node.clone()).or_default() |= mask;
        }
        for (node, mask) in &inferred {
            // Omitting `rdf:type` must not evade validation: untyped, unlinked
            // nodes take the role their predicates imply.
            if !roles.contains_key(node) {
                roles.insert(node.clone(), *mask);
            }
        }
        // Shapes: typed, referenced as an argument/result shape, or a `sh:property` value.
        let mut shapes = BTreeSet::new();
        for row in &self.rows {
            if (row.p == RDF_TYPE
                && matches!(iri_of(&row.o), Some(SH_NODE_SHAPE | SH_PROPERTY_SHAPE)))
                || row.p == SH_PROPERTY
            {
                shapes.insert(if row.p == SH_PROPERTY {
                    row.o.clone()
                } else {
                    row.s.clone()
                });
            }
            if row.p == SH_PROPERTY {
                shapes.insert(row.s.clone());
            }
        }
        for (node, mask) in &linked {
            if mask & Role::SHAPE != 0 {
                shapes.insert(node.clone());
            }
        }
        let mut shape_cells = BTreeSet::new();
        for node in &shapes {
            for row in self.rows_of(node) {
                self.claim_list(&row.o, &mut shape_cells, governor)?;
            }
        }
        let mut fno = self.fno_subjects();
        let mut cells = BTreeSet::new();
        for subject in &fno {
            for predicate in [FNO_EXPECTS, FNO_RETURNS] {
                for head in self.objects(subject, predicate) {
                    self.claim_list(head, &mut cells, governor)?;
                }
            }
        }
        fno.extend(cells);
        Ok(Nodes {
            roles,
            fno,
            shapes,
            shape_cells,
        })
    }

    fn fno_subjects(&self) -> BTreeSet<TermValue> {
        let mut fno = BTreeSet::new();
        for indexes in self
            .by_subject
            .chunk_by(|&left, &right| self.rows[left].s == self.rows[right].s)
        {
            let subject = &self.rows[indexes[0]].s;
            let is_fno = indexes.iter().any(|&at| {
                let row = &self.rows[at];
                row.p == DCTERMS_IS_PART_OF
                    || (row.p == RDF_TYPE
                        && iri_of(&row.o).is_some_and(|iri| {
                            iri.starts_with(FNO)
                                || iri.starts_with(FNOM)
                                || self.bound(iri) == Some("Tool")
                        }))
            });
            if is_fno {
                fno.insert(subject.clone());
            }
        }
        fno
    }

    /// Collect the blank cells of the RDF list starting at `head`.
    fn claim_list(
        &self,
        head: &TermValue,
        cells: &mut BTreeSet<TermValue>,
        governor: &mut Governor,
    ) -> Result<(), LoadError> {
        let mut at = head.clone();
        while matches!(at, TermValue::Blank { .. })
            && self.rows_of(&at).any(|row| row.p == RDF_FIRST)
            && cells.insert(at.clone())
        {
            work(governor, 1)?;
            let Some(next) = self
                .objects(&at, RDF_REST)
                .first()
                .map(|next| (*next).clone())
            else {
                break;
            };
            at = next;
        }
        Ok(())
    }

    /// Every row has exactly one owning view; an unowned row is a refusal.
    fn own_every_row(&self, nodes: &Nodes, governor: &mut Governor) -> Result<(), LoadError> {
        for row in &self.rows {
            work(governor, 1)?;
            let vocabulary_predicate = self.in_namespace(&row.p);
            let vocabulary_class =
                row.p == RDF_TYPE && iri_of(&row.o).is_some_and(|iri| self.in_namespace(iri));
            let declaration = row.p == RDF_TYPE
                && matches!(iri_of(&row.o), Some(RDFS_CLASS | OWL_CLASS | RDF_PROPERTY));
            let annotation = matches!(row.p.as_str(), RDFS_LABEL | RDFS_COMMENT);
            let owned = if nodes.fno.contains(&row.s) {
                !vocabulary_predicate && !vocabulary_class
                    || nodes
                        .roles
                        .get(&row.s)
                        .is_some_and(|mask| mask & Role::TOOL != 0)
            } else if vocabulary_predicate || vocabulary_class {
                nodes.roles.contains_key(&row.s)
            } else if nodes.shapes.contains(&row.s) {
                row.p.starts_with(SH) || row.p == RDF_TYPE || annotation
            } else if nodes.shape_cells.contains(&row.s) {
                matches!(row.p.as_str(), RDF_FIRST | RDF_REST)
            } else if nodes.roles.contains_key(&row.s) {
                annotation || row.p == RDF_TYPE
            } else {
                declaration
            };
            if !owned {
                return Err(unsupported(&row.s, &row.p, Construct::UnownedStatement));
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

fn lexical(value: &TermValue) -> String {
    match value {
        TermValue::Literal { lexical_form, .. } => lexical_form.clone(),
        other => show(other),
    }
}

impl Doc<'_> {
    /// The concrete roles of a node: its recognised bits, with rule markers resolved.
    fn concrete(&self, node: &TermValue, mask: u16) -> Result<u16, LoadError> {
        let mut mask = mask & !Role::SHAPE;
        if mask & Role::RULE_ANY != 0 {
            let concrete = mask & Role::RULES;
            let concrete = if concrete == 0 || concrete == Role::RULES {
                // Infer from concrete-only predicates when the class was omitted.
                self.rows_of(node)
                    .filter_map(|row| self.bound(&row.p))
                    .map(|name| Role::domain(name) & Role::RULES)
                    .filter(|domain| *domain != Role::RULES)
                    .fold(0, |all, domain| all | domain)
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
        nodes: &Nodes,
        view: &V,
        source: &BorrowedSource<'_>,
        governor: &mut Governor,
    ) -> Result<(), LoadError> {
        for (node, &mask) in &nodes.roles {
            let mask = self.concrete(node, mask)?;
            if mask == 0 {
                // A node with no concrete role (a bare argument/result shape, or one
                // typed only as the abstract `Node` class) has no property table, so
                // a vocabulary statement on it would be validated by nothing.
                for row in self.rows_of(node) {
                    work(governor, 1)?;
                    let vocabulary_class = row.p == RDF_TYPE
                        && iri_of(&row.o).is_some_and(|iri| self.in_namespace(iri));
                    if self.in_namespace(&row.p) || vocabulary_class {
                        return Err(unsupported(node, &row.p, Construct::UnownedStatement));
                    }
                }
                continue;
            }
            // A predicate whose domain excludes every role of its subject.
            for row in self.rows_of(node) {
                work(governor, 1)?;
                if let Some(name) = self.bound(&row.p)
                    && Role::domain(name) & mask == 0
                {
                    let at =
                        control::default_row_extent(view, source, node, &row.p, &row.o, governor)?;
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
                for spec in Spec::table(role) {
                    work(governor, 1)?;
                    let predicate = self.term(spec.name);
                    let values = self.objects(node, predicate);
                    let found = values.len();
                    if found < usize::from(spec.min) {
                        return Err(Self::wrap(role, node, predicate, Violation::Missing, None));
                    }
                    if found > usize::from(spec.max) {
                        return Err(Self::wrap(
                            role,
                            node,
                            predicate,
                            Violation::TooMany { found },
                            None,
                        ));
                    }
                    for value in values {
                        if let Err(violation) = check_value(spec.kind, value) {
                            let at = control::default_row_extent(
                                view, source, node, predicate, value, governor,
                            )?;
                            return Err(Self::wrap(role, node, predicate, violation, at));
                        }
                    }
                }
                if role == Role::NETWORK {
                    let has = |name| !self.objects_of(node, name).is_empty();
                    if has("socketType") == has("protocol") {
                        return Err(Self::wrap(
                            role,
                            node,
                            self.term("protocol"),
                            Violation::Exclusive,
                            None,
                        ));
                    }
                }
                if role == Role::MOUNT
                    && let Some(value) = self
                        .objects_of(node, "deny")
                        .into_iter()
                        .find(|value| !boolean(value))
                {
                    let predicate = self.term("deny");
                    let at = control::default_row_extent(
                        view, source, node, predicate, value, governor,
                    )?;
                    return Err(Self::wrap(
                        role,
                        node,
                        predicate,
                        Violation::OutOfRange {
                            expected: "true: a mount rule is always a denial",
                        },
                        at,
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Doc<'_> {
    /// Read the strict `FnO` catalog from the rows the `FnO` view owns, under the
    /// governor's storage, work and stop authority.
    fn read_catalog<V: DatasetView<Id = TermId>>(
        &self,
        nodes: &Nodes,
        original: &V,
        source: &BorrowedSource<'_>,
        governor: &Shared<'_>,
    ) -> Result<Option<FnoCatalog>, LoadError> {
        if nodes.fno.is_empty() {
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
        nodes: &Nodes,
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
        nodes: &Nodes,
        memory: &mut Memory<'_, impl Admission>,
    ) -> Result<RdfDataset, NativeBuildError> {
        let mut builder = RdfDatasetBuilder::new();
        for row in &self.rows {
            memory
                .admission_mut()
                .checkpoint(control::NATIVE_UNITS_PER_WORK)?;
            let is_vocabulary = self.in_namespace(&row.p)
                || (row.p == RDF_TYPE && iri_of(&row.o).is_some_and(|iri| self.in_namespace(iri)));
            if !nodes.fno.contains(&row.s) || is_vocabulary {
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

impl Doc<'_> {
    fn build_tools(
        &self,
        nodes: &Nodes,
        catalog: Option<&FnoCatalog>,
        governor: &mut Governor,
    ) -> Result<Vec<Tool>, LoadError> {
        let tool_nodes: Vec<&TermValue> = nodes
            .roles
            .iter()
            .filter(|(_, mask)| **mask & Role::TOOL != 0)
            .map(|(node, _)| node)
            .collect();
        let tool_iris: BTreeSet<&str> = tool_nodes.iter().filter_map(|node| iri_of(node)).collect();
        if let Some(catalog) = catalog {
            for function in &catalog.as_input().functions {
                if !tool_iris.contains(function.iri.as_str()) {
                    return Err(LoadError::UnsupportedConstruct {
                        subject: function.iri.clone(),
                        predicate: RDF_TYPE.to_owned(),
                        construct: Construct::FunctionWithoutTool,
                        at: None,
                    });
                }
            }
        }
        let mut consumed = BTreeSet::new();
        let mut tools = Vec::new();
        let mut seen_names = BTreeSet::new();
        for node in tool_nodes {
            let Some(catalog) = catalog.filter(|_| self.has_type(node, FNO_FUNCTION)) else {
                return Err(LoadError::InvalidFnoBinding {
                    subject: show(node),
                    predicate: Some(RDF_TYPE.to_owned()),
                    detail: "a tool must be typed as an fno:Function".to_owned(),
                    at: None,
                });
            };
            let tool = self.build_tool(node, catalog, &tool_iris, &mut consumed, governor)?;
            if !seen_names.insert(tool.name.clone()) {
                return Err(LoadError::InvalidShape {
                    subject: show(node),
                    predicate: self.term("name").to_owned(),
                    at: None,
                    violation: Violation::Duplicate,
                });
            }
            tools.push(tool);
        }
        for (at, reifier) in self.reifiers.iter().enumerate() {
            if consumed.contains(&at) {
                continue;
            }
            return Err(if self.has_type(&reifier.id, self.term("Hint")) {
                LoadError::InvalidHint {
                    subject: show(&reifier.id),
                    predicate: reifier.p.clone(),
                    at: None,
                    violation: Violation::Unresolved,
                }
            } else {
                unsupported(&reifier.id, &reifier.p, Construct::Reifier)
            });
        }
        tools.sort_by(|left, right| left.iri.cmp(&right.iri));
        Ok(tools)
    }

    fn single<'a>(&'a self, node: &TermValue, name: &'static str) -> Option<&'a TermValue> {
        self.objects_of(node, name).first().copied()
    }

    fn text(&self, node: &TermValue, name: &'static str) -> String {
        self.single(node, name).map_or_else(String::new, lexical)
    }

    fn flag(&self, node: &TermValue, name: &'static str) -> Option<bool> {
        self.single(node, name).map(boolean)
    }

    fn texts(&self, node: &TermValue, name: &'static str) -> Vec<String> {
        let mut all: Vec<String> = self
            .objects_of(node, name)
            .into_iter()
            .map(lexical)
            .collect();
        all.sort();
        all
    }

    fn number(&self, node: &TermValue, name: &'static str) -> u64 {
        // INVARIANT: `validate_nodes` admitted this value as an unsigned 64-bit integer.
        parse_u64(&self.text(node, name)).expect("validated integer")
    }

    fn build_tool(
        &self,
        node: &TermValue,
        catalog: &FnoCatalog,
        tool_iris: &BTreeSet<&str>,
        consumed: &mut BTreeSet<usize>,
        governor: &mut Governor,
    ) -> Result<Tool, LoadError> {
        let TermValue::Iri(iri) = node else {
            return Err(LoadError::InvalidShape {
                subject: show(node),
                predicate: self.term("name").to_owned(),
                at: None,
                violation: Violation::WrongKind {
                    expected: "an IRI subject",
                },
            });
        };
        let mut subtools = self.texts(node, "subtool");
        if subtools.iter().any(|iri| !tool_iris.contains(iri.as_str())) {
            return Err(LoadError::InvalidShape {
                subject: iri.clone(),
                predicate: self.term("subtool").to_owned(),
                at: None,
                violation: Violation::Unresolved,
            });
        }
        subtools.sort();
        let (parameters, output, implementation) =
            self.build_callable(node, iri, catalog, consumed, governor)?;
        let annotations = ToolAnnotations {
            title: self.single(node, "title").map(lexical),
            read_only_hint: self.flag(node, "readOnlyHint"),
            destructive_hint: self.flag(node, "destructiveHint"),
            idempotent_hint: self.flag(node, "idempotentHint"),
            open_world_hint: self.flag(node, "openWorldHint"),
        };
        let profile_node = self.node_of(node, "executionProfile")?;
        let mut stack = Vec::new();
        Ok(Tool {
            iri: iri.clone(),
            name: self.text(node, "name"),
            description: self.text(node, "description"),
            annotations,
            input_shape: show(&self.node_of(node, "inputShape")?),
            output_shape: show(&self.node_of(node, "outputShape")?),
            implementation,
            parameters,
            output,
            execution_profile: self.build_profile(&profile_node, &mut stack, governor)?,
            context: self.build_context(&self.node_of(node, "contextManagement")?, governor)?,
            parameter_constraints: self.text(node, "parameterConstraints"),
            subtools,
        })
    }

    fn node_of(&self, node: &TermValue, name: &'static str) -> Result<TermValue, LoadError> {
        self.single(node, name)
            .cloned()
            .ok_or_else(|| LoadError::InvalidShape {
                subject: show(node),
                predicate: self.term(name).to_owned(),
                at: None,
                violation: Violation::Missing,
            })
    }
}

impl Doc<'_> {
    fn build_callable(
        &self,
        node: &TermValue,
        iri: &str,
        catalog: &FnoCatalog,
        consumed: &mut BTreeSet<usize>,
        governor: &mut Governor,
    ) -> Result<(Vec<ToolParameter>, ToolOutput, ToolImplementation), LoadError> {
        let fno_error = |detail: String| LoadError::InvalidFnoBinding {
            subject: iri.to_owned(),
            predicate: None,
            detail,
            at: None,
        };
        let mappings: Vec<usize> = catalog
            .as_input()
            .mappings
            .iter()
            .enumerate()
            .filter(|(_, mapping)| mapping.function == iri)
            .map(|(at, _)| at)
            .collect();
        let [mapping] = mappings[..] else {
            return Err(fno_error(format!(
                "a tool needs exactly one fno:Mapping, found {}",
                mappings.len()
            )));
        };
        let binding =
            CallableBinding::admit(catalog, mapping, governor).map_err(|error| match error {
                crate::BindingError::Resource(reason) => stop(reason),
                other => fno_error(other.to_string()),
            })?;
        let input_shape = self.node_of(node, "inputShape")?;
        let output_shape = self.node_of(node, "outputShape")?;
        let input_paths = self.shape_paths(&input_shape)?;
        let output_paths = self.shape_paths(&output_shape)?;
        let mut parameters = Vec::new();
        let mut used = BTreeSet::new();
        for parameter in binding.parameters() {
            work(governor, 1)?;
            // INVARIANT: `CallableBinding::admit` refuses a parameter without a predicate.
            let predicate = parameter
                .parameter
                .predicate
                .clone()
                .expect("admitted predicate");
            let shape = Self::path_shape(&input_shape, &input_paths, &predicate)?;
            used.insert(predicate.clone());
            let hint = self.build_hint(&shape, &predicate, consumed, governor)?;
            parameters.push(ToolParameter {
                iri: parameter.parameter.iri.clone(),
                predicate,
                datatype: parameter.parameter.r#type.clone(),
                required: parameter.parameter.required,
                implementation_property: parameter.mapping.implementation_property.clone(),
                hint,
            });
        }
        if let Some((extra, _)) = input_paths.iter().find(|(path, _)| !used.contains(*path)) {
            return Err(LoadError::InvalidShape {
                subject: show(&input_shape),
                predicate: extra.clone(),
                at: None,
                violation: Violation::Unresolved,
            });
        }
        let returned = binding.output();
        // INVARIANT: `CallableBinding::admit` refuses an output without a predicate.
        let output_predicate = returned
            .output
            .predicate
            .clone()
            .expect("admitted predicate");
        Self::path_shape(&output_shape, &output_paths, &output_predicate)?;
        if let Some((extra, _)) = output_paths
            .iter()
            .find(|(path, _)| **path != output_predicate)
        {
            return Err(LoadError::InvalidShape {
                subject: show(&output_shape),
                predicate: extra.clone(),
                at: None,
                violation: Violation::Unresolved,
            });
        }
        let implementation = binding.implementation();
        Ok((
            parameters,
            ToolOutput {
                iri: returned.output.iri.clone(),
                predicate: output_predicate,
                datatype: returned.output.r#type.clone(),
                implementation_property: returned.mapping.implementation_property.clone(),
            },
            ToolImplementation {
                iri: implementation.iri.clone(),
                format: implementation.format.clone(),
                see_also: implementation.see_also.clone(),
            },
        ))
    }

    /// `sh:path` IRI -> property shapes of a node shape, refusing anything else.
    fn shape_paths(
        &self,
        shape: &TermValue,
    ) -> Result<BTreeMap<String, Vec<TermValue>>, LoadError> {
        if !self.has_type(shape, SH_NODE_SHAPE) {
            return Err(LoadError::InvalidShape {
                subject: show(shape),
                predicate: RDF_TYPE.to_owned(),
                at: None,
                violation: Violation::Unresolved,
            });
        }
        let mut paths: BTreeMap<String, Vec<TermValue>> = BTreeMap::new();
        for property in self.objects(shape, SH_PROPERTY) {
            let [path] = self.objects(property, SH_PATH)[..] else {
                return Err(LoadError::InvalidShape {
                    subject: show(property),
                    predicate: SH_PATH.to_owned(),
                    at: None,
                    violation: Violation::Missing,
                });
            };
            let Some(path) = iri_of(path) else {
                return Err(unsupported(property, SH_PATH, Construct::ComplexPath));
            };
            paths
                .entry(path.to_owned())
                .or_default()
                .push(property.clone());
        }
        Ok(paths)
    }

    fn path_shape(
        shape: &TermValue,
        paths: &BTreeMap<String, Vec<TermValue>>,
        predicate: &str,
    ) -> Result<TermValue, LoadError> {
        match paths.get(predicate).map(Vec::as_slice) {
            Some([only]) => Ok(only.clone()),
            Some(_) => Err(LoadError::InvalidShape {
                subject: show(shape),
                predicate: SH_PROPERTY.to_owned(),
                at: None,
                violation: Violation::Duplicate,
            }),
            None => Err(LoadError::InvalidShape {
                subject: show(shape),
                predicate: SH_PROPERTY.to_owned(),
                at: None,
                violation: Violation::Unresolved,
            }),
        }
    }

    fn build_hint(
        &self,
        shape: &TermValue,
        predicate: &str,
        consumed: &mut BTreeSet<usize>,
        governor: &mut Governor,
    ) -> Result<Option<Hint>, LoadError> {
        let path = TermValue::Iri(predicate.to_owned());
        let matching: Vec<usize> = self
            .reifiers
            .iter()
            .enumerate()
            .filter(|(_, reifier)| &reifier.s == shape && reifier.p == SH_PATH && reifier.o == path)
            .map(|(at, _)| at)
            .collect();
        let [at] = matching[..] else {
            return match matching.get(1) {
                Some(&second) => Err(LoadError::InvalidHint {
                    subject: show(&self.reifiers[second].id),
                    predicate: SH_PATH.to_owned(),
                    at: None,
                    violation: Violation::Duplicate,
                }),
                None => Ok(None),
            };
        };
        consumed.insert(at);
        let reifier = &self.reifiers[at];
        let id = &reifier.id;
        let coercions = self.texts(id, "typeCoercion");
        let converting = coercions.iter().filter(|name| *name != "none").count();
        if converting != 0 && converting != coercions.len() {
            return Err(LoadError::InvalidHint {
                subject: show(id),
                predicate: self.term("typeCoercion").to_owned(),
                at: None,
                violation: Violation::OutOfRange {
                    expected: "none alone, or conversions without none",
                },
            });
        }
        Ok(Some(Hint {
            reifier: Self::reifier_reference(id, governor)?,
            shape: show(shape),
            max_edit_distance: self.number(id, "maxEditDistance"),
            aliases: self.texts(id, "alias"),
            coercions: coercions
                .iter()
                .filter_map(|name| match name.as_str() {
                    "string-to-integer" => Some(Coercion::StringToInteger),
                    "integer-to-string" => Some(Coercion::IntegerToString),
                    _ => None,
                })
                .collect(),
            case_fold: self.flag(id, "caseFold").unwrap_or(false),
            unicode_fold: self.flag(id, "unicodeFold").unwrap_or(false),
            taints: self.texts(id, "taint"),
            session_binding: self.single(id, "sessionBinding").map(show),
        }))
    }

    /// Use the same scoped blank label as the retained Turtle graph.
    fn reifier_reference(id: &TermValue, governor: &mut Governor) -> Result<String, LoadError> {
        let Some((label, scope)) = id.as_blank() else {
            return Ok(show(id));
        };
        let shared = RefCell::new(governor);
        let (mut account, rejection) = ControlledStorage::owned(&shared);
        let mut memory = Memory::new(&mut account);
        let storage_error = |cause| control::storage_stop(&shared, &rejection, &cause);
        let label = encode_blank_label_with_memory(
            label,
            scope,
            LabelAlphabet::BlankNodeLabel,
            &mut memory,
        )
        .map_err(storage_error)?;
        work(
            &mut shared.borrow_mut(),
            u64::try_from(label.len())
                .unwrap_or(u64::MAX)
                .saturating_add(2),
        )?;
        // The published Tool value has its own storage. Only the temporary
        // encoded label belongs to the scratch account.
        Memory::new(&mut Resident)
            .format(&format_args!("_:{label}"))
            .map_err(storage_error)
    }
}

impl Doc<'_> {
    fn build_context(
        &self,
        node: &TermValue,
        governor: &mut Governor,
    ) -> Result<ContextManagement, LoadError> {
        let projection = self.text(node, "outputProjectionJq");
        let mask =
            FieldMask::parse_governed(&projection, governor).map_err(|error| match error {
                purrassist_core::output::FieldMaskError::Resource(reason) => stop(reason),
                error => LoadError::UnsupportedProjection {
                    subject: show(node),
                    blank_scope: node.as_blank().map(|(_, scope)| scope),
                    predicate: self.term("outputProjectionJq").to_owned(),
                    projection,
                    error,
                    at: None,
                },
            })?;
        let limit = OutputByteLimit::new(self.number(node, "maxOutputBytes"))
            // INVARIANT: `validate_nodes` admitted a positive value.
            .expect("validated positive byte limit");
        let loading = if self.text(node, "loadingStrategy") == "always" {
            LoadingStrategy::Always
        } else {
            LoadingStrategy::OnDemand
        };
        Ok(ContextManagement::new(
            mask,
            limit,
            self.number(node, "historyRetentionTurns"),
            self.texts(node, "intentTags"),
            loading,
        ))
    }

    fn build_profile(
        &self,
        node: &TermValue,
        stack: &mut Vec<TermValue>,
        governor: &mut Governor,
    ) -> Result<ToolExecutionProfile, LoadError> {
        work(governor, 1)?;
        if stack.contains(node) {
            return Err(LoadError::InvalidExecutionProfile {
                subject: show(node),
                predicate: self.term("subprofile").to_owned(),
                at: None,
                violation: Violation::Cycle,
            });
        }
        let depth = u32::try_from(stack.len()).unwrap_or(u32::MAX);
        if depth >= governor.limits().depth {
            return Err(stop(StopReason::BudgetExceeded(Resource::Depth)));
        }
        stack.push(node.clone());
        let mut subprofiles = Vec::new();
        let mut children: Vec<&TermValue> = self.objects_of(node, "subprofile");
        children.sort();
        for child in children {
            subprofiles.push(self.build_profile(child, stack, governor)?);
        }
        stack.pop();
        let mut rules = Vec::new();
        for rule in self.objects_of(node, "rule") {
            rules.push(self.build_rule(rule)?);
        }
        rules.sort_by(|left, right| left.iri.cmp(&right.iri));
        let mut waivers = Vec::new();
        for waiver in self.objects_of(node, "waiver") {
            waivers.push(Waiver {
                iri: show(waiver),
                waived_rule: self
                    .single(waiver, "waivedRule")
                    .map_or_else(String::new, show),
                reviewer: self.text(waiver, "reviewer"),
                rationale: self.text(waiver, "rationale"),
                source: self.text(waiver, "waiverSource"),
            });
        }
        waivers.sort_by(|left, right| left.iri.cmp(&right.iri));
        let mode = if self.text(node, "confinementMode") == "aa-exec" {
            ExecutionMode::AaExec
        } else {
            ExecutionMode::ChangeHat
        };
        Ok(ToolExecutionProfile {
            iri: show(node),
            mode,
            profile_name: self.text(node, "profileName"),
            hat_name: self.text(node, "hatName"),
            egress_allowed: self.flag(node, "egressAllowed").unwrap_or(false),
            limits: ResourceLimits {
                address_space_bytes: self.number(node, "addressSpaceBytes"),
                file_size_bytes: self.number(node, "fileSizeBytes"),
                processes: self.number(node, "processCount"),
                cpu_seconds: self.number(node, "cpuSeconds"),
                open_files: self.number(node, "openFileCount"),
            },
            rules,
            subprofiles,
            waivers,
        })
    }

    fn build_rule(&self, node: &TermValue) -> Result<ProfileRule, LoadError> {
        let mask = node_mask(self, node);
        let class = Role::label_class(self.concrete(node, mask)? & Role::RULES);
        let mut properties: Vec<(&'static str, String)> = self
            .rows_of(node)
            .filter_map(|row| self.bound(&row.p).map(|name| (name, lexical(&row.o))))
            .collect();
        properties.sort();
        Ok(ProfileRule {
            iri: show(node),
            class,
            properties,
        })
    }
}

/// Recompute a rule node's raw role mask from its types and predicates.
fn node_mask(doc: &Doc<'_>, node: &TermValue) -> u16 {
    let mut mask = Role::RULE_ANY;
    for row in doc.rows_of(node) {
        if row.p == RDF_TYPE
            && let Some(class) = iri_of(&row.o).and_then(|iri| doc.bound(iri))
        {
            mask |= Role::of_class(class);
        }
    }
    mask
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
    fn temporary_fno_storage_is_scoped_to_the_reader_and_refunds_on_refusal() {
        let vocabulary = fixture_vocabulary();
        let mut fixture = Governor::new(Budget::default());
        let source_owner = RefCell::new(&mut fixture);
        let parsed =
            control::parse(CATALOG_SOURCE, NativeRdfFormat::Turtle, None, &source_owner).unwrap();
        let view = parsed.dataset().unwrap();
        with_doc(&view, &parsed.source(), &vocabulary, &source_owner, |doc| {
            let nodes = doc.classify(&mut source_owner.borrow_mut()).unwrap();
            check_fno_view_account(doc, &nodes);
            Ok(())
        })
        .unwrap();
    }

    fn check_fno_view_account(doc: &Doc<'_>, nodes: &Nodes) {
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
