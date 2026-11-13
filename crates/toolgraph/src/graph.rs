// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Typed, all-or-nothing loading of an RDF 1.2 tool graph.
//!
//! [`ToolGraph::load`] parses RDF text with `purrdf_rdf::parse_dataset`, partitions
//! every statement into an owned view (the strict `FnO` catalog, the vocabulary
//! nodes, the SHACL shapes, the reifier side table), and returns typed [`Tool`]
//! values. A statement that no view owns, a term outside the caller's
//! [`Vocabulary`], and a violated shape are each a [`LoadError`] naming the
//! subject and predicate. Nothing is skipped.
//!
//! # Stated limits
//!
//! * This delivery uses the ungoverned entry points `parse_dataset` and
//!   `FnoCatalog::from_dataset`. The pinned `PurRDF` 4.0.2 also provides the
//!   governed `FnoCatalog::from_dataset_with_control` and
//!   `purrdf_rdf::parse_dataset_controlled` (exact ordered source occurrences);
//!   adopting them is the next step. Until then the caller's [`Governor`] meters
//!   only this crate's own row accounting (input bytes, one unit of work per
//!   statement and per validated property), not the parser's or reader's
//!   allocations, and errors name subject and predicate rather than an exact
//!   per-quad span. [`LoadError::Parse`] carries the parser's own line and column.
//! * Only default-graph statements and reifiers load; a named-graph one is refused.
//! * The vocabulary shapes are checked by a Rust table that is not equivalent to
//!   the shapes document; see `docs/tool-graph/TOOLS.md` for the differences.
//!
//! The runtime cost of a load is one parse, one pass over every statement to
//! materialise owned rows (memory is proportional to the document), and one
//! `FnO` catalog construction.

mod load;
mod model;
mod problem;
mod tool_set;

pub use tool_set::ToolSetBridgeError;

pub use model::{
    Coercion, Hint, ProfileRule, Tool, ToolAnnotations, ToolExecutionProfile, ToolImplementation,
    ToolOutput, ToolParameter, Waiver,
};
pub use problem::{Construct, LoadError, Violation};

use std::sync::Arc;

use purrassist_core::Governor;
use purrdf_core::{RdfDataset, fno::FnoCatalog};

use crate::Vocabulary;

/// A fully loaded tool graph: typed tools over the original parsed dataset.
///
/// A value exists only when every statement was owned and every shape held.
/// Tools are ordered by their IRI. The original [`RdfDataset`] and the strict
/// `FnO` catalog stay available, so later projections consult the source rather
/// than a lossy rebuild.
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
    /// `media_type` selects the `PurRDF` text codec (for example `text/turtle`)
    /// and `base` is the optional document base IRI. `governor` meters this
    /// crate's own accounting and carries cancellation; it does not govern the
    /// upstream parser or `FnO` reader (see the module's stated limits).
    ///
    /// # Errors
    /// Returns [`LoadError`] for the first parse failure, out-of-vocabulary term,
    /// unowned statement, violated vocabulary shape, invalid hint, execution
    /// profile or `FnO` binding, unsupported context projection, exhausted budget
    /// or cancellation. No partial graph escapes.
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

    /// The original parsed dataset, including reifiers and every unprojected statement.
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
