// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The `CompactSignature` lane: one TypeScript-like line per tool.
//!
//! The signature is derived from the same input and output JSON Schemas the MCP
//! Extended manifest carries, so it cannot disagree with them. A line is
//! `name(param:type,optional?:type)->result` followed by one LF, and lines are
//! ordered by tool name bytes. Types are `string`, `integer`, `number`,
//! `boolean`, `null`, `T[]` (a union element is parenthesized), `{k:T}` for an
//! object, and `A|B` for a union. A name outside the core name grammar
//! (`is_valid_name`) is a JSON string literal.
//!
//! Everything else a schema says is omitted from the line and recorded on the
//! returned ledger, never approximated. Object detail nested deeper than the
//! caller's depth renders as `object` and is ledgered the same way.

use std::borrow::Cow;
use std::fmt;

use purrassist_core::{
    Governor, StopReason,
    contract::Resource,
    json::{self, Node, ParseFailure, Value},
    tool_call::is_valid_name,
};
use purrdf_core::{
    diagnostic::RdfLocation,
    loss::{LossEntry, LossLedger, LossSoundnessError, check_ledger_sound_in},
};

use crate::{
    ProjectionTarget, SOURCE_REPRESENTATION, Tool, ToolGraph, Vocabulary,
    codec::mcp::{self, CodecError},
};

/// A validated, owned compact signature document.
///
/// One line per tool, each ending in LF; an empty graph yields the empty text.
/// Its representation identity is `compact-signature-v1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactSignature(String);

impl CompactSignature {
    /// The signature text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Take the owned signature text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// A refusal to project a graph to compact signatures. Nothing partial escapes.
#[derive(Debug)]
#[non_exhaustive]
pub enum CompactError {
    /// The graph has no MCP Extended manifest projection to derive schemas from.
    Manifest(Box<CodecError>),
    /// The projected manifest could not be read back as JSON.
    Json(ParseFailure),
    /// A schema construct has no signature form: a `$ref`, a type that names no
    /// supported type, or a schema with neither `type` nor `anyOf`/`oneOf`.
    Unsupported {
        /// The tool's name.
        tool: String,
        /// RFC 6901 pointer from the schema root.
        pointer: String,
        /// What the signature cannot represent.
        reason: &'static str,
    },
    /// A recorded loss is not registered in the caller's loss profile.
    Unsound(LossSoundnessError),
    /// The caller refused further work or cancelled the operation.
    Resource(StopReason),
}

impl fmt::Display for CompactError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(cause) => write!(output, "compact signature manifest: {cause}"),
            Self::Json(cause) => write!(output, "compact signature schema JSON: {cause}"),
            Self::Unsupported {
                tool,
                pointer,
                reason,
            } => write!(output, "tool {tool}: {pointer}: {reason}"),
            Self::Unsound(cause) => write!(output, "unregistered loss: {cause}"),
            Self::Resource(cause) => write!(output, "compact signature stopped: {cause:?}"),
        }
    }
}

impl std::error::Error for CompactError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(cause) => Some(cause.as_ref()),
            Self::Json(cause) => Some(cause),
            _ => None,
        }
    }
}

