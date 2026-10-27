// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Caller-owned metadata applied before lossless declaration assembly.

use super::definition::{Effects, ObjectSpec, ParameterKind, ParameterSpec, RepairPolicy};
use super::toolset::{Pending, ToolSetError, invalid, retain_text};
use crate::contract::{Governor, Resource, ScratchVec, saturating_size as size};

/// Caller-supplied recovery metadata for one imported MCP tool.
///
/// This is a typed caller input, not authority derived from MCP annotations.
/// Importing without metadata keeps the conservative mutating effects and the
/// existing repair policy. Metadata never changes the original declaration or
/// its exact schema validator.
///
/// ```
/// use purrassist_core::{Budget, Governor, tool_call::{
///     DeclarationForm, DeclarationMetadata, Effects, ParameterAliases,
///     RepairPolicy, ToolSet,
/// }};
/// let source = r#"{"name":"read","inputSchema":{"type":"object","properties":{"path":{"type":"string"}}}}"#;
/// let metadata = DeclarationMetadata {
///     tool_name: "read".into(), effects: Effects::ReadOnly,
///     policy: RepairPolicy::Tolerant,
///     parameter_aliases: vec![ParameterAliases {
///         property_path: vec!["path".into()], aliases: vec!["file".into()],
///     }],
/// };
/// let tools = ToolSet::from_mcp_with_metadata_governed(
///     source, &[metadata], &mut Governor::new(Budget::default()),
/// ).unwrap();
/// assert_eq!(tools.argument_shape("read").unwrap().arguments.resolve_name("file"), Some("path"));
/// assert_eq!(tools.render_declaration("read", DeclarationForm::Mcp).unwrap(), source);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclarationMetadata {
    /// Exact canonical tool name; aliases and approximate names are not used.
    pub tool_name: String,
    /// Effects established by the caller, independently of wire hints.
    pub effects: Effects,
    /// Recovery policy selected by the caller.
    pub policy: RepairPolicy,
    /// Parameter-name aliases, each attached at one canonical member path.
    pub parameter_aliases: Vec<ParameterAliases>,
}

/// Explicit aliases for a parameter at a canonical object-member path.
///
/// Array items do not add a path component: `customers`, `name` identifies the
/// `name` member of every customer item when `customers` is an array of objects.
/// Traversing an unmodeled value is refused instead of guessing its structure.
/// Aliasing a declared member with an unmodeled value is supported: only its
/// name is changed, and its exact schema still validates the complete candidate.
///
/// See [`DeclarationMetadata`] for an executable import and alias example.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterAliases {
    /// Nonempty sequence of exact object-member names, not a `JSONPath` string.
    pub property_path: Vec<String>,
    /// Explicit spellings checked by the normal object-scope collision rules.
    pub aliases: Vec<String>,
}

pub(super) fn apply(
    pending: &mut [Pending],
    metadata: &[DeclarationMetadata],
    governor: &mut Governor,
) -> Result<(), ToolSetError> {
    for (index, entry) in metadata.iter().enumerate() {
        for earlier in &metadata[..index] {
            if equal(&entry.tool_name, &earlier.tool_name, governor)? {
                return Err(metadata_error(
                    &entry.tool_name,
                    "duplicate metadata record",
                    governor,
                ));
            }
        }
        let mut position = None;
        for (index, tool) in pending.iter().enumerate() {
            if equal(&tool.definition.name, &entry.tool_name, governor)? {
                position = Some(index);
                break;
            }
        }
        let Some(position) = position else {
            return Err(metadata_error(
                &entry.tool_name,
                "metadata names an unknown tool",
                governor,
            ));
        };
        let definition = &mut pending[position].definition;
        definition.effects = entry.effects;
        definition.policy = Some(entry.policy);
        for (index, aliases) in entry.parameter_aliases.iter().enumerate() {
            governor
                .charge(Resource::Work, 1)
                .map_err(ToolSetError::Resource)?;
            if aliases.property_path.is_empty() {
                return Err(metadata_error(
                    &entry.tool_name,
                    "metadata property path is empty",
                    governor,
                ));
            }
            for earlier in &entry.parameter_aliases[..index] {
                if same_path(&aliases.property_path, &earlier.property_path, governor)? {
                    return Err(path_error(
                        &entry.tool_name,
                        "duplicate metadata property path",
                        &aliases.property_path,
                        governor,
                    ));
                }
            }
            let spec = parameter_at(
                &mut definition.arguments,
                &aliases.property_path,
                &entry.tool_name,
                governor,
            )?;
            let mut retained = ScratchVec::new(governor);
            retained
                .reserve(spec.aliases.len().saturating_add(aliases.aliases.len()))
                .map_err(ToolSetError::Resource)?;
            retained
                .extend(core::mem::take(&mut spec.aliases))
                .map_err(ToolSetError::Resource)?;
            for alias in &aliases.aliases {
                retain_text(governor, alias.len())?;
                retained
                    .push(alias.clone())
                    .map_err(ToolSetError::Resource)?;
            }
            spec.aliases = retained.into_vec();
        }
    }
    Ok(())
}

