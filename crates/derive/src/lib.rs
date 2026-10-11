// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Zero-dependency derives for typed `PurRAssist` tool definitions.
//!
//! `#[derive(ToolDefinition)]` on a struct and `#[derive(ToolEnum)]` on a
//! unit enum generate the same typed builder calls as a hand-built tool
//! definition, plus an exact, single-pass conversion from a validated
//! argument object back into the struct. The crate uses only the built-in
//! `proc_macro` crate: it has a first-party token parser and code writer and
//! no dependencies (no `syn`, no `quote`).
//!
//! The impls are written against `purrassist-core`, which must therefore be a
//! dependency of the crate that derives. Users of the `purrassist` facade
//! crate derive through `purrassist::ToolDefinition` and
//! `purrassist::ToolEnum`, which need no other dependency.
//!
//! ```
//! use purrassist_core::tool_call::{
//!     FromNode, Param, ToolArguments, ToolDefinition as Definition, ToolRecoveryRequest, ToolSet,
//! };
//! use purrassist_core::json::{Node, Value};
//! use purrassist_core::{Budget, Outcome};
//! use purrassist_derive::{ToolDefinition, ToolEnum};
//!
//! #[derive(ToolEnum, Debug, PartialEq)]
//! enum Order {
//!     Asc,
//!     Desc,
//! }
//!
//! /// Search the index.
//! #[derive(ToolDefinition, Debug, PartialEq)]
//! #[tool(effects = read_only)]
//! struct Search {
//!     /// What to look for.
//!     query: String,
//!     #[param(default = 10)]
//!     limit: u32,
//!     order: Option<Order>,
//! }
//!
//! // The derived definition is the hand-built one.
//! let by_hand = Definition::new("search")
//!     .description("Search the index.")
//!     .effects(purrassist_core::tool_call::Effects::ReadOnly)
//!     .param("query", Param::string().required().description("What to look for."))
//!     .param("limit", Param::integer().default(Node::new(Value::Number("10".parse().unwrap()))))
//!     .param("order", Param::enumeration(&["Asc", "Desc"]));
//! assert_eq!(Search::tool_definition(), by_hand);
//!
//! // Recover a model turn against it, and read the arguments as the struct.
//! let tools = ToolSet::compile([Search::tool_definition()], Budget::default()).unwrap();
//! let turn = "<tool_call>\n{\"name\": \"search\", \"arguments\": {\"query\": \"cats\"}}\n</tool_call>";
//! let result = tools.recover(turn.as_bytes(), &ToolRecoveryRequest::default());
//! let Outcome::Resolved(calls) = &result.outcome else { panic!("{:?}", result.outcome) };
//! assert_eq!(
//!     Search::from_node(&calls.calls[0].arguments),
//!     Ok(Search { query: "cats".into(), limit: 10, order: None }),
//! );
//! ```
//!
//! # Type map
//!
//! A field's type decides its schema through the `ToolParameter` trait of
//! `purrassist-core` (the derive never inspects a field type's spelling, so
//! aliases and full paths behave the same):
//!
//! | Rust type | Parameter |
//! |---|---|
//! | `String` | string |
//! | `i8`..`i128`, `isize`, `u8`..`u128`, `usize` | integer |
//! | `f64` | number |
//! | `json::Number` | number, read back with its exact spelling |
//! | `bool` | boolean |
//! | `Vec<T>` | array of `T` |
//! | `Option<T>` | `T`, not required |
//! | a struct deriving `ToolDefinition` | nested object |
//! | an enum deriving `ToolEnum` | enumeration of its values |
//!
//! Every other parameter is required unless it declares a `default`. Other
//! field types fail to compile with rustc's "trait bound not satisfied" error
//! at the field's type, as do `Vec<Option<T>>` and `Option<Option<T>>` (JSON
//! has one `null`).
//!
//! # Attributes
//!
//! `#[tool(..)]` goes on the struct; every key is optional.
//!
//! | Key | Meaning |
//! |---|---|
//! | `name = ".."` | the tool name (default: the type name in `snake_case`) |
//! | `alias = ".."` | another accepted name; repeatable |
//! | `description = ".."` | the description (default: the doc comment) |
//! | `effects = mutating \| read_only` | what a call can do (default: `mutating`) |
//! | `policy = structural \| tolerant` | how far recovery may repair a call |
//! | `unknown = error \| drop \| pass` | handling of undeclared arguments |
//!
//! `#[param(..)]` goes on a field; every key is optional.
//!
//! | Key | Meaning |
//! |---|---|
//! | `name = ".."` | the parameter name (default: the field name without `r#`) |
//! | `alias = ".."` | another accepted name; repeatable |
//! | `description = ".."` | the description (default: the doc comment) |
//! | `repeat` | the parameter may repeat in markup; only on `Vec<T>` and `Option<Vec<T>>` |
//! | `default = <json>` | the value used when the argument is absent |
//! | `pattern = ".."` | a regular expression each string must match; only on `String`, `Option<String>`, `Vec<String>`, `Option<Vec<String>>` |
//! | `extra` | receives the undeclared arguments (see `unknown = pass`) |
//!
//! `#[param(name = "..")]` also goes on an enum variant, naming the value
//! that variant stands for. A nested struct needs no attributes of its own:
//! its tool-level keys `name`, `alias`, `effects` and `policy` are accepted
//! and ignored when it is used as a field, `unknown` applies to the nested
//! object, and its description becomes the field's description when the field
//! has none.
//!
//! Unknown keys, repeated keys and keys in the wrong place are compile
//! errors, spanned at the key. Attributes of other macros (`#[serde]`,
//! `#[allow]`, ...) are ignored.
//!
//! # Names and descriptions
//!
//! A tool is named after its struct in `snake_case`: `ReadFile` is
//! `read_file`, `HTTPRequest` is `http_request`, `ReadFile2` is `read_file2`
//! and `Read2File` is `read2_file`. A name that is not a valid tool name
//! (ASCII letters, digits and `_ . : -`) asks for `#[tool(name = "..")]`. A
//! parameter is named after its field with any `r#` removed, and an enum value
//! after its variant, verbatim. An explicit `name` always wins. Every tool name,
//! parameter name and alias, derived or explicit, is checked at compile time
//! against the same rule as the core crate (a `const` assertion, reported at the
//! `#[derive]`); a field whose name is not valid needs `#[param(name = "..")]`.
//!
//! The doc comment of a struct, field or enum is the description when no
//! `description` key is given (an explicit key wins): each line is trimmed and
//! the lines are joined with `\n`. Only `///` and `#[doc = "literal"]` lines
//! count. A variant's doc comment is accepted and has no effect, because an
//! enumeration carries only its values.
//!
//! # Defaults
//!
//! `default = <json>` takes JSON written as Rust tokens: `10`, `-1.5`,
//! `true`, `null`, `"text"`, `[1, 2]`, `{ "key": value }`. A number must be a
//! JSON number as spelled (no suffix, `_`, radix prefix, leading zero or
//! trailing `.`); trailing commas are allowed in arrays and objects; a string
//! literal is used as written. Anything else is a compile error at the token.
//! A parameter with a default is not required, and the generated conversion
//! fills the default when the argument is absent. A default of the wrong type
//! is reported when the tool set is compiled, and again by the conversion as
//! a type error at the parameter's pointer.
//!
//! On an `Option<T>` field a default also fills: an absent argument reads as
//! `Some(default)`, and `None` arises only from an explicit `null` in a
//! hand-made node.
//!
//! # The conversion (`FromNode`)
//!
//! The generated conversion reads an object in one pass over its members
//! (O(n log k) for n members and k declared parameters). It accepts canonical
//! parameter names only, as recovered arguments are canonical, and reports,
//! each with an exact pointer:
//!
//! * a missing required parameter (`expected: "required member"`);
//! * a repeated parameter (`"a single occurrence"`), which is never
//!   mistaken for a missing one;
//! * an undeclared parameter (`"a declared member"`), unless `unknown = pass`,
//!   which collects them into the `#[param(extra)]` field of type
//!   `json::Node`, an object of the undeclared members. `pass` needs exactly
//!   one `extra` field and `extra` needs `pass`;
//! * a value of the wrong type, with the pointer of the nested value
//!   (`/items/0/name`).
//!
//! # Integer ranges
//!
//! Every integer type declares the same `integer` schema, which has no range:
//! recovery and schema validation accept `300` for a `u8`, and the conversion
//! then fails with `integer within the u8 range`. Prefer fixed-width integers
//! in portable definitions: `usize` and `isize` have different ranges on
//! `wasm32` than on 64-bit targets.
//!
//! # Recursion
//!
//! A tool's arguments cannot contain themselves. A field naming its own struct
//! (including inside `Vec` or `Option`) is a compile error at that token.
//! Recursion through other types (`A` holds `Vec<B>`, `B` holds `Option<A>`)
//! is a compile-time cycle error (E0391) in an associated constant; because
//! the generated code is built from text, that error is reported at the
//! `#[derive]` rather than at a field. A recursive schema must be declared by
//! hand.
//!
//! # Limits
//!
//! Generic types, tuple structs, unions and data-carrying enum variants are
//! rejected. A unit struct and `struct Name {}` are valid tools with no
//! parameters. `repeat` and `pattern` can be combined; `extra` cannot be
//! combined with any other `#[param]` key.
//!
//! # Rejected input
//!
//! The pure parsing layer produces each message below (unit-tested verbatim);
//! `compile_fail` doctests in this crate pair every rejection with a passing
//! twin that differs only in the rejected construct.
//!
//! | Input | Message begins |
//! |---|---|
//! | generics, `where` | `generic parameters are not supported` |
//! | tuple struct | `tuple structs are not supported` |
//! | union | `unions cannot describe tool arguments` |
//! | enum to `ToolDefinition`, struct to `ToolEnum` | `applies to structs`, `applies to enums` |
//! | non-unit variant | `variant .. carries data` |
//! | unknown or repeated key | `unknown key ..`, `duplicate key ..` |
//! | malformed `default` | `.. is not a JSON number`, `must be a JSON value` |
//! | `pass` and `extra` misuse | `needs exactly one field marked`, `needs #[tool(unknown = pass)]` |
//! | direct self-reference | `refers to .. itself` |

