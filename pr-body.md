Tool-graph implementation needs a single reviewed contract before the loader, policy gate, compiler lanes and confinement can be built. Add the specification and delivery plan under `docs/tool-graph/`, with caller-owned vocabulary and shapes, two worked tools, complete projections and loss checks, explicit trust boundaries, typed failures, dependency closure and per-issue acceptance.

All tool-graph behavior is marked specified, not implemented. Phase 3 still requires the maintainer's approval of both documents and the dependency closure, recorded verbatim on #32. The plan also records new upstream prerequisite G7: the existing JSON-LD schema projection needs a plain FnO argument profile before #33 can deliver the promised MCP schemas.

Equivalent Option-combinator changes in core and test helpers repair existing Clippy failures without raising the Rust 1.98 requirement or changing recovery behavior.

Validation: independent specification review passed; three Turtle blocks plus the embedded manifest dataset, two SRL programs, five strict JSON blocks, one AppArmor policy on parser 4.1.7 (`-QK`, no kernel load), and one GBNF using the actual llama.cpp parser passed. Eight explicit declaration payloads match the existing renderers. Both documents pass `prose-check` and repository documentation checks. The full mandatory `make check` passed, including both Clippy commands with warnings as errors, optimized workspace tests, doctests and Rustdoc.

Closes #32 for delivery of the Phase 2 specification. Maintainer approval remains a separate requirement before Phase 3. All eight hosted CI jobs passed.
