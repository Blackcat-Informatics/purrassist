// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed compilation of tool definitions into a validated, indexed set.
use super::adapter::{self, DeclarationForm, ToolDeclaration, ToolSource};
use super::definition::{
    Effects, ObjectSpec, ParameterKind, ParameterSpec, RepairPolicy, ToolDefinition,
    UnknownParameters,
};
use super::emit::{self, EmitError};
use super::scope::{Nesting, ScopeCompiler, ScopeGraph};
use super::shape::{
    ArgumentShape, NameLiteral, ObjectShape, ParameterDefault, ParameterShape, PatternConstraint,
    ShapeStep, ValueShape,
};
use crate::contract::{
    Budget, Governor, Resource, ScratchCharge, ScratchMap, ScratchVec, StopReason,
    saturating_size as size,
};
use crate::json::{self, Node, Value, pointer_segment};
use crate::pattern::{Pattern, PatternCompileError};
use crate::schema::{CompilationError, Schema};
use crate::text::comparison_key;
use crate::wire;
use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

/// Portable per-parameter charge for its retained shape, name literal, and
/// schema-node records (64-bit layout over-estimate; see [`ScratchCharge`]).
pub(super) const PARAMETER_RECORD: usize = 512;
/// Portable per-step charge for the transient path used in diagnostics.
const STEP_RECORD: usize = 32;
/// Portable charge for one rendered schema object's members and keywords.
const RENDER_OVERHEAD: u64 = 1024;
/// Worst-case growth of a string through JSON escaping (`\u001f`), applied
/// once for the schema node copy and once for its serialized text.
const ESCAPED_COPIES: u64 = 12;

impl ScratchCharge for ToolDefinition {
    const CHARGE: usize = 160;
}
impl ScratchCharge for CompiledTool {
    const CHARGE: usize = 1024;
}
impl ScratchCharge for Pending {
    const CHARGE: usize = 176;
}
impl ScratchCharge for Declared {
    const CHARGE: usize = 1024;
}
const _: () = {
    assert!(core::mem::size_of::<ToolDefinition>() <= ToolDefinition::CHARGE);
    assert!(core::mem::size_of::<Pending>() <= Pending::CHARGE);
    assert!(core::mem::size_of::<Declared>() <= Declared::CHARGE);
    assert!(core::mem::size_of::<CompiledTool>() <= CompiledTool::CHARGE);
    assert!(
        core::mem::size_of::<ParameterShape>() + core::mem::size_of::<NameLiteral>()
            <= PARAMETER_RECORD
    );
    assert!(core::mem::size_of::<ShapeStep>() <= STEP_RECORD);
    assert!(core::mem::size_of::<(String, usize)>() <= <(String, usize) as ScratchCharge>::CHARGE);
    assert!(
        core::mem::size_of::<(String, String)>() <= <(String, String) as ScratchCharge>::CHARGE
    );
};

/// The collision key of a tool or parameter name.
///
/// The key is [`comparison_key`] of the name with each of `-`, `_`, `.`, and
/// `:` folded to `_` (one character each; runs are not collapsed). Two names
/// or aliases collide when their keys are equal. Infallible.
///
/// ```
/// use purrassist_core::tool_call::collision_key;
/// assert_eq!(collision_key("Read-File"), collision_key("read_file"));
/// assert_eq!(collision_key("a.b:c"), "a_b_c");
/// assert_ne!(collision_key("a--b"), collision_key("a-b"));
/// ```
#[must_use]
pub fn collision_key(name: &str) -> String {
    comparison_key(name)
        .chars()
        .map(|character| {
            if matches!(character, '-' | '_' | '.' | ':') {
                '_'
            } else {
                character
            }
        })
        .collect()
}

/// Whether `name` is a syntactically valid tool, alias, or parameter name:
/// non-empty and matching `[A-Za-z0-9_.:-]+`. There is no length cap.
///
/// ```
/// use purrassist_core::tool_call::is_valid_name;
/// assert!(is_valid_name("fs.read_file:v2-beta"));
/// assert!(!is_valid_name(""));
/// assert!(!is_valid_name("read file"));
/// ```
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-'))
}

/// Why a tool set could not be compiled.
///
/// Compilation either returns a complete set or one of these errors; no
/// partially compiled set exists.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet, ToolSetError}};
/// let error = ToolSet::compile(
///     [ToolDefinition::new("read_file"), ToolDefinition::new("Read-File")],
///     Budget::default(),
/// ).unwrap_err();
/// assert_eq!(error, ToolSetError::Collision {
///     first: "Read-File".into(), second: "read_file".into(), key: "read_file".into(),
/// });
/// assert!(error.to_string().contains("collide"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolSetError {
    /// A definition violates its contract (name syntax, misplaced modifier,
    /// invalid pattern, empty or duplicated enumeration, nonconforming
    /// default, or a schema the validator rejects).
    InvalidDefinition {
        /// Canonical name of the offending tool, as supplied.
        tool: String,
        /// Human-readable diagnostic.
        reason: String,
    },
    /// Two names or aliases share a [`collision_key`]. Tool-level entries are
    /// the literals themselves; parameter entries are qualified as
    /// `tool#/outer/inner`, with `[]` marking an array item. `first` sorts
    /// before or equal to `second` by bytes.
    Collision {
        /// The byte-wise smaller colliding entry.
        first: String,
        /// The byte-wise larger (or equal) colliding entry.
        second: String,
        /// The shared collision key.
        key: String,
    },
    /// Compilation stopped on resource exhaustion or cancellation.
    Resource(StopReason),
}

impl fmt::Display for ToolSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDefinition { tool, reason } => {
                write!(formatter, "invalid tool definition `{tool}`: {reason}")
            }
            Self::Collision { first, second, key } => {
                write!(
                    formatter,
                    "`{first}` and `{second}` collide under key `{key}`"
                )
            }
            Self::Resource(reason) => reason.fmt(formatter),
        }
    }
}

impl std::error::Error for ToolSetError {}

/// One compiled tool: resolved policies, argument schema, argument shape, and
/// provenance.
///
/// A tool built with [`ToolDefinition`] carries a canonical schema; a tool
/// built by a declaration adapter ([`ToolSet::from_openai`],
/// [`ToolSet::from_mcp`], [`ToolSet::from_anthropic`]) carries its exact
/// source declaration and a schema compiled from the exact parameters text
/// (see [`Self::source`]).
///
/// The repair policy is resolved from [`Self::effects`] when it is read, so
/// it always agrees with the effects the tool carries; unknown-parameter
/// handling is fixed at compilation because it is part of the validated
/// schema.
///
/// ```
/// use purrassist_core::{Budget, json, tool_call::{Effects, Param, ToolDefinition, ToolSet, UnknownParameters}};
/// let tool = ToolDefinition::new("read").param("path", Param::string().required());
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let compiled = tools.tool("read").unwrap();
/// assert_eq!(compiled.effects(), Effects::Mutating);
/// assert_eq!(compiled.unknown_parameters(), UnknownParameters::Error);
/// assert!(compiled.schema().validate(&json::parse(r#"{"path":"a"}"#).unwrap()).unwrap().valid);
/// ```
#[derive(Clone, Debug)]
pub struct CompiledTool {
    name: String,
    aliases: Vec<String>,
    description: Option<String>,
    effects: Effects,
    explicit_policy: Option<RepairPolicy>,
    unknown_parameters: UnknownParameters,
    schema_document: Node,
    schema_text: String,
    schema: Schema,
    shape: ArgumentShape,
    defaults: Vec<ParameterDefault>,
    source: ToolSource,
    unmodeled_keywords: Vec<(String, String)>,
    locations: RootLocations,
}

