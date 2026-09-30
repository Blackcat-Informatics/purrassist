<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Coverage-guided and differential qualification

`scripts/fuzz.py --target <name>` generalizes the original single-purpose
JSON runner (`scripts/fuzz-json.py`, now removed) across eight
`purrassist-core` targets: `json`, `markdown`, `schema`, `pattern`,
`unified`, `fuzzy`, `matching`, `patch`. Each target is a process-isolated
Rust example (`crates/core/examples/fuzz_<target>.rs`, registered as
`[[example]] name = "fuzz-<target>"` in `crates/core/Cargo.toml`) that reads
bounded stdin (at most 4,096 bytes), asserts the target's invariants, and
completes its protocol (`ok` or `invalid-utf8`, optionally followed by
newline-delimited JSON differential records) or panics.

Run with Rust `nightly-2026-09-14` and its bundled `llvm-tools` component
(`rustup component add llvm-tools`). `scripts/fuzz.py` resolves `llvm-cov`
and `llvm-profdata` via `rustc --print sysroot`, not a separately installed
system package: the bundled component is always built from the exact LLVM
revision `rustc` itself uses, which a coincidentally version-matched system
package is not guaranteed to be. Both tools' reported LLVM version (with the
`-rust-<rustc version>` suffix the bundled component appends stripped before
comparison) must match `rustc -vV`'s own LLVM version and the
`llvm-coverage` entry pinned in `vendor/tooling.json`; a mismatch fails the
campaign before it builds anything. Only the qualification executable
receives `-C instrument-coverage`, at optimization level 1 with debug
information, debug assertions, and overflow checks. This campaign does not
qualify the exact release binary.

## Coverage feedback

