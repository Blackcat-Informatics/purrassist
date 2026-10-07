// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Typed, builder-style tool and parameter definitions.
//!
//! Builders never fail: every constraint (name syntax, collisions, misplaced
//! modifiers, patterns, defaults) is checked once, under a governor, by
//! [`super::ToolSet::compile_governed`].
use crate::json::Node;

/// Whether invoking a tool can change state outside the call.
///
/// Effects select the conservative defaults of [`RepairPolicy`] and
/// [`UnknownParameters`] when a definition does not set them explicitly.
///
/// ```
/// use purrassist_core::tool_call::{Effects, RepairPolicy, UnknownParameters};
/// assert_eq!(Effects::default(), Effects::Mutating);
/// assert_eq!(Effects::ReadOnly.default_policy(), RepairPolicy::Tolerant);
/// assert_eq!(Effects::Mutating.default_unknown_parameters(), UnknownParameters::Error);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effects {
    /// The tool only observes state; a mistaken call is recoverable.
    ReadOnly,
    /// The tool may change state. This is the default because a definition
    /// that never states its effects must be treated as dangerous.
    #[default]
    Mutating,
}

impl Effects {
    /// The repair policy used when a definition does not set one.
    ///
    /// `Mutating` tools default to [`RepairPolicy::Structural`] and
    /// `ReadOnly` tools to [`RepairPolicy::Tolerant`]. This is a total,
    /// infallible mapping.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, RepairPolicy};
    /// assert_eq!(Effects::Mutating.default_policy(), RepairPolicy::Structural);
    /// ```
    #[must_use]
    pub const fn default_policy(self) -> RepairPolicy {
        match self {
            Self::ReadOnly => RepairPolicy::Tolerant,
            Self::Mutating => RepairPolicy::Structural,
        }
    }

    /// The unknown-parameter handling used when a definition does not set one.
    ///
    /// `Mutating` tools default to [`UnknownParameters::Error`] and
    /// `ReadOnly` tools to [`UnknownParameters::DropWithLoss`]. This is a
    /// total, infallible mapping.
    ///
    /// ```
    /// use purrassist_core::tool_call::{Effects, UnknownParameters};
    /// assert_eq!(Effects::ReadOnly.default_unknown_parameters(), UnknownParameters::DropWithLoss);
    /// ```
    #[must_use]
    pub const fn default_unknown_parameters(self) -> UnknownParameters {
        match self {
            Self::ReadOnly => UnknownParameters::DropWithLoss,
            Self::Mutating => UnknownParameters::Error,
        }
    }
}

/// How aggressively later recovery may repair a call to this tool.
///
/// Compilation records the policy; it does not repair anything itself.
///
/// ```
/// use purrassist_core::tool_call::{RepairPolicy, ToolDefinition};
/// let definition = ToolDefinition::new("delete_file").policy(RepairPolicy::Tolerant);
/// assert_eq!(definition.name(), "delete_file");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepairPolicy {
    /// Only structural (syntax and shape) repairs; no value guesses.
    Structural,
    /// Structural repairs plus validated, labeled value inferences.
    Tolerant,
}

/// Handling of argument members that match no declared parameter.
///
/// In the canonical JSON Schema, `Error` and `DropWithLoss` render as
/// `"additionalProperties": false` and `PassThrough` as `true`.
///
/// ```
/// use purrassist_core::tool_call::{ToolDefinition, UnknownParameters};
/// let definition = ToolDefinition::new("search")
///     .unknown_parameters(UnknownParameters::PassThrough);
/// assert_eq!(definition.name(), "search");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnknownParameters {
    /// Reject the call.
    Error,
    /// Remove the member and record a loss.
    DropWithLoss,
    /// Keep the member unchanged.
    PassThrough,
}

/// The declared value type of one parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ParameterKind {
    String,
    Integer,
    Number,
    Boolean,
    Enumeration(Vec<String>),
    Array(Box<ParameterSpec>),
    Object(ObjectSpec),
    /// A value the typed model cannot express; only declaration adapters
    /// produce it, and only the exact source schema validates it.
    Unmodeled,
}

