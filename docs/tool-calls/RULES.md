<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Tool-call rule registry

Every repair, reading, and record that tool-call recovery can itemize on a
call is one named rule of `purrassist_core::tool_call::rules::RULES`. A
recovered call's `repairs` ledger (and the receipt) names these ids in its
evidence, losses, and inferences. This page lists every registry entry. The
`rules_doc_matches_registry` test in `crates/core/tests/tool_call_rules.rs`
parses the tables below and requires them to list exactly the registry's
entries, in registry order, with the same fields; the tables cannot drift from
the code.

## Columns

- **Cost**: `Rule::cost`, the ranking cost of one application: the base cost
  times the grounding multiplier (1 for `definition`, 2 for `syntax`), so a
  repair grounded in the tool definition beats a syntax-only repair of equal
  base cost. Readings of a call are ranked by the sum of their rules' costs.
  `tool_call.name.match`, `json.key.match`, `tool_call.value.enum-normalize`
  and `json.enum.match` charge the shared approximate-name cost per
  application (2 for equal comparison keys, 3 without punctuation and
  separators, 5 plus the edit distance otherwise); their rows list the
  minimum.
- **Guess**: whether an application is recorded as a guess inference. A
  guess makes a call at most `BestGuess`.
- **Readings**: `single` when the repair leaves no other reading of the input,
  `multiple` when other readings remain possible.
- **Grounding**: what the decision rests on: the tool `definition`, or the
  dialect grammar alone (`syntax`).
- **Loss**: the loss kind an application records. `markup` losses (closers,
  framing, fence markers) are itemized and never demote a call; `data` losses
  make the call at most `BestGuess`.
- **Policies**: the `RepairPolicy` values that admit the rule. `Structural`
  admits exactly the single-reading rules; `Tolerant` admits every rule. A
  reading that needs a rule its tool's policy does not admit is `Unresolved`
  (`UnresolvedReason::RuleNotAdmitted`).
- **`Exact`**: `refused` for a repair (`Rule::is_repair`: the rule costs
  something or guesses). Under `Effort::Exact` a call that needs a refused
  rule is `Unresolved` with `UnresolvedReason::RepairNotPermitted` naming it.
  `applied` rules are canonical parsing or records of facts about the input,
  and apply under every effort.
- **Source**: the grammar `EditOperator` the rule is (engine, reasoning,
  context, and codec rules, in `EditOperator` order), or `resolution` for a
  rule applied while resolving a call against its tool.

## Registry

