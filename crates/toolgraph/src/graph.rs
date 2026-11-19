// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Typed, all-or-nothing loading of an RDF 1.2 tool graph.
//!
//! [`ToolGraph::load`] parses Turtle or `TriG` with the governed
//! `purrdf_rdf::parse_dataset_controlled` (exact ordered source occurrences
//! retained), partitions every statement into an owned view (the strict `FnO`
//! catalog, built with `FnoCatalog::from_dataset_with_control`; the vocabulary
//! nodes, the SHACL shapes, the reifier side table), and returns typed [`Tool`]
//! values. A statement that no view owns, a term outside the caller's
//! [`Vocabulary`], and a violated shape are each a [`LoadError`] naming the
//! subject and predicate; the statement-level refusals also carry the exact
//! [`SourceExtent`]. Nothing is skipped.
//!
//! # Governance
//!
//! The parser, owned statement rows, role and ownership indexes, temporary
//! `FnO` dataset builder, and catalog reader share the caller's [`Governor`].
//! Each native account admits its live storage
//! through the scratch meter and polls cancellation on every native step.
//! It charges one work unit per 64 native units, carrying the remainder
//! across charges. Vocabulary validation scans the existing subject rows in
//! place and charges for row visits, predicate comparisons, and variable-length
//! lexical checks. The temporary dataset stays charged through catalog reading.
//! The rows and classification tables stay charged through validation and typing.
//!
//! An exhausted work or scratch budget the governor recorded is
//! [`LoadError::Exhausted`] and a cancellation is [`LoadError::Cancelled`],
//! in any governed stage. Both outcomes withhold the graph. Native allocation
//! and layout failures preserve the cause as [`LoadError::Parse`] with code
//! `storage` during parsing, classification or catalog reading, and as
//! [`LoadError::InvalidFnoBinding`] during temporary view construction. A
//! rejected scratch release is [`LoadError::Parse`] with code `storage-accounting`.
//! The catalog's own storage stays charged while the [`ToolGraph`] holds it.
//!
//! # Stated limits
//!
//! * Only Turtle and `TriG` load: `PurRDF` 4.0.2 governs and retains source
//!   occurrences for those formats alone, so another media type is a
//!   [`LoadError::Parse`] with code `unsupported-format`.
//! * Typed tool construction still allocates outside the scratch account.
//!   The published [`RdfDataset`] copy returned by
//!   [`ToolGraph::dataset`] is also outside that account. `PurRDF` exposes a
//!   borrowed view of the retained parse.
//! * For a diagnostic that identifies a specific row or reifier binding,
//!   [`SourceExtent`] names the first written occurrence of that RDF statement.
//!   Matching preserves blank scopes and literal facets. Refusals that identify
//!   only a node, including missing `FnO` facts, carry no statement extent.
//!   The syntax that introduces a generated term supplies its location.
//! * Only default-graph statements and reifiers load; a named-graph one is refused.
//! * The vocabulary shapes are checked by a Rust table that is not equivalent to
//!   the shapes document; see `docs/tool-graph/TOOLS.md` for the differences.
//!
//! The runtime cost of a load is one governed parse, one pass over every statement
//! to materialise owned rows (memory is proportional to the document), one `FnO`
//! catalog construction and one owned dataset copy.

mod load;
mod model;
mod problem;
mod tool_set;

pub use tool_set::ToolSetBridgeError;

pub use model::{
    Coercion, Hint, ProfileRule, Tool, ToolAnnotations, ToolExecutionProfile, ToolImplementation,
    ToolOutput, ToolParameter, Waiver,
};
pub use problem::{Construct, LoadError, SourceExtent, Violation};

use std::sync::Arc;

use purrassist_core::Governor;
use purrdf_core::{RdfDataset, fno::FnoCatalog};

use crate::Vocabulary;

/// A fully loaded tool graph: typed tools over a dataset re-interned from the parse.
///
/// A value exists only when every statement was owned and every shape held.
/// Tools are ordered by their IRI. The [`RdfDataset`] is a copy re-interned from
/// the retained parse (not the parser's own dataset), holding every parsed
/// statement, and the strict `FnO` catalog stays available, so later projections
/// consult the source statements rather than a lossy rebuild.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{LoadError, ToolGraph};
/// # use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
/// # use purrdf_core::loss::LossProfile;
/// # let namespace = "https://example.org/toolgraph#";
/// # let vocabulary = Vocabulary::new("pa", PrefixTable(vec![("pa".into(), namespace.into())]),
/// #     TermBindings { namespace_iri: namespace.into(), local_names: TERM_NAMES.iter()
/// #         .map(|name| ((*name).into(), format!("{namespace}{name}"))).collect() },
/// #     register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// // An empty document has no tools; a statement outside the vocabulary is refused.
/// let empty = ToolGraph::load(b"", "text/turtle", None, &vocabulary, &mut governor).unwrap();
/// assert!(empty.tools().is_empty());
/// let source = b"@prefix pa: <https://example.org/toolgraph#> . <urn:t> pa:unbound 1 .";
/// let refused = ToolGraph::load(source, "text/turtle", None, &vocabulary, &mut governor);
/// assert!(matches!(refused, Err(LoadError::UnknownTerm { .. })));
/// ```
#[derive(Debug)]
pub struct ToolGraph {
    tools: Vec<Tool>,
    dataset: Arc<RdfDataset>,
    catalog: Option<FnoCatalog>,
}

impl ToolGraph {
    /// Load RDF text into typed tools, all or nothing.
    ///
    /// `media_type` selects the `PurRDF` text codec (`text/turtle` or
    /// `application/trig`) and `base` is the optional document base IRI.
    /// `governor` supplies the parser, temporary `FnO` view, catalog reader, and
    /// loader passes with their work and cancellation authority.
    ///
    /// # Errors
    /// Returns [`LoadError`] for the first parse failure (including a media type
    /// the governed parser does not support, code `unsupported-format`), out-of-vocabulary term, unowned
    /// statement, violated vocabulary shape, invalid hint, execution profile or
    /// `FnO` binding, unsupported context projection, exhausted budget or
    /// cancellation. Resource exhaustion is [`LoadError::Exhausted`]. Cancellation
    /// is [`LoadError::Cancelled`]. Native storage failures retain the original
    /// cause under the stage's error category. Every refusal withholds the graph.
    pub fn load(
        bytes: &[u8],
        media_type: &str,
        base: Option<&str>,
        vocabulary: &Vocabulary,
        governor: &mut Governor,
    ) -> Result<Self, LoadError> {
        load::load(bytes, media_type, base, vocabulary, governor)
    }

    /// The loaded tools in IRI order.
    #[must_use]
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// Find a tool by its unique `name` property.
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    /// The dataset re-interned from the parse (a copy, not the parser's own
    /// dataset), including reifiers and every unprojected statement.
    #[must_use]
    pub const fn dataset(&self) -> &Arc<RdfDataset> {
        &self.dataset
    }

    /// The strict `FnO` catalog read from the owned `FnO` view; `None` only when the
    /// document declares no catalog at all.
    #[must_use]
    pub const fn catalog(&self) -> Option<&FnoCatalog> {
        self.catalog.as_ref()
    }
}