/// One declared parameter: its type plus modifiers.
///
/// Construct with [`Param`]. Every declared string value (plain, enum,
/// or patterned) is verbatim: there are no trimming or normalization
/// switches, so whitespace and case are significant to validation.
///
/// Modifier placement is checked at compile time and reported as
/// [`super::ToolSetError::InvalidDefinition`]: [`Self::pattern`] applies
/// only to plain strings, [`Self::repeatable`] only to non-array parameters,
/// and an array's item specification cannot be required, aliased, repeatable, or
/// defaulted.
///
/// ```
/// use purrassist_core::{json, tool_call::{Param, ToolDefinition, ToolSet}};
/// use purrassist_core::Budget;
/// let tool = ToolDefinition::new("read").param(
///     "path",
///     Param::string().required().alias("file").description("Path to read"),
/// );
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// assert!(tools.render_json_schema("read").unwrap().contains(r#""required":["path"]"#));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterSpec {
    pub(super) kind: ParameterKind,
    pub(super) required: bool,
    pub(super) aliases: Vec<String>,
    pub(super) repeatable: bool,
    pub(super) default: Option<Node>,
    pub(super) description: Option<String>,
    pub(super) pattern: Option<String>,
}

impl ParameterSpec {
    pub(super) const fn of(kind: ParameterKind) -> Self {
        Self {
            kind,
            required: false,
            aliases: Vec::new(),
            repeatable: false,
            default: None,
            description: None,
            pattern: None,
        }
    }

    /// Require the parameter in every call.
    ///
    /// Infallible; misuse on an array item is reported at compile time.
    ///
    /// ```
    /// use purrassist_core::tool_call::Param;
    /// let spec = Param::integer().required();
    /// assert_eq!(spec, Param::integer().required());
    /// ```
    #[must_use]
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Accept an explicit alternative spelling for this parameter's name.
    ///
    /// Aliases follow the name syntax and collision rules of
    /// [`super::collision_key`] within their object scope; violations are
    /// reported at compile time.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("read").param("path", Param::string().alias("file"));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// let shape = tools.argument_shape("read").unwrap();
    /// assert_eq!(shape.arguments.resolve_name("file"), Some("path"));
    /// ```
    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Declare that a non-array parameter may repeat (R1.4.4 "may repeat").
    ///
    /// The argument value of a repeatable parameter `p: T` is an array of
    /// `T`: each occurrence in tool-call markup supplies one item in source
    /// order, so a single occurrence yields a one-element array. Its schema
    /// renders as `{"type":"array","items":<T schema>}`, carrying the
    /// description and default on the array, and a declared default must be
    /// an array of `T`. In the argument shape the parameter keeps
    /// `repeatable: true` with `value` describing one item.
    ///
    /// Without this modifier a scalar or object parameter that occurs twice
    /// is never resolved by last-wins or silent array collection; later
    /// recovery reports the call as unresolved, with both readings. Array
    /// parameters ([`Param::array`]) always accept repeated occurrences, so
    /// applying `repeatable` to one is reported at compile time as
    /// [`super::ToolSetError::InvalidDefinition`].
    ///
    /// ```
    /// use purrassist_core::{Budget, json, tool_call::{Param, ToolDefinition, ToolSet, ValueShape}};
    /// let tool = ToolDefinition::new("tag").param("label", Param::string().repeatable());
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// let label = &tools.argument_shape("tag").unwrap().arguments.parameters[0];
    /// assert!(label.repeatable);
    /// assert_eq!(label.value, ValueShape::String(None));
    /// assert!(tools.render_json_schema("tag").unwrap()
    ///     .contains(r#""label":{"type":"array","items":{"type":"string"}}"#));
    /// let schema = tools.tool("tag").unwrap().schema();
    /// assert!(schema.validate(&json::parse(r#"{"label":["a"]}"#).unwrap()).unwrap().valid);
    /// assert!(!schema.validate(&json::parse(r#"{"label":"a"}"#).unwrap()).unwrap().valid);
    /// ```
    #[must_use]
    pub const fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }

    /// Declare a default value.
    ///
    /// The value is validated against the parameter's declared type at
    /// compile time and exposed through [`super::ToolSet::defaults`] and
    /// the rendered schema's `default` annotation. No repair ever applies it.
    ///
    /// ```
    /// use purrassist_core::{Budget, json, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("list")
    ///     .param("limit", Param::integer().default(json::parse("10").unwrap()));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(tools.defaults("list").unwrap()[0].value.to_json(), "10");
    /// ```
    #[must_use]
    pub fn default(mut self, value: Node) -> Self {
        self.default = Some(value);
        self
    }

    /// Attach a human-readable description, used only for rendering.
    ///
    /// Descriptions never influence matching or repair.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("read").param("path", Param::string().description("A path"));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert!(tools.render_json_schema("read").unwrap().contains(r#""description":"A path""#));
    /// ```
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Constrain a plain string parameter with an ECMA-262 `u`-mode pattern
    /// (unanchored, as in JSON Schema `pattern`).
    ///
    /// The pattern is compiled under the governor at tool-set compile time;
    /// a malformed pattern, or a pattern on a non-string parameter, is
    /// reported as [`super::ToolSetError::InvalidDefinition`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("open").param("id", Param::string().pattern("^[0-9]+$"));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert!(tools.render_json_schema("open").unwrap().contains(r#""pattern":"^[0-9]+$""#));
    /// ```
    #[must_use]
    pub fn pattern(mut self, pattern: impl Into<String>) -> Self {
        self.pattern = Some(pattern.into());
        self
    }
}