// `unreachable_pub` and `redundant_pub_crate` disagree on the right visibility
// for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); the modules of a
// proc-macro crate export nothing, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in the core crate.
#![allow(clippy::redundant_pub_crate)]

mod bridge;
mod model;
mod parse;
mod write;

use parse::Target;
use proc_macro::TokenStream;
use write::Flavor;

/// Derives `ToolArguments`, `ToolParameter`, `ToolItem` and `FromNode` for a
/// struct with named fields, against `purrassist-core`.
///
/// The struct becomes a tool definition (`Self::tool_definition()`), a nested
/// object when used as a field of another derived struct, and the type its
/// recovered arguments convert to (`Self::from_node(&arguments)`). A unit
/// struct or `struct Name {}` is a tool without parameters.
///
/// See the [crate documentation](crate) for the type map, the `#[tool(..)]`
/// and `#[param(..)]` attributes, defaults, naming, and the conversion.
///
/// ```
/// use purrassist_core::{json, tool_call::{FromNode, FromNodeError, ToolArguments}};
/// use purrassist_derive::ToolDefinition;
///
/// #[derive(ToolDefinition, Debug, PartialEq)]
/// struct ReadFile {
///     path: String,
///     limit: Option<u8>,
/// }
///
/// assert_eq!(ReadFile::tool_definition().name(), "read_file");
/// let node = json::parse(r#"{"path": "a.txt"}"#).unwrap();
/// assert_eq!(ReadFile::from_node(&node), Ok(ReadFile { path: "a.txt".into(), limit: None }));
/// ```
///
/// # Errors
///
/// Invalid input is a `compile_error!` spanned at the offending token:
/// generics, tuple structs, unions, enums, unknown, repeated or misplaced
/// `#[tool]`/`#[param]` keys, a malformed `default`, a field naming its own
/// struct, a derived tool name that is not a valid name, repeated parameter
/// names, and `unknown = pass` without exactly one `extra` field (or the
/// reverse). A field type that is not a tool parameter, `repeat` on a
/// non-`Vec`, `pattern` on a non-string, `Vec<Option<T>>` and a non-`Node`
/// `extra` field are rustc trait errors at the field's type.
///
/// The generated conversion returns a `FromNodeError` whose `pointer`,
/// `expected` and `found` locate the first problem: a missing, repeated or
/// undeclared member, or a value of the wrong type or outside the range of
/// its Rust integer type (the schema has no integer ranges).
///
/// # Defaults, ranges and recursion
///
/// A `default` fills an absent argument (`Option<T>` included: it then reads
/// as `Some(default)`). Integer types are range-checked only by the
/// conversion; `usize`/`isize` differ on `wasm32`. Direct self-reference is
/// rejected and indirect recursion is a cycle error reported at the
/// `#[derive]`.
#[proc_macro_derive(ToolDefinition, attributes(tool, param))]
pub fn derive_tool_definition(input: TokenStream) -> TokenStream {
    bridge::expand(Target::Definition, Flavor::Core, input)
}

