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
mod dialect;
mod from_node;
mod ir;
mod recognizer;
pub mod recover;
pub mod resolve;
pub mod rules;
mod shape;
mod toolset;

pub use adapter::{DeclarationForm, ToolDeclaration, ToolSource};
pub use definition::{
    Effects, ObjectSpec, Param, ParameterSpec, RepairPolicy, ToolDefinition, UnknownParameters,
};
pub use dialect::{DialectId, DialectSet, DialectSetError};
pub use from_node::{FromNode, FromNodeError};
pub use ir::{
    ArgLeaf, ArgOrigin, ArgTree, ArgumentFault, CallSegment, CaptureSpan, Choice, Closure,
    LEAF_CHARGE, Piece, ProductionKind, RawCall, ReasoningClosure, ReasoningSegment, Rejection,
    Segment, Spanned, UnresolvedArgument,
};
pub use recognizer::{GenerationStop, ReasoningPolicy, RecognizedTurn, Recognizer};
pub use recover::{
    AmbiguityPolicy, Finish, IdGenerator, IdPolicy, KeywordUse, RECOVER_OPERATION, RejectedCall,
    TOOL_CALL_VALIDATOR, ToolCall, ToolCalls, ToolChoice, ToolChoiceCheck, ToolChoiceError,
    ToolRecoveryRequest,
};
pub use resolve::{
    CallFacts, CallLedger, CallLoss, CallOutcome, CallReading, Policies, PolicySelection,
    ResolvedCall, ResolvedChoice, ResolvedPiece, ResolvedSegment, ResolvedTurn, UnresolvedReason,
    resolve_forest,
};
pub use rules::{RULES, Rule};
pub use shape::{
    ArgumentShape, NameLiteral, ObjectShape, ParameterDefault, ParameterShape, PatternConstraint,
    ShapeStep, ValueShape,
};
pub use toolset::{CompiledTool, ToolSet, ToolSetError, collision_key, is_valid_name};
