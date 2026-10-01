// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Strict, lossless adapters from wire tool declarations to tool sets.
//!
//! [`ToolSet::from_openai`], [`ToolSet::from_mcp`], and
//! [`ToolSet::from_anthropic`] (and their `_governed` variants) parse
//! declarations with the strict JSON parser and keep, per tool:
//!
//! 1. the exact source bytes of its declaration object and the byte spans,
//!    in the original input, of that object and of its parameters schema
//!    ([`ToolDeclaration`]);
//! 2. a typed projection into [`super::ArgumentShape`];
//! 3. a [`crate::schema::Schema`] compiled from the *exact* parameters text
//!    slice, never from a re-rendering, so the complete arguments of a call
//!    are always validated by the Draft 2020-12 engine against what the
//!    model was shown; and
//! 4. the `(schema pointer, keyword)` list of every keyword the typed model
//!    does not represent ([`super::CompiledTool::unmodeled_keywords`]).
//!
//! Rendering back in the source form ([`super::CompiledTool::render_declaration`])
//! returns the original declaration bytes exactly; unknown declaration
//! fields (`strict`, `annotations`, `title`, vendor fields, ...) and unknown
//! schema keywords are never dropped.
//!
//! # Accepted input
//!
//! * `OpenAI`: an array of `{"type":"function","function":{"name",
//!   "description"?, "parameters"?, "strict"?, ...}}` declarations (the
//!   request's `tools` value), or a single such declaration.
//! * MCP: a `tools/list` result object `{"tools":[...], ...}` (an object
//!   with a `tools` member and no `name` member), an array of tools, or a
//!   single `{"name", "description"?, "inputSchema", ...}` tool.
//! * Anthropic: an array of `{"name", "description"?, "input_schema", ...}`
//!   tools, or a single such tool.
//!
//! A declaration without a parameters schema accepts any arguments object:
//! its effective schema is `{"type":"object"}`, which
//! [`super::CompiledTool::json_schema`] reports, while
//! [`ToolDeclaration::parameters_span`] is `None` and same-form rendering
//! still returns the original declaration, with no parameters member.
//!
//! Unparseable JSON, a wrong top-level shape, a non-object declaration, an
//! `OpenAI` declaration whose `type` is not `"function"` or whose `function`
//! is not an object, a missing or non-string `name`, a non-string
//! `description`, a duplicated `type`/`function`/`name`/`description`/
//! parameters member, a parameters schema that is not a JSON object, whose
//! root `type` (when present) is not `"object"`, or that the Draft 2020-12
//! engine rejects, an invalid tool or parameter name, and any collision are
//! hard [`ToolSetError::InvalidDefinition`] or [`ToolSetError::Collision`]
//! errors; no declaration is ever skipped. Errors raised before a
//! declaration's name is known report an empty `tool` and the declaration's
//! zero-based index in `reason`. Tool and parameter names follow the same
//! syntax and collision rules as [`super::ToolDefinition`] tools.
//!
//! # Typed projection
//!
//! The typed model represents `type` (`string`, `integer`, `number`,
//! `boolean`, `object`, `array`), string `enum`s, `properties`, `required`,
//! object-form `items`, string `pattern`, `default`, `description`, and
//! boolean `additionalProperties`. A parameter becomes
//! [`super::ValueShape::Unmodeled`] — validated only by the exact schema and
//! treated by repair as an opaque JSON value — when its schema is a boolean
//! schema, uses any of `anyOf`, `oneOf`, `allOf`, `not`, `if`, `then`,
//! `else`, `$ref`, `$dynamicRef`, `const`, `prefixItems`, or
//! `dependentSchemas`, has an `enum` that is not a non-empty list of distinct
//! strings under an absent or `"string"` type, or has no single supported
//! `type` string (absent, `"null"`, or a type array such as
//! `["string","null"]`). An array without object-form `items` has
//! [`super::ValueShape::Unmodeled`] items. Adapters create no aliases.
//! Declared `default`s are validated against their exact subschema (resolved
//! within the complete parameters schema, so `$ref`s work) and retained with
//! source spans relative to [`super::CompiledTool::json_schema`]; a
//! nonconforming default is an invalid declaration.
//!
//! # Unmodeled keywords
//!
//! The walk visits the root schema, every member of a projected object's
//! `properties`, and a projected array's object-form `items`; it does not
//! descend into an unmodeled value or into the value of any unmodeled
//! keyword (both are retained verbatim). At each visited schema object, in
//! member order and parents before children, a member is listed unless the
//! projection carries it: `type` unless the value is unmodeled; `enum` for
//! an enumeration; `pattern` for a plain string; object-form `items` for an
//! array; object-form `properties` and boolean `additionalProperties` for an
//! object; `required` for an object when every name it lists is a declared
//! property; `description` except at the root; and `default` only on a
//! parameter (neither the root nor an array item carries one). Everything
//! else — `format`, `minLength`, `$defs`, `title`, `x-*` vendor keywords,
//! and so on — is listed. Pointers are RFC 6901 pointers relative to the
//! parameters schema.
//!
//! # Effects and unknown parameters
//!
//! Wire declarations do not state effects, so every adapter-built tool is
//! [`Effects::Mutating`] with no explicit policy, hence
//! [`super::RepairPolicy::Structural`]; the policy is resolved from effects
//! whenever it is read. Unknown-parameter handling is derived from the exact
//! schema, the final authority, rather than from effects: an object scope
//! with `additionalProperties: false` or `unevaluatedProperties: false`, and
//! none of the keywords above or `patternProperties` that could admit other
//! members, is [`UnknownParameters::Error`]; every other scope (absent,
//! `true`, or a schema) is [`UnknownParameters::PassThrough`], because the
//! exact schema permits extra properties and dropping or rejecting them
//! would contradict it.
//!
//! # Resources
//!
//! Parsing, every retained text copy, the projection, keyword records, and
//! schema compilation are charged to the governor with portable amounts;
//! exhaustion is [`ToolSetError::Resource`] and is sticky on the governor.
use super::definition::{
    Effects, ObjectSpec, ParameterKind, ParameterSpec, ToolDefinition, UnknownParameters,
};
use super::toolset::{
    Declared, PARAMETER_RECORD, Pending, ToolSet, ToolSetError, compiled, invalid, retain_text,
    sort_factor,
};
use crate::contract::{
    Budget, Governor, Resource, ScratchCharge, ScratchVec, saturating_size as size,
};
use crate::json::{self, Member, Node, ParseFailure, Value, pointer_segment};
use crate::schema::Schema;
use core::ops::Range;

