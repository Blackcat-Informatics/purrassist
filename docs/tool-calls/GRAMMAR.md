<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Dialect grammar reference

Every tool-call dialect is a declarative grammar written in a small textual
DSL and compiled by `purrassist_core::grammar::Grammar::compile_governed`
into a `CompiledGrammar`. One compiled grammar serves three purposes:

- **Recognition.** The `call` and `result` productions are compiled into
  byte-level recognizer programs. The recognizer runs every dialect of a
  `DialectSet` concurrently over the output and repairs failed call attempts
  with weighted edit operators (missing, stray, and variant closers, and
  confusable spellings).
- **Printing.** The productions are retained as written, so the printer can
  run them forward: recovered calls, tool results, and tool declarations are
  printed in any dialect, and recognizing what is printed gives back the same
  calls.
- **Witnesses.** `grammar::witness::generate` drives the printer with seeded
  choices to print well-formed calls for given argument shapes, for tests and
  corpora.

The twelve built-in dialects are written in this DSL:
[`qwen3-coder-xml.v1`](../../crates/core/src/tool_call/dialects/qwen3-coder-xml.v1.grammar),
[`hermes.v1`](../../crates/core/src/tool_call/dialects/hermes.v1.grammar),
[`openai-tool-calls.v1`](../../crates/core/src/tool_call/dialects/openai-tool-calls.v1.grammar),
[`llama3-json.v1`](../../crates/core/src/tool_call/dialects/llama3-json.v1.grammar),
[`mistral-v3.v1`](../../crates/core/src/tool_call/dialects/mistral-v3.v1.grammar),
[`mistral-v11.v1`](../../crates/core/src/tool_call/dialects/mistral-v11.v1.grammar),
[`deepseek-v3.1.v1`](../../crates/core/src/tool_call/dialects/deepseek-v3.1.v1.grammar),
[`deepseek-v3.v1`](../../crates/core/src/tool_call/dialects/deepseek-v3.v1.grammar),
[`kimi-k2.v1`](../../crates/core/src/tool_call/dialects/kimi-k2.v1.grammar),
[`anthropic-tool-use.v1`](../../crates/core/src/tool_call/dialects/anthropic-tool-use.v1.grammar),
[`harmony.v1`](../../crates/core/src/tool_call/dialects/harmony.v1.grammar), and
[`bare-json.v1`](../../crates/core/src/tool_call/dialects/bare-json.v1.grammar).
A caller-compiled grammar becomes a dialect with `DialectId::custom`; see
[Add a custom dialect](#add-a-custom-dialect).

## Lexical structure

- A grammar file is a sequence of statements, each terminated by `;`.
- `#` starts a comment that runs to the end of the line. Whitespace between
  tokens is insignificant.
- Identifiers match `[A-Za-z_][A-Za-z0-9_-]*`. Productions and closer classes
  share one namespace. The keywords `grammar`, `optional-id`,
  `declaration-content`, `reasoning`, `confusable`, `closer`, `layout`,
  `until`, `slot`, `commit`, `next`, and `insert` cannot name a production or
  a closer.
- String literals are double-quoted byte strings with the escapes `\\`, `\"`,
  `\n`, `\r`, `\t`, `\xHH` (one byte), and `\u{HEX}` (one Unicode scalar
  value, encoded as UTF-8). Every literal, delimiter, spelling, and marker is
  non-empty.
- Class terminals are written between slashes, `/pattern/`; `\/` is a slash
  inside the pattern.

## Statements

| Statement | Meaning |
| --- | --- |
| `grammar "ID";` | The header, and the first statement. `ID` matches `[A-Za-z0-9._-]+`; by convention it is versioned (`name.v1`). |
| `reasoning "OPEN" "CLOSE";` | One pair of the dialect's reasoning-block markers. Several statements declare several spellings of a block (repeating a pair is `DuplicateName`). |
| `confusable "CANONICAL" "VARIANT";` | `VARIANT` is a confusable spelling of `CANONICAL` inside any literal. |
| `closer NAME = "S1" \| "S2" ...;` | A closer class: `S1` is canonical, the rest are variant spellings. One spelling is allowed. |
| `optional-id;` | The call's `id:` capture reads an id a model may add, but the format does not provide for one: a call without an id is numbered without a `tool_call.id.dedup` inference, and printing writes no id (`CompiledGrammar::call_carries_ids` is false). The built-in Hermes and Llama 3 JSON dialects declare it. A second statement is `DuplicateName`. |
| `declaration-content;` | A call whose source begins with its JSON object (`{`, no opener token before it) and whose arguments as written have the shape of a tool declaration's parameters schema (an object whose `"type"` is `"object"` and whose `"properties"` is an object) is content, not a call, unless the named tool is declared and resolves those arguments with no guess and no loss (`CallOutcome::Resolved`): a JSON Schema or tool declaration pasted as an answer is not a call to the tool it describes (`CompiledGrammar::declaration_content`). The built-in Llama 3 JSON and bare JSON dialects declare it; after Llama 3's `<\|python_tag\|>` the object is always a call. A second statement is `DuplicateName`. |
| `layout NAME "TEXT";` | Printing only: the class production `NAME` prints as `TEXT` wherever it occurs uncaptured, optional occurrences included. Recognition is unchanged. |
| `NAME = EXPR;` | A production. |

## Expressions

| Form | Meaning |
| --- | --- |
| `"bytes"` | A literal terminal. |
| `/pattern/` | A class terminal: a maximal run of bytes matching the pattern as a whole. |
| `until("D1", "D2", ...)` | A verbatim body up to, not including, the first occurrence of any delimiter, or to the end of input. |
| `slot(CODEC, "D1", ...)` | Like `until`, with the body decoded by the named argument codec. |
| `NAME` | A production (inlined) or a closer class. |
| `commit` | Zero-width: an attempt that passes it is a call (or result) attempt from here on. See [Commit points](#commit-points). |
| `next` | Zero-width, in `call` only: the next call of the same attempt begins here. See [Several calls in one attempt](#several-calls-in-one-attempt). |
| `insert("bytes")` | Zero-width: a token the model omitted, read as inserted here and recorded as the markup repair `tool_call.markup.missing-token`. It prints nothing, and the printer and witnesses never choose an alternative holding it when another can be printed. An `insert` may come before a production's first literal (`call = "<open>" body \| insert("<open>") body;`): an attempt that opens at that literal records it. |
| `KIND:item` | A capture of `item`'s span. |
| `a b` | Sequence. |
| `a \| b` | Alternation. |
| `item*`, `item+`, `item?` | Repetition: zero or more, one or more, optional. |
| `( ... )` | Grouping. |

Postfix repetition binds tightest, then capture (`KIND:item*` captures the
whole repetition), then sequence, then alternation.

### Terminals

- **Literals** match their exact bytes, or one of their confusable spellings
  (recorded as a repair).
- **Class terminals** are ECMA-262 `u`-mode patterns compiled to a byte
  automaton. The pattern is anchored and matched as a whole against a maximal
  run; it must not match the empty string and must not need backtracking
  (no backreferences or lookaround). A class terminal is how names, ids, and
  whitespace are read: `tool-name = /[A-Za-z0-9_.:-]+/;`.
- **`until`** reads a verbatim body, for example a tool result's content.
  Its delimiters also match their confusable spellings, so a body ends at an
  ASCII spelling of a fullwidth delimiter.
- **`slot`** is an `until` whose body is read by a codec (below). A slot
  holding the `json` codec is self-delimiting: the value may end before any
  delimiter, and the grammar continues after it. When two readings of one
  attempt reach the same slot at equal cost with values that begin at
  different offsets (an `insert` before a fence, say, beside the fence
  itself), the value that begins later is kept: its grammar read more of the
  text as markup instead of leaving it to the codec. Of two `until` bodies
  the one that begins earlier is kept.

### Productions

A reference to a production inlines it. Recursion, direct or indirect, is a
compile error: call framing is regular, and nested argument structure belongs
to the codecs.

## Closer classes

```text
closer call-end = "</tool_call>" | "</tool_calls>";
```

A closer class names the closing token of an element. Referencing `call-end`
in a production is a closer terminal:

- Recognition accepts every spelling. A variant spelling is recorded as
  `tool_call.markup.closer-variant` (a markup loss).
- Printing writes the canonical spelling.
- Every spelling of every closer class is known to the repair search. A
  missing closer is inserted mid-message
  (`tool_call.markup.missing-function-close` when more of the production
  follows it, `tool_call.markup.missing-wrapper-close` when it ends the
  production) or at the end of output under a non-length stop
  (`tool_call.markup.missing-closers-at-end`). A stray or repeated closer is
  deleted (`tool_call.markup.stray-closer`), and closer spellings right
  after a completed call are absorbed into it as stray closers, so they never
  leak into content.
- Only closer-class literals are inserted as missing closers. Declare every
  closing token as a closer class, even with a single spelling, when it may
  be omitted.

A literal equal to the caller's stop sequence
(`GenerationStop::StopSequence`) is reinserted at the end of output when the
stop consumed it (`tool_call.stop.consumed-closer`).

## Confusables

```text
confusable "｜" "|";
```

`VARIANT` is a confusable spelling of `CANONICAL` wherever `CANONICAL` occurs
inside a literal. For each literal, the compiler enumerates every spelling
that replaces the occurrences of any non-empty subset of the confusable pairs
present in it. Recognition reads such a spelling as the literal, recorded as
`tool_call.markup.terminal-confusable` (a markup loss); `until` and `slot`
delimiters match their confusable spellings too. Either spelling may be any
UTF-8 text: a class run reads whole characters and ends before the first
character it cannot consume, so a multi-byte variant right after a class
terminal (`"` written as `”` after a name) is read like an ASCII one,
from its first byte. Printing writes the
canonical spelling. Each spelling adds recognizer configurations
(`CompiledGrammar::configuration_bound`), and so per-byte work; declare only
the confusions a model actually makes. A confusable or closer-variant
spelling is read even when another literal the grammar expects at the same
place shares its first bytes and is still being matched when the spelling
departs from its canonical form (Kimi-K2's singular
`<|tool_call_section_end|>` beside `<|tool_call_begin|>`): the failed literal
stays a respelling seed until the sibling fails too. One canonical text may have several
variants: the built-in DeepSeek-V3.1 and V3 dialects read `｜` as `|`, and
`▁` as `_` or a space, the renderings detokenizers produce for their special
tokens; a canonical text may also be longer than one character (Kimi-K2's
`confusable "calls_section" "call_section"`).

## Layout

```text
ws = /[ \t\r\n]+/;
layout ws "\n";
```

`layout` changes printing only. It must name a production whose expression is
a class terminal (directly or through references to such a production), and
`TEXT` must be a member of that class. Every uncaptured occurrence of the
production, including optional ones, prints as `TEXT`; without a layout an
optional class occurrence is omitted when printing. Naming a closer class,
naming a production that is not a class, giving text the class does not match,
or a second `layout` for one production is a compile error. The built-in
Qwen3-Coder XML and Hermes grammars use `layout` so that printed calls match
the chat templates' newline layout byte for byte.

## Commit points

A `call` or `result` attempt normally *opens* when its opener literal
completes: from then on the turn holds its bytes, and an attempt that no
repair completes is a rejected attempt (finish `Unresolved`). That is right
for a distinctive opener such as `<tool_call>`, and wrong for a format with
none: a Llama 3 JSON call is a plain `{"name": ..., "parameters": ...}`
object, and an opener of `{` would turn every JSON object in prose into a
rejected attempt.

`commit` moves the opening point. In a program that contains `commit`, an
attempt opens only when one of its configurations reaches a location that
every path from the start passes a `commit` to reach, or when it completes:

```text
call = "{" ws? "\"name\"" ws? ":" ws? "\"" name:tool-name "\"" ws? "," ws?
       "\"parameters\"" commit ws? ":" arguments:slot(json, "\x00") ws? call-end;
```

- Before it commits, an attempt that fails is silent: no rejection is
  reported and its bytes stay content. A length or other stop before the
  commit point leaves the bytes as content too. Repairs are limited as
  before an opener completes (closer insertion and stray-closer deletion
  need an opened attempt).
- After it commits, the attempt is a call attempt like any other: repaired,
  rejected, or truncated, from its opener's start.
- Reports at or after the start of an uncommitted attempt (other dialects'
  calls, reasoning markers) are held until it commits or ends, so the
  result never depends on how the output is chunked. A committed attempt
  takes the bytes from its start, so a call of another dialect inside it
  is not read; a call inside an attempt that never commits is.
- `commit` prints nothing. Several commits on one path are one.
- An alternative that already starts with a distinctive token commits
  right after it (`"<|python_tag|>" commit ...`), so whatever follows the
  token is a call attempt.

Choose the commit point at the first evidence that the text is a call and
not data, and keep every token before it free of repairs the format needs.

## Several calls in one attempt

Some formats write several calls behind one opener: Mistral v3 writes
`[TOOL_CALLS]` once, then a JSON array with one object per call. The calls
after the first have no opener of their own. `next` marks where the next
call of the same attempt begins:

```text
element = "{" ... object-end;
call = "[TOOL_CALLS]" ws? "[" ws? element (ws? "," ws? next element)* ws? array-end;
```

- Each part of the attempt is its own call: the first spans from the
  opener to the first `next`, each later one from its `next` to the next,
  the last to the end of the attempt. Separators placed before `next`
  belong to the call before it; the closing framing belongs to the last
  call. Each call has its own captures, edits, closure and cost.
- A cut-off attempt (a length stop) is dangling in its last call only: the
  calls before it are whole. Closers inserted at the end of output belong
  to the last call.
- Equal-cost readings that begin a call at different offsets are competing
  segmentations of the attempt, so the turn's ambiguity rules apply.
- A slot's codec learns the declared tool from the `name:` capture of its
  own call, never from an earlier call of the attempt.
- Printing: a production with `next` prints every emitted call in one
  instance (a repetition holding `next` repeats once per further call), and
  a witness prints all of its calls in one instance.
- `next` may not appear inside a capture, or in a production reachable from
  `result` or `declaration` (`GrammarError::MisplacedNext`).
- An attempt reads every call it can. Its completions are held until the
  attempt ends; a completion is dropped when a later completion of the same
  attempt reads more calls and reads its own calls no worse (the summed
  cost of those calls is no higher), whatever repair the further calls
  need, or when a length stop's cut-off reading reaches past it with more
  calls, the last of which began its arguments (the model was still
  writing). So a production that may end after any call (`element (sep next
  element)*` with nothing after it) ends after the last call-shaped part,
  never earlier, and text after it is content; text that cannot be read as
  a call is never absorbed. A cut-off reading is dropped in favour of a
  completion that reaches as far or reads as many calls no worse, and of
  any completion when its last call never began its arguments.
- A form that omits the list's framing can say so with `insert`, recorded
  as a markup repair:

  ```text
  call = "[TOOL_CALLS]" ws? ("[" ws? elements ws? array-end
                            | insert("[") element (sep next element)* insert("]"));
  ```

## Reasoning markers

`reasoning "<think>" "</think>";` declares the dialect's reasoning block.
A grammar may declare several marker pairs, one per spelling of a block (the
built-in Anthropic dialect declares its `thinking` and `redacted_thinking`
blocks, compact and spaced); `CompiledGrammar::reasoning_markers` lists them.
A `DialectSet` combines the markers of all its dialects. Under
`ReasoningPolicy::Separate` (the default) the markers delimit reasoning
blocks; a call inside a closed block stays reasoning text and is offered only
as a guess-flagged alternative; an unclosed block containing a call opener is
closed at the opener by a guess; `starts_open: true` begins the turn inside a
block. Under `ReasoningPolicy::AsContent` the markers are plain content.

## Argument codecs

| Codec | Value |
| --- | --- |
| `raw` | The verbatim body as one UTF-8 string leaf. Malformed UTF-8 rejects the attempt, except a character cut off by a length stop. It records no edits. A `raw` value is always a string: a parameter declared as an integer, number, boolean, array, or object read through `raw` fails validation (`UnresolvedReason::SchemaValidation`), because no rule coerces a string into a typed value. |
| `json` | One JSON value read by the push-based tolerant JSON parser: exact number spellings, each repair recorded with its `json.syntax.*` rule. Self-delimiting. A whole arguments value that is a JSON string holding JSON is decoded as stringified arguments and flagged (`tool_call.arguments.decode-string`). Under a length stop, a value that is already complete at the cut (only whitespace after it) ends there, as at a delimiter, when the production completes from the slot with no further input: a call that ends with its arguments value is then complete, not cut off. |
| `json-args` | The `json` codec for an arguments value: the value must be an object, or a string that is empty or whose text begins with `{` after spaces (stringified arguments). Any other value (a number, an array, `null`, prose in a string) faults at its first byte, rejecting a committed attempt and leaving an uncommitted one as content, so a grammar whose only call evidence is the object itself (`bare-json.v1`) never reads `{"name": "pi", "arguments": 3}` as a call. |
| `xml-params` | A sequence of `<parameter=NAME>value</parameter>` elements, typed by the declared tool the call names: verbatim strings with one framing newline consumed on each side, typed values parsed as JSON, nested parameter elements for object parameters, a whole-JSON body, and the `</parameters>` variant. See [the Qwen3-Coder XML dialect](dialects/qwen3-coder-xml.v1.md) for every reading. |

A codec may depend on the call's tool. When the recognizer is given the tool
set and the grammar captured the call's `name:` before the slot, the codec
receives the argument shape of the declared tool that name refers to (exact
names and aliases). Without a shape every codec still decodes, without type
information.

Choose slot delimiters so that a missing closer is noticed at the next token:
list the element's own closer spellings, the openers of sibling elements, and
the enclosing call's closer spellings. A value that runs into a sibling's
opener is then ended there by a repair rather than swallowing the rest of the
call.

## Captures

| Capture | Records |
| --- | --- |
| `name:` | The tool name. When printing, an alternation never chooses an alternative whose first `name:` capture is a class terminal that cannot spell the call's name (the built-in Harmony grammar reads `browser.search` and `python` recipients through their own classes). |
| `id:` | The call id (printed from the id policy's assignment). An `id:` capture whose expression holds a `name:` capture is a *composite id* (Kimi-K2's `functions.NAME:IDX`): recognition takes the whole span as the call's id and the inner capture as its name; printing writes the expression with the call's name, and each uncaptured class terminal inside it as the call's position in the printed instance (0, 1, …) when the class accepts that, so the printed ids are distinct. |
| `arguments:` | The whole argument value; must wrap a `slot(...)`. |
| `param-name:` | One parameter's name. |
| `param-value:` | One parameter's value; must wrap a `slot(...)`. |
| `reasoning:` | Reasoning text inside a production. |
| `content:` | Free content, for example a tool result's body. |
| `dropped:` | Text inside a call that is not part of it: a code-fence marker is recorded as `tool_call.extract.fence` (markup), any other text as `tool_call.extract.prose` (data). Dropped text is never printed: a repetition holding it prints its minimum, and the printer and witnesses never choose an alternative that always holds it when another can be printed (as with `insert`). |

`param-name:` and `param-value:` captures come in adjacent pairs, in that
order, in one sequence. A production uses either one `arguments` slot or
parameter pairs, never both.

## Required productions

| Production | Role |
| --- | --- |
| `call` | One tool call. Recognized in model output. |
| `result` | One tool result as the model would see it. Recognized in model output (a result the model writes is content) and printed by `emit`. |
| `declaration` | One tool declaration as a chat template lists it. Printing only. |

`call` and `result` must begin with a literal opener (that is how the
recognizer knows where an attempt may start) and must not match the empty
string. Text outside every call and result is content.

## Compile errors

Every error except a missing production and a resource stop carries the byte
offset in the grammar source (`GrammarError::offset`).

| Error | Cause |
| --- | --- |
| `Syntax` | Malformed DSL: an unexpected character, a missing `;`, a bad escape, a missing or duplicated header. |
| `EmptyLiteral` | A literal, delimiter, spelling, or marker is the empty string. |
| `ReservedName` | A keyword used as a production or closer name. |
| `DuplicateName` | A name defined twice, a `reasoning` statement repeating an earlier marker pair, or a second `layout` for one production. |
| `UnknownName` | A reference to an undefined production or closer. |
| `Recursive` | A production that refers to itself, directly or indirectly. |
| `UnknownCapture` | A capture kind that is not one of the eight above. |
| `UnknownCodec` | A slot naming a codec other than `raw`, `json`, `json-args`, or `xml-params`. |
| `InvalidClass` | A class pattern that is malformed, needs backtracking, or matches the empty string. |
| `MissingProduction` | `call`, `result`, or `declaration` is missing. |
| `InvalidOpener` | `call` or `result` does not begin with a literal opener, or matches the empty string. |
| `CaptureTarget` | An `arguments:` or `param-value:` capture that does not wrap a slot. |
| `UnpairedParameter` | A `param-name`/`param-value` capture outside an adjacent pair in one sequence. |
| `ConflictingArguments` | A production with both an `arguments` slot and parameter pairs. |
| `MisplacedNext` | A `next` inside a capture, or reachable from `result` or `declaration`. |
| `InvalidLayout` | A `layout` naming a closer or a production that is not a class, or text the class does not match. |
| `Resource` | The governor's budget was exhausted, or the caller cancelled. |

Compilation charges `InputBytes` for the source, `Work` per source byte,
syntax node, instruction, and compiled table entry, `Depth` per nesting
level, and `ScratchBytes` for every retained table. A built-in grammar is
compiled once, on first use, under its own default budget, never charged to a
caller. The per-byte recognition cost of a dialect is `K_THREAD` times its
`configuration_bound()`; the published bound and the built-in values are in
[RESOURCES.md](../RESOURCES.md#tool-call-recovery-cost-bound).

## Add a custom dialect

This walkthrough defines a dialect for a model that writes each call between
two special tokens spelled with fullwidth vertical bars, with one XML-style
element per argument:

```text
<｜call｜>read_file#r1
<arg name="path">src/lib.rs</arg>
<｜end｜>
```

### 1. Write the grammar

<!-- custom-dialect-grammar -->
```text
grammar "acme-call.v1";

reasoning "<think>" "</think>";

# The call markers are special tokens spelled with fullwidth vertical bars;
# a model that writes an ASCII bar has made a confusable slip.
confusable "｜" "|";

# Every closing token is a closer class, so a missing, repeated or variant
# closer is repaired rather than rejected.
closer call-end = "<｜end｜>" | "<｜end_call｜>";
closer arg-end = "</arg>" | "</args>";

tool-name = /[A-Za-z0-9_.:-]+/;
call-id = /[A-Za-z0-9_-]+/;
ws = /[ \t\r\n]+/;
# Printing: whitespace between elements is one newline.
layout ws "\n";

# A value ends at its closer, at the next argument's opener, or at the call's
# closer, so a missing </arg> is noticed at the following token.
arg = "<arg name=\"" param-name:tool-name "\">"
      param-value:slot(raw, "</arg>", "</args>", "<arg name=", "<｜end｜>", "<｜end_call｜>")
      arg-end;

call = "<｜call｜>" name:tool-name ("#" id:call-id)? ws? (arg ws?)* call-end;

result = "<｜result｜>" content:until("<｜end｜>") "<｜end｜>";

declaration = "<tool name=\"" name:tool-name "\">" ws?
              ("<description>" content:until("</description>") "</description>" ws?)?
              "<schema>" arguments:slot(raw, "</schema>") "</schema>" ws? "</tool>";
```

Every argument is read by the `raw` codec, so this dialect carries string
parameters. For typed parameters, read each value with
`param-value:slot(json, ...)` or the whole body with
`arguments:slot(json, ...)` or `arguments:slot(xml-params, ...)`.

### 2. Compile it and recover with it

```rust
use purrassist_core::grammar::Grammar;
use purrassist_core::tool_call::{
    DialectId, DialectSet, Effects, Emission, EmitTarget, IdPolicy, Param, ToolDefinition,
    ToolRecoveryRequest, ToolSet,
};
use purrassist_core::{Budget, Effort, Governor, Outcome};

fn recover(grammar_source: &str, output: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut governor = Governor::new(Budget::default());
    // A GrammarError carries the byte offset of the problem.
    let acme = DialectId::custom(Grammar::compile_governed(grammar_source, &mut governor)?);
    let dialects = DialectSet::new([acme.clone()], &mut governor)?;
    let tools = ToolSet::compile(
        [ToolDefinition::new("read_file")
            .effects(Effects::ReadOnly)
            .param("path", Param::string().required())],
        Budget::default(),
    )?;
    let request = ToolRecoveryRequest::new(Effort::Standard)
        .dialects(dialects)
        .ids(IdPolicy::Source)
        .emit(EmitTarget::Dialect(acme));
    let result = tools.recover(output, &request);
    if let Outcome::Resolved(calls) = &result.outcome {
        for call in calls.usable() {
            println!("{} {:?} {}", call.id, call.tool, call.arguments.to_json());
        }
        if let Some(Emission::Dialect { emitted, .. }) = &calls.emitted {
            println!("{}", String::from_utf8_lossy(&emitted.output));
        }
    }
    Ok(())
}
```

A custom grammar can be combined with the built-in dialects in one
`DialectSet`; a set is ordered canonically by `(id, digest)`, so the order of
the allow-list never changes a result. Custom dialects are a Rust API: the
CLI and MCP surfaces accept the built-in dialect ids.

### 3. What recovery does with it

Given this output, written with ASCII bars and a variant `</args>` closer:

<!-- custom-dialect-output -->
```text
Let me read it.
<|call|>read_file#r1
<arg name="path">src/lib.rs</args>
<|end|>
```

the turn is `Resolved`. The text before the call is content. The call is
`read_file` with id `r1` (the `Source` id policy keeps it) and arguments
`{"path":"src/lib.rs"}`, `Resolved` even under `Structural`, with three
itemized markup repairs: two `tool_call.markup.terminal-confusable` (the
ASCII-barred opener and closer) and one `tool_call.markup.closer-variant`
(`</args>`). Printing the call back in the same dialect gives the canonical
form, which recognizes with no repair at all:

<!-- custom-dialect-emitted -->
```text
<｜call｜>read_file#r1
<arg name="path">src/lib.rs</arg>
<｜end｜>
```

The `grammar_doc_example_compiles` test in
`crates/core/tests/tool_call_rules.rs` reads the three blocks above from this
file, compiles the grammar, recovers the output, and checks every statement
of this paragraph, so the walkthrough cannot drift from the engine.

### 4. Checklist

- Give the grammar a versioned id; two dialects are equal when their ids and
  source digests are equal.
- Start `call` and `result` with a distinctive literal opener. Content may not
  contain a complete opener: every complete opener starts a call attempt.
  A format without one opens at its first byte and places a `commit` at its
  first call-shaped evidence ([Commit points](#commit-points)).
- Declare every closing token as a closer class, and list the variant
  spellings models actually produce.
- List sibling openers and enclosing closers among slot delimiters.
- Use `layout` so printed calls match the chat template byte for byte.
- Check printing round trips: `grammar::witness::generate` prints seeded
  well-formed calls for the tools' argument shapes, and recognizing them must
  give back the same calls with no repair.