/// Project every tool of `graph` to its compact signature line.
///
/// `depth` is the number of object levels expanded inside parameter and result
/// types. A result object is level one, an object-valued parameter is level
/// one, and an object inside it is level two; an object past `depth` renders as
/// `object` and the omission is ledgered at its exact schema pointer.
///
/// The ledger holds one `LossLedger` entry per omitted source construct under
/// the registered representation `compact-signature-v1`, in this order for each
/// tool (tools in name order): `authorization-not-expressible`,
/// `session-binding-not-expressible`, `taint-not-expressible`,
/// `recovery-hints-not-expressible`, `confinement-not-expressible`,
/// `resource-ceiling-not-expressible`, `context-contract-not-expressible`,
/// `tool-annotations-not-expressible`, `rdf-binding-not-expressible`,
/// `description-not-expressible`, then `schema-keyword-not-expressible` for the
/// input and output shape. Hint rows are located at the hint's reifier and the
/// other rows at the tool or shape IRI. Each note names the mandatory check
/// that enforces the omitted constraint. A construct the upstream schema
/// compiler already dropped is carried as a `schema-keyword-not-expressible`
/// entry quoting its note. The ledger is checked against the vocabulary's loss
/// profile before it is returned.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{ToolGraph, lanes};
/// # use purrassist_toolgraph::{PrefixTable, TermBindings, Vocabulary, TERM_NAMES, register_loss_profile};
/// # use purrdf_core::loss::LossProfile;
/// # let namespace = "https://example.org/toolgraph#";
/// # let vocabulary = Vocabulary::new("pa", PrefixTable(vec![("pa".into(), namespace.into())]),
/// #     TermBindings { namespace_iri: namespace.into(), local_names: TERM_NAMES.iter()
/// #         .map(|name| ((*name).into(), format!("{namespace}{name}"))).collect() },
/// #     register_loss_profile(LossProfile::builtin()).unwrap()).unwrap();
/// let mut governor = Governor::new(Budget::default());
/// let graph = ToolGraph::load(b"", "text/turtle", None, &vocabulary, &mut governor).unwrap();
/// let (signature, losses) = lanes::compact(&graph, &vocabulary, 2, &mut governor).unwrap();
/// assert_eq!(signature.as_str(), "");
/// assert!(losses.is_empty());
/// ```
///
/// # Errors
/// [`CompactError::Manifest`] when the graph has no manifest projection (for
/// example a shape without exactly one `sh:targetClass`),
/// [`CompactError::Unsupported`] for a schema construct without a signature
/// form (a `$ref`, or a schema with no `type`, `anyOf` or `oneOf`),
/// [`CompactError::Unsound`] when a ledger entry is not registered, and
/// [`CompactError::Resource`] when the governor stops the operation. Nothing is
/// returned on failure.
pub fn compact(
    graph: &ToolGraph,
    vocabulary: &Vocabulary,
    depth: u64,
    governor: &mut Governor,
) -> Result<(CompactSignature, LossLedger), CompactError> {
    let (manifest, upstream) = mcp::encode(graph, vocabulary, governor)
        .map_err(|cause| CompactError::Manifest(Box::new(cause)))?;
    let root = json::parse_with(manifest.as_str(), governor).map_err(CompactError::Json)?;
    let declarations = match root.get("tools").map(|node| &node.value) {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    let mut order: Vec<usize> = (0..graph.tools().len()).collect();
    order.sort_by(|&left, &right| graph.tools()[left].name.cmp(&graph.tools()[right].name));
    let mut text = String::new();
    let mut ledger = LossLedger::new();
    for index in order {
        let tool = &graph.tools()[index];
        let declaration = declarations.get(index);
        let mut input = Lowering::new(&tool.name, depth);
        let mut output = Lowering::new(&tool.name, depth);
        let schema = |member: &'static str| {
            declaration
                .and_then(|node| node.get(member))
                .ok_or_else(|| CompactError::Unsupported {
                    tool: tool.name.clone(),
                    pointer: format!("/{member}"),
                    reason: "the manifest declaration has no such schema",
                })
        };
        let parameters = input.parameters(schema("inputSchema")?, governor)?;
        let result = output.result(schema("outputSchema")?, governor)?;
        text.push_str(&identifier(&tool.name, governor)?);
        text.push('(');
        text.push_str(&parameters);
        text.push_str(")->");
        text.push_str(&result);
        text.push('\n');
        record_tool(&mut ledger, tool, &input, &output);
    }
    for entry in upstream.entries() {
        let note = format!(
            "upstream schema compiler omitted a construct ({}): {}",
            entry.code, entry.note
        );
        ledger.record(LossEntry {
            location: entry.location.clone(),
            ..entry_for("schema-keyword-not-expressible", note, None)
        });
    }
    check_ledger_sound_in(
        vocabulary.loss_profile(),
        &ledger,
        SOURCE_REPRESENTATION,
        ProjectionTarget::CompactSignature.representation(),
    )
    .map_err(|cause| CompactError::Unsound(cause.into_owned()))?;
    Ok((CompactSignature(text), ledger))
}

/// One schema lowering and the keywords it left out.
struct Lowering<'a> {
    tool: &'a str,
    depth: u64,
    /// Pointers of omitted constructs, in schema order.
    omitted: Vec<String>,
}

impl<'a> Lowering<'a> {
    const fn new(tool: &'a str, depth: u64) -> Self {
        Self {
            tool,
            depth,
            omitted: Vec::new(),
        }
    }

    fn unsupported(&self, pointer: &str, reason: &'static str) -> CompactError {
        CompactError::Unsupported {
            tool: self.tool.to_owned(),
            pointer: pointer.to_owned(),
            reason,
        }
    }