/// The effective parameters schema of a declaration that has none.
const NO_PARAMETERS: &str = r#"{"type":"object"}"#;

/// Keywords whose presence makes a parameter's value unmodeled.
const RESHAPING: [&str; 12] = [
    "$dynamicRef",
    "$ref",
    "allOf",
    "anyOf",
    "const",
    "dependentSchemas",
    "else",
    "if",
    "not",
    "oneOf",
    "prefixItems",
    "then",
];

/// Portable charge for one retained `(String, String)` keyword record.
const KEYWORD_RECORD: u64 = 48;

/// A wire format of tool declarations.
///
/// Canonical (non-source) renderings are compact JSON:
/// `{"type":"function","function":{"name":..,"description":..,"parameters":..}}`
/// for `OpenAI`, `{"name":..,"description":..,"inputSchema":..}` for MCP, and
/// `{"name":..,"description":..,"input_schema":..}` for Anthropic. The
/// description appears only when present. `OpenAI` omits `parameters` for a
/// tool declared without one; MCP and Anthropic, which require a schema,
/// render `{"type":"object"}`.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{DeclarationForm, ToolSet}};
/// let tools = ToolSet::from_openai(r#"[{"type":"function","function":{"name":"ls"}}]"#, Budget::default()).unwrap();
/// assert_eq!(
///     tools.render_declaration("ls", DeclarationForm::Anthropic).unwrap(),
///     r#"{"name":"ls","input_schema":{"type":"object"}}"#,
/// );
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclarationForm {
    /// `OpenAI` Chat Completions `tools[]` function declarations.
    OpenAi,
    /// Model Context Protocol `tools/list` tools.
    Mcp,
    /// Anthropic Messages API client tools.
    Anthropic,
}

impl DeclarationForm {
    const fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Mcp => "MCP",
            Self::Anthropic => "Anthropic",
        }
    }

    const fn parameters_key(self) -> &'static str {
        match self {
            Self::OpenAi => "parameters",
            Self::Mcp => "inputSchema",
            Self::Anthropic => "input_schema",
        }
    }
}

