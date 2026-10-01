<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# String matching quality

`purrassist-core::matching` resolves a caller's query string against a set of
identified candidates. This document describes the two structures that govern
correction quality — the normalization refinement lattice and the protected
token classes — and the curated evidence corpus in
`crates/core/tests/matching_quality.rs` that measures them.

## The refinement lattice

Equal spelling after normalization is not one signal; it is a chain of
increasingly permissive equivalence relations, finest (least collapsing)
first:

```
identity ⊑ NFC ⊑ case-fold ⊑ width/compatibility-fold ⊑ diacritic-strip
         ⊑ UTS-39 confusable skeleton ⊑ depunctuation ⊑ phonetics
```

Each level a query and a candidate's primary spelling agree at emits exactly
one named, versioned [`Signal`](../crates/core/src/matching.rs) with its own
score; only the finest level that matches is reported, so a coarser level's
evidence never shadows a finer one. Because `SelectionPolicy` accepts the
highest-scoring candidate (subject to the minimum score and margin), a query
resolves at the finest level where it has a unique acceptable candidate.

| Level | Signal | Score | What it means |
|---|---|---|---|
| Identity | `exact-v1` | 10000 | Byte-identical spelling. |
| NFC | `unicode-17-nfc-v1` | 9950 | Equal after canonical composition alone (e.g. a combining accent vs. its precomposed form), before any case or width folding. |
| Case-fold | `unicode-17-case-fold-v1` | 9925 | Equal after NFC and full Unicode case folding, before width/compatibility folding (e.g. `Straße` vs. `strasse`, whose case fold maps `ß` to `ss`). |
| Width/compatibility-fold | `unicode-17-caseless-v1` | 9900 | Equal after compatibility decomposition (NFKD), case folding, and recomposition (NFKC) — catches fullwidth/halfwidth and other compatibility variants that pure case-folding does not. |
| Diacritic-strip | `unicode-17-diacritic-insensitive-v1` | 9700 | Equal after the above plus removing all combining marks (`café` vs. `cafe`). |
| UTS-39 skeleton | `unicode-17-skeleton-v1` | 9650 | Equal after the above plus substituting each character for its confusable prototype from the pinned Unicode 17.0.0 UTS #39 `confusables.txt` (e.g. Cyrillic `а` vs. Latin `a`). |
| Depunctuation | `unicode-17-depunctuated-v1` | 9600 | Equal after removing punctuation and separator categories (`user_name` vs. `user-name`); checked independently of the chain above. |
| Edit distance / lexical | `levenshtein-unicode-v1`, `damerau-levenshtein-unicode-v1`, `lexical-jaccard-v1` | ≤ 9500 | Graduated similarity, always computed. |
| Phonetics | `soundex-en-v1` | 6000 | English Soundex code equality; always checked independently. |

`unicode-17-*` names are versioned independently of the Unicode data version
in the name (currently pinned to Unicode 17.0.0 data); a future revision of
any table gets a new version suffix rather than silently changing an existing
signal's behavior. All of `text::normalize`, `text::case_fold`,
`text::comparison_key`, `text::depunctuate`, and the new `text::skeleton` are
reused directly by this lattice; none of Unicode normalization, case folding,
or confusable-skeleton generation is reimplemented in `matching.rs`.

## Protected token classes

Independent of the lattice above, six **protected classes** bound every
normalizer: no level of the lattice is permitted to make a query resolve to a
candidate whose protected tokens differ from the query's, no matter how
similar the surrounding text is. A difference excludes the candidate outright
with explicit exclusion evidence, unless one of the candidate's
caller-declared aliases matches the query exactly (`comparison_key` equality),
in which case the guard is waived for that candidate entirely.

| Class | Evidence rule | Extracted from | Example |
|---|---|---|---|
| Decimal digit runs | `matching.numeric-identity-v1` | Maximal runs of Unicode `Nd` characters. | `invoice-1001` vs. `invoice-1002`. |
| Negation markers | `matching.protected-negation-v1` | Standalone `not`/`no` tokens, a curated list of `non-`/`un-`-prefixed negation words, and literal `!`. | `task unavailable` vs. `task available`. |
| Semver components | `matching.protected-semver-v1` | `major.minor.patch[-pre][+build]` substrings. | `release-v1.2.3` vs. `release-v1.2.4`. |
| ISO dates | `matching.protected-iso-date-v1` | `YYYY-MM-DD` substrings. | `log-2024-01-15-error` vs. `log-2024-01-16-error`. |
| Hex digests | `matching.protected-hex-digest-v1` | Runs of ≥ 7 hexadecimal characters containing at least one letter. | `build-deadbeef` vs. `build-deadbeee`. |
| Units | `matching.protected-unit-v1` | A decimal quantity immediately paired with a known unit abbreviation (`ms`, `s`, `kb`, `mb`, `gb`, …). | `timeout 5ms` vs. `timeout 5s`. |

