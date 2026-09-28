<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Applying source-bound plans

Markdown planning tools return one edit plan in `value`; text replacement and
unified-diff planning return candidates in `plans`. A plan contains the exact
`sha256:` source identity and an `edits` array with byte `start`, exclusive `end`,
and replacement text. Select one plan explicitly before applying it. Application
does not choose between ambiguous candidates or certify a previous guess about
intent. Keep the planning receipt alongside the application receipt to retain
the full history of selection and inference.

`apply_edit` accepts `source`, `plan`, optional `language`, and optional `effort`.
It invokes the same governed Rust application engine as `EditPlan::apply_governed`.
With `language` set to `python`, `rust`, `javascript`, `typescript`, or `tsx`, it
uses `syntax::apply_checked_governed`, including its bounded punctuation repair
and whole-buffer grammar validation. The default language is `text`.

```sh
purrassist call apply_edit --root . < examples/apply-edit.json
```

The result is an ordinary assistance envelope. A resolved `value` contains
`text` and `inverse`; the inverse is another plan bound to the exact output.
Applying that inverse with `source` set to the output restores original bytes,
including CRLF, UTF-8 spelling, and the absence of a final newline. This operation
does not read or write files. Stale digests, invalid UTF-8 boundaries, overlapping
edits, and ambiguous insertion ordering produce an unresolved receipt. Resource
exhaustion and cancellation remain explicit. Malformed tool arguments are rejected
before engine execution. Syntax-repair alternatives remain ambiguous and are not
silently selected.

`apply_file` accepts `path`, `expected_digest`, and exactly one of `replacement`
or `plan`; `effort` selects its finite execution budget. Supplying neither or both
is an argument error. The existing replacement-buffer form remains supported.
The plan form avoids making callers reconstruct the complete file themselves.

Both the expected digest and the plan's source must match the file snapshot.
Supported code-file extensions still trigger grammar validation automatically.
Only a resolved, unambiguous application reaches the journaled store commit;
stale, invalid, ambiguous, exhausted, or cancelled applications do not enter it.
Cancellation is checked again before commit. Once a transaction begins, it
finishes under the existing durability/recovery contract and reports its actual
outcome. The returned `commit.transaction` is accepted by `undo_file`, which
restores the journaled original only while the exact committed output remains.

These operations use the shared `Tools` dispatcher from Rust, CLI `call`, and
MCP `tools/call`. `apply_edit` is advertised as read-only; `apply_file` remains
a destructive file operation. File scope, cooperating-writer locks, stale checks,
and crash-recovery limitations are unchanged.