| Rule id | Cost | Guess | Readings | Grounding | Loss | Policies | `Exact` | Source | Meaning |
| --- | ---: | --- | --- | --- | --- | --- | --- | --- | --- |
| `tool_call.markup.missing-function-close` | 4 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `MissingFunctionClose` | A missing inner closer inserted mid-message, where more of the production follows it (for example `</function>` before `</tool_call>`). |
| `tool_call.markup.missing-wrapper-close` | 4 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `MissingWrapperClose` | A missing closer that ends the production, inserted where another call's opener or the end of output follows the call. |
| `tool_call.markup.missing-closers-at-end` | 4 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `MissingClosersAtEnd` | Closers inserted at the end of output under a non-length stop; usable only when the call is otherwise complete (name, every value closed, required parameters present). |
| `tool_call.markup.stray-closer` | 6 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `DeleteStrayCloser` | A stray or repeated closer deleted, including closer spellings absorbed right after a completed call. |
| `tool_call.markup.closer-variant` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `CloserVariant` | A declared variant spelling of a closer class read as its canonical spelling (`</functions>` as `</function>`). |
| `tool_call.markup.terminal-confusable` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `TerminalConfusable` | A declared confusable spelling of a literal read as the literal. |
| `tool_call.stop.consumed-closer` | 2 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `ConsumedCloser` | The closer that the caller's stop sequence consumed reinserted at the end of output (the stop sequence equals the expected closer). |
| `tool_call.reasoning.unclosed` | 8 | yes | multiple | syntax | none | `Tolerant` | refused | `ReasoningUnclosed` | An unclosed reasoning block closed at the first call opener inside it; later calls depend on this guess. |
| `tool_call.reasoning.embedded-call` | 0 | yes | multiple | syntax | none | `Tolerant` | refused | `ReasoningEmbeddedCall` | A call written inside a closed reasoning block, offered only as an alternative in `ToolCalls::embedded`. |
| `tool_call.reasoning.unterminated` | 0 | no | single | syntax | data | `Structural`, `Tolerant` | applied | `ReasoningUnterminated` | A reasoning block with no closer and no call opener, running to the end of output. |
| `tool_call.markup.missing-parameter-close` | 2 | no | single | definition | none | `Structural`, `Tolerant` | refused | `MissingParameterClose` | A parameter value ended at the opener of a parameter the call's tool declares; the opener is always a boundary. |
| `tool_call.markup.parameter-closed-by-element` | 4 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `ParameterClosedByElement` | A parameter value ended by the end of its function element. |
| `tool_call.markup.undeclared-parameter-boundary` | 4 | yes | multiple | syntax | none | `Tolerant` | refused | `UndeclaredParameterBoundary` | A parameter value ended at a parameter opener when the call names no declared tool; the opener may be text of the value. |
| `tool_call.markup.closers-only-empty` | 3 | no | single | definition | markup | `Structural`, `Tolerant` | refused | `ClosersOnlyEmpty` | A function body holding only closers read as no arguments. |
| `tool_call.markup.nested-parameter` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | `NestedParameter` | An object parameter written as nested parameter elements instead of inline JSON. |
| `tool_call.markup.json-body` | 0 | no | single | syntax | none | `Structural`, `Tolerant` | applied | `JsonBody` | A function body that is one whole JSON object read as the arguments. |
| `tool_call.xml.array-item` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | `ArrayItem` | A value of an array parameter that is not a JSON array read as one item; repeated elements collect. |
| `tool_call.value.trim-nonverbatim` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | `TrimNonverbatim` | Whitespace beyond the framing newlines trimmed around a typed or enumeration value. |
| `tool_call.xml.framing-newline` | 0 | no | single | syntax | markup | `Structural`, `Tolerant` | applied | `FramingNewline` | One framing newline consumed on each side of an XML parameter value. |
| `tool_call.arguments.decode-string` | 0 | no | single | syntax | none | `Structural`, `Tolerant` | applied | `StringifiedArguments` | Stringified JSON arguments decoded into the arguments object and flagged. |
| `tool_call.extract.prose` | 4 | no | single | syntax | data | `Structural`, `Tolerant` | refused | `ExtractProse` | Text inside a call's markup that is not a fence marker dropped (a `dropped:` capture). |
| `tool_call.extract.fence` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `ExtractFence` | A code-fence marker inside a call's markup dropped (a `dropped:` capture). |
| `tool_call.context.fenced-example` | 0 | yes | multiple | syntax | none | `Tolerant` | refused | `ContextFencedExample` | A call recognized inside fenced code, an inline code span, or a block quote: it may be an example. |
| `json.syntax.comment` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `JsonComment` | A `//` or `/* */` comment in a JSON value dropped. |
| `json.syntax.missing-value` | 8 | yes | multiple | syntax | none | `Tolerant` | refused | `JsonMissingValue` | `null` inserted where a JSON value is missing. |
| `json.syntax.literal-case` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `JsonLiteralCase` | A literal respelled (`None`, `NULL`, `True`, `FALSE` and similar). |
| `json.syntax.bare-string` | 8 | yes | multiple | syntax | none | `Tolerant` | refused | `JsonBareString` | A bare token that is neither a literal nor a number quoted as a string. |
| `json.syntax.bare-key` | 2 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `JsonBareKey` | A bare object key quoted. |
| `json.syntax.string` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `JsonString` | A single-quoted, curly-quoted, or otherwise non-canonically quoted string requoted. |
| `json.syntax.unterminated-string` | 8 | yes | multiple | syntax | none | `Tolerant` | refused | `JsonUnterminatedString` | An unterminated string closed. |
| `json.syntax.literal-escape` | 8 | yes | multiple | syntax | none | `Tolerant` | refused | `JsonLiteralEscape` | An invalid escape kept as literal text. |
| `json.syntax.missing-colon` | 2 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `JsonMissingColon` | A missing `:` inserted after a key. |
| `json.syntax.missing-comma` | 2 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `JsonMissingComma` | A missing `,` inserted between elements. |
| `json.syntax.trailing-comma` | 2 | no | single | syntax | markup | `Structural`, `Tolerant` | refused | `JsonTrailingComma` | A comma before a closing bracket or brace dropped. |
| `json.syntax.close-container` | 2 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `JsonCloseContainer` | A container left open at the end of the value closed. |
| `tool_call.markup.missing-wrapper-close.before-text` | 4 | yes | multiple | syntax | none | `Tolerant` | refused | `MissingWrapperCloseBeforeText` | A missing wrapper closer inserted before text that no closer follows up to the next opener or the end of output; the text may instead be inside a wrapper that never closes. |
| `tool_call.markup.missing-token` | 4 | no | single | syntax | none | `Structural`, `Tolerant` | refused | `MissingToken` | A token the dialect's grammar names as omissible (`insert("...")`), read as inserted where the model left it out (the brackets of a Mistral v3 call list written without them). |
| `tool_call.name.alias` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | resolution | A tool named by an explicit alias. |
| `tool_call.name.match` | 2 | yes | multiple | definition | none | `Tolerant` | refused | resolution | A near-miss tool name accepted by approximate name matching (cost varies with the names compared; minimum listed). |
| `tool_call.name.from-arguments` | 8 | yes | multiple | definition | none | `Tolerant` | refused | resolution | A call that names no tool (a Kimi-K2 bare call counter, a Hermes object without `name`) read as the declared tool its arguments fit: every required parameter present and every member a declared parameter or alias; several fitting tools make the call `Ambiguous`, none leave it `Unresolved` (missing name). |
| `tool_call.parameter.alias` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | resolution | A parameter named by an explicit alias. |
| `json.key.match` | 2 | yes | multiple | definition | none | `Tolerant` | refused | resolution | A near-miss parameter name renamed to a declared one (cost varies with the names compared; minimum listed). |
| `json.key.relocate` | 12 | yes | multiple | definition | none | `Tolerant` | refused | resolution | A missing required parameter moved out of a nested object. |
| `tool_call.parameter.drop-unknown` | 50 | no | single | definition | data | `Structural`, `Tolerant` | refused | resolution | An undeclared parameter dropped under `UnknownParameters::DropWithLoss`. |
| `tool_call.parameter.pass-through` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | resolution | An undeclared parameter kept under `UnknownParameters::PassThrough`. |
| `tool_call.parameter.repeated` | 0 | yes | multiple | definition | data | `Tolerant` | refused | resolution | One occurrence of a repeated non-repeatable parameter kept (first or last; both readings compete). |
| `tool_call.parameter.repeated-choice` | 0 | yes | multiple | definition | none | `Tolerant` | refused | resolution | Which occurrence of a repeated non-repeatable parameter the selected reading keeps, and why: on evidence (the other readings are blocked or cost more), or as the first-ranked reading of an `Ambiguous` call, citing the alternatives. |
| `tool_call.parameter.collect` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | resolution | The occurrences of a repeatable parameter collected into an array. |
| `tool_call.value.enum-normalize` | 2 | no | single | definition | none | `Structural`, `Tolerant` | refused | resolution | An enumeration value equal to exactly one literal after case folding and whitespace normalization (cost varies with the names compared; minimum listed). |
| `json.enum.match` | 2 | yes | multiple | definition | none | `Tolerant` | refused | resolution | An enumeration value matched approximately (cost varies with the names compared; minimum listed). |
| `tool_call.value.decode-typed` | 0 | no | single | definition | none | `Structural`, `Tolerant` | applied | resolution | XML text of a typed parameter read by its declared type once name resolution identified the tool. |
| `tool_call.truncated` | 0 | no | single | syntax | data | `Structural`, `Tolerant` | applied | resolution | A call cut off by a length stop. |
| `tool_call.markup.dangling-opener` | 0 | no | single | syntax | data | `Structural`, `Tolerant` | applied | resolution | A call attempt still open at the end of output under a non-length stop. |
| `tool_call.id.dedup` | 0 | no | single | syntax | none | `Structural`, `Tolerant` | applied | resolution | A missing or duplicate source call id renumbered under `IdPolicy::Source`. |
| `tool_call.input.invalid-utf8` | 0 | no | single | syntax | data | `Structural`, `Tolerant` | applied | resolution | A maximal run of bytes that are not well-formed UTF-8; the bytes stay retained. |
| `tool_call.markup.unparsed-dialect` | 0 | no | single | syntax | none | `Structural`, `Tolerant` | applied | resolution | Call markup of a family no allowed dialect reads (not built in yet, or a built-in dialect the allow-list leaves out), reported as a rejected attempt; the bytes stay content. |