The digit-run class keeps its original `matching.numeric-identity-v1` name
and behavior from before this generalization; callers pinned to that name are
unaffected. The other five classes are new. All six are checked uniformly by
`matching::rank`/`rank_governed`, `matching::resolve`/`resolve_governed`, and
the `purrassist-host` `match_strings`/`resolve_string` tools, which call the
same governed functions and therefore inherit the guard without separate
host-side logic. Charging for the extraction (`Resource::Work` and
`Resource::ScratchBytes`, released once the comparison completes) happens
before any of the extraction's Strings are allocated.

## The curated evidence set

`crates/core/tests/matching_quality.rs` exercises a curated corpus of 30
cases, each declaring the expected outcome (`Resolved`, `Guess`, `Ambiguous`,
or `Unresolved`) and, where an automatic selection is expected, the correct
answer. The corpus specifically covers:

- Every lattice level: byte-exact spelling, NFC-only equivalence, case
  folding, width/compatibility folding, diacritic stripping, UTS-39
  confusable skeletons (a Cyrillic/Latin lookalike), depunctuation, edit
  distance/transposition, and English phonetics.
- Every protected class: digit runs (three cases, including a caller-alias
  bypass), negation, semver components, ISO dates, hex digests, and units.
- Caller-declared alias behavior: a domain alias resolving a query no textual
  signal would otherwise match, competing aliases producing ambiguity, and
  `Effort::Exact` ignoring aliases entirely.
- Deliberately unresolved cases: unrelated strings, a single-character
  substitution, a short Unicode transposition below the acceptable threshold,
  and a generic phrase extension.

The test asserts two things that must remain true together, and a mismatch
fails the build:

- **Zero incorrect automatic resolutions.** Every case whose outcome is
  `Resolved` or `BestGuess` must select the case's declared correct answer;
  the test counts `correct_auto`, `incorrect_auto`, and `abstained` outcomes
  and asserts `incorrect_auto == 0`.
- **Exactly 30 cases.** `assert_eq!(cases.len(), 30)` keeps this document's
  case count in sync with the corpus; adding or removing a case requires
  updating both.

Running the test (`cargo test -p purrassist-core --test matching_quality --
--nocapture`) prints the current split, presently 13 correct automatic
selections and 17 abstentions (`Ambiguous` or `Unresolved`) out of the 30
cases, with zero incorrect automatic selections.

## Large-scale incorrect-correction evaluation

