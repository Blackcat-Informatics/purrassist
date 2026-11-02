// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Compiled JSON Schema constraints and source-addressed validation.
pub mod algebra;
mod build;
mod builtin;
pub use build::CompilationError;
mod compile;
mod evaluate;
mod format;
mod index;
mod layout;
pub mod recovery;
mod reference;
mod report;
mod uri;
mod vocabulary;
use crate::digest::{Digest, DigestBuilder};
use crate::json::{Node, Value};
use crate::{Budget, Governor, StopReason};
use crate::{ScratchString, ScratchVec};
use core::fmt;
use index::{Arena, Map, Route};
use std::collections::BTreeMap;
pub use vocabulary::Dialect;
use vocabulary::{Keywords, Vocabularies};

/// Default schema dialect URI: Draft 2020-12, used by a root without `$schema`.
pub const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Draft-07 dialect URI (selected by `$schema` with or without the empty `#` fragment).
pub const DRAFT_07: &str = "http://json-schema.org/draft-07/schema";

/// A compiled schema plus caller-supplied external resources. Compilation does no I/O.
///
/// The schema owns prepared pattern storage and does not implement `Clone`.
/// Borrow it for validation, generation, recovery and host helper lifetimes.
#[derive(Debug)]
pub struct Schema {
    documents: Arena<crate::json::OwnedDocument>,
    index: Index,
    root: usize,
    digest: Digest,
}
#[derive(Debug)]
struct Index {
    locations: Arena<Location>,
    by_route: Map<Route, usize>,
    resources: Map<ScratchString, usize>,
    anchors: Map<ScratchString, usize>,
    /// Unsupported optional (`$vocabulary` ... `false`)
    /// vocabulary URIs discovered during compilation, mapped to the
    /// declaring schema resource's own keywords this engine does not
    /// otherwise recognize.
    unsupported: Map<ScratchString, Map<ScratchString, ()>>,
    /// Dialect inherited by the root of a separately supplied document that
    /// does not declare `$schema`: the dialect of the location whose
    /// reference first loaded it, recorded only when that is Draft-07 (absent
    /// documents default to Draft 2020-12).
    document_dialects: Map<usize, Vocabularies>,
}
impl Index {
    fn new(governor: &Governor) -> Self {
        Self {
            locations: Arena::new(governor),
            by_route: Map::new(governor),
            resources: Map::new(governor),
            anchors: Map::new(governor),
            unsupported: Map::new(governor),
            document_dialects: Map::new(governor),
        }
    }
}
#[derive(Debug)]
struct Location {
    vocabularies: Vocabularies,
    /// Every prepared child key was unique before last-value map assembly.
    children_unique: bool,
    document: usize,
    route: ScratchVec<usize>,
    pointer: ScratchString,
    base: ScratchString,
    children: Map<ScratchString, usize>,
    patterns: Map<ScratchString, crate::pattern::Pattern>,
    dynamic_anchor: Option<ScratchString>,
}

/// Invalid schema syntax, keyword use, or resource linkage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaError {
    /// Location within the schema resource.
    pub location: String,
    /// Why compilation failed.
    pub message: String,
}
impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at schema {}", self.message, self.location)
    }
}
impl std::error::Error for SchemaError {}

/// One unsupported, optional (`$vocabulary` ... `false`)
/// vocabulary URI, naming the keywords its declaring schema resource used
/// that this engine leaves otherwise unevaluated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedVocabulary {
    /// The unrecognized vocabulary URI.
    pub uri: String,
    /// Keywords on the schema resource that declared this dialect, not
    /// otherwise recognized by any supported vocabulary.
    pub keywords: Vec<String>,
}

/// One failed constraint at a precise instance and schema location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// RFC 6901 pointer into the instance.
    pub instance_path: String,
    /// Absolute schema location and keyword.
    pub schema_path: String,
    /// Keyword that failed.
    pub keyword: String,
    /// Observable reason for the failure.
    pub message: String,
}

/// Whole-instance validation evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationReport {
    /// Identity of the root, supplied resources, and validator dialect profile.
    pub schema: Digest,
    /// Dialect of the root schema resource, which names the validator.
    pub dialect: Dialect,
    /// Whether all evaluated assertions passed.
    pub valid: bool,
    /// Every collected failed constraint.
    pub violations: Vec<Violation>,
    /// Results of every evaluated `format` keyword, including nested instances
    /// and unsuccessful, negated, or conditional branches. A failed annotation
    /// becomes a [`Violation`] only under the format-assertion vocabulary.
    /// Each entry identifies its format validator and instance JSON Pointer;
    /// evaluating several schemas at one instance can produce several entries.
    pub format_evidence: Vec<crate::contract::Validation>,
    /// Pattern-engine wiring: which pattern-matching engine
    /// (`pattern.engine.derivative-v1`/`pattern.engine.backtrack-v1`)
    /// evaluated each distinct `pattern`/`patternProperties` keyword this
    /// validation actually exercised.
    pub pattern_evidence: Vec<crate::contract::Evidence>,
}

