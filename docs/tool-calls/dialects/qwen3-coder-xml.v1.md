<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `qwen3-coder-xml.v1`

Grammar: [`crates/core/src/tool_call/dialects/qwen3-coder-xml.v1.grammar`](../../../crates/core/src/tool_call/dialects/qwen3-coder-xml.v1.grammar),
built in as `DialectId::Qwen3CoderXml`. The grammar and its fixtures are
first-party; no chat-template text is copied into this repository.

## Public sources

- Qwen3-Coder model repository, chat template `chat_template.jinja`:
  <https://huggingface.co/Qwen/Qwen3-Coder-480B-A35B-Instruct/blob/9d90cf8fca1bf7b7acca42d3fc9ae694a2194069/chat_template.jinja>
  (revision `9d90cf8fca1bf7b7acca42d3fc9ae694a2194069`, last modified
  2025-08-21). The template documents the call markup an assistant turn
  carries, how a tool message is rendered back to the model, and how the
  system prompt lists tools.
- Qwen3 model repository, chat template in `tokenizer_config.json`:
  <https://huggingface.co/Qwen/Qwen3-8B/blob/b968826d9c46dd6066d109eabc6255188de91218/tokenizer_config.json>
  (revision `b968826d9c46dd6066d109eabc6255188de91218`, 2025-07-26), for the
  `<think>`/`</think>` reasoning markers of the Qwen3 family.

## What the grammar accepts

- `call`: `<tool_call>`, optional whitespace, `<function=NAME>`, a body read
  by the `xml-params` codec, `</function>`, optional whitespace,
  `</tool_call>`. Each call has its own wrapper; calls follow one another
  with any content (usually a newline) between them.
- The bare `<function=NAME>...</function>` element, without the wrapper:
  `<tool_call>` and `</tool_call>` are special tokens that detokenizers
  commonly strip, so the missing wrapper tokens are read as inserted, each
  recorded as `tool_call.markup.missing-token` (a markup repair: a call that
  needs nothing else is `Resolved`). A `</tool_call>` after the bare element
  is read as its closer. Each bare element is its own call, and the attempt
  opens at `<function=`: an element that does not parse (no `>`, an empty
  name, text between parameters, a nested `<function=`) is a rejected
  attempt, never silent content. Inside a wrapper the wrapped reading
  covers the element.
