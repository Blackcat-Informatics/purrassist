// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The GBNF lane: one grammar per tool for its argument object.
//!
//! The grammar is derived from the input JSON Schema the MCP Extended manifest
//! carries, the same schema the compact lane reads, so it cannot disagree with
//! it. It generates the argument object only; the model family's call envelope
//! belongs to the prompt projector.
//!
//! # Language
//!
//! The grammar admits one canonical serialization of each schema-valid value:
//! object members appear in schema property order with required members first,
//! optional members follow in order, and whitespace between tokens is JSON
//! whitespace. Integers, numbers, strings, booleans, `null`, arrays,
//! objects, `enum` and `const` scalars, type arrays and `anyOf` lower exactly.
//! `oneOf` lowers when its alternatives cannot overlap. Members outside
//! `properties` are never generated, which narrows an open object.
//!
//! A `pattern` and the numeric bounds (`minimum`, `maximum`,
//! `exclusiveMinimum`, `exclusiveMaximum`) are represented exactly:
//!
//! - A `pattern` is read in the ECMAScript dialect with the Unicode (`u`)
//!   reading, as JSON Schema 2020-12 Core section 6.4 asks. It is not anchored:
//!   the string must contain a match, and `^` and `$` pin an end only where they
//!   are written. The grammar is the automaton of the core pattern engine's own
//!   search semantics, read one scalar value at a time, so literals and escapes,
//!   classes, `\p{...}`, the dot, alternation, groups, every quantifier,
//!   anchors, `\b` and `\B` are all exact. A string is generated with the
//!   escapes JSON requires (the quote, the backslash and the control
//!   characters) and every other scalar raw: that narrows the spelling of a
//!   value, never the set of values. A pattern that is invalid under `u` only
//!   by an identity escape of a character that is not a syntax character (an
//!   escaped underscore) is read as that literal character.
//! - An integer bound is exact: the grammar spells every integer in the
//!   interval and no other. A decimal bound is exact too: the grammar spells, in
//!   plain notation without an exponent, every finite decimal in the interval
//!   and no other, compared digit by digit with no rounding. See `decimal`.
//! - An `enum` or `const` member that fails a sibling `pattern` or bound is
//!   omitted from the alternatives, which is the same set the schema admits.
//! - `minItems` and `maxItems` admit exactly the stated number of items in an
//!   array. Count rules reuse the item grammar and grow with the bit width of
//!   the bounds rather than with the number of items.
//! - `minLength` and `maxLength` count Unicode scalar values. The canonical
//!   spelling of one scalar consumes one count, including an escaped scalar.
//!   A pattern shares its scalar automaton with a length counter.
//!
//! # Losses
//!
//! A construct that would make the grammar accept a value the schema rejects
//! (an open object, or a keyword such as `format` the grammar does not carry)
//! is omitted from the grammar and recorded on the returned ledger at its schema
//! pointer, never dropped silently; the mandatory `validate-input` check
//! enforces it. A property count and `multipleOf` are refused by
//! name as not yet translated, and `uniqueItems: true` as not expressible. A graph's
//! `sh:pattern` with a back-reference never gets here: the upstream schema
//! compiler refuses it first, as [`GbnfError::Manifest`]. The back-reference
//! branch below is a defence for a schema that arrives by another route; it
//! ledgers the pattern, because no context-free grammar states a repeat of an
//! earlier match. At the entry point the ledger row for this target is exercised
//! by open input shapes. A construct that
//! cannot be lowered soundly at all is a typed refusal, including a pattern with
//! a look-around, whose exact translation is not implemented.

use std::fmt::{self, Write as _};

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, ScratchLease, saturating_size as size},
    json::{self, IntegerReading, Node, Number, ParseFailure, Value},
    pattern::PatternError,
    schema::{CompilationError, Schema},
};
use purrdf_core::loss::{LossLedger, LossSoundnessError, check_ledger_sound_in};

use self::{
    decimal::{Bound, BoundError, Decimal},
    regex::{Failure, Lowered},
};
use super::shared::{entry_for, record_tool, record_upstream, uncarried};
use crate::{
    ProjectionTarget, SOURCE_REPRESENTATION, Tool, ToolGraph, Vocabulary,
    codec::mcp::{self, CodecError},
};

/// A validated, owned GBNF grammar for one tool's argument object.
///
/// The start rule is `root`. Its representation identity is `llama-gbnf`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolGrammar {
    tool: String,
    text: String,
}

impl ToolGrammar {
    /// The tool's name.
    #[must_use]
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// The grammar text: one rule per line, each ending in LF.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Take the owned grammar text.
    #[must_use]
    pub fn into_string(self) -> String {
        self.text
    }
}

/// A refusal to compile a graph to argument grammars. Nothing partial escapes.
#[derive(Debug)]
#[non_exhaustive]
pub enum GbnfError {
    /// The graph has no MCP Extended manifest projection to derive schemas from.
    Manifest(Box<CodecError>),
    /// The projected manifest could not be read back as JSON.
    Json(ParseFailure),
    /// A schema construct the grammar cannot lower soundly: a `$ref`, a boolean
    /// schema, a type that names no supported type, an array without `items`, an
    /// `enum` member that is not a scalar or contradicts the type, a required
    /// member without a property schema, `oneOf` alternatives that may overlap,
    /// a `pattern` or bound beside `anyOf` or `oneOf`,
    /// bounds, a pattern or an `enum` that admit no value, an invalid or
    /// oversized count, a `pattern` with a look-around, a keyword not yet
    /// translated (`minProperties`, `maxProperties`, `multipleOf`), or
    /// `uniqueItems: true`, which no context-free grammar states.
    Unsupported {
        /// The tool's name.
        tool: String,
        /// RFC 6901 pointer from the schema root.
        pointer: String,
        /// What the grammar cannot represent.
        reason: &'static str,
    },
    /// A `pattern` that is not valid under the Unicode reading of the ECMAScript
    /// dialect.
    Pattern {
        /// The tool's name.
        tool: String,
        /// RFC 6901 pointer from the schema root.
        pointer: String,
        /// The syntax error and its offset in the pattern.
        cause: PatternError,
    },
    /// The `pattern` or bounds beside an `enum` or `const` are not a valid
    /// schema, so its members cannot be filtered.
    Constraint {
        /// The tool's name.
        tool: String,
        /// RFC 6901 pointer from the schema root.
        pointer: String,
        /// Why the keywords do not compile.
        cause: Box<CompilationError>,
    },
    /// A recorded loss is not registered in the caller's loss profile.
    Unsound(LossSoundnessError),
    /// The caller refused further work or cancelled the operation.
    Resource(StopReason),
}

