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
//!    are always validated against what the model was shown, under the
//!    dialect its `$schema` selects (Draft 2020-12 by default, or Draft-07
//!    for `http://json-schema.org/draft-07/schema#`, common in MCP
//!    servers); and
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
//! root `type` (when present) is not `"object"`, or that the schema engine
//! rejects (including a `$schema` naming any other dialect), an invalid tool or parameter name, and any collision are
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
//! treated by repair as an opaque JSON value, except that XML text under it
//! (Qwen3-Coder) is typed by the exact schema itself
//! (`tool_call.value.schema-typed`) — when its schema is a boolean
//! schema, uses any of `anyOf`, `oneOf`, `allOf`, `not`, `if`, `then`,
//! `else`, `$ref`, `$dynamicRef`, `const`, `prefixItems`, or
//! `dependentSchemas`, has an `enum` that is not a non-empty list of distinct
//! strings under an absent or `"string"` type, or has no single supported
//! `type` string (absent, `"null"`, or a type array such as
//! `["string","null"]`). An array without object-form `items` (including a
//! Draft-07 tuple-form `items` array) has [`super::ValueShape::Unmodeled`]
//! items. Adapters create no aliases.
//!
//! The member names an unmodeled value's exact schema declares are kept
//! beside the typed shape, as a scope graph ([`super::ScopeGraph`], one
//! node per schema location, local `$ref` followed one hop per location and
//! a recursive `$ref` a back-edge by node index; compile cost is charged to
//! the governor and bounded by the schema's size): the Qwen3-Coder XML codec
//! uses it to tell which scope a nested `<parameter=NAME>` opener belongs to.
//! [`super::ParameterShape::nesting`] refers a parameter into it. It carries
//! names only; typing stays with the exact schema.
//!
//! Draft-07 schemas follow Draft-07 rules: a parameter using `dependencies`
//! (whose schema form is `dependentSchemas`) is unmodeled; a nested
//! `$schema` that switches dialect makes its value unmodeled; a `$ref`
//! overrides its siblings, so a root `$ref` is projected from its target
//! alone (see "Root `$ref` and composed roots" below) and every ignored
//! sibling is still listed as an unmodeled keyword; and
//! `unevaluatedProperties`, not a Draft-07 keyword, never closes a scope.
//! `definitions`, `additionalItems`, and `dependencies` are listed as
//! unmodeled keywords like every other keyword the projection does not carry.
//! Declared `default`s are validated against their exact subschema (resolved
//! within the complete parameters schema, so `$ref`s work) and retained with
//! source spans relative to [`super::CompiledTool::json_schema`]; a
//! nonconforming default is an invalid declaration.
//!
//! # Root `$ref` and composed roots
//!
//! The pydantic and `FastMCP` shape puts no `properties` at the root:
//! `{"$ref":"#/definitions/Args","definitions":{"Args":{...}}}` (Draft-07) or
//! `{"$ref":"#/$defs/Args","$defs":{...}}` (2020-12). The projection follows
//! it, so the target's parameters are the tool's parameters:
//!
//! * a root `$ref` to a local pointer (`#/definitions/X` or `#/$defs/X`,
//!   and a target that is itself a `$ref`, transitively) projects the
//!   target object's `properties`, `required`, and `additionalProperties`.
//!   In Draft-07 the root's siblings (`properties`, `type`, ...) are
//!   *ignored for projection*, as they are for validation, and stay listed
//!   as unmodeled keywords beside `$ref` and `definitions`; in Draft 2020-12
//!   the siblings apply with the target, so their parameters are projected
//!   too;
//! * a root `allOf` of exactly one schema that resolves the same way
//!   projects it together with the root;
//! * a root `anyOf` or `oneOf` whose every branch is an object schema (or a
//!   `$ref` to one) projects the union of the branches' names;
//! * a name declared by one applying schema is typed from it. A name a
//!   union branch declares, or that several schemas declare, is unmodeled
//!   with no `required` mark: its typing stays with the exact schema
//!   (`tool_call.value.schema-typed`), as for the unions of the scope
//!   graph, whose node for it is the union of its declarations;
//! * a nested `$ref` member below a projected object goes through the scope
//!   graph as any other, its schema pointer taken from the target.
//!
//! The keywords of the followed schemas that the projection does not carry
//! are listed at their own pointers (for example `/definitions/Args` for a
//! `title` there). An external, missing, or cyclic `$ref`, a target that is
//! not an object schema (`type` other than `"object"`, no `properties`), and
//! a union with a non-object branch are not followed: the projection is then
//! what it is for the root alone (Draft-07: no parameter, unknown
//! parameters pass through, every root keyword listed). A reference the
//! schema engine cannot resolve (external or missing) never gets this far:
//! it makes the declaration invalid. Validation uses the exact declaration
//! and its draft, and rendering back embeds the parameters text verbatim, so
//! neither is affected; the prompt forms print that same text.
//!
//! ```
//! use purrassist_core::{Budget, tool_call::{ToolSet, ValueShape}};
//! let tools = ToolSet::from_mcp(
//!     r##"{"name":"search","inputSchema":{
//!         "$schema":"http://json-schema.org/draft-07/schema#",
//!         "$ref":"#/definitions/Args",
//!         "definitions":{"Args":{"type":"object",
//!             "properties":{"query":{"type":"string"},"limit":{"type":"integer"}},
//!             "required":["query"]}}}}"##,
//!     Budget::default(),
//! ).unwrap();
//! let arguments = &tools.argument_shape("search").unwrap().arguments;
//! assert_eq!(arguments.required, ["query"]);
//! assert_eq!(arguments.parameter("limit").unwrap().value, ValueShape::Integer);
//! ```
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
//! with `additionalProperties: false` or (Draft 2020-12 only)
//! `unevaluatedProperties: false`, and
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
use super::DialectId;
use super::definition::{
    Effects, ObjectSpec, ParameterKind, ParameterSpec, ToolDefinition, UnknownParameters,
};
use super::emit::NO_PARAMETERS;
use super::scope::local_pointer;
use super::toolset::{
    Declared, PARAMETER_RECORD, Pending, RootLocations, ToolSet, ToolSetError, compiled, invalid,
    retain_text, sort_factor,
};
use crate::contract::{
    Budget, Governor, Resource, ScratchCharge, ScratchVec, StopReason, saturating_size as size,
};
use crate::json::{self, Member, Node, ParseFailure, Value, pointer_segment};
use crate::schema::{Dialect, Schema};
use core::ops::Range;
use std::collections::{BTreeMap, BTreeSet};

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

