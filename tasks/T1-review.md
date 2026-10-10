# T1 review (issue #8 §1, arrays of nested-parameter objects)

## Scope
Working-tree diff vs 943fa04: xml.rs, edit.rs, ir.rs, resolve.rs, recognizer.rs (wording), goldens (8 new
xml_array_*), tool_call_goldens.rs, tool_call_rules.rs, corpus mod.rs, evaluation data/manifest/constants,
MATCHING-QUALITY.md, RULES.md, INTEGRATION.md, qwen3-coder-xml.v1.md. Source not modified; one throwaway test
file was created in crates/core/tests and deleted (git status clean of it).

## Evidence
- `cargo test -q -p purrassist-core --test tool_call_goldens --test tool_call_rules`: 92 + 41 passed.
- `cargo clippy -q -p purrassist-core --all-targets -- -D warnings`: clean (rc 0).
- `cargo test -q -p purrassist-core --test correction_evaluation`: 10 passed (constants 18166/12621/5545 agree
  with MATCHING-QUALITY.md row 147, 18,166 prose, 5413-of-5413 abstention recall, manifest).
- Real callers: all goldens run through `ToolSet::recover` (golden runner); the named tests in
  tool_call_goldens.rs use the same path; the corpus mutant runs through the evaluation (zero incorrect).

## 1. Contract rows
- xml_array_nested_object_items: arguments equal the issue JSON exactly (OUTFITS const, `to_json()` compare),
  resolved, rules nested-parameter x2, array-item x2, collect x1, no alternatives. OK.
- in_object, mixed, item_unclosed (parameter-closed-by-element x1), unclosed_before_sibling
  (missing-parameter-close x1, two items in order), unmodeled ($ref, schema-typed x2): captured goldens match
  plan; each asserted by a named test.
- same_name_member: {"children":[{"name":"a","children":[{"name":"b"}]}]}, resolved, nested-parameter 2,
  array-item 2, collect 0, missing-parameter-close 0. Matches plan; input is the plan's string.
- string item golden: tags == ["<parameter=wearer>Alice</parameter>"] verbatim, no nested-parameter. OK.
- Evidence spans: test keeps_their_own_spans checks /outfits/1/wearer == "Bob", /0/ == "Alice", garments too.
- Required member: test removes Bob's wearer, asserts no call Resolved and usable().len()==0. OK
  (asserts "not Resolved", not the specific reason; acceptable per plan wording).
- Unit test next to repeated_array_param_is_array exists (repeated_nested_array_items_are_objects).
- Corpus mutant (outfits-items, outfits-items-repeated) exists and evaluates with 0 incorrect.

## 2. Codec
- `nesting()` maps Object/Unmodeled params to item=false, Array(Object|Unmodeled) to item=true, all else None,
  so Array(String) and scalar items never nest.
- `Frame.item` is set only at the single nested push (xml.rs ~977). All three `pop` call sites (L919, L929, L955)
  go through `pop`, which records ArrayItem + `wrap_item` iff `frame.item`; the final top-level frame is
  taken with `frames.pop()` directly (never an item). No path wraps a non-item or skips an item.
- pop returns Result and all three callers use `?`; record errors propagate StopReason, nothing swallowed.
- Depth is charged at the push with the same `size(self.frames.len())` as object nesting; item frames get no
  discount and no extra charge.

## 3. Pointer fix
Throwaway test results (xs: array of {n:int, g:[int]}), pointers from InvalidArgument:
- JSON array [good,good] then nested bad n -> /xs/2/n (correct)
- nested bad then JSON array -> /xs/0/n (correct, no shift for first occurrence)
- nested good, nested bad -> /xs/1/n; array(2), nested good, nested bad -> /xs/3/n
- array(2) then nested item with bad g element -> /xs/2/g
- resulting argument arrays are in source order in every mix.
Bad-in-JSON-array-item cases surface only SchemaValidation (no pointer in the reason), so item-2-of-array
pointers were checked via the index arithmetic above, not directly. Non-array repeated params: the shift is
gated on `value.root` being `Value::Array`, so scalar/object repeats are untouched. Nested arrays inside items
shift only the first segment (/N/g/k keeps k). Stand-ins (`whole`) are skipped and counted as 1, matching
`collect` (non-array occurrence = one item). adopt's count and collect's `before + index` use the same rule.
Existing goldens: shift only applies when an earlier same-named member exists and the new value is an array,
i.e. only repeated array occurrences, which previously aliased pointers; all existing goldens pass.

