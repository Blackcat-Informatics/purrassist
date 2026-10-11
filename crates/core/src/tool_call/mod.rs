// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Typed tool definitions compiled into validated, indexed tool sets.
//!
//! A [`ToolDefinition`] declares a tool's canonical name, aliases, effects,
//! policies, and typed parameters. [`ToolSet::compile_governed`] checks every
//! definition under a governor and produces, per tool, a canonical Draft
//! 2020-12 argument schema (text, document, and compiled validator), an
//! engine-independent [`ArgumentShape`] for tool-call grammars, and the
//! validated, never-applied defaults. [`FromNode`] converts validated
//! argument values into Rust types exactly.
//!
//! Wire declarations (`OpenAI` `tools[].function`, MCP `tools/list`, Anthropic
//! `tools[]`) are the primary input path: [`ToolSet::from_openai`],
//! [`ToolSet::from_mcp`], and [`ToolSet::from_anthropic`] keep each
//! declaration's exact bytes, compile its exact parameters schema, project it
//! into the typed shape model, list every keyword the model does not
//! represent, and render it back byte for byte (see [`DeclarationForm`]).
//!
//! [`ToolSet::recover`] and [`ToolSet::recover_governed`] recover the tool
//! calls of one model turn (the `tool_call.recover.v1` operation): the
//! [`Recognizer`] segments the raw output over a [`DialectSet`],
//! [`resolve_forest`] resolves every call against the tool set, and the
//! result is a [`ToolCalls`] value with per-call outcomes, itemized repairs,
//! ids, the tool choice check, and how the turn finished, plus a receipt
//! (see [`recover`] for every step and outcome rule).
//!
//! [`emit`] prints in the other direction, through the same dialect
//! grammars: recovered calls as canonical markup of any dialect or as the
//! `OpenAI` `tool_calls` array, tool results, tool declarations
//! ([`DeclarationForm`]), and minimal-diff re-emission of edited calls
//! ([`ToolCalls::reemit_with`]).
//!
//! Names (tools, aliases, parameters, parameter aliases) are non-empty and
//! match `[A-Za-z0-9_.:-]+`; two collide when their [`collision_key`]s are
//! equal. Results never depend on declaration order.
//!
//! ```
//! use purrassist_core::{Budget, json, tool_call::{Effects, FromNode, Param, ToolDefinition, ToolSet}};
//! let tool = ToolDefinition::new("read_file")
//!     .effects(Effects::ReadOnly)
//!     .param("path", Param::string().required().alias("file"))
//!     .param("limit", Param::integer().default(json::parse("100").unwrap()));
//! let tools = ToolSet::compile([tool], Budget::default()).unwrap();
//! let compiled = tools.lookup_exact("read_file").unwrap();
//! let arguments = json::parse(r#"{"path":"README.md","limit":5}"#).unwrap();
//! assert!(compiled.schema().validate(&arguments).unwrap().valid);
//! assert_eq!(u32::from_node(arguments.get("limit").unwrap()), Ok(5));
//! ```
mod adapter;
mod context;
mod definition;
mod derive_support;
mod detect;
mod dialect;
pub mod emit;
mod from_node;
mod ir;
mod recognizer;
pub mod recover;
pub mod resolve;
pub mod rules;
mod scope;
mod shape;
mod toolset;

pub use adapter::{DeclarationForm, ToolDeclaration, ToolSource};
pub use definition::{
    Effects, ObjectSpec, Param, ParameterSpec, RepairPolicy, ToolDefinition, UnknownParameters,
};
#[doc(hidden)]
pub use derive_support::__derive_number;
pub use derive_support::{
    Members, Slot, ToolArguments, ToolExtra, ToolItem, ToolParameter, ToolPattern, ToolRepeatable,
    max_depth,
};
pub use detect::{DETECTION_BYTES, Detection, MarkupFamily, detect_unparsed};
pub use dialect::{DialectId, DialectSet, DialectSetError};
pub use emit::{
    CallEdit, Emission, EmitError, EmitTarget, Emitted, RenderedResult, SkipReason, Skipped,
    ToolResult,
};
pub use from_node::{FromNode, FromNodeError};
pub use ir::{
    ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, CallSegment, CaptureSpan, Choice, Closure,
    CollectedMember, LEAF_CHARGE, Piece, ProductionKind, RawCall, ReasoningClosure,
    ReasoningSegment, Rejection, Segment, Spanned, UnresolvedArgument,
};
pub use recognizer::{GenerationStop, ReasoningPolicy, RecognizedTurn, Recognizer};
pub use recover::{
    AmbiguityPolicy, DEFAULT_ID_PREFIX, Finish, IdGenerator, IdPolicy, KeywordUse,
    RECOVER_OPERATION, ReasoningBlock, RejectedCall, StrictForced, TOOL_CALL_VALIDATOR,
    TURN_TAG_DIGITS, ToolCall, ToolCalls, ToolChoice, ToolChoiceCheck, ToolChoiceError,
    ToolRecoveryRequest, Verdict, exit_code, openai_finish_reason, turn_id_prefix,
};
pub use resolve::{
    CallFacts, CallLedger, CallLoss, CallOutcome, CallReading, Policies, PolicySelection,
    ResolvedCall, ResolvedChoice, ResolvedPiece, ResolvedSegment, ResolvedTurn, UnresolvedReason,
    resolve_forest,
};
pub use rules::{RULES, Rule};
pub use scope::{Nesting, ScopeGraph, ScopeMember, ScopeNode};
pub use shape::{
    ArgumentShape, NameLiteral, ObjectShape, ParameterDefault, ParameterShape, PatternConstraint,
    ShapeStep, ValueShape,
};
pub use toolset::{CompiledTool, ToolSet, ToolSetError, collision_key, is_valid_name};

/// The entry of `items`, sorted by the name `key` gives, whose name is
/// exactly `name`: one binary search (tools of a set, members of a scope).
fn by_name<'a, T>(items: &'a [T], name: &str, key: impl Fn(&'a T) -> &'a str) -> Option<&'a T> {
    items
        .binary_search_by(|item| key(item).cmp(name))
        .ok()
        .map(|index| &items[index])
}