    /// The parameter list of an input schema, which is always expanded.
    fn parameters(
        &mut self,
        schema: &Node,
        governor: &mut Governor,
    ) -> Result<String, CompactError> {
        self.require_object_type(schema, "")?;
        self.note_uncarried(schema, "", &["type", "properties", "required"]);
        self.members(schema, "", 0, governor)
    }

    /// The result type of an output schema.
    fn result(&mut self, schema: &Node, governor: &mut Governor) -> Result<String, CompactError> {
        self.lower(schema, "", 1, governor)
    }

    fn require_object_type(&self, schema: &Node, pointer: &str) -> Result<(), CompactError> {
        match schema.get("type").map(|node| &node.value) {
            Some(Value::String(name)) if name == "object" => Ok(()),
            _ => Err(self.unsupported(pointer, "an input schema must have type object")),
        }
    }

    /// `name:type,optional?:type` for an object schema's properties.
    fn members(
        &mut self,
        schema: &Node,
        pointer: &str,
        level: u64,
        governor: &mut Governor,
    ) -> Result<String, CompactError> {
        let required: Vec<&str> = match schema.get("required").map(|node| &node.value) {
            Some(Value::Array(items)) => items.iter().filter_map(Node::as_str).collect(),
            _ => Vec::new(),
        };
        let Some(Value::Object(properties)) = schema.get("properties").map(|node| &node.value)
        else {
            return Ok(String::new());
        };
        let mut parts = Vec::with_capacity(properties.len());
        for member in properties {
            let child = format!(
                "{pointer}/properties/{}",
                purrassist_core::json::pointer_segment(&member.key)
            );
            let mut part = identifier(&member.key, governor)?;
            if !required.contains(&member.key.as_str()) {
                part.push('?');
            }
            part.push(':');
            part.push_str(&self.lower(&member.value, &child, level + 1, governor)?);
            parts.push(part);
        }
        Ok(parts.join(","))
    }

    /// The signature type of one schema node, `level` being its object nesting.
    fn lower(
        &mut self,
        schema: &Node,
        pointer: &str,
        level: u64,
        governor: &mut Governor,
    ) -> Result<String, CompactError> {
        governor
            .charge(Resource::Work, 1)
            .map_err(CompactError::Resource)?;
        if schema.get("$ref").is_some() {
            return Err(self.unsupported(pointer, "a schema reference has no signature form"));
        }
        let alternatives = ["anyOf", "oneOf"]
            .into_iter()
            .filter_map(|key| schema.get(key).map(|node| (key, node)))
            .collect::<Vec<_>>();
        let mut names: Vec<String> = Vec::new();
        match (
            alternatives.as_slice(),
            schema.get("type").map(|n| &n.value),
        ) {
            ([(key, node)], _) => {
                let Value::Array(items) = &node.value else {
                    return Err(self.unsupported(pointer, "anyOf and oneOf must be arrays"));
                };
                for (position, item) in items.iter().enumerate() {
                    let child = format!("{pointer}/{key}/{position}");
                    names.push(self.lower(item, &child, level, governor)?);
                }
                self.note_uncarried(schema, pointer, &["anyOf", "oneOf"]);
            }
            ([], Some(Value::String(name))) => {
                names.push(self.named(name, schema, pointer, level, governor)?);
                self.note_type(&[name.as_str()], schema, pointer, level);
            }
            ([], Some(Value::Array(types))) => {
                let mut spelled = Vec::with_capacity(types.len());
                for item in types {
                    let Value::String(name) = &item.value else {
                        return Err(self.unsupported(pointer, "a type name must be a string"));
                    };
                    names.push(self.named(name, schema, pointer, level, governor)?);
                    spelled.push(name.as_str());
                }
                self.note_type(&spelled, schema, pointer, level);
            }
            ([], _) => {
                return Err(self.unsupported(pointer, "a schema without type, anyOf or oneOf"));
            }
            _ => return Err(self.unsupported(pointer, "anyOf and oneOf together")),
        }
        names.dedup();
        Ok(names.join("|"))
    }

    fn named(
        &mut self,
        name: &str,
        schema: &Node,
        pointer: &str,
        level: u64,
        governor: &mut Governor,
    ) -> Result<String, CompactError> {
        match name {
            "string" | "integer" | "number" | "boolean" | "null" => Ok(name.to_owned()),
            "array" => {
                let Some(items) = schema.get("items") else {
                    return Err(self.unsupported(pointer, "an array schema without items"));
                };
                let element = self.lower(items, &format!("{pointer}/items"), level, governor)?;
                Ok(if element.contains('|') {
                    format!("({element})[]")
                } else {
                    format!("{element}[]")
                })
            }
            "object" if level > self.depth => {
                self.omitted
                    .push(format!("{pointer} (object detail beyond depth)"));
                Ok("object".to_owned())
            }
            "object" => Ok(format!(
                "{{{}}}",
                self.members(schema, pointer, level, governor)?
            )),
            _ => Err(self.unsupported(pointer, "an unknown type name")),
        }
    }

