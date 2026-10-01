<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Correction-evaluation corpora

Five first-party, deterministic corpora consumed by
`crates/core/tests/correction_evaluation.rs` to measure the incorrect
automatic-correction rate of `purrassist_core::matching`, `purrassist_core::paths`,
`purrassist_core::schema::recovery`, and tool-call recovery
(`purrassist_core::tool_call::ToolSet::recover`). Registered as a `test_data`
entry in `vendor/dependencies.json`.

## Provenance

Every JSON file here is produced by `generate.py` in this directory, run with
`python3 crates/core/tests/data/evaluation/generate.py`. The generator uses
no random-number generation, so re-running it against an unchanged
`crates/core/tests/data/unicode/confusables.txt` reproduces byte-identical
output; `manifest.json` records each output file's case count and SHA-256
digest, plus the generator's own digest, so a stale regeneration is
detectable. The only external input is the project's own pinned UTS #39
confusables fixture, read to source *authentic* Cyrillic/Latin confusable
character pairs (never a hand-invented lookalike table); every word list,
phrase template, homophone pair, and JSON schema/value pair is curated by
hand for this generator, under this project's own license (MIT OR
Apache-2.0 OR MulanPSL-2.0), matching every other source file in the
repository. No third-party natural-language corpus is copied. The confusable
character
mappings derive from the pinned Unicode fixture and retain its registered
Unicode license and provenance.

## Files

- `strings.json` -- string-selection cases for `matching::resolve`. Each case
  has `query`, `candidates` (`id`/`text`/`aliases`), a `category` (one of the
  matching lattice levels, one of the six protected classes, alias behavior,
  or an unrelated-string baseline), a `minimum_score` selection-policy
  threshold, and `truth`: the candidate `id` a correct automatic resolution
  must select, or `null` meaning the case is expected to abstain (any
  automatic `Resolved`/`BestGuess` on such a case counts as an incorrect
  correction, regardless of which candidate it names).
- `paths.json` -- path-matching cases for `paths::PathIndex::resolve`. Each
  case has `paths` (the indexed candidate set), `query`, `context`, a
  `category`, and `truth`: the expected resolved path, or `null` for an
  expected abstention (including malformed/traversal queries that must never
  resolve to any indexed path).
- `phonetic.json` -- English near-homophone cases for `matching::resolve` at
  a lowered `minimum_score` (6000, matching the Soundex signal's score).
  Homophone pairs are a hand-curated list of real English near-homophones
  (their/there, to/too, weather/whether, ...); each pair is tried in both
  directions against several rotating unrelated-word distractor pairs.
  For the 1,280 directional examples,
  `truth` is the paired homophone's candidate `id`
  because the pairing itself is the ground truth, independent of whether the
  Soundex signal actually fires for a given pair (a pair whose codes differ
  legitimately abstains, which only affects coverage, not correctness). A
  further 128 cases offer two equally spelled candidate identities and have
  `truth: null`; they must abstain rather than choose an identity arbitrarily.
- `json_recovery.json` -- JSON Schema recovery cases for
  `Schema::recover`. Each case has a first-party JSON Schema, a corrupted
  `input` string, and `truth`: the exact original JSON value (compared with
  `Node::equivalent`), or `null` when the corruption is a genuine, symmetric
  ambiguity (a duplicated key with two distinct, still schema-valid values)
  that must never be silently resolved one way. Six corruption families are
  used, matching the vocabulary already established by
  `crates/core/tests/corruption_oracle.rs` for the (distinct, Task 8-owned)
  known-origin corruption oracle: truncation, prose/fence wrapping, a
  realistic adjacent-letter key typo, scalar stringification, a
  string/number type swap, and an ambiguity-preserving duplicate key. Fourteen
  variants of eight first-party templates produce 112 distinct
  schema/value pairs by consistently varying property names, enum choices,
  quantities, strings, booleans and arrays. Prose padding and array-wrapper
  cases exercise composed transformations. There are 714 examples and 686
  distinct requests: scalar stringification and integer type swapping overlap.
  Whitespace repetitions do not inflate this corpus.
- `tool-calls.jsonl` -- tool-call recovery cases for `ToolSet::recover`, one
  JSON record per line: `id`, `category` (the repair rule the mutation
  inverts, or `canonical` for an unmutated witness), `dialect` (the
  request's comma-separated dialect allow-list), `policy` (`per_tool`,
  `structural` or `tolerant`), `stop` (`stop`, `tool_calls`, `length`, or
  `{"sequence": ...}`), `input` (the model turn), and `truth`: the calls a
  correct answer recovers (`name` and `arguments`, in order) or
  `"Unresolved"` when the only correct answer is abstention. Every request is
  distinct. See `docs/MATCHING-QUALITY.md` ("Tool-call recovery evaluation")
  for the mutation families and the labelling rule.

## Tool-call corpus provenance

`tool-calls.jsonl` is produced by the `tool-call-corpus` example
(`crates/core/examples/tool_call_corpus.rs`, sharing its generator with the
evaluation test through `crates/core/tests/tool_call_corpus/mod.rs`). It is
derived only from first-party material: the generator's own tool
definitions and mutation tables, and well-formed calls printed by
`purrassist_core::grammar::witness` from this project's built-in dialect
grammars (`crates/core/src/tool_call/dialects/`). Witness seeds are fixed and
the remaining choices use a first-party xorshift generator seeded from each
case's coordinates, so the output is byte-for-byte reproducible; no model
output, third-party transcript or external dataset is copied, and the file
carries this project's license. `manifest.json` registers it under
`generators`: its SHA-256 digest, case count, distinct-request count, the
command that writes it, and the digests of both generator sources.
`scripts/check-repository.py` verifies every one of them, and
`correction_evaluation.rs::corpus_regenerates_identically` regenerates the
corpus in memory (which also exercises the library the generator calls) and
requires it to equal the committed file.

## Regenerating

```sh
python3 crates/core/tests/data/evaluation/generate.py
cargo run --locked -p purrassist-core --example tool-call-corpus
```

The Python generator overwrites the four `*.json` files and `manifest.json`
in place, keeping the manifest's `generators` entries. The example
overwrites `tool-calls.jsonl` and its `generators` entry, keeping every other
manifest entry. Review the diff before committing a regeneration; `correction_evaluation.rs` pins
the exact case counts and outcome tallies documented in
`docs/MATCHING-QUALITY.md`, so a corpus change requires updating both.

## Interpretation limits

These are deterministic regression corpora, with shared templates and repeated
word families. Case counts are not independent random samples of production
traffic. `manifest.json` records distinct request counts after removing case
identity/category/truth metadata; the Rust tests require at least 500 per corpus.
The exact Clopper–Pearson calculation in the quality report is a binomial-model
summary, whose confidence interpretation depends on independent trials and a
common error probability. It is not an empirically established deployment
error-rate guarantee. Score-threshold sweeps expose abstention alongside errors.

The tool-call corpus applies one mutation family per case to synthetic
witness calls; it measures whether each repair rule recovers exactly the
original call or abstains, not how often real models produce each
malformation, and its cases share tools, witness value vocabularies and
mutation templates.
