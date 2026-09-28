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

`match_embeddings` takes a caller-declared `space`, a numeric `query` vector,
and `candidates` containing unique `id`/`vector` pairs. No model is loaded.
`minimum_score` defaults to `0.75`; `minimum_margin` defaults to `0.05`.
A unique qualifying cosine winner is a best guess, ties/near ties remain
ambiguous, and weak matches remain unresolved. Scores are not probabilities.
Vectors must be finite, nonzero, and dimensionally consistent. JSON decimals are
converted to binary64; overflow and nonzero underflow are rejected. Admitted
requests retain their structured JSON input in receipts, with separate ledgers
for ambiguous alternatives. Admission failures can return a tool error before
retention. `embeddings::EmbeddingIndex` provides reusable normalized vectors to
Rust callers. See `examples/match-embeddings.json` for an illustrative input.

`render_markdown` takes `source`, optional `dialect` (`commonmark` by default or
`gfm`), optional `extensions`, and `effort`. Omitting extensions uses the dialect's
default profile; an explicit array replaces it, including `[]` to disable all.
Names are `autolinks`, `strikethrough`, `tables`, `task_lists`, and `tag_filter`.
Results include HTML, source/output digests, renderer/specification identities,
effective extensions, and a receipt retaining the original source. Tag filtering
is not a general HTML or URL sanitizer. For example:

```sh
purrassist call render_markdown --root . < examples/render-markdown.json
```

`inspect_markdown` returns nested blocks, original line maps, root sections, nested `section_sites`, and
reference-definition occurrences with original field spans. `markdown_context`
takes UTF-8 byte offsets `start`/`end` and optional neighboring-block count.
`plan_markdown_section` selects either a root `heading` index or a nested `path`
from `section_sites`. Root bodies are inserted verbatim. Nested bodies omit outer
container markers; new lines receive quote prefixes and list-continuation spaces.
Reparsing preserves the heading, section boundary, ancestors, and outside blocks.
Supply line endings and separators explicitly. Bodies that capture following
sections, introduce peer headings, or change structure under container prefixes
are rejected; this includes trailing blanks that leave a list item and some
column-sensitive tab indentation. Identical logical bodies preserve original
prefix spelling and yield empty plans. See `examples/markdown-section.json`.
`plan_markdown_task` sets the supplied `checked` state using either a root
`block` index or a nested `path` from inspection (for example `[0,0,1]`).
Exactly one selector is required; paths follow successive child indices.
`plan_markdown_list_item` selects an item by `path` and accepts `before`, `after`,
`replace`, or `remove`. Insertions require an explicit `marker` (`-`, `+`, `*`,
`1.`, or `1)` through nine-digit ordinals) and logical `body`; replacement uses
`body` and retains the selected marker. Existing items are never renumbered.
New lines receive container prefixes and continuation indentation; required
separators follow the anchor's newline style and EOF convention. Removing the
only item can leave an empty container line. Every plan reparses the intended
item and checks outside siblings and ancestors. See `examples/markdown-list-item.json`.
`plan_markdown_table_cell` selects a table by `path`, with zero-based `row` and
`column` (row zero is the header). Its `markdown` replaces one explicitly present
cell; surrounding whitespace, pipes, container prefixes, and line endings remain
exact. Leading/trailing replacement whitespace is trimmed. Escape literal pipes
as `\|`; line breaks and proposals that change table structure are rejected after
reparsing. Implicit padding cells and columns beyond the header width are not
editable through this operation.
For an executable nested-table example:

```sh
purrassist call plan_markdown_table_cell --root . < examples/markdown-table-cell.json
```

These operations take exact `source` and optional `effort`, retain admitted source, and
return receipts. Edit plans in `value` bind to the source digest and do not write
files. Block indices come from inspection of the same source version.
Markdown editing operations retain the original source and support exact undo.
`plan_markdown_table_row` inserts or removes body rows. Supply a table `path`,
`action` (`insert` or `remove`), and zero-based body `index`; insertion also requires
`cells` with exactly the header width. Inserting at the row count appends.
New rows use canonical `| cell |` spacing, the delimiter line's container prefix
and newline style, and preserve EOF's final-newline convention. Existing bytes
outside the edit remain exact. Cell and row plans include structural validation
in their receipts.
`plan_markdown_table_column` takes a table `path`, zero-based column `index`, and
`action` (`insert`, `remove`, or `align`). Insertion requires `header`, `alignment`
(`default`, `left`, `right`, or `center`), and one `cells` value per body row;
alignment changes require only `alignment`. Removing the final column is rejected.
Insert/remove preserves existing cell spelling, makes implicit padding explicit,
keeps ignored extra cells beyond the new header width, and adds outer pipes when
absent. Alignment changes preserve delimiter margins. Every proposal is reparsed
and supports exact conditional undo. See `examples/markdown-table-column.json`.
`plan_markdown_reference` selects a definition by its inspection `path` and
`occurrence`, including shadowed duplicate labels. Supply decoded `destination`
and an explicit `title` (string or `null` to remove it). The plan encodes these
values safely for Markdown syntax, preserves label spelling and surrounding
source, and reparses every definition to check that unselected values stay exact.
Multiline destination/title regions can collapse to one physical line; newline
and tab values use entities. NUL values are rejected because Markdown cannot
retain them. Equal decoded fields produce an empty plan. See
`examples/markdown-reference.json`.
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
make wasm-test   # compare native/WASI recovery, embeddings, Markdown, and grammars
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
