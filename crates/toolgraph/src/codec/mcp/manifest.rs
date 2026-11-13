// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The bidirectional MCP Extended manifest codec over a loaded [`ToolGraph`].
//!
//! [`encode`] projects every tool to the standard MCP declaration members plus
//! the graph-aware `_meta` members, and retains the complete source dataset as
//! Turtle in `_meta.security.sourceGraph`. [`decode`] is the inverse: it reads
//! the envelope, loads the retained source through [`ToolGraph::load`], and
//! requires every typed declaration to agree with a fresh projection of that
//! graph. Disagreement is an error; neither side is preferred.
//!
//! The input and output JSON Schemas come from the shared upstream
//! `compile_tool_arguments` plain-argument compiler. Its dropped constructs are
//! carried into the returned [`LossLedger`] unchanged. Everything else in the
//! graph survives in the retained source, so the manifest introduces no losses
//! of its own.

use std::fmt;

use purrassist_core::{
    Governor, StopReason,
    contract::Resource,
    json::{self, Member, Node, ParseFailure, Value},
};
use purrdf_core::loss::{LossLedger, LossSoundnessError, check_ledger_sound_in};
use purrdf_rdf::{RdfDiagnostic, SerializeGraph};
use purrdf_shapes::{
    ShapesError,
    json_schema::{
        ToolArgumentProfile, ToolFieldBinding, ToolSchemaError, ToolSchemaLimits,
        compile_tool_arguments,
    },
    shapes::{Shapes, Target},
    term::Term,
};

use super::{EnvelopeError, ManifestEnvelope, ToolEnvelope, execution};
use crate::{
    ContextCodecError, LOSS_PROFILE_ID, LoadError, MCP_CONTEXT_FORMAT, Tool, ToolAnnotations,
    ToolGraph, Vocabulary,
};

/// The enclosing bundle format; legacy formats are refused rather than migrated.
const FORMAT: &str = "purrassist-toolgraph-v2";
/// The retained source graph's media type.
const MEDIA_TYPE: &str = "text/turtle";
/// The only trust statement a manifest makes: the caller's registry decides.
const POLICY_TRUST: &str = "caller-registry-required";
/// Source and target of the schema-compiler losses this codec carries.
const SCHEMA_FROM: &str = "shacl";
const SCHEMA_TO: &str = "json-schema";

/// An MCP Extended manifest as JSON text.
///
/// The wrapper makes no claim that its text is valid; [`decode`] parses and
/// checks it. Text produced by [`encode`] is byte-deterministic: fixed member
/// order, compact separators and no time- or hash-seed-dependent content.
///
/// ```
/// use purrassist_toolgraph::codec::mcp::ManifestJson;
/// let manifest = ManifestJson::from_text(r#"{"tools":[]}"#);
/// assert_eq!(manifest.as_str(), r#"{"tools":[]}"#);
/// assert_eq!(manifest.into_string(), r#"{"tools":[]}"#);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestJson {
    text: String,
}

impl ManifestJson {
    /// Wrap manifest text without parsing or validating it.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// The manifest text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Take the manifest text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.text
    }
}

/// A refusal to encode a tool graph or decode a manifest. No partial result escapes.
#[derive(Debug)]
#[non_exhaustive]
pub enum CodecError {
    /// The manifest text is not strict JSON, or a generated fragment could not be parsed.
    Json(ParseFailure),
    /// The envelope has a structural fault, an unsupported format or a legacy field.
    Envelope(EnvelopeError),
    /// A tool's context fragment is refused by the context codec (for example the
    /// legacy `maxOutputTokens` member).
    Context {
        /// Original tool-array index, or the encoded tool's index.
        tool: usize,
        /// The context codec's own refusal.
        error: ContextCodecError,
    },
    /// The retained source graph does not load.
    Load(Box<LoadError>),
    /// The loaded dataset has no admissible SHACL shapes model.
    Shapes(Box<ShapesError>),
    /// The plain-argument schema compiler refused a tool's shape.
    Schema {
        /// The tool's name.
        tool: String,
        /// `inputSchema` or `outputSchema`.
        member: &'static str,
        /// The upstream compiler's refusal.
        error: Box<ToolSchemaError>,
    },
    /// The source dataset could not be written as Turtle.
    Serialize(Box<RdfDiagnostic>),
    /// A recorded loss is not registered in the caller's loss profile.
    UnsoundLoss(LossSoundnessError),
    /// A typed declaration member does not agree with the retained source graph.
    Disagreement {
        /// Declaration index, or `None` for an envelope member.
        tool: Option<usize>,
        /// The first disagreeing member, for example `inputSchema` or
        /// `_meta.executionProfile`.
        member: &'static str,
    },
    /// The caller refused further work or cancelled the operation.
    Resource(StopReason),
}

