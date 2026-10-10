# R2 hand derivation and expected values (written before any golden was blessed)

Cost model used (from `grammar/cost.rs`, `grammar/edit.rs`, `tool_call/rules.rs`):
registry cost = operator base cost x grounding multiplier (definition 1, syntax 2).

| Rule | base | grounding | registry cost | guess |
| --- | ---: | --- | ---: | --- |
| `tool_call.markup.missing-parameter-close` | 2 | definition | 2 | no |
| `tool_call.markup.unmodeled-scope-boundary` (new) | 2 | syntax | 4 | yes, readings multiple |
| `tool_call.markup.stray-closer` | 3 | syntax | 6 | no |
| `nested-parameter`, `array-item`, `framing-newline` | 0 | - | 0 | no |

The new operator mirrors `undeclared-parameter-boundary`: the plan's "cost 2" is the base cost
(registry 4), the same convention every other cost in the plan uses.

## Golden `xml_unmodeled_opaque_scope_boundary`

Tool `rm(path: string required, keep: array of {"type":"object","additionalProperties":true})`.
The adapter projects the item as an open object with no declared member (`ValueShape::Object`
with zero parameters and `PassThrough`), so its scope is `Opaque` (membership unknown: it
declares no name and may hold any). Input inside the normal markup:

    <parameter=keep>\n<parameter=path>\n/home/user/important\n</parameter>\n</parameter>

Trace:

1. `<parameter=keep>`: array of objects, body begins with `<parameter=`: push item frame (Opaque),
   edit `nested-parameter` (0).
2. `<parameter=path>`: `owner("path")` = the arguments frame (declares `path`; the Opaque frame
   declares nothing). The Opaque item frame is popped: `unmodeled-scope-boundary` (4, guess),
   then `pop` records `array-item` (0) and adopts `[{}]` as `keep`.
3. `path` value `\nL\n` ends at its `</parameter>`: two `framing-newline` (0).
4. The final `</parameter>` is outside any value and no nested object is open: `stray-closer` (6).

Committed reading: `{"keep":[{}],"path":"/home/user/important"}`, total cost 4 + 6 = 10, one
guess inference (`unmodeled-scope-boundary`). Matches the plan's expected value.

Outcome. Default policy for a `Mutating` tool is `Structural`, which does not admit a
multiple-reading rule: the operation outcome is `best_guess` (the single reading carries a
guess), the call is `Unresolved` (`rule_not_admitted`), usable counts 0/0, verdict
`nothing_usable`, exit 4. Under `--policy tolerant` the call itself is `BestGuess`; still not
usable on a mutating tool (`usable_calls_accepting_guesses` counts only read-only guesses), so the
verdict stays `nothing_usable` and exit 4. The previous head reading was `missing-parameter-close`
(2) + `stray-closer` (6) = 8, not a guess, `Resolved`, verdict `run`: the hoisted `path` ran.

Observation (no deviation from the plan, recorded for honesty). A zero-repair reading also
exists: keep `path` inside the item and let the two balanced closers close the item and `keep`
(`{"keep":[{"path":"/home/user/important"}]}`, cost 0). The codec reads by scope ownership, not
by search, and the plan fixes "the guessed reading" as the single reading; the call is therefore a
guess that is never usable on a mutating tool, which is what removes the incorrect `run`. The
balanced closer is not hidden: it is the recorded `stray-closer` loss.

## Expected values for the other goldens (exact JSON, fixed before capture)

| Golden | Tool / input | Expected arguments | Outcome | Ledger rules (in order) |
| --- | --- | --- | --- | --- |
| `xml_array_recursive_ref_items_nest` | p9r: `tree(children: array of $ref node{name, children: array of $ref node})` | `{"children":[{"name":"a","children":[{"name":"b"}]}]}` | Resolved | nested-parameter, nested-parameter, array-item, array-item |
| `xml_array_ref_items_member_shares_outer_name` | p10_ref_plan | `{"name":"party","outfits":[{"name":"Alice","size":3},{"name":"Bob","size":4}]}` | Resolved | includes nested-parameter x2, array-item x2, collect x1, schema-typed x2 |
| `xml_array_ref_items_keep_path_not_hijacked` | `rm(path, keep: array of $ref k{path})`, `path=/tmp/x` then `keep[0].path=/home/user/important` | `{"path":"/tmp/x","keep":[{"path":"/home/user/important"}]}` | Resolved | nested-parameter x1, array-item x1, no missing-parameter-close, no stray-closer |
| `xml_array_ref_items_outer_array_collision` | `save(tags: string[], notes: array of $ref note{text, tags: string[]})` | `{"tags":["work"],"notes":[{"text":"buy milk","tags":["home"]}]}` | Resolved | array-item x3 (outer tags, note, note.tags), nested-parameter x1 |
| `xml_object_ref_param_member_shares_outer_name` | `one(path, cfg: $ref{path, mode})` | `{"path":"/a","cfg":{"path":"/b","mode":"x"}}` | Resolved | nested-parameter x1 |
| `xml_array_anyof_items_union_nest` | `un(path, keep: array of anyOf[object{path, n:int}, string])` | `{"path":"/a","keep":[{"path":"/b","n":3}]}` | Resolved | nested-parameter x1, array-item x1, schema-typed x1 |
| `xml_array_ref_items_repeated_member_collects` | p9r with two nested `children` in the item | `{"children":[{"name":"a","children":[{"name":"b"},{"name":"c"}]}]}` | Resolved | nested-parameter x3, array-item x3 |
| `xml_unmodeled_opaque_scope_boundary` | above | `{"keep":[{}],"path":"/home/user/important"}` | best_guess (operation) | nested-parameter, unmodeled-scope-boundary, array-item, framing-newline x2, stray-closer |