    /// Record the keywords of a node whose type names are `names`, given that
    /// only the keywords those types lower are carried.
    fn note_type(&mut self, names: &[&str], schema: &Node, pointer: &str, level: u64) {
        let mut carried = vec!["type"];
        if names.contains(&"object") && level <= self.depth {
            carried.extend(["properties", "required"]);
        }
        if names.contains(&"array") {
            carried.push("items");
        }
        self.note_uncarried(schema, pointer, &carried);
    }

    /// Record every keyword of `schema` that is neither `carried` nor schema
    /// metadata (`$schema`, an empty `$defs`).
    fn note_uncarried(&mut self, schema: &Node, pointer: &str, carried: &[&str]) {
        let Value::Object(members) = &schema.value else {
            return;
        };
        for member in members {
            let key = member.key.as_str();
            let metadata = key == "$schema"
                || (key == "$defs"
                    && matches!(&member.value.value, Value::Object(inner) if inner.is_empty()));
            if !metadata && !carried.contains(&key) {
                self.omitted.push(format!(
                    "{pointer}/{}",
                    purrassist_core::json::pointer_segment(key)
                ));
            }
        }
    }
}

/// `name` bare when it matches the core name grammar
/// ([`purrassist_core::tool_call::is_valid_name`]), otherwise a JSON string literal.
fn identifier(name: &str, governor: &mut Governor) -> Result<String, CompactError> {
    if is_valid_name(name) {
        return Ok(name.to_owned());
    }
    Node::new(Value::String(name.to_owned()))
        .to_json_governed(governor)
        .map_err(CompactError::Resource)
}

/// Builds one ledger entry: a code, a note and an optional subject location.
fn entry_for(code: &'static str, note: String, subject: Option<(&str, &'static str)>) -> LossEntry {
    LossEntry {
        code: Cow::Borrowed(code),
        from: Cow::Borrowed(SOURCE_REPRESENTATION),
        to: Cow::Borrowed(ProjectionTarget::CompactSignature.representation()),
        note: Cow::Owned(note),
        location: subject.map(|(iri, member)| {
            Box::new(RdfLocation {
                subject: Some(iri.to_owned()),
                logical: Some(member.to_owned()),
                ..RdfLocation::default()
            })
        }),
    }
}

/// The ledger rows of one tool, in the documented order.
fn record_tool(ledger: &mut LossLedger, tool: &Tool, input: &Lowering<'_>, output: &Lowering<'_>) {
    let mut row = |code, note: &str, iri: &str, member| {
        ledger.record(entry_for(code, note.to_owned(), Some((iri, member))));
    };
    let hints = || tool.parameters.iter().filter_map(|p| p.hint.as_ref());
    if !tool.parameter_constraints.is_empty() {
        row(
            "authorization-not-expressible",
            "parameterConstraints are not enforced by the signature; mandatory check authorize-call",
            &tool.iri,
            "parameterConstraints",
        );
    }
    for hint in hints().filter(|hint| hint.session_binding.is_some()) {
        row(
            "session-binding-not-expressible",
            "the session binding is not enforced by the signature; mandatory check bind-session",
            &hint.reifier,
            "sessionBinding",
        );
    }
    for hint in hints().filter(|hint| !hint.taints.is_empty()) {
        row(
            "taint-not-expressible",
            "taint labels are not enforced by the signature; mandatory check taint-policy",
            &hint.reifier,
            "taint",
        );
    }
    for hint in hints() {
        row(
            "recovery-hints-not-expressible",
            "recovery tolerances are not carried by the signature; mandatory check recovery-tolerance",
            &hint.reifier,
            "recoveryHint",
        );
    }
    let profile = &tool.execution_profile.iri;
    row(
        "confinement-not-expressible",
        "the execution profile is not enforced by the signature; mandatory check confine-worker",
        profile,
        "executionProfile",
    );
    row(
        "resource-ceiling-not-expressible",
        "the five resource ceilings are not enforced by the signature; mandatory check confine-worker",
        profile,
        "resourceCeilings",
    );
    row(
        "context-contract-not-expressible",
        "the output projection, byte cap and retention are not carried by the signature; mandatory check project-context",
        &tool.iri,
        "contextManagement",
    );
    let annotations = &tool.annotations;
    if annotations.title.is_some()
        || annotations.read_only_hint.is_some()
        || annotations.destructive_hint.is_some()
        || annotations.idempotent_hint.is_some()
        || annotations.open_world_hint.is_some()
    {
        row(
            "tool-annotations-not-expressible",
            "the title and behavior hints are not carried by the signature; mandatory check verify-effects",
            &tool.iri,
            "annotations",
        );
    }
    row(
        "rdf-binding-not-expressible",
        "the FnO mappings and source reifiers are not carried by the signature; mandatory check bind-manifest",
        &tool.iri,
        "fnoBinding",
    );
    if !tool.description.is_empty() {
        row(
            "description-not-expressible",
            "the description is not carried by the signature; mandatory check bind-manifest",
            &tool.iri,
            "description",
        );
    }
    record_schemas(ledger, tool, input, output);
}