impl CompiledTool {
    /// The canonical name.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").alias("dir")], Budget::default()).unwrap();
    /// assert_eq!(tools.lookup_exact("dir").unwrap().name(), "ls");
    /// ```
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Explicit aliases, sorted by bytes.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").alias("dir").alias("ll")], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().aliases(), ["dir", "ll"]);
    /// ```
    #[must_use]
    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    /// The description, retained for rendering only and never used for repair.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().description(), None);
    /// ```
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Declared effects ([`Effects::Mutating`] when never declared).
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Effects, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("rm")], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("rm").unwrap().effects(), Effects::Mutating);
    /// ```
    #[must_use]
    pub const fn effects(&self) -> Effects {
        self.effects
    }

    /// The resolved repair policy: explicit, else derived from effects.
    ///
    /// Resolution happens on every call rather than at compilation, so the
    /// policy of a tool without an explicit one always follows its
    /// effects. Adapter-built tools never set one explicitly. Infallible.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{RepairPolicy, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("rm")], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("rm").unwrap().policy(), RepairPolicy::Structural);
    /// ```
    #[must_use]
    pub const fn policy(&self) -> RepairPolicy {
        self.policy_for(self.effects)
    }

    /// The repair policy this tool would have with `effects` in place of
    /// its own: explicit, else derived from `effects`. Infallible.
    ///
    /// Per-request effects overrides
    /// ([`Policies::effects`](super::Policies::effects)) resolve the policy
    /// through this, so an override changes the policy of a tool without an
    /// explicit one exactly as declaring those effects would.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Effects, RepairPolicy, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("rm")], Budget::default()).unwrap();
    /// let rm = tools.tool("rm").unwrap();
    /// assert_eq!(rm.policy_for(Effects::ReadOnly), RepairPolicy::Tolerant);
    /// assert_eq!(rm.policy_for(Effects::Mutating), RepairPolicy::Structural);
    /// ```
    #[must_use]
    pub const fn policy_for(&self, effects: Effects) -> RepairPolicy {
        match self.explicit_policy {
            Some(policy) => policy,
            None => effects.default_policy(),
        }
    }

    /// The resolved top-level unknown-argument handling.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Effects, ToolDefinition, ToolSet, UnknownParameters}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().unknown_parameters(), UnknownParameters::DropWithLoss);
    /// ```
    #[must_use]
    pub const fn unknown_parameters(&self) -> UnknownParameters {
        self.unknown_parameters
    }

    /// The argument schema text that [`Self::schema`] compiles: Draft
    /// 2020-12 for builder tools; for adapter tools, whichever dialect the
    /// declaration's `$schema` selects (see [`crate::schema::Dialect`]).
    ///
    /// For an adapter-built tool this is the exact parameters schema text of
    /// its source declaration, byte for byte (whitespace, key order, escapes,
    /// number spellings, and every keyword, modeled or not), or
    /// `{"type":"object"}` when the declaration had no parameters schema.
    ///
    /// For a [`ToolDefinition`] tool it is the canonical compact rendering,
    /// exactly `self.schema_document().to_json()`. Each schema object lists
    /// its keywords in the fixed order `type`, `description`, `enum`,
    /// `items`, `properties`, `required`, `additionalProperties`, `pattern`,
    /// `default`; `properties` follow declaration order, `required` is sorted
    /// by bytes and omitted when empty, and `additionalProperties` is
    /// `false` for [`UnknownParameters::Error`] and
    /// [`UnknownParameters::DropWithLoss`] and `true` for
    /// [`UnknownParameters::PassThrough`]. The tool description is not part
    /// of the argument schema, and no `$schema` member is emitted.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("read").param("path", Param::string().required());
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(
    ///     tools.tool("read").unwrap().json_schema(),
    ///     r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}"#,
    /// );
    /// ```
    #[must_use]
    pub fn json_schema(&self) -> &str {
        &self.schema_text
    }

    /// The argument schema as a JSON value.
    ///
    /// For an adapter-built tool this is the parsed exact parameters text,
    /// with source spans relative to [`Self::json_schema`]; for a
    /// [`ToolDefinition`] tool it is the generated canonical document (no
    /// spans).
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// let document = tools.tool("ls").unwrap().schema_document();
    /// assert_eq!(document.get("type").and_then(|node| node.as_str()), Some("object"));
    /// ```
    #[must_use]
    pub const fn schema_document(&self) -> &Node {
        &self.schema_document
    }

    /// The compiled validator for [`Self::json_schema`].
    ///
    /// ```
    /// use purrassist_core::{Budget, json, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// let schema = tools.tool("ls").unwrap().schema();
    /// assert!(!schema.validate(&json::parse(r#"{"x":1}"#).unwrap()).unwrap().valid);
    /// ```
    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The engine-independent argument shape.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// assert!(tools.tool("ls").unwrap().argument_shape().arguments.parameters.is_empty());
    /// ```
    #[must_use]
    pub const fn argument_shape(&self) -> &ArgumentShape {
        &self.shape
    }

    /// Validated defaults in declaration order, nested defaults before the
    /// default of their enclosing parameter; never applied by any repair.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// assert!(tools.tool("ls").unwrap().defaults().is_empty());
    /// ```
    #[must_use]
    pub fn defaults(&self) -> &[ParameterDefault] {
        &self.defaults
    }

    /// Where this tool came from: a [`ToolDefinition`] builder or an exact
    /// source declaration.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{DeclarationForm, ToolDefinition, ToolSet, ToolSource}};
    /// let built = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// assert_eq!(built.tool("ls").unwrap().source(), &ToolSource::Builder);
    /// let adapted = ToolSet::from_mcp(r#"{"name":"ls","inputSchema":{"type":"object"}}"#, Budget::default()).unwrap();
    /// let ToolSource::Declaration(declaration) = adapted.tool("ls").unwrap().source() else { unreachable!() };
    /// assert_eq!(declaration.form(), DeclarationForm::Mcp);
    /// ```
    #[must_use]
    pub const fn source(&self) -> &ToolSource {
        &self.source
    }

    /// Every keyword of an adapter-built tool's parameters schema that the
    /// typed model does not represent, as `(schema pointer, keyword)` pairs.
    ///
    /// Pointers are RFC 6901 JSON Pointers relative to the parameters schema
    /// (`""` for its root); see the adapter module documentation for exactly
    /// which keywords count as represented. Empty for builder tools, which
    /// only produce modeled keywords. Infallible.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::ToolSet};
    /// let tools = ToolSet::from_mcp(
    ///     r#"{"name":"t","inputSchema":{"type":"object","x-vendor":1,"properties":{"a":{"type":"string","format":"uri"}}}}"#,
    ///     Budget::default(),
    /// ).unwrap();
    /// assert_eq!(
    ///     tools.tool("t").unwrap().unmodeled_keywords(),
    ///     [("".to_owned(), "x-vendor".to_owned()), ("/properties/a".to_owned(), "format".to_owned())],
    /// );
    /// ```
    #[must_use]
    pub fn unmodeled_keywords(&self) -> &[(String, String)] {
        &self.unmodeled_keywords
    }

    /// The schema pointer of the top-level parameter `name`: below the
    /// location the root projection read it from (the first that declares
    /// it).
    pub(super) fn parameter_pointer(&self, name: &str) -> String {
        self.locations.declaring(name).swap_remove(0)
    }

    /// Where an unmodeled-keyword `pointer` sits relative to the arguments
    /// object: `""` or `/properties/NAME` and `/items` steps.
    pub(super) fn argument_pointer<'p>(&self, pointer: &'p str) -> &'p str {
        self.locations.relative(pointer)
    }

    /// Render this tool's declaration in `form` under a fresh governor with
    /// the default [`Budget`].
    ///
    /// For an adapter-built tool whose source form is `form`, the result is
    /// exactly the original declaration bytes. For the other wire forms it is
    /// the compact canonical declaration of `form` (see [`DeclarationForm`])
    /// carrying the name, the description when present, and the parameters
    /// schema: the exact source parameters text for adapter-built tools
    /// (verbatim, never re-serialized) or the canonical [`Self::json_schema`]
    /// for builder tools. Form-specific declaration fields such as `OpenAI`
    /// `strict` or MCP `annotations` are retained only in their own form's
    /// exact text. The prompt-dialect forms ([`DeclarationForm::dialect`])
    /// are printed by the dialect grammar's `declaration` production with the
    /// same name, description, and exact parameters text.
    ///
    /// # Errors
    /// See [`Self::render_declaration_governed`]; the wire forms never fail.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{DeclarationForm, ToolSet}};
    /// let source = r#"{ "name": "ls",  "input_schema": {"type":"object"} }"#;
    /// let tools = ToolSet::from_anthropic(source, Budget::default()).unwrap();
    /// let ls = tools.tool("ls").unwrap();
    /// assert_eq!(ls.render_declaration(DeclarationForm::Anthropic)?, source);
    /// assert_eq!(
    ///     ls.render_declaration(DeclarationForm::Mcp)?,
    ///     r#"{"name":"ls","inputSchema":{"type":"object"}}"#,
    /// );
    /// assert_eq!(
    ///     ls.render_declaration(DeclarationForm::Qwen3CoderXml)?,
    ///     "<function>\n<name>ls</name>\n<parameters>{\"type\":\"object\"}</parameters>\n</function>",
    /// );
    /// # Ok::<(), purrassist_core::tool_call::EmitError>(())
    /// ```
    pub fn render_declaration(&self, form: DeclarationForm) -> Result<String, EmitError> {
        self.render_declaration_governed(form, &mut Governor::new(Budget::default()))
    }

    /// Render this tool's declaration in `form` against a caller-owned
    /// governor (see [`Self::render_declaration`]). The wire forms copy
    /// text already charged when the tool set was compiled and charge
    /// nothing; the prompt-dialect forms charge their printing.
    ///
    /// # Errors
    /// For a prompt-dialect form: [`EmitError::Unrepresentable`] when the
    /// description or the parameters text would end its element early (a
    /// Qwen3-Coder description containing `</description>`, a schema
    /// containing `</parameters>` or, for Hermes, `</tools>`), and
    /// [`EmitError::Resource`] on exhaustion or cancellation.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, tool_call::{DeclarationForm, EmitError, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").description("List </description> files")], Budget::default()).unwrap();
    /// let ls = tools.tool("ls").unwrap();
    /// let mut governor = Governor::new(Budget::default());
    /// assert!(matches!(
    ///     ls.render_declaration_governed(DeclarationForm::Qwen3CoderXml, &mut governor),
    ///     Err(EmitError::Unrepresentable { .. })
    /// ));
    /// let hermes = ls.render_declaration_governed(DeclarationForm::Hermes, &mut governor)?;
    /// assert!(hermes.starts_with(r#"{"type":"function","function":{"name":"ls","description":"List </description> files","parameters":{"#));
    /// # Ok::<(), EmitError>(())
    /// ```
    pub fn render_declaration_governed(
        &self,
        form: DeclarationForm,
        governor: &mut Governor,
    ) -> Result<String, EmitError> {
        let parameters = match &self.source {
            ToolSource::Declaration(declaration) if declaration.form() == form => {
                return Ok(declaration.text().to_owned());
            }
            ToolSource::Declaration(declaration) => declaration.parameters_text(),
            ToolSource::Builder => Some(self.schema_text.as_str()),
        };
        let description = self.description.as_deref();
        form.dialect().map_or_else(
            || {
                Ok(adapter::render_canonical(
                    form,
                    &self.name,
                    description,
                    parameters,
                ))
            },
            |dialect| emit::declaration(&dialect, &self.name, description, parameters, governor),
        )
    }
}

