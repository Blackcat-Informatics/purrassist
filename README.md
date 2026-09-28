<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
<p align="center"><img src="docs/purrassist-logo.svg" width="220" alt="PurRAssist black cat holding a repaired structure"></p>

# PurRAssist

**Assistance for AIs that get the details almost right.**

PurRAssist recovers intent from imperfect JSON, names, paths, and text. Give a
decoder a schema and an effort level: it searches composed corrections, validates
the complete result, and returns evidence of what changed. A schema-valid guess
is still a guess. Ambiguous alternatives remain visible.

**Pre-release implementation in progress.** The accepted scope and remaining
qualification requirements are in [the implementation contract](docs/IMPLEMENTATION.md).
This repository has not been published or release-qualified.

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

The pure core owns its JSON parser, exact decimal arithmetic, schema validator,
Unicode normalization, edit distances, phonetic matching, semantic rules, recovery
search, and receipts. It has **zero third-party runtime dependencies**. The
isolated syntax crate uses the pinned Tree-sitter grammar family; see the
[dependency inventory](docs/DEPENDENCIES.md).

Current surfaces include:

- JSON recovery with nested unwrapping, syntax repair, approximate keys/enums,
  stringified values, inferred required fields, and whole-schema validation.
- Unicode, lexical, Levenshtein, unrestricted Damerau-Levenshtein, English
  Soundex, aliases, and contextual path matching.
- Explicit semantic domain rules and caller-supplied embedding similarity.
- Source-bound text edits, bounded fuzzy replacement, relocatable unified diffs,
  overlap checks, exact-byte undo, and a Markdown
  editing index. Full CommonMark/GFM conformance remains unfinished.
- Python, Rust, JavaScript/JSX, TypeScript/TSX grammar inspection and validated
  punctuation repairs. Grammar acceptance does not prove program correctness.
- Journaled single-file replacement and conditional undo for coordinated writers.
- CLI, stdio MCP, and resumable helper request/response adapters. No model is
  required, loaded, or contacted by default.

## Use the checkout

```sh
cargo run -p purrassist-cli -- --help
cargo run -p purrassist-cli -- match raeder reader writer
cargo run -p purrassist-cli -- decode --schema examples/temperature.schema.json \
  --input examples/temperature.input.txt
cargo run -p purrassist-cli -- mcp --root /path/to/workspace
```

`decode` returns the answer and receipt as JSON. `Exact` performs no recovery;
`Quick`, `Standard`, `Deep`, and `Maximum` increase finite search budgets.
Exhaustion is explicit and never proves that a schema is unsatisfiable.
`Schema::compile_governed` and `with_resources_governed` return typed
`CompilationError` values separating invalid schemas from resource failures.
Pass the same governor to `recover_governed` to share the total preparation and
recovery budget. The `decode_json` tool does this automatically. The convenience
compilation methods use the default finite budget.

`call TOOL --root DIRECTORY` reads a JSON argument object from stdin and invokes
the same operation used by MCP. `match_paths` takes a `query`, caller-supplied `paths`,
optional directory `context`, effort, and score/margin thresholds. It returns
`resolved`, `best_guess`, `ambiguous`, or `unresolved` with methods and a receipt.
It matches names only; choosing a path does not grant file access. `resolve_intent`
takes `input`, an explicit `domain` (identity, phrase rules, ignored words), and
`concepts` (identities, labels, attribute maps). It applies the declared rules,
retains the request, and returns concept interpretations with evidence. Unknown
words are errors; effort bounds work without changing domain meanings.
Ambiguous semantic answers carry separate ordered receipt ledgers with output
digests, supporting rules, derived values, and validation references. Duplicate
outputs retain the first supporting rule derivation; rejected branches do not
contribute evidence to accepted answers.

`inspect_markdown` returns nested blocks and root sections. `markdown_context`
takes UTF-8 byte offsets `start`/`end` and optional neighboring-block count.
`plan_markdown_section` replaces the body of a root `heading` block;
`plan_markdown_task` sets a root task `block` to the supplied `checked` state.
All four take exact `source` and optional `effort`, retain admitted source, and
return receipts. Edit plans in `value` bind to the source digest and do not write
files. Block indices come from inspection of the same source version. This
Markdown editing operations retain the original source and exact undo.
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
match exact HTML under their upstream profiles. Broader adversarial and release
qualification remains in progress.

File tools reject stale versions and record transaction identities for undo. The portable filesystem layer coordinates
cooperating writers; it is not a sandbox against malicious directory replacement
or an atomic multi-file transaction.

## Develop and verify

Rust edition 2024, MSRV 1.98. Development uses nightly Rust with Clippy and
rustfmt. No commit-verification bypass is permitted.

```sh
make check       # formatting, attribution/dependencies, lint, optimized tests, docs
make wasm-test   # execute and compare native/WASI core and grammar transcripts
make syntax-oracles  # compare fixed syntax probes to independent compilers
make benchmark  # recovery quality assertions and latency baseline
```

The checked-in main Draft 2020-12 corpus contains 1,301 tests, with another 121
selected optional cases exercised; Unicode 17
normalization contains 20,034 cases. These suites, regression tests, fixed helper
transcripts, and bounded mutation probes run locally. Full release qualification
also requires the remaining conformance, performance, packaging, CI, and platform
work described in the [qualification record](docs/QUALIFICATION.md).

## License and family

First-party code is available under **MIT OR Apache-2.0 OR MulanPSL-2.0**, at your
choice. Third-party data and grammar dependencies retain their own notices.
PurRAssist shares Blackcat Informatics' cat silhouette and palette with PurRDF;
the braces and validation mark are this crate's service emblem.
