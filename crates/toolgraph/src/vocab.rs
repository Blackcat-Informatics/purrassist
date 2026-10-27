// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Explicit vocabulary ownership, validated before namespace compaction.

mod profile;
pub use profile::{
    LOSS_PROFILE_ID, ModelFamily, ProjectionTarget, SOURCE_REPRESENTATION, register_loss_profile,
};

use std::fmt;

use purrdf_core::{loss::LossProfile, parse_iri, terminals};
use purrdf_shapes::json_schema::Namespaces;

/// Complete versioned term set from the tool-graph vocabulary.
///
/// These are local identifiers, not namespace IRIs. The [`Vocabulary`] example
/// constructs every term under a caller-selected namespace.
pub const TERM_NAMES: &[&str] = &[
    "Call",
    "CapabilityRule",
    "Context",
    "ExecutionProfile",
    "FileRule",
    "Hint",
    "MountRule",
    "NetworkRule",
    "Node",
    "PtraceRule",
    "Rule",
    "Session",
    "SignalRule",
    "Tool",
    "UnixRule",
    "Waiver",
    "access",
    "active",
    "address",
    "addressSpaceBytes",
    "alias",
    "audit",
    "candidateAllowed",
    "candidateDenied",
    "capability",
    "caseFold",
    "confinementMode",
    "conflict",
    "contextManagement",
    "cpuSeconds",
    "customerId",
    "denies",
    "deny",
    "description",
    "destructiveHint",
    "domain",
    "egressAllowed",
    "execTransition",
    "executionProfile",
    "filePermissions",
    "fileSizeBytes",
    "hatName",
    "historyRetentionTurns",
    "idempotentHint",
    "inputShape",
    "intentTags",
    "loadingStrategy",
    "localIp",
    "localPort",
    "maxEditDistance",
    "maxOutputTokens",
    "mountOperation",
    "name",
    "openFileCount",
    "openWorldHint",
    "outputProjectionJq",
    "outputShape",
    "owner",
    "parameterConstraints",
    "pathGlob",
    "peerAddress",
    "peerIp",
    "peerLabel",
    "peerPort",
    "permits",
    "processCount",
    "profileName",
    "protocol",
    "rationale",
    "readOnlyHint",
    "reviewer",
    "rule",
    "session",
    "sessionBinding",
    "signalSet",
    "socketType",
    "subprofile",
    "subtool",
    "taint",
    "tenant",
    "title",
    "tool",
    "transitionTarget",
    "typeCoercion",
    "unicodeFold",
    "userId",
    "waivedRule",
    "waiver",
    "waiverSource",
];

/// Raw caller-declared prefix bindings, checked before upstream deduplication.
///
/// See [`Vocabulary`] for an executable construction example. Empty prefixes
/// are valid Turtle default prefixes; nonempty prefixes follow `PN_PREFIX`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixTable(pub Vec<(String, String)>);

/// Exact term meanings under one explicitly caller-owned namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermBindings {
    /// Absolute namespace IRI; it must match the selected primary prefix.
    pub namespace_iri: String,
    /// Every [`TERM_NAMES`] identifier mapped to a distinct absolute IRI
    /// beginning with `namespace_iri`, with a nonempty suffix.
    pub local_names: Vec<(String, String)>,
}

/// Which raw input contains an invalid binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingSection {
    /// The caller-owned namespace IRI.
    Namespace,
    /// A raw prefix declaration.
    Prefix,
    /// A raw local-term declaration.
    Term,
}

/// Why a raw vocabulary binding cannot be admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingProblem {
    /// An IRI is malformed or lacks an absolute scheme.
    InvalidAbsoluteIri,
    /// A prefix does not satisfy the Turtle prefix grammar.
    InvalidPrefix,
    /// A prefix or term key was declared more than once.
    Duplicate,
    /// The primary prefix is missing or names a different namespace.
    PrimaryMismatch,
    /// A term key does not belong to this version's vocabulary.
    UnknownTerm,
    /// A term IRI does not belong to the caller-owned namespace.
    ForeignTerm,
    /// Two term keys name the same IRI.
    AliasedTerm,
}

