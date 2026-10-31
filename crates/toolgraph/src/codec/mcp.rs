// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Borrowed MCP Extended bundle envelopes over the existing ordered JSON DOM.
//!
//! Envelope reading checks wire structure and versioning without copying JSON
//! trees. RDF parsing, vocabulary binding, schema compilation and agreement
//! between the source graph and projected metadata belong to the graph loader.
//! These views do not establish graph admission or executable tool identities.

use std::{fmt, ops::Range};

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, saturating_size},
    json::{Node, Value},
};

use crate::{ContextCodecError, ContextManagement, LOSS_PROFILE_ID, MCP_CONTEXT_FORMAT};

/// Structural refusal at an original JSON occurrence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvelopeProblem {
    /// An expected object, array or string has a different JSON type.
    WrongType(&'static str),
    /// A required member is absent from the named object.
    MissingField {
        /// Object containing the missing declaration.
        object: &'static str,
        /// Required member name.
        field: &'static str,
    },
    /// A fixed wire member occurred more than once.
    DuplicateField {
        /// Object containing the duplicate.
        object: &'static str,
        /// Repeated member name.
        field: &'static str,
    },
    /// An unrecognized fixed wire member cannot be silently discarded.
    UnknownField {
        /// Object containing the unsupported member.
        object: &'static str,
        /// Original member position, without copying its name.
        index: usize,
    },
    /// The bundle does not explicitly declare the supported v2 format.
    UnsupportedFormat,
    /// The source graph is not declared as Turtle.
    UnsupportedMediaType,
    /// The bundle names a different projection loss contract.
    UnsupportedLossProfile,
    /// The caller refused further work or cancelled the operation.
    Resource(StopReason),
}

/// Envelope error with its original location and optional tool occurrence.
///
/// Missing-member errors locate the containing object. Duplicate and unknown
/// members locate their original keys. Generated JSON retains an absent span;
/// no synthetic source location is invented. The caller's DOM is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeError {
    /// Original typed failure, distinct from resource refusal.
    pub problem: EnvelopeProblem,
    /// Half-open original JSON byte range, when available.
    pub span: Option<Range<usize>>,
    /// Original tool-array index, when the error belongs to a tool.
    pub tool: Option<usize>,
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(index) = self.tool {
            write!(output, "tool at index {index}: ")?;
        }
        match &self.problem {
            EnvelopeProblem::WrongType(field) => write!(output, "wrong JSON type for {field}"),
            EnvelopeProblem::MissingField { object, field } => {
                write!(output, "missing {object}.{field}")
            }
            EnvelopeProblem::DuplicateField { object, field } => {
                write!(output, "duplicate {object}.{field}")
            }
            EnvelopeProblem::UnknownField { object, index } => {
                write!(output, "unknown {object} member at index {index}")
            }
            EnvelopeProblem::UnsupportedFormat => {
                write!(output, "bundle format must be {MCP_CONTEXT_FORMAT}")
            }
            EnvelopeProblem::UnsupportedMediaType => {
                output.write_str("bundle source mediaType must be text/turtle")
            }
            EnvelopeProblem::UnsupportedLossProfile => {
                write!(output, "bundle lossProfile must be {LOSS_PROFILE_ID}")
            }
            EnvelopeProblem::Resource(cause) => cause.fmt(output),
        }
    }
}

impl std::error::Error for EnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.problem {
            EnvelopeProblem::Resource(cause) => Some(cause),
            _ => None,
        }
    }
}

/// Complete borrowed outer bundle and its original ordered tool occurrences.
///
/// For example, an empty bundle has `tools: []` and `_meta.security` containing
/// `format: "purrassist-toolgraph-v2"`, `mediaType: "text/turtle"`,
/// `prefixes: {}`, `sourceGraph: ""` and
/// `lossProfile: "purrassist-toolgraph-v1"`. Reading that envelope exposes an
/// empty tool slice and the exact empty source string; it does not infer an
/// admitted RDF graph. Nonempty bundles retain every original declaration.
///
/// Prefix meanings and duplicates, tool-name uniqueness, nested metadata and
/// schema semantics require their owning vocabulary and graph operations.
#[derive(Clone, Copy, Debug)]
pub struct ManifestEnvelope<'a> {
    original: &'a Node,
    tools: &'a [Node],
    prefixes: &'a Node,
    source_graph: &'a str,
}

