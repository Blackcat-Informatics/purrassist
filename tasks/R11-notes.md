# R11 notes: project a root `$ref` declaration's parameters (G19)

User decision (verbatim): "Fix them in this PR". Head at start: c2f1916. Not committed.

## Design

`Projector::root` (adapter.rs) asks `plan()` whether the root composes object
schemas through `$ref`, `allOf` or `anyOf`/`oneOf`. If one resolves, `composed()`
projects them; otherwise the previous code runs unchanged (inline roots, and
the Draft-07 "root `$ref` projects nothing" fallback).

- Root `$ref` to a local pointer (`#/definitions/X`, `#/$defs/X`, percent
  decoding via the scope graph's `local_pointer`), followed transitively
  (64 hops, cycle-checked). A followed schema must be a JSON object whose
  `type` is absent or `"object"`, and the leaf must declare an object
  (`type: "object"` or an object `properties`).
- Draft-07: a node with `$ref` is "overridden": every member is noted as an
  unmodeled keyword at its pointer, nothing is projected from it. 2020-12:
  the node applies together with its target.
- Root `allOf` with exactly one element that resolves the same way: applies
  with the root. Root `anyOf`/`oneOf` where every branch resolves to an object
  schema: union branches. A keyword whose targets do not all resolve is not
  followed (the projection stays what it was; so `oneOf:[{required:[a]},...]`
  roots keep today's behaviour).
- Typing: a name declared by exactly one applying (non-branch) schema is typed
  from it via the existing `spec()`; required = any applying schema's
  `required`. A name a union branch declares, or that several schemas declare,
  is `Unmodeled`, not required (typing stays with the exact schema, R2-style);
  each declaring property schema is noted as unmodeled evidence.
- `unknown_parameters`: `Error` when some applying schema closes its scope
  (`additionalProperties:false`, plus `unevaluatedProperties:false` outside
  Draft-07) or every union branch does; a composing keyword (`$ref`, `allOf`,
  ...) or `patternProperties` on the schema keeps it open, as for an inline
  schema (this preserves the pinned `unevaluatedProperties + anyOf` expectation).
- Pointers: new `RootLocations` (toolset.rs) records the schema locations the
  properties were read from. `ShapeBuilder` uses it for the exact-subschema
  default validation and for the scope graph (new `ScopeCompiler::scope_many`,
  a joint node whose members are the union of every declaring location, so a
  union name declared in two branches gets one merged scope node). `schema_sites`
  (resolve.rs) uses `CompiledTool::parameter_pointer` for evidence pointers,
  and `recover.rs::keywords` maps unmodeled-keyword pointers below a followed
  location to arguments-relative pointers (`CompiledTool::argument_pointer`) so
  `on_path` still works. Validation (`CompiledTool::schema`) and
  `render_canonical` are untouched.

## Deviation from the plan text (finding)

The plan/review says prompt-form `render_declaration` renders from the projected
spec and would change for a root-`$ref` tool. It does not: for an adapter-built
tool `render_declaration_governed` prints `declaration.parameters_text()` (the
exact schema bytes) into the prompt dialect (emit::declaration), never the
projected spec. So there is no prompt-form change. This is named and asserted
both ways: `tool_adapters::root_ref_prompt_form_declaration_prints_the_exact_schema`
asserts the Qwen3-Coder XML and Hermes declarations contain the verbatim
FastMCP schema (including `$ref`/`definitions`). R9 need not list a change.

Also: a missing or external root `$ref` cannot reach the projection. The schema
engine rejects it at compile time ("reference pointer does not exist" /
"unresolved resource"), so today's behaviour for those is an invalid
declaration (pinned by `unprojectable_root_ref_stays_pass_through_with_its_evidence`).
The PassThrough cases that do compile are a non-object target, a cyclic `$ref`
chain, `$ref: "#"`, and a type-array target; those are pinned.

## `assert_draft07_root_rules` (tool_adapters.rs): old vs new

| Expectation | Old | New | Reason |
|---|---|---|---|
| `wrapped.parameters.len()` | 0 | 1 (`q`, `ValueShape::String(None)`) | root `$ref` to `#/definitions/args` now projects the target |
| `wrapped.parameter("ignored")` | (n/a, 0 params) | `None` | Draft-07: sibling `properties` ignored for projection |
| `wrapped.unknown_parameters` | PassThrough | PassThrough | target has no `additionalProperties:false` |
| `wrapped` unmodeled keywords | `$schema, definitions, $ref, properties, type` at `""` | identical | siblings and `$ref`/`definitions` stay evidence; the target's `type`/`properties` are carried, so nothing new is listed |
| `{"q":1}` validation | invalid | invalid (kept) | exact schema |
| `{"q":"a","ignored":1}` validation | (not asserted) | valid (added) | sibling `properties` constrain nothing in Draft-07 |

## Tests and goldens

New tests (tool_adapters.rs): `root_ref_projects_the_target_object` (projection,
defaults, enum, array, keyword evidence incl. `/definitions/Args` `title`,
validation, render-back in all three wire forms byte-exact via `assert_render_back`),
`root_ref_prompt_form_declaration_prints_the_exact_schema`,
`root_ref_defs_2020_12_merges_siblings_with_the_target`,
`root_ref_defs_2020_12_target_alone` (closed target -> `Error`),
`root_all_of_single_ref_projects_the_target` (and two-element allOf not followed),
`root_any_of_objects_projects_the_union_of_names` (and a `oneOf` of `required`-only
branches not followed), `unprojectable_root_ref_stays_pass_through_with_its_evidence`,
`root_ref_nested_ref_members_flow_into_the_scope_graph` (scope node declares
`kind`; default validated below the target; bad default is `InvalidDefinition`).

Golden runner: new `tools_mcp` key (exact MCP declaration / `tools/list` text or
value, `ToolSet::from_mcp`); exactly one of `tools_openai`, `tools_mcp`, `tools`.

New goldens (all new files; expectations written first as descriptions and in the
named tests `mcp_root_ref_calls_resolve_with_typed_arguments`,
`mcp_root_ref_misspelled_parameter_repairs_like_inline`,
`mcp_root_ref_unprojectable_stays_pass_through`, each also asserting MCP/OpenAI/
Anthropic render-back byte-exact):

- `mcp_root_ref_qwen3_xml_resolved`, `mcp_root_ref_hermes_json_resolved`
- `mcp_root_ref_misspelled_parameter_repaired` + inline twin `mcp_inline_misspelled_parameter_repaired` (hermes, `json.key.match`)
- `mcp_root_ref_misspelled_qwen_parameter_repaired` + twin `mcp_inline_misspelled_qwen_parameter_repaired`
- `mcp_root_defs_2020_12_resolved`, `mcp_root_all_of_ref_resolved`, `mcp_root_any_of_union_resolved`
- `mcp_root_ref_unprojectable_stays_pass_through` (type-array target: no repair, `tool_call.parameter.pass-through`, Unresolved SchemaValidation)

Existing goldens changed by the re-bless: none (git status shows only the ten new files).
`PINNED_*_DIGEST` constants: unchanged, all `tool_call_recover` tests pass, so the
Digest rule is not triggered.