impl fmt::Display for CodecError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(cause) => write!(output, "manifest JSON: {cause}"),
            Self::Envelope(cause) => write!(output, "manifest envelope: {cause}"),
            Self::Context { tool, error } => {
                write!(output, "tool at index {tool}: contextManagement: {error}")
            }
            Self::Load(cause) => write!(output, "retained source graph: {cause}"),
            Self::Shapes(cause) => write!(output, "shapes model: {cause}"),
            Self::Schema {
                tool,
                member,
                error,
            } => write!(output, "tool {tool}: {member}: {error}"),
            Self::Serialize(cause) => write!(output, "source graph serialization: {cause}"),
            Self::UnsoundLoss(cause) => write!(output, "unregistered loss: {cause}"),
            Self::Disagreement {
                tool: Some(index),
                member,
            } => write!(
                output,
                "tool at index {index}: {member} disagrees with the source graph"
            ),
            Self::Disagreement { tool: None, member } => {
                write!(output, "{member} disagrees with the source graph")
            }
            Self::Resource(cause) => write!(output, "manifest codec stopped: {cause:?}"),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(cause) => Some(cause),
            Self::Envelope(cause) => Some(cause),
            Self::Context { error, .. } => Some(error),
            Self::Load(cause) => Some(cause.as_ref()),
            Self::Shapes(cause) => Some(cause.as_ref()),
            Self::Schema { error, .. } => Some(error.as_ref()),
            Self::Serialize(cause) => Some(cause.as_ref()),
            Self::UnsoundLoss(cause) => Some(cause),
            Self::Resource(cause) => Some(cause),
            Self::Disagreement { .. } => None,
        }
    }
}

/// Encode a loaded graph as an MCP Extended manifest.
///
/// Every tool, in the graph's IRI order, emits `name`, `description`,
/// `inputSchema`, `outputSchema`, `annotations` (only the hints the source
/// states), `_meta.security`, `_meta.executionProfile` and
/// `_meta.contextManagement`. The enclosing `_meta.security` declares format
/// `purrassist-toolgraph-v2`, the vocabulary's prefix table, loss profile
/// `purrassist-toolgraph-v1` and the complete source dataset as Turtle, so the
/// original statements, reifiers and unprojected nodes survive. Output byte
/// limits keep their exact unit: `maxOutputBytes` with
/// `outputEnforcement: "exact-bytes"`.
///
/// The returned ledger holds exactly the losses the upstream schema compiler
/// reported, each checked against the vocabulary's loss profile; it is empty
/// when every constraint has a plain JSON Schema spelling.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{ToolGraph, codec::mcp};
/// # use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
/// # use purrdf_core::loss::LossProfile;
/// # let namespace = "https://example.org/toolgraph#";
/// # let vocabulary = Vocabulary::new("pa", PrefixTable(vec![("pa".into(), namespace.into())]),
/// #     TermBindings { namespace_iri: namespace.into(), local_names: TERM_NAMES.iter()
/// #         .map(|name| ((*name).into(), format!("{namespace}{name}"))).collect() },
/// #     register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let graph = ToolGraph::load(b"", "text/turtle", None, &vocabulary, &mut governor).unwrap();
/// let (manifest, losses) = mcp::encode(&graph, &vocabulary, &mut governor).unwrap();
/// assert!(manifest.as_str().starts_with(r#"{"tools":[],"_meta":{"security":{"format":"purrassist-toolgraph-v2""#));
/// assert!(losses.is_empty());
/// let (decoded, _) = mcp::decode(&manifest, &vocabulary, &mut governor).unwrap();
/// assert!(decoded.tools().is_empty());
/// ```
///
/// # Errors
/// Returns [`CodecError`] when a tool's shapes have no plain-argument schema,
/// a context fragment is refused, a recorded loss is not registered, the
/// dataset cannot be written, or the governor stops the operation. Nothing is
/// returned on failure.
pub fn encode(
    graph: &ToolGraph,
    vocabulary: &Vocabulary,
    governor: &mut Governor,
) -> Result<(ManifestJson, LossLedger), CodecError> {
    let (tools, ledger) = project(graph, vocabulary, governor)?;
    let bytes = purrdf_rdf::serialize_dataset(
        graph.dataset().as_ref(),
        MEDIA_TYPE,
        SerializeGraph::Dataset,
    )
    .map_err(|cause| CodecError::Serialize(Box::new(cause)))?;
    let source = String::from_utf8(bytes).map_err(|cause| {
        CodecError::Serialize(Box::new(RdfDiagnostic::error(
            "source-graph-encoding",
            cause.to_string(),
        )))
    })?;
    let security = object([
        ("format", string(FORMAT)),
        ("mediaType", string(MEDIA_TYPE)),
        ("prefixes", prefixes(vocabulary)),
        ("sourceGraph", string(&source)),
        ("lossProfile", string(LOSS_PROFILE_ID)),
    ]);
    let manifest = object([
        ("tools", Node::new(Value::Array(tools))),
        ("_meta", object([("security", security)])),
    ]);
    let text = manifest
        .to_json_governed(governor)
        .map_err(CodecError::Resource)?;
    Ok((ManifestJson { text }, ledger))
}