impl ValidationReport {
    /// The receipt record of this whole-instance validation.
    ///
    /// The record names the root dialect's validator ([`Dialect::validator`]:
    /// `purrassist.schema.draft2020-12.v1` or `purrassist.schema.draft-07.v1`), uses this report's schema digest as
    /// its constraint, copies [`Self::valid`], and carries one diagnostic
    /// `"{instance_path}: {message} ({schema_path})"` per violation in
    /// report order. [`Self::format_evidence`] entries are separate
    /// validation records and are not folded in; callers that publish them
    /// append them after this record. Conversion is infallible and charges
    /// nothing: callers retaining the result in a receipt account for its
    /// bytes themselves.
    ///
    /// ```
    /// use purrassist_core::{json, schema::Schema};
    /// let schema = Schema::compile(r#"{"type":"integer"}"#).unwrap();
    /// let report = schema.validate(&json::parse(r#""x""#).unwrap()).unwrap();
    /// let validation = report.to_validation();
    /// assert_eq!(validation.validator, "purrassist.schema.draft2020-12.v1");
    /// assert!(!validation.passed);
    /// assert_eq!(validation.constraint, schema.digest().to_string());
    /// assert_eq!(validation.diagnostics.len(), 1);
    /// assert!(validation.diagnostics[0].starts_with(": "));
    ///
    /// let draft07 = Schema::compile(
    ///     r#"{"$schema":"http://json-schema.org/draft-07/schema#","type":"integer"}"#,
    /// ).unwrap();
    /// let report = draft07.validate(&json::parse("1").unwrap()).unwrap();
    /// assert_eq!(report.to_validation().validator, "purrassist.schema.draft-07.v1");
    /// ```
    #[must_use]
    pub fn to_validation(&self) -> crate::contract::Validation {
        crate::contract::Validation {
            validator: self.dialect.validator().into(),
            constraint: self.schema.to_string(),
            passed: self.valid,
            diagnostics: self
                .violations
                .iter()
                .map(|violation| {
                    format!(
                        "{}: {} ({})",
                        violation.instance_path, violation.message, violation.schema_path
                    )
                })
                .collect(),
        }
    }
}

impl Schema {
    /// Compile with no external resources.
    ///
    /// The root's `$schema` selects the [`Dialect`]: absent or
    /// `https://json-schema.org/draft/2020-12/schema` is Draft 2020-12;
    /// `http://json-schema.org/draft-07/schema#` (with or without the `#`)
    /// is Draft-07. The official meta-schemas of both dialects are built in.
    ///
    /// ```
    /// use purrassist_core::{json, schema::Schema};
    /// // Draft-07 `$ref` overrides its sibling `type`; Draft 2020-12 applies both.
    /// let source = |dialect: &str| format!(
    ///     r##"{{"$schema":"{dialect}","definitions":{{"n":{{"type":"number"}}}},"$ref":"#/definitions/n","type":"integer"}}"##,
    /// );
    /// let half = json::parse("1.5")?;
    /// let draft07 = Schema::compile(&source("http://json-schema.org/draft-07/schema#"))?;
    /// assert!(draft07.validate(&half)?.valid);
    /// let draft2020 = Schema::compile(&source("https://json-schema.org/draft/2020-12/schema"))?;
    /// assert!(!draft2020.validate(&half)?.valid);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    /// Rejects invalid schemas, unsupported required vocabularies, dialects
    /// that are neither built in nor supplied, and unresolved references.
    pub fn compile(source: &str) -> Result<Self, SchemaError> {
        Self::with_resources(source, &BTreeMap::new())
    }

    /// Compile with an explicit URI-to-schema resource map. Nothing is fetched implicitly.
    ///
    /// # Errors
    /// Rejects malformed resource content, conflicting identities, or invalid schemas.
    pub fn with_resources(
        source: &str,
        resources: &BTreeMap<String, String>,
    ) -> Result<Self, SchemaError> {
        Self::with_resources_governed(source, resources, &mut Governor::new(Budget::default()))
            .map_err(|error| match error {
                CompilationError::InvalidSchema(error) => error,
                CompilationError::Resource(reason) => SchemaError {
                    location: String::new(),
                    message: reason.to_string(),
                },
            })
    }

