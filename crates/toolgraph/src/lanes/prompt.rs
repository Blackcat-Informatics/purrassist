// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The model-family prompt lane: one family's exact declaration injection.
//!
//! Only [`ModelFamily::Hermes`] is projected. Its declaration form is the
//! existing `DeclarationForm::Hermes` renderer, invoked once per tool over the
//! tool set that the MCP Extended manifest already derives, so the prompt cannot
//! disagree with the manifest. The injected system content is
//! `"<tools>" LF declaration (LF declaration)* LF "</tools>"`, with tools in
//! canonical name order. Every other family is a typed refusal until its own
//! projection is delivered.
//!
//! The declaration carries the input JSON Schema and the description, so those
//! are not losses. Everything else a tool declares is ledgered by name under the
//! registered `prompt.hermes/hermes.v1` representation.

use std::fmt;

use purrassist_core::{
    Governor, StopReason,
    json::{self, Node, ParseFailure, Value},
    tool_call::{DeclarationForm, EmitError, ToolSet, ToolSetError},
};
use purrdf_core::loss::{LossLedger, LossSoundnessError, check_ledger_sound_in};

use super::shared::{entry_for, record_tool, record_upstream};
use crate::{
    ModelFamily, ProjectionTarget, SOURCE_REPRESENTATION, ToolGraph, Vocabulary,
    codec::mcp::{self, CodecError},
    prompt_bytes::{FramedPrompt, PromptByteFailure, PromptBytePart},
};

/// The exact system content one model family receives, with its byte count.
///
/// Its representation identity is that of the family's
/// [`ProjectionTarget::Prompt`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptProjection {
    family: ModelFamily,
    text: String,
    exact_bytes: u64,
}

impl PromptProjection {
    /// The family this text was projected for.
    #[must_use]
    pub const fn family(&self) -> ModelFamily {
        self.family
    }

    /// The exact text injected into the family's system content.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The exact UTF-8 byte length of [`Self::as_str`], every framing byte included.
    ///
    /// This is a byte measurement only; it is not a token bound.
    #[must_use]
    pub const fn exact_bytes(&self) -> u64 {
        self.exact_bytes
    }
}

/// A refusal to project a graph for a model family. Nothing partial escapes.
#[derive(Debug)]
#[non_exhaustive]
pub enum PromptError {
    /// This family has no delivered projection.
    UnsupportedFamily(ModelFamily),
    /// The graph has no MCP Extended manifest projection to derive declarations from.
    Manifest(Box<CodecError>),
    /// The projected manifest could not be read back as JSON.
    Json(ParseFailure),
    /// The manifest could not be read back as a tool set.
    ToolSet(ToolSetError),
    /// The family's renderer refused a declaration, for example one whose text
    /// would end the `<tools>` element early.
    Declaration {
        /// The tool's name.
        tool: String,
        /// The renderer's refusal.
        cause: EmitError,
    },
    /// The complete injected text exceeds the caller's byte ceiling. No
    /// declaration is dropped or truncated to fit.
    ByteCeiling {
        /// Exact bytes the complete projection needs.
        required_bytes: u64,
        /// The caller's ceiling.
        max_bytes: u64,
    },
    /// The complete text length cannot be represented.
    LengthOverflow,
    /// A recorded loss is not registered for the family's representation.
    Unsound(LossSoundnessError),
    /// The caller refused further work or cancelled the operation.
    Resource(StopReason),
}

