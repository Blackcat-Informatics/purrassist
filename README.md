<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
<p align="center"><img src="docs/purrassist-logo.svg" width="220" alt="PurRAssist black cat holding a repaired structure"></p>

# PurRAssist

**Assistance for AIs that get the details almost right.**

PurRAssist recovers intent from imperfect JSON, names, paths, and text. Give a
decoder a schema and an effort level: it searches composed corrections, validates
the complete result, and returns evidence of what changed. A schema-valid guess
is still a guess. Ambiguous alternatives remain visible.

**Pre-release 0.1.0 implementation.** The accepted scope is in
[the implementation contract](docs/IMPLEMENTATION.md), with executable evidence
and limits in [the qualification record](docs/QUALIFICATION.md). Publication
acceptance follows review and merge; 0.1.0 is not claimed as published.

The [release procedure](docs/RELEASE.md) records the package and registry gates.

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
  their own with the grammar DSL. Still tracked in issue #4: the
  `pythonic.v1`, `gemma3-tool-code.v1` and `glm-4.5.v1` dialects, the
  `*-stripped` variants, streaming recognition, and `purrassist-derive`. See
  [tool-call recovery](docs/tool-calls/README.md).
- CLI, stdio MCP, and resumable helper request/response adapters. No model is
  required, loaded, or contacted by default.

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
compilation methods use the default finite budget. `validate_json` exposes
whole-instance validation itself: given `schema` and `instance`, it returns
`valid`, every `violations` entry, always-evaluated format/pattern evidence,
and any unsupported-vocabulary evidence, alongside a receipt. The shipped CLI's
`purrassist validate` command emits exactly this tool's output. `generate_json_witness`
exposes the same witness-generation algebra: given `schema` and optional
`resources`, it returns `outcome` (`witness`, `unsatisfiable`, or `exhausted`),
the generated `witness` value or non-empty `refutation` evidence, and a
receipt; a governor hard stop while generating is reported as `exhausted`
with `budget_exceeded` in the receipt's termination, never as `unsatisfiable`.

`recover-tool-calls` reads one model turn as raw bytes from a file or stdin
and recovers its tool calls against the declarations in `--tools`
(`--tools-format openai`, `mcp`, or `anthropic`). It prints the same result as
the `recover_tool_calls` MCP tool: per-call outcomes, itemized repairs and
losses, rejected attempts, the tool-choice check, the turn's finish and its
`OpenAI` `finish_reason`, an optional `--emit` re-emission (`openai` for the
`tool_calls` array, or a dialect id), and a receipt. It never executes a call.
The `finish_reason` is `"length"` whenever the caller's stop reason is a length stop (whatever the
finish: a length stop is never reported as a clean finish, and the usable
calls are still emitted), otherwise `"tool_calls"` exactly when the `OpenAI`
emission holds a call, and `"stop"` otherwise;
neither says whether anything else went wrong, so read `finish`, `verdict`
and `rejected` as well. The command exits 0 when at least one call is usable (`usable_accepting_guesses`) and the
tool-choice check passed, even if other attempts in the turn were rejected or
are unparsed call markup (the JSON itemizes them, so a script keyed on the
exit code never discards good calls because of one stray opener), and for
clean content (finish `content_only`). It exits 3 when nothing is usable and
the operation or a call is ambiguous, and 4 when nothing is usable (unresolved
calls, only rejected or unparsed call markup, a `truncated` turn, only guesses
that may not be used, such as a mutating tool's, or an `Unresolved` operation:
an exhausted budget, invalid input) or the tool-choice check failed, even with
usable calls: a violated tool choice is a contract failure the caller must
see. Invocation errors exit 1. The printed JSON is the same whatever the exit
code; the code only summarizes it. `--tool-choice NAME` forces a tool; with
`--tool-choice-exactly-one`, `--tool-choice-no-content` and
`--tool-choice-no-reasoning` (a usage error without `--tool-choice NAME`) the
turn must hold exactly one call, no content other than ASCII whitespace, and
no reasoning block, or the tool-choice check fails like any violated choice.
A rejected call attempt of any reason counts as a call beyond the one and as
content, and `--tool-choice-no-reasoning` with `--reasoning content` (where
reasoning is never recognized) is invalid input.
The [integration guide](docs/tool-calls/INTEGRATION.md)
lists every option, the integrator guarantees, and the evidence for each.

`prepare_helper`/`complete_helper` expose optional external-model helper
proposals (see [helpers](docs/HELPERS.md)) as a two-call tool pair: the first
returns the exact request to send and an opaque, tamper-evident `pending`
string with no network I/O performed here; the second resumes with the
caller-observed HTTP status/body once the caller's own transport has sent it.

