// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The bridge from a loaded [`ToolGraph`] to the core recovery engine.
//!
//! [`ToolGraph::tool_set`] encodes the graph as its MCP Extended manifest and
//! imports that exact text through the core MCP adapter with one
//! `DeclarationMetadata` record per tool. The adapter keeps the exact input
//! schema and the original declaration bytes, so recovery validates against the
//! schema the compiler emitted and the declaration renders back unchanged; no
//! typed subset of the schema is rebuilt here.

use std::collections::BTreeSet;
use std::fmt;

use purrassist_core::{
    Governor,
    tool_call::{
        DeclarationMetadata, Effects, ParameterAliases, RepairPolicy, ToolSet, ToolSetError,
    },
};
use purrdf_core::loss::LossLedger;

use super::{Tool, ToolExecutionProfile, ToolGraph};
use crate::{Vocabulary, codec::mcp};

/// A refusal to build the core tool set for a loaded graph. Nothing partial escapes.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolSetBridgeError {
    /// The graph has no MCP Extended manifest projection.
    Manifest(Box<mcp::CodecError>),
    /// The core adapter refused the declarations or their metadata. An alias that
    /// collides with a sibling parameter or alias is [`ToolSetError::Collision`];
    /// an alias path the schema cannot carry is [`ToolSetError::InvalidDefinition`];
    /// an exhausted or cancelled governor is [`ToolSetError::Resource`].
    ToolSet(ToolSetError),
}

impl fmt::Display for ToolSetBridgeError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(cause) => write!(output, "tool set manifest: {cause}"),
            Self::ToolSet(cause) => write!(output, "tool set import: {cause}"),
        }
    }
}

impl std::error::Error for ToolSetBridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(cause) => Some(cause.as_ref()),
            Self::ToolSet(cause) => Some(cause),
        }
    }
}

impl ToolGraph {
    /// Build the core [`ToolSet`] for this graph, with the schema losses the
    /// manifest projection recorded.
    ///
    /// The graph is encoded with [`mcp::encode`] and imported through
    /// `ToolSet::from_mcp_with_metadata_governed`, so each tool keeps its exact
    /// input schema and its original declaration text, and
    /// `render_declaration` returns the manifest bytes unchanged. Per tool the
    /// metadata is:
    ///
    /// * aliases: each reified hint alias becomes a parameter alias at the
    ///   parameter's implementation property;
    /// * effects: `Effects::ReadOnly` only when the `readOnlyHint` is true and the
    ///   graph proves no write capability, otherwise `Effects::Mutating`. A hint
    ///   alone never selects `ReadOnly`. Writes are excluded when every allowed
    ///   file rule, mount rule and capability rule of the execution profile and
    ///   its subprofiles is write-free, no waiver or non-inherit exec transition
    ///   is present, and every nested subtool resolves in this graph and is itself
    ///   proven. The proof is a static read of the execution-profile tree and the
    ///   subtool closure only. It is not security admission: `tool.implementation`
    ///   is not inspected, and network, Unix-socket, signal and ptrace allowances
    ///   are treated as non-writes. `ReadOnly` also changes how core handles
    ///   unknown arguments (its default `DropWithLoss`), so the choice is
    ///   behavioural, not only a label. Admission remains #33 work;
    /// * policy: `RepairPolicy::Structural` when no parameter hint permits edit
    ///   distance, coercion or folding, otherwise `RepairPolicy::Tolerant`.
    ///
    /// Core offers no per-parameter edit-distance, coercion or folding switch, so
    /// a `Tolerant` tool can repair more broadly than one hint allows. The graph's
    /// per-hint bounds stay on [`Tool::parameters`] for the later repair check;
    /// this method does not claim to enforce them.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::ToolGraph;
    /// # use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
    /// # use purrdf_core::loss::LossProfile;
    /// # let namespace = "https://example.org/toolgraph#";
    /// # let vocabulary = Vocabulary::new("pa", PrefixTable(vec![("pa".into(), namespace.into())]),
    /// #     TermBindings { namespace_iri: namespace.into(), local_names: TERM_NAMES.iter()
    /// #         .map(|name| ((*name).into(), format!("{namespace}{name}"))).collect() },
    /// #     register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
    /// let mut governor = Governor::new(Budget::default());
    /// let graph = ToolGraph::load(b"", "text/turtle", None, &vocabulary, &mut governor).unwrap();
    /// let (tools, losses) = graph.tool_set(&vocabulary, &mut governor).unwrap();
    /// assert!(tools.tool("anything").is_none());
    /// assert!(losses.is_empty());
    /// ```
    ///
    /// # Errors
    /// [`ToolSetBridgeError::Manifest`] when the graph cannot be projected (for
    /// example a shape without exactly one `sh:targetClass`), and
    /// [`ToolSetBridgeError::ToolSet`] when the adapter refuses the declarations
    /// or metadata, including an alias collision. No partial set is returned.
    pub fn tool_set(
        &self,
        vocabulary: &Vocabulary,
        governor: &mut Governor,
    ) -> Result<(ToolSet, LossLedger), ToolSetBridgeError> {
        let (manifest, losses) = mcp::encode(self, vocabulary, governor)
            .map_err(|cause| ToolSetBridgeError::Manifest(Box::new(cause)))?;
        let metadata: Vec<DeclarationMetadata> = self
            .tools()
            .iter()
            .map(|tool| metadata(self, tool))
            .collect();
        let tools =
            ToolSet::from_mcp_with_metadata_governed(manifest.as_str(), &metadata, governor)
                .map_err(ToolSetBridgeError::ToolSet)?;
        Ok((tools, losses))
    }
}