/// An explicit vocabulary admission failure; raw binding indices are zero-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VocabularyError {
    /// A raw binding violates the vocabulary's ownership or identity contract.
    Binding {
        /// Input collection containing the failure.
        section: BindingSection,
        /// Original input index, or `None` for a namespace/primary-prefix failure.
        index: Option<usize>,
        /// Actionable reason for refusing the binding.
        problem: BindingProblem,
    },
    /// One required local identifier is absent.
    MissingTerm {
        /// Required identifier from [`TERM_NAMES`].
        name: &'static str,
    },
    /// A required loss meaning is missing or differs from this profile version.
    LossMeaning {
        /// Exact target representation.
        target: &'static str,
        /// Required loss code.
        code: &'static str,
    },
    /// Upstream namespace construction refused an already checked table.
    NamespaceConstruction(String),
}

impl fmt::Display for VocabularyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding {
                section,
                index,
                problem,
            } => write!(f, "invalid {section:?} binding {index:?}: {problem:?}"),
            Self::MissingTerm { name } => write!(f, "required vocabulary term {name:?} is missing"),
            Self::LossMeaning { target, code } => write!(
                f,
                "loss meaning {code:?} for {target:?} is missing or conflicting"
            ),
            Self::NamespaceConstruction(reason) => write!(f, "namespace table refused: {reason}"),
        }
    }
}

impl std::error::Error for VocabularyError {}

/// A validated complete caller-owned vocabulary, with one shared namespace table.
///
/// The constructor receives raw prefixes rather than an opaque precompiled
/// namespace table: upstream namespace construction silently deduplicates keys.
/// No default vocabulary or implicit namespace is provided.
///
/// ```
/// use purrdf_core::loss::LossProfile;
/// use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
/// let namespace = "https://example.org/my-tools#";
/// let terms = TermBindings {
///     namespace_iri: namespace.into(),
///     local_names: TERM_NAMES.iter().map(|name| ((*name).into(), format!("{namespace}{name}"))).collect(),
/// };
/// let vocabulary = Vocabulary::new("tools", PrefixTable(vec![("tools".into(), namespace.into())]),
///     terms, register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
/// assert_eq!(vocabulary.term("Tool"), Some("https://example.org/my-tools#Tool"));
/// assert_eq!(vocabulary.namespaces().primary_ns(), namespace);
/// ```
#[derive(Clone, Debug)]
pub struct Vocabulary {
    namespaces: Namespaces,
    prefixes: PrefixTable,
    terms: TermBindings,
    losses: LossProfile,
}

impl Vocabulary {
    /// Validate raw inputs, then construct the single upstream namespace table.
    ///
    /// Ownership of supplied strings/profile moves into the vocabulary. Stored
    /// prefixes and term keys use canonical lexical order; term meanings and
    /// the supplied profile are retained. See the type example for construction.
    ///
    /// # Errors
    /// Returns [`VocabularyError`] for invalid, duplicate, unknown, missing or
    /// foreign bindings, mismatched primary namespace or incomplete loss meanings.
    pub fn new(
        primary_prefix: &str,
        mut prefixes: PrefixTable,
        mut terms: TermBindings,
        losses: LossProfile,
    ) -> Result<Self, VocabularyError> {
        if !absolute(&terms.namespace_iri) {
            return Err(binding(
                BindingSection::Namespace,
                None,
                BindingProblem::InvalidAbsoluteIri,
            ));
        }
        validate_prefixes(primary_prefix, &prefixes, &terms)?;
        validate_terms(&terms)?;
        for definition in profile::definitions() {
            if losses.description(&definition.from, &definition.to, &definition.code)
                != Some(definition.description.as_ref())
            {
                // Every required definition borrows versioned static strings.
                let (std::borrow::Cow::Borrowed(target), std::borrow::Cow::Borrowed(code)) =
                    (definition.to, definition.code)
                else {
                    unreachable!("required profile definitions are static");
                };
                return Err(VocabularyError::LossMeaning { target, code });
            }
        }
        prefixes
            .0
            .sort_unstable_by(|left, right| left.0.cmp(&right.0));
        terms
            .local_names
            .sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let namespaces = Namespaces::new(primary_prefix, &prefixes.0)
            .map_err(VocabularyError::NamespaceConstruction)?;
        Ok(Self {
            namespaces,
            prefixes,
            terms,
            losses,
        })
    }

