// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Engine-independent argument shapes emitted by tool-set compilation.
//!
//! A shape lists, per object scope, every literal a tool-call grammar must
//! recognize (parameter names and aliases mapped to canonical names, enum
//! literal sets, compiled string patterns), each parameter's declared type,
//! the required set, and repeatable flags. It carries no parser state, so any
//! grammar engine can compile it.
use super::UnknownParameters;
use crate::json::Node;
use crate::pattern::Pattern;

/// The argument shape of one compiled tool.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("read").param("path", Param::string().required());
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let shape = tools.argument_shape("read").unwrap();
/// assert_eq!(shape.tool, "read");
/// assert_eq!(shape.arguments.required, ["path"]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgumentShape {
    /// Canonical tool name.
    pub tool: String,
    /// The top-level arguments object.
    pub arguments: ObjectShape,
}

/// One object scope: its parameters and name literal table.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet, UnknownParameters}};
/// let tool = ToolDefinition::new("read").param("path", Param::string().alias("file"));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let object = &tools.argument_shape("read").unwrap().arguments;
/// assert_eq!(object.names.len(), 2);
/// assert_eq!(object.unknown_parameters, UnknownParameters::Error);
/// assert!(object.parameter("path").is_some());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectShape {
    /// Parameters in declaration order.
    pub parameters: Vec<ParameterShape>,
    /// Every accepted member-name literal, sorted by literal bytes.
    pub names: Vec<NameLiteral>,
    /// Canonical names of required parameters, sorted by bytes.
    pub required: Vec<String>,
    /// Resolved handling of members matching no parameter.
    pub unknown_parameters: UnknownParameters,
}

impl ObjectShape {
    /// The canonical parameter name an exact name or alias literal denotes.
    ///
    /// Exact byte comparison only; returns `None` for any other spelling.
    /// Lookup is a binary search over [`Self::names`].
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
    /// let tool = ToolDefinition::new("read").param("path", Param::string().alias("file"));
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// let object = &tools.argument_shape("read").unwrap().arguments;
    /// assert_eq!(object.resolve_name("file"), Some("path"));
    /// assert_eq!(object.resolve_name("File"), None);
    /// ```
    #[must_use]
    pub fn resolve_name(&self, literal: &str) -> Option<&str> {
        self.names
            .binary_search_by(|entry| entry.literal.as_str().cmp(literal))
            .ok()
            .map(|index| self.names[index].canonical.as_str())
    }

    /// The parameter with this exact canonical name.
    ///
    /// Returns `None` for aliases and unknown names.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet, ValueShape}};
    /// let tool = ToolDefinition::new("sum").param("n", Param::integer());
    /// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
    /// let object = &tools.argument_shape("sum").unwrap().arguments;
    /// assert_eq!(object.parameter("n").unwrap().value, ValueShape::Integer);
    /// ```
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<&ParameterShape> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name == name)
    }
}

/// One accepted member-name literal and the canonical name it denotes.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("read").param("path", Param::string().alias("file"));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let alias = &tools.argument_shape("read").unwrap().arguments.names[0];
/// assert_eq!((alias.literal.as_str(), alias.canonical.as_str(), alias.alias), ("file", "path", true));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameLiteral {
    /// The exact accepted spelling.
    pub literal: String,
    /// The canonical parameter name.
    pub canonical: String,
    /// Whether the literal is an explicit alias rather than the canonical name.
    pub alias: bool,
}

/// One parameter's shape.
///
/// ```
/// use purrassist_core::{Budget, json, tool_call::{Param, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("list")
///     .param("limit", Param::integer().required().default(json::parse("5").unwrap()));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let limit = tools.argument_shape("list").unwrap().arguments.parameter("limit").unwrap();
/// assert!(limit.required && !limit.repeatable);
/// assert_eq!(limit.default.as_ref().unwrap().to_json(), "5");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterShape {
    /// Canonical name.
    pub name: String,
    /// Explicit aliases in declaration order.
    pub aliases: Vec<String>,
    /// Declared type and nested constraints. For a [`Self::repeatable`]
    /// parameter this describes one item: the argument value is an array of
    /// such items.
    pub value: ValueShape,
    /// Whether every call must supply this parameter.
    pub required: bool,
    /// Whether the non-array parameter may repeat. When set, the argument
    /// value is an array of [`Self::value`] items, one per occurrence in
    /// source order (a single occurrence yields a one-element array). Array
    /// parameters always accept repeated occurrences and never set this flag;
    /// any other parameter occurring twice is unresolved.
    pub repeatable: bool,
    /// The declared default; exposed only, never applied by repair.
    pub default: Option<Node>,
}