/// The exact source declaration of an adapter-built tool.
///
/// Spans are half-open byte ranges of the complete adapter input; the text
/// is the input bytes of [`Self::span`].
///
/// ```
/// use purrassist_core::{Budget, tool_call::{DeclarationForm, ToolSet, ToolSource}};
/// let input = r#"[ {"name":"ls","input_schema":{"type":"object"}} ]"#;
/// let tools = ToolSet::from_anthropic(input, Budget::default()).unwrap();
/// let ToolSource::Declaration(declaration) = tools.tool("ls").unwrap().source() else { unreachable!() };
/// assert_eq!(declaration.form(), DeclarationForm::Anthropic);
/// assert_eq!(&input[declaration.span()], declaration.text());
/// assert_eq!(declaration.parameters_text(), Some(r#"{"type":"object"}"#));
/// assert_eq!(&input[declaration.parameters_span().unwrap()], r#"{"type":"object"}"#);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDeclaration {
    form: DeclarationForm,
    text: String,
    span: Range<usize>,
    parameters: Option<Range<usize>>,
}

impl ToolDeclaration {
    /// The wire form the declaration was read from.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{DeclarationForm, ToolSet, ToolSource}};
    /// let tools = ToolSet::from_mcp(r#"{"tools":[{"name":"ls","inputSchema":{}}]}"#, Budget::default()).unwrap();
    /// let ToolSource::Declaration(declaration) = tools.tool("ls").unwrap().source() else { unreachable!() };
    /// assert_eq!(declaration.form(), DeclarationForm::Mcp);
    /// ```
    #[must_use]
    pub const fn form(&self) -> DeclarationForm {
        self.form
    }

    /// The exact declaration object bytes, including every field.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, ToolSource}};
    /// let input = r#"{"name":"ls","inputSchema":{},"annotations":{"readOnlyHint":true}}"#;
    /// let tools = ToolSet::from_mcp(input, Budget::default()).unwrap();
    /// let ToolSource::Declaration(declaration) = tools.tool("ls").unwrap().source() else { unreachable!() };
    /// assert_eq!(declaration.text(), input);
    /// ```
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The declaration object's byte range in the adapter input.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, ToolSource}};
    /// let tools = ToolSet::from_mcp(r#" [{"name":"ls"}]"#, Budget::default()).unwrap();
    /// let ToolSource::Declaration(declaration) = tools.tool("ls").unwrap().source() else { unreachable!() };
    /// assert_eq!(declaration.span(), 2..15);
    /// ```
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// The parameters schema's byte range in the adapter input, or `None`
    /// when the declaration has no parameters schema.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, ToolSource}};
    /// let tools = ToolSet::from_mcp(r#"{"name":"ls"}"#, Budget::default()).unwrap();
    /// let ToolSource::Declaration(declaration) = tools.tool("ls").unwrap().source() else { unreachable!() };
    /// assert_eq!(declaration.parameters_span(), None);
    /// ```
    #[must_use]
    pub fn parameters_span(&self) -> Option<Range<usize>> {
        self.parameters.clone()
    }

    /// The exact parameters schema text, or `None` when absent.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, ToolSource}};
    /// let tools = ToolSet::from_openai(
    ///     r#"{"type":"function","function":{"name":"ls","parameters":{ "type" : "object" }}}"#,
    ///     Budget::default(),
    /// ).unwrap();
    /// let ToolSource::Declaration(declaration) = tools.tool("ls").unwrap().source() else { unreachable!() };
    /// assert_eq!(declaration.parameters_text(), Some(r#"{ "type" : "object" }"#));
    /// ```
    #[must_use]
    pub fn parameters_text(&self) -> Option<&str> {
        self.parameters
            .as_ref()
            .map(|range| &self.text[range.start - self.span.start..range.end - self.span.start])
    }
}

/// Where a compiled tool came from.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet, ToolSource}};
/// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
/// assert_eq!(tools.tool("ls").unwrap().source(), &ToolSource::Builder);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolSource {
    /// A typed [`ToolDefinition`]; its schema is the canonical rendering.
    Builder,
    /// An exact wire declaration read by an adapter.
    Declaration(ToolDeclaration),
}