## 4. DEVIATION A (inline-closers-then-call narrowed)
Legitimate generator fix. The codec consumes exactly one framing newline on each side of a value
(qwen3-coder-xml.v1.md L40; tool_call.xml.framing-newline). A witness value ending in "\n" followed inline by
`</parameter>` spells "v\n</parameter>", which by the dialect reads as value "v"; the mutant's truth ("v\n")
was not what its text says, so recovery was right and the truth wrong. This does not change a truth for an
issue clause; it removes an ill-formed case, so F15 (truths change only with a cited clause) is not engaged,
though the in-code comment is the only justification. Acceptable. The comment is present at both sites.
It is a latent condition that surfaced because the new tool reshuffled seeded values (base had 34 such
mutants, now 38). A reviewer note in the PR body would be good.

## 5. DEVIATION B (18166 cases / 18165 distinct)
Yes, one duplicate: ids `qwen/plan_outfits/s7/...outfits-items` and `.../s19/...outfits-items` have identical
request and truth (two seeds drew the same one-item witness `wearer: "a-"`). Base had 16438/16438, so this
is new. No test or doc claims all requests are distinct: `distinct_requests` is a recorded count verified
against the manifest, the test only requires unique ids and >= 500 distinct requests, and MATCHING-QUALITY.md
does not say "distinct". Benign (same truth, same outcome). Accepted.

## 6. Docs
- MATCHING-QUALITY.md numbers agree with constants (above). Per-category rows moved for unrelated categories
  (e.g. json.syntax.literal-case 77->55, tool_call.value.invalid-utf8 correct 5->2, trim-nonverbatim 25->24)
  because the extra tool reshuffles the seeded stream; incorrect stays 0 everywhere.
- RULES.md, qwen3-coder-xml.v1.md, INTEGRATION.md, xml.rs and edit.rs docs describe the implemented behaviour,
  including unknown-tool caveat, string-array negative, same-name-member limit and the $ref sibling reading.
  The $ref sibling reading has no golden (plan says none required); it is asserted by the dialect doc only.
- INTEGRATION.md wording says "items are unmodeled" only; object items (modeled) are not mentioned there
  (ADVISORY).

## Findings
1. ADVISORY: the recursive-$ref sibling claim (missing-parameter-close, next item) is documented but has no
   executable evidence; consider one golden or soften the doc to "is read as".
2. ADVISORY: `inline_closers_pair` got a second narrowing (`!head.ends_with('\n') || head.ends_with(">\n")`)
   that the deviation note did not mention; the `>\n` escape is heuristic. Mention it in the PR body.
3. ADVISORY: INTEGRATION.md omits modeled object items in its summary sentence.
4. ADVISORY: the required-member test asserts "not Resolved" only; could name the reason.
5. ADVISORY: `ArgTree::adopt` shift also fires for a repeated non-array-declared param that carries an
   array value (e.g. unmodeled); that path's pointers become index-shifted. Unreachable by any current test and
   harmless to outcomes; note only.
No REQUIRED findings.

VERDICT: PASS

## Orchestrator dispositions
- Advisory 1 (no evidence for the $ref sibling limit): addressed with the golden `xml_array_recursive_ref_items_are_siblings` and the named test of the same name, which asserts `[{name:a},{name:b}]`, Resolved, `missing-parameter-close`×1, `stray-closer`×1.
- Advisory 2: the second narrowing (`inline_closers_pair`) will be named in the PR body.
- Advisory 3: declined. The INTEGRATION.md sentence sits in the schema-typed (unmodeled) text section, where it is accurate; modeled object items are documented in `qwen3-coder-xml.v1.md`.
- Advisories 4 and 5: noted; outcomes are unaffected.