impl<'a> ManifestEnvelope<'a> {
    /// Read every envelope occurrence under the caller's original governor.
    ///
    /// Fixed member names, their uniqueness, required members and wire types
    /// are checked without allocating or reparsing. Legacy formats are refused
    /// before any tool is interpreted. Source graph and schema bytes are not
    /// rewritten. The caller retains ownership of the parsed DOM throughout.
    ///
    /// # Errors
    /// Returns an original-location structural error, unsupported wire contract
    /// or native resource refusal. No partial envelope is returned.
    pub fn read(source: &'a Node, governor: &mut Governor) -> Result<Self, EnvelopeError> {
        let [tools, meta] = fields(source, "manifest", ["tools", "_meta"], 0, governor)?;
        let [security] = fields(meta, "bundle _meta", ["security"], 1, governor)?;
        let [format, media, prefixes, graph, losses] = fields(
            security,
            "bundle security",
            [
                "format",
                "mediaType",
                "prefixes",
                "sourceGraph",
                "lossProfile",
            ],
            2,
            governor,
        )?;
        tag(
            format,
            MCP_CONTEXT_FORMAT,
            EnvelopeProblem::UnsupportedFormat,
            governor,
        )?;
        tag(
            media,
            "text/turtle",
            EnvelopeProblem::UnsupportedMediaType,
            governor,
        )?;
        tag(
            losses,
            LOSS_PROFILE_ID,
            EnvelopeProblem::UnsupportedLossProfile,
            governor,
        )?;
        object(prefixes, "prefixes", 3, governor)?;
        let source_graph = text(graph, "sourceGraph", governor)?;
        step(tools, Resource::Depth, 1, governor)?;
        let Value::Array(declarations) = &tools.value else {
            return Err(error(tools, EnvelopeProblem::WrongType("tools")));
        };
        for (index, declaration) in declarations.iter().enumerate() {
            ToolEnvelope::read(declaration, governor).map_err(|mut cause| {
                cause.tool = Some(index);
                cause
            })?;
        }
        step(source, Resource::Work, 1, governor)?;
        Ok(Self {
            original: source,
            tools: declarations,
            prefixes,
            source_graph,
        })
    }

    /// Original DOM with all number spellings, member order and source spans.
    #[must_use]
    pub const fn original(&self) -> &'a Node {
        self.original
    }

    /// Original tool declarations, including their opaque schemas and metadata.
    #[must_use]
    pub const fn declarations(&self) -> &'a [Node] {
        self.tools
    }

    /// Original prefix object for the caller-owned vocabulary validator.
    #[must_use]
    pub const fn prefixes(&self) -> &'a Node {
        self.prefixes
    }

    /// Decoded Turtle string without normalization, parsing or graph selection.
    #[must_use]
    pub const fn source_graph(&self) -> &'a str {
        self.source_graph
    }

    /// Inspect one original declaration using the same governed field reader.
    ///
    /// For example, `tool(0, governor)` reads the first declaration; an empty
    /// bundle returns `Ok(None)`. No declaration is synthesized on a miss.
    ///
    /// # Errors
    /// Returns cancellation or work refusal, or a located declaration error.
    pub fn tool(
        &self,
        index: usize,
        governor: &mut Governor,
    ) -> Result<Option<ToolEnvelope<'a>>, EnvelopeError> {
        step(self.original, Resource::Work, 1, governor)?;
        self.tools
            .get(index)
            .map(|source| ToolEnvelope::read(source, governor))
            .transpose()
            .map_err(|mut cause| {
                cause.tool = Some(index);
                cause
            })
    }
}

/// Original MCP declaration fields under a checked enclosing v2 envelope.
///
/// JSON Schema and annotation/profile objects remain opaque original nodes.
/// This is a wire view; semantic agreement with RDF requires the graph loader.
#[derive(Clone, Copy, Debug)]
pub struct ToolEnvelope<'a> {
    original: &'a Node,
    name: &'a str,
    description: &'a str,
    input: &'a Node,
    output: &'a Node,
    annotations: &'a Node,
    security: &'a Node,
    execution: &'a Node,
    context: &'a Node,
}

impl<'a> ToolEnvelope<'a> {
    fn read(source: &'a Node, governor: &mut Governor) -> Result<Self, EnvelopeError> {
        let [name, description, input, output, annotations, meta] = fields(
            source,
            "tool",
            [
                "name",
                "description",
                "inputSchema",
                "outputSchema",
                "annotations",
                "_meta",
            ],
            2,
            governor,
        )?;
        let name = text(name, "name", governor)?;
        let description = text(description, "description", governor)?;
        for (field, node) in [
            ("inputSchema", input),
            ("outputSchema", output),
            ("annotations", annotations),
        ] {
            object(node, field, 3, governor)?;
        }
        let [security, execution, context] = fields(
            meta,
            "tool _meta",
            ["security", "executionProfile", "contextManagement"],
            3,
            governor,
        )?;
        for (field, node) in [
            ("security", security),
            ("executionProfile", execution),
            ("contextManagement", context),
        ] {
            object(node, field, 4, governor)?;
        }
        Ok(Self {
            original: source,
            name,
            description,
            input,
            output,
            annotations,
            security,
            execution,
            context,
        })
    }