- The body (the `xml-params` codec, typed by the declared tool `NAME`):
  `<parameter=P>` elements. A string, enumeration or unmodeled parameter is
  verbatim text with exactly one framing newline consumed on each side
  (recorded as `tool_call.xml.framing-newline`, a markup loss that is not a
  repair). Integer, number, boolean, array and object parameters are parsed
  as JSON with their exact spellings. An object parameter may hold nested
  `<parameter=...>` elements instead of inline JSON; both give the same
  tree. An array parameter whose items are objects (or unmodeled, such as a
  `$ref` or `anyOf` item schema) may be written as one `<parameter=P>` element
  per item, each holding nested `<parameter=...>` elements; every repetition
  is one object item (`tool_call.markup.nested-parameter` and
  `tool_call.xml.array-item`), the items collect in source order
  (`tool_call.parameter.collect`), and they mix freely with items written as
  inline JSON. *Proved by* `tool_call_goldens::xml_array_nested_object_items`
  and its siblings (`..._in_object`, `..._mixed`, `..._item_unclosed`,
  `..._item_unclosed_before_sibling`, `xml_array_unmodeled_items_nested`).
  This needs the tool to be known when the body is read: with no declared tool
  there is no scope to open, so the repetitions are not nested objects.
  An array of strings never nests: nested tags stay verbatim text of the item
  (`xml_array_string_item_keeps_nested_tags`). An item's members are
  declared only in the item's scope, so a member whose name is the array's
  own name is the item's member (a child), never the next item
  (`xml_array_nested_object_same_name_member`).
  *Scope graph.* A `$ref`, `anyOf`, `oneOf` or `allOf` item or parameter is
  unmodeled by the typed shape, but the tool's exact schema still declares
  its member names, and the codec uses them: the tool's schema is compiled
  once into a scope graph with one node per distinct schema location (a
  `$defs` or `definitions` entry, an inline object, or the union of the
  object branches of an `anyOf`/`oneOf`/`allOf`), each node holding its
  member names and, per member, whether its value nests as an object or as an
  array of objects. A recursive `$ref` is a back-edge to its own node, never
  an unrolling. So a recursive `$ref` item nests exactly like its inline
  twin (`xml_array_recursive_ref_items_nest`, balanced input needing no
  repair; `xml_array_ref_items_repeated_member_collects`), and a `$ref` item
  member that shares the name of an outer parameter stays inside its item
  (`xml_array_ref_items_member_shares_outer_name`,
  `xml_array_ref_items_keep_path_not_hijacked`,
  `xml_array_ref_items_outer_array_collision`, and for a single `$ref`
  object parameter `xml_object_ref_param_member_shares_outer_name`). Under a
  union scope a member nests when its body begins with `<parameter=` and any
  object branch declares it an object or an array of objects
  (`xml_array_anyof_items_union_nest`). The graph names scopes only: typing
  stays with the tool's exact schema (`tool_call.value.schema-typed`), except
  that a member whose every non-null branch is an array reads text that is
  not a JSON array as one item and collects repeated occurrences.
  *Opaque scopes.* A scope whose schema names no member (a free-form object
  item such as `{"type":"object","additionalProperties":true}`, or a `$ref`
  to a value that is not an object) has unknown membership. If its open
  nested element's opener names a parameter an enclosing scope declares, the
  codec closes the scope there as a guess,
  `tool_call.markup.unmodeled-scope-boundary` (not the definition-grounded
  `tool_call.markup.missing-parameter-close`: the opener may be the scope's
  own member). The call is a best guess, never usable on a mutating tool
  (`xml_unmodeled_opaque_scope_boundary`: `rm` with `keep` of free-form
  objects does not run with the item's `path` as its own path). The scope
  graph resolves local `$ref` pointers only (`#`, `#/...`); a remote or
  anchor reference leaves the scope open and, with no name, opaque.
  A body that is one whole JSON object is accepted as the arguments.
  A string value may hold closer text (see
  [Closer text inside a value](#closer-text-inside-a-value)).
- The `<tool_call>` opener is shared with [Hermes](hermes.v1.md). With both
  dialects allowed (the default), each attempts a call there and the body
  decides: a `<function=` body is read by this dialect, and the other dialect's attempt
  is rejected at the body's first byte. An attempt neither dialect reads
  (dangling, cut off, or malformed) is one rejected attempt in the dialect
  that read furthest, with both dialects in its `candidate_dialects`; see
  [the README](../README.md#design).
- `result`: `<tool_response>CONTENT</tool_response>`, the rendering of a tool
  message.
- `declaration`: one `<function>` entry of the tool list: `<name>`, an
  optional `<description>`, and a `<parameters>` block (kept verbatim).
- Reasoning: `<think>` ... `</think>`.

## Printing (canonical form)

The printer (`tool_call::emit`) runs the same productions forward with
canonical choices: the first alternative carrying the most values, closer
classes as their canonical spelling, and whitespace as the grammar's
printing layout. The grammar declares `layout ws "\n";`, so every `ws`
between tags prints as one newline: the layout the published chat template
renders, so a printed call replayed into chat history is byte-identical to
what the template would produce for the same call. (`layout` only affects
printing; recognition accepts any whitespace as before.) Recognizing a
printed call gives back the same tool, an equivalent argument tree, and no
edit but the framing newlines.

- `call`: `<tool_call>\n<function=NAME>\n` + elements + `</function>\n</tool_call>`,
  that is `<tool_call>\n<function=NAME>\n<parameter=P>\nVALUE\n</parameter>\n</function>\n</tool_call>`
  for one parameter; several calls are separated by one newline.
- The body: one `<parameter=P>` element per top-level argument member, each
  `<parameter=P>` `\n` VALUE `\n` `</parameter>` `\n`, after one leading
  newline. A string value of a parameter the codec reads verbatim (declared
  string, enumeration or unmodeled, or undeclared) is written verbatim
  between the two framing newlines; every other value is inline compact
  JSON with exact number spellings. Nested objects are written as inline
  JSON, not as nested parameter elements (both read to the same tree; inline
  JSON is the single canonical choice), and so are the object items of an
  array parameter. A repeatable parameter is one element
  per item.
- The body is checked by decoding it with the codec under the tool's shape
  (a round trip). When that gives back the same tree, resolved, with no
  edit but its framing newlines, closer text a value holds as paired markup
  (`tool_call.value.closer-in-value`, a value like `x <parameter>v</parameter>
  y`) and JSON string escapes (`json.syntax.string`), it is printed.
  Otherwise (for example an unmodeled parameter holding a non-string, a
  value with an unpaired `</parameter>` followed by text, which reads back
  as `Ambiguous`, wherever the closer sits on its line, or one ending in a
  `</parameter>`, which reads back as the value ending there) the body is instead one whole JSON object
  (`tool_call.markup.json-body`), which reads independently of the shape,
  printed only when it too reads back as the call; a call neither reads
  back as is skipped as `unrepresentable`. JSON text that would contain
  parameter markup or a call closer writes every `<` as `\u003c`. *Proved
  by* `tool_call_roundtrip::values_holding_closer_text_roundtrip_to_qwen`.
- `result`: `<tool_response>CONTENT</tool_response>`, content verbatim (a
  content containing `</tool_response>` cannot be printed). The tool message
  carries no call id.
- `declaration`: `<function>\n<name>NAME</name>\n<description>DESC</description>\n<parameters>SCHEMA</parameters>\n</function>`,
  where `SCHEMA` is the tool's exact parameters schema text, verbatim (the
  `<parameters>` body is read by the `raw` codec, so either the exact schema
  text or the template's per-parameter XML expansion is accepted). The
  template's XML expansion is not printed: it renders nested schemas lossily,
  while the exact text is what arguments are validated against.

## Known variants handled by closer classes

- `</functions>` is read as `</function>` and `</tool_calls>` as
  `</tool_call>` (closer-class variants, recorded as
  `tool_call.markup.closer-variant`).
- `</parameters>` is read as `</parameter>` by the codec.
- Repeated or missing closers: every closer-class spelling is also known to
  the engine's weighted repair search, which may delete a stray closer
  (`tool_call.markup.stray-closer`) or insert a missing one mid-message
  (`tool_call.markup.missing-function-close`,
  `tool_call.markup.missing-wrapper-close` where the next call's opener or
  the end of output follows; before text that no closer follows up to the
  next opener or the end of output, the text may be content or inside a
  wrapper that never closes, so the insertion is the guess
  `tool_call.markup.missing-wrapper-close.before-text`, never usable under
  `Structural`) or at the end of output
  (`tool_call.markup.missing-closers-at-end`, usable only when the call is
  otherwise complete), keeping the least-cost reading. A closer spelling
  immediately after a complete call (`</function></function>`,
  `</tool_call></tool_call>`) is absorbed into that call as a stray closer,
  never left as content. A stray `</parameter>` between parameters (after a
  typed value, or one a string value cannot hold) is deleted by the codec,
  a body of only closers is no arguments
  (`tool_call.markup.closers-only-empty`), a declared parameter's opener ends
  an unclosed value (`tool_call.markup.missing-parameter-close`), and the
  end of the function element ends one
  (`tool_call.markup.parameter-closed-by-element`).

These are repairs and are covered by the repair goldens, not by the
conformance anchors, which hold canonical markup only.

## Closer text inside a value

A parameter value can legitimately contain the exact text of a closer inside
its content: a `write_file` whose content includes
`<parameter>v</parameter>`, or documentation that mentions
`</function></tool_call>`. For such text the maintainer's decision on
issue #3 applies ("Pick by evidence, else ambiguous"): the value has two
readings, and recovery picks between them by evidence
(`tool_call.value.closer-in-value`):

- (A) the value ends at the first closer, as the markup would end it;
- (B) the value holds that closer as text and ends at the closer that
  actually completes the call.

Both are read, and compared by the existing cost machinery (the repairs each
needs, with their rule costs): when only one reading parses the rest of the
call without repairs the other does not need, it wins and the call stays
`Resolved`. When both parse cleanly (closers mentioned inline and the call
then left unclosed), the call is `Ambiguous` and reports both values (never
usable on a mutating tool); and a value holding text after an unpaired
closer is always `Ambiguous` (below). A value read as B records
`tool_call.value.closer-in-value`, spanning from the first closer it holds
to where it ends; it is not a guess, since it is chosen only when the other
reading needs more repair, and equally clean readings are reported side by
side.

The decision covers closer text inside real value content; it does not
override issue #3 §3 ("`</parameter>`, `</parameters>`, and repeated or
stray closers are all one class"), its negative corpus ("text between
parameters" and "trailing text inside the wrapper" are rejected or stay
unresolved), or its hard limit ("Never accept trailing free text as an
argument"). Reading B is offered only where the closers it holds sit inside
the value's text:

- **Stray closers.** Closers followed by nothing but closers and whitespace
  up to where the value would end (`a.py\n</parameter>\n</parameter>`,
  `a.py</parameter></parameter>`,
  `</parameter></parameters>`, a value followed by
  `</parameter>\n</function>\n</tool_call>` and the real closers) hold no
  value text: they are stray closers (`tool_call.markup.stray-closer`) and
  reading B is not offered. Closers left after a call once its value
  ended, `</parameter>` included, are that call's stray closers, recorded
  and never assistant text.
- **Layout carries no meaning.** Where a closer sits on its line (after a
  newline, mid-line, indented), and whether the value is framed by
  newlines, never decides how it is read (maintainer, adversary pass 6:
  "Newlines have zero meaning here"). Trimming one framing newline on each
  side of a value is value normalization, not closer semantics.
- **Paired markup.** A closer that closes an element the value opened
  (`<parameter>v</parameter>`, `<tool_calls>…</tool_calls>`,
  `<functions>f</functions>`, by tag name) is text of the value.
- **Text after an unpaired closer.** An unpaired closer (a `</parameter>`,
  `</parameters>` or a closer of the call, closing no element the value
  opened and no call it holds) followed by text before the closer that
  would end the value has two equally cheap readings: the value holding the
  text (`tool_call.value.text-after-closer`), or the value ending at that
  closer with the text dropped as text between parameters
  (`tool_call.markup.text-between-parameters`, a data loss). Both are
  guesses and they tie, so the call is `Ambiguous` with both values, never
  usable on a mutating tool: `a.py</parameter> I will now write the
  file.\n</parameter>`, `a.py\n</parameter>\nI will now write the
  file.\n</parameter>`, an indented ` </parameter>` and `End with
  </function></tool_call> then stop.\n</parameter>` all read alike (issue
  #3: "Never accept trailing free text as an argument"). When a string's
  declared pattern rejects either value, neither is a reading: the value
  ends at its first closer and the text after it makes the call malformed.
- **Whole calls.** A value holding call markup holds whole calls (next
  section).
- **Misspellings.** A misspelled closer inside a value (`</paramete>`) is
  text of it: no repair reads it as a closer (T5/F9).

Where the closers are:

- inside the function body, `</parameter>` (and `</parameters>`): the
  `xml-params` codec weighs every closer before the next declared parameter
  as a place the value may end, in one pass over the value;
- the call's own closers (`</function>`, `</tool_call>` and their declared
  variants `</functions>`, `</tool_calls>`), which end the arguments slot:
  the engine reads the slot on past each one beside the reading that ends
  there, and a reading that since read only closers and whitespace never
  ends there.

What a value never holds besides: a declared parameter's opener outside a
call it holds (it always ends a value); a misspelled declared opener after a
closer (between parameters it begins that parameter,
`tool_call.markup.token-misspelled`); an opening reasoning marker (a call is
cut there, `tool_call.reasoning.marker-cut`); and, for a string with a
declared pattern, a value the pattern rejects (the value then ends at its
first closer). Typed values (parsed as JSON) and enumerations end at their
first closer.

Under `Effort::Exact` the rule applies too (it is parsing, not a repair): a
canonical call whose value holds closer text resolves, and a doubled closer
is a stray closer, a repair `Effort::Exact` refuses.

Reading B is bounded and linear: in the codec it is one pass over the
value; in the engine the reading that holds the call's closers forks one
reading per closer it passes, whose value is read only if that reading
completes the call (a fork that dies at the bytes after its closer costs
nothing more, and only the last fork's failure is ever repaired), and of
the readings that complete it only the latest is read (a reading of the
same span already costing more than one reported is dropped unread). Under
`ToolRecoveryRequest::sized` (33 `Work` units per byte with the `Standard`
preset at 1 MiB), a 1 MiB `write_file` content holding 1,000 closers, each
followed by text (so `Ambiguous`, both values, the codec reading the body
once per reading), recovers in 22.6 units per byte for `</parameter>`, 26.7
for `</function>`, 16.5 for `</function></tool_call>` and 30.4 for
`</parameter></function></tool_call>`, against 17.0 for the same value with
no closer; a 1 MiB value
followed by a closer sequence on its own lines resolves (stray closers) in
23.0 (`tool_call_cost::closer_text_in_values_is_linear` proves the scaling).

## Call markup inside a value

A value may also hold a whole call's markup, as a `write_file` content
documenting the format does (`Use <function=g><parameter=x>1</parameter></function>
to call g`). The markup of a call the value holds is that call's own: its
parameter openers (even a name the outer tool declares) and its closers are
text of the value, so no reading ends the value inside it, and the value
holds it whole: one call with the full content, and no call is read from
the value. A reading whose value would end inside a call another attempt
opened in it (its closers are the outer call's own, as for two calls in a
row) is no reading, so two consecutive calls are never read as one call
holding the other; and a value followed by its closers and then a whole
call, whether the closers are on their own lines or written inline
(`x</parameter></function></tool_call>\n<tool_call>...`), ends at its
first closer (the call after it is read apart, the closers after that are
stray): a call opener after an unpaired closer always begins the next
call.

Measured at 1 MiB under `ToolRecoveryRequest::sized` (33 units per byte):
a `write_file` content holding 1,000 documented calls recovers as one call
with the full content in 23.2 units per byte when they are written inline
(`<function=g>...</function>`; 26.8 for 4,000), and in 27.0 when each is
wrapped in `<tool_call>` (30.9 for 2,000; a value holding 4,000 wrapped
calls in 1 MiB exhausts the sized budget, an explicit
`BudgetExceeded(Work)`).

The reading is bounded: the value of the call just before another call's
opener reads on over that one call, the value of the earliest call read as
written only with a repair reads on over every call after it (so a README
documenting several calls is one call), and every other value stops at the
next call's opener; a reading never forks while a call it holds is open (a
call it holds closes at its closer, even while that call's trail still
weighs the stray closers after it).
*Proved by* the goldens `qwen_value_holds_call_markup`,
`qwen_value_holds_several_documented_calls`,
`qwen_value_holds_several_wrapped_documented_calls`,
`qwen_value_holds_wrapped_call_then_text`,
`qwen_value_documents_call_with_declared_parameter`,
`qwen_value_holds_closers_then_whole_call`,
`qwen_value_holds_closers_then_undeclared_call` and
`qwen_consecutive_calls_of_different_tools`, and the `call-markup`,
`wrapped-call-markup` and `closers-then-call` rows of the correction
evaluation.

## Deliberately not accepted

- Several `<function=...>` elements inside one `<tool_call>` wrapper: the
  template always wraps each call separately.
- Attributes or quoting in the opener (`<function name="x">`,
  `<parameter name="p">`): these belong to other XML dialects.
- Text between parameters other than whitespace (unless a later closer
  ends the value that holds it), text after a whole-JSON body, and text
  between `</function>` and a later closer such as `</tool_call>` (the call
  is malformed).
- Python-style literals in typed values (`True`, `None`) are not canonical;
  the JSON codec's literal-case repair applies and is recorded.
- Streaming deltas (no `delta` production).