Source-region feedback (not edge coverage or sanitizer instrumentation), as
documented by the
[Rust compiler](https://doc.rust-lang.org/rustc/instrument-coverage.html) and
[LLVM coverage tool](https://llvm.org/docs/CommandGuide/llvm-cov.html): a
mutation is retained in the corpus only when it exercises a previously
unseen code region in `crates/core/src`. Harness and standard-library
regions, uncovered regions, execution-count changes, and expansion/gap
regions never count as novelty; generic instantiations share source-
coordinate identities. Every fixed seed is retained unconditionally. The
fixed LCG mutation algorithm (insert/delete/replace/duplicate/wrap, drawn
from a per-target token set) and retained corpus determine every subsequent
input deterministically from `--seed`. Successful per-input coverage
profiles are deleted and never merged across inputs for novelty decisions;
a timeout or tool diagnostic fails the campaign rather than silently
skipping.

## Targets, invariants, and seed provenance

Every probe rejects invalid UTF-8 explicitly before entering the core (the
report counts those executions separately). Resource exhaustion
(`StopReason::BudgetExceeded`/`Cancelled`) is a declared non-result, not a
successful comparison. The Markdown probe emits exactly one HTML or closed
resource-stop record, with the original source, parse/render/neutral-render
stage, exact reason and exhausted dimension. The runner records these in
`resource_non_results`, `resource_non_result_reasons` and distinct
`resource_non_result_cases`, rejects missing/duplicate/contradictory records,
and fails on unexpected parse/render errors such as `InvalidInput`. Other
probes retain their documented compile or structural-operation refusal rules;
these new Markdown stop records are not reconstructed into old phase reports. Multi-field probe inputs are split on the
ASCII record-separator control byte (`\u{1e}`, included in the relevant
targets' mutation tokens so mutation can create or move field boundaries).

- **`json`**: strict parse/serialize/parse round trips and stable
  serialization; recovery against four fixed schemas, with every returned
  answer (including each `Ambiguous` alternative) revalidated by the schema
  engine; source digest, guess classification, alternative-ledger count,
  and reported work/candidate/scratch usage checked against the supplied
  budget. Seeds are the original `fuzz-json.py` corpus plus seven seeds that
  reimplement the six corruption transformations tested against a
  known-origin witness in `crates/core/tests/corruption_oracle.rs` (Task 8:
  truncation, prose wrapping, key typo, stringification, type swap in both
  directions, duplicate keys), applied here to one small canonical object
  independent of that test code, purely to seed this campaign with the same
  corruption shapes.
- **`schema`**: a schema source and an instance text. Compilation and
  recovery never panic; a generated witness
  (`Schema::generate_witness_governed`) always validates against its own
  schema; recovery never returns a `Resolved`/`BestGuess` value, or an
  `Ambiguous` alternative, that fails validation against its own schema —
  the schema engine is the oracle here, not an independent implementation.
  Seeds cover `type`, `enum`, `allOf`, `not`, `uniqueItems`, `false`, and
  `pattern`, in the same style as the official JSON-Schema-Test-Suite
  already vendored at `crates/core/tests/data/json-schema/`, plus the same
  seven corruption-oracle instance shapes as `json`.
- **`pattern`**: a pattern source and a subject text. Compilation never
  panics; the derivative and backtracking engines
  (`Pattern::is_match`/`is_match_backtracking`) agree whenever both
  complete; any `Pattern::generate` witness matches its own pattern. Seeds
  cover classes, quantifiers, named groups, backreferences, lookaround,
  `\p{Script=...}`, anchors, and the documented pathological
  `(a+)+$` case. **Differential**: Node's `RegExp` (`u` mode) — see below.
- **`unified`**: a source buffer and a unified-diff patch text
  (`edits::plan_unified_governed`). Planning never panics; every
  `Resolved`/`BestGuess` plan, and every `Ambiguous` alternative (checked
  against the receipt's alternative-ledger count), applies and its inverse
  restores the exact original source (apply-then-invert identity, via
  `crate::patch::Patch`).
- **`fuzzy`**: a source, needle, replacement, and maximum edit distance
  (`edits::locate_fuzzy_governed`). Same apply-then-invert and
  alternative-ledger checks as `unified`.
- **`matching`**: a query and a list of candidate spellings
  (`matching::rank_governed`/`resolve_governed`). Ranking and selection
  never panic; every ranked or selected identity is among the supplied
  candidates; an `Ambiguous` outcome's alternative count matches the
  receipt's alternative ledger. Seeds include the documented
  numeric-protected-class exclusion (`invoice-1001` vs `invoice-1002`) and a
  negation-marker case.
- **`patch`**: a base text and two independently modified texts
  (`crate::patch::Patch::diff/apply/invert/commute/rebase`).
  Apply-then-invert restores the exact base text; when two patches derived
  against the same base commute, applying either one first and the other
  rebased onto it reaches the same final text regardless of order (the
  commute law); when they do not commute, `rebase` explicitly refuses.
- **`markdown`**: a Markdown source. Parsing never panics; every structural
  operation this crate exposes (section insert/remove/rename/move,
  list-item edit/move/renumber, table-row/column edit, and every reference
  operation) either refuses cleanly or produces a plan whose
  apply-then-invert restores the exact original source. Seeds are adapted
  from examples already vendored at
  `crates/core/tests/data/markdown/{commonmark-0.31.2,gfm-0.29.0.gfm.13}.json`
  (headings, lists, block quotes, fenced code, tables, reference links, GFM
  strikethrough/autolink/tasklist/tagfilter, setext headings, hard breaks).
  **Differential**: `cmark-gfm` — see below.

## Differential probes

**`pattern` vs Node's `RegExp`.** Node (pinned in `vendor/tooling.json`,
already used for the wasm acceptance transcripts) is invoked out-of-process
per admitted execution whose pattern compiles and whose match completes:
`new RegExp(pattern, 'u').test(subject)`. A pattern Node itself refuses to
compile under `u` mode is counted as a reference rejection, not a mismatch — this restricts the
comparison to the ECMA-262 subset both engines implement. Node timing out
under the bounded `--timeout` is counted separately as a reference timeout, never raised as a mismatch:
V8's own backtracking regex engine is exactly as vulnerable to catastrophic
backtracking as any backtracking engine (this crate's derivative engine is
specifically designed to avoid that on the same inputs, e.g. the documented
`(a+)+$` seed against a long run of `a`s), so a reference-tool timeout on an
adversarial pattern says nothing about which engine, if either, is wrong.
Any other disagreement is a crash-equivalent failure, reported with the
exact pattern, subject, and both engines' answers.

**`markdown` render vs `cmark-gfm`.** `scripts/fuzz.py` builds the pinned
`cmark-gfm` `0.29.0.gfm.13` CLI (matching the GFM example corpus already
vendored at `crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json`) from a
digest-verified source tarball into `.cache/tools/cmark-gfm-0.29.0.gfm.13/`,
cached across runs; registered in `vendor/tooling.json` with its license
notice under `vendor/licenses/`. Every admitted execution's
`Document::render_html(Dialect::Gfm, ...)` output is compared, after
normalization, against `cmark-gfm -e table -e strikethrough -e autolink -e
tasklist -e tagfilter --unsafe --strikethrough-double-tilde` on the identical source.

The explicit `--strikethrough-double-tilde` selects the documented GFM
profile: double tildes create strikethrough, while a lone tilde is literal.
The CLI defaults to an additional single-tilde extension unless this flag
is supplied. Supplying it aligns the independent parser's dialect with the
GFM specification fixtures and PurRAssist's `Dialect::Gfm`.

The tasklist reference has independently identified limits. Given
`- - [x] done`, or `> - [x] done`, the task item and its outer container
open on the same physical line. The pinned `cmark-gfm` tasklist extension
leaves the marker as text. The [GFM task-list specification](https://github.github.io/gfm/#task-list-items-extension-)
defines a task by the first paragraph and marker and permits arbitrary
nesting; the [list composition rule](https://github.github.io/gfm/#list-items)
permits same-line nested containers (example 276). PurRAssist preserves
that specified checkbox behavior. The pinned extension also searches the entire
remaining line for `[x]`/`[X]`, incorrectly checking an initially unchecked
task when its content contains a later literal checked marker. The first
marker alone determines the state under the same specification. The extension
also strips a marker before ordinary paragraph continuation is finished:
`- [ ] \ntodo` must retain the lazy continuation in its first paragraph.
A logical newline may separate the marker from content or be its single
unchecked whitespace character; tab, vertical tab, and form feed are also
ASCII whitespace under the grammar. The pinned line scanner does not
implement all of these paragraph-level cases.

Before executing the probe, an independent raw-input prefix recognizer in
`scripts/fuzz.py` selects `tasklist-reference-limited` for those
precise raw-source conditions: same-line containers, a later checked literal,
non-space marker whitespace, an empty first task line, or a marker split
across physical lines. Physical lines split only at CR/LF; marker
separators include ASCII space, tab, vertical tab, and form feed, matching
the specification and the pinned scanner. Selection never occurs after
a mismatch. For this profile both differential renderers disable the
reference's tasklist extension and compare all remaining block, inline,
table, strikethrough, autolink, and tag-filter behavior. Every other input
uses all five extensions.

The probe also retains its full tasklist-enabled rendering. An independent
projection of literal first-paragraph markers in the neutral HTML into
specified checked/unchecked inputs must equal that full rendering,
preserving every other byte of HTML when its source provenance can be
certified. Availability is selected from raw source before probe execution.
Sources containing raw HTML, entity/escape spelling, a possible marker
spanning a physical newline, an `x`/`X` reference-definition label, or a
source-selected otherwise empty task first line with only spaces/tabs after
the marker receive
an explicit unavailable projection count. Neutral HTML can create or hide
marker-looking text in these cases. In particular, softbreak rendering
discards indentation and trailing spaces, so its displayed `[\n]` cannot
establish the parsed paragraph's exact marker grammar or lazy-prefix
interpretation. A reference link can likewise hide the literal marker.
The neutral rendering of an empty `[x] ` task also trims the required
separator, making it indistinguishable from a bare non-task `[x]` without
its source; the raw prefix recognizer selects this loss before execution.
Both full and neutral core outputs and the reference outputs remain
available; the strict neutral full-content differential still executes.
These inputs do not count as completed independent full-task projections.
An extra checkbox without a raw-source unavailable certificate fails;
availability never depends on the core's checkbox count. Reports
include `reference_profile_exclusions`, `tasklist_projection_checks`,
`tasklist_projection_unavailable`, and each distinct excluded source,
its digest, exact selected reasons, `projection_unavailable_reasons`, and
whether the projection completed. Disagreement in the selected neutral or
content HTML contract remains a campaign failure; unavailable full-task
projections are counted separately.
`markdown_render.rs` additionally generates 192 authoritative tasklist
cases covering depths 1–8, same-line lists and quotes, checked/unchecked
and uppercase markers, spaces/tabs, and LF/CRLF, preserving exact source.
Another 32 source cases verify initially unchecked tasks containing later
checked literals, all four horizontal ASCII separators, and LF/CRLF.

The reference-definition oracle has a separate admission correction. The
[CommonMark destination grammar](https://spec.commonmark.org/0.31.2/#link-destination)
requires balanced unescaped parentheses in a bare destination. The pinned
reference's `manual_scan_link_url_2` tracks depth but fails to reject a
nonzero depth at whitespace or EOF; it can discard an invalid definition
that must remain ordinary source content.

Before the probe executes, a raw-source recognizer selects only a first-line
definition-shaped prefix with a nonempty label, no unescaped label brackets
or non-whitespace control characters, a bare destination, and a certified
unmatched opening parenthesis. A label may contain a physical newline under
the primary grammar, but a blank line ends the paragraph and does not qualify. Balanced or escaped parentheses, angle destinations, unmatched
closing parentheses, and definitions beginning on later lines do not qualify.
Both the original reference HTML and a second guarded reference execution
are retained. A fixed plain sentinel before the first source line prevents
faulty definition admission; the oracle removes only that exact sentinel
from a verified leading paragraph, setext heading, or table-header position.
An absent or unexpected prefix fails closed. Every other byte of guarded
HTML, including continuation lines and subsequent blocks, is compared to
the core rendering of the original source.

Reports record `reference_admission_projections`, the number of original
reference disagreements in `reference_admission_disagreements`, and exact
source/digest, core HTML, raw reference HTML, and guarded reference HTML
in `reference_admission_cases`. A successful comparison uses the certified
guarded oracle for these inputs; it does not claim agreement with the
unmodified reference or silently normalize away its differing content.

The independent reference-label certificate also admits literal backslashes
before non-punctuation characters. Such characters remain ordinary label text;
backslashes before ASCII punctuation retain the separate escape rule. Unescaped
brackets, controls, blank lines and balanced destinations remain excluded from
this invalid-destination certificate. Tests include Unicode and whitespace
following a literal backslash and preserve the exact original source.

A possible task marker together with a physical blank line selects the neutral
task profile before execution, since the pinned reference inserts checkboxes
outside the first `<p>` in loose list items. The task-list grammar replaces the
marker **inside the first paragraph**. The strict full-task projection verifies
that placement and all other paragraph content; moving a checkbox outside its
paragraph still fails. Neutral whole-content comparison remains mandatory, and
both full raw reference and full core answers are retained.

The same guarded oracle also selects one narrow title-admission defect:
an isolated first physical definition-shaped line whose same-line title has
only an escaped terminal closing delimiter and no unescaped closer, followed
only by blank lines. [Link titles](https://spec.commonmark.org/0.31.2/#link-title)
require an unescaped closing quote or parenthesis. The pinned title scanner's
ordinary-character alternative includes backslashes, so it can incorrectly
consume `[tea]: /tea "Tea\\"` as a definition. This independently selected
certificate covers all three title delimiters and fails closed on even escape
counts, valid later closers, multiline titles or other following blocks.
The same exact sentinel stripping and whole-content comparison apply; reports
record the distinct admission profile/reason and both raw and guarded answers.

The pinned reference inline parser trims paragraph-final whitespace but can
retain initial physical spaces after a reference definition. For example,
`- [d]: /x` followed by a lazy ` a` line produces `<li> a</li>` in the reference
and `<li>a</li>` in the core. [GFM paragraph content](https://github.github.io/gfm/#paragraphs)
removes initial and final whitespace. Before execution, the runner certifies
sources containing neither `<` nor `&`; only in this subset, it removes ASCII
spaces or tabs immediately after generated `<p>` and `<li>` opening tags in
the reference HTML. Code, interior whitespace, semantic entities and raw HTML
remain outside this transformation. The whole resulting HTML is compared.
Reports retain the source, raw reference, core and projected answers in
`paragraph_prefix_reference_cases`, with distinct projection and original
disagreement counters. Successful projected comparisons do not claim original
reference agreement. A possible task marker coexisting with a definition
selects the neutral task profile before execution, since the pinned task
extension also misses first-paragraph markers following definitions.

The pinned backslash-hardbreak handler also omits the leading-space skip
performed by its ordinary newline handler. A backslash at the end of a list paragraph followed by an indented lazy
continuation can consequently retain indentation after `<br>`.
[CommonMark §6.7 examples 636–637](https://spec.commonmark.org/0.31.2/#hard-line-breaks)
require it to be ignored for both hardbreak spellings. The core follows
that rule; authoritative tests include ordinary paragraphs, lists, nested
lists, quotes, all line endings, and literal code/entity whitespace controls.

Before execution, a separate raw-source certificate selects sources with
a possible physical hardbreak and **neither `<` nor `&`**. In that subset,
actual `<br>` markup can only be generated hardbreaks; raw HTML and
entity-produced semantic whitespace cannot qualify. The oracle projection
removes only ASCII spaces/tabs immediately after a generated `<br>` and
its newline. It preserves every other byte of the reference HTML and
leaves the core HTML unchanged. Raw reference output, output before this
projection, projected output, original source, digest, and core renderings
are retained for each actual correction. Reports distinguish
`hardbreak_reference_projections`, `hardbreak_reference_disagreements`,
and `hardbreak_reference_cases`; successful corrected comparisons do not
claim agreement with the unmodified reference. Code content and whitespace
elsewhere are protected by tests, and lost content still fails comparison.

The pinned reference's backtick cache has another primary-grammar defect.
An unmatched opener scans to the paragraph end and caches its furthest
closers. A subsequent successful span then overwrites that cache entry with
an earlier closer, incorrectly suppressing later spans of the same width.
[GFM §6.3](https://github.github.io/gfm/#code-spans) requires equal-width
openers and closers to form those spans. The core retains that behavior.

The runner builds a separate reference executable from the same verified
archive with one cache correction: retain the maximum known closer position.
The original executable remains unchanged. An exact unique source preimage
and the correction digest registered in `vendor/tooling.json` are required;
unexpected source or registration fails closed. Before probe execution,
raw input containing a backtick selects this corrected independent oracle.
Both original and corrected references execute, and all corrected HTML is
compared against the core. Selection never depends on their disagreement.
Reports record the archive, patch preimage/postimage and digest, corrected
source-file digest, and both executable digests. Actual raw disagreements
retain source/digest, both reference outputs, and core renderings in
`backtick_reference_cases`; `backtick_reference_checks` and
`backtick_reference_disagreements` distinguish corrected checks from raw
agreements. These comparisons do not claim agreement with the unmodified
reference. Executed controls cover unmatched and balanced openers, multiple
span widths, line endings, and content loss. The separate build uses the
existing registered CMake/C toolchain and upstream licenses; production
crates do not include it.

Each Markdown campaign also executes the original and corrected references
against all 672 vendored GFM examples and requires identical outputs between
the two executables. Normalized comparison with the specification HTML is
recorded separately: the unchanged pinned renderer differs on examples
140, 141, 142, 145, 147, 610, 614, 616, 619, and 620. That compatibility
check proves the cache correction preserves existing reference behavior
on the corpus; it does not claim the reference passes those ten examples.

The unbalanced-destination guard also admits one physical CR/LF/CRLF
separator between the definition colon and its bare destination, as allowed
by the reference-definition grammar. `[tetea]:` followed by `(` is literal
paragraph content; the pinned scanner incorrectly consumes it. A dedicated
matcher extends only this invalid-destination certificate. Blank lines,
balanced or escaped parentheses, and angle destinations are rejected;
the isolated escaped-title matcher retains its same-line contract. Exact
guard preservation and strict whole-HTML comparison still apply.

Container reference definitions use a further registered independent scanner
correction. The pinned `manual_scan_link_url_2` tracks bare-destination
parentheses but fails to reject nonzero depth when scanning stops at whitespace.
[GFM link destination grammar](https://github.github.io/gfm/#link-destination)
requires balanced unescaped parentheses. A separate executable combines the
registered backtick/emphasis fixes with one exact unique full-function preimage
change: reject a nonzero final depth. Original and earlier corrected binaries
remain unchanged; archive, patch, source and all executable identities are
recorded. No production crate links this reference implementation.

Before execution, raw input containing both `]:` and `(` selects the balanced
scanner unless an existing independently certified literal-prefix guard already
applies. This admits potential definitions regardless of their container or
whether their parentheses are actually invalid; it never depends on a mismatch.
The core and corrected oracle receive the complete original document unchanged.
Balanced and escaped bare destinations, angle destinations, later valid shadow
references and literal uses are verified alongside malformed multiline labels,
quotes, nested quotes and lists under LF/CRLF/CR. Each campaign executes 27
controls (18 disagreements with the previous reference) and all 672 official
GFM examples, requiring identical old/new outputs on that corpus and recording
the ten unchanged reference/gold differences separately.

`destination_reference_checks`, `destination_reference_disagreements` and
`destination_reference_cases` retain separate correction counts, the exact
source, full core HTML, unmodified reference HTML, the preceding corrected
reference HTML and final corrected HTML. Strict comparison of all final HTML
still rejects missing literal content, changed later blocks or invented links.
These results do not claim agreement with the original reference.

The pinned reference also shares its emphasis opener search floor between
close-only and dual-purpose delimiter runs. A failed dual-purpose match can
then hide a valid later close-only match. The
[GFM emphasis processing algorithm](https://github.github.io/gfm/#emphasis-and-strong-emphasis)
and [upstream CommonMark cmark 0.31.1](https://github.com/commonmark/cmark/blob/0.31.1/src/inlines.c)
keep these categories separate. For example, `*a**b** c**` forms emphasis
around `a`, strong `b`, and ` c`, followed by a literal final star. The core
and actual upstream parser agree; the original pinned GFM parser leaves the
outer stars literal.

Before probe execution, raw `*` or `_` selects a separate independent
reference executable combining the backtick correction with this emphasis
floor correction. Quotes and extension delimiters retain their original
floor behavior. Every exact unique source preimage and the
`emphasis_floors_patch_sha256` registration must match; unexpected source or
registration fails closed. The original and backtick-only references remain
unchanged. Reports identify the archive, both patches and their full
preimages/postimages, corrected source, and executable digests.
`emphasis_reference_checks`, `emphasis_reference_disagreements`, and
`emphasis_reference_cases` distinguish the combined output from the
backtick-only output, retaining the raw original, each selected reference,
and complete core output. Each campaign executes 11 independently checked
controls and all 672 official examples; the latter must remain byte-identical
between backtick-only and combined references. The same ten unchanged
reference/specification differences are recorded separately. Corrected
comparisons do not claim unmodified reference agreement.

The pinned extended-email parser retains its domain-period counter when
retrying at another `@`. The
[GFM extended-email grammar](https://github.github.io/gfm/#autolinks-extension-)
requires a period after the particular address's own `@`. For example,
`user@ee.or@exampl~om` is literal, whereas the original renderer links
`ee.or@exampl`. The runner builds a further separate reference executable
combining the three earlier registered patches with a correction at the exact
unique `found_at:` scanner-label preimage in `extensions/autolink.c`: reset
`np` before scanning each retried candidate. The original and earlier corrected
executables remain unchanged; upstream licenses and notices apply, and no
production crate links this implementation.

Before execution, raw `@` **or** `&` selects this corrected reference, including
potential entity-produced at-signs. Both core and reference receive the complete
original source; no HTML link is unwrapped or removed. The text-node scanner
retains protected code/HTML/explicit links and independently admits valid final
email domains. Each campaign executes 69 controls across LF/CRLF/CR (39
disagreements with the immediately preceding reference), including entirely
encoded `&#64;`, `&#x40;` and `&commat;` delimiters, encoded valid-domain dots,
escaped delimiters, multiple retries, literal HTML attributes, raw script data,
angle/explicit/reference links, URL paths and code. All 672 official GFM
outputs must remain byte-identical before/after this correction; the ten
unchanged original reference/gold differences are recorded separately.

Reports retain archive and all patch digests/preimages/postimages, both corrected
source-file digests, and every reference executable identity.
`email_reference_checks`, `email_reference_disagreements` and
`email_reference_cases` distinguish this correction from the immediately
preceding destination-corrected binary executing the same source. Cases retain
full original, preceding, final and core answers. The preceding binary runs for
every email-selected input even if no standalone destination profile applies,
so another patch's effect cannot be attributed to this email correction. Strict
whole-HTML comparison rejects invented links and any lost or changed content.

Earlier failed phase reports retain their original narrow email projection
metadata and answers unchanged. That source-certified projection was superseded
by the registered scanner correction for new campaigns; new results do not claim
agreement with the unmodified reference, and historical metadata is never
reconstructed or rewritten.

Comparison normalization (`normalize_html`, unit-tested in
`scripts/test-fuzz.py`) canonicalizes only formatting with no HTML meaning:
CRLF line endings, a missing/extra trailing newline, attribute order within
a tag with unique attribute names (duplicate names preserve their order
because the first value wins, case-insensitively; comment and CDATA content
are preserved), and a void
element's optional self-closing `/>` (also insignificant — `<input ...>` and
`<input ... />` are the same element). The last two account for a
**documented spec divergence**: the official GFM specification examples
(e.g. `gfm-0.29.0.gfm.13.json` example 279, `- [ ] foo\n- [x] bar\n`) render
task-list checkboxes as `<input disabled="" type="checkbox">` — exactly what
`Document::render_html` produces, and what
`crates/core/tests/markdown_render.rs::official_documents` checks
byte-for-byte against that same corpus (1,324/1,324 examples) — while the
`cmark-gfm` CLI's own default HTML renderer always self-closes void elements
and orders attributes differently, diverging from its own project's
specification fixture in this purely stylistic way. No other difference is
normalized away.

Raw-text and RCDATA bodies (`script`, `style`, `xmp`, `iframe`, `noembed`,
`noframes`, `plaintext`, `title`, `textarea`) and complete SVG/MathML fragments
remain opaque, including nested foreign elements and tag-looking literal text.
Tokenization starts at the earliest `<` and is anchored there. Valid custom
tag names remain eligible for ordinary attribute normalization; unrecognized
or malformed markup remains opaque. Recognized declarations, processing
instructions and closed lexical tags retain their complete spelling and then
resume normalizing real following tags; genuinely unterminated tokens retain
their whole remaining fragment opaque. Attribute separators use only HTML
ASCII whitespace, preserving nonbreaking spaces and vertical tabs as data. Searching
past an unrecognized outer token
cannot turn tag-looking attribute data into a void element. Quoted attributes,
comments and CDATA cannot introduce false raw-text starts.
Unterminated opaque content extending to EOF retains its trailing newlines;
terminal formatting-newline adjustment applies only outside such content.
These controls prevent changed script strings, visible textarea data or foreign
self-closing semantics from becoming an apparent agreement.

## Regression fixes discovered by fuzzing

The campaigns identified and repaired additional defects, with executable
regressions in `crates/core/tests/pattern.rs` and
`crates/core/tests/markdown_render.rs`:

- The derivative-state cache now distinguishes the start-of-input anchor
  context. Previously `^a$` could match `a a`, despite the backtracking
  engine and V8 rejecting it.
- GFM bare URLs respect the preceding-character boundary and unresolved
  link-bracket context. `[www.example.org]` stays plain text, including
  when inline emphasis splits the surrounding text into multiple nodes.
- A blockquote's lazy paragraph continuation stops before an empty list
  item or an ordered list starting outside the quote.
- GFM email scanning rejects a candidate followed by a second at-sign and
  resumes the local-part scan beyond the first at-sign.
  `user@example.comuser@example.com` therefore links only
  `example.comuser@example.com`, matching `cmark-gfm`. Protocol-prefixed
  mailto and XMPP addresses preserve their protocol in the label and
  destination, and consecutive domain periods terminate the candidate.
  Resolved nested links preserve the surrounding unresolved-bracket context.

- Single-column table headers may omit pipes; their delimiter rows still
  determine the column count. Setext headings retain precedence, list
  markers interrupt paragraphs before table admission, and a delimiter
  rejected for a header-width mismatch preserves the current paragraph.
  Controls include later table admission after a blank line and scanner
  rows that are not valid delimiter candidates. Every list item, including
  empty items and ordered items starting above one, and indented code end
  a table body; twenty LF/CRLF source cases cover those boundaries.
- Task recognition follows the first parsed paragraph after original tab
  spans are restored. A newline may separate the marker and content; a
  single ASCII whitespace character may form an unchecked marker. Task
  edits target that paragraph even when definitions precede it, and a
  split marker edit records the exact joining splice and inverse.

- Extended email/protocol autolinks scan interpreted text nodes, as required
  by GFM. Escaped punctuation and decoded entities can form an email; a
  charged linear source map retains each original escape/entity spelling in
  the resulting ranges. Multi-character entities, Unicode prefixes/suffixes,
  multiple-at-sign retries, protected code/HTML/links, literal entity suffixes
  without double interpretation, all line endings and container prefixes are
  checked. Work/scratch exhaustion and cancellation are exact typed errors.
- Email retries resume at the first unconsumed domain delimiter rather than
  inside an examined domain. `a@b.@c.com` links only `.@c.com`; an adjacent
  address after a completed link starts a fresh text segment. Sixteen
  source cases exercise multiple at-signs, domain punctuation, protocols,
  adjacent addresses, and LF/CRLF.

- A lone pipe terminates a table body because it has no explicit cells;
  it becomes a following paragraph instead of a fabricated empty row.
- Bare URLs are recognized before emphasis delimiter resolution, preventing
  embedded tildes in URLs from creating spurious strikethrough. Unicode
  symbol characters in a host remain admitted where GFM permits them;
  protocol spelling, short protocol hosts, and trailing punctuation follow
  the reference parser's rules.
- Task markers require separating ASCII whitespace after the closing
  bracket within the first parsed paragraph. A bare `[x]` at paragraph EOF
  remains literal; a newline followed by paragraph content is a valid
  separator. Empty first task lines followed by paragraph continuation
  discard the marker's leading paragraph whitespace instead of inserting
  a spurious soft break. Regression
  cases cover 60 marker/separator/end-of-input combinations, both dialects,
  source preservation, editing-index state, and paragraph continuations.
- Bare-URL domain admission counts periods and segment underscores before
  trimming trailing URL delimiters. A punctuation suffix such as `www.[org`
  remains part of the URL after the domain scan ends, instead of creating an
  unmatched inline bracket that prevents autolinking. Regression cases cover
  brackets, braces, paths, queries, escaped underscores, short protocol hosts,
  final delimiters, and trailing domain periods under LF/CRLF and both dialects.
- Paragraph rendering removes ordinary block indentation before inline
  parsing and preserves indentation in lazy continuation lines whose
  container prefix was omitted. Raw HTML and code spans therefore retain
  their specified whitespace; the document retains every original byte.
- Multiline setext titles concatenate inline source before trimming only
  their outer spaces and tabs. Interior hardbreak spaces, raw lazy indentation,
  code-span/HTML whitespace, and residual padding from partially consumed tabs
  remain intact. Regression cases cover LF/CRLF/CR, both dialects, exact original
  source ranges, heading rename and byte-identical inverses.
- An indented continuation of an already open paragraph can become a GFM
  table header when a matching non-lazy delimiter follows. Standalone indented
  code, blank boundaries, lazy delimiters, and indented table-body boundaries
  retain their distinct block rules. Root, explicit/lazy quote, nested list,
  tab and partial-tab controls verify rendering, original cell spans, governed
  editing and exact inverses. No reference projection replaces these core
  admission checks.
- GFM tables interrupt preceding paragraphs and stop before any HTML block,
  including complete Type 7 tags. When Tables is disabled, rendering
  reinterprets the source under ordinary block rules, preserving CommonMark
  paragraphs, HTML, setext headings, and reference definitions across those
  table boundaries. The source-addressed structural table index remains
  available for editing.
- Empty link/image titles retain their parsed source metadata and omit the
  HTML title attribute, matching the reference renderer. Generated tests
  cover all three title delimiters, inline/reference forms, and both links
  and images across empty, nonempty, whitespace, and entity-encoded titles.
- Inline HTML declarations require uppercase ASCII names followed by ASCII
  whitespace. Lowercase declarations and uppercase names without separating
  whitespace are escaped; block HTML retains its separate declaration rule.

### Bounded HTML grammar diagnostics

`scripts/probe-markdown-html-boundaries.py --report <campaign>/report.json`
compares 2,592 independently generated declaration, comment, processing
instruction, CDATA, and tag boundary cases against the recorded private
executable and pinned reference. The archived
[diagnostic observations](qualification/markdown-html-boundaries-linux-x86_64.json)
record 2,574 exact differential agreements and 18 reference limits, with no
unexpected mismatches. This is a diagnostic probe, not a claim that all
2,592 independent comparisons succeeded.

The 18 disagreements have a hyphen immediately before an inline comment's
closing `-->`. For example, `a <!--x---> b` remains literal raw HTML under
[CommonMark 0.31.2 §6.6](https://spec.commonmark.org/spec#raw-html), which
permits comment content without `-->`; the pinned `cmark-gfm` scanner
incorrectly escapes it. Its own source `test/spec.txt` states the modern
rule, while `src/scanners.re` retains the older trailing-hyphen restriction.
[The corresponding upstream reference defect](https://github.com/commonmark/commonmark.js/issues/285)
documents this exact grammar discrepancy. Renderer tests generate 63
modern comment cases independently, including consecutive/trailing
hyphens, line endings, NUL replacement, and quote/list containers.
These diagnostic reference limits are selected from raw syntax before
execution and retained with both answers. They do not change `fuzz.py`'s
mandatory campaign profile: a disagreement there still fails, including
this known reference corner if encountered.

The same diagnostic runner's `--case-set links` generates 1,008 independent
link/image title comparisons; [its archived report](qualification/markdown-link-boundaries-linux-x86_64.json)
records 1,008 agreements and no reference limits or unexpected mismatches.
The `--case-set tables` generates 1,728 table/paragraph/container boundaries;
[its archived report](qualification/markdown-table-boundaries-linux-x86_64.json)
records 1,584 agreements, 144 precisely selected reference limits, and no
unexpected mismatches.

Those 144 limits are header-only tables followed by another block inside a
list item without a source blank line. The pinned extension consumes the
whole delimiter line and retains a blank-line flag for the table. A following
block therefore makes the reference list loose, whereas inserting a single
body row restores tight rendering. [GFM §5.4](https://github.github.io/gfm/#lists)
requires an actual blank line between items or directly contained blocks
for loose rendering. Authoritative tests preserve tight list behavior with
and without a body row; the diagnostic retains every disagreement and both
HTML answers. Mandatory campaign acceptance remains unchanged.

## Running a campaign

```sh
make fuzz TARGET=json FUZZ_ARGS='--iterations 10000 --seed 6491701121104122699'
make fuzz TARGET=markdown FUZZ_ARGS='--replay .cache/fuzz-markdown-EXAMPLE/current.bin'
make fuzz  # every target once, at the smoke count (1,000), with TARGET unset; FUZZ_ARGS ignored
```

Replay rebuilds and tests the saved bytes against the current source.
Preserve the report and source revision to reproduce the original build.
Each new build records separate hashes of the core engine and build manifests,
the target harness, and the runner, alongside the executable hash. These
inputs must stay byte-identical before and after compilation or the runner
refuses to campaign. The built executable is copied into the unique campaign
directory before execution, so concurrent Cargo builds cannot replace the
probe between inputs. The full nonignored repository digest additionally
identifies documentation and qualification evidence at campaign start;
unrelated documentation changes do not alter the engine/harness identity.
Earlier reports retain their recorded executable and repository identities;
source hashes are never reconstructed and added to an old run.
Exact discovery counts are tied to compiler, flags, and source mapping; they
are not comparable coverage percentages across toolchain versions.

**Smoke** (`verify.yml` on pull requests, main-branch pushes, and manual
runs): `make fuzz` runs the corpus-feedback test once and all eight targets
with the runner's default 1,000 mutations each. `TARGET` and `FUZZ_ARGS`
apply only to a selected-target invocation. **Qualification** (this task, evidence committed
under `docs/qualification/`): 10,000 iterations per target, matching both
this plan's stated floor and the sample size of the existing
`derivative_and_backtracking_engines_agree_on_ten_thousand_generated_pairs`
and property-based oracles elsewhere in this codebase, providing the same mutation-count floor for every target. Counts measure
executed cases and do not imply statistical independence or a bound on
undiscovered defects. `docs/qualification/fuzz-<target>-linux-x86_64.json` records
successful status, executions, regions covered, retained corpus size,
differential checks performed, zero differential mismatches, the seed, and
tool identities after clean qualification runs. Reports with an explicit
`crashes` field record zero; the older fuzzy, matching, patch, and unified
phase reports have no such field, and no crash metadata is added retrospectively.

Qualification records are **phase observations**, not a claim that every
10,000-case campaign executed against the final PR head. Earlier reports
record their exact built executable and repository digest at campaign
start, before subsequent accounting and Markdown corrections. Their source hash
fields are never reconstructed retrospectively. New Markdown reports
add guarded engine/harness/runner hashes and a private executable snapshot.
The full local gate and hosted smoke campaigns on the exact final commit
provide separate final-integration evidence; hosted smoke must execute
1,000 mutations for each of all eight targets.

### Recorded 10,000-mutation baselines

Every row is copied from its actual successful phase report. Counts include fixed
seeds in addition to the 10,000 mutations; they are observations of the recorded
source/tool identities, rather than whole-crate coverage percentages.

| Target | Executions | Regions | Retained | Differential checks | Mismatches | Crashes |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| [json](qualification/fuzz-json-linux-x86_64.json) | 10,021 | 3,799 | 86 | 0 | 0 | 0 |
| [markdown](qualification/fuzz-markdown-linux-x86_64.json) | 10,015 | 5,524 | 158 | 9,683 | 0 | 0 |
| [schema](qualification/fuzz-schema-linux-x86_64.json) | 10,016 | 4,766 | 95 | 0 | 0 | 0 |
| [pattern](qualification/fuzz-pattern-linux-x86_64.json) | 10,011 | 1,748 | 76 | 2,669 | 0 | 0 |
| [unified](qualification/fuzz-unified-linux-x86_64.json) | 10,003 | 609 | 20 | 0 | 0 | not recorded |
| [fuzzy](qualification/fuzz-fuzzy-linux-x86_64.json) | 10,003 | 849 | 6 | 0 | 0 | not recorded |
| [matching](qualification/fuzz-matching-linux-x86_64.json) | 10,004 | 1,390 | 22 | 0 | 0 | not recorded |
| [patch](qualification/fuzz-patch-linux-x86_64.json) | 10,003 | 435 | 5 | 0 | 0 | not recorded |

The final Markdown phase rejected 332 invalid UTF-8 inputs explicitly and
reported zero resource non-results. Its 9,683 comparisons include 27 independently
selected task-neutral profiles: five full task projections were checked and
22 were source-certified unavailable, while all neutral content comparisons
remained strict. Registered reference corrections recorded two backtick, one
bare-destination and two email disagreements; the literal-admission guard
recorded 21 original-reference disagreements. Corrected comparisons are
separate from claims of agreement with an unmodified reference. All original
answers, reasons and transformation counters remain in the report.

The Markdown report is archived byte for byte from the completed campaign.
Its actual launch commit and dirty-state metadata remain unchanged, even though
subsequent commits recorded the same frozen source bytes. The final smoke gate
provides separate integration evidence; it does not rewrite these baselines.

### Final all-target smoke

The final `env -u TARGET make fuzz` gate exited zero after its real coverage
feedback test and all eight default 1,000-mutation campaigns. The
[smoke evidence index](qualification/fuzz-smoke-linux-x86_64.json) records the
actual invocation, observed launch/build identities and SHA-256 of each unedited
report; the reports retain every tool identity and transformation record.
All engine/harness/runner hashes still matched the current frozen source after
completion. These are integration observations, separate from the 10,000-case
phase baselines above and from hosted CI or packaging evidence.

| Target | Executions | Differential checks | Regions | Mismatches | Crashes |
| --- | ---: | ---: | ---: | ---: | ---: |
| [json](qualification/fuzz-smoke-json-linux-x86_64.json) | 1,021 | 0 | 3,775 | 0 | 0 |
| [markdown](qualification/fuzz-smoke-markdown-linux-x86_64.json) | 1,015 | 981 | 5,256 | 0 | 0 |
| [schema](qualification/fuzz-smoke-schema-linux-x86_64.json) | 1,016 | 0 | 4,610 | 0 | 0 |
| [pattern](qualification/fuzz-smoke-pattern-linux-x86_64.json) | 1,011 | 286 | 1,674 | 0 | 0 |
| [unified](qualification/fuzz-smoke-unified-linux-x86_64.json) | 1,003 | 0 | 607 | 0 | 0 |
| [fuzzy](qualification/fuzz-smoke-fuzzy-linux-x86_64.json) | 1,003 | 0 | 848 | 0 | 0 |
| [matching](qualification/fuzz-smoke-matching-linux-x86_64.json) | 1,004 | 0 | 1,388 | 0 | 0 |
| [patch](qualification/fuzz-smoke-patch-linux-x86_64.json) | 1,003 | 0 | 435 | 0 | 0 |

The eight smoke campaigns executed 8,076 inputs in total, including their fixed
seeds. None reported a crash or mismatch. Zero declared Markdown resource
non-results were observed; other targets retain their documented refusal rules.

The lightweight runner tests (`scripts/test-fuzz.py`) run in `make check`
via `make hygiene`, including coverage-region admission and differential normalization tests;
`scripts/test-fuzz-feedback.py`, also run by hygiene, proves that the retained corpus
grows from new coverage on the real `patch` target end to end. `make fuzz`
runs this feedback test before the campaigns. The feedback test reads the
pinned nightly channel from `rust-toolchain.toml` and sets the child process
`RUSTUP_TOOLCHAIN` explicitly; a stable parent gate therefore uses the same
registered LLVM coverage tooling, with no skips or alternate coverage path; the instrumented campaigns are a separate qualification
command. Bounded campaigns cannot establish absence of bugs, complete
recovery quality, or whole-crate coverage. Observations apply only to the
exercised budget/effort profiles, input sizes, oracles, and campaign lengths.