/// Whether `keyword` makes a value unmodeled ([`RESHAPING`]): such a value
/// is typed by the exact schema itself (`tool_call.value.schema-typed`), so
/// the keyword is used exactly when that typing decided one of its values.
pub(super) fn reshaping(keyword: &str) -> bool {
    RESHAPING.contains(&keyword)
}

/// Whether the typed projection repair works from depends on `keyword`
/// when it is listed as unmodeled: a [`RESHAPING`] keyword, Draft-07
/// `dependencies`, or `$schema` makes a value opaque to repair, and
/// `unevaluatedProperties` or `patternProperties` decides how an object
/// scope treats unknown members. Repair consults no other unmodeled
/// keyword; those are enforced only by validation against the exact schema.
pub(super) fn consulted_by_repair(keyword: &str) -> bool {
    RESHAPING.contains(&keyword)
        || matches!(
            keyword,
            "dependencies" | "$schema" | "unevaluatedProperties" | "patternProperties"
        )
}

/// Portable charge for one retained `(String, String)` keyword record.
const KEYWORD_RECORD: u64 = 48;

/// A format of tool declarations: the three wire formats adapters read,
/// and the system-prompt tool entries of the built-in prompt dialects.
///
/// Canonical (non-source) renderings of the wire formats are compact JSON:
/// `{"type":"function","function":{"name":..,"description":..,"parameters":..}}`
/// for `OpenAI`, `{"name":..,"description":..,"inputSchema":..}` for MCP, and
/// `{"name":..,"description":..,"input_schema":..}` for Anthropic. The
/// description appears only when present. `OpenAI` omits `parameters` for a
/// tool declared without one; MCP and Anthropic, which require a schema,
/// render `{"type":"object"}`.
///
/// [`DeclarationForm::Qwen3CoderXml`], [`DeclarationForm::Hermes`],
/// [`DeclarationForm::Llama3Json`], [`DeclarationForm::MistralV3`],
/// [`DeclarationForm::MistralV11`], [`DeclarationForm::DeepSeekV31`],
/// [`DeclarationForm::DeepSeekV3`], [`DeclarationForm::KimiK2`],
/// [`DeclarationForm::Harmony`] and [`DeclarationForm::BareJson`] are
/// printed by their dialect grammar's
/// `declaration` production (see [`emit`](super::emit)): a Qwen3-Coder
/// `<function>` entry
/// `<function><name>NAME</name><description>DESC</description><parameters>SCHEMA</parameters></function>`,
/// a DeepSeek-V3.1 `## Tools` entry
/// `### NAME\nDescription: DESC\n\nParameters: SCHEMA`, a DeepSeek-V3
/// fenced entry (the shape below between a `` ```json `` line and a
/// `` ``` `` line), a Harmony `# Tools` entry
/// `// DESC\ntype NAME = (_: SCHEMA) => any;`, a bare JSON function
/// object `{"name":"NAME","description":"DESC","parameters":SCHEMA}`, and a Hermes
/// `<tools>` entry, Llama 3 function entry, Mistral `[AVAILABLE_TOOLS]`
/// entry or Kimi-K2 `tool_declare` entry (the same shape)
/// `{"type":"function","function":{"name":"NAME","description":"DESC","parameters":SCHEMA}}`.
/// `SCHEMA` is always the exact parameters schema text, verbatim (the
/// chat template's per-parameter XML expansion of the schema is not
/// reproduced: it is lossy for nested schemas, while the exact text is what
/// arguments are validated against).
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
    /// A Qwen3-Coder system-prompt `<function>` entry (`qwen3-coder-xml.v1`).
    Qwen3CoderXml,
    /// A Hermes system-prompt `<tools>` entry (`hermes.v1`).
    Hermes,
    /// A Llama 3.x prompt function entry (`llama3-json.v1`).
    Llama3Json,
    /// A Mistral `[AVAILABLE_TOOLS]` entry (`mistral-v3.v1`).
    MistralV3,
    /// A Mistral v11+ `[AVAILABLE_TOOLS]` entry (`mistral-v11.v1`).
    MistralV11,
    /// A DeepSeek-V3.1 system-prompt `## Tools` entry (`deepseek-v3.1.v1`).
    DeepSeekV31,
    /// A DeepSeek-V3 / R1 fenced JSON tool entry (`deepseek-v3.v1`).
    DeepSeekV3,
    /// A Kimi-K2 `tool_declare` entry (`kimi-k2.v1`).
    KimiK2,
    /// A Harmony developer-message `# Tools` function (`harmony.v1`).
    Harmony,
    /// A bare JSON function object (`bare-json.v1`).
    BareJson,
}

