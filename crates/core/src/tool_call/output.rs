// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact optional output contracts, independent of argument projection.
use super::{DeclarationForm, EmitError, ToolSetError};
use crate::{
    Governor, ScratchString, StopReason,
    contract::{Resource, format_scratch_governed, saturating_size as size},
    json::{Node, Value},
    schema::{CompilationError, Schema},
};
use core::fmt;

#[derive(Debug)]
pub(super) struct OutputSchema {
    pub(super) text: ScratchString,
    pub(super) schema: Schema,
    mcp: bool,
}

pub(super) fn invalid(
    tool: &str,
    reason: fmt::Arguments<'_>,
    governor: &mut Governor,
) -> ToolSetError {
    let mut build = || -> Result<ToolSetError, StopReason> {
        governor.charge(Resource::Work, size(tool.len()))?;
        let mut name = ScratchString::new(governor);
        name.push_str(tool)?;
        let reason = format_scratch_governed(format_args!("output schema: {reason}"), governor)?;
        governor.check()?;
        Ok(ToolSetError::InvalidDefinition {
            tool: name.into_string(),
            reason: reason.into_string(),
        })
    };
    build().unwrap_or_else(ToolSetError::Resource)
}

pub(super) fn field<'n>(
    node: &'n Node,
    key: &str,
    tool: &str,
    governor: &mut Governor,
) -> Result<Option<&'n Node>, ToolSetError> {
    let Value::Object(members) = &node.value else {
        return Ok(None);
    };
    let mut found = None;
    for member in members {
        governor
            .charge(Resource::Work, size(member.key.len()).saturating_add(1))
            .map_err(ToolSetError::Resource)?;
        if member.key == key {
            if found.is_some() {
                return Err(invalid(
                    tool,
                    format_args!("duplicate `{key}` member"),
                    governor,
                ));
            }
            found = Some(&member.value);
        }
    }
    Ok(found)
}

fn mcp_object(node: &Node, tool: &str, governor: &mut Governor) -> Result<bool, ToolSetError> {
    governor
        .charge(Resource::Work, 1)
        .map_err(ToolSetError::Resource)?;
    let Some(value) = field(node, "type", tool, governor)? else {
        return Ok(false);
    };
    if let Value::String(text) = &value.value {
        governor
            .charge(Resource::Work, size(text.len()))
            .map_err(ToolSetError::Resource)?;
        return Ok(text == "object");
    }
    Ok(false)
}

impl OutputSchema {
    pub(super) fn from_builder(
        definition: &super::ToolDefinition,
        governor: &mut Governor,
    ) -> Result<Option<Self>, ToolSetError> {
        definition
            .output_schema
            .as_deref()
            .map(|source| Self::compile(&definition.name, source, false, governor))
            .transpose()
    }
    pub(super) fn compile(
        tool: &str,
        source: &str,
        require_mcp: bool,
        governor: &mut Governor,
    ) -> Result<Self, ToolSetError> {
        governor.check().map_err(ToolSetError::Resource)?;
        governor
            .charge(Resource::InputBytes, size(source.len()))
            .map_err(ToolSetError::Resource)?;
        governor
            .charge(Resource::Work, size(source.len()))
            .map_err(ToolSetError::Resource)?;
        let mut text = ScratchString::new(governor);
        text.push_str(source).map_err(ToolSetError::Resource)?;
        let schema = match Schema::compile_governed(text.as_str(), governor) {
            Ok(schema) => schema,
            Err(CompilationError::Resource(reason)) => return Err(ToolSetError::Resource(reason)),
            Err(CompilationError::InvalidSchema(error)) => {
                return Err(invalid(tool, format_args!("rejected: {error}"), governor));
            }
        };
        let mcp = mcp_object(schema.document(), tool, governor)?;
        if require_mcp && !mcp {
            return Err(invalid(
                tool,
                format_args!(
                    "MCP `outputSchema` requires an object with one root `type` equal to `object`"
                ),
                governor,
            ));
        }
        governor.check().map_err(ToolSetError::Resource)?;
        Ok(Self { text, schema, mcp })
    }
    pub(super) fn admit_target(
        &self,
        form: DeclarationForm,
        governor: &mut Governor,
    ) -> Result<(), EmitError> {
        governor.check()?;
        if form == DeclarationForm::Mcp && self.mcp {
            return Ok(());
        }
        let reason = format_scratch_governed(
            format_args!("output schema cannot be represented by {form:?}"),
            governor,
        )?;
        governor.check()?;
        Err(EmitError::Unsupported {
            reason: reason.into_string(),
        })
    }
}

// Include the fragment's adjacent JSON whitespace so builder source identity
// survives insertion after a colon and subsequent import without trimming.
pub(super) fn fragment_span(
    input: &str,
    node: &Node,
    tool: &str,
    governor: &mut Governor,
) -> Result<core::ops::Range<usize>, ToolSetError> {
    let Some(mut range) = node.span.clone() else {
        return Err(invalid(
            tool,
            format_args!("source span is missing"),
            governor,
        ));
    };
    let bytes = input.as_bytes();
    while range.start > 0 {
        governor
            .charge(Resource::Work, 1)
            .map_err(ToolSetError::Resource)?;
        if !matches!(bytes[range.start - 1], b' ' | b'\t' | b'\r' | b'\n') {
            break;
        }
        range.start -= 1;
    }
    while range.end < bytes.len() {
        governor
            .charge(Resource::Work, 1)
            .map_err(ToolSetError::Resource)?;
        if !matches!(bytes[range.end], b' ' | b'\t' | b'\r' | b'\n') {
            break;
        }
        range.end += 1;
    }
    Ok(range)
}
