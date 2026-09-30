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

Both the expected digest and the plan's source must match the file snapshot,
unless `rebase_from` reconciles a stale one (below). Supported code-file
extensions still trigger grammar validation automatically. Only a resolved,
unambiguous application reaches the journaled store commit; stale, invalid,
ambiguous, exhausted, or cancelled applications do not enter it. Cancellation
is checked again before commit. Once a transaction begins, it finishes under
the existing durability/recovery contract and reports its actual outcome. The
returned `commit.transaction` is accepted by `undo_file`, which restores the
journaled original only while the exact committed output remains.

These operations use the shared `Tools` dispatcher from Rust, CLI `call`, and
MCP `tools/call`. `apply_edit` is advertised as read-only; `apply_file` remains
a destructive file operation. File scope, cooperating-writer locks, stale checks,
and crash-recovery limitations are unchanged.

## The `Patch` algebra and governed rebase

`plan_markdown_structure` provides section insertion/removal/movement/renaming,
list movement/renumbering, table row/column movement, and reference
addition/relabeling/removal/unlinking through the shared Rust, CLI, and MCP
handler. Each operation reparses its proposal and checks the selected structure
and surrounding blocks before returning a digest-bound plan.

Heading renaming preserves ATX or setext style. A multiline setext title becomes
the requested single-line title while retaining its first-line indentation,
final title-line ending, and exact underline. Moving a setext section with a
requested level of 1 or 2 changes only the underline markers; levels 3–6 convert
the heading to ATX and join title lines with spaces. Literal closing hashes are
escaped when needed. Planning evidence records the old/new level and style,
title-line joining, and original source span. Application retains replaced
source in its loss ledger and supplies an inverse that restores every original
byte. Preserve both receipts to retain the full transformation history.

`plan_markdown_task` selects the first visible paragraph of the chosen list
item. When its marker contains a newline, changing the state joins the marker
lines and removes the intervening continuation prefixes. Planning evidence
records the exact replaced span as `markdown.task-marker-join.v1`; outside
bytes remain exact, and applying the inverse restores the original split marker.

`purrassist_core::patch::Patch` (an ordered, non-overlapping, source-digest-
bound set of byte-range replacements) is the crate's single edit
representation; `edits::EditPlan` and `edits::TextEdit` are aliases for
`Patch` and its element type, not a second implementation. Every plan this
document describes — from Markdown planning tools, `plan_replacement`,
`plan_diff`, and the wire `plan` object `apply_edit`/`apply_file` accept — is
a `Patch`. Beyond `apply`, `Patch` exposes a governed algebra: `invert`
(the exact inverse, matching a plan's `inverse` field), `compose` (sequential
chaining through an intermediate text), `commute` (whether two patches made
against the same source have disjoint edited ranges, and if so, each rebased
onto the other), and `diff` (a single covering replacement from one text
directly to another).

A plan's `source` names the exact bytes it was made against. If that source
has since changed — another writer's edit landed first — applying the plan
directly is refused as stale. `apply_edit` and `apply_file` accept an
explicit `rebase_from` object, `{"source": "<prior text>"}`, naming the exact
text the plan's own declared `source` digest matches. When the plan's
declared source no longer matches the live buffer/file but does match
`rebase_from.source`, the tool diffs `rebase_from.source` against the current
text into an intervening `Patch` (`Patch::diff`) and rebases the plan onto it
(`Patch::rebase`): if the plan's edits and the intervening change commute
(their byte ranges are disjoint), the rebased plan is applied and the
resulting receipt's `evidence` gains a leading `patch.rebase.v1` entry
recording the rebase; if they overlap, the application is refused as stale,
exactly as an unrebased stale plan already is — a plan whose own edits
conflict with what changed underneath it cannot be unambiguously
reinterpreted. For `apply_file`, `expected_digest` is checked against
`rebase_from.source` (the plan's actual baseline) rather than the live file
when `rebase_from` is supplied; the live file's digest is reconciled by the
rebase itself. `rebase_from` requires `plan` (it has no meaning for
`apply_file`'s `replacement` form).

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

### The journal's recorded `patch`

`prepared.json` is a versioned, self-checked object (`Digest::of` its own
bytes must equal the transaction directory's name); `Store::entry` rejects
any `version` it does not explicitly recognize, rather than guessing at an
unfamiliar or missing one. Version 1 held only `path`, `before` (the
original text's digest), and `after` (the replacement's digest). Version 2
(current) adds `patch`: the wire `Patch` (`crates/core/src/wire.rs`'s
`EditPlan::to_node`, since `edits::EditPlan` is an alias for
[`purrassist_core::patch::Patch`](../crates/core/src/patch.rs)) from the
journaled `before` piece's exact bytes to the `after` piece's, computed with
`Patch::diff` at commit time. Because a patch's replacement text can be as
large as the file itself, `prepared.json`'s admitted size now scales with
the store's own `max_bytes`, not a small fixed constant.

`Store::undo` no longer treats `entry.before` as an independent restore
target. It governed-`Patch::invert`s the recorded patch (bound to the
journaled `before` bytes) to get the patch that maps `after` back to
`before`, then governed-`Patch::apply`s that inverse to the *current* file
text: that apply's own stale-source check — the same one every other
`Patch` consumer relies on — is what validates the current file against the
inverse's expected source digest, and its own edit-range/UTF-8-boundary
validation is what rejects a journal whose recorded patch does not actually
invert against its own `before` bytes. Both failure modes surface as
`Error::Invalid` (a tampered or otherwise non-inverting patch) or
`Error::Stale` (the file no longer holds the exact committed output),
exactly as before, but reached through `Patch`'s own governed algebra
instead of a second, independently maintained comparison.

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
never sleeping, before returning explicit `Error::Busy`; any other
failure is reported immediately as an I/O error. The temporary file is opened with
`GENERIC_WRITE | DELETE` access because renaming through a handle requires
`DELETE`. `Busy` also identifies the cooperating-writer lock. Directory
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
