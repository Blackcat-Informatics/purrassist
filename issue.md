# Issue #14: Tool-call recovery: add the gemma3-tool-code.v1 dialect

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

## Objective

Add the built-in dialect **`gemma3-tool-code.v1`**: Gemma 3's tool-call format, a fenced `` ```tool_code `` block containing a Python-style call, for example:

````
```tool_code
get_weather(city="Paris")
```
````

## Today

The markup is detected and reported as `Rejection::UnsupportedDialect { family: gemma3-tool-code.v1 }`. Its bytes stay content, and the finish is `Unresolved`.

## Scope

- **Grammar:** the `` ```tool_code `` fence and its closer, with one call per block, or several if the format allows it. Document whichever applies.
- **Arguments:** read with the **`pythonic` codec** from #12. **Depends on #12 landing first.**
- **Tool results:** document the `` ```tool_output `` result convention, if it applies, as content or a result, consistent with how other dialects treat tool results.
- **Code fences:** an ordinary ```` ```python ```` fence that only *mentions* a call stays content. Negative cases pin this.

## Definition of done (same bar as the nine dialects PR #5 delivered)

- **Grammar.** A grammar in the first-party DSL (`docs/tool-calls/GRAMMAR.md`), registered as a built-in dialect. `MarkupFamily::…::built_in()` becomes true, and the `detect_unparsed` detector stops reporting this family as `unsupported_dialect`, because it now parses.
- **Fixtures and tests.**
  - At least 3 canonical anchor fixtures in the conformance set.
  - Goldens, including a malformed or repaired case per new rule.
  - Negative-corpus cases.
  - Coverage in `every_pair_roundtrips` (print/recognize round trips with every other dialect).
- **Docs.** A dialect doc, `docs/tool-calls/dialects/<id>.md`, with a provenance note (D21: where the format is specified, and which model versions it was checked against). The dialect table in `docs/tool-calls/README.md` is updated.
- **Fuzz and evaluation.**
  - Fuzz seeds for the `tool-call` (and, if useful, `tool-call-ref`) targets.
  - A WASM transcript entry (`make wasm-test`).
  - Correction-corpus witnesses, keeping 0 incorrect answers.
- **Cost.** It stays within the published linear cost bound and the sized per-byte budget (`docs/RESOURCES.md`), with a dense-markup row added for the dialect.
- **Docs that currently list this dialect under #4** (`README.md`, `docs/IMPLEMENTATION.md`, `docs/QUALIFICATION.md`, `docs/tool-calls/README.md`) are updated.
- **Standing constraints from #3 hold:** zero third-party dependencies in core, no network, deterministic linear cost, identical native and WASM output, and a guess is never reported as resolved.

Split out of #4 (Deferred Spec C). Builds on the grammar engine and codec slots from #3 / PR #5, and on `main` at `fccfbe7`.


## Comments (0)