impl fmt::Display for GbnfError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(cause) => write!(output, "gbnf manifest: {cause}"),
            Self::Json(cause) => write!(output, "gbnf schema JSON: {cause}"),
            Self::Unsupported {
                tool,
                pointer,
                reason,
            } => write!(output, "tool {tool}: {pointer}: {reason}"),
            Self::Pattern {
                tool,
                pointer,
                cause,
            } => write!(output, "tool {tool}: {pointer}: invalid pattern: {cause}"),
            Self::Constraint {
                tool,
                pointer,
                cause,
            } => write!(
                output,
                "tool {tool}: {pointer}: invalid constraint: {cause}"
            ),
            Self::Unsound(cause) => write!(output, "unregistered loss: {cause}"),
            Self::Resource(cause) => write!(output, "gbnf stopped: {cause:?}"),
        }
    }
}

impl std::error::Error for GbnfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Manifest(cause) => Some(cause.as_ref()),
            Self::Json(cause) => Some(cause),
            Self::Pattern { cause, .. } => Some(cause),
            Self::Constraint { cause, .. } => Some(cause.as_ref()),
            _ => None,
        }
    }
}

/// Compile every tool of `graph` to the GBNF grammar of its argument object.
///
/// Grammars are ordered by tool name bytes and are deterministic: the same graph
/// yields the same bytes. The ledger holds one `LossLedger` entry per omitted
/// source construct under the registered representation `llama-gbnf`, in this
/// order for each tool: the rows common to every lane (authorization, session
/// binding, taint, recovery hints, confinement, resource ceilings, context
/// contract, annotations, RDF binding and description, each located at its
/// source node), `json-string-validity-not-expressible` when the grammar
/// generates a string, `output-shape-not-expressible`, and
/// `input-shape-not-expressible` listing every schema keyword the grammar did not
/// carry by pointer. A construct the upstream schema compiler already dropped is
/// carried as an `input-shape-not-expressible` entry quoting its note. The ledger
/// is checked against the vocabulary's loss profile before it is returned.
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
/// let (grammars, losses) = lanes::gbnf(&graph, &vocabulary, &mut governor).unwrap();
/// assert!(grammars.is_empty());
/// assert!(losses.is_empty());
/// ```
///
/// # Errors
/// [`GbnfError::Manifest`] when the graph has no manifest projection (for
/// example a shape without exactly one `sh:targetClass`),
/// [`GbnfError::Unsupported`] for a schema construct the grammar cannot lower
/// soundly, [`GbnfError::Unsound`] when a ledger entry is not registered, and
/// [`GbnfError::Resource`] when the governor stops the operation. Nothing is
/// returned on failure.
pub fn gbnf(
    graph: &ToolGraph,
    vocabulary: &Vocabulary,
    governor: &mut Governor,
) -> Result<(Vec<ToolGrammar>, LossLedger), GbnfError> {
    let (manifest, upstream) = mcp::encode(graph, vocabulary, governor)
        .map_err(|cause| GbnfError::Manifest(Box::new(cause)))?;
    let root = json::parse_with(manifest.as_str(), governor).map_err(GbnfError::Json)?;
    let declarations = match root.get("tools").map(|node| &node.value) {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    let mut order: Vec<usize> = (0..graph.tools().len()).collect();
    order.sort_by(|&left, &right| graph.tools()[left].name.cmp(&graph.tools()[right].name));
    let mut grammars = Vec::with_capacity(order.len());
    let mut ledger = LossLedger::new();
    for index in order {
        let tool = &graph.tools()[index];
        let schema = declarations
            .get(index)
            .and_then(|node| node.get("inputSchema"))
            .ok_or_else(|| GbnfError::Unsupported {
                tool: tool.name.clone(),
                pointer: "/inputSchema".to_owned(),
                reason: "the manifest declaration has no such schema",
            })?;
        let mut builder = Builder::new(&tool.name);
        let text = builder.grammar(schema, governor)?;
        record_tool(&mut ledger, tool, ProjectionTarget::Gbnf);
        record_schemas(&mut ledger, tool, &builder);
        grammars.push(ToolGrammar {
            tool: tool.name.clone(),
            text,
        });
    }
    record_upstream(
        &mut ledger,
        &upstream,
        ProjectionTarget::Gbnf,
        "input-shape-not-expressible",
    );
    check_ledger_sound_in(
        vocabulary.loss_profile(),
        &ledger,
        SOURCE_REPRESENTATION,
        ProjectionTarget::Gbnf.representation(),
    )
    .map_err(|cause| GbnfError::Unsound(cause.into_owned()))?;
    Ok((grammars, ledger))
}

/// Which helper rules a grammar references.
#[derive(Default)]
struct Helpers {
    string: bool,
    integer: bool,
    number: bool,
}

/// One tool's grammar under construction.
struct Builder<'a> {
    tool: &'a str,
    /// Named rules in creation order; a reserved rule has an empty body until filled.
    rules: Vec<(String, String)>,
    /// Reservations for the count rules and their serialization scratch.
    count_storage: Vec<ScratchLease>,
    helpers: Helpers,
    /// Pointers of omitted constructs, in schema order.
    omitted: Vec<String>,
}

// Bits of the JSON kinds an alternative can generate, for `oneOf` overlap.
const STRING: u8 = 1;
const INTEGER: u8 = 2;
const NUMBER: u8 = 4 | INTEGER;
const BOOLEAN: u8 = 8;
const NULL: u8 = 16;
const ARRAY: u8 = 32;
const OBJECT: u8 = 64;

/// The keywords this lane lowers exactly, for the types they apply to.
const CONSTRAINTS: [&str; 5] = [
    "pattern",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
];
const ARRAY_COUNTS: [&str; 2] = ["minItems", "maxItems"];
const STRING_LENGTHS: [&str; 2] = ["minLength", "maxLength"];
/// Portable admission for either arena record moved by count-rule growth.
const COUNT_RECORD_BYTES: u64 = 64;
const _: () = {
    assert!(std::mem::size_of::<(String, String)>() as u64 <= COUNT_RECORD_BYTES);
    assert!(std::mem::size_of::<ScratchLease>() as u64 <= COUNT_RECORD_BYTES);
};