/// Derives `ToolParameter`, `ToolItem` and `FromNode` for an enum of unit
/// variants, against `purrassist-core`.
///
/// The enum is an enumeration parameter whose values are its variant names,
/// verbatim, or the `#[param(name = "..")]` of a variant. The conversion
/// accepts exactly those strings. The enum's doc comment, or
/// `#[tool(description = "..")]`, is the description of the enumeration.
///
/// ```
/// use purrassist_core::{json, tool_call::{FromNode, Param, ToolParameter}};
/// use purrassist_derive::ToolEnum;
///
/// #[derive(ToolEnum, Debug, PartialEq)]
/// enum Mode {
///     Fast,
///     #[param(name = "careful")]
///     Careful,
/// }
///
/// assert_eq!(Mode::parameter(), Param::enumeration(&["Fast", "careful"]));
/// assert_eq!(Mode::from_node(&json::parse(r#""careful""#).unwrap()), Ok(Mode::Careful));
/// let error = Mode::from_node(&json::parse(r#""slow""#).unwrap()).unwrap_err();
/// assert_eq!(error.expected, "one of \"Fast\", \"careful\"");
/// assert_eq!(error.found, "string");
/// ```
///
/// # Errors
///
/// A `compile_error!` spanned at the offending token for a struct or union,
/// generics, an enum without variants, a variant that carries data, repeated
/// values, and keys other than `#[tool(description)]` on the enum or
/// `#[param(name)]` on a variant. The conversion returns a `FromNodeError`
/// (`expected: "one of \"A\", \"B\""`) for any value that is not one of the
/// declared strings, including non-strings and different case.
#[proc_macro_derive(ToolEnum, attributes(tool, param))]
pub fn derive_tool_enum(input: TokenStream) -> TokenStream {
    bridge::expand(Target::Enumeration, Flavor::Core, input)
}