/// A validated, collision-free set of compiled tools.
///
/// Tools are stored and iterated sorted by canonical name, and every result
/// (including which error is reported) is independent of declaration order.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tools = ToolSet::compile(
///     [ToolDefinition::new("write"), ToolDefinition::new("read").alias("cat")],
///     Budget::default(),
/// ).unwrap();
/// let names: Vec<_> = tools.tools().iter().map(|tool| tool.name()).collect();
/// assert_eq!(names, ["read", "write"]);
/// assert_eq!(tools.lookup_exact("cat").unwrap().name(), "read");
/// assert!(tools.tool("cat").is_none());
/// ```
#[derive(Clone, Debug)]
pub struct ToolSet {
    tools: Vec<CompiledTool>,
    index: Vec<(String, usize)>,
}

impl ToolSet {
    /// Compile definitions under a fresh governor with `budget`.
    ///
    /// # Errors
    /// See [`Self::compile_governed`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet, ToolSetError}};
    /// assert!(matches!(
    ///     ToolSet::compile([ToolDefinition::new("bad name")], Budget::default()),
    ///     Err(ToolSetError::InvalidDefinition { .. })
    /// ));
    /// ```
    pub fn compile(
        definitions: impl IntoIterator<Item = ToolDefinition>,
        budget: Budget,
    ) -> Result<Self, ToolSetError> {
        Self::compile_governed(definitions, &mut Governor::new(budget))
    }