#[derive(Clone, Copy)]
struct CountBounds {
    minimum: u64,
    maximum: Option<u64>,
}

impl CountBounds {
    const fn unrestricted(self) -> bool {
        self.minimum == 0 && self.maximum.is_none()
    }

    /// Additional occurrences after the required prefix.
    fn additional(self) -> Option<u64> {
        self.maximum.map(|maximum| maximum - self.minimum)
    }

    fn power_bits(self) -> usize {
        self.minimum.max(self.additional().unwrap_or(0)).bit_width() as usize
    }

    fn rule_count(self) -> usize {
        self.power_bits() + 2 * self.additional().unwrap_or(0).bit_width() as usize
    }
}

impl<'a> Builder<'a> {
    const fn new(tool: &'a str) -> Self {
        Self {
            tool,
            rules: Vec::new(),
            count_storage: Vec::new(),
            helpers: Helpers {
                string: false,
                integer: false,
                number: false,
            },
            omitted: Vec::new(),
        }
    }

    fn unsupported(&self, pointer: &str, reason: &'static str) -> GbnfError {
        GbnfError::Unsupported {
            tool: self.tool.to_owned(),
            pointer: pointer.to_owned(),
            reason,
        }
    }

    /// The complete grammar text of an input schema, which must be an object.
    fn grammar(&mut self, schema: &Node, governor: &mut Governor) -> Result<String, GbnfError> {
        match schema.get("type").map(|node| &node.value) {
            Some(Value::String(name)) if name == "object" => {}
            _ => return Err(self.unsupported("", "an input schema must have type object")),
        }
        let start = self.lower(schema, "", governor)?;
        let mut text = format!("root ::= {start}\n");
        for (name, body) in &self.rules {
            let _ = writeln!(text, "{name} ::= {body}");
        }
        text.push_str("ws ::= [ \\t\\n\\r]*\n");
        if self.helpers.string {
            text.push_str(STRING_RULES);
        }
        if self.helpers.integer || self.helpers.number {
            text.push_str("integer ::= \"-\"? (\"0\" | [1-9] [0-9]*)\n");
        }
        if self.helpers.number {
            text.push_str("number ::= integer (\".\" [0-9]+)? ([eE] [-+]? [0-9]+)?\n");
        }
        Ok(text)
    }

    /// Reserve a rule name, fill it later with [`Self::define`].
    fn reserve(&mut self, kind: &str) -> usize {
        self.rules
            .push((format!("{kind}-{}", self.rules.len() + 1), String::new()));
        self.rules.len() - 1
    }

    fn define(&mut self, slot: usize, body: String) -> String {
        self.rules[slot].1 = body;
        self.rules[slot].0.clone()
    }