fn metadata_error(tool: &str, detail: &str, governor: &mut Governor) -> ToolSetError {
    // Error payloads are retained too. Never format an unvisited caller path.
    if let Err(error) =
        retain_text(governor, tool.len()).and_then(|()| retain_text(governor, detail.len()))
    {
        return error;
    }
    invalid(tool, detail.into())
}

fn path_error(tool: &str, detail: &str, path: &[String], governor: &mut Governor) -> ToolSetError {
    // Debug string escaping uses at most six output bytes per input byte.
    // Admit every segment before formatting, including ones not yet visited.
    let admitted = (|| {
        retain_text(governor, tool.len())?;
        retain_text(governor, detail.len().saturating_add(3))?;
        for segment in path {
            retain_text(governor, segment.len().saturating_mul(6).saturating_add(4))?;
        }
        Ok::<(), ToolSetError>(())
    })();
    if let Err(error) = admitted {
        return error;
    }
    invalid(tool, format!("{detail} {path:?}"))
}

fn equal(left: &str, right: &str, governor: &mut Governor) -> Result<bool, ToolSetError> {
    governor
        .charge(
            Resource::Work,
            1_u64.saturating_add(size(left.len().max(right.len()))),
        )
        .map_err(ToolSetError::Resource)?;
    Ok(left == right)
}

fn same_path(
    left: &[String],
    right: &[String],
    governor: &mut Governor,
) -> Result<bool, ToolSetError> {
    governor
        .charge(Resource::Work, 1)
        .map_err(ToolSetError::Resource)?;
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        if !equal(left, right, governor)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn parameter_at<'a>(
    mut object: &'a mut ObjectSpec,
    path: &[String],
    tool: &str,
    governor: &mut Governor,
) -> Result<&'a mut ParameterSpec, ToolSetError> {
    for (depth, name) in path.iter().enumerate() {
        let mut position = None;
        for (index, (canonical, _)) in object.parameters.iter().enumerate() {
            if equal(canonical, name, governor)? {
                position = Some(index);
                break;
            }
        }
        let Some(position) = position else {
            return Err(path_error(
                tool,
                "unknown metadata property path",
                path,
                governor,
            ));
        };
        let parameter = &mut object.parameters[position].1;
        if depth + 1 == path.len() {
            return Ok(parameter);
        }
        let mut kind = &mut parameter.kind;
        loop {
            governor
                .charge(Resource::Work, 1)
                .map_err(ToolSetError::Resource)?;
            match kind {
                ParameterKind::Array(item) => kind = &mut item.kind,
                ParameterKind::Object(nested) => {
                    object = nested;
                    break;
                }
                _ => {
                    return Err(path_error(
                        tool,
                        "metadata property path traverses a non-object or unmodeled value",
                        path,
                        governor,
                    ));
                }
            }
        }
    }
    Err(metadata_error(
        tool,
        "metadata property path is empty",
        governor,
    ))
}