    /// Compile definitions against a caller-owned governor.
    ///
    /// Definitions are first sorted by canonical name; names and aliases are
    /// then checked for syntax and set-wide collisions, and each tool's
    /// parameters, patterns, schema, and defaults are compiled. Work, depth,
    /// input bytes (via pattern and schema compilation), and scratch for
    /// every retained and transient record are charged with portable,
    /// target-independent amounts before allocation.
    ///
    /// # Errors
    /// [`ToolSetError::InvalidDefinition`] or [`ToolSetError::Collision`] for
    /// a definition that violates its contract, and
    /// [`ToolSetError::Resource`] when the governor is exhausted or
    /// cancelled; exhaustion is sticky on `governor`.
    ///
    /// ```
    /// use purrassist_core::{Budget, CancellationToken, Governor, StopReason};
    /// use purrassist_core::tool_call::{ToolDefinition, ToolSet, ToolSetError};
    /// let token = CancellationToken::new();
    /// let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
    /// token.cancel();
    /// assert_eq!(
    ///     ToolSet::compile_governed([ToolDefinition::new("ls")], &mut governor).unwrap_err(),
    ///     ToolSetError::Resource(StopReason::Cancelled),
    /// );
    /// ```
    pub fn compile_governed(
        definitions: impl IntoIterator<Item = ToolDefinition>,
        governor: &mut Governor,
    ) -> Result<Self, ToolSetError> {
        governor.check().map_err(ToolSetError::Resource)?;
        let mut pending: ScratchVec<Pending> = ScratchVec::new(governor);
        for definition in definitions {
            governor
                .charge(Resource::Work, 1)
                .map_err(ToolSetError::Resource)?;
            pending
                .push(Pending {
                    definition,
                    declaration: None,
                })
                .map_err(ToolSetError::Resource)?;
        }
        Self::assemble(pending, governor)
    }

    /// Sort, name-check, collision-check, index, and compile pending tools.
    pub(super) fn assemble(
        mut sorted: ScratchVec<Pending>,
        governor: &mut Governor,
    ) -> Result<Self, ToolSetError> {
        let name_bytes = sorted.as_slice().iter().fold(0_u64, |total, tool| {
            total.saturating_add(size(tool.definition.name.len()))
        });
        governor
            .charge(
                Resource::Work,
                name_bytes.saturating_mul(sort_factor(sorted.len())),
            )
            .map_err(ToolSetError::Resource)?;
        sorted
            .as_mut_slice()
            .sort_by(|left, right| left.definition.name.cmp(&right.definition.name));
        for Pending { definition, .. } in sorted.as_slice() {
            for literal in core::iter::once(&definition.name).chain(&definition.aliases) {
                if !is_valid_name(literal) {
                    return Err(invalid(
                        &definition.name,
                        format!("tool name or alias `{literal}` must match [A-Za-z0-9_.:-]+"),
                    ));
                }
            }
        }
        let literals = sorted
            .as_slice()
            .iter()
            .flat_map(|Pending { definition, .. }| {
                core::iter::once(definition.name.as_str())
                    .chain(definition.aliases.iter().map(String::as_str))
            });
        if let Some((key, first, second)) = find_collision(literals, governor)? {
            return Err(ToolSetError::Collision { first, second, key });
        }
        let mut tools: ScratchVec<CompiledTool> = ScratchVec::new(governor);
        let mut index: ScratchMap<String, usize> = ScratchMap::new(governor);
        for entry in sorted.into_vec() {
            let position = tools.len();
            let definition = &entry.definition;
            for literal in core::iter::once(&definition.name).chain(&definition.aliases) {
                retain_text(governor, literal.len())?;
                index
                    .insert(literal.clone(), position)
                    .map_err(ToolSetError::Resource)?;
            }
            tools
                .push(compile_tool(entry, governor)?)
                .map_err(ToolSetError::Resource)?;
        }
        Ok(Self {
            tools: tools.into_vec(),
            index: index.into_pairs(),
        })
    }

    /// Every compiled tool, sorted by canonical name.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("b"), ToolDefinition::new("a")], Budget::default()).unwrap();
    /// assert_eq!(tools.tools()[0].name(), "a");
    /// ```
    #[must_use]
    pub fn tools(&self) -> &[CompiledTool] {
        &self.tools
    }

    /// The tool with this exact canonical name (aliases are not consulted).
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").alias("dir")], Budget::default()).unwrap();
    /// assert!(tools.tool("ls").is_some());
    /// assert!(tools.tool("dir").is_none());
    /// assert!(tools.tool("LS").is_none());
    /// ```
    #[must_use]
    pub fn tool(&self, name: &str) -> Option<&CompiledTool> {
        self.tools
            .binary_search_by(|tool| tool.name.as_str().cmp(name))
            .ok()
            .map(|position| &self.tools[position])
    }

    /// Resolve an exact canonical name or explicit alias (byte equality).
    ///
    /// No normalization is applied; approximate resolution belongs to later
    /// recovery, which must label it.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").alias("dir")], Budget::default()).unwrap();
    /// assert_eq!(tools.lookup_exact("dir").unwrap().name(), "ls");
    /// assert!(tools.lookup_exact("Dir").is_none());
    /// ```
    #[must_use]
    pub fn lookup_exact(&self, name_or_alias: &str) -> Option<&CompiledTool> {
        self.index
            .binary_search_by(|(literal, _)| literal.as_str().cmp(name_or_alias))
            .ok()
            .map(|position| &self.tools[self.index[position].1])
    }

    /// The canonical compact JSON Schema of a tool's arguments, by canonical
    /// name; see [`CompiledTool::json_schema`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls")], Budget::default()).unwrap();
    /// assert_eq!(
    ///     tools.render_json_schema("ls"),
    ///     Some(r#"{"type":"object","properties":{},"additionalProperties":false}"#),
    /// );
    /// assert_eq!(tools.render_json_schema("missing"), None);
    /// ```
    #[must_use]
    pub fn render_json_schema(&self, tool: &str) -> Option<&str> {
        self.tool(tool).map(CompiledTool::json_schema)
    }

    /// A tool's declaration rendered in `form`, by canonical name; see
    /// [`CompiledTool::render_declaration`].
    ///
    /// # Errors
    /// [`EmitError::UnknownTool`] for an unknown name, otherwise as
    /// [`CompiledTool::render_declaration`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{DeclarationForm, EmitError, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").description("List")], Budget::default()).unwrap();
    /// assert_eq!(
    ///     tools.render_declaration("ls", DeclarationForm::OpenAi)?,
    ///     r#"{"type":"function","function":{"name":"ls","description":"List","parameters":{"type":"object","properties":{},"additionalProperties":false}}}"#,
    /// );
    /// assert_eq!(
    ///     tools.render_declaration("missing", DeclarationForm::Mcp),
    ///     Err(EmitError::UnknownTool { name: "missing".into() }),
    /// );
    /// # Ok::<(), EmitError>(())
    /// ```
    pub fn render_declaration(
        &self,
        tool: &str,
        form: DeclarationForm,
    ) -> Result<String, EmitError> {
        self.tool(tool)
            .ok_or_else(|| EmitError::UnknownTool { name: tool.into() })?
            .render_declaration(form)
    }

    /// The argument shape of a tool, by canonical name.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("ls").param("all", Param::boolean());
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(tools.argument_shape("ls").unwrap().arguments.parameters[0].name, "all");
    /// ```
    #[must_use]
    pub fn argument_shape(&self, tool: &str) -> Option<&ArgumentShape> {
        self.tool(tool).map(CompiledTool::argument_shape)
    }