`call TOOL --root DIRECTORY` reads a JSON argument object from stdin and invokes
the same operation used by MCP. `match_strings` accepts plain candidate strings
or objects with stable `id`, `text`, and caller-declared `aliases`. Optional
`effort`, `english_phonetics`, and `limit` control ranking. Its results retain
each identity, score, aliases, and comparison signals; scores are ranking units,
not proof of semantic equivalence. See [the example](examples/match-strings.json).
`resolve_string` applies caller-adjustable `minimum_score` and `minimum_margin`
thresholds to all candidates, even when ranking would normally return only its
first ten. A unique byte-exact spelling is `resolved`; a unique accepted
approximation is `best_guess`; close alternatives remain `ambiguous` with
separate digest-bound inference ledgers; weak matches and exhaustion are
`unresolved`. Normalization equivalence is an explicit lattice — NFC,
case-fold, width/compatibility-fold, diacritic-strip, UTS-39 confusable
skeleton, and depunctuation, each a named ranking signal — resolving a query
at the finest level where it has a unique acceptable candidate. Protected
token classes (decimal digit runs, negation markers, semver components, ISO
dates, hex digests, and units) must match for automatic selection unless a
caller-declared alias matches the query exactly; a discarded protected-class
mismatch appears in the receipt. Aliases count as guesses when selected.
Thresholds are policy on ranking units, not calibrated confidence.
The [curated quality set](docs/MATCHING-QUALITY.md) records correction and
abstention outcomes across the full lattice and every protected class.
`match_paths` takes a `query`, caller-supplied `paths`,
optional directory `context`, effort, and score/margin thresholds. It returns
`resolved`, `best_guess`, `ambiguous`, or `unresolved` with methods and a receipt.
Ambiguous paths carry separate evidence and inference ledgers bound to each
serialized alternative.
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
are rejected. A trailing blank body line and a leading tab are layout-recovered
(reindented to the container level, or to the number of spaces the tab would
occupy from column zero) rather than rejected. Identical logical bodies preserve
original prefix spelling and yield empty plans. See `examples/markdown-section.json`.
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
reparsing. Columns beyond the header width are not editable through this
operation, but editing an implicit padded cell of a body row shorter than the
header materializes every implicit cell up to it instead of refusing.
For an executable nested-table example:

```sh
purrassist call plan_markdown_table_cell --root . < examples/markdown-table-cell.json
```

These operations take exact `source` and optional `effort`, retain admitted source, and
return receipts. Edit plans in `value` bind to the source digest and do not write
files. Block indices come from inspection of the same source version.
Markdown editing operations retain the original source and support exact undo.

Pass one selected plan to `apply_edit` with the original `source` to obtain the
edited text and a source-bound inverse. `apply_file` also accepts a `plan` instead
of a full replacement buffer, with its expected digest and journaled undo checks.
See [plan application](docs/EDIT-APPLICATION.md) and `examples/apply-edit.json`.
`recover_files` reconciles prepared journals against actual file versions
without overwriting anything, reporting each transaction's exact outcome and
removing evidence-bearing orphan temporaries. `commit_files` independently
journals and commits several file replacements in one call — never as one
atomic multi-file transaction; each entry reports its own commit or error.
`plan_replacement`, `plan_diff`, and `inspect_code` each also accept an
`effort` argument selecting their search/traversal budget preset.
`inspect_code` reports each JSX/TSX rename issue's repair options and the
governed parse's allocator accounting (retained/peak/overshoot bytes).
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

## Develop and verify

Rust edition 2024, MSRV 1.98. Development uses nightly Rust with Clippy and
rustfmt. No commit-verification bypass is permitted.

```sh
make check       # formatting, attribution/dependencies, lint, optimized tests, docs
make wasm-test   # compare native/WASI recovery, matching, embeddings, Markdown, and grammars
make syntax-oracles  # compare fixed syntax probes to independent compilers
make benchmark  # recovery quality assertions and latency baseline
make fuzz       # nine coverage-guided targets and independent pattern/Markdown oracles
make resources  # allocations, heap/RSS, latency, artifact size, and uncached build cost
make package    # Cargo verification and registered source/license archive audit
```

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
choice. Third-party data and grammar dependencies retain their own notices.
PurRAssist shares Blackcat Informatics' cat silhouette and palette with PurRDF;
the braces and validation mark are this crate's service emblem.

Resource profiling is available with `make resources`; see [measurement scope and prerequisites](docs/RESOURCES.md). It uses external profilers and adds no crate runtime dependencies.

Coverage-guided JSON campaigns are available with `make fuzz TARGET=json`; see
[properties, replay, and qualification limits](docs/FUZZING.md).

Governed engines and host tools accept [caller-owned cancellation](docs/CANCELLATION.md),
with explicit transaction boundaries. The stdio MCP server handles cancellation
while tools run and admits at most four active tool calls.
