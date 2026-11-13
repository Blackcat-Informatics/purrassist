// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Owned typed values produced by a successful load.

use crate::{ContextManagement, ResourceLimits, codec::mcp::ExecutionMode};

/// One callable, security- and context-bound tool.
///
/// Every field is owned data copied from the admitted graph; the `iri` fields
/// identify the source nodes in [`ToolGraph::dataset`](super::ToolGraph::dataset).
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tool {
    /// The tool's `FnO` function IRI.
    pub iri: String,
    /// The unique core tool name.
    pub name: String,
    /// The model-visible description.
    pub description: String,
    /// MCP annotations with their original presence.
    pub annotations: ToolAnnotations,
    /// The complete argument shape node.
    pub input_shape: String,
    /// The complete result shape node.
    pub output_shape: String,
    /// The `FnO` implementation bound by the tool's unique mapping.
    pub implementation: ToolImplementation,
    /// Parameters in declared `fno:expects` order.
    pub parameters: Vec<ToolParameter>,
    /// The single declared output.
    pub output: ToolOutput,
    /// The confinement contract.
    pub execution_profile: ToolExecutionProfile,
    /// Output projection and prompt-loading contract.
    pub context: ContextManagement,
    /// The tool's policy document, verbatim and unevaluated.
    pub parameter_constraints: String,
    /// Declared nested-call edges by tool IRI, in lexical order.
    pub subtools: Vec<String>,
}

/// MCP annotations; `None` records absence in the source graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolAnnotations {
    /// `annotations.title`, verbatim.
    pub title: Option<String>,
    /// `annotations.readOnlyHint` as written.
    pub read_only_hint: Option<bool>,
    /// `annotations.destructiveHint` as written.
    pub destructive_hint: Option<bool>,
    /// `annotations.idempotentHint` as written.
    pub idempotent_hint: Option<bool>,
    /// `annotations.openWorldHint` as written.
    pub open_world_hint: Option<bool>,
}

impl ToolAnnotations {
    /// The effective read-only hint under MCP defaults (`false`). A hint, never trust.
    #[must_use]
    pub fn effective_read_only(&self) -> bool {
        self.read_only_hint.unwrap_or(false)
    }

    /// The effective destructive hint under MCP defaults (`true`).
    #[must_use]
    pub fn effective_destructive(&self) -> bool {
        self.destructive_hint.unwrap_or(true)
    }

    /// The effective idempotent hint under MCP defaults (`false`).
    #[must_use]
    pub fn effective_idempotent(&self) -> bool {
        self.idempotent_hint.unwrap_or(false)
    }

    /// The effective open-world hint under MCP defaults (`true`).
    #[must_use]
    pub fn effective_open_world(&self) -> bool {
        self.open_world_hint.unwrap_or(true)
    }
}

/// The host implementation a tool's `FnO` mapping selects.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolImplementation {
    /// The implementation node.
    pub iri: String,
    /// Its `dcterms:format` media type.
    pub format: String,
    /// Its `rdfs:seeAlso` target.
    pub see_also: String,
}

/// One declared parameter with its implementation property and optional hint.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolParameter {
    /// The `FnO` parameter node.
    pub iri: String,
    /// Its RDF predicate, which names the input shape property.
    pub predicate: String,
    /// Its declared datatype IRI.
    pub datatype: String,
    /// Whether `fno:required` is true.
    pub required: bool,
    /// The unique implementation (JSON) property.
    pub implementation_property: String,
    /// The reified hint attached to this parameter's shape path, if any.
    pub hint: Option<Hint>,
}

/// The declared output with its implementation property.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolOutput {
    /// The `FnO` output node.
    pub iri: String,
    /// Its RDF predicate, which names the output shape property.
    pub predicate: String,
    /// Its declared datatype IRI.
    pub datatype: String,
    /// The unique implementation (JSON) property.
    pub implementation_property: String,
}

/// An allowed conversion a recovery may apply to an argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Coercion {
    /// `string-to-integer`.
    StringToInteger,
    /// `integer-to-string`.
    IntegerToString,
}

/// A reified parameter hint read from the reifier side table.
///
/// The reifier annotates the triple `(property shape, sh:path, parameter predicate)`.
/// Only default-graph reifiers load; a named-graph reifier is refused.
/// Hints are untrusted descriptions of permitted recovery, never authority.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hint {
    /// The reifier node carrying the hint.
    pub reifier: String,
    /// The property shape whose path this hint annotates.
    pub shape: String,
    /// Maximum admitted edit-distance inference; zero forbids it.
    pub max_edit_distance: u64,
    /// Explicit parameter-name aliases, in lexical order.
    pub aliases: Vec<String>,
    /// Allowed conversions; empty is the explicit `none`.
    pub coercions: Vec<Coercion>,
    /// Permission for labeled case-folding inferences.
    pub case_fold: bool,
    /// Permission for labeled Unicode-folding inferences.
    pub unicode_fold: bool,
    /// Taint labels, in lexical order.
    pub taints: Vec<String>,
    /// The trusted session predicate the argument must equal, if declared.
    pub session_binding: Option<String>,
}

/// A rule node of an execution profile, kept as vocabulary-checked data.
///
/// Loading verifies the rule's class, predicates and value kinds. Interpreting
/// globs, access sets and transitions stays with the policy components.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProfileRule {
    /// The rule node.
    pub iri: String,
    /// The concrete vocabulary class, such as `FileRule`.
    pub class: &'static str,
    /// `(vocabulary term name, value)` pairs in lexical order; values are lexical
    /// forms, or IRIs for node-valued properties.
    pub properties: Vec<(&'static str, String)>,
}

/// A waiver node covering one unconfined exec rule. Self-asserted: no authority.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Waiver {
    /// The waiver node.
    pub iri: String,
    /// The covered file rule.
    pub waived_rule: String,
    /// The claimed reviewer.
    pub reviewer: String,
    /// The nonempty rationale.
    pub rationale: String,
    /// The claimed approval source.
    pub source: String,
}

/// One confinement contract; loading proves it well formed, not available or trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolExecutionProfile {
    /// The profile node.
    pub iri: String,
    /// The explicitly selected confinement mode.
    pub mode: ExecutionMode,
    /// The exact standalone profile name.
    pub profile_name: String,
    /// The exact hat name.
    pub hat_name: String,
    /// Whether the profile may grant outbound network access.
    pub egress_allowed: bool,
    /// The five exact positive resource limits.
    pub limits: ResourceLimits,
    /// Granular rules in lexical IRI order.
    pub rules: Vec<ProfileRule>,
    /// Nested child profiles in lexical IRI order; cycles are refused.
    pub subprofiles: Vec<Self>,
    /// Waivers in lexical IRI order.
    pub waivers: Vec<Waiver>,
}