impl ToolSet {
    /// Adapt `OpenAI` function declarations under a fresh governor.
    ///
    /// # Errors
    /// See [`Self::from_openai_governed`].
    ///
    /// ```
    /// use purrassist_core::{Budget, json, tool_call::{DeclarationForm, ToolSet}};
    /// let input = r#"[{"type":"function","function":{"name":"read","strict":true,
    ///   "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}}]"#;
    /// let tools = ToolSet::from_openai(input, Budget::default()).unwrap();
    /// let read = tools.tool("read").unwrap();
    /// assert_eq!(tools.render_declaration("read", DeclarationForm::OpenAi).unwrap(), &input[1..input.len() - 1]);
    /// assert!(read.schema().validate(&json::parse(r#"{"path":"a"}"#).unwrap()).unwrap().valid);
    /// ```
    pub fn from_openai(json: &str, budget: Budget) -> Result<Self, ToolSetError> {
        Self::from_openai_governed(json, &mut Governor::new(budget))
    }

    /// Adapt `OpenAI` function declarations against a caller-owned governor.
    ///
    /// Accepts an array of `{"type":"function","function":{..}}` objects or
    /// a single one; see the module documentation for every rule.
    ///
    /// # Errors
    /// [`ToolSetError::InvalidDefinition`] or [`ToolSetError::Collision`]
    /// for input that is not strict JSON or violates the declaration
    /// contract, and [`ToolSetError::Resource`] when the governor is
    /// exhausted or cancelled (sticky on `governor`).
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, tool_call::{ToolSet, ToolSetError}};
    /// let mut governor = Governor::new(Budget::default());
    /// assert!(matches!(
    ///     ToolSet::from_openai_governed(r#"{"type":"function"}"#, &mut governor),
    ///     Err(ToolSetError::InvalidDefinition { .. })
    /// ));
    /// ```
    pub fn from_openai_governed(json: &str, governor: &mut Governor) -> Result<Self, ToolSetError> {
        adapt(json, DeclarationForm::OpenAi, governor)
    }

    /// Adapt MCP tools under a fresh governor.
    ///
    /// # Errors
    /// See [`Self::from_mcp_governed`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, UnknownParameters}};
    /// let tools = ToolSet::from_mcp(
    ///     r#"{"tools":[{"name":"echo","inputSchema":{"type":"object","properties":{"text":{"type":"string"}}}}],"nextCursor":"2"}"#,
    ///     Budget::default(),
    /// ).unwrap();
    /// assert_eq!(tools.tool("echo").unwrap().unknown_parameters(), UnknownParameters::PassThrough);
    /// ```
    pub fn from_mcp(json: &str, budget: Budget) -> Result<Self, ToolSetError> {
        Self::from_mcp_governed(json, &mut Governor::new(budget))
    }

    /// Adapt MCP tools against a caller-owned governor.
    ///
    /// Accepts a `tools/list` result object, an array of tools, or a single
    /// tool; see the module documentation for every rule.
    ///
    /// # Errors
    /// As [`Self::from_openai_governed`].
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, tool_call::{ToolSet, ToolSetError}};
    /// let mut governor = Governor::new(Budget::default());
    /// assert!(matches!(
    ///     ToolSet::from_mcp_governed(r#"{"tools":{}}"#, &mut governor),
    ///     Err(ToolSetError::InvalidDefinition { .. })
    /// ));
    /// ```
    pub fn from_mcp_governed(json: &str, governor: &mut Governor) -> Result<Self, ToolSetError> {
        adapt(json, DeclarationForm::Mcp, governor)
    }

    /// Adapt Anthropic tools under a fresh governor.
    ///
    /// # Errors
    /// See [`Self::from_anthropic_governed`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Effects, RepairPolicy, ToolSet}};
    /// let tools = ToolSet::from_anthropic(r#"[{"name":"ls","input_schema":{"type":"object"}}]"#, Budget::default()).unwrap();
    /// let ls = tools.tool("ls").unwrap();
    /// assert_eq!((ls.effects(), ls.policy()), (Effects::Mutating, RepairPolicy::Structural));
    /// ```
    pub fn from_anthropic(json: &str, budget: Budget) -> Result<Self, ToolSetError> {
        Self::from_anthropic_governed(json, &mut Governor::new(budget))
    }

    /// Adapt Anthropic tools against a caller-owned governor.
    ///
    /// Accepts an array of tools or a single tool; see the module
    /// documentation for every rule.
    ///
    /// # Errors
    /// As [`Self::from_openai_governed`].
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, StopReason, contract::Resource, tool_call::{ToolSet, ToolSetError}};
    /// let mut governor = Governor::new(Budget { work: 3, ..Budget::default() });
    /// assert_eq!(
    ///     ToolSet::from_anthropic_governed(r#"{"name":"ls"}"#, &mut governor).unwrap_err(),
    ///     ToolSetError::Resource(StopReason::BudgetExceeded(Resource::Work)),
    /// );
    /// ```
    pub fn from_anthropic_governed(
        json: &str,
        governor: &mut Governor,
    ) -> Result<Self, ToolSetError> {
        adapt(json, DeclarationForm::Anthropic, governor)
    }
}