/// A declared value type with the literals and constraints a grammar needs.
///
/// ```
/// use purrassist_core::{Budget, tool_call::{Param, ToolDefinition, ToolSet, ValueShape}};
/// let tool = ToolDefinition::new("t").param("xs", Param::array(Param::boolean()));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let value = &tools.argument_shape("t").unwrap().arguments.parameters[0].value;
/// assert_eq!(value, &ValueShape::Array(Box::new(ValueShape::Boolean)));
/// assert_eq!(value.type_name(), Some("array"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueShape {
    /// A verbatim string, optionally constrained by a compiled pattern.
    String(Option<PatternConstraint>),
    /// An exact integer.
    Integer,
    /// Any JSON number.
    Number,
    /// A JSON boolean.
    Boolean,
    /// A string from this verbatim literal set, in declared order.
    Enumeration(Vec<String>),
    /// An array of items of the inner shape.
    Array(Box<Self>),
    /// A nested object scope.
    Object(ObjectShape),
    /// A value whose declared schema the typed model cannot express without
    /// loss: for example one using `anyOf`, `oneOf`, `allOf`, `not`,
    /// `if`/`then`/`else`, `$ref`, `const`, `prefixItems`, a type array such
    /// as `["string","null"]`, a non-string or duplicated `enum`, or no
    /// determinable `type`. Only declaration adapters
    /// ([`super::ToolSet::from_openai`] and its siblings) produce it.
    ///
    /// The value is validated only by the tool's exact source schema; repair
    /// treats it structurally, as an opaque JSON value of any type, and never
    /// guesses a narrower shape for it.
    ///
    /// ```
    /// use purrassist_core::{Budget, tool_call::{ToolSet, ValueShape}};
    /// let tools = ToolSet::from_anthropic(
    ///     r#"{"name":"t","input_schema":{"type":"object","properties":{"id":{"type":["string","null"]}}}}"#,
    ///     Budget::default(),
    /// ).unwrap();
    /// let id = &tools.argument_shape("t").unwrap().arguments.parameters[0];
    /// assert_eq!(id.value, ValueShape::Unmodeled);
    /// assert_eq!(id.value.type_name(), None);
    /// ```
    Unmodeled,
}

impl ValueShape {
    /// The JSON Schema `type` keyword value this shape denotes, or `None`
    /// for [`Self::Unmodeled`], which admits any JSON type its exact schema
    /// accepts.
    ///
    /// Enumerations denote `"string"`. For a repeatable parameter this
    /// is the item type; the parameter itself renders as `"array"`. Infallible.
    ///
    /// ```
    /// use purrassist_core::tool_call::ValueShape;
    /// assert_eq!(ValueShape::Enumeration(vec!["a".into()]).type_name(), Some("string"));
    /// assert_eq!(ValueShape::Number.type_name(), Some("number"));
    /// assert_eq!(ValueShape::Unmodeled.type_name(), None);
    /// ```
    #[must_use]
    pub const fn type_name(&self) -> Option<&'static str> {
        match self {
            Self::String(_) | Self::Enumeration(_) => Some("string"),
            Self::Integer => Some("integer"),
            Self::Number => Some("number"),
            Self::Boolean => Some("boolean"),
            Self::Array(_) => Some("array"),
            Self::Object(_) => Some("object"),
            Self::Unmodeled => None,
        }
    }
}

/// A string pattern constraint: its source and its governed compilation.
///
/// Equality compares sources only; compilation is deterministic, so equal
/// sources denote identical patterns.
///
/// ```
/// use purrassist_core::{Budget, Governor, tool_call::{Param, ToolDefinition, ToolSet, ValueShape}};
/// let tool = ToolDefinition::new("open").param("id", Param::string().pattern("^[0-9]+$"));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let ValueShape::String(Some(constraint)) = &tools.argument_shape("open").unwrap().arguments.parameters[0].value
/// else { unreachable!() };
/// assert_eq!(constraint.source, "^[0-9]+$");
/// assert_eq!(constraint.pattern.is_match("42", &mut Governor::new(Budget::default())), Ok(true));
/// ```
#[derive(Clone, Debug)]
pub struct PatternConstraint {
    /// The ECMA-262 pattern source, exactly as declared.
    pub source: String,
    /// The compiled pattern.
    pub pattern: Pattern,
}

impl PartialEq for PatternConstraint {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}
impl Eq for PatternConstraint {}

/// One step from the arguments object to a declared parameter.
///
/// ```
/// use purrassist_core::tool_call::ShapeStep;
/// let path = [ShapeStep::Parameter("items".into()), ShapeStep::Item];
/// assert_eq!(path.len(), 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShapeStep {
    /// The member with this canonical parameter name.
    Parameter(String),
    /// Any item of the enclosing array.
    Item,
}

/// A declared default and the parameter it belongs to.
///
/// Defaults are validated against their parameter's declared type at compile
/// time and exposed for callers and renderers; no repair ever applies one.
///
/// ```
/// use purrassist_core::{Budget, json, tool_call::{Param, ShapeStep, ToolDefinition, ToolSet}};
/// let tool = ToolDefinition::new("list").param("limit", Param::integer().default(json::parse("10").unwrap()));
/// let tools = ToolSet::compile([tool], Budget::default()).unwrap();
/// let default = &tools.defaults("list").unwrap()[0];
/// assert_eq!(default.path, [ShapeStep::Parameter("limit".into())]);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterDefault {
    /// Path from the arguments object to the parameter.
    pub path: Vec<ShapeStep>,
    /// The declared default value.
    pub value: Node,
}
