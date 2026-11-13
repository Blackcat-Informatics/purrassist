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
//! The caller's [`Governor`] bounds the whole load. The parser and the `FnO`
//! reader charge their live storage to the governor's scratch meter and poll its
//! work and cancellation authority at their native counting points; this crate's
//! own passes charge one unit of work per statement and per validated property.
//! The native steps are finer than a work unit: the loader charges one work unit
//! per 64 native units, carrying the remainder, and polls cancellation on every
//! native step. Measured with a governor's `usage()` after a load, on the worked example (9,518 bytes) a load charges 33,191 work units (3.49 per
//! source byte) and peaks at 569,829 scratch bytes (59.9 per source byte). On the
//! bulk document (the worked example body repeated 114 times under distinct
//! prefixes, names and blank-node labels; 1,049,171 bytes) it charges 4,084,594 work
//! units (3.89 per source byte) and peaks at 66,933,394 scratch bytes (63.8 per
//! source byte). The sizes scale with document shape. Each level has the same
//! scratch limit (64 MiB), so whichever of work and scratch is reached first limits
//! the admitted size, and a larger document is the explicit `Exhausted` outcome.
//! Sizes are measured on the bulk document to within one repetition (about 9,100
//! bytes); the method and table are in `docs/tool-graph/TOOLS.md`.
//!
//! | Effort | Work budget | Admitted Turtle size (measured) | Binding resource |
//! | --- | --- | --- | --- |
//! | `Exact` | 100,000 | 27,648 bytes (about 27.6 KB) | work |
//! | `Quick` | 250,000 | 63,724 bytes (about 63.7 KB) | work |
//! | `Standard` | 1,000,000 | 265,429 bytes (about 265.4 KB) | work |
//! | `Deep` | 10,000,000 | 1,049,171 bytes (about 1.05 MB) | scratch |
//! | `Maximum` | 50,000,000 | 1,049,171 bytes (about 1.05 MB) | scratch |
//!
//! The worked example loads at every level; `Deep` and `Maximum` admit the same size
//! because the scratch limit is shared.
//!
//! An exhausted work or scratch budget the governor recorded is
//! [`LoadError::Exhausted`] and a cancellation is [`LoadError::Cancelled`],
//! whichever stage observed it; neither is reported as invalid syntax, and no
//! partial graph escapes. A native storage failure the governor did not record (an
//! allocation or layout limit) is not claimed as a budget: it is a
//! [`LoadError::Parse`] with code `storage` and the native cause, and a rejected
//! scratch release is a [`LoadError::Parse`] with code `storage-accounting`. The storage a catalog holds
//! stays charged to the governor's scratch meter until the [`ToolGraph`] is dropped.
//!
//! # Stated limits
//!
//! * Only Turtle and `TriG` load: `PurRDF` 4.0.2 governs and retains source
//!   occurrences for those formats alone, so another media type is a
//!   [`LoadError::Parse`] with code `unsupported-format`.
//! * The owned rows this crate materialises for validation, their per-subject
//!   index, the `FnO` view dataset built for the catalog reader, and the owned
//!   [`RdfDataset`] copy returned by [`ToolGraph::dataset`] (`PurRDF` offers no
//!   governed detach of a retained parse), are allocated outside the governor's
//!   storage account; their size is proportional to the document and bounded by
//!   the input-byte limit and the per-statement work charge.
//! * [`SourceExtent`] names the first statement in source order whose subject and
//!   predicate (and term) match the refusal. Refusals that name a node rather than
//!   one statement (shape, hint, profile, `FnO` binding) carry subject and
//!   predicate only. A term the grammar generates without source spelling is
//!   located by the syntax that introduced it.
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
    /// `governor` bounds the parser, the `FnO` reader and this crate's own passes
    /// and carries cancellation (see the module's governance notes).
    ///
    /// # Errors
    /// Returns [`LoadError`] for the first parse failure (including a media type
    /// the governed parser does not support, code `unsupported-format`), out-of-vocabulary term, unowned
    /// statement, violated vocabulary shape, invalid hint, execution profile or
    /// `FnO` binding, unsupported context projection, exhausted budget or
    /// cancellation. A work or scratch budget the governor recorded as exhausted by
    /// the parser or the `FnO` reader is [`LoadError::Exhausted`], and a
    /// cancellation observed there is [`LoadError::Cancelled`]. A native storage
    /// failure the governor did not record is a [`LoadError::Parse`] (code `storage`
    /// or `storage-accounting`) carrying the native cause, not a budget claim. No
    /// partial graph escapes.
    ///
    /// The Turtle size each effort level admits, measured, is in the module's
    /// governance notes.
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