/// Constructors for every supported parameter type.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{ObjectSpec, Param, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("configure")
///     .param("name", Param::string())
///     .param("count", Param::integer())
///     .param("ratio", Param::number())
///     .param("enabled", Param::boolean())
///     .param("mode", Param::enumeration(&["fast", "safe"]))
///     .param("tags", Param::array(Param::string()))
///     .param("extra", Param::object(ObjectSpec::new().param("key", Param::string())));
/// assert!(ToolSet::compile([tool], Budget::default()).is_ok());
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Param;

impl Param {
    /// A verbatim JSON string parameter (`"type":"string"`).
    ///
    /// ```
    /// use purrassist_core::tool_call::Param;
    /// assert_ne!(Param::string(), Param::integer());
    /// ```
    #[must_use]
    pub const fn string() -> ParameterSpec {
        ParameterSpec::of(ParameterKind::String)
    }

    /// An exact integer parameter (`"type":"integer"`); integral spellings
    /// such as `1e2` satisfy it, as in JSON Schema.
    ///
    /// ```
    /// use purrassist_core::tool_call::Param;
    /// assert_ne!(Param::integer(), Param::number());
    /// ```
    #[must_use]
    pub const fn integer() -> ParameterSpec {
        ParameterSpec::of(ParameterKind::Integer)
    }

    /// Any JSON number (`"type":"number"`), compared by exact decimal value.
    ///
    /// ```
    /// use purrassist_core::tool_call::Param;
    /// assert_ne!(Param::number(), Param::boolean());
    /// ```
    #[must_use]
    pub const fn number() -> ParameterSpec {
        ParameterSpec::of(ParameterKind::Number)
    }

    /// A JSON boolean (`"type":"boolean"`).
    ///
    /// ```
    /// use purrassist_core::tool_call::Param;
    /// assert_eq!(Param::boolean(), Param::boolean());
    /// ```
    #[must_use]
    pub const fn boolean() -> ParameterSpec {
        ParameterSpec::of(ParameterKind::Boolean)
    }