/// Compact canonical declaration of `form`; `parameters` is embedded
/// verbatim.
pub(super) fn render_canonical(
    form: DeclarationForm,
    name: &str,
    description: Option<&str>,
    parameters: Option<&str>,
) -> String {
    let mut fields = format!("\"name\":{}", json::quote(name));
    if let Some(description) = description {
        fields.push_str(",\"description\":");
        fields.push_str(&json::quote(description));
    }
    let schema = match form {
        DeclarationForm::OpenAi => parameters,
        DeclarationForm::Mcp | DeclarationForm::Anthropic => {
            Some(parameters.unwrap_or(NO_PARAMETERS))
        }
    };
    if let Some(schema) = schema {
        fields.push_str(",\"");
        fields.push_str(form.parameters_key());
        fields.push_str("\":");
        fields.push_str(schema);
    }
    match form {
        DeclarationForm::OpenAi => format!("{{\"type\":\"function\",\"function\":{{{fields}}}}}"),
        DeclarationForm::Mcp | DeclarationForm::Anthropic => format!("{{{fields}}}"),
    }
}

fn adapt(
    input: &str,
    form: DeclarationForm,
    governor: &mut Governor,
) -> Result<ToolSet, ToolSetError> {
    governor.check().map_err(ToolSetError::Resource)?;
    let root = json::parse_with(input, governor).map_err(|failure| match failure {
        ParseFailure::Syntax(error) => invalid(
            "",
            format!("{} declarations are not strict JSON: {error}", form.label()),
        ),
        ParseFailure::Resource(reason) => ToolSetError::Resource(reason),
    })?;
    let declarations = top_level(&root, form)?;
    let mut pending: ScratchVec<Pending> = ScratchVec::new(governor);
    for (index, node) in declarations.iter().enumerate() {
        governor
            .charge(Resource::Work, 1)
            .map_err(ToolSetError::Resource)?;
        pending
            .push(declare(input, form, index, node, governor)?)
            .map_err(ToolSetError::Resource)?;
    }
    ToolSet::assemble(pending, governor)
}

/// The declaration nodes of a parsed adapter input.
fn top_level(root: &Node, form: DeclarationForm) -> Result<&[Node], ToolSetError> {
    match &root.value {
        Value::Array(items) => Ok(items),
        Value::Object(_)
            if form == DeclarationForm::Mcp
                && field(root, "tools", "", "the tools/list result")?.is_some()
                && field(root, "name", "", "the tools/list result")?.is_none() =>
        {
            match root.get("tools").map(|tools| &tools.value) {
                Some(Value::Array(items)) => Ok(items),
                _ => Err(invalid(
                    "",
                    "the MCP tools/list result's `tools` member must be an array".into(),
                )),
            }
        }
        Value::Object(_) => Ok(core::slice::from_ref(root)),
        _ => Err(invalid(
            "",
            format!(
                "{} declarations must be an array or a single declaration object",
                form.label()
            ),
        )),
    }
}

/// The unique member `key` of `node`, `None` when absent; a duplicated
/// member is an invalid declaration of `tool` described by `owner`.
fn field<'n>(
    node: &'n Node,
    key: &str,
    tool: &str,
    owner: &str,
) -> Result<Option<&'n Node>, ToolSetError> {
    let Value::Object(members) = &node.value else {
        return Ok(None);
    };
    let mut found = members.iter().filter(|member| member.key == key);
    let first = found.next();
    if found.next().is_some() {
        return Err(invalid(
            tool,
            format!("{owner} has a duplicate `{key}` member"),
        ));
    }
    Ok(first.map(|member| &member.value))
}

/// The original byte range of a parsed node.
fn span_of(node: &Node, tool: &str) -> Result<Range<usize>, ToolSetError> {
    node.span
        .clone()
        .ok_or_else(|| invalid(tool, "a declaration node has no source span".into()))
}

