<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
<p align="center"><img src="docs/purrassist-logo.svg" width="220" alt="PurRAssist black cat holding a repaired structure"></p>

# PurRAssist

**Assistance for AIs that get the details almost right.**

PurRAssist recovers intent from imperfect JSON, names, paths, and text. Give a
decoder a schema and an effort level: it searches composed corrections, validates
the complete result, and returns evidence of what changed. A schema-valid guess
is still a guess. Ambiguous alternatives remain visible. Use it as a
[Rust crate, an MCP server, or a CLI](#one-engine-three-interfaces).

**Version 0.1.0.** Merged to `main`, not yet tagged `rust-v0.1.0`. The
`cargo add` and `cargo install` commands below work once 0.1.0 is published;
until then, build from [a checkout](#use-the-checkout). Read the
[changelog](CHANGELOG.md), the [implementation contract](docs/IMPLEMENTATION.md)
for the accepted scope, and the [qualification record](docs/QUALIFICATION.md)
for executable evidence. Its [known limitations](docs/QUALIFICATION.md#known-limitations)
are not release blockers. The [release procedure](docs/RELEASE.md) records the
package and registry gates.

<!-- three-interfaces:begin -->
## One engine, three interfaces

One recovery engine, three equal ways to use it: as a Rust crate, as an MCP
server, and as a command-line tool. They share the same engines and the same
evidence receipts.

### Rust crate

```sh
cargo add purrassist
```

<!-- three-interfaces-rust:begin -->
```rust
use purrassist::{Outcome, RecoveryRequest, Schema};

let schema = Schema::compile(r#"{
  "type": "object",
  "required": ["temperature"],
  "properties": {"temperature": {"type": "number"}},
  "additionalProperties": false
}"#)?;

let answer = schema.recover("[[{temperatur: '42',}]]", RecoveryRequest::default());
match answer.outcome {
    Outcome::Resolved(value) => assert_eq!(value.to_json(), r#"{"temperature":42}"#),
    Outcome::BestGuess(value) => println!("inferred: {}", value.to_json()),
    Outcome::Ambiguous(values) => println!("{} interpretations", values.len()),
    Outcome::Unresolved => println!("{}", answer.receipt.stopped),
}
# Ok::<(), purrassist::schema::SchemaError>(())
```
<!-- three-interfaces-rust:end -->

The [`purrassist` crate](crates/purrassist/README.md) is the Rust interface:
it re-exports the pure engines, the syntax integration and the derives.

### MCP server

```sh
cargo install purrassist-cli
purrassist mcp --root /path/to/workspace
```

Add the server to any MCP client that launches stdio servers:

```json
{
  "mcpServers": {
    "purrassist": {
      "command": "purrassist",
      "args": ["mcp", "--root", "/path/to/workspace"]
    }
  }
}
```

The server speaks newline-delimited JSON-RPC over stdio. Every tool is listed in
the [tool reference](docs/TOOLS.md). Starting the server creates a `.purrassist/`
directory in the root, where the file tools keep their transaction journals; the
`call` command below does the same.

### CLI

```sh
purrassist decode --schema schema.json --input response.txt
purrassist call match_strings --root DIR < args.json
```

`purrassist call TOOL --root DIR` reads a JSON argument object from stdin and
invokes the same operation used by MCP: for every registered tool the CLI and
MCP results are identical, which the test
`every_registry_entry_has_a_fixture_validated_through_cli_and_mcp` checks tool by
tool. The convenience commands `decode` and `match` are shortcuts with their own
output, not the tools' output. `purrassist validate` prints exactly the
`validate_json` tool's output.
<!-- three-interfaces:end -->

## Crate map

Six crates make up the release set, published in this order:

| Crate | Role |
|---|---|
| `purrassist-core` | Shared engine: the pure recovery, matching, schema, Markdown and tool-call engines, with zero third-party runtime dependencies. |
| `purrassist-derive` | Shared engine: zero-dependency `#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]` for typed tool definitions. |
| `purrassist-syntax` | Shared engine: isolated Tree-sitter grammar integration for source inspection and validated repairs. |
| `purrassist` | The Rust interface: the facade that re-exports the engines and the derives. |
| `purrassist-host` | Filesystem and helper integration behind both the CLI and the MCP server: root-scoped files, journaled replacement, the shared tool registry. |
| `purrassist-cli` | The CLI and stdio MCP server: the `purrassist` executable. |

Two workspace crates are internal and not published: `wasm-harness` (native,
WASI and browser-WASM acceptance transcripts) and `purrassist-helper-census`
(the duplicate-functionality census).

## Capabilities

The pure core owns its JSON parser, exact decimal arithmetic, schema validator,
Unicode normalization, edit distances, phonetic matching, semantic rules, recovery
search, and receipts. It has **zero third-party runtime dependencies**. The
isolated syntax crate uses the pinned Tree-sitter grammar family; see the
[dependency inventory](docs/DEPENDENCIES.md). Each tool is described in the
[tool reference](docs/TOOLS.md).

Current surfaces include:

- JSON recovery with nested unwrapping, syntax repair, approximate keys/enums,
  stringified values, whole-schema validation, and invented values (required
  properties, `patternProperties`/`additionalProperties`/`dependentSchemas`
  fills, `propertyNames`-conformant key repairs) produced by a first-party
  witness-generation algebra rather than a schema-blind heuristic.
- Unicode, lexical, Levenshtein, unrestricted Damerau-Levenshtein, English
  Soundex, aliases, and contextual path matching.
- Explicit semantic domain rules and caller-supplied embedding similarity.
- Source-bound text edits, bounded fuzzy replacement, relocatable unified diffs,
  overlap checks, exact-byte undo, and a Markdown
  editing index and structural section/list/table/reference operations. All
  1,324 pinned CommonMark/GFM examples have exact rendering checks, alongside
  generated preservation properties and independent differential campaigns.
- Python, Rust, JavaScript/JSX, TypeScript/TSX grammar inspection and validated
  punctuation repairs. Grammar acceptance does not prove program correctness.
- Journaled single-file replacement and conditional undo for coordinated writers.
- Tool-call recovery from raw model output: weighted, invertible dialect
  grammars with best-first markup repair, resolution against typed tools or
  lossless `OpenAI`/MCP/Anthropic declarations (Draft 2020-12 or Draft-07
  schemas), per-call outcomes with itemized repairs, conservative guards for
  mutating tools, and re-emission as any dialect or the `OpenAI` `tool_calls`
  array. Call markup of a dialect that is not built in yet, or not in the
  request's allow-list, is reported as a rejected attempt
  (`unsupported_dialect`), never silent content. Twelve dialects ship: `qwen3-coder-xml.v1`, `hermes.v1`,
  `openai-tool-calls.v1`, `llama3-json.v1`, `mistral-v3.v1`,
  `mistral-v11.v1`, `deepseek-v3.1.v1`, `deepseek-v3.v1`, `kimi-k2.v1`,
  `anthropic-tool-use.v1`, `harmony.v1`, and `bare-json.v1`; callers add
  their own with the grammar DSL. Typed tool definitions derive from Rust
  structs and enums with `#[derive(ToolDefinition)]` and `#[derive(ToolEnum)]`
  (the zero-dependency `purrassist-derive` crate, re-exported by the facade;
  see the example below). Still tracked: the `pythonic.v1` (#12), `glm-4.5.v1`
  (#13) and `gemma3-tool-code.v1` (#14) dialects, the `*-stripped` variants
  (#18), streaming recognition (#16), and provider streaming events (#17). See
  [tool-call recovery](docs/tool-calls/README.md).
- CLI, stdio MCP, and resumable helper request/response adapters
  (`prepare_helper` and `complete_helper`; see [helpers](docs/HELPERS.md)). No
  model is required, loaded, or contacted by default.

A tool's schema and its typed arguments can come from one Rust struct:

<!-- derive-example:begin -->
```rust
use purrassist::json;
use purrassist::tool_call::{FromNode, ToolArguments};

#[derive(purrassist::ToolEnum, Debug, PartialEq)]
enum Order {
    Asc,
    Desc,
}

/// Search the index.
#[derive(purrassist::ToolDefinition, Debug, PartialEq)]
#[tool(effects = read_only)]
struct Search {
    /// What to look for.
    query: String,
    #[param(default = 10)]
    limit: u32,
    order: Option<Order>,
}

assert_eq!(Search::tool_definition().name(), "search");
let arguments = json::parse(r#"{"query": "cats", "order": "Desc"}"#)?;
assert_eq!(
    Search::from_node(&arguments),
    Ok(Search { query: "cats".into(), limit: 10, order: Some(Order::Desc) }),
);
# Ok::<(), Box<dyn std::error::Error>>(())
```
<!-- derive-example:end -->

`Schema::generate_witness`/`generate_witness_governed` separately implement a
witness-generation algebra (canonicalize, eliminate `not` by De Morgan duality,
merge `allOf` constraints, and generate a type-directed value, recursing over
`$ref`/`$dynamicRef`): its result is a validated `Witness`, a closed-form
`Unsatisfiable` proof (never inferred from search exhaustion), or an honest
`Exhausted`, and recovery invents every guessed value through this same
algebra. `Schema::compile_governed` and `with_resources_governed` return typed
`CompilationError` values separating invalid schemas from resource failures.
Pass the same governor to `recover_governed` to share the total preparation and
recovery budget. The `decode_json` tool does this automatically. The convenience
compilation methods use the default finite budget.

`purrassist_core::patch::Patch` (an ordered, non-overlapping, source-digest-
bound set of byte-range replacements) is the crate's single edit
representation; `edits::EditPlan`/`TextEdit` are aliases for it. Beyond
`apply`, it exposes a governed `invert`, `compose`, `commute`, `rebase`, and
`diff` algebra; `apply_edit`/`apply_file` use `rebase`/`diff` to reconcile a
stale plan against an intervening change through an explicit `rebase_from`
input. See [plan application](docs/EDIT-APPLICATION.md). The Rust
`markdown::Document` API's structural operations — `insert_section`,
`remove_section`, `move_section` (with optional re-leveling of the moved
section's own heading), `rename_heading`, `ListItemEdit::Move`,
`renumber_list`, `TableRowEdit::Move`, `TableColumnEdit::Move`, and
`add_reference`/`relabel_reference`/`remove_reference`/`unlink_reference`
(which rewrite every shortcut/collapsed/full-reference use resolving to the
affected definition) — are exposed through the host tool
`plan_markdown_structure`, selected by a typed `op` (`section_insert`,
`section_remove`, `section_move`, `section_rename`, `list_move`,
`list_renumber`, `table_row_move`, `table_column_move`, `reference_add`,
`reference_relabel`, `reference_remove`, `reference_unlink`). Each is governed,
digest-bound, reparse-checked, and provides an exact inverse.

The Rust `markdown::InlineFragment` layer retains exact byte spans and renders
code spans, emphasis, strong emphasis, inline links/images, escapes, line breaks,
autolinks, and raw inline HTML. Explicit dialect selection preserves the pinned
CommonMark/GFM rendering differences. Named/numeric entities are decoded.
`markdown::References` collects definitions from supplied paragraph blocks;
`InlineFragment::parse_with_references` resolves full, collapsed, and shortcut
links/images while retaining definition provenance. `Document::render_html`
collects definitions across containers and renders blocks. GFM rendering enables
tables, task lists, strikethrough, extended autolinks, and tag filtering; explicit
extension profiles are also available. All 1,324 pinned official document examples
match exact HTML under their upstream profiles. Generated adversarial
preservation and independent differential checks extend this finite corpus;
the qualification record states their coverage and limits.

File tools reject stale versions and record transaction identities for undo. The portable filesystem layer coordinates
cooperating writers; it is not a sandbox against malicious directory replacement
or an atomic multi-file transaction.

Governed engines and host tools accept [caller-owned cancellation](docs/CANCELLATION.md),
with explicit transaction boundaries. The stdio MCP server handles cancellation
while tools run and admits at most four active tool calls.

## Use the checkout

```sh
cargo run -p purrassist-cli -- --help
cargo run -p purrassist-cli -- match raeder reader writer
cargo run -p purrassist-cli -- call match_strings --root . < examples/match-strings.json
cargo run -p purrassist-cli -- call resolve_string --root . < examples/resolve-string.json
cargo run -p purrassist-cli -- decode --schema examples/temperature.schema.json \
  --input examples/temperature.input.txt
cargo run -p purrassist-cli -- recover-tool-calls --tools tools.json --emit openai turn.txt
cargo run -p purrassist-cli -- mcp --root /path/to/workspace
```

`decode` returns the answer and receipt as JSON. `Exact` performs no recovery;
`Quick`, `Standard`, `Deep`, and `Maximum` increase finite search budgets.
Exhaustion is explicit and never proves that a schema is unsatisfiable.
`recover-tool-calls` prints the same result as the `recover_tool_calls` MCP tool
and never executes a call. Every example request under `examples/` is run
through both the CLI and MCP by the test suite. The [tool reference](docs/TOOLS.md)
describes each tool, its arguments, and its example; the
[integration guide](docs/tool-calls/INTEGRATION.md) lists every
`recover-tool-calls` option.

## Develop and verify

Rust edition 2024, MSRV 1.98. Development uses nightly Rust with Clippy and
rustfmt. No commit-verification bypass is permitted.

```sh
make check       # formatting, attribution/dependencies, lint, optimized tests, docs
make wasm-test   # compare native/WASI recovery, matching, embeddings, Markdown, and grammars
make syntax-oracles  # compare fixed syntax probes to independent compilers
make benchmark  # recovery quality assertions and latency baseline
make fuzz       # ten coverage-guided targets and independent pattern/Markdown oracles
make fuzz-hosted  # the CI and release fuzz gate (known #6/#11 limitations handled)
make resources  # allocations, heap/RSS, latency, artifact size, and uncached build cost
make package    # Cargo verification and registered source/license archive audit
```

Resource profiling is available with `make resources`; see [measurement scope and prerequisites](docs/RESOURCES.md). It uses external profilers and adds no crate runtime dependencies.

Coverage-guided campaigns for each of the ten targets are available with
`make fuzz TARGET=<target>` (for example `TARGET=json`); see
[properties, replay, and qualification limits](docs/FUZZING.md). The default
`make fuzz` currently stops on the issue #11 and #6 signatures, listed under
[known limitations](docs/QUALIFICATION.md#known-limitations); `make fuzz-hosted`
completes every target with those limitations retained and named.

## Conformance

JSON Schema validation implements two dialects, selected by `$schema`: Draft
2020-12 (the default) and Draft-07 (`http://json-schema.org/draft-07/schema#`,
with or without the `#`, as MCP servers commonly declare). Draft-07 support
covers tuple `items` with `additionalItems`, `definitions`, both forms of
`dependencies`, `$ref` overriding its siblings, `$id` base-URI changes and
plain-name `#anchor` identifiers, and annotation-only `format`; Draft-07
validations report validator `purrassist.schema.draft-07.v1`. Draft 2019-09,
draft-06, and earlier dialects are rejected at compilation. Witness generation,
schema-guided recovery, and the tool-declaration adapters follow whichever of
the two dialects applies.

The checked-in main Draft 2020-12 corpus contains 1,301 tests, with another 121
selected optional cases exercised, plus the complete vendored `optional/format/`
corpus (878 cases across every 2020-12 format, including RFC 5892/5893 IDNA2008
contextual and Bidi rules) run under a dialect declaring the format-assertion
vocabulary. All 929 required Draft-07 cases pass; of 911 Draft-07 `optional/`
cases (formats judged by their annotation results), 905 pass, and the 6 that
do not are listed by group with reasons: content assertion (annotation-only by
design) and a Draft 2019-09 cross-draft reference (not an implemented dialect).
Unicode 17
normalization contains 20,034 cases. These suites, regression tests, fixed helper
transcripts, and bounded mutation probes run locally. Full release qualification
requires the exact-commit gates and registry acceptance described in the
[qualification record](docs/QUALIFICATION.md).

## License and family

First-party code is available under **MIT OR Apache-2.0 OR MulanPSL-2.0**, at your
choice ([MIT](LICENSE-MIT), [Apache-2.0](LICENSE-APACHE), [MulanPSL-2.0](LICENSE-MULAN)).
GitHub's sidebar shows only one of the three licenses; the SPDX expression in
`Cargo.toml` is authoritative. Third-party data and grammar dependencies retain
their own notices.
PurRAssist shares Blackcat Informatics' cat silhouette and palette with PurRDF;
the braces and validation mark are this crate's service emblem.