    /// The validated, never-applied defaults of a tool, by canonical name.
    ///
    /// ```
    /// use purrassist_core::{Budget, json, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("ls").param("all", Param::boolean().default(json::parse("false").unwrap()));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(tools.defaults("ls").unwrap().len(), 1);
    /// ```
    #[must_use]
    pub fn defaults(&self, tool: &str) -> Option<&[ParameterDefault]> {
        self.tool(tool).map(CompiledTool::defaults)
    }
}

pub(super) fn invalid(tool: &str, reason: String) -> ToolSetError {
    ToolSetError::InvalidDefinition {
        tool: tool.to_owned(),
        reason,
    }
}

/// A comparison-sort bound: `ceil(log2(n)) + 1` comparisons per element.
pub(super) fn sort_factor(length: usize) -> u64 {
    u64::from(length.bit_width()) + 1
}

/// Charge and permanently retain `bytes` of copied text.
pub(super) fn retain_text(governor: &mut Governor, bytes: usize) -> Result<(), ToolSetError> {
    governor
        .charge(Resource::Work, size(bytes))
        .map_err(ToolSetError::Resource)?;
    governor
        .charge(Resource::ScratchBytes, size(bytes))
        .map_err(ToolSetError::Resource)
}

/// The first colliding pair among `literals`, in collision-key order, as
/// `(key, first, second)` with `first <= second` by bytes.
///
/// Every literal must already be a valid (ASCII) name, so its key has the
/// literal's length; key and literal copies are leased until return.
fn find_collision<'a>(
    literals: impl Iterator<Item = &'a str> + Clone,
    governor: &mut Governor,
) -> Result<Option<(String, String, String)>, ToolSetError> {
    let bytes = literals.clone().fold(0_u64, |total, literal| {
        total.saturating_add(size(literal.len()))
    });
    let _strings = governor
        .lease_scratch(bytes.saturating_mul(2))
        .map_err(ToolSetError::Resource)?;
    governor
        .charge(Resource::Work, bytes)
        .map_err(ToolSetError::Resource)?;
    let mut entries: ScratchVec<(String, String)> = ScratchVec::new(governor);
    for literal in literals {
        entries
            .push((collision_key(literal), literal.to_owned()))
            .map_err(ToolSetError::Resource)?;
    }
    governor
        .charge(
            Resource::Work,
            bytes.saturating_mul(sort_factor(entries.len())),
        )
        .map_err(ToolSetError::Resource)?;
    entries.as_mut_slice().sort();
    Ok(entries.as_slice().windows(2).find_map(|pair| {
        (pair[0].0 == pair[1].0).then(|| (pair[0].0.clone(), pair[0].1.clone(), pair[1].1.clone()))
    }))
}

/// One tool awaiting set assembly: its typed definition and, for an
/// adapter-built tool, everything retained from its exact declaration.
pub(super) struct Pending {
    pub(super) definition: ToolDefinition,
    pub(super) declaration: Option<Box<Declared>>,
}

/// What a declaration adapter retains beyond the typed definition.
pub(super) struct Declared {
    /// The exact source declaration.
    pub(super) source: ToolDeclaration,
    /// The exact parameters text, or `{"type":"object"}` when absent.
    pub(super) schema_text: String,
    /// The validator compiled from `schema_text`.
    pub(super) schema: Schema,
    /// `(schema pointer, keyword)` pairs the typed model does not represent.
    pub(super) unmodeled_keywords: Vec<(String, String)>,
    /// The schema locations the root projection read its `properties` from.
    pub(super) locations: RootLocations,
}

/// The schema locations that describe an adapter tool's arguments object.
///
/// An inline parameters schema describes the arguments at its own root
/// (`""`). A root `$ref` (Draft-07 `definitions`, 2020-12 `$defs`), a root
/// `allOf` of one object, and a root `anyOf`/`oneOf` of objects describe
/// them at other locations of the same document; the projection reads
/// `properties` from those, and every pointer below them is a pointer into
/// the arguments.
#[derive(Clone, Debug, Default)]
pub(super) struct RootLocations {
    /// Per top-level property name, the pointers of the schemas that declare
    /// it, in order (empty for an inline root, whose one location is `""`).
    declared: BTreeMap<String, Vec<String>>,
    /// Every location that describes the arguments object itself (empty for
    /// an inline root).
    describing: BTreeSet<String>,
}

impl RootLocations {
    /// The root of an inline schema.
    pub(super) const fn inline() -> Self {
        Self {
            declared: BTreeMap::new(),
            describing: BTreeSet::new(),
        }
    }

    pub(super) const fn new(
        declared: BTreeMap<String, Vec<String>>,
        describing: BTreeSet<String>,
    ) -> Self {
        Self {
            declared,
            describing,
        }
    }

    /// The pointers of the property schemas that declare the top-level
    /// property `name`, in order; the inline location's when none is
    /// recorded.
    pub(super) fn declaring(&self, name: &str) -> Vec<String> {
        self.declared
            .get(name)
            .cloned()
            .unwrap_or_else(|| vec![format!("/properties/{}", pointer_segment(name))])
    }

    /// The location of `pointer` relative to the arguments object it is
    /// below, when it lies at or below a location that describes it (the
    /// longest such location wins).
    pub(super) fn relative<'p>(&self, pointer: &'p str) -> &'p str {
        if self.describing.is_empty() {
            return pointer;
        }
        // Longest prefix first: every `/` boundary, then the whole pointer.
        let mut boundaries = pointer.match_indices('/').map(|(at, _)| at);
        let mut cuts: Vec<usize> = boundaries.by_ref().collect();
        cuts.push(pointer.len());
        cuts.into_iter()
            .rev()
            .find(|&at| self.describing.contains(&pointer[..at]))
            .map_or(pointer, |at| &pointer[at..])
    }
}

/// The base URI under which an adapter tool's exact parameters schema is
/// supplied when compiling a parameter subschema to validate a default.
const ARGUMENTS_URI: &str = "urn:purrassist:tool-arguments";