impl DeclarationForm {
    /// Every declaration form.
    ///
    /// ```
    /// use purrassist_core::tool_call::DeclarationForm;
    /// assert_eq!(DeclarationForm::ALL.len(), 13);
    /// ```
    pub const ALL: [Self; 13] = [
        Self::OpenAi,
        Self::Mcp,
        Self::Anthropic,
        Self::Qwen3CoderXml,
        Self::Hermes,
        Self::Llama3Json,
        Self::MistralV3,
        Self::MistralV11,
        Self::DeepSeekV31,
        Self::DeepSeekV3,
        Self::KimiK2,
        Self::Harmony,
        Self::BareJson,
    ];

    /// The dialect whose `declaration` production prints this form, or
    /// `None` for the wire formats.
    ///
    /// ```
    /// use purrassist_core::tool_call::{DeclarationForm, DialectId};
    /// assert_eq!(DeclarationForm::Hermes.dialect(), Some(DialectId::Hermes));
    /// assert_eq!(DeclarationForm::Mcp.dialect(), None);
    /// ```
    #[must_use]
    pub const fn dialect(self) -> Option<DialectId> {
        match self {
            Self::Qwen3CoderXml => Some(DialectId::Qwen3CoderXml),
            Self::Hermes => Some(DialectId::Hermes),
            Self::Llama3Json => Some(DialectId::Llama3Json),
            Self::MistralV3 => Some(DialectId::MistralV3),
            Self::MistralV11 => Some(DialectId::MistralV11),
            Self::DeepSeekV31 => Some(DialectId::DeepSeekV31),
            Self::DeepSeekV3 => Some(DialectId::DeepSeekV3),
            Self::KimiK2 => Some(DialectId::KimiK2),
            Self::Harmony => Some(DialectId::Harmony),
            Self::BareJson => Some(DialectId::BareJson),
            Self::OpenAi | Self::Mcp | Self::Anthropic => None,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Mcp => "MCP",
            Self::Anthropic => "Anthropic",
            Self::Qwen3CoderXml => "Qwen3-Coder XML",
            Self::Hermes => "Hermes",
            Self::Llama3Json => "Llama 3 JSON",
            Self::MistralV3 => "Mistral v3",
            Self::MistralV11 => "Mistral v11",
            Self::DeepSeekV31 => "DeepSeek-V3.1",
            Self::DeepSeekV3 => "DeepSeek-V3",
            Self::KimiK2 => "Kimi-K2",
            Self::Harmony => "Harmony",
            Self::BareJson => "bare JSON",
        }
    }