    /// The expression generating every value of `schema`.
    fn lower(
        &mut self,
        schema: &Node,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        governor
            .charge(Resource::Work, 1)
            .map_err(GbnfError::Resource)?;
        if !matches!(schema.value, Value::Object(_)) {
            return Err(self.unsupported(pointer, "a boolean schema has no grammar"));
        }
        if schema.get("$ref").is_some() {
            return Err(self.unsupported(pointer, "a schema reference has no grammar"));
        }
        let fixed: Vec<(&'static str, &Node)> = ["enum", "const"]
            .into_iter()
            .filter_map(|key| schema.get(key).map(|node| (key, node)))
            .collect();
        let alternatives: Vec<(&'static str, &Node)> = ["anyOf", "oneOf"]
            .into_iter()
            .filter_map(|key| schema.get(key).map(|node| (key, node)))
            .collect();
        match (fixed.as_slice(), alternatives.as_slice()) {
            ([(key, node)], _) => self.literals(schema, pointer, key, node, governor),
            ([], [(key, node)]) => self.union(schema, pointer, key, node, governor),
            ([], []) => self.typed(schema, pointer, governor),
            _ => Err(self.unsupported(
                pointer,
                "enum and const together, or anyOf and oneOf together",
            )),
        }
    }

    /// An `enum` or `const`: the exact JSON text of each scalar member.
    fn literals(
        &mut self,
        schema: &Node,
        pointer: &str,
        key: &'static str,
        node: &Node,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let members: &[Node] = match (&node.value, key) {
            (Value::Array(items), "enum") => items,
            (_, "const") => std::slice::from_ref(node),
            _ => return Err(self.unsupported(pointer, "enum must be an array")),
        };
        if members.is_empty() {
            return Err(self.unsupported(pointer, "an empty enum admits no value"));
        }
        self.refuse_untranslated(schema, pointer, "*")?;
        let types = type_names(schema);
        let constraint = self.constraint(schema, pointer, governor)?;
        let mut names = Vec::with_capacity(members.len());
        for member in members {
            let admitted = match &member.value {
                Value::String(_) => ["string"].as_slice(),
                Value::Bool(_) => &["boolean"],
                Value::Null => &["null"],
                Value::Number(number) if number.is_integer() => &["integer", "number"],
                Value::Number(_) => &["number"],
                _ => {
                    return Err(self.unsupported(pointer, "an enum member that is not a scalar"));
                }
            };
            if types
                .as_ref()
                .is_some_and(|types| !types.iter().any(|name| admitted.contains(name)))
            {
                return Err(self.unsupported(pointer, "an enum member outside the schema type"));
            }
            // A member the sibling pattern or bounds reject is not a value.
            if let Some(constraint) = &constraint
                && !constraint
                    .validate_with(member, governor)
                    .map_err(GbnfError::Resource)?
                    .valid
            {
                continue;
            }
            let text = member
                .to_json_governed(governor)
                .map_err(GbnfError::Resource)?;
            names.push(literal(&text));
        }
        if names.is_empty() {
            return Err(self.unsupported(pointer, "no enum member satisfies its pattern or bounds"));
        }
        let mut carried = vec![key, "type"];
        carried.extend(CONSTRAINTS);
        carried.extend(ARRAY_COUNTS);
        carried.extend(STRING_LENGTHS);
        self.omitted.extend(uncarried(schema, pointer, &carried));
        Ok(alternation(names))
    }

    /// An `anyOf` or `oneOf`; `oneOf` alternatives must not overlap.
    fn union(
        &mut self,
        schema: &Node,
        pointer: &str,
        key: &str,
        node: &Node,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let Value::Array(items) = &node.value else {
            return Err(self.unsupported(pointer, "anyOf and oneOf must be arrays"));
        };
        self.refuse_untranslated(schema, pointer, "*")?;
        for name in ARRAY_COUNTS {
            if schema.get(name).is_some() {
                return Err(self.unsupported(
                    &format!("{pointer}/{name}"),
                    "an item count beside anyOf or oneOf",
                ));
            }
        }
        for name in STRING_LENGTHS {
            if schema.get(name).is_some() {
                return Err(self.unsupported(
                    &format!("{pointer}/{name}"),
                    "a string length beside anyOf or oneOf",
                ));
            }
        }
        if CONSTRAINTS.iter().any(|name| schema.get(name).is_some()) {
            return Err(
                self.unsupported(pointer, "a pattern or numeric bound beside anyOf or oneOf")
            );
        }
        if items.is_empty() {
            return Err(self.unsupported(pointer, "an empty anyOf or oneOf admits no value"));
        }
        let mut seen = 0_u8;
        let mut names = Vec::with_capacity(items.len());
        for (position, item) in items.iter().enumerate() {
            let child = format!("{pointer}/{key}/{position}");
            if key == "oneOf" {
                let kinds = kinds(item).ok_or_else(|| {
                    self.unsupported(&child, "a oneOf alternative whose overlap is undecidable")
                })?;
                if seen & kinds != 0 {
                    return Err(self.unsupported(&child, "oneOf alternatives that overlap"));
                }
                seen |= kinds;
            }
            names.push(self.lower(item, &child, governor)?);
        }
        self.omitted
            .extend(uncarried(schema, pointer, &["anyOf", "oneOf"]));
        Ok(alternation(names))
    }

    /// A schema selected by its `type` keyword.
    fn typed(
        &mut self,
        schema: &Node,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let Some(types) = type_names(schema) else {
            return Err(self.unsupported(pointer, "a schema without type, enum, anyOf or oneOf"));
        };
        // A pattern or bound is lowered, ledgered by name, or inert for the type.
        let mut carried = vec!["type"];
        carried.extend(CONSTRAINTS);
        carried.extend(ARRAY_COUNTS);
        carried.extend(STRING_LENGTHS);
        let mut names = Vec::with_capacity(types.len());
        for name in &types {
            self.refuse_untranslated(schema, pointer, name)?;
            match *name {
                "string" => names.push(self.string(schema, pointer, governor)?),
                "integer" => names.push(self.numeric(schema, pointer, true, governor)?),
                "number" => names.push(self.numeric(schema, pointer, false, governor)?),
                "boolean" => names.push(r#"("true" | "false")"#.to_owned()),
                "null" => names.push(r#""null""#.to_owned()),
                "array" => {
                    carried.push("items");
                    names.push(self.array(schema, pointer, governor)?);
                }
                "object" => {
                    carried.extend(["properties", "required"]);
                    if matches!(
                        schema.get("additionalProperties").map(|node| &node.value),
                        Some(Value::Bool(false))
                    ) {
                        carried.push("additionalProperties");
                    }
                    names.push(self.object(schema, pointer, governor)?);
                }
                _ => return Err(self.unsupported(pointer, "an unknown type name")),
            }
        }
        self.omitted.extend(uncarried(schema, pointer, &carried));
        Ok(alternation(names))
    }

    /// Refuse a size, divisibility or uniqueness keyword of the type `name`.
    ///
    /// A keyword a grammar can state but this compiler does not translate yet is
    /// refused as not yet translated; uniqueness of items no context-free
    /// grammar states, and is refused as not expressible.
    fn refuse_untranslated(
        &self,
        schema: &Node,
        pointer: &str,
        name: &str,
    ) -> Result<(), GbnfError> {
        let keywords: &[(&str, &'static str)] = match name {
            "array" => &[(
                "uniqueItems",
                "uniqueItems is not expressible by a context-free grammar",
            )],
            "object" => &[
                ("minProperties", "minProperties is not yet translated"),
                ("maxProperties", "maxProperties is not yet translated"),
            ],
            "integer" | "number" => &[("multipleOf", "multipleOf is not yet translated")],
            // An `enum`, `const`, `anyOf` or `oneOf` has no type of its own: any is refused.
            "*" => &[
                (
                    "uniqueItems",
                    "uniqueItems is not expressible by a context-free grammar",
                ),
                ("minProperties", "minProperties is not yet translated"),
                ("maxProperties", "maxProperties is not yet translated"),
                ("multipleOf", "multipleOf is not yet translated"),
            ],
            _ => &[],
        };
        for &(keyword, reason) in keywords {
            let present = schema.get(keyword).is_some_and(|node| {
                // `uniqueItems: false` constrains nothing.
                !matches!(node.value, Value::Bool(false))
            });
            if present {
                return Err(self.unsupported(&format!("{pointer}/{keyword}"), reason));
            }
        }
        Ok(())
    }

    /// A string with its optional exact scalar count and pattern.
    fn string(
        &mut self,
        schema: &Node,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let counts = CountBounds {
            minimum: self
                .count_bound(schema, pointer, "minLength", governor)?
                .unwrap_or(0),
            maximum: self.count_bound(schema, pointer, "maxLength", governor)?,
        };
        if counts
            .maximum
            .is_some_and(|maximum| maximum < counts.minimum)
        {
            return Err(self.unsupported(
                &format!("{pointer}/maxLength"),
                "maxLength is less than minLength",
            ));
        }
        let Some(node) = schema.get("pattern") else {
            return self.string_without_pattern(counts, governor);
        };
        let child = format!("{pointer}/pattern");
        let Value::String(source) = &node.value else {
            return Err(self.unsupported(&child, "a pattern that is not a string"));
        };
        match regex::lower(source, governor) {
            Ok(Lowered::Automaton(automaton)) => {
                self.pattern_string(&automaton, counts, pointer, governor)
            }
            Ok(Lowered::Backreference) => {
                self.omitted.push(format!(
                    "{child} (back-reference: the accepted set repeats an earlier match, which no context-free grammar states)"
                ));
                self.string_without_pattern(counts, governor)
            }
            Ok(Lowered::Lookaround) => Err(self.unsupported(
                &child,
                "look-around is not yet translated: its exact translation needs an automaton product with a size bound, filed as issue 224",
            )),
            Err(Failure::Syntax(cause)) => Err(GbnfError::Pattern {
                tool: self.tool.to_owned(),
                pointer: child,
                cause,
            }),
            Err(Failure::Empty) => {
                Err(self.unsupported(&child, "no string contains a match of the pattern"))
            }
            Err(Failure::Unnamed) => Err(self.unsupported(
                &child,
                "a pattern construct that needs backtracking and is neither a back-reference nor a look-around",
            )),
            Err(Failure::Resource(reason)) => Err(GbnfError::Resource(reason)),
        }
    }

    fn string_without_pattern(
        &mut self,
        counts: CountBounds,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        if counts.unrestricted() {
            self.helpers.string = true;
            return Ok("string".to_owned());
        }
        if counts.maximum == Some(0) {
            return Ok(literal(r#""""#));
        }
        let rules = counts.rule_count() + 2;
        self.reserve_count_rules(rules, 512 * size(rules) + 2048, governor)?;
        let low = json_edge('\0', '\u{D7FF}');
        let high = json_edge('\u{E000}', char::MAX);
        let character = self.reserve("scalar");
        let unit = self.define(character, format!("{low} | {high}"));
        let sequence = self.counted_sequence(&unit, counts, "scalar");
        let quote = literal("\"");
        let string = self.reserve("string");
        Ok(self.define(string, format!("{quote}{sequence} {quote}")))
    }

    /// Emit the exact scalar automaton, with productive counter states when
    /// bounded. A finite ceiling removes outgoing edges in its final layer.
    /// A minimum-only counter saturates and follows the base graph there.
    fn pattern_string(
        &mut self,
        automaton: &regex::Automaton,
        counts: CountBounds,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let width = automaton.states.len();
        let keyword = if counts.maximum.is_some() {
            "maxLength"
        } else {
            "minLength"
        };
        let tool = self.tool;
        let too_large = || GbnfError::Unsupported {
            tool: tool.to_owned(),
            pointer: format!("{pointer}/{keyword}"),
            reason: "the pattern length product exceeds the 32-bit rule space",
        };
        let limit = counts.maximum.unwrap_or(counts.minimum);
        let layers = limit.checked_add(1).ok_or_else(too_large)?;
        let cells = layers
            .checked_mul(size(width))
            .filter(|&cells| cells < u64::from(u32::MAX))
            .and_then(|cells| u32::try_from(cells).ok())
            .ok_or_else(too_large)? as usize;
        let layers = usize::try_from(layers).map_err(|_| too_large())?;
        if !counts.unrestricted() {
            let rules = cells.checked_add(1).ok_or_else(too_large)?;
            governor
                .charge(Resource::Work, size(width))
                .map_err(GbnfError::Resource)?;
            let edges = automaton.states.iter().fold(0_u64, |edges, state| {
                edges.saturating_add(size(state.edges.len()))
            });
            let edge_layers = layers - usize::from(counts.maximum.is_some());
            // Cover each slot, rule record and emission scratch, plus each
            // edge's scalar classes and escaped spellings in its layers.
            let bytes = size(rules)
                .saturating_mul(512)
                .saturating_add(edges.saturating_mul(size(edge_layers)).saturating_mul(2048));
            self.reserve_count_rules(rules, bytes, governor)?;
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(cells)
            .map_err(|_| GbnfError::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes)))?;
        slots.resize(cells, None);
        for layer in (0..layers).rev() {
            let start = layer * width;
            let final_layer = layer + 1 == layers;
            for (index, state) in automaton.states.iter().enumerate() {
                let accepting = size(layer) >= counts.minimum && state.accepting;
                let productive = if final_layer {
                    counts.maximum.is_none() || accepting
                } else {
                    accepting
                        || state
                            .edges
                            .iter()
                            .any(|&(_, _, to)| slots[start + width + to].is_some())
                };
                if productive {
                    slots[start + index] = Some(self.reserve("pattern"));
                }
            }
        }
        let Some(root) = slots[0] else {
            return Err(self.unsupported(
                &format!("{pointer}/{keyword}"),
                "the pattern and length bounds admit no string",
            ));
        };
        self.fill_pattern_rules(automaton, counts, &slots);
        let body = format!("{} {}", literal("\""), self.rules[root].0);
        let entry = self.reserve("string");
        Ok(self.define(entry, body))
    }

    /// Emit only edges whose target remains productive in the next layer.
    fn fill_pattern_rules(
        &mut self,
        automaton: &regex::Automaton,
        counts: CountBounds,
        slots: &[Option<usize>],
    ) {
        let width = automaton.states.len();
        let layers = slots.len() / width;
        for (layer, row) in slots.chunks_exact(width).enumerate() {
            let targets = if layer + 1 < layers {
                let start = (layer + 1) * width;
                &slots[start..start + width]
            } else if counts.maximum.is_none() {
                row
            } else {
                &[]
            };
            for (&slot, state) in row.iter().zip(&automaton.states) {
                if let Some(slot) = slot {
                    let edges = if targets.is_empty() {
                        &[]
                    } else {
                        state.edges.as_slice()
                    };
                    let mut alternatives: Vec<String> = edges
                        .iter()
                        .filter_map(|&(first, last, target)| {
                            let next = targets.get(target).copied().flatten()?;
                            Some(format!("{} {}", json_edge(first, last), self.rules[next].0))
                        })
                        .collect();
                    if size(layer) >= counts.minimum && state.accepting {
                        alternatives.push(literal("\""));
                    }
                    self.rules[slot].1 = alternatives.join(" | ");
                }
            }
        }
    }

    /// An integer or number: the generic helper, or the digits its bounds admit.
    fn numeric(
        &mut self,
        schema: &Node,
        pointer: &str,
        integer: bool,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let lower = self.bound(
            schema,
            pointer,
            ["minimum", "exclusiveMinimum"],
            true,
            governor,
        )?;
        let upper = self.bound(
            schema,
            pointer,
            ["maximum", "exclusiveMaximum"],
            false,
            governor,
        )?;
        if lower.is_none() && upper.is_none() {
            if integer {
                self.helpers.integer = true;
                return Ok("integer".to_owned());
            }
            self.helpers.number = true;
            return Ok("number".to_owned());
        }
        let body = if integer {
            decimal::integer(lower.as_ref(), upper.as_ref())
        } else {
            decimal::number(lower.as_ref(), upper.as_ref())
        };
        let Some(body) = body else {
            return Err(self.unsupported(pointer, "numeric bounds that admit no value"));
        };
        let slot = self.reserve(if integer {
            "bounded-integer"
        } else {
            "bounded-number"
        });
        Ok(self.define(slot, body))
    }

    /// The tightest of the inclusive and exclusive forms of one bound.
    fn bound(
        &self,
        schema: &Node,
        pointer: &str,
        keys: [&'static str; 2],
        lower: bool,
        governor: &mut Governor,
    ) -> Result<Option<Bound>, GbnfError> {
        let mut tightest: Option<(&Number, bool)> = None;
        for (key, exclusive) in keys.into_iter().zip([false, true]) {
            let Some(node) = schema.get(key) else {
                continue;
            };
            let Value::Number(number) = &node.value else {
                return Err(
                    self.unsupported(&format!("{pointer}/{key}"), "a bound that is not a number")
                );
            };
            let wins = tightest.is_none_or(|(held, held_exclusive)| match number.compare(held) {
                std::cmp::Ordering::Equal => exclusive && !held_exclusive,
                order => (order.is_gt()) == lower,
            });
            if wins {
                tightest = Some((number, exclusive));
            }
        }
        tightest
            .map(|(number, exclusive)| {
                Decimal::read(number, governor)
                    .map(|value| Bound::new(value, exclusive))
                    .map_err(|cause| match cause {
                        BoundError::Resource(reason) => GbnfError::Resource(reason),
                        BoundError::Exponent => self.unsupported(
                            pointer,
                            "a bound whose exponent does not fit a digit grammar",
                        ),
                    })
            })
            .transpose()
    }

    /// The sibling `pattern` and bounds of an `enum` or `const` as one schema.
    fn constraint(
        &self,
        schema: &Node,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<Option<Schema>, GbnfError> {
        let mut members = Vec::new();
        for key in CONSTRAINTS.into_iter().chain(STRING_LENGTHS) {
            if let Some(node) = schema.get(key) {
                let text = node
                    .to_json_governed(governor)
                    .map_err(GbnfError::Resource)?;
                members.push(format!("\"{key}\":{text}"));
            }
        }
        if members.is_empty() {
            return Ok(None);
        }
        Schema::compile_governed(&format!("{{{}}}", members.join(",")), governor)
            .map(Some)
            .map_err(|cause| match cause {
                CompilationError::Resource(reason) => GbnfError::Resource(reason),
                invalid @ CompilationError::InvalidSchema(_) => GbnfError::Constraint {
                    tool: self.tool.to_owned(),
                    pointer: pointer.to_owned(),
                    cause: Box::new(invalid),
                },
            })
    }

    fn array(
        &mut self,
        schema: &Node,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let minimum = self
            .count_bound(schema, pointer, "minItems", governor)?
            .unwrap_or(0);
        let maximum = self.count_bound(schema, pointer, "maxItems", governor)?;
        if maximum.is_some_and(|maximum| maximum < minimum) {
            return Err(self.unsupported(
                &format!("{pointer}/maxItems"),
                "maxItems is less than minItems",
            ));
        }
        if maximum == Some(0) {
            let slot = self.reserve("array");
            return Ok(self.define(slot, r#""[" ws "]""#.to_owned()));
        }
        let Some(items) = schema.get("items") else {
            return Err(self.unsupported(pointer, "an array schema without items"));
        };
        let slot = self.reserve("array");
        let item = self.lower(items, &format!("{pointer}/items"), governor)?;
        let body = if minimum == 0 && maximum.is_none() {
            format!(r#""[" ws ({item} (ws "," ws {item})*)? ws "]""#)
        } else {
            self.counted_items(item, minimum, maximum, governor)?
        };
        Ok(self.define(slot, body))
    }

    fn count_bound(
        &self,
        schema: &Node,
        pointer: &str,
        keyword: &str,
        governor: &mut Governor,
    ) -> Result<Option<u64>, GbnfError> {
        let Some(node) = schema.get(keyword) else {
            return Ok(None);
        };
        let invalid = || {
            self.unsupported(
                &format!("{pointer}/{keyword}"),
                "a count bound must be a nonnegative integer",
            )
        };
        let too_large = || {
            self.unsupported(
                &format!("{pointer}/{keyword}"),
                "a count bound exceeds the 64-bit grammar bound",
            )
        };
        let Value::Number(number) = &node.value else {
            return Err(invalid());
        };
        match number
            .integer_magnitude_governed(governor)
            .map_err(GbnfError::Resource)?
        {
            IntegerReading::Exact {
                negative: false,
                magnitude,
            } => u64::try_from(magnitude).map(Some).map_err(|_| too_large()),
            IntegerReading::OutOfRange if !number.as_str().starts_with('-') => Err(too_large()),
            _ => Err(invalid()),
        }
    }

    /// Count tails after the delimiter-free first item. For a zero minimum
    /// the whole nonempty branch is optional.
    fn counted_items(
        &mut self,
        item: String,
        minimum: u64,
        maximum: Option<u64>,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let first_count = minimum.max(1);
        let tail = CountBounds {
            minimum: first_count - 1,
            maximum: maximum.map(|maximum| maximum - 1),
        };
        let rules = 1 + tail.rule_count();
        // Each generated name has at most twenty counter digits. Admit the
        // rule strings and their enclosing references before emitting them.
        let bytes = (512 * size(rules)).saturating_add(size(item.len()).saturating_mul(3));
        self.reserve_count_rules(rules, bytes, governor)?;

        let entry = self.reserve("array-item");
        let item = self.define(entry, item);
        let separator = format!(r#"ws "," ws {item}"#);
        let tail = self.counted_sequence(&separator, tail, "array");
        let nonempty = format!("{item}{tail}");
        let inner = if minimum == 0 {
            format!("({nonempty})?")
        } else {
            nonempty
        };
        Ok(format!(r#""[" ws {inner} ws "]""#))
    }

    /// Append the counted uses of one expression after an existing prefix.
    /// The caller reserves the rules and the sequence's rendered storage.
    fn counted_sequence(&mut self, atom: &str, bounds: CountBounds, family: &str) -> String {
        let power_bits = bounds.power_bits();
        let power_kind = format!("{family}-power");
        let powers = self.rules.len();
        for bit in 0..power_bits {
            let body = if bit == 0 {
                atom.to_owned()
            } else {
                let previous = &self.rules[powers + bit - 1].0;
                format!("{previous} {previous}")
            };
            let rule = self.reserve(&power_kind);
            self.rules[rule].1 = body;
        }

        let mut body = String::new();
        for bit in 0..power_bits {
            if bounds.minimum & (1 << bit) != 0 {
                let _ = write!(body, " {}", self.rules[powers + bit].0);
            }
        }
        if let Some(limit) = bounds.additional() {
            if let Some(rule) = self.bounded_tail(powers, limit, family) {
                let _ = write!(body, " {}", self.rules[rule].0);
            }
        } else {
            let _ = write!(body, " ({atom})*");
        }
        body
    }

    /// A count rule can grow an arena that already holds schema rules.
    /// Admit the replacement capacity and live record copies before reserve.
    fn reserve_count_rules(
        &mut self,
        additional: usize,
        bytes: u64,
        governor: &mut Governor,
    ) -> Result<(), GbnfError> {
        let (rule_space, rule_moves) =
            count_arena_growth(self.rules.len(), self.rules.capacity(), additional);
        let (lease_space, lease_moves) =
            count_arena_growth(self.count_storage.len(), self.count_storage.capacity(), 1);
        let moves = size(rule_moves.saturating_add(lease_moves));
        let spaces = size(rule_space.saturating_add(lease_space)).saturating_add(moves);
        governor
            .charge(
                Resource::Work,
                bytes.saturating_add(moves.saturating_mul(COUNT_RECORD_BYTES)),
            )
            .map_err(GbnfError::Resource)?;
        let owned = governor
            .lease_scratch(bytes.saturating_add(spaces.saturating_mul(COUNT_RECORD_BYTES)))
            .map_err(GbnfError::Resource)?;
        self.rules
            .try_reserve_exact(rule_space)
            .map_err(|_| GbnfError::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes)))?;
        self.count_storage
            .try_reserve_exact(lease_space)
            .map_err(|_| GbnfError::Resource(StopReason::BudgetExceeded(Resource::ScratchBytes)))?;
        self.count_storage.push(owned);
        Ok(())
    }

    /// For a bound with highest set bit `k`, disjoint alternatives accept
    /// fewer than two to the power of `k` tails, or that many followed by
    /// at most the remainder. Reuse the fully occupied lower bit ranges.
    fn bounded_tail(&mut self, powers: usize, limit: u64, family: &str) -> Option<usize> {
        let bits = limit.bit_width() as usize;
        if bits == 0 {
            return None;
        }
        let complete_kind = format!("{family}-complete");
        let bounded_kind = format!("{family}-bounded");
        let complete = self.rules.len();
        for bit in 0..bits.saturating_sub(1) {
            let power = &self.rules[powers + bit].0;
            let body = if bit == 0 {
                format!("{power}?")
            } else {
                format!("{} {power}?", self.rules[complete + bit - 1].0)
            };
            let rule = self.reserve(&complete_kind);
            self.rules[rule].1 = body;
        }
        let mut bounded = None::<usize>;
        for bit in 0..bits {
            if limit & (1 << bit) == 0 {
                continue;
            }
            if bit == 0 && bits > 1 {
                bounded = Some(complete);
                continue;
            }
            let power = &self.rules[powers + bit].0;
            let body = if bit == 0 {
                format!("{power}?")
            } else {
                let smaller = &self.rules[complete + bit - 1].0;
                match bounded {
                    Some(remainder) => format!("{smaller} | {power} {}", self.rules[remainder].0),
                    None => format!("{smaller} | {power}"),
                }
            };
            let rule = self.reserve(&bounded_kind);
            self.rules[rule].1 = body;
            bounded = Some(rule);
        }
        bounded
    }

    /// Required members first, then optional members, each in property order.
    fn object(
        &mut self,
        schema: &Node,
        pointer: &str,
        governor: &mut Governor,
    ) -> Result<String, GbnfError> {
        let required: Vec<&str> = match schema.get("required").map(|node| &node.value) {
            Some(Value::Array(items)) => items.iter().filter_map(Node::as_str).collect(),
            _ => Vec::new(),
        };
        let properties: &[json::Member] = match schema.get("properties").map(|node| &node.value) {
            Some(Value::Object(members)) => members,
            _ => &[],
        };
        if required
            .iter()
            .any(|key| !properties.iter().any(|member| member.key == *key))
        {
            let child = format!("{pointer}/required");
            return Err(self.unsupported(&child, "a required member without a property schema"));
        }
        if !matches!(
            schema.get("additionalProperties").map(|node| &node.value),
            Some(Value::Bool(false))
        ) {
            self.omitted.push(format!(
                "{pointer}/additionalProperties (members outside properties are not generated)"
            ));
        }
        let slot = self.reserve("object");
        let (mut mandatory, mut optional) = (Vec::new(), Vec::new());
        for member in properties {
            let child = format!(
                "{pointer}/properties/{}",
                json::pointer_segment(&member.key)
            );
            let value = self.lower(&member.value, &child, governor)?;
            let key = json::quote_governed(&member.key, governor).map_err(GbnfError::Resource)?;
            let rule = self.reserve("member");
            let rule = self.define(rule, format!(r#"{} ws ":" ws {value}"#, literal(&key)));
            if required.contains(&member.key.as_str()) {
                mandatory.push(rule);
            } else {
                optional.push(rule);
            }
        }
        let comma = r#"ws "," ws"#;
        let mut inner = mandatory.join(&format!(" {comma} "));
        if mandatory.is_empty() {
            // The first member present opens the object, so each choice owns the
            // optional members that follow it.
            let choices: Vec<String> = (0..optional.len())
                .map(|first| {
                    let mut choice = optional[first].clone();
                    for rule in &optional[first + 1..] {
                        let _ = write!(choice, " ({comma} {rule})?");
                    }
                    choice
                })
                .collect();
            if !choices.is_empty() {
                inner = format!("({})?", choices.join(" | "));
            }
        } else {
            for rule in &optional {
                let _ = write!(inner, " ({comma} {rule})?");
            }
        }
        let body = if inner.is_empty() {
            r#""{" ws "}""#.to_owned()
        } else {
            format!(r#""{{" ws {inner} ws "}}""#)
        };
        Ok(self.define(slot, body))
    }
}

/// Additional reserve and live record copies for geometric arena growth.
fn count_arena_growth(len: usize, capacity: usize, additional: usize) -> (usize, usize) {
    let needed = len.saturating_add(additional);
    if needed <= capacity {
        (0, 0)
    } else {
        (needed.max(capacity.saturating_mul(2)) - len, len)
    }
}

/// The helper rules a grammar with strings references.
const STRING_RULES: &str = concat!(
    "string ::= \"\\\"\" character* \"\\\"\"\n",
    "character ::= [^\"\\\\\\x00-\\x1f] | \"\\\\\" ([\"\\\\/bfnrt] | \"u\" hex hex hex hex)\n",
    "hex ::= [0-9a-fA-F]\n",
);

/// The `type` names of a schema, whether one string or an array of strings.
fn type_names(schema: &Node) -> Option<Vec<&str>> {
    match schema.get("type").map(|node| &node.value) {
        Some(Value::String(name)) => Some(vec![name.as_str()]),
        Some(Value::Array(items)) => items.iter().map(Node::as_str).collect(),
        _ => None,
    }
}

/// The JSON kinds a schema can generate, or `None` when they cannot be told.
fn kinds(schema: &Node) -> Option<u8> {
    for key in ["enum", "const"] {
        if let Some(node) = schema.get(key) {
            let members = match &node.value {
                Value::Array(items) if key == "enum" => items.as_slice(),
                _ if key == "const" => std::slice::from_ref(node),
                _ => return None,
            };
            return members.iter().try_fold(0, |all, member| {
                Some(
                    all | match &member.value {
                        Value::String(_) => STRING,
                        Value::Bool(_) => BOOLEAN,
                        Value::Null => NULL,
                        Value::Number(number) if number.is_integer() => INTEGER,
                        Value::Number(_) => NUMBER,
                        _ => return None,
                    },
                )
            });
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(node) = schema.get(key) {
            let Value::Array(items) = &node.value else {
                return None;
            };
            return items
                .iter()
                .try_fold(0, |all, item| Some(all | kinds(item)?));
        }
    }
    type_names(schema)?.iter().try_fold(0, |all, name| {
        Some(
            all | match *name {
                "string" => STRING,
                "integer" => INTEGER,
                "number" => NUMBER,
                "boolean" => BOOLEAN,
                "null" => NULL,
                "array" => ARRAY,
                "object" => OBJECT,
                _ => return None,
            },
        )
    })
}

/// `expressions` as one parenthesized alternation, or the single expression itself.
fn alternation(mut expressions: Vec<String>) -> String {
    expressions.dedup();
    match expressions.as_slice() {
        [only] => only.clone(),
        _ => format!("({})", expressions.join(" | ")),
    }
}

/// The JSON text of the scalar values `first..=last` inside a string: each raw
/// unless JSON requires an escape (the quote, the backslash, a control character).
fn json_edge(first: char, last: char) -> String {
    let (low, high) = (u32::from(first), u32::from(last));
    let (mut runs, mut escaped) = (Vec::new(), Vec::new());
    let mut cursor = low;
    for (start, end) in [(0, 0x1F), (0x22, 0x22), (0x5C, 0x5C)] {
        if end < cursor || start > high {
            continue;
        }
        if start > cursor {
            runs.push((cursor, start - 1));
        }
        escaped.extend(start.max(low)..=end.min(high));
        cursor = end + 1;
    }
    if cursor <= high {
        runs.push((cursor, high));
    }
    let mut parts = Vec::new();
    match runs.as_slice() {
        [] => {}
        &[(only, same)] if only == same => {
            parts.push(literal(&char::from_u32(only).map_or_default(String::from)));
        }
        ranges => {
            let mut class = String::from("[");
            for &(start, end) in ranges {
                class.push_str(&class_character(start));
                if end != start {
                    class.push('-');
                    class.push_str(&class_character(end));
                }
            }
            class.push(']');
            parts.push(class);
        }
    }
    for code in escaped {
        let text = match code {
            0x22 => "\\\"".to_owned(),
            0x5C => "\\\\".to_owned(),
            0x08 => "\\b".to_owned(),
            0x09 => "\\t".to_owned(),
            0x0A => "\\n".to_owned(),
            0x0C => "\\f".to_owned(),
            0x0D => "\\r".to_owned(),
            other => format!("\\u{other:04x}"),
        };
        parts.push(literal(&text));
    }
    match parts.as_slice() {
        [only] => only.clone(),
        _ => format!("({})", parts.join(" | ")),
    }
}

/// One scalar value inside a GBNF character class.
fn class_character(code: u32) -> String {
    match char::from_u32(code) {
        Some(character) if character.is_ascii_alphanumeric() => character.to_string(),
        _ if code <= 0xFF => format!("\\x{code:02X}"),
        _ if code <= 0xFFFF => format!("\\u{code:04X}"),
        _ => format!("\\U{code:08X}"),
    }
}

/// `text` as a GBNF string literal.
fn literal(text: &str) -> String {
    let mut output = String::with_capacity(text.len() + 2);
    output.push('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control.is_control() => {
                let _ = write!(output, "\\x{:02X}", u32::from(control));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

/// The omission rows of a tool's grammar, in the documented order.
fn record_schemas(ledger: &mut LossLedger, tool: &Tool, builder: &Builder<'_>) {
    let target = ProjectionTarget::Gbnf;
    if builder.helpers.string {
        ledger.record(entry_for(
            target,
            "json-string-validity-not-expressible",
            "the string grammar admits lone UTF-16 surrogate escapes and does not check Unicode validity; mandatory check validate-input".to_owned(),
            Some((&tool.input_shape, "inputSchema")),
        ));
    }
    ledger.record(entry_for(
        target,
        "output-shape-not-expressible",
        "the grammar covers arguments only; mandatory check validate-output".to_owned(),
        Some((&tool.output_shape, "outputSchema")),
    ));
    if !builder.omitted.is_empty() {
        let note = format!(
            "schema constructs omitted from the grammar at {}; mandatory check validate-input",
            builder.omitted.join(", ")
        );
        ledger.record(entry_for(
            target,
            "input-shape-not-expressible",
            note,
            Some((&tool.input_shape, "inputSchema")),
        ));
    }
}

mod decimal;
mod regex;
#[cfg(test)]
mod tests;