    /// Resolve a versioned local identifier to its exact caller-supplied IRI.
    /// See the type example for a lookup and namespace-table check.
    #[must_use]
    pub fn term(&self, name: &str) -> Option<&str> {
        self.terms
            .local_names
            .binary_search_by(|(key, _)| key.as_str().cmp(name))
            .ok()
            .map(|index| self.terms.local_names[index].1.as_str())
    }

    /// The single upstream namespace table shared by future projections.
    #[must_use]
    pub const fn namespaces(&self) -> &Namespaces {
        &self.namespaces
    }

    /// Caller-declared prefixes in lexical order, without injected builtin rows.
    #[must_use]
    pub const fn prefixes(&self) -> &PrefixTable {
        &self.prefixes
    }

    /// Complete term bindings in lexical local-identifier order.
    #[must_use]
    pub const fn terms(&self) -> &TermBindings {
        &self.terms
    }

    /// The explicit caller profile, validated against every required meaning.
    #[must_use]
    pub const fn loss_profile(&self) -> &LossProfile {
        &self.losses
    }
}

const fn binding(
    section: BindingSection,
    index: Option<usize>,
    problem: BindingProblem,
) -> VocabularyError {
    VocabularyError::Binding {
        section,
        index,
        problem,
    }
}

fn absolute(text: &str) -> bool {
    parse_iri(text).is_ok_and(|iri| iri.has_scheme())
}

fn valid_prefix(prefix: &str) -> bool {
    let mut chars = prefix.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    terminals::is_pn_chars_base(first)
        && !prefix.ends_with('.')
        && chars.all(|c| c == '.' || terminals::is_pn_chars(c))
}

fn validate_prefixes(
    primary_prefix: &str,
    prefixes: &PrefixTable,
    terms: &TermBindings,
) -> Result<(), VocabularyError> {
    for (index, (prefix, namespace)) in prefixes.0.iter().enumerate() {
        if !valid_prefix(prefix) {
            return Err(binding(
                BindingSection::Prefix,
                Some(index),
                BindingProblem::InvalidPrefix,
            ));
        }
        if !absolute(namespace) {
            return Err(binding(
                BindingSection::Prefix,
                Some(index),
                BindingProblem::InvalidAbsoluteIri,
            ));
        }
        if prefixes.0[..index]
            .iter()
            .any(|(previous, _)| previous == prefix)
        {
            return Err(binding(
                BindingSection::Prefix,
                Some(index),
                BindingProblem::Duplicate,
            ));
        }
    }
    if !prefixes
        .0
        .iter()
        .any(|(prefix, namespace)| prefix == primary_prefix && namespace == &terms.namespace_iri)
    {
        return Err(binding(
            BindingSection::Namespace,
            None,
            BindingProblem::PrimaryMismatch,
        ));
    }
    Ok(())
}

fn validate_terms(terms: &TermBindings) -> Result<(), VocabularyError> {
    for (index, (name, iri)) in terms.local_names.iter().enumerate() {
        if TERM_NAMES.binary_search(&name.as_str()).is_err() {
            return Err(binding(
                BindingSection::Term,
                Some(index),
                BindingProblem::UnknownTerm,
            ));
        }
        if !absolute(iri) {
            return Err(binding(
                BindingSection::Term,
                Some(index),
                BindingProblem::InvalidAbsoluteIri,
            ));
        }
        if !iri.starts_with(&terms.namespace_iri) || iri.len() == terms.namespace_iri.len() {
            return Err(binding(
                BindingSection::Term,
                Some(index),
                BindingProblem::ForeignTerm,
            ));
        }
        for (previous, previous_iri) in &terms.local_names[..index] {
            if previous == name {
                return Err(binding(
                    BindingSection::Term,
                    Some(index),
                    BindingProblem::Duplicate,
                ));
            }
            if previous_iri == iri {
                return Err(binding(
                    BindingSection::Term,
                    Some(index),
                    BindingProblem::AliasedTerm,
                ));
            }
        }
    }
    for name in TERM_NAMES {
        if !terms.local_names.iter().any(|(given, _)| given == name) {
            return Err(VocabularyError::MissingTerm { name });
        }
    }
    Ok(())
}