fn metadata(graph: &ToolGraph, tool: &Tool) -> DeclarationMetadata {
    let hints = || tool.parameters.iter().filter_map(|p| p.hint.as_ref());
    let effects = if tool.annotations.effective_read_only()
        && proves_no_write(graph, tool, &mut BTreeSet::new())
    {
        Effects::ReadOnly
    } else {
        Effects::Mutating
    };
    let infers = hints().any(|hint| {
        hint.max_edit_distance > 0
            || !hint.coercions.is_empty()
            || hint.case_fold
            || hint.unicode_fold
    });
    DeclarationMetadata {
        tool_name: tool.name.clone(),
        effects,
        policy: if infers {
            RepairPolicy::Tolerant
        } else {
            RepairPolicy::Structural
        },
        parameter_aliases: tool
            .parameters
            .iter()
            .filter_map(|parameter| {
                let hint = parameter.hint.as_ref()?;
                (!hint.aliases.is_empty()).then(|| ParameterAliases {
                    property_path: vec![parameter.implementation_property.clone()],
                    aliases: hint.aliases.clone(),
                })
            })
            .collect(),
    }
}

/// Whether the graph proves the tool and everything it can call cannot write.
/// Anything the graph cannot establish counts as a possible write.
fn proves_no_write<'g>(graph: &'g ToolGraph, tool: &'g Tool, seen: &mut BTreeSet<&'g str>) -> bool {
    if !seen.insert(tool.iri.as_str()) {
        return true; // a cycle adds no capability beyond the members already checked
    }
    profile_is_write_free(&tool.execution_profile)
        && tool.subtools.iter().all(|iri| {
            graph
                .tools()
                .iter()
                .find(|candidate| candidate.iri == *iri)
                .is_some_and(|nested| proves_no_write(graph, nested, seen))
        })
}

fn profile_is_write_free(profile: &ToolExecutionProfile) -> bool {
    profile.waivers.is_empty()
        && profile.rules.iter().all(|rule| {
            let value = |name: &str| {
                rule.properties
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, value)| value.as_str())
            };
            if matches!(value("deny"), Some("true" | "1")) {
                return true;
            }
            match rule.class {
                "FileRule" => {
                    !value("filePermissions")
                        .unwrap_or_default()
                        .contains(['w', 'a', 'k', 'l'])
                        && value("execTransition").is_none_or(|transition| transition == "ix")
                }
                "MountRule" | "CapabilityRule" => false,
                _ => true,
            }
        })
        && profile.subprofiles.iter().all(profile_is_write_free)
}