/// Read one declaration into a pending tool.
fn declare(
    input: &str,
    form: DeclarationForm,
    index: usize,
    node: &Node,
    governor: &mut Governor,
) -> Result<Pending, ToolSetError> {
    let owner = format!("declaration {index}");
    if !matches!(node.value, Value::Object(_)) {
        return Err(invalid("", format!("{owner} must be a JSON object")));
    }
    let fields = if form == DeclarationForm::OpenAi {
        if field(node, "type", "", &owner)?.and_then(Node::as_str) != Some("function") {
            return Err(invalid("", format!("{owner}: `type` must be \"function\"")));
        }
        match field(node, "function", "", &owner)? {
            Some(
                function @ Node {
                    value: Value::Object(_),
                    ..
                },
            ) => function,
            _ => {
                return Err(invalid(
                    "",
                    format!("{owner}: `function` must be an object"),
                ));
            }
        }
    } else {
        node
    };
    let Some(name) = field(fields, "name", "", &owner)?.and_then(Node::as_str) else {
        return Err(invalid("", format!("{owner}: `name` must be a string")));
    };
    let description = match field(fields, "description", name, &owner)? {
        None => None,
        Some(Node {
            value: Value::String(text),
            ..
        }) => Some(text.clone()),
        Some(_) => return Err(invalid(name, "`description` must be a string".into())),
    };
    let key = form.parameters_key();
    let parameters = match field(fields, key, name, &owner)? {
        None => None,
        Some(
            schema @ Node {
                value: Value::Object(_),
                ..
            },
        ) => Some(schema),
        Some(_) => {
            return Err(invalid(
                name,
                format!("`{key}` must be a JSON object schema"),
            ));
        }
    };
    let span = span_of(node, name)?;
    let parameters_span = parameters.map(|schema| span_of(schema, name)).transpose()?;
    let schema_text = parameters_span
        .clone()
        .map_or(NO_PARAMETERS, |range| &input[range]);
    retain_text(governor, span.len().saturating_add(schema_text.len()))?;
    governor
        .charge(Resource::ScratchBytes, size(Declared::CHARGE))
        .map_err(ToolSetError::Resource)?;
    let schema = compiled(
        name,
        "parameters schema",
        Schema::compile_governed(schema_text, governor),
    )?;
    let mut projector = Projector {
        tool: name,
        governor,
        unmodeled: Vec::new(),
    };
    // Project the compiled document (the exact parameters text, parsed with
    // spans relative to it) so retained defaults never depend on where the
    // declaration sat in the input.
    let arguments = match parameters {
        Some(_) => projector.root(schema.document())?,
        None => ObjectSpec {
            parameters: Vec::new(),
            unknown: Some(UnknownParameters::PassThrough),
        },
    };
    let unmodeled_keywords = projector.unmodeled;
    Ok(Pending {
        definition: ToolDefinition {
            name: name.to_owned(),
            aliases: Vec::new(),
            description,
            effects: Effects::Mutating,
            policy: None,
            arguments,
        },
        declaration: Some(Box::new(Declared {
            source: ToolDeclaration {
                form,
                text: input[span.clone()].to_owned(),
                span,
                parameters: parameters_span,
            },
            schema_text: schema_text.to_owned(),
            schema,
            unmodeled_keywords,
        })),
    })
}

/// Where a schema object sits in the projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    Root,
    Parameter,
    Item,
}

/// The projected kind of a visited schema object, before construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Unmodeled,
    String,
    Integer,
    Number,
    Boolean,
    Enumeration,
    Array,
    /// An object; `required` records whether every required name is a
    /// declared property.
    Object {
        required: bool,
    },
}

/// Governed projection of an exact parameters schema into typed specs,
/// collecting unmodeled keywords on the way.
struct Projector<'a> {
    tool: &'a str,
    governor: &'a mut Governor,
    unmodeled: Vec<(String, String)>,
}