/// Decode an MCP Extended manifest into the tool graph its retained source defines.
///
/// The manifest is parsed as strict JSON and its envelope is read: the bundle
/// must declare format `purrassist-toolgraph-v2`, `text/turtle` and loss
/// profile `purrassist-toolgraph-v1`, and every tool's context fragment must
/// pass the context codec, so a legacy `maxOutputTokens` member is refused and
/// never read as bytes. The prefix table must equal the vocabulary's. The
/// retained source is loaded with [`ToolGraph::load`] and every declaration,
/// in order, must equal a fresh projection of the loaded graph; the first
/// disagreeing member is reported.
///
/// The ledger holds the schema-compiler losses of that projection, checked
/// against the vocabulary's loss profile. For a manifest produced by
/// [`encode`] from graph `g`, the decoded graph has the same tools and the
/// same dataset as `g`, and the ledger equals the one [`encode`] returned.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::codec::mcp::{self, ManifestJson};
/// # use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
/// # use purrdf_core::loss::LossProfile;
/// # let namespace = "https://example.org/toolgraph#";
/// # let vocabulary = Vocabulary::new("pa", PrefixTable(vec![("pa".into(), namespace.into())]),
/// #     TermBindings { namespace_iri: namespace.into(), local_names: TERM_NAMES.iter()
/// #         .map(|name| ((*name).into(), format!("{namespace}{name}"))).collect() },
/// #     register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let legacy = ManifestJson::from_text(
///     r#"{"tools":[],"_meta":{"security":{"format":"purrassist-toolgraph-v1","mediaType":"text/turtle","prefixes":{},"sourceGraph":"","lossProfile":"purrassist-toolgraph-v1"}}}"#,
/// );
/// let refused = mcp::decode(&legacy, &vocabulary, &mut governor);
/// assert!(matches!(refused, Err(mcp::CodecError::Envelope(_))));
/// ```
///
/// # Errors
/// Returns [`CodecError`] for invalid JSON, a structural, versioning or
/// legacy-field fault, a prefix table that differs from the vocabulary's, a
/// source graph that does not load, any declaration member that disagrees with
/// the loaded graph, an unregistered loss, or a stopped governor.
pub fn decode(
    manifest: &ManifestJson,
    vocabulary: &Vocabulary,
    governor: &mut Governor,
) -> Result<(ToolGraph, LossLedger), CodecError> {
    let root = json::parse_with(manifest.as_str(), governor).map_err(CodecError::Json)?;
    let envelope = ManifestEnvelope::read(&root, governor).map_err(CodecError::Envelope)?;
    let expected = prefixes(vocabulary);
    if !envelope.prefixes().equivalent(&expected) {
        return Err(CodecError::Disagreement {
            tool: None,
            member: "prefixes",
        });
    }
    let mut declarations = Vec::with_capacity(envelope.declarations().len());
    for index in 0..envelope.declarations().len() {
        let tool = envelope
            .tool(index, governor)
            .map_err(CodecError::Envelope)?
            .ok_or(CodecError::Disagreement {
                tool: None,
                member: "tools",
            })?;
        tool.decode_context(governor)
            .map_err(|error| CodecError::Context { tool: index, error })?;
        declarations.push(tool);
    }
    let graph = ToolGraph::load(
        envelope.source_graph().as_bytes(),
        MEDIA_TYPE,
        None,
        vocabulary,
        governor,
    )
    .map_err(|cause| CodecError::Load(Box::new(cause)))?;
    let (projected, ledger) = project(&graph, vocabulary, governor)?;
    if projected.len() != declarations.len() {
        return Err(CodecError::Disagreement {
            tool: None,
            member: "tools",
        });
    }
    for (index, (expected, declared)) in projected.iter().zip(&declarations).enumerate() {
        agree(index, expected, declared)?;
    }
    Ok((graph, ledger))
}

