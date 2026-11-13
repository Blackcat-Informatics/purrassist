Partial implementation of #16: batch recovery and ToolCallStream now share one incremental source intake, recognizer, governor and finalizer. The committed kernel retains complete admitted source and its digest, publishes conservative native-history events, and emits EOF events from unique completed readings. Semantic request refusals still permit complete source capture under a healthy governor.

This draft keeps all remaining #16 requirements open: the full longest-stable-prefix contract, ambiguous EOF intersection, exact hold-back/candidate accounting, Clone and constrained-decoding queries, CLI streaming, MCP progress, incremental emitters, split-invariance/failure coverage, fuzzing and WASM qualification. No issue closure is requested.

Validation: independent committed-source review found no stop-publication defect in the inspected intake, frontier, event ownership, finalization and facade paths. Normal configured commit hooks passed. Compilation, execution and full batch/stream parity remain unverified; automatic CI is pending. No local compilation, tests or replay ran under the standing execution restriction.

References #16.
