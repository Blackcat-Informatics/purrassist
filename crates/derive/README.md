<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-derive

Zero-dependency derives for typed PurRAssist tool definitions:
`#[derive(ToolDefinition)]` on an argument struct and `#[derive(ToolEnum)]` on
a unit enum. The generated code is the same typed builder calls as a
hand-built `ToolDefinition`, plus an exact, single-pass `FromNode` conversion,
so the recovered arguments of a tool call can be read as the struct.

The crate is a procedural macro that uses only the built-in `proc_macro` crate:
a first-party token parser and code writer, with no `syn` and no `quote`. It
has no Cargo dependencies (`purrassist-core` is a dev-dependency used by its
tests and examples).

```rust
use purrassist_core::tool_call::{FromNode, ToolArguments};
use purrassist_core::json;
use purrassist_derive::{ToolDefinition, ToolEnum};

#[derive(ToolEnum, Debug, PartialEq)]
enum Order {
    Asc,
    Desc,
}

/// Search the index.
#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(effects = read_only)]
struct Search {
    /// What to look for.
    query: String,
    #[param(default = 10)]
    limit: u32,
    order: Option<Order>,
}

assert_eq!(Search::tool_definition().name(), "search");
let arguments = json::parse(r#"{"query": "cats", "order": "Desc"}"#).unwrap();
assert_eq!(
    Search::from_node(&arguments),
    Ok(Search { query: "cats".into(), limit: 10, order: Some(Order::Desc) }),
);
```

A derived nested struct states its own handling of undeclared members: its
`#[tool(unknown = ..)]` value, or drop-with-recorded-loss when it declares no
`unknown` key, whatever the enclosing tool does. Under a `pass` tool, an
undeclared member of a nested struct without `extra` is therefore dropped and
recorded (a `BestGuess` call), and a nested struct that should keep such members
declares `unknown = pass` with its own `extra` field. A nested struct that
inherited `error` and left a call unresolved now drops the member, so that call
becomes a `BestGuess`; declare `unknown = error` on it to stay strict.

The impls are written against `purrassist-core`, which must be a dependency of
the crate that derives. Users of the `purrassist` facade crate derive through
`purrassist::ToolDefinition` and `purrassist::ToolEnum` and need no other
dependency. The crate documentation lists the type map, the `#[tool(..)]` and
`#[param(..)]` attributes, defaults, naming, the conversion's error semantics,
integer-range and recursion limits, and every rejected input.

## Versions, the crate path and which calls to convert

- **Same version.** `purrassist-derive` and `purrassist-core` must have the
  same major and minor version (patch versions may differ). Generated code
  references a marker in core, `tool_call::__DERIVE_ABI_<major>_<minor>`, so a
  mismatched pair fails to compile with an error naming `__DERIVE_ABI_*`.
  Cargo accepts such a pair (the requirements are caret requirements); bring
  both to the same `major.minor`. The marker is `#[doc(hidden)]`, not API.
- **Crate path.** The generated code names `::purrassist_core` (this crate's
  derives) or `::purrassist::__core` (the derives re-exported by `purrassist`),
  so depend on `purrassist-core` (direct flavor) or `purrassist` (facade
  flavor) under that name. If the dependency is renamed, or core is reached
  through a re-export, add `#[tool(crate = path)]` to the struct or enum, for
  example `#[tool(crate = my_core)]`, `#[tool(crate = crate::reexports::pa)]`
  or `#[tool(crate = ::purrassist)]`. The path replaces the whole prefix and
  must be one at which the modules `tool_call` and `json` resolve as in
  `purrassist-core`. It is a plain `::`-separated path; an empty path, a
  trailing `::`, a literal, generics and a second `crate =` are compile
  errors.
- **Convert only usable calls.** `from_node` reads arguments exactly but does
  not decide whether a call may run. Call it only on the arguments of calls
  that recovery reports as usable or resolved:
  [`ToolCalls::usable`](https://docs.rs/purrassist-core/latest/purrassist_core/tool_call/recover/struct.ToolCalls.html#method.usable)
  (`CallOutcome::Resolved`), or
  [`ToolCalls::usable_accepting_guesses`](https://docs.rs/purrassist-core/latest/purrassist_core/tool_call/recover/struct.ToolCalls.html#method.usable_accepting_guesses)
  (which also lists best guesses of read-only tools). The arguments of an
  unresolved call are only the best tree reached, and a mutating tool's best
  guess never runs.

Licensed under MIT OR Apache-2.0 OR MulanPSL-2.0.
