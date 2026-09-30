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
correction_evaluation.rs` complements it with four much larger, first-party,
deterministic corpora (`crates/core/tests/data/evaluation/`, provenance in
that directory's `README.md`) — string selection, path matching, English
phonetic ambiguity, and JSON Schema recovery — generated by `generate.py` in
that directory and measured statistically rather than case-by-case.

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
