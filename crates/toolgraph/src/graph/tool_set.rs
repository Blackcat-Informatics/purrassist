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

use std::cmp::Ordering;
use std::fmt;
use std::slice::Iter;

use purrassist_core::{
    Governor,
    contract::saturating_size,
    tool_call::{
        DeclarationMetadata, Effects, ParameterAliases, RepairPolicy, ToolSet, ToolSetError,
    },
};
use purrdf_core::loss::LossLedger;

use super::{
    LoadError, Tool, ToolExecutionProfile, ToolGraph,
    load::{BuildStorage, with_build_storage},
};
use crate::{Vocabulary, codec::mcp};

/// A refusal to build the core tool set for a loaded graph. Nothing partial escapes.
#[derive(Debug)]
#[non_exhaustive]
pub enum ToolSetBridgeError {
    /// The graph has no MCP Extended manifest projection.
    Manifest(Box<mcp::CodecError>),
    /// Preparing the temporary aliases or checking transitive effects failed.
    Metadata(Box<LoadError>),
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
            Self::Metadata(cause) => write!(output, "tool set metadata: {cause}"),
            Self::ToolSet(cause) => write!(output, "tool set import: {cause}"),
        }
    }
}

impl std::error::Error for ToolSetBridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(cause) => Some(cause.as_ref()),
            Self::Metadata(cause) => Some(cause.as_ref()),
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
    ///   are treated as non-writes. Unknown-argument handling comes from the exact
    ///   schema, not from effects. Admission remains #33 work;
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
    /// or metadata, including an alias collision. The caller's governor admits
    /// the alias copies and iterative transitive effect checks. A refusal there
    /// returns [`ToolSetBridgeError::Metadata`]. No partial set is returned.
    pub fn tool_set(
        &self,
        vocabulary: &Vocabulary,
        governor: &mut Governor,
    ) -> Result<(ToolSet, LossLedger), ToolSetBridgeError> {
        let (manifest, losses) = mcp::encode(self, vocabulary, governor)
            .map_err(|cause| ToolSetBridgeError::Manifest(Box::new(cause)))?;
        let tools = with_build_storage(governor, |storage| {
            let metadata = bridge_metadata(self, storage)
                .map_err(|cause| ToolSetBridgeError::Metadata(Box::new(cause)))?;
            storage
                .with_governor(|governor| {
                    ToolSet::from_mcp_with_metadata_governed(manifest.as_str(), &metadata, governor)
                })
                .map_err(ToolSetBridgeError::ToolSet)
        })?;
        Ok((tools, losses))
    }
}

fn bridge_metadata(
    graph: &ToolGraph,
    storage: &mut BuildStorage<'_, '_, '_>,
) -> Result<Vec<DeclarationMetadata>, LoadError> {
    let mut result = Vec::new();
    let mut proof = EffectProof::default();
    for (index, tool) in graph.tools().iter().enumerate() {
        storage.charge(1)?;
        let effects = if tool.annotations.effective_read_only()
            && proof.proves_no_write(graph.tools(), index, storage)?
        {
            Effects::ReadOnly
        } else {
            Effects::Mutating
        };
        let entry = metadata(tool, effects, storage)?;
        storage.push(&mut result, entry)?;
    }
    storage.release(proof.seen)?;
    storage.release(proof.tools)?;
    storage.release(proof.profiles)?;
    Ok(result)
}

fn metadata(
    tool: &Tool,
    effects: Effects,
    storage: &mut BuildStorage<'_, '_, '_>,
) -> Result<DeclarationMetadata, LoadError> {
    let mut infers = false;
    let mut parameter_aliases = Vec::new();
    for parameter in &tool.parameters {
        storage.charge(1)?;
        let Some(hint) = &parameter.hint else {
            continue;
        };
        infers |= hint.max_edit_distance > 0
            || !hint.coercions.is_empty()
            || hint.case_fold
            || hint.unicode_fold;
        if !hint.aliases.is_empty() {
            let mut property_path = Vec::new();
            let property = storage.string(&parameter.implementation_property)?;
            storage.push(&mut property_path, property)?;
            let mut aliases = Vec::new();
            for alias in &hint.aliases {
                storage.charge(1)?;
                let alias = storage.string(alias)?;
                storage.push(&mut aliases, alias)?;
            }
            storage.push(
                &mut parameter_aliases,
                ParameterAliases {
                    property_path,
                    aliases,
                },
            )?;
        }
    }
    Ok(DeclarationMetadata {
        tool_name: storage.string(&tool.name)?,
        effects,
        policy: if infers {
            RepairPolicy::Tolerant
        } else {
            RepairPolicy::Structural
        },
        parameter_aliases,
    })
}