    const fn parameters_key(self) -> &'static str {
        match self {
            Self::OpenAi
            | Self::Qwen3CoderXml
            | Self::Hermes
            | Self::Llama3Json
            | Self::MistralV3
            | Self::MistralV11
            | Self::DeepSeekV31
            | Self::DeepSeekV3
            | Self::KimiK2
            | Self::Harmony
            | Self::BareJson => "parameters",
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
        adapt(json, DeclarationForm::OpenAi, &[], governor)
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
        Self::from_mcp_with_metadata_governed(json, &[], governor)
    }

    /// Import exact MCP declarations with explicit caller-owned recovery metadata.
    ///
    /// Applies [`super::DeclarationMetadata`] before normal set compilation;
    /// declarations, schema text, unknown-parameter handling and validators stay
    /// unchanged. Tool annotations do not supply this metadata. An empty slice
    /// behaves exactly like [`Self::from_mcp_governed`].
    ///
    /// See [`super::DeclarationMetadata`] for an executable alias and byte-exact
    /// rendering example.
    ///
    /// # Errors
    /// Returns the ordinary import errors plus [`ToolSetError::InvalidDefinition`]
    /// for unknown tools, empty/unknown/non-object paths or duplicate metadata;
    /// [`ToolSetError::Collision`] for aliases that collide in their object scope;
    /// and [`ToolSetError::Resource`] on cancellation or exhausted work/storage.
    pub fn from_mcp_with_metadata_governed(
        json: &str,
        metadata: &[super::DeclarationMetadata],
        governor: &mut Governor,
    ) -> Result<Self, ToolSetError> {
        adapt(json, DeclarationForm::Mcp, metadata, governor)
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
        adapt(json, DeclarationForm::Anthropic, &[], governor)
    }
}

/// Compact canonical declaration of a wire `form`; `parameters` is embedded
/// verbatim. The caller selects this path only for the three wire forms.
pub(super) fn render_canonical(
    form: DeclarationForm,
    name: &str,
    description: Option<&str>,
    parameters: Option<&str>,
    governor: &mut Governor,
) -> Result<String, StopReason> {
    let mut output = json::JsonOutput::new(governor)?;
    let wrapped = form == DeclarationForm::OpenAi;
    output.raw(if wrapped {
        r#"{"type":"function","function":{"name":"#
    } else {
        r#"{"name":"#
    })?;
    output.quoted(name)?;
    if let Some(description) = description {
        output.raw(r#","description":"#)?;
        output.quoted(description)?;
    }
    let schema = if wrapped {
        parameters
    } else {
        Some(parameters.unwrap_or(NO_PARAMETERS))
    };
    if let Some(schema) = schema {
        output.raw(",\"")?;
        output.raw(form.parameters_key())?;
        output.raw("\":")?;
        output.raw(schema)?;
    }
    output.raw(if wrapped { "}}" } else { "}" })?;
    output.finish()
}

fn adapt(
    input: &str,
    form: DeclarationForm,
    metadata: &[super::DeclarationMetadata],
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
    super::metadata::apply(pending.as_mut_slice(), metadata, governor)?;
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
    let mut projector = Projector::new(name, schema.dialect(), governor);
    // Project the compiled document (the exact parameters text, parsed with
    // spans relative to it) so retained defaults never depend on where the
    // declaration sat in the input.
    let (arguments, locations) = match parameters {
        Some(_) => projector.root(schema.document())?,
        None => (
            ObjectSpec {
                parameters: Vec::new(),
                unknown: Some(UnknownParameters::PassThrough),
            },
            RootLocations::inline(),
        ),
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
            locations,
        })),
    })
}