/// Project every tool and merge the schema compiler's ledgers, checking soundness.
fn project(
    graph: &ToolGraph,
    vocabulary: &Vocabulary,
    governor: &mut Governor,
) -> Result<(Vec<Node>, LossLedger), CodecError> {
    let mut ledger = LossLedger::new();
    let mut tools = Vec::with_capacity(graph.tools().len());
    if !graph.tools().is_empty() {
        let shapes = purrdf_shapes::shapes::from_dataset(graph.dataset())
            .map_err(|cause| CodecError::Shapes(Box::new(cause)))?;
        let budget = governor.limits();
        let limits = ToolSchemaLimits {
            work: budget.work,
            bytes: budget.scratch_bytes,
            nodes: budget.work,
        };
        for (index, tool) in graph.tools().iter().enumerate() {
            governor
                .charge(Resource::Work, 1)
                .map_err(CodecError::Resource)?;
            let context = Context {
                shapes: &shapes,
                vocabulary,
                limits: &limits,
            };
            tools.push(project_tool(index, tool, &context, governor, &mut ledger)?);
        }
    }
    check_ledger_sound_in(vocabulary.loss_profile(), &ledger, SCHEMA_FROM, SCHEMA_TO)
        .map_err(|cause| CodecError::UnsoundLoss(cause.into_owned()))?;
    Ok((tools, ledger))
}

/// Shared inputs of one tool projection.
struct Context<'a> {
    shapes: &'a Shapes,
    vocabulary: &'a Vocabulary,
    limits: &'a ToolSchemaLimits,
}

fn project_tool(
    index: usize,
    tool: &Tool,
    context: &Context<'_>,
    governor: &mut Governor,
    ledger: &mut LossLedger,
) -> Result<Node, CodecError> {
    let input = schema(
        tool,
        "inputSchema",
        &tool.input_shape,
        tool.parameters
            .iter()
            .map(|parameter| (&parameter.predicate, &parameter.implementation_property)),
        context,
        governor,
        ledger,
    )?;
    let output = schema(
        tool,
        "outputSchema",
        &tool.output_shape,
        std::iter::once((&tool.output.predicate, &tool.output.implementation_property)),
        context,
        governor,
        ledger,
    )?;
    let recovery_hints = tool
        .parameters
        .iter()
        .filter_map(|parameter| parameter.hint.as_ref())
        .map(|hint| string(&hint.reifier))
        .collect();
    let security = object([
        ("graphBinding", string(&tool.iri)),
        ("parameterConstraints", string(&tool.parameter_constraints)),
        (
            "subtools",
            Node::new(Value::Array(
                tool.subtools.iter().map(|iri| string(iri)).collect(),
            )),
        ),
        ("recoveryHints", Node::new(Value::Array(recovery_hints))),
        ("policyTrust", string(POLICY_TRUST)),
    ]);
    let context_fragment = tool
        .context
        .encode_mcp(Some(MCP_CONTEXT_FORMAT), governor)
        .map_err(|error| CodecError::Context { tool: index, error })?;
    Ok(object([
        ("name", string(&tool.name)),
        ("description", string(&tool.description)),
        ("inputSchema", input),
        ("outputSchema", output),
        ("annotations", annotations(&tool.annotations)),
        (
            "_meta",
            object([
                ("security", security),
                (
                    "executionProfile",
                    execution::profile_node(&tool.execution_profile),
                ),
                ("contextManagement", context_fragment),
            ]),
        ),
    ]))
}