/// `ToolDefinition` for users of the `purrassist` facade crate: the same
/// derive, generating paths through `::purrassist::__core`.
///
/// Not meant to be named directly; the facade re-exports it as
/// `purrassist::ToolDefinition`, so that
/// `#[derive(purrassist::ToolDefinition)]` works in a crate that depends only
/// on `purrassist`. See [`ToolDefinition`](macro@ToolDefinition) for
/// everything else, including errors.
#[proc_macro_derive(FacadeToolDefinition, attributes(tool, param))]
pub fn derive_facade_tool_definition(input: TokenStream) -> TokenStream {
    bridge::expand(Target::Definition, Flavor::Facade, input)
}

/// `ToolEnum` for users of the `purrassist` facade crate: the same derive,
/// generating paths through `::purrassist::__core`.
///
/// Not meant to be named directly; the facade re-exports it as
/// `purrassist::ToolEnum`. See [`ToolEnum`](macro@ToolEnum) for everything
/// else, including errors.
#[proc_macro_derive(FacadeToolEnum, attributes(tool, param))]
pub fn derive_facade_tool_enum(input: TokenStream) -> TokenStream {
    bridge::expand(Target::Enumeration, Flavor::Facade, input)
}

// Evidence for the rejections: every case is a `compile_fail` doctest paired
// with a passing twin that differs only in the rejected construct, so the
// failure cannot be an unrelated error such as a missing import. Cases whose
// failure is a rustc trait error or a const cycle name the expected error
// code (checked by nightly rustdoc). The text of each `compile_error!`
// message is asserted verbatim by the unit tests of the pure layer.
#[cfg(doctest)]
mod rejected_input {
    //! # 1. Rejected: a type parameter
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A<T> { t: T }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { t: u8 }
    //! ```
    //!
    //! # 2. Rejected: a lifetime parameter
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A<'a> { t: &'a str }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { t: String }
    //! ```
    //!
    //! # 3. Rejected: a const parameter
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A<const N: usize> { t: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { t: u8 }
    //! ```
    //!
    //! # 4. Rejected: a where clause
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A where u8: Copy { t: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { t: u8 }
    //! ```
    //!
    //! # 5. Rejected: a generic enum
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A<T> { X(T) }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X }
    //! ```
    //!
    //! # 6. Rejected: a tuple struct
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A(u8);
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: u8 }
    //! ```
    //!
    //! # 7. Rejected: a union
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! union A { a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: u8 }
    //! ```
    //!
    //! # 8. Rejected: an unknown `tool` key
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(colour = "red")]
    //! struct A { a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "red")]
    //! struct A { a: u8 }
    //! ```
    //!
    //! # 9. Rejected: an unknown `param` key
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(colour)] a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(name = "colour")] a: u8 }
    //! ```
    //!
    //! # 10. Rejected: a `param` key on a variant that only accepts `name`
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { #[param(repeat)] X }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { #[param(name = "x")] X }
    //! ```
    //!
    //! # 11. Rejected: a `tool` key other than `description` on an enum
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! #[tool(name = "a")]
    //! enum A { X }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! #[tool(description = "a")]
    //! enum A { X }
    //! ```
    //!
    //! # 12. Rejected: `param` on the struct
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[param(name = "x")]
    //! struct A { a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(name = "x")] a: u8 }
    //! ```
    //!
    //! # 13. Rejected: `tool` on a field
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[tool(name = "x")] a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "x")]
    //! struct A { a: u8 }
    //! ```
    //!
    //! # 14. Rejected: a key given twice
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "a", name = "b")]
    //! struct A {}
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "a", alias = "b")]
    //! struct A {}
    //! ```
    //!
    //! # 15. Rejected: a key without its value
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name)]
    //! struct A {}
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "a")]
    //! struct A {}
    //! ```
    //!
    //! # 16. Rejected: a name that is not a string literal
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = 4)]
    //! struct A {}
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "four")]
    //! struct A {}
    //! ```
    //!
    //! # 17. Rejected: an invalid `effects` value
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(effects = loud)]
    //! struct A {}
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(effects = read_only)]
    //! struct A {}
    //! ```
    //!
    //! # 18. Rejected: an enum given to `ToolDefinition`
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! enum A { X }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X }
    //! ```
    //!
    //! # 19. Rejected: a struct given to `ToolEnum`
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! struct A { a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: u8 }
    //! ```
    //!
    //! # 20. Rejected: a variant that carries data
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X(u8) }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X }
    //! ```
    //!
    //! # 21. Rejected: an enum without variants
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A {}
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X }
    //! ```
    //!
    //! # 22. Rejected: two variants with one value
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X, #[param(name = "X")] Y }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolEnum)]
    //! enum A { X, #[param(name = "Y")] Y }
    //! ```
    //!
    //! # 23. Rejected: a default that is not a JSON number
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(default = 1_000)] a: u32 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(default = 1000)] a: u32 }
    //! ```
    //!
    //! # 24. Rejected: a default that is not JSON
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(default = foo)] a: u32 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(default = null)] a: Option<u32> }
    //! ```
    //!
    //! # 25. Rejected: a default with a trailing operator
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(default = [1,,2])] a: Vec<u8> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(default = [1, 2])] a: Vec<u8> }
    //! ```
    //!
    //! # 26. Rejected: `unknown = pass` without an `extra` field
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { a: u8, #[param(extra)] rest: Node }
    //! ```
    //!
    //! # 27. Rejected: an `extra` field without `unknown = pass`
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: u8, #[param(extra)] rest: Node }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { a: u8, #[param(extra)] rest: Node }
    //! ```
    //!
    //! # 28. Rejected: two `extra` fields
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { #[param(extra)] x: Node, #[param(extra)] y: Node }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { #[param(extra)] x: Node }
    //! ```
    //!
    //! # 29. Rejected: `extra` combined with another key
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { #[param(extra, name = "n")] x: Node }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { #[param(extra)] x: Node }
    //! ```
    //!
    //! # 30. Rejected: an `extra` field that is not a `Node` (rustc trait error)
    //!
    //! ```compile_fail,E0277
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { #[param(extra)] x: String }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(unknown = pass)]
    //! struct A { #[param(extra)] x: Node }
    //! ```
    //!
    //! # 31. Rejected: a field of its own struct
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Vec<A> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Vec<u8> }
    //! ```
    //!
    //! # 32. Rejected: a field of `Self`
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Option<Self> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Option<u8> }
    //! ```
    //!
    //! # 33. Rejected: indirect recursion (a cycle in an associated constant)
    //!
    //! ```compile_fail,E0391
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { b: Vec<B> }
    //! #[derive(ToolDefinition)]
    //! struct B { a: Option<A> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { b: Vec<B> }
    //! #[derive(ToolDefinition)]
    //! struct B { a: String }
    //! ```
    //!
    //! # 34. Rejected: `Vec<Option<T>>` (rustc trait error)
    //!
    //! ```compile_fail,E0277
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Vec<Option<u8>> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Vec<u8> }
    //! ```
    //!
    //! # 35. Rejected: `Option<Option<T>>` (rustc trait error)
    //!
    //! ```compile_fail,E0277
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Option<Option<u8>> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Option<u8> }
    //! ```
    //!
    //! # 36. Rejected: `repeat` on a type that is not a vector (rustc trait error)
    //!
    //! ```compile_fail,E0277
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(repeat)] a: String }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(repeat)] a: Vec<String> }
    //! ```
    //!
    //! # 37. Rejected: `pattern` on a type that is not a string (rustc trait error)
    //!
    //! ```compile_fail,E0277
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(pattern = "^a")] a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(pattern = "^a")] a: String }
    //! ```
    //!
    //! # 38. Rejected: a field type that is not a tool parameter (rustc trait error)
    //!
    //! ```compile_fail,E0277
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: std::collections::HashMap<String, u8> }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: Vec<u8> }
    //! ```
    //!
    //! # 39. Rejected: a tool name that is not valid (ASCII letters, digits and `_ . : -`)
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct Größe {}
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(name = "groesse")]
    //! struct Größe {}
    //! ```
    //!
    //! # 40. Rejected: two parameters with one name
    //!
    //! ```compile_fail
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: u8, #[param(name = "a")] b: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::{ToolDefinition, ToolEnum};
    //! use purrassist_core::json::Node;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { a: u8, #[param(name = "b")] b: u8 }
    //! ```
    //!
    //!
    //! # 41. Rejected: an explicit name that is not valid (a compile-time assertion)
    //!
    //! ```compile_fail,E0080
    //! use purrassist_derive::ToolDefinition;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(name = "bad name")] a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::ToolDefinition;
    //!
    //! #[derive(ToolDefinition)]
    //! struct A { #[param(name = "good_name")] a: u8 }
    //! ```
    //!
    //! # 42. Rejected: an alias that is not valid
    //!
    //! ```compile_fail,E0080
    //! use purrassist_derive::ToolDefinition;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(alias = "")]
    //! struct A { a: u8 }
    //! ```
    //!
    //! The twin without it:
    //!
    //! ```
    //! use purrassist_derive::ToolDefinition;
    //!
    //! #[derive(ToolDefinition)]
    //! #[tool(alias = "a")]
    //! struct A { a: u8 }
    //! ```
}