/// Longest chain of local `$ref`s the root projection follows.
const REF_HOPS: usize = 64;

/// How a followed schema relates to the arguments object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// It applies together with the root (a `$ref` target, an `allOf`
    /// member): its parameters are typed unless declared more than once.
    Apply,
    /// It is one branch of a root `anyOf`/`oneOf`: its parameters are
    /// unmodeled.
    Branch,
}

/// The object schemas of a root that composes others, as pointers.
#[derive(Debug, Default)]
struct RootPlan {
    /// Draft-07 `$ref` nodes whose siblings the `$ref` overrides.
    overridden: Vec<String>,
    /// Schemas that apply together.
    apply: Vec<String>,
    /// Branches of a union.
    branches: Vec<String>,
    /// Every `(is a branch, pointer)` in `apply` and `branches`.
    known: BTreeSet<(bool, String)>,
}

impl RootPlan {
    fn add(&mut self, role: Role, at: &str) {
        let list = match role {
            Role::Apply => &mut self.apply,
            Role::Branch => &mut self.branches,
        };
        if self.known.insert((role == Role::Branch, at.to_owned())) {
            list.push(at.to_owned());
        }
    }

    fn merge(&mut self, other: Self) {
        for at in other.overridden {
            if !self.overridden.contains(&at) {
                self.overridden.push(at);
            }
        }
        for at in other.apply {
            self.add(Role::Apply, &at);
        }
        for at in other.branches {
            self.add(Role::Branch, &at);
        }
    }
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
    /// Dialect of the parameters schema root.
    dialect: Dialect,
    governor: &'a mut Governor,
    unmodeled: Vec<(String, String)>,
}

impl<'a> Projector<'a> {
    const fn new(tool: &'a str, dialect: Dialect, governor: &'a mut Governor) -> Self {
        Self {
            tool,
            dialect,
            governor,
            unmodeled: Vec::new(),
        }
    }

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

    /// Project the root parameters schema, which must describe an object,
    /// and the locations its parameters were read from.
    fn root(&mut self, node: &Node) -> Result<(ObjectSpec, RootLocations), ToolSetError> {
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
        if let Some(plan) = self.plan(node)? {
            return self.composed(node, &plan);
        }
        if self.dialect == Dialect::Draft07 && node.get("$ref").is_some() {
            // Draft-07 `$ref` overrides every sibling, and this one names
            // no local object schema (external, missing, cyclic, or not an
            // object): nothing describes the arguments, so no parameter is
            // projected.
            self.note(members, "", Position::Root, Tag::Unmodeled)?;
            return Ok((
                ObjectSpec {
                    parameters: Vec::new(),
                    unknown: Some(UnknownParameters::PassThrough),
                },
                RootLocations::inline(),
            ));
        }
        let tag = Tag::Object {
            required: self.required_declared(node)?,
        };
        self.note(members, "", Position::Root, tag)?;
        Ok((self.object(node, members, "", 2)?, RootLocations::inline()))
    }