    /// A string drawn from a fixed literal set (`"type":"string","enum":[..]`).
    ///
    /// Values are verbatim and keep their declared order. Only an exact value
    /// validates; later recovery may *propose* a member whose
    /// [`crate::text::comparison_key`] matches the input, labeled as a repair.
    /// An empty set or an exact duplicate is reported at compile time.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet, ValueShape}};
    /// let tool = ToolDefinition::new("sort").param("order", Param::enumeration(&["asc", "desc"]));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// let shape = &tools.argument_shape("sort").unwrap().arguments.parameters[0].value;
    /// assert_eq!(shape, &ValueShape::Enumeration(vec!["asc".into(), "desc".into()]));
    /// ```
    #[must_use]
    pub fn enumeration(values: &[&str]) -> ParameterSpec {
        ParameterSpec::of(ParameterKind::Enumeration(
            values.iter().map(|value| (*value).to_owned()).collect(),
        ))
    }

    /// An array whose every item satisfies `items` (`"type":"array","items":..`).
    ///
    /// ```
    /// use purrassist_core::tool_call::Param;
    /// let tags = Param::array(Param::string().pattern("^[a-z]+$"));
    /// assert_ne!(tags, Param::array(Param::string()));
    /// ```
    #[must_use]
    pub fn array(items: ParameterSpec) -> ParameterSpec {
        ParameterSpec::of(ParameterKind::Array(Box::new(items)))
    }

    /// A nested object with its own parameter scope.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ObjectSpec, Param};
    /// let range = Param::object(ObjectSpec::new().param("start", Param::integer().required()));
    /// assert_ne!(range, Param::object(ObjectSpec::new()));
    /// ```
    #[must_use]
    pub const fn object(object: ObjectSpec) -> ParameterSpec {
        ParameterSpec::of(ParameterKind::Object(object))
    }
}

/// A nested object's parameters and unknown-member handling.
///
/// When [`Self::unknown_parameters`] is not called, the object inherits the
/// resolved handling of its enclosing tool.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{ObjectSpec, Param, ToolDefinition, ToolSet, UnknownParameters}};
/// let options = ObjectSpec::new()
///     .param("depth", Param::integer())
///     .unknown_parameters(UnknownParameters::PassThrough);
/// let tool = ToolDefinition::new("scan").param("options", Param::object(options));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// assert!(tools.render_json_schema("scan").unwrap().contains(r#""additionalProperties":true"#));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectSpec {
    pub(super) parameters: Vec<(String, ParameterSpec)>,
    pub(super) unknown: Option<UnknownParameters>,
}

impl ObjectSpec {
    /// An object with no parameters that inherits unknown-member handling.
    ///
    /// ```
    /// use purrassist_core::tool_call::ObjectSpec;
    /// assert_eq!(ObjectSpec::new(), ObjectSpec::default());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            parameters: Vec::new(),
            unknown: None,
        }
    }

    /// Declare a member parameter. Declaration order is the rendered
    /// `properties` order; name and collision errors surface at compile time.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ObjectSpec, Param};
    /// let spec = ObjectSpec::new().param("x", Param::number()).param("y", Param::number());
    /// assert_ne!(spec, ObjectSpec::new());
    /// ```
    #[must_use]
    pub fn param(mut self, name: impl Into<String>, spec: ParameterSpec) -> Self {
        self.parameters.push((name.into(), spec));
        self
    }

    /// Set how members matching no parameter of this object are handled.
    ///
    /// ```
    /// use purrassist_core::tool_call::{ObjectSpec, UnknownParameters};
    /// let spec = ObjectSpec::new().unknown_parameters(UnknownParameters::Error);
    /// assert_ne!(spec, ObjectSpec::new());
    /// ```
    #[must_use]
    pub const fn unknown_parameters(mut self, handling: UnknownParameters) -> Self {
        self.unknown = Some(handling);
        self
    }
}

