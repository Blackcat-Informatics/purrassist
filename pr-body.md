Complete the remaining numeric prerequisite for the shared plain FnO argument compiler by adopting the merged PurRDF numeric implementation. The downstream consumer exercises exact decimal bounds, enums and constants beyond binary floating-point precision, decimal logical constraints, registered IEEE representability losses, and typed source-bearing refusals with valid neighbors.

The existing worked input/output schemas, nested bindings, requiredness, closedness, native resource refusal and JSON-LD neighbors remain part of this consumer. The implementation continues to use the upstream compiler directly.

Adopts PurRDF PR #888 at `742bbbfbe2240e1fc6f178badd049420753ef358`. The resolved inventory retains all 99 package identities, updates 19 upstream source registrations, preserves the empty core dependency closure, and retains seven byte-identical source license notices.

Validation passed: independent source review, all 11 native plain-schema consumer tests, the release graph-library build for `wasm32-unknown-unknown`, and the complete mandatory `make check` gate.

Closes #43. The complete production loader and bidirectional codec contract remains open under #33.