    /// The object schemas that describe the arguments through the root's
    /// `$ref`, `allOf`, `anyOf`, and `oneOf`, when any of them resolves.
    ///
    /// - A root `$ref` to a local pointer (`#/definitions/X`, `#/$defs/X`,
    ///   followed through further local `$ref`s) names an object schema. In
    ///   Draft-07 the `$ref` overrides its siblings, so the root describes
    ///   nothing itself; in Draft 2020-12 the siblings and the target apply
    ///   together.
    /// - A root `allOf` of exactly one schema that resolves the same way
    ///   applies together with the root.
    /// - A root `anyOf` or `oneOf` whose every branch resolves to an object
    ///   schema is a union of the branches.
    ///
    /// A keyword whose targets cannot all be resolved to object schemas
    /// (external, missing, cyclic, or not an object) contributes nothing: the
    /// projection is then what it was before the keyword was followed.
    fn plan(&mut self, root: &Node) -> Result<Option<RootPlan>, ToolSetError> {
        let mut plan = RootPlan::default();
        let mut used = false;
        if self.dialect == Dialect::Draft07 && root.get("$ref").is_some() {
            let mut part = RootPlan::default();
            if self.follow(root, "", Role::Apply, &mut part, &mut BTreeSet::new())? {
                return Ok(Some(part));
            }
            return Ok(None);
        }
        plan.apply.push(String::new());
        if root.get("$ref").is_some() {
            let mut part = RootPlan::default();
            if self.follow_ref(root, "", Role::Apply, &mut part, &mut BTreeSet::new())? {
                plan.merge(part);
                used = true;
            }
        }
        if let Some(Value::Array(branches)) = root.get("allOf").map(|node| &node.value)
            && branches.len() == 1
        {
            let mut part = RootPlan::default();
            if self.follow(
                root,
                "/allOf/0",
                Role::Apply,
                &mut part,
                &mut BTreeSet::new(),
            )? {
                plan.merge(part);
                used = true;
            }
        }
        for keyword in ["anyOf", "oneOf"] {
            let Some(Value::Array(branches)) = root.get(keyword).map(|node| &node.value) else {
                continue;
            };
            if branches.is_empty() {
                continue;
            }
            let mut part = RootPlan::default();
            let mut all = true;
            for index in 0..branches.len() {
                let at = format!("/{keyword}/{index}");
                if !self.follow(root, &at, Role::Branch, &mut part, &mut BTreeSet::new())? {
                    all = false;
                    break;
                }
            }
            if all {
                plan.merge(part);
                used = true;
            }
        }
        Ok(used.then_some(plan))
    }

    /// Follow the schema object at `at` into `plan` as `role`; whether it
    /// (and everything it references) is an object schema.
    fn follow(
        &mut self,
        root: &Node,
        at: &str,
        role: Role,
        plan: &mut RootPlan,
        seen: &mut BTreeSet<String>,
    ) -> Result<bool, ToolSetError> {
        self.charge(Resource::Work, size(at.len() / 16 + 1))?;
        if seen.len() >= REF_HOPS || !seen.insert(at.to_owned()) {
            return Ok(false);
        }
        let Some(node) = root.pointer(at) else {
            return Ok(false);
        };
        let Value::Object(members) = &node.value else {
            return Ok(false);
        };
        if node
            .get("type")
            .is_some_and(|kind| kind.as_str() != Some("object"))
        {
            return Ok(false);
        }
        if node.get("$ref").is_some() {
            if self.dialect == Dialect::Draft07 {
                // The `$ref` overrides every sibling.
                plan.overridden.push(at.to_owned());
                self.charge(Resource::ScratchBytes, size(at.len()) + KEYWORD_RECORD)?;
            } else if !members.is_empty() && Self::declares_object(node) {
                plan.add(role, at);
                self.charge(Resource::ScratchBytes, size(at.len()) + KEYWORD_RECORD)?;
            }
            return self.follow_ref(root, at, role, plan, seen);
        }
        if !Self::declares_object(node) {
            return Ok(false);
        }
        plan.add(role, at);
        self.charge(Resource::ScratchBytes, size(at.len()) + KEYWORD_RECORD)?;
        Ok(true)
    }

    /// Follow the local `$ref` of the schema object at `at`.
    fn follow_ref(
        &mut self,
        root: &Node,
        at: &str,
        role: Role,
        plan: &mut RootPlan,
        seen: &mut BTreeSet<String>,
    ) -> Result<bool, ToolSetError> {
        let Some(target) = root
            .pointer(at)
            .and_then(|node| node.get("$ref"))
            .and_then(Node::as_str)
            .and_then(local_pointer)
        else {
            return Ok(false);
        };
        self.follow(root, &target, role, plan, seen)
    }

