// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Intent-aware assistance for imperfect AI output.
#![doc = include_str!("../README.md")]

/// Compiled schema validation and recovery entry point.
pub use purrassist_core::schema::Schema;
/// Effort, resource, and invention policy for JSON recovery.
pub use purrassist_core::schema::recovery::RecoveryRequest;
/// Pure first-party assistance engines and contracts.
pub use purrassist_core::*;

/// Isolated grammar parsing and validated code edits.
pub use purrassist_syntax as syntax;

/// The core crate under a name the derive macros can reach through
/// `::purrassist`. Generated code uses `::purrassist::__core` paths; it is not
/// public API and carries no compatibility promise.
#[doc(hidden)]
pub use purrassist_core as __core;

/// Derives `ToolArguments`, `ToolParameter`, `ToolItem` and `FromNode` for a
/// struct of named fields, so the struct is its own tool definition and its own
/// typed arguments. Unlike `purrassist_derive::ToolDefinition`, the generated
/// code reaches the engine through `::purrassist`, so a crate needs only the
/// `purrassist` dependency (no `crate =` attribute, no direct core
/// dependency).
///
/// The derive macro `purrassist::ToolDefinition` and the type
/// [`tool_call::ToolDefinition`](crate::tool_call::ToolDefinition) are
/// distinct items; the type is not re-exported at the crate root. The
/// attributes, naming, and errors are those of `purrassist_derive::ToolDefinition`.
///
/// ```
/// use purrassist::json;
/// use purrassist::tool_call::{FromNode, ToolArguments, ToolSet};
/// use purrassist::Budget;
///
/// /// Reads a file.
/// #[derive(purrassist::ToolDefinition, Debug, PartialEq)]
/// struct ReadFile {
///     path: String,
///     limit: Option<u32>,
/// }
///
/// let tools = ToolSet::compile([ReadFile::tool_definition()], Budget::default()).unwrap();
/// assert!(tools.tool("read_file").is_some());
/// let node = json::parse(r#"{"path":"a.txt","limit":3}"#).unwrap();
/// assert_eq!(
///     ReadFile::from_node(&node),
///     Ok(ReadFile { path: "a.txt".into(), limit: Some(3) })
/// );
/// ```
pub use purrassist_derive::FacadeToolDefinition as ToolDefinition;

/// Derives `ToolParameter`, `ToolItem` and `FromNode` for an enum of unit
/// variants (a string enumeration), generating paths through `::purrassist`.
/// See [`ToolDefinition`](macro@ToolDefinition) for the facade rationale and
/// `purrassist_derive::ToolEnum` for the attributes and errors.
///
/// ```
/// use purrassist::json;
/// use purrassist::tool_call::{FromNode, Param, ToolParameter};
///
/// #[derive(purrassist::ToolEnum, Debug, PartialEq)]
/// enum Mode {
///     Fast,
///     #[param(name = "careful")]
///     Careful,
/// }
///
/// assert_eq!(Mode::parameter(), Param::enumeration(&["Fast", "careful"]));
/// assert_eq!(Mode::from_node(&json::parse(r#""careful""#).unwrap()), Ok(Mode::Careful));
/// ```
pub use purrassist_derive::FacadeToolEnum as ToolEnum;