/// Reuse admitted traversal buffers across each requested proof.
#[derive(Default)]
struct EffectProof<'g> {
    seen: Vec<usize>,
    tools: Vec<Iter<'g, String>>,
    profiles: Vec<Iter<'g, ToolExecutionProfile>>,
}

impl<'g> EffectProof<'g> {
    fn proves_no_write(
        &mut self,
        tools: &'g [Tool],
        root: usize,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<bool, LoadError> {
        if self.seen.is_empty() {
            storage.fill(&mut self.seen, tools.len(), 0)?;
        }
        self.tools.clear();
        // Zero is unvisited. Each root has its own index, so stamps need no reset.
        let visit = root + 1;
        let mut index = root;
        loop {
            storage.charge(1)?;
            self.seen[index] = visit;
            let tool = &tools[index];
            if !self.profile_is_write_free(&tool.execution_profile, storage)? {
                return Ok(false);
            }
            if !tool.subtools.is_empty() {
                storage.push(&mut self.tools, tool.subtools.iter())?;
            }
            index = loop {
                let Some(iri) = next_child(&mut self.tools, storage)? else {
                    return Ok(true);
                };
                storage.charge(1)?;
                let Some(child) = find_tool(tools, iri, storage)? else {
                    return Ok(false);
                };
                if self.seen[child] != visit {
                    break child;
                }
            };
        }
    }

    fn profile_is_write_free(
        &mut self,
        mut profile: &'g ToolExecutionProfile,
        storage: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<bool, LoadError> {
        self.profiles.clear();
        loop {
            storage.charge(1)?;
            if !profile.waivers.is_empty() || !rules_are_write_free(profile, storage)? {
                return Ok(false);
            }
            if !profile.subprofiles.is_empty() {
                storage.push(&mut self.profiles, profile.subprofiles.iter())?;
            }
            let Some(child) = next_child(&mut self.profiles, storage)? else {
                return Ok(true);
            };
            profile = child;
        }
    }
}

fn next_child<'a, T>(
    parents: &mut Vec<Iter<'a, T>>,
    storage: &BuildStorage<'_, '_, '_>,
) -> Result<Option<&'a T>, LoadError> {
    while let Some(siblings) = parents.last_mut() {
        storage.charge(0)?;
        if let Some(child) = siblings.next() {
            return Ok(Some(child));
        }
        parents.pop();
    }
    Ok(None)
}

fn find_tool(
    tools: &[Tool],
    iri: &str,
    storage: &BuildStorage<'_, '_, '_>,
) -> Result<Option<usize>, LoadError> {
    let (mut start, mut end) = (0, tools.len());
    while start < end {
        let index = start + (end - start) / 2;
        let candidate = tools[index].iri.as_str();
        storage.charge(saturating_size(candidate.len().min(iri.len())).saturating_add(1))?;
        match candidate.cmp(iri) {
            Ordering::Less => start = index + 1,
            Ordering::Equal => return Ok(Some(index)),
            Ordering::Greater => end = index,
        }
    }
    Ok(None)
}

fn rules_are_write_free(
    profile: &ToolExecutionProfile,
    storage: &BuildStorage<'_, '_, '_>,
) -> Result<bool, LoadError> {
    for rule in &profile.rules {
        storage.charge(1)?;
        let mut deny = None;
        let mut permissions = None;
        let mut transition = None;
        for (key, value) in &rule.properties {
            storage.charge(1)?;
            match *key {
                "deny" => deny.get_or_insert(value.as_str()),
                "filePermissions" => permissions.get_or_insert(value.as_str()),
                "execTransition" => transition.get_or_insert(value.as_str()),
                _ => continue,
            };
        }
        if matches!(deny, Some("true" | "1")) {
            continue;
        }
        match rule.class {
            "FileRule" => {
                let permissions = permissions.unwrap_or_default();
                storage.charge(saturating_size(permissions.len()))?;
                if permissions.contains(['w', 'a', 'k', 'l'])
                    || transition.is_some_and(|transition| transition != "ix")
                {
                    return Ok(false);
                }
            }
            "MountRule" | "CapabilityRule" => return Ok(false),
            _ => {}
        }
    }
    Ok(true)
}