/// Compile one shape to a plain JSON Schema node, recording its upstream losses.
fn schema<'a>(
    tool: &Tool,
    member: &'static str,
    shape: &str,
    fields: impl Iterator<Item = (&'a String, &'a String)>,
    context: &Context<'_>,
    governor: &mut Governor,
    ledger: &mut LossLedger,
) -> Result<Node, CodecError> {
    let refuse = |error| CodecError::Schema {
        tool: tool.name.clone(),
        member,
        error: Box::new(error),
    };
    let class = root_class(context.shapes, shape).ok_or_else(|| {
        refuse(ToolSchemaError::InvalidRoot {
            shape: shape.to_owned(),
        })
    })?;
    let profile = ToolArgumentProfile {
        root_shape_iri: shape.to_owned(),
        root_class_iri: class,
        fields: fields
            .map(|(predicate, property)| ToolFieldBinding {
                parent_shape_iri: shape.to_owned(),
                predicate_iri: predicate.clone(),
                implementation_property: property.clone(),
            })
            .collect(),
    };
    let compiled = compile_tool_arguments(
        context.shapes,
        context.vocabulary.namespaces(),
        &profile,
        context.limits,
    )
    .map_err(refuse)?;
    for entry in compiled.losses.entries() {
        ledger.record(entry.clone());
    }
    json::parse_with(&compiled.schema_json, governor).map_err(CodecError::Json)
}

/// The one named class a node shape targets; no class is inferred otherwise.
fn root_class(shapes: &Shapes, shape: &str) -> Option<String> {
    let found = shapes
        .node_shapes
        .iter()
        .find(|candidate| matches!(&candidate.id, Term::NamedNode(id) if id.as_str() == shape))?;
    let mut classes = found.targets.iter().filter_map(|target| match target {
        Target::Class(class) => Some(class.as_str()),
        _ => None,
    });
    let class = classes.next()?;
    classes.next().is_none().then(|| class.to_owned())
}

/// Require a declaration to equal the projection of the loaded graph, member by member.
fn agree(index: usize, expected: &Node, declared: &ToolEnvelope<'_>) -> Result<(), CodecError> {
    for (member, path) in MEMBERS {
        let wanted = lookup(expected, path);
        let found = lookup(declared.original(), path);
        let equal = match (wanted, found) {
            (Some(wanted), Some(found)) => wanted.equivalent(found),
            _ => false,
        };
        if !equal {
            return Err(CodecError::Disagreement {
                tool: Some(index),
                member,
            });
        }
    }
    Ok(())
}

/// Declaration members compared by [`agree`], with their JSON paths. The envelope
/// reader refuses unknown members, so no other member can differ.
const MEMBERS: [(&str, &[&str]); 8] = [
    ("name", &["name"]),
    ("description", &["description"]),
    ("inputSchema", &["inputSchema"]),
    ("outputSchema", &["outputSchema"]),
    ("annotations", &["annotations"]),
    ("_meta.security", &["_meta", "security"]),
    ("_meta.executionProfile", &["_meta", "executionProfile"]),
    ("_meta.contextManagement", &["_meta", "contextManagement"]),
];

fn lookup<'a>(node: &'a Node, path: &[&str]) -> Option<&'a Node> {
    path.iter().try_fold(node, |at, key| at.get(key))
}

fn annotations(source: &ToolAnnotations) -> Node {
    let mut members = Vec::new();
    if let Some(title) = &source.title {
        members.push(("title", string(title)));
    }
    for (key, hint) in [
        ("readOnlyHint", source.read_only_hint),
        ("destructiveHint", source.destructive_hint),
        ("idempotentHint", source.idempotent_hint),
        ("openWorldHint", source.open_world_hint),
    ] {
        if let Some(value) = hint {
            members.push((key, Node::new(Value::Bool(value))));
        }
    }
    object(members)
}

/// The vocabulary's prefix table in its stored lexical order.
fn prefixes(vocabulary: &Vocabulary) -> Node {
    object(
        vocabulary
            .prefixes()
            .0
            .iter()
            .map(|(prefix, iri)| (prefix.as_str(), string(iri))),
    )
}

fn string(text: &str) -> Node {
    Node::new(Value::String(text.to_owned()))
}

fn object<'a>(members: impl IntoIterator<Item = (&'a str, Node)>) -> Node {
    Node::new(Value::Object(
        members
            .into_iter()
            .map(|(key, value)| Member {
                key: key.to_owned(),
                key_span: None,
                value,
            })
            .collect(),
    ))
}