    /// Whether the schema object `node` declares an object: `type` is
    /// `"object"`, or `properties` is an object.
    fn declares_object(node: &Node) -> bool {
        node.get("type").and_then(Node::as_str) == Some("object")
            || matches!(
                node.get("properties").map(|properties| &properties.value),
                Some(Value::Object(_))
            )
    }

    /// Record the keywords of every schema `plan` follows as unmodeled
    /// evidence, and return the names the applying schemas require and the
    /// handling of members no schema declares.
    fn note_plan<'n>(
        &mut self,
        root: &'n Node,
        plan: &RootPlan,
    ) -> Result<(BTreeSet<&'n str>, UnknownParameters), ToolSetError> {
        let mut declared: BTreeSet<&str> = BTreeSet::new();
        for at in &plan.apply {
            if let Some(Node {
                value: Value::Object(properties),
                ..
            }) = root.pointer(at).and_then(|node| node.get("properties"))
            {
                declared.extend(properties.iter().map(|property| property.key.as_str()));
            }
        }
        // Overridden nodes: every member is evidence, none is projected.
        for at in &plan.overridden {
            let Some(
                node @ Node {
                    value: Value::Object(members),
                    ..
                },
            ) = root.pointer(at)
            else {
                continue;
            };
            if !at.is_empty() {
                self.enter(node, at, 2)?;
            }
            self.note(members, at, Position::Root, Tag::Unmodeled)?;
        }
        let mut closed = false;
        let mut required: BTreeSet<&str> = BTreeSet::new();
        for at in &plan.apply {
            let Some(
                node @ Node {
                    value: Value::Object(members),
                    ..
                },
            ) = root.pointer(at)
            else {
                continue;
            };
            if !at.is_empty() {
                self.enter(node, at, 2)?;
            }
            let listed = match node.get("required").map(|names| &names.value) {
                None => true,
                Some(Value::Array(names)) => {
                    self.charge(Resource::Work, size(names.len()))?;
                    required.extend(names.iter().filter_map(Node::as_str));
                    names
                        .iter()
                        .all(|name| name.as_str().is_some_and(|name| declared.contains(name)))
                }
                Some(_) => false,
            };
            self.note(
                members,
                at,
                Position::Root,
                Tag::Object { required: listed },
            )?;
            closed |= self.closes(node, members);
        }
        let mut every_branch_closed = !plan.branches.is_empty();
        for at in &plan.branches {
            let Some(
                node @ Node {
                    value: Value::Object(members),
                    ..
                },
            ) = root.pointer(at)
            else {
                continue;
            };
            self.enter(node, at, 2)?;
            self.note(members, at, Position::Root, Tag::Unmodeled)?;
            every_branch_closed &= self.closes(node, members);
        }
        let unknown = if closed || every_branch_closed {
            UnknownParameters::Error
        } else {
            UnknownParameters::PassThrough
        };
        Ok((required, unknown))
    }

    /// Project the composed root `plan`: the parameters its object schemas
    /// declare together.
    ///
    /// A name declared by exactly one applying schema is typed from it. A
    /// name a union branch declares, or several schemas declare, is
    /// [`ParameterKind::Unmodeled`]: its typing stays with the exact schema
    /// (`tool_call.value.schema-typed`), as for the unions of the scope
    /// graph, and it is not marked required.
    fn composed(
        &mut self,
        root: &Node,
        plan: &RootPlan,
    ) -> Result<(ObjectSpec, RootLocations), ToolSetError> {
        let (required, unknown) = self.note_plan(root, plan)?;
        let (order, sites) = declarations(root, plan);
        let mut parameters = Vec::with_capacity(order.len());
        let mut declared = BTreeMap::new();
        for name in order {
            let entries = &sites[name];
            let pointers: Vec<String> = entries
                .iter()
                .map(|(pointer, _, _)| pointer.clone())
                .collect();
            let bytes = pointers.iter().fold(size(name.len()), |total, pointer| {
                total.saturating_add(size(pointer.len()))
            });
            self.charge(Resource::ScratchBytes, bytes.saturating_add(KEYWORD_RECORD))?;
            declared.insert(name.to_owned(), pointers);
            let spec = if let [(pointer, property, false)] = entries.as_slice() {
                let mut spec = self.spec(property, pointer, Position::Parameter, 2)?;
                spec.required = required.contains(name);
                spec
            } else {
                for (pointer, property, _) in entries {
                    self.enter(property, pointer, 2)?;
                    if let Value::Object(members) = &property.value {
                        self.note(members, pointer, Position::Parameter, Tag::Unmodeled)?;
                    }
                }
                ParameterSpec::of(ParameterKind::Unmodeled)
            };
            parameters.push((name.to_owned(), spec));
        }
        let describing: BTreeSet<String> = plan
            .overridden
            .iter()
            .chain(&plan.apply)
            .chain(&plan.branches)
            .cloned()
            .collect();
        let bytes = describing.iter().fold(0_u64, |total, at| {
            total.saturating_add(size(at.len()).saturating_add(KEYWORD_RECORD))
        });
        self.charge(Resource::ScratchBytes, bytes)?;
        Ok((
            ObjectSpec {
                parameters,
                unknown: Some(unknown),
            },
            RootLocations::new(declared, describing),
        ))
    }

    /// Whether the schema object `node` at `at` closes its object scope: a
    /// false `additionalProperties` (or, outside Draft-07,
    /// `unevaluatedProperties`) with nothing that could admit other members.
    /// A composing keyword (`$ref`, `allOf`, `anyOf`, `oneOf`, ...) could
    /// admit other members, as in an inline schema, so it keeps the scope
    /// open even where the projection followed it.
    fn closes(&self, node: &Node, members: &[Member]) -> bool {
        let closing: &[&str] = match self.dialect {
            Dialect::Draft07 => &["additionalProperties"],
            _ => &["additionalProperties", "unevaluatedProperties"],
        };
        closing.iter().any(|keyword| {
            node.get(keyword)
                .is_some_and(|value| value.value == Value::Bool(false))
        }) && !members.iter().any(|member| {
            member.key == "patternProperties" || RESHAPING.contains(&member.key.as_str())
        })
    }

    /// Whether `keyword` makes a value unmodeled: [`RESHAPING`], Draft-07
    /// `dependencies` (its schema form is `dependentSchemas`), and a nested
    /// `$schema` that switches away from the root's dialect.
    fn reshaping(&self, member: &Member) -> bool {
        RESHAPING.contains(&member.key.as_str())
            || (self.dialect == Dialect::Draft07 && member.key == "dependencies")
            || (member.key == "$schema"
                && member
                    .value
                    .as_str()
                    .and_then(Dialect::from_uri)
                    .is_none_or(|dialect| dialect != self.dialect))
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
        let reshaped = members.iter().any(|member| self.reshaping(member));
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
        // Draft-07 has no `unevaluatedProperties`; there it is ignored.
        let closing: &[&str] = match self.dialect {
            Dialect::Draft07 => &["additionalProperties"],
            _ => &["additionalProperties", "unevaluatedProperties"],
        };
        let closed = closing.iter().any(|keyword| {
            node.get(keyword)
                .is_some_and(|value| value.value == Value::Bool(false))
        });
        // Draft-07 `dependencies` constrains but never admits members.
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

/// Per property name, each declaring `(property pointer, schema, in a union
/// branch)`.
type Sites<'n> = BTreeMap<&'n str, Vec<(String, &'n Node, bool)>>;

/// The top-level property declarations of a composed root by name: the names
/// in order of first declaration (applying schemas, then union branches) and,
/// per name, each declaring `(property pointer, schema, in a union branch)`.
fn declarations<'n>(root: &'n Node, plan: &RootPlan) -> (Vec<&'n str>, Sites<'n>) {
    let mut order: Vec<&str> = Vec::new();
    let mut sites: Sites = BTreeMap::new();
    for (list, branch) in [(&plan.apply, false), (&plan.branches, true)] {
        for at in list {
            let Some(Node {
                value: Value::Object(properties),
                ..
            }) = root.pointer(at).and_then(|node| node.get("properties"))
            else {
                continue;
            };
            for property in properties {
                let entry = sites.entry(property.key.as_str()).or_insert_with(|| {
                    order.push(property.key.as_str());
                    Vec::new()
                });
                entry.push((
                    format!("{at}/properties/{}", pointer_segment(&property.key)),
                    &property.value,
                    branch,
                ));
            }
        }
    }
    (order, sites)
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