The curated 30-case corpus above is a hand-picked contrast set proving each
lattice level and protected class in isolation. `crates/core/tests/
correction_evaluation.rs` complements it with five much larger, first-party,
deterministic corpora (`crates/core/tests/data/evaluation/`, provenance in
that directory's `README.md`) — string selection, path matching, English
phonetic ambiguity, and JSON Schema recovery, generated by `generate.py` in
that directory, and tool-call recovery, generated by the `tool-call-corpus`
example (see "Tool-call recovery evaluation" below) — and measured
statistically rather than case-by-case.

For each corpus the test tallies **correct** automatic decisions (a
`Resolved`/`BestGuess` matching the case's declared truth), **incorrect**
automatic decisions (any automatic decision that does not — including any
automatic decision at all on a case whose truth is "should abstain"), and
**abstained** cases (`Ambiguous`/`Unresolved`). It asserts `incorrect == 0`
for every corpus, then computes the exact Clopper-Pearson 95% upper
confidence bound on the incorrect-correction rate (via the regularized
incomplete beta function, implemented in the test with no dependencies) over
the attempted decisions (correct + incorrect), and asserts that bound at or
under a stated limit. The table below is asserted exactly by the test
(`assert_eq!` on the case count, the correct count, and the abstained
count for each corpus) and printed by `cargo test -p purrassist-core --test
correction_evaluation -- --nocapture`; a mismatch between this table and the
test's assertions fails the build by design.

| Corpus | Cases | Correct | Incorrect | Abstained | Coverage | Risk | CP 95% upper bound | Limit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| String selection | 907 | 539 | 0 | 368 | 59.4% | 0.0% | 0.554% | 2% |
| Path matching | 1594 | 1279 | 0 | 315 | 80.2% | 0.0% | 0.234% | 2% |
| Phonetic ambiguity | 1408 | 1089 | 0 | 319 | 77.3% | 0.0% | 0.275% | 5% |
| JSON recovery | 714 | 630 | 0 | 84 | 88.2% | 0.0% | 0.474% | 2% |
| Tool-call recovery | 3234 | 2107 | 0 | 1127 | 65.2% | 0.0% | 0.142% | 1% |

"Coverage" is the fraction of cases an automatic decision was reached for at
all (correct + incorrect, over the total); "risk" is the observed error rate
among those attempted decisions (incorrect / attempted, always `0.0%` here
because `incorrect == 0` is asserted). The Clopper-Pearson bound is
substantially tighter than the stated limit in every corpus because each
corpus attempts several hundred automatic decisions with zero observed
errors; the limit itself is chosen with headroom (phonetic ambiguity's
Soundex-based signal is coarser than the rest of the lattice, so its limit is
wider) rather than tuned to the currently observed bound.

The tests read this document and assert each measured table row, including its
limit. These deterministic examples are correlated (common templates, repeated
homophone pairs and shared normalization rules), rather than independently
sampled Bernoulli trials. The CP numbers are the exact binomial-model calculation
for the displayed decision counts; their 95% interpretation requires independent
trials with a common error probability. They do not establish a 95% guarantee for
arbitrary deployment inputs. Zero observed errors and the stated limits are
regression criteria for these corpora. JSON uses 112 distinct schema/value pairs (714 examples, 686 distinct requests);
it does not repeat cases at different whitespace widths to inflate the count.
Every corpus has at least 500 distinct requests, asserted independently of case
identities. The phonetic corpus includes 128 deliberate identity ambiguities,
which must abstain.

Score-threshold sweeps measure risk against coverage at four operating points.
Every point has zero incorrect decisions. Increasing the threshold must never
increase the number of decisions. The zero-decision string point reports no
empirical precision or confidence bound; its abstention count remains measurable.
Paths retain exact identities even at the highest spelling threshold. JSON recovery
has one operating point above because its whole-schema validation has no ranking
threshold switch.

| Corpus | Minimum score | Correct | Incorrect | Abstained |
| --- | ---: | ---: | ---: | ---: |
| String selection | 7500 | 539 | 0 | 368 |
| String selection | 9000 | 483 | 0 | 424 |
| String selection | 9650 | 405 | 0 | 502 |
| String selection | 10000 | 0 | 0 | 907 |
| Path matching | 7500 | 1279 | 0 | 315 |
| Path matching | 9000 | 1258 | 0 | 336 |
| Path matching | 9650 | 1081 | 0 | 513 |
| Path matching | 10000 | 300 | 0 | 1294 |
| Phonetic ambiguity | 6000 | 1089 | 0 | 319 |
| Phonetic ambiguity | 7500 | 210 | 0 | 1198 |
| Phonetic ambiguity | 9000 | 0 | 0 | 1408 |
| Phonetic ambiguity | 10000 | 0 | 0 | 1408 |

An independent oracle campaign backs this evaluation:
`crates/core/tests/matching_oracle.rs` checks every ordered pair of strings
of length <= 4 on a 3-symbol alphabet (14,641 pairs) against an independent
breadth-first shortest-edit search, for both Levenshtein and unrestricted
Damerau-Levenshtein distance, and checks a from-scratch reimplementation of
the documented path-normalization invariants (portability admission and
case/width/diacritic-insensitive basename equivalence) against
`paths::PathIndex` on 231 admission cases and 112 generated basename/context
checks, plus curated regressions and exact-identity precedence. The independent
fold oracle covers ASCII, fullwidth Latin and the listed Latin diacritics; the
large corpora add confusables and wider Unicode examples without claiming this
small oracle exhaustively proves the Unicode tables.

## Tool-call recovery evaluation

`crates/core/tests/data/evaluation/tool-calls.jsonl` holds 3,234 mutated model
turns, each with its `id`, `category` (the rule expected to repair it),
`dialect` (the request's dialect allow-list), `policy`, `stop`, `input` and
`truth`. The `tool-call-corpus` example generates it deterministically: it
prints well-formed calls to eight first-party tools (four read-only, four
mutating; string, patterned, integer, boolean, enumeration, array and
nested-object parameters, tool and parameter aliases) as grammar witnesses
(`grammar::witness`, fixed seeds) in every built-in dialect, then applies
the inverse of one repair rule or edit operator per case: a dropped,
variant or stray closer, a `</parameter>` dropped before a declared
parameter, prose or a code fence inside or around the call, stringified
arguments, a cut-off output under a `length` stop, a misspelled tool or
parameter name, a case-changed or misspelled enumeration value, a
whitespace-padded typed value, a JSON syntax slip, reasoning markers, and
more. Turns of two calls (one dialect, and Qwen3-Coder XML mixed with
Hermes) cover the multi-call rules. `truth` is the original call or calls
when the mutation leaves what the caller needs, and `Unresolved` when the
only correct answer is abstention: the information is destroyed or
genuinely ambiguous (a hallucinated tool name, a missing required argument,
a cut-off call, a repeated argument, a value of the wrong JSON type, a call
written only inside reasoning, text between two closers of one value), or
the repair is a guess on a tool whose effective policy is `Structural` or
that is `Mutating`, or the repair loses data on a `Mutating` tool. The label
follows from the mutation, the tool definition and the case's policy alone,
never from running recovery.

Each case runs `ToolSet::recover` at `Effort::Standard` with the case's
allow-list, policy and stop. The answer is the turn's
`usable_accepting_guesses()` calls: `Resolved` calls, and `BestGuess` calls on
read-only tools. A `BestGuess` on a mutating tool, an `Ambiguous` or
`Unresolved` call or turn, and a turn with no usable call count as
abstentions. An answer is correct when its calls equal the truth (canonical
tool, `Node::equivalent` arguments, order) and incorrect otherwise, including
any answer to a truth-`Unresolved` case. The test pins the totals above,
asserts zero incorrect answers overall, per rule and per dialect, at least 20
cases per rule, and abstention recall of 100%:

Abstention recall: 1061 of 1061 truth-`Unresolved` cases abstained (100.0%).

The 66 other abstentions are coverage gaps, not errors: 48 calls followed by
a dangling `<tool_call>` opener under the all-dialect allow-list (Qwen3-Coder
XML and Hermes both read the opener, so the whole message has two readings
and is `Ambiguous`), and 18 read-only calls whose required arguments were
nested one level down (`DropWithLoss` drops the unknown wrapper before
`json.key.relocate` can apply, so the call is missing a required argument).
The corpus found one incorrect correction class while it was built: under
`Structural`, a misspelled parameter of a `DropWithLoss` tool was dropped
with a loss (a usable `BestGuess` on a read-only tool) instead of being
treated as the near miss it is. Resolution now leaves such a parameter
unassignable (`UnknownParameter`), so the call abstains.

| Rule | Cases | Correct | Incorrect | Abstained | Truth `Unresolved` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `canonical` | 224 | 224 | 0 | 0 | 0 |
| `json.enum.match` | 42 | 24 | 0 | 18 | 18 |
| `json.key.match` | 93 | 36 | 0 | 57 | 57 |
| `json.key.relocate` | 56 | 0 | 0 | 56 | 38 |
| `json.required.invent` | 84 | 0 | 0 | 84 | 84 |
| `json.syntax.bare-key` | 62 | 62 | 0 | 0 | 0 |
| `json.syntax.bare-string` | 34 | 12 | 0 | 22 | 22 |
| `json.syntax.close-container` | 24 | 24 | 0 | 0 | 0 |
| `json.syntax.comment` | 62 | 62 | 0 | 0 | 0 |
| `json.syntax.literal-case` | 24 | 24 | 0 | 0 | 0 |
| `json.syntax.missing-colon` | 62 | 62 | 0 | 0 | 0 |
| `json.syntax.missing-comma` | 52 | 52 | 0 | 0 | 0 |
| `json.syntax.missing-value` | 36 | 0 | 0 | 36 | 36 |
| `json.syntax.string` | 54 | 54 | 0 | 0 | 0 |
| `json.syntax.trailing-comma` | 62 | 62 | 0 | 0 | 0 |
| `json.syntax.unterminated-string` | 24 | 12 | 0 | 12 | 12 |
| `json.type.decode-string` | 24 | 0 | 0 | 24 | 24 |
| `json.type.stringify-scalar` | 32 | 0 | 0 | 32 | 32 |
| `tool_call.arguments.decode-string` | 48 | 48 | 0 | 0 | 0 |
| `tool_call.context.fenced-example` | 96 | 36 | 0 | 60 | 60 |
| `tool_call.extract.fence` | 32 | 32 | 0 | 0 | 0 |
| `tool_call.extract.prose` | 32 | 16 | 0 | 16 | 16 |
| `tool_call.id.dedup` | 32 | 32 | 0 | 0 | 0 |
| `tool_call.markup.closer-variant` | 127 | 127 | 0 | 0 | 0 |
| `tool_call.markup.closers-only-empty` | 29 | 1 | 0 | 28 | 28 |
| `tool_call.markup.dangling-opener` | 96 | 48 | 0 | 48 | 0 |
| `tool_call.markup.json-body` | 32 | 32 | 0 | 0 | 0 |
| `tool_call.markup.missing-closers-at-end` | 64 | 64 | 0 | 0 | 0 |
| `tool_call.markup.missing-function-close` | 32 | 32 | 0 | 0 | 0 |
| `tool_call.markup.missing-parameter-close` | 26 | 26 | 0 | 0 | 0 |
| `tool_call.markup.missing-wrapper-close` | 64 | 64 | 0 | 0 | 0 |
| `tool_call.markup.missing-wrapper-close.before-text` | 64 | 24 | 0 | 40 | 40 |
| `tool_call.markup.nested-parameter` | 24 | 24 | 0 | 0 | 0 |
| `tool_call.markup.parameter-closed-by-element` | 31 | 31 | 0 | 0 | 0 |
| `tool_call.markup.stray-closer` | 251 | 223 | 0 | 28 | 28 |
| `tool_call.markup.undeclared-parameter-boundary` | 26 | 0 | 0 | 26 | 26 |
| `tool_call.name.alias` | 24 | 24 | 0 | 0 | 0 |
| `tool_call.name.match` | 192 | 36 | 0 | 156 | 156 |
| `tool_call.parameter.alias` | 24 | 24 | 0 | 0 | 0 |
| `tool_call.parameter.collect` | 24 | 24 | 0 | 0 | 0 |
| `tool_call.parameter.drop-unknown` | 96 | 48 | 0 | 48 | 48 |
| `tool_call.parameter.repeated` | 84 | 0 | 0 | 84 | 84 |
| `tool_call.reasoning.embedded-call` | 96 | 0 | 0 | 96 | 96 |
| `tool_call.reasoning.unclosed` | 96 | 36 | 0 | 60 | 60 |
| `tool_call.reasoning.unterminated` | 96 | 96 | 0 | 0 | 0 |
| `tool_call.stop.consumed-closer` | 64 | 64 | 0 | 0 | 0 |
| `tool_call.truncated` | 192 | 96 | 0 | 96 | 96 |
| `tool_call.value.enum-normalize` | 42 | 42 | 0 | 0 | 0 |
| `tool_call.value.trim-nonverbatim` | 25 | 25 | 0 | 0 | 0 |
| `tool_call.xml.array-item` | 22 | 22 | 0 | 0 | 0 |

By printed dialect (`mixed` turns hold a Qwen3-Coder XML and a Hermes call)
and by dialect allow-list:

| Dialect | Cases | Correct | Incorrect | Abstained |
| --- | ---: | ---: | ---: | ---: |
| `hermes` | 1152 | 747 | 0 | 405 |
| `mixed` | 64 | 64 | 0 | 0 |
| `openai` | 854 | 503 | 0 | 351 |
| `qwen` | 1164 | 793 | 0 | 371 |

| Allow-list | Cases | Correct | Incorrect | Abstained |
| --- | ---: | ---: | ---: | ---: |
| `hermes.v1` | 542 | 358 | 0 | 184 |
| `hermes.v1,openai-tool-calls.v1,qwen3-coder-xml.v1` | 1712 | 1080 | 0 | 632 |
| `hermes.v1,qwen3-coder-xml.v1` | 64 | 64 | 0 | 0 |
| `openai-tool-calls.v1` | 382 | 231 | 0 | 151 |
| `qwen3-coder-xml.v1` | 534 | 374 | 0 | 160 |

Rules no case exercises: `tool_call.markup.terminal-confusable` (no built-in
grammar declares a confusable spelling) and `json.syntax.literal-escape` (no
witness value contains a backslash to unescape).