    /// Original complete declaration, including exact schema occurrences.
    #[must_use]
    pub const fn original(&self) -> &'a Node {
        self.original
    }
    /// Unmodified declared name; uniqueness belongs to graph admission.
    #[must_use]
    pub const fn name(&self) -> &'a str {
        self.name
    }
    /// Unmodified declared description.
    #[must_use]
    pub const fn description(&self) -> &'a str {
        self.description
    }
    /// Complete original input JSON Schema object.
    #[must_use]
    pub const fn input_schema(&self) -> &'a Node {
        self.input
    }
    /// Complete original output JSON Schema object.
    #[must_use]
    pub const fn output_schema(&self) -> &'a Node {
        self.output
    }
    /// Original annotation object, including member presence.
    #[must_use]
    pub const fn annotations(&self) -> &'a Node {
        self.annotations
    }
    /// Original rule metadata for subsequent graph agreement checking.
    #[must_use]
    pub const fn security(&self) -> &'a Node {
        self.security
    }
    /// Original execution metadata for subsequent typed validation.
    #[must_use]
    pub const fn execution_profile(&self) -> &'a Node {
        self.execution
    }
    /// Original context fragment for the existing context codec.
    #[must_use]
    pub const fn context(&self) -> &'a Node {
        self.context
    }

    /// Decode this fragment using the existing canonical v2 context codec.
    ///
    /// For example, a legacy `maxOutputTokens` field produces the existing
    /// `ContextCodecError::LegacyTokenLimit`; it is never reinterpreted as bytes.
    /// The returned context retains the ordinary codec's publication contract.
    ///
    /// # Errors
    /// Returns the original context syntax, unit, mode or resource refusal.
    pub fn decode_context(
        &self,
        governor: &mut Governor,
    ) -> Result<ContextManagement, ContextCodecError> {
        ContextManagement::decode_mcp(Some(MCP_CONTEXT_FORMAT), self.context, governor)
    }
}

fn error(source: &Node, problem: EnvelopeProblem) -> EnvelopeError {
    EnvelopeError {
        problem,
        span: source.span.clone(),
        tool: None,
    }
}

fn step(
    source: &Node,
    resource: Resource,
    units: u64,
    governor: &mut Governor,
) -> Result<(), EnvelopeError> {
    governor
        .charge(resource, units)
        .map_err(|cause| error(source, EnvelopeProblem::Resource(cause)))
}

fn object(
    source: &Node,
    field: &'static str,
    depth: u64,
    governor: &mut Governor,
) -> Result<(), EnvelopeError> {
    step(source, Resource::Depth, depth, governor)?;
    step(source, Resource::Work, 1, governor)?;
    if matches!(&source.value, Value::Object(_)) {
        Ok(())
    } else {
        Err(error(source, EnvelopeProblem::WrongType(field)))
    }
}

fn text<'a>(
    source: &'a Node,
    field: &'static str,
    governor: &mut Governor,
) -> Result<&'a str, EnvelopeError> {
    step(source, Resource::Work, 1, governor)?;
    match &source.value {
        Value::String(value) => Ok(value),
        _ => Err(error(source, EnvelopeProblem::WrongType(field))),
    }
}

fn tag(
    source: &Node,
    expected: &'static str,
    problem: EnvelopeProblem,
    governor: &mut Governor,
) -> Result<(), EnvelopeError> {
    let value = text(source, "bundle contract tag", governor)?;
    // A fixed expected tag bounds equality work even for an arbitrary input.
    step(
        source,
        Resource::Work,
        saturating_size(expected.len()).saturating_add(1),
        governor,
    )?;
    if value == expected {
        Ok(())
    } else {
        Err(error(source, problem))
    }
}

fn fields<'a, const N: usize>(
    source: &'a Node,
    label: &'static str,
    names: [&'static str; N],
    depth: u64,
    governor: &mut Governor,
) -> Result<[&'a Node; N], EnvelopeError> {
    object(source, label, depth, governor)?;
    let Value::Object(members) = &source.value else {
        unreachable!("object type was checked")
    };
    let mut selected = [None; N];
    for (index, member) in members.iter().enumerate() {
        step(&member.value, Resource::Work, 1, governor)?;
        let mut slot = None;
        for (candidate, name) in names.iter().enumerate() {
            step(
                &member.value,
                Resource::Work,
                saturating_size(name.len()).saturating_add(1),
                governor,
            )?;
            if member.key == *name {
                slot = Some(candidate);
                break;
            }
        }
        let Some(slot) = slot else {
            return Err(EnvelopeError {
                problem: EnvelopeProblem::UnknownField {
                    object: label,
                    index,
                },
                span: member.key_span.clone(),
                tool: None,
            });
        };
        if selected[slot].replace(&member.value).is_some() {
            return Err(EnvelopeError {
                problem: EnvelopeProblem::DuplicateField {
                    object: label,
                    field: names[slot],
                },
                span: member.key_span.clone(),
                tool: None,
            });
        }
    }
    for (slot, value) in selected.iter().enumerate() {
        step(source, Resource::Work, 1, governor)?;
        if value.is_none() {
            return Err(error(
                source,
                EnvelopeProblem::MissingField {
                    object: label,
                    field: names[slot],
                },
            ));
        }
    }
    Ok(selected.map(|value| value.expect("all required envelope members were checked")))
}