## Disabled rules

`rules::DISABLED` lists the schema-recovery rules that tool-call recovery never
applies. None of them is in the registry, so no recovered call can record one.

| Rule id | Why it is never applied |
| --- | --- |
| `json.required.invent` | A missing required argument is never invented: the call is `Unresolved` with `UnresolvedReason::MissingRequired`. |
| `json.type.stringify-scalar` | A scalar is never stringified to satisfy a string parameter: the call fails validation (`UnresolvedReason::SchemaValidation`). |
| `json.type.decode-string` | A JSON string is never decoded into a value to satisfy a typed parameter: the call fails validation. Stringified *whole arguments* are a separate, applied rule (`tool_call.arguments.decode-string`). |

No rule synthesizes a parameter name from positional text, accepts trailing
free text as an argument, or invents a value.

## Where the rules apply

- Engine markup repairs (the first seven rows and
  `tool_call.markup.missing-wrapper-close.before-text`) come from the
  grammar engine's best-first repair search over a call attempt; see
  [GRAMMAR.md](GRAMMAR.md) for closer classes and confusables.
- Reasoning rules come from the turn layer under `ReasoningPolicy::Separate`.
- `tool_call.extract.*` come from a grammar's `dropped:` captures, and
  `tool_call.context.fenced-example` from the context tracker.
- The `xml-params`, `json`, and stringified-arguments rows are recorded by
  argument codecs; the `json.syntax.*` rows are the tolerant JSON parser's
  repairs inside an argument value.
- `resolution` rows are applied by `resolve_forest` against the tool set,
  except `tool_call.id.dedup` (recorded by the id policy) and
  `tool_call.input.invalid-utf8` (recorded by the recovery operation's
  encoding scan) and `tool_call.markup.unparsed-dialect` (recorded by the
  recovery operation's unparsed-markup detector on the rejected attempt it
  reports).

How often each rule is exercised, and with what result, is in the tool-call
section of [MATCHING-QUALITY.md](../MATCHING-QUALITY.md).
