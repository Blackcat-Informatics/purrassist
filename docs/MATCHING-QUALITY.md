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