impl Projector<'_> {
    fn charge(&mut self, resource: Resource, amount: u64) -> Result<(), ToolSetError> {
        self.governor
            .charge(resource, amount)
            .map_err(ToolSetError::Resource)
    }

    /// Charge one visited schema object at `depth`.
    fn enter(&mut self, node: &Node, pointer: &str, depth: u32) -> Result<(), ToolSetError> {
        let members = match &node.value {
            Value::Object(members) => members.len(),
            _ => 0,
        };
        self.charge(
            Resource::Work,
            size(members.saturating_add(pointer.len()).saturating_add(1)),
        )?;
        self.charge(Resource::Depth, u64::from(depth))?;
        self.charge(Resource::ScratchBytes, size(PARAMETER_RECORD))
    }

    /// Project the root parameters schema, which must describe an object.
    fn root(&mut self, node: &Node) -> Result<ObjectSpec, ToolSetError> {
        // Every string, default, and key the projection copies is part of
        // this tree, so its estimate bounds those copies.
        self.charge(Resource::ScratchBytes, node.estimated_bytes())?;
        self.enter(node, "", 1)?;
        if node
            .get("type")
            .is_some_and(|kind| kind.as_str() != Some("object"))
        {
            return Err(invalid(
                self.tool,
                "the parameters schema must describe an object: its `type` must be \"object\" when present"
                    .into(),
            ));
        }
        let Value::Object(members) = &node.value else {
            return Err(invalid(
                self.tool,
                "the parameters schema must be a JSON object".into(),
            ));
        };
        let tag = Tag::Object {
            required: self.required_declared(node)?,
        };
        self.note(members, "", Position::Root, tag)?;
        self.object(node, members, "", 2)
    }

    /// Project the schema at a parameter or item position.
    fn spec(
        &mut self,
        node: &Node,
        pointer: &str,
        position: Position,
        depth: u32,
    ) -> Result<ParameterSpec, ToolSetError> {
        self.enter(node, pointer, depth)?;
        let Value::Object(members) = &node.value else {
            return Ok(ParameterSpec::of(ParameterKind::Unmodeled));
        };
        let enumeration = node
            .get("enum")
            .map(|values| self.string_enum(values))
            .transpose()?;
        let type_name = node.get("type").and_then(Node::as_str);
        let reshaped = members
            .iter()
            .any(|member| RESHAPING.contains(&member.key.as_str()));
        let tag = match (reshaped, &enumeration, type_name) {
            (false, Some(Some(_)), None | Some("string")) => Tag::Enumeration,
            (false, None, Some("string")) => Tag::String,
            (false, None, Some("integer")) => Tag::Integer,
            (false, None, Some("number")) => Tag::Number,
            (false, None, Some("boolean")) => Tag::Boolean,
            (false, None, Some("array")) => Tag::Array,
            (false, None, Some("object")) => Tag::Object {
                required: self.required_declared(node)?,
            },
            _ => Tag::Unmodeled,
        };
        self.note(members, pointer, position, tag)?;
        let kind = match tag {
            Tag::Unmodeled => ParameterKind::Unmodeled,
            Tag::String => ParameterKind::String,
            Tag::Integer => ParameterKind::Integer,
            Tag::Number => ParameterKind::Number,
            Tag::Boolean => ParameterKind::Boolean,
            Tag::Enumeration => {
                ParameterKind::Enumeration(enumeration.flatten().unwrap_or_default())
            }
            Tag::Array => ParameterKind::Array(Box::new(match node.get("items") {
                Some(
                    items @ Node {
                        value: Value::Object(_),
                        ..
                    },
                ) => self.spec(
                    items,
                    &format!("{pointer}/items"),
                    Position::Item,
                    depth.saturating_add(1),
                )?,
                _ => ParameterSpec::of(ParameterKind::Unmodeled),
            })),
            Tag::Object { .. } => ParameterKind::Object(self.object(
                node,
                members,
                pointer,
                depth.saturating_add(1),
            )?),
        };
        let mut spec = ParameterSpec::of(kind);
        if position != Position::Root {
            spec.description = node
                .get("description")
                .and_then(Node::as_str)
                .map(str::to_owned);
        }
        if position == Position::Parameter {
            spec.default = node.get("default").cloned();
        }
        if tag == Tag::String {
            spec.pattern = node
                .get("pattern")
                .and_then(Node::as_str)
                .map(str::to_owned);
        }
        Ok(spec)
    }

    /// Project an object scope's properties and unknown-member handling.
    fn object(
        &mut self,
        node: &Node,
        members: &[Member],
        pointer: &str,
        depth: u32,
    ) -> Result<ObjectSpec, ToolSetError> {
        let required: Vec<&str> = match node.get("required").map(|names| &names.value) {
            Some(Value::Array(names)) => names.iter().filter_map(Node::as_str).collect(),
            _ => Vec::new(),
        };
        let mut parameters = Vec::new();
        if let Some(Value::Object(properties)) = node.get("properties").map(|node| &node.value) {
            self.charge(
                Resource::Work,
                size(properties.len()).saturating_mul(size(required.len())),
            )?;
            for property in properties {
                let child = format!("{pointer}/properties/{}", pointer_segment(&property.key));
                let mut spec = self.spec(&property.value, &child, Position::Parameter, depth)?;
                spec.required = required.contains(&property.key.as_str());
                parameters.push((property.key.clone(), spec));
            }
        }
        let closed = ["additionalProperties", "unevaluatedProperties"]
            .iter()
            .any(|keyword| {
                node.get(keyword)
                    .is_some_and(|value| value.value == Value::Bool(false))
            });
        let admitting = members.iter().any(|member| {
            member.key == "patternProperties" || RESHAPING.contains(&member.key.as_str())
        });
        Ok(ObjectSpec {
            parameters,
            unknown: Some(if closed && !admitting {
                UnknownParameters::Error
            } else {
                UnknownParameters::PassThrough
            }),
        })
    }

    /// Whether every name `required` lists is a declared property.
    fn required_declared(&mut self, node: &Node) -> Result<bool, ToolSetError> {
        let Some(required) = node.get("required") else {
            return Ok(true);
        };
        let (
            Value::Array(names),
            Some(Node {
                value: Value::Object(properties),
                ..
            }),
        ) = (&required.value, node.get("properties"))
        else {
            return Ok(false);
        };
        self.charge(
            Resource::Work,
            size(names.len()).saturating_mul(size(properties.len())),
        )?;
        Ok(names.iter().all(|name| {
            name.as_str()
                .is_some_and(|name| properties.iter().any(|property| property.key == name))
        }))
    }

    /// The values of an `enum` the typed model represents: a non-empty list
    /// of distinct strings; `None` otherwise.
    fn string_enum(&mut self, values: &Node) -> Result<Option<Vec<String>>, ToolSetError> {
        let Value::Array(items) = &values.value else {
            return Ok(None);
        };
        let Some(strings) = items
            .iter()
            .map(Node::as_str)
            .collect::<Option<Vec<&str>>>()
        else {
            return Ok(None);
        };
        let bytes = strings.iter().fold(0_u64, |total, value| {
            total.saturating_add(size(value.len()))
        });
        self.charge(
            Resource::Work,
            bytes.saturating_mul(sort_factor(strings.len())),
        )?;
        let mut sorted = strings.clone();
        sorted.sort_unstable();
        if sorted.is_empty() || sorted.windows(2).any(|pair| pair[0] == pair[1]) {
            return Ok(None);
        }
        Ok(Some(strings.into_iter().map(str::to_owned).collect()))
    }

    /// Record every member of one visited schema object that the
    /// projection of kind `tag` at `position` does not carry.
    fn note(
        &mut self,
        members: &[Member],
        pointer: &str,
        position: Position,
        tag: Tag,
    ) -> Result<(), ToolSetError> {
        for member in members {
            if represented(member, position, tag) {
                continue;
            }
            self.charge(
                Resource::ScratchBytes,
                size(pointer.len().saturating_add(member.key.len())).saturating_add(KEYWORD_RECORD),
            )?;
            self.unmodeled
                .push((pointer.to_owned(), member.key.clone()));
        }
        Ok(())
    }
}

/// Whether the projection of kind `tag` at `position` carries `member`.
fn represented(member: &Member, position: Position, tag: Tag) -> bool {
    let object_form = matches!(member.value.value, Value::Object(_));
    match member.key.as_str() {
        "type" => tag != Tag::Unmodeled,
        "description" => position != Position::Root && member.value.as_str().is_some(),
        "default" => position == Position::Parameter,
        "enum" => tag == Tag::Enumeration,
        "pattern" => tag == Tag::String && member.value.as_str().is_some(),
        "items" => tag == Tag::Array && object_form,
        "properties" => matches!(tag, Tag::Object { .. }) && object_form,
        "required" => tag == Tag::Object { required: true },
        "additionalProperties" => {
            matches!(tag, Tag::Object { .. }) && matches!(member.value.value, Value::Bool(_))
        }
        _ => false,
    }
}