/// One tool: its name, aliases, effects, policies, and argument parameters.
///
/// A definition that never calls [`Self::effects`] is [`Effects::Mutating`],
/// the conservative choice. Unless set explicitly, the repair policy and
/// unknown-parameter handling derive from the effects (see
/// [`Effects::default_policy`] and [`Effects::default_unknown_parameters`]).
/// The description is stored for rendering only and is never used for repair.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Effects, Param, RepairPolicy, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("read_file")
///     .alias("cat")
///     .description("Read a file")
///     .effects(Effects::ReadOnly)
///     .param("path", Param::string().required());
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let compiled = tools.lookup_exact("cat").unwrap();
/// assert_eq!(compiled.name(), "read_file");
/// assert_eq!(compiled.policy(), RepairPolicy::Tolerant);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDefinition {
    pub(super) name: String,
    pub(super) aliases: Vec<String>,
    pub(super) description: Option<String>,
    pub(super) effects: Effects,
    pub(super) policy: Option<RepairPolicy>,
    pub(super) arguments: ObjectSpec,
}

impl ToolDefinition {
    /// Start a definition with its canonical name.
    ///
    /// Name syntax (`[A-Za-z0-9_.:-]+`) and collisions are checked at compile
    /// time, never here.
    ///
    /// ```
    /// use purrassist_core::tool_call::ToolDefinition;
    /// assert_eq!(ToolDefinition::new("search").name(), "search");
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            description: None,
            effects: Effects::Mutating,
            policy: None,
            arguments: ObjectSpec::new(),
        }
    }

    /// The canonical name, exactly as supplied.
    ///
    /// ```
    /// use purrassist_core::tool_call::ToolDefinition;
    /// assert_eq!(ToolDefinition::new("a.b").name(), "a.b");
    /// ```
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Accept an explicit alternative tool name for exact lookup.
    ///
    /// An alias whose [`super::collision_key`] equals that of any tool name
    /// or alias in the same set is reported as a collision at compile time.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("grep").alias("search")], Budget::default()).unwrap();
    /// assert_eq!(tools.lookup_exact("search").unwrap().name(), "grep");
    /// ```
    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Attach a description used only for rendering, never for repair.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").description("List")], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().description(), Some("List"));
    /// ```
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Declare the tool's effects (default [`Effects::Mutating`]).
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Effects, ToolDefinition, ToolSet}};
    /// let tools = ToolSet::compile([ToolDefinition::new("ls").effects(Effects::ReadOnly)], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().effects(), Effects::ReadOnly);
    /// ```
    #[must_use]
    pub const fn effects(mut self, effects: Effects) -> Self {
        self.effects = effects;
        self
    }

    /// Set the repair policy explicitly instead of deriving it from effects.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Effects, RepairPolicy, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("ls").effects(Effects::ReadOnly).policy(RepairPolicy::Structural);
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().policy(), RepairPolicy::Structural);
    /// ```
    #[must_use]
    pub const fn policy(mut self, policy: RepairPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Set top-level unknown-argument handling instead of deriving it from
    /// effects. Nested objects without their own setting inherit this value.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolDefinition, ToolSet, UnknownParameters}};
    /// let tool = ToolDefinition::new("ls").unknown_parameters(UnknownParameters::PassThrough);
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(tools.tool("ls").unwrap().unknown_parameters(), UnknownParameters::PassThrough);
    /// ```
    #[must_use]
    pub const fn unknown_parameters(mut self, handling: UnknownParameters) -> Self {
        self.arguments.unknown = Some(handling);
        self
    }

    /// Declare an argument parameter. Declaration order is the rendered
    /// `properties` order; name and collision errors surface at compile time.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("echo").param("text", Param::string().required());
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// assert_eq!(tools.argument_shape("echo").unwrap().arguments.required, ["text"]);
    /// ```
    #[must_use]
    pub fn param(mut self, name: impl Into<String>, spec: ParameterSpec) -> Self {
        self.arguments.parameters.push((name.into(), spec));
        self
    }

    /// The top-level unknown-argument handling: explicit, else effect-derived.
    pub(super) fn resolved_unknown_parameters(&self) -> UnknownParameters {
        self.arguments
            .unknown
            .unwrap_or_else(|| self.effects.default_unknown_parameters())
    }
}