fn compile_tool(entry: Pending, governor: &mut Governor) -> Result<CompiledTool, ToolSetError> {
    governor.check().map_err(ToolSetError::Resource)?;
    let Pending {
        definition,
        declaration,
    } = entry;
    let text_bytes = definition
        .aliases
        .iter()
        .chain(&definition.description)
        .fold(definition.name.len(), |total, text| {
            total.saturating_add(text.len())
        });
    retain_text(governor, text_bytes)?;
    let unknown = definition.resolved_unknown_parameters();
    let mut builder = ShapeBuilder {
        tool: &definition.name,
        governor,
        path: Vec::new(),
        defaults: Vec::new(),
        exact: declaration
            .as_ref()
            .map(|declared| declared.schema_text.as_str()),
        resources: BTreeMap::new(),
        scopes: declaration.as_ref().map(|declared| {
            ScopeCompiler::new(declared.schema.document(), declared.schema.dialect())
        }),
        root: declaration.as_ref().map(|declared| &declared.locations),
    };
    let arguments = builder.object(&definition.arguments, unknown, 1)?;
    let defaults = builder.defaults;
    let scopes = match builder.scopes {
        Some(compiler) => compiler
            .finish(builder.governor)
            .map_err(ToolSetError::Resource)?,
        None => ScopeGraph::default(),
    };
    let (schema_document, schema_text, schema, source, unmodeled_keywords, locations) =
        if let Some(declared) = declaration {
            let Declared {
                source,
                schema_text,
                schema,
                unmodeled_keywords,
                locations,
            } = *declared;
            governor
                .charge(Resource::ScratchBytes, schema.document().estimated_bytes())
                .map_err(ToolSetError::Resource)?;
            (
                schema.document().clone(),
                schema_text,
                schema,
                ToolSource::Declaration(source),
                unmodeled_keywords,
                locations,
            )
        } else {
            let (schema_document, schema_text, schema) =
                generated_schema(&definition, unknown, governor)?;
            (
                schema_document,
                schema_text,
                schema,
                ToolSource::Builder,
                Vec::new(),
                RootLocations::inline(),
            )
        };
    let mut aliases = definition.aliases.clone();
    aliases.sort();
    Ok(CompiledTool {
        name: definition.name.clone(),
        aliases,
        description: definition.description.clone(),
        effects: definition.effects,
        explicit_policy: definition.policy,
        unknown_parameters: unknown,
        schema_document,
        schema_text,
        schema,
        shape: ArgumentShape {
            tool: definition.name.clone(),
            arguments,
            scopes,
        },
        defaults,
        source,
        unmodeled_keywords,
        locations,
    })
}

/// The schema document, text, and validator generated from a typed
/// definition's arguments.
fn generated_schema(
    definition: &ToolDefinition,
    unknown: UnknownParameters,
    governor: &mut Governor,
) -> Result<(Node, String, Schema), ToolSetError> {
    governor
        .charge(
            Resource::ScratchBytes,
            object_estimate(&definition.arguments),
        )
        .map_err(ToolSetError::Resource)?;
    let schema_document = wire::object(object_fields(
        &definition.arguments,
        unknown,
        vec![("type", wire::string("object"))],
    ));
    let schema_text = schema_document.to_json();
    let schema = compiled(
        &definition.name,
        "generated schema",
        Schema::compile_governed(&schema_text, governor),
    )?;
    Ok((schema_document, schema_text, schema))
}

/// Map a schema compilation result for `tool`, labeling a rejection with
/// `context` and passing resource exhaustion through unchanged.
pub(super) fn compiled(
    tool: &str,
    context: &str,
    result: Result<Schema, CompilationError>,
) -> Result<Schema, ToolSetError> {
    result.map_err(|error| match error {
        CompilationError::InvalidSchema(error) => {
            invalid(tool, format!("{context} rejected: {error}"))
        }
        CompilationError::Resource(reason) => ToolSetError::Resource(reason),
    })
}

/// Conservative scratch estimate for rendering one parameter's schema node
/// and its serialized text, computed without allocating.
fn parameter_estimate(spec: &ParameterSpec) -> u64 {
    let enum_bytes = match &spec.kind {
        ParameterKind::Enumeration(values) => values.iter().fold(0_u64, |total, value| {
            total.saturating_add(size(value.len()).saturating_add(8))
        }),
        _ => 0,
    };
    let text = spec
        .description
        .iter()
        .chain(&spec.pattern)
        .fold(enum_bytes, |total, text| {
            total.saturating_add(size(text.len()))
        });
    let nested = match &spec.kind {
        ParameterKind::Array(items) => parameter_estimate(items),
        ParameterKind::Object(object) => object_estimate(object),
        _ => 0,
    };
    let default = spec.default.as_ref().map_or(0, Node::estimated_bytes);
    let wrappers = if spec.repeatable { 2 } else { 1 };
    RENDER_OVERHEAD
        .saturating_mul(wrappers)
        .saturating_add(text.saturating_mul(ESCAPED_COPIES))
        .saturating_add(default.saturating_mul(ESCAPED_COPIES))
        .saturating_add(nested)
}

fn object_estimate(object: &ObjectSpec) -> u64 {
    object
        .parameters
        .iter()
        .fold(RENDER_OVERHEAD, |total, (name, spec)| {
            total
                .saturating_add(size(name.len()).saturating_mul(2 * ESCAPED_COPIES))
                .saturating_add(parameter_estimate(spec))
        })
}

/// Render one parameter's schema. A repeatable parameter renders as
/// `{"type":"array","description":..,"items":<item schema>,"default":..}`,
/// carrying its description and default on the array rather than the item.
fn render_parameter(spec: &ParameterSpec, inherited: UnknownParameters) -> Node {
    if !spec.repeatable {
        return render_value(spec, inherited, true);
    }
    let mut fields = vec![("type", wire::string("array"))];
    if let Some(description) = &spec.description {
        fields.push(("description", wire::string(description.as_str())));
    }
    fields.push(("items", render_value(spec, inherited, false)));
    if let Some(default) = &spec.default {
        fields.push(("default", default.clone()));
    }
    wire::object(fields)
}

