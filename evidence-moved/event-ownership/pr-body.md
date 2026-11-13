Partial implementation of #16: batch recovery and ToolCallStream share one incremental source intake, recognizer, governor and finalizer. The committed kernel retains complete admitted source and its digest, publishes conservative native-history events, and emits EOF events from unique completed readings.

Prepared events now retain their record and actual payload reservations through sink handoff, then release producer storage on success or refusal. Batch discard, external sinks and convenience collectors use the same per-event construction/handoff schedule. Caller sink and collector memory belongs to the caller from inception; recovery does not claim to bound arbitrary retained caller output. Performed Work and cumulative scratch admissions remain. Printing and streaming also share the same governed text-copy authority, replacing duplicated helpers.

This draft keeps all remaining #16 requirements open: the full longest-stable-prefix contract, ambiguous EOF intersection, exact hold-back/candidate accounting, Clone and constrained-decoding queries, CLI streaming, MCP progress, incremental emitters, split-invariance/failure coverage, fuzzing and WASM qualification. No issue closure is requested.

Validation: independent design and implemented-source review, formatting, prose checks and normal configured hooks passed at 3644a94. No local compilation, tests or replay ran. Automatic current-head CI is pending. Previous 7fef945 CI exposed permanent event-storage debt, Work exhaustion and the duplicated copier; the ownership/copy changes address those source causes, but resource acceptance and complete batch/stream parity remain unverified. Repeated unchanged-frontier computation remains an open Work finding.

References #16.