    /// Compile with a caller-owned governor shared with later validation or recovery.
    ///
    /// # Errors
    /// Distinguishes invalid schemas from sticky resource exhaustion or cancellation.
    /// No partially compiled schema is returned.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, schema::Schema, json};
    /// let mut governor = Governor::new(Budget::default());
    /// let schema = Schema::compile_governed(r#"{"type":"integer"}"#, &mut governor)?;
    /// assert!(schema.validate_with(&json::parse("42")?, &mut governor)?.valid);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn compile_governed(
        source: &str,
        governor: &mut Governor,
    ) -> Result<Self, CompilationError> {
        Self::with_resources_governed(source, &BTreeMap::new(), governor)
    }

    /// Compile root and explicit resources using one cumulative resource budget.
    ///
    /// Unused supplied resources still count toward identity hashing work. No I/O
    /// occurs. Dialect loading, reference indexing, and pattern preparation share
    /// this governor; failure leaves it stopped when a resource limit was exceeded.
    /// Bundle identity hashing uses bounded admitted updates without a framing
    /// allocation. A final governor check precedes complete schema publication;
    /// source/index/reference ownership retains its separate preparation contract.
    ///
    /// ```
    /// use std::collections::BTreeMap;
    /// use purrassist_core::{Budget, Governor, schema::Schema};
    /// let resources = BTreeMap::from([("urn:integer".into(), r#"{"type":"integer"}"#.into())]);
    /// let mut governor = Governor::new(Budget::default());
    /// let schema = Schema::with_resources_governed(
    ///     r#"{"$ref":"urn:integer"}"#, &resources, &mut governor,
    /// )?;
    /// assert!(schema.validate_with(&purrassist_core::json::parse("42")?, &mut governor)?.valid);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    /// Returns an invalid-schema error or an explicit resource/cancellation error.
    pub fn with_resources_governed(
        source: &str,
        resources: &BTreeMap<String, String>,
        governor: &mut Governor,
    ) -> Result<Self, CompilationError> {
        build::compile(source, resources, governor)
    }

    /// Root schema document, preserving source spans and annotations.
    #[must_use]
    pub fn document(&self) -> &Node {
        self.body(self.root)
    }

    /// Dialect of the root schema resource; it names the validator in
    /// [`ValidationReport::to_validation`]. Embedded resources may declare
    /// another dialect with their own `$schema` and are evaluated under it.
    ///
    /// ```
    /// use purrassist_core::schema::{Dialect, Schema};
    /// let schema = Schema::compile(r#"{"$schema":"http://json-schema.org/draft-07/schema"}"#).unwrap();
    /// assert_eq!(schema.dialect(), Dialect::Draft07);
    /// ```
    #[must_use]
    pub fn dialect(&self) -> Dialect {
        self.index.locations[self.root].vocabularies.dialect
    }

    /// Identity of this complete schema configuration.
    ///
    /// Binds exact root bytes and every supplied resource URI and byte buffer,
    /// including custom meta-schemas. Unused resources also affect identity;
    /// resource insertion order does not. Built-in dialect data is versioned.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }

    /// Every unsupported optional vocabulary this schema's
    /// dialects declared, with the keywords left unevaluated because of it.
    /// Empty when every declared vocabulary is either required-and-known
    /// (compilation would otherwise have failed) or fully supported.
    #[must_use]
    pub fn unsupported_vocabularies(&self) -> Vec<UnsupportedVocabulary> {
        self.index
            .unsupported
            .iter()
            .map(|(uri, keywords)| UnsupportedVocabulary {
                uri: uri.as_str().to_owned(),
                keywords: keywords
                    .iter()
                    .map(|(key, ())| key.as_str().to_owned())
                    .collect(),
            })
            .collect()
    }

    /// Validate using ordinary bounds.
    ///
    /// # Errors
    /// Returns resource exhaustion or an unproductive recursive reference.
    pub fn validate(&self, instance: &Node) -> Result<ValidationReport, StopReason> {
        self.validate_with(instance, &mut Governor::new(Budget::default()))
    }

    /// Validate with caller-controlled limits shared with a recovery operation.
    ///
    /// Evaluator control, annotation and report storage is admitted before
    /// allocation. `Work` covers visited schema locations and admitted
    /// operations; `Depth` bounds nesting. `ScratchBytes` covers collections
    /// at portable element sizes, member names, pattern keys, transient child
    /// keys and instance paths, and held applicator branches. Temporary owners
    /// release their storage when the evaluation drops them. The
    /// report's violations, format annotations and pattern evidence keep their
    /// exact owners until all fallible preparation and the final governor check
    /// succeed. Published output retains its charge; internal witness and
    /// recovery checks release discarded reports. General scalar/format
    /// computation retains its separate resource contract.
    ///
    /// # Errors
    /// Governor failure is distinct from an instance failing its schema:
    /// exhaustion or cancellation is an `Err` naming the stop, never a
    /// report.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, StopReason, contract::Resource, json, schema::Schema};
    /// let schema = Schema::compile(r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#).unwrap();
    /// let instance = json::parse(r#"{"path":"a.txt"}"#).unwrap();
    /// let mut governor = Governor::new(Budget::default());
    /// assert!(schema.validate_with(&instance, &mut governor).unwrap().valid);
    /// let working_set = governor.usage().peak_scratch_bytes;
    /// assert!(working_set > 0);
    /// let mut starved = Governor::new(Budget { scratch_bytes: working_set - 1, ..Budget::default() });
    /// assert_eq!(
    ///     schema.validate_with(&instance, &mut starved).map(|report| report.valid),
    ///     Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    /// );
    /// ```
    pub fn validate_with(
        &self,
        instance: &Node,
        governor: &mut Governor,
    ) -> Result<ValidationReport, StopReason> {
        evaluate::run(self, self.root, instance, governor)?.publish(self, governor)
    }

    fn keywords(&self, id: usize) -> Keywords<'_> {
        Keywords {
            node: self.body(id),
            vocabularies: self.index.locations[id].vocabularies,
        }
    }
    fn body(&self, id: usize) -> &Node {
        at_route(
            self.documents[self.index.locations[id].document].root(),
            &self.index.locations[id].route,
        )
    }
    fn child(&self, id: usize, key: &str) -> usize {
        self.index.locations[id].children[key]
    }
}
/// Identity of a compiled bundle. The prefix names the built-in dialect data
/// the bundle can observe: the Draft 2020-12 meta-schemas always, and the
/// Draft-07 meta-schema (with the dialect's rules) only when the root is
/// Draft-07 or the built-in Draft-07 meta-schema was loaded, so every bundle
/// that compiled before Draft-07 support keeps its exact identity.
fn bundle_digest(
    root: Digest,
    resources: &BTreeMap<String, String>,
    draft07: bool,
    governor: &mut Governor,
) -> Result<Digest, StopReason> {
    let mut identity = DigestBuilder::new();
    hash_chunks(
        &mut identity,
        b"purrassist.schema.bundle.v1\0draft2020-12.601a66c8b0f25246bf0e1fb488c5b5f030a79b72\0",
        governor,
    )?;
    if draft07 {
        hash_chunks(
            &mut identity,
            b"draft-07.20a3fee852db88519dcb3bb329b7a872ebad8953\0",
            governor,
        )?;
    }
    hash_chunks(&mut identity, root.as_bytes(), governor)?;
    for (uri, document) in resources {
        // Fixed-width hashes preserve the existing unambiguous identity protocol.
        for bytes in [uri.as_bytes(), document.as_bytes()] {
            let digest = hash_schema_bytes(bytes, governor)?;
            hash_chunks(&mut identity, digest.as_bytes(), governor)?;
        }
    }
    governor.charge(crate::contract::Resource::Work, 128)?;
    Ok(identity.finish())
}

/// Hash original schema/resource bytes without retaining a framing allocation.
fn hash_schema_bytes(bytes: &[u8], governor: &mut Governor) -> Result<Digest, StopReason> {
    let mut hash = DigestBuilder::new();
    hash_chunks(&mut hash, bytes, governor)?;
    // SHA-256 finalization processes at most two 64-byte padding blocks.
    governor.charge(crate::contract::Resource::Work, 128)?;
    Ok(hash.finish())
}

/// Bound cancellation latency while admitting bytes and per-chunk control work.
fn hash_chunks(
    hash: &mut DigestBuilder,
    bytes: &[u8],
    governor: &mut Governor,
) -> Result<(), StopReason> {
    for chunk in bytes.chunks(4096) {
        governor.charge(
            crate::contract::Resource::Work,
            crate::contract::saturating_size(chunk.len()).saturating_add(1),
        )?;
        hash.update(chunk);
    }
    Ok(())
}

fn at_route<'a>(mut node: &'a Node, route: &[usize]) -> &'a Node {
    for &index in route {
        node = match &node.value {
            Value::Object(members) => &members[index].value,
            Value::Array(items) => &items[index],
            _ => unreachable!("compiled route"),
        };
    }
    node
}
