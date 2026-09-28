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

## Filesystem durability

`purrassist_host::files::Store` commits a replacement in three phases: a
journal (`before`/`after`/`prepared.json`, each written then synced), the
atomic replacement of the target file, and a `committed` marker. Every
`commit` returns `Commit::durability`, evidence of which platform-specific
mechanism actually reached durability for that call; it is exposed on the
wire under `apply_file`/`undo_file`'s `commit.durability` object as
`rename` (`posix_semantics`, `move_file_ex_fallback`, or `already_current`
for an idempotent retry that needed no rename) and `directory_sync`
(`flushed` or `access_denied_fallback`).

On Unix, replacement is `rename(2)` and directory durability is `fsync` on
an open directory handle; both are unconditional and always succeed or the
commit fails outright. The target's Unix permission bits are copied onto
the replacement file.

On Windows (`crates/host/src/files/windows.rs`), the temporary file is
opened with the write-through flag, so every write it accepts is durable as
soon as it returns, before any rename is attempted. Replacement then tries
`SetFileInformationByHandle` with `FileRenameInfoEx` and
`FILE_RENAME_FLAG_POSIX_SEMANTICS | FILE_RENAME_FLAG_REPLACE_IF_EXISTS` on
that still-open handle. If the filesystem rejects POSIX rename semantics
(`ERROR_INVALID_PARAMETER` or `ERROR_NOT_SUPPORTED`), the handle is closed
and replacement falls back to `MoveFileExW` with
`MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`. Either attempt that
instead fails with a sharing/lock violation (`ERROR_SHARING_VIOLATION` or
`ERROR_LOCK_VIOLATION` — commonly an antivirus scanner or search indexer
holding a brief handle open) is retried a small fixed number of times,
yielding the calling thread (`std::thread::yield_now`) between attempts and
never sleeping, before returning the last operating-system error; any other
failure is reported immediately. The temporary file is opened with
`GENERIC_WRITE | DELETE` access because renaming through a handle requires
`DELETE`. `Busy` is reserved for the cooperating-writer lock. Directory
durability opens the
directory with `FILE_FLAG_BACKUP_SEMANTICS` and calls `FlushFileBuffers`;
some filesystems deny this (`ERROR_ACCESS_DENIED`, at open or at flush),
which is reported as `access_denied_fallback` evidence rather than treated
as a failure, since the write-through rename already made the replacement
durable on its own.

Windows has no permission-bit concept to preserve. The replacement file is
created fresh in the target's containing directory and inherits that
directory's ACL, which is ordinary Windows file-creation behavior requiring
no code here; nothing analogous to the Unix `0700`/`0600` journal modes is
set explicitly on Windows, and none is needed for the same reason.

`Store::recover` never aborts on one bad journal: a torn, truncated, or
otherwise unreadable `prepared.json` or piece file is reported as
`Recovery::Corrupt { id, reason }` for that transaction only, and every
other journal is still recovered. It also scans the store root for
`.purrassist-<id>.tmp` replacement temporary files left behind by an
interrupted commit: ones whose id matches a known journal are removed and
reported as `Recovery::OrphanRemoved`; ones matching no known journal are
reported as `Recovery::OrphanUnknown` and left in place.

`crates/host/tests/crash_recovery.rs` proves this by re-executing its own
test binary as a child process that installs a crash hook
(`purrassist_host::files::install_crash_hook`, a `#[doc(hidden)]` test-only
seam with a default no-op) and calls `std::process::abort` at one of eight
named checkpoints inside `commit` (after each journal piece write, after
the journal directory syncs, after the temporary file is written, before
and after the rename, and before the `committed` marker). The parent then
runs `recover` and a retry against the exact bytes left on disk and asserts
the file is exactly the before or exactly the after version — never
mixed — that the classification matches, and that the retry completes
idempotently.