/// The `schema-keyword-not-expressible` rows of a tool's input and output shape.
fn record_schemas(
    ledger: &mut LossLedger,
    tool: &Tool,
    input: &Lowering<'_>,
    output: &Lowering<'_>,
) {
    for (lowering, shape, member, check) in [
        (input, &tool.input_shape, "inputSchema", "validate-input"),
        (
            output,
            &tool.output_shape,
            "outputSchema",
            "validate-output",
        ),
    ] {
        if !lowering.omitted.is_empty() {
            let note = format!(
                "schema constructs omitted from the signature at {}; mandatory check {check}",
                lowering.omitted.join(", ")
            );
            ledger.record(entry_for(
                "schema-keyword-not-expressible",
                note,
                Some((shape, member)),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use purrassist_core::{Budget, Governor, json};

    use super::{CompactError, Lowering};

    fn lower(schema: &str, depth: u64) -> (Result<String, CompactError>, Vec<String>) {
        let mut governor = Governor::new(Budget::default());
        let node = json::parse_with(schema, &mut governor).expect("schema JSON");
        let mut lowering = Lowering::new("t", depth);
        let result = lowering.result(&node, &mut governor);
        (result, lowering.omitted)
    }

    // Invariant: unions, arrays of unions and optional members keep their
    // documented spelling, names in the core name grammar stay bare and others are quoted, and a keyword the line cannot carry is ledgered by pointer.
    #[test]
    fn unions_arrays_and_optional_members_keep_their_spelling_and_uncarried_keywords_are_recorded()
    {
        let (line, omitted) = lower(
            r#"{"type":"object","required":["a"],"properties":{"a":{"type":["string","null"]},"b c":{"type":"array","items":{"anyOf":[{"type":"integer"},{"type":"string","maxLength":3}]}},"fs.read_file":{"type":"string"},"read-file":{"type":"string"}}}"#,
            1,
        );
        assert_eq!(
            line.unwrap(),
            r#"{a:string|null,"b c"?:(integer|string)[],fs.read_file?:string,read-file?:string}"#
        );
        assert_eq!(omitted, ["/properties/b c/items/anyOf/1/maxLength"]);
    }

    // Invariant: object detail beyond the caller's depth is never silently cut;
    // it renders as `object` and the cut pointer, with its dropped keywords, is ledgered.
    #[test]
    fn object_detail_beyond_the_depth_is_ledgered_at_its_pointer() {
        let schema = r#"{"type":"object","properties":{"o":{"type":"object","properties":{"x":{"type":"string"}}}}}"#;
        let (line, omitted) = lower(schema, 1);
        assert_eq!(line.unwrap(), "{o?:object}");
        assert_eq!(
            omitted,
            [
                "/properties/o (object detail beyond depth)",
                "/properties/o/properties"
            ]
        );
        let (line, omitted) = lower(schema, 2);
        assert_eq!(line.unwrap(), "{o?:{x?:string}}");
        assert_eq!(omitted, Vec::<String>::new());
    }

    // Invariant: a schema reference has no signature form and is a typed refusal,
    // not a truncated description.
    #[test]
    fn a_schema_reference_is_a_typed_refusal() {
        let (line, _) = lower(
            r##"{"type":"object","properties":{"r":{"$ref":"#/$defs/r"}}}"##,
            2,
        );
        assert!(matches!(line, Err(CompactError::Unsupported { .. })));
    }
}
