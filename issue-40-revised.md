## Objective

Give purrassist a canonical **RDF 1.2 tool specification** and the machinery that enforces it. Tools are described once, as an RDF 1.2 graph built from W3C FnO, MCP `ToolAnnotations`, SHACL 1.2 shapes, reified parameter hints and a granular AppArmor security vocabulary. Everything else is compiled from that graph:

1. **Canonical tool graph:** FnO plus SHACL 1.2 plus RDF 1.2 reifiers carrying recovery tolerances, taint and session-binding rules on individual property shapes.
2. **Datalog policy gate:** SPARQL 1.2 RL `parameterConstraints` evaluated over session facts, call arguments and the policy graph under a governed ceiling, with proofs; static security conflicts are refused at load.
3. **Compiler lanes:** MCP Extended JSON, llama.cpp GBNF, `CompactSignature`, and model-family prompts for Claude, Qwen/Hermes, Llama, Mistral, DeepSeek and CompactTypeScript. **Every lane returns a `LossLedger`, and every dropped constraint becomes a mandatory post-generation check.**
4. **AppArmor synthesis and confinement:** full AppArmor v3/v4 profiles and `^tool` hats (files, capabilities, network, unix, signals, ptrace, mounts, rlimits), parameterized by `--root` and the session, applied with `aa-exec` or `change_hat`.

All of it is built on purrdf: `purrdf-core`, `-rdf`, `-shapes` (`json_schema`, `typescript`, `srl`), `-datalog`, `-text`, `-gts` and `-validate`. Nothing purrdf already does is reimplemented.

## Phases (sub-issues)

- [x] #31: Phase 1, reconnaissance (complete; the findings live there)
- [ ] #32: Phase 2, specification `docs/tool-graph/TOOLS.md` and `TOOLS_PLAN.md`. **Review stop:** Phase 3 starts only after maintainer approval.
- [ ] #38: Phase 3, implementation (#33, #34, #35, #36, #37)
- [ ] #39: Phase 4, verification and conformance suite

Upstream prerequisites in purrdf:
- Blackcat-Informatics/purrdf#747: FnO reader;
- Blackcat-Informatics/purrdf#748: caller-registered loss profiles;
- Blackcat-Informatics/purrdf#749: governed SRL `infer`.
- #43 (G7, tracked in this repository): upstream plain FnO argument-schema projection. The existing emitter is JSON-LD; #33 requires the plain profile and a qualified consumer, without a second local schema compiler.

## Design decisions

- **D1. Crate layout.** One new pure crate, `purrassist-toolgraph`, built on purrdf leaf crates rather than the `purrdf` facade, and buildable for `wasm32-unknown-unknown`. Runtime confinement goes in `purrassist-host` under `cfg(target_os = "linux")`. `purrassist-core` keeps its empty dependency closure.
- **D2. Spec location.** The specification lives in `docs/tool-graph/`, because `docs/TOOLS.md` is the generated MCP tool reference.
- **D3. Upstream fixes.** purrdf gaps are fixed upstream, not worked around: FnO reader (G1), open loss profiles (G2), governed SRL (G5). The AppArmor glob language stays first-party (G3). GBNF regex lowering uses `regex-syntax` HIR (G4).
- **D4. Unexpressible network scopes are refused.** AppArmor 4.1 has no CIDR form, and its network rules take type *or* protocol, not both. A destination set AppArmor cannot express exactly is refused (`UnenforceableNetworkScope`). It is never widened, and never only recorded as a loss, because a runtime check cannot confine a socket. Explicit enumeration is allowed under a ceiling the caller sets.
- **D5. Token budgets are byte bounds.** `bytes ≤ N` implies `tokens ≤ N` for byte-level BPE. A caller-supplied counter may tighten the bound. Floats are never used.
- **D6. `outputProjectionJq` is a field-mask subset.** It accepts `.a.b`, `.[]`, `{a, b}` and `.a[]?`. Anything else is refused at load.
- **D7. Hashing.** Receipts keep receipt v3 SHA-256. Manifest bundles are purrdf-gts BLAKE3, optionally COSE_Sign1. The auditable receipt binds both digests.
- **D8. Reuse existing renderers.** Prompt projection reuses the existing `DeclarationForm` renderers. The GBNF call envelope comes from the existing dialect grammars, so the grammar and recovery agree by construction.
- **D9. One path for host tools.** purrassist's own MCP tools are described in the tool graph, and `tools/list` serves the derived `_meta.*`.
- **D10. Dependency approval.** New dependencies (the purrdf crates and their transitive closure) are listed in TOOLS_PLAN.md. The maintainer's Phase 2 approval is the approval `docs/DEPENDENCIES.md` requires.

## User rulings

- 2026-10-25, original brief: "Work in strict phases. **STOP after Phase 2** for my review before implementing Phase 3."
- 2026-10-25: "Your goal is to write a series of issue to do the work."
- 2026-10-25: "phases should be linked github issues."