/// Render the value schema of `spec`, with its description and default
/// annotations only when `annotated`.
fn render_value(spec: &ParameterSpec, inherited: UnknownParameters, annotated: bool) -> Node {
    let type_name = match &spec.kind {
        ParameterKind::String | ParameterKind::Enumeration(_) => Some("string"),
        ParameterKind::Integer => Some("integer"),
        ParameterKind::Number => Some("number"),
        ParameterKind::Boolean => Some("boolean"),
        ParameterKind::Array(_) => Some("array"),
        ParameterKind::Object(_) => Some("object"),
        // Any JSON value: the exact source schema is the only constraint.
        ParameterKind::Unmodeled => None,
    };
    let mut fields: Vec<(&'static str, Node)> = type_name
        .map(|name| ("type", wire::string(name)))
        .into_iter()
        .collect();
    if let Some(description) = spec.description.as_ref().filter(|_| annotated) {
        fields.push(("description", wire::string(description.as_str())));
    }
    match &spec.kind {
        ParameterKind::Enumeration(values) => fields.push((
            "enum",
            wire::array(values.iter().map(|value| wire::string(value.as_str()))),
        )),
        ParameterKind::Array(items) => fields.push(("items", render_parameter(items, inherited))),
        ParameterKind::Object(object) => {
            fields = object_fields(object, object.unknown.unwrap_or(inherited), fields);
        }
        _ => {}
    }
    if let Some(pattern) = &spec.pattern {
        fields.push(("pattern", wire::string(pattern.as_str())));
    }
    if let Some(default) = spec.default.as_ref().filter(|_| annotated) {
        fields.push(("default", default.clone()));
    }
    wire::object(fields)
}

/// Append `properties`, `required` (when non-empty), and
/// `additionalProperties` for an object scope with resolved `unknown`.
fn object_fields(
    object: &ObjectSpec,
    unknown: UnknownParameters,
    mut fields: Vec<(&'static str, Node)>,
) -> Vec<(&'static str, Node)> {
    fields.push((
        "properties",
        wire::object(
            object
                .parameters
                .iter()
                .map(|(name, spec)| (name.as_str(), render_parameter(spec, unknown))),
        ),
    ));
    let mut required: Vec<&str> = object
        .parameters
        .iter()
        .filter(|(_, spec)| spec.required)
        .map(|(name, _)| name.as_str())
        .collect();
    required.sort_unstable();
    if !required.is_empty() {
        fields.push((
            "required",
            wire::array(required.into_iter().map(wire::string)),
        ));
    }
    fields.push((
        "additionalProperties",
        Node::new(Value::Bool(unknown == UnknownParameters::PassThrough)),
    ));
    fields
}

/// Recursive shape construction for one tool, with the current parameter
/// path for diagnostics and default locations.
struct ShapeBuilder<'a> {
    tool: &'a str,
    governor: &'a mut Governor,
    path: Vec<ShapeStep>,
    defaults: Vec<ParameterDefault>,
    /// The exact parameters schema text of an adapter-built tool; defaults
    /// are then validated against its subschemas rather than a rendering.
    exact: Option<&'a str>,
    /// `exact` supplied under [`ARGUMENTS_URI`], filled on first use.
    resources: BTreeMap<String, String>,
    /// The scope graph under construction (an adapter tool's exact schema).
    scopes: Option<ScopeCompiler<'a>>,
    /// Where an adapter tool's root projection read its parameters from.
    root: Option<&'a RootLocations>,
}

impl ShapeBuilder<'_> {
    /// How the nested parameter elements of a parameter of shape `value`
    /// at the current path are scoped: the scope graph node of its exact
    /// schema location when the value is unmodeled (or an array of
    /// unmodeled items).
    fn nesting(&mut self, value: &ValueShape) -> Result<Nesting, ToolSetError> {
        if self.scopes.is_none() {
            return Ok(Nesting::None);
        }
        let pointers = match value {
            ValueShape::Unmodeled => self.schema_pointers(),
            ValueShape::Array(items) if **items == ValueShape::Unmodeled => self
                .schema_pointers()
                .into_iter()
                .map(|pointer| format!("{pointer}/items"))
                .collect(),
            _ => return Ok(Nesting::None),
        };
        let Some(scopes) = self.scopes.as_mut() else {
            return Ok(Nesting::None);
        };
        let node = scopes
            .scope_many(&pointers, self.governor)
            .map_err(ToolSetError::Resource)?;
        Ok(if matches!(value, ValueShape::Unmodeled) {
            Nesting::Object(node)
        } else {
            Nesting::ArrayOfObject(node)
        })
    }

    fn location(&self, leaf: Option<&str>) -> String {
        let mut location = String::from("#");
        for step in &self.path {
            match step {
                ShapeStep::Parameter(name) => {
                    location.push('/');
                    location.push_str(name);
                }
                ShapeStep::Item => location.push_str("[]"),
            }
        }
        if let Some(leaf) = leaf {
            location.push('/');
            location.push_str(leaf);
        }
        location
    }

    /// The JSON Pointers of the current parameter within an adapter tool's
    /// parameters schema: adapters project only `properties` members and
    /// object-form `items`, so each step has one schema location below the
    /// location(s) the root projection read the top-level parameter from
    /// (several when the root is a union that declares the name in more
    /// than one branch).
    fn schema_pointers(&self) -> Vec<String> {
        let mut pointers = vec![String::new()];
        for (index, step) in self.path.iter().enumerate() {
            match step {
                ShapeStep::Parameter(name) => {
                    if index == 0 {
                        pointers = self.root.map_or_else(
                            || vec![format!("/properties/{}", pointer_segment(name))],
                            |locations| locations.declaring(name),
                        );
                    } else {
                        for pointer in &mut pointers {
                            pointer.push_str("/properties/");
                            pointer.push_str(&pointer_segment(name));
                        }
                    }
                }
                ShapeStep::Item => {
                    for pointer in &mut pointers {
                        pointer.push_str("/items");
                    }
                }
            }
        }
        pointers
    }

    /// The first of [`Self::schema_pointers`]: the location a default or
    /// an exact subschema is read from.
    fn schema_pointer(&self) -> String {
        self.schema_pointers().swap_remove(0)
    }

    /// Compile the exact subschema at the current parameter, resolving it
    /// (and any `$ref` it contains) within the complete exact parameters
    /// schema supplied as an explicit resource.
    fn exact_subschema(&mut self, text: &str) -> Result<Schema, ToolSetError> {
        if self.resources.is_empty() {
            retain_text(self.governor, text.len())?;
            self.resources
                .insert(ARGUMENTS_URI.to_owned(), text.to_owned());
        }
        let reference = format!("{ARGUMENTS_URI}#{}", self.schema_pointer());
        let root = format!("{{\"$ref\":{}}}", json::quote(&reference));
        retain_text(self.governor, root.len())?;
        compiled(
            self.tool,
            "parameters schema",
            Schema::with_resources_governed(&root, &self.resources, self.governor),
        )
    }

    fn invalid(&self, reason: &str) -> ToolSetError {
        invalid(
            self.tool,
            format!("parameter `{}`: {reason}", self.location(None)),
        )
    }

    fn object(
        &mut self,
        spec: &ObjectSpec,
        unknown: UnknownParameters,
        depth: u32,
    ) -> Result<ObjectShape, ToolSetError> {
        self.governor
            .charge(Resource::Depth, u64::from(depth))
            .map_err(ToolSetError::Resource)?;
        for (name, parameter) in &spec.parameters {
            for literal in core::iter::once(name).chain(&parameter.aliases) {
                if !is_valid_name(literal) {
                    return Err(invalid(
                        self.tool,
                        format!(
                            "parameter name or alias `{}` must match [A-Za-z0-9_.:-]+",
                            self.location(Some(literal))
                        ),
                    ));
                }
            }
        }
        let literals = spec.parameters.iter().flat_map(|(name, parameter)| {
            core::iter::once(name.as_str()).chain(parameter.aliases.iter().map(String::as_str))
        });
        if let Some((key, first, second)) = find_collision(literals, self.governor)? {
            return Err(ToolSetError::Collision {
                first: format!("{}{}", self.tool, self.location(Some(&first))),
                second: format!("{}{}", self.tool, self.location(Some(&second))),
                key,
            });
        }
        let mut parameters = Vec::new();
        let mut names = Vec::new();
        let mut required = Vec::new();
        for (name, parameter) in &spec.parameters {
            let shape = self.parameter(name, parameter, unknown, depth)?;
            for literal in core::iter::once(name).chain(&parameter.aliases) {
                names.push(NameLiteral {
                    literal: literal.clone(),
                    canonical: name.clone(),
                    alias: literal != name,
                });
            }
            if parameter.required {
                required.push(name.clone());
            }
            parameters.push(shape);
        }
        self.governor
            .charge(
                Resource::Work,
                size(names.len()).saturating_mul(sort_factor(names.len())),
            )
            .map_err(ToolSetError::Resource)?;
        names.sort_by(|left: &NameLiteral, right| left.literal.cmp(&right.literal));
        required.sort();
        Ok(ObjectShape {
            parameters,
            names,
            required,
            unknown_parameters: unknown,
        })
    }

    /// Enter `step` for the duration of `build`, leasing its path storage.
    fn within<T>(
        &mut self,
        step: ShapeStep,
        build: impl FnOnce(&mut Self) -> Result<T, ToolSetError>,
    ) -> Result<T, ToolSetError> {
        let label = match &step {
            ShapeStep::Parameter(name) => name.len(),
            ShapeStep::Item => 0,
        };
        let _step = self
            .governor
            .lease_scratch(size(STEP_RECORD.saturating_add(label)))
            .map_err(ToolSetError::Resource)?;
        self.path.push(step);
        let result = build(self);
        self.path.pop();
        result
    }

    fn parameter(
        &mut self,
        name: &str,
        spec: &ParameterSpec,
        inherited: UnknownParameters,
        depth: u32,
    ) -> Result<ParameterShape, ToolSetError> {
        self.governor
            .charge(Resource::Work, 1)
            .map_err(ToolSetError::Resource)?;
        self.within(ShapeStep::Parameter(name.to_owned()), |builder| {
            let text_bytes = spec
                .aliases
                .iter()
                .fold(name.len().saturating_mul(3), |total, alias| {
                    total.saturating_add(alias.len().saturating_mul(2))
                });
            retain_text(builder.governor, text_bytes)?;
            builder
                .governor
                .charge(
                    Resource::ScratchBytes,
                    size(PARAMETER_RECORD.saturating_mul(spec.aliases.len() + 1)),
                )
                .map_err(ToolSetError::Resource)?;
            if spec.repeatable && matches!(spec.kind, ParameterKind::Array(_)) {
                return Err(builder.invalid(
                    "array parameters always accept repeated occurrences; `repeatable` applies only to non-array parameters",
                ));
            }
            let value = builder.value(spec, inherited, depth)?;
            let nesting = builder.nesting(&value)?;
            if let Some(default) = &spec.default {
                builder.validate_default(spec, inherited, default)?;
            }
            Ok(ParameterShape {
                name: name.to_owned(),
                aliases: spec.aliases.clone(),
                value,
                required: spec.required,
                repeatable: spec.repeatable,
                default: spec.default.clone(),
                nesting,
            })
        })
    }

    fn value(
        &mut self,
        spec: &ParameterSpec,
        inherited: UnknownParameters,
        depth: u32,
    ) -> Result<ValueShape, ToolSetError> {
        if spec.pattern.is_some() && spec.kind != ParameterKind::String {
            return Err(self.invalid("`pattern` applies only to string parameters"));
        }
        Ok(match &spec.kind {
            ParameterKind::String => ValueShape::String(
                spec.pattern
                    .as_deref()
                    .map(|source| self.pattern(source))
                    .transpose()?,
            ),
            ParameterKind::Integer => ValueShape::Integer,
            ParameterKind::Number => ValueShape::Number,
            ParameterKind::Boolean => ValueShape::Boolean,
            ParameterKind::Enumeration(values) => {
                ValueShape::Enumeration(self.enumeration(values)?)
            }
            ParameterKind::Array(items) => {
                let next = depth.saturating_add(1);
                self.governor
                    .charge(Resource::Depth, u64::from(next))
                    .map_err(ToolSetError::Resource)?;
                if items.required
                    || items.repeatable
                    || items.default.is_some()
                    || !items.aliases.is_empty()
                {
                    return Err(self.invalid(
                        "an array item specification cannot be required, aliased, repeatable, or defaulted",
                    ));
                }
                self.governor
                    .charge(Resource::ScratchBytes, size(PARAMETER_RECORD))
                    .map_err(ToolSetError::Resource)?;
                ValueShape::Array(Box::new(self.within(ShapeStep::Item, |builder| {
                    builder.value(items, inherited, next)
                })?))
            }
            ParameterKind::Object(object) => ValueShape::Object(self.object(
                object,
                object.unknown.unwrap_or(inherited),
                depth.saturating_add(1),
            )?),
            ParameterKind::Unmodeled => ValueShape::Unmodeled,
        })
    }

    fn pattern(&mut self, source: &str) -> Result<PatternConstraint, ToolSetError> {
        retain_text(self.governor, source.len())?;
        match Pattern::compile_governed(source, self.governor) {
            Ok(pattern) => Ok(PatternConstraint {
                source: source.to_owned(),
                pattern,
            }),
            Err(PatternCompileError::Syntax(error)) => {
                Err(self.invalid(&format!("invalid pattern `{source}`: {error}")))
            }
            Err(PatternCompileError::Resource(reason)) => Err(ToolSetError::Resource(reason)),
        }
    }

    fn enumeration(&mut self, values: &[String]) -> Result<Vec<String>, ToolSetError> {
        if values.is_empty() {
            return Err(self.invalid("an enumeration must declare at least one value"));
        }
        let bytes = values.iter().fold(0_u64, |total, value| {
            total.saturating_add(size(value.len()))
        });
        self.governor
            .charge(
                Resource::Work,
                bytes.saturating_mul(sort_factor(values.len())),
            )
            .map_err(ToolSetError::Resource)?;
        let mut sorted: ScratchVec<usize> = ScratchVec::new(self.governor);
        sorted
            .extend(0..values.len())
            .map_err(ToolSetError::Resource)?;
        sorted
            .as_mut_slice()
            .sort_by(|left, right| values[*left].cmp(&values[*right]));
        if let Some(pair) = sorted
            .as_slice()
            .windows(2)
            .find(|pair| values[pair[0]] == values[pair[1]])
        {
            return Err(self.invalid(&format!(
                "enumeration value `{}` is declared more than once",
                values[pair[0]]
            )));
        }
        self.governor
            .charge(
                Resource::ScratchBytes,
                bytes.saturating_add(size(values.len()).saturating_mul(24)),
            )
            .map_err(ToolSetError::Resource)?;
        Ok(values.to_vec())
    }

    /// Validate a declared default against the parameter's own rendered
    /// schema, compiled by the same engine that validates calls.
    fn validate_default(
        &mut self,
        spec: &ParameterSpec,
        inherited: UnknownParameters,
        default: &Node,
    ) -> Result<(), ToolSetError> {
        let schema = if let Some(text) = self.exact {
            self.exact_subschema(text)?
        } else {
            let _render = self
                .governor
                .lease_scratch(parameter_estimate(spec))
                .map_err(ToolSetError::Resource)?;
            let text = render_parameter(spec, inherited).to_json();
            compiled(
                self.tool,
                "generated schema",
                Schema::compile_governed(&text, self.governor),
            )?
        };
        let report = schema
            .validate_with(default, self.governor)
            .map_err(ToolSetError::Resource)?;
        if !report.valid {
            let violations: Vec<String> = report
                .violations
                .iter()
                .map(|violation| format!("{}: {}", violation.instance_path, violation.message))
                .collect();
            return Err(self.invalid(&format!(
                "default {} does not conform to the declared type ({})",
                default.to_json(),
                violations.join("; ")
            )));
        }
        self.governor
            .charge(Resource::ScratchBytes, default.estimated_bytes())
            .map_err(ToolSetError::Resource)?;
        self.defaults.push(ParameterDefault {
            path: self.path.clone(),
            value: default.clone(),
        });
        Ok(())
    }
}