impl fmt::Display for PromptError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFamily(family) => {
                write!(output, "no prompt projection is delivered for {family:?}")
            }
            Self::Manifest(cause) => write!(output, "prompt manifest: {cause}"),
            Self::Json(cause) => write!(output, "prompt manifest JSON: {cause}"),
            Self::ToolSet(cause) => write!(output, "prompt tool set: {cause:?}"),
            Self::Declaration { tool, cause } => {
                write!(output, "tool {tool}: declaration refused: {cause:?}")
            }
            Self::ByteCeiling {
                required_bytes,
                max_bytes,
            } => write!(
                output,
                "prompt requires {required_bytes} bytes, exceeding the {max_bytes} byte ceiling"
            ),
            Self::LengthOverflow => output.write_str("prompt byte length overflow"),
            Self::Unsound(cause) => write!(output, "unregistered loss: {cause}"),
            Self::Resource(cause) => write!(output, "prompt projection stopped: {cause:?}"),
        }
    }
}

impl std::error::Error for PromptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(cause) => Some(cause.as_ref()),
            Self::Json(cause) => Some(cause),
            _ => None,
        }
    }
}

/// Project every tool of `graph` into `family`'s system-content injection.
///
/// The text is `<tools>`, LF, the Hermes declaration of each tool in canonical
/// name order separated by LF, LF and `</tools>`. A graph with no tools yields
/// `<tools>` LF LF `</tools>`. `max_bytes` is an independent semantic ceiling
/// over the complete text.
///
/// The ledger holds, per tool in name order, the rows
/// `authorization-not-expressible`, `session-binding-not-expressible`,
/// `taint-not-expressible`, `recovery-hints-not-expressible`,
/// `confinement-not-expressible`, `resource-ceiling-not-expressible`,
/// `context-contract-not-expressible`, `tool-annotations-not-expressible` and
/// `rdf-binding-not-expressible` (each as the compact lane records it), then
/// `output-shape-not-expressible` at the output shape, because a declaration
/// carries no output schema. The description is not a loss: the declaration
/// carries it. Each note names the mandatory check that enforces the omitted
/// constraint. The ledger is checked against the vocabulary's loss profile for
/// the family's representation before it is returned.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{ModelFamily, ToolGraph, lanes};
/// # use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
/// # use purrdf_core::loss::LossProfile;
/// # let namespace = "https://example.org/toolgraph#";
/// # let vocabulary = Vocabulary::new("pa", PrefixTable(vec![("pa".into(), namespace.into())]),
/// #     TermBindings { namespace_iri: namespace.into(), local_names: TERM_NAMES.iter()
/// #         .map(|name| ((*name).into(), format!("{namespace}{name}"))).collect() },
/// #     register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let graph = ToolGraph::load(b"", "text/turtle", None, &vocabulary, &mut governor).unwrap();
/// let (prompt, losses) = lanes::prompt(&graph, &vocabulary, ModelFamily::Hermes, 64, &mut governor).unwrap();
/// assert_eq!(prompt.as_str(), "<tools>\n\n</tools>");
/// assert_eq!(prompt.exact_bytes(), 17);
/// assert!(losses.is_empty());
/// ```
///
/// # Errors
/// [`PromptError::UnsupportedFamily`] for any family other than
/// [`ModelFamily::Hermes`], [`PromptError::Manifest`] when the graph has no
/// manifest projection, [`PromptError::ToolSet`] when the manifest is not a
/// valid tool set, [`PromptError::Declaration`] when the renderer refuses a
/// declaration, [`PromptError::ByteCeiling`] when the complete text exceeds
/// `max_bytes`, [`PromptError::LengthOverflow`] when its length is unrepresentable, [`PromptError::Unsound`] when a ledger entry is not registered
/// (including a construct the upstream schema compiler dropped, which this
/// representation has no code for), and [`PromptError::Resource`] when the
/// governor stops the operation. Nothing is returned on failure.
pub fn prompt(
    graph: &ToolGraph,
    vocabulary: &Vocabulary,
    family: ModelFamily,
    max_bytes: u64,
    governor: &mut Governor,
) -> Result<(PromptProjection, LossLedger), PromptError> {
    if family != ModelFamily::Hermes {
        return Err(PromptError::UnsupportedFamily(family));
    }
    let target = ProjectionTarget::Prompt(family);
    let (manifest, upstream) = mcp::encode(graph, vocabulary, governor)
        .map_err(|cause| PromptError::Manifest(Box::new(cause)))?;
    let tools = declared_tools(manifest.as_str(), governor)?;
    let mut declarations = Vec::with_capacity(tools.tools().len());
    for tool in tools.tools() {
        let text = tool
            .render_declaration_governed(DeclarationForm::Hermes, governor)
            .map_err(|cause| match cause {
                EmitError::Resource(stop) => PromptError::Resource(stop),
                cause => PromptError::Declaration {
                    tool: tool.name().to_owned(),
                    cause,
                },
            })?;
        declarations.push((tool.name(), text));
    }
    let parts: Vec<PromptBytePart<'_>> = declarations
        .iter()
        .map(|(identity, text)| PromptBytePart { identity, text })
        .collect();
    let framed = FramedPrompt {
        source_identity: "purrassist-toolgraph",
        prefix: "<tools>\n",
        separator: "\n",
        suffix: "\n</tools>",
        parts: &parts,
    };
    let bytes = framed
        .assemble_bytes(max_bytes, governor)
        .map_err(|error| match error.reason {
            PromptByteFailure::ByteCeiling {
                required_bytes,
                max_bytes,
            } => PromptError::ByteCeiling {
                required_bytes,
                max_bytes,
            },
            PromptByteFailure::Resource(stop) => PromptError::Resource(stop),
            PromptByteFailure::LengthOverflow => PromptError::LengthOverflow,
        })?;

    let mut by_name: Vec<_> = graph.tools().iter().collect();
    by_name.sort_by(|left, right| left.name.cmp(&right.name));
    let mut ledger = LossLedger::new();
    for tool in by_name {
        // The declaration carries the description, so that row is not a loss.
        let mut rows = LossLedger::new();
        record_tool(&mut rows, tool, target);
        for entry in rows.entries() {
            if entry.code != "description-not-expressible" {
                ledger.record(entry.clone());
            }
        }
        ledger.record(entry_for(
            target,
            "output-shape-not-expressible",
            "the declaration carries no output schema; mandatory check validate-output".to_owned(),
            Some((&tool.output_shape, "outputSchema")),
        ));
    }
    // This representation registers no code for a dropped schema construct, so
    // the soundness check below refuses it instead of losing it silently.
    record_upstream(
        &mut ledger,
        &upstream,
        target,
        "schema-keyword-not-expressible",
    );
    check_ledger_sound_in(
        vocabulary.loss_profile(),
        &ledger,
        SOURCE_REPRESENTATION,
        target.representation(),
    )
    .map_err(|cause| PromptError::Unsound(cause.into_owned()))?;
    Ok((
        PromptProjection {
            family,
            text: bytes.text,
            exact_bytes: bytes.exact_bytes,
        },
        ledger,
    ))
}

/// The tool set the declaration renderer reads: each manifest tool without its
/// `outputSchema`, which a Hermes declaration has no place for. The renderer
/// refuses a tool that still carries one, so the omission is explicit here and
/// ledgered by the caller as `output-shape-not-expressible`.
fn declared_tools(manifest: &str, governor: &mut Governor) -> Result<ToolSet, PromptError> {
    let root = json::parse_with(manifest, governor).map_err(PromptError::Json)?;
    let items = match root.get("tools").map(|node| &node.value) {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    let mut array = String::from("[");
    for (position, item) in items.iter().enumerate() {
        let Value::Object(members) = &item.value else {
            continue;
        };
        let kept = members
            .iter()
            .filter(|member| member.key != "outputSchema")
            .cloned()
            .collect();
        if position > 0 {
            array.push(',');
        }
        let text = Node::new(Value::Object(kept))
            .to_json_governed(governor)
            .map_err(PromptError::Resource)?;
        array.push_str(&text);
    }
    array.push(']');
    ToolSet::from_mcp_governed(&array, governor).map_err(PromptError::ToolSet)
}
