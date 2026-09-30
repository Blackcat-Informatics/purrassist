// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Coordinated filesystem transactions with persistent journals and conditional undo.
//!
//! The lock coordinates users of this store. It does not lock out arbitrary editors.
//! Atomic replacement covers one file; a batch is not an atomic multi-file commit.
//! Canonical root and symlink checks prevent accidental escapes; this portable API
//! is not a sandbox against malicious concurrent directory replacement.
//!
//! Unix durability uses `rename(2)` plus directory `fsync`. Windows durability
//! (`crates/host/src/files/windows.rs`) uses `SetFileInformationByHandle` with
//! POSIX rename semantics on a write-through handle, falls back to
//! `MoveFileExW` where the filesystem rejects that, retries sharing/lock
//! violations a bounded number of times before reporting the last
//! operating-system error as [`Error::Io`], and
//! records the mechanism actually used in [`Commit::durability`]. See
//! `docs/EDIT-APPLICATION.md` for the full design.
use purrassist::{
    Budget, Governor, Outcome, StopReason,
    digest::Digest,
    json::{self, Node, Value},
    patch::{Patch, TextEdit},
    wire::{integer, object, string},
};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

/// Filesystem failures distinguish conflicts from I/O and invalid paths.
#[derive(Debug)]
pub enum Error {
    /// Operating-system failure.
    Io(std::io::Error),
    /// Invalid path, journal, or input.
    Invalid(String),
    /// A cooperating writer holds the store lock, or Windows rename sharing/lock
    /// contention exhausted its bounded retries.
    Busy,
    /// Current file differs from the expected version.
    Stale,
    /// A governed journal or patch operation exhausted its resource allowance.
    /// This does not establish a stale source or a corrupt journal.
    Resource(StopReason),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Invalid(message) => f.write_str(message),
            Self::Busy => {
                f.write_str("file transaction is busy (writer lock or rename contention)")
            }
            Self::Stale => f.write_str("file changed since the expected snapshot"),
            Self::Resource(reason) => reason.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<StopReason> for Error {
    fn from(reason: StopReason) -> Self {
        match reason {
            StopReason::BudgetExceeded(_) | StopReason::Cancelled => Self::Resource(reason),
            other => Self::Invalid(other.to_string()),
        }
    }
}

#[cfg(windows)]
mod windows;

/// Evidence describing which durability mechanism a commit actually used.
///
/// Unix always uses `rename(2)` and a directory `fsync`, so both fields are
/// constant there. On Windows the rename may fall back from POSIX rename
/// semantics to `MoveFileExW`, and a directory metadata flush may be denied
/// by the filesystem; both are reported as evidence instead of being
/// silently assumed or ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Durability {
    /// How the target file's replacement became visible.
    pub rename: RenameMethod,
    /// Whether every directory this commit synced actually flushed its metadata.
    pub directory: DirectorySync,
}
/// The mechanism used to atomically replace the target file for one commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameMethod {
    /// Unix `rename(2)`, or on Windows `SetFileInformationByHandle` with
    /// `FileRenameInfoEx` and POSIX rename semantics.
    PosixSemantics,
    /// Windows `MoveFileExW` with write-through, used because the target
    /// filesystem rejected POSIX rename semantics.
    MoveFileExFallback,
    /// The target already held the committed bytes; no rename ran this call.
    AlreadyCurrent,
}
/// Whether a directory's metadata flush actually reached durable storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectorySync {
    /// The directory handle's `fsync`/`FlushFileBuffers` succeeded.
    Flushed,
    /// The platform denied the directory flush (observed on some Windows
    /// filesystems/handles); the write-through file rename is the
    /// durability boundary instead.
    AccessDeniedFallback,
}
const fn worse(a: DirectorySync, b: DirectorySync) -> DirectorySync {
    if matches!((a, b), (DirectorySync::Flushed, DirectorySync::Flushed)) {
        DirectorySync::Flushed
    } else {
        DirectorySync::AccessDeniedFallback
    }
}

/// Named checkpoints inside [`Store::commit`] where `crates/host/tests/crash_recovery.rs`
/// injects a process abort to prove [`Store::recover`] classifies every
/// partial on-disk state correctly. Production code never triggers these
/// except through the default no-op hook.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    /// Immediately after the `before` journal piece is written and synced.
    AfterJournalBefore,
    /// Immediately after the `after` journal piece is written and synced.
    AfterJournalAfter,
    /// Immediately after the `prepared.json` journal piece is written and synced.
    AfterJournalPrepared,
    /// Immediately after the journal directory, journal root, and store root are directory-synced.
    AfterJournalSync,
    /// Immediately after the replacement temporary file is written and synced.
    AfterTempWrite,
    /// Immediately before the temporary file replaces the target.
    BeforeRename,
    /// Immediately after the temporary file has replaced the target.
    AfterRename,
    /// Immediately before the journal's `committed` marker is written.
    BeforeCommittedMarker,
}
impl CrashPoint {
    /// Every checkpoint, in the order [`Store::commit`] reaches them.
    #[doc(hidden)]
    pub const ALL: [Self; 8] = [
        Self::AfterJournalBefore,
        Self::AfterJournalAfter,
        Self::AfterJournalPrepared,
        Self::AfterJournalSync,
        Self::AfterTempWrite,
        Self::BeforeRename,
        Self::AfterRename,
        Self::BeforeCommittedMarker,
    ];
}
static CRASH_HOOK: std::sync::OnceLock<fn(CrashPoint)> = std::sync::OnceLock::new();
/// Install a process-global crash-injection hook for `crates/host/tests/crash_recovery.rs`.
///
/// This is a test-only seam, not a production extension point: nothing
/// outside this crate's own tests calls it, and the default (nothing
/// installed) is a no-op. Proving exact crash-recovery behavior requires
/// aborting the process at a precise point inside [`Store::commit`] and then
/// running [`Store::recover`] against the exact bytes left on disk; that
/// cannot be done by unwinding a panic, only by re-executing this test
/// binary as a child process that installs this hook and calls
/// [`std::process::abort`]. `OnceLock` only ever accepts the first `set`, so
/// a second call is a no-op — fine, since each test-binary process installs
/// this at most once.
#[doc(hidden)]
pub fn install_crash_hook(hook: fn(CrashPoint)) {
    let _ = CRASH_HOOK.set(hook);
}
fn fire(point: CrashPoint) {
    if let Some(hook) = CRASH_HOOK.get() {
        hook(point);
    }
}

/// Explicitly selected filesystem root and persistent journal location.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
    journal: PathBuf,
    max_bytes: u64,
}
/// Original UTF-8 source, root-relative path, and version identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Root-relative file name.
    pub path: String,
    /// Exact source text.
    pub text: String,
    /// Exact source digest.
    pub digest: Digest,
}
/// Successful durable replacement and the journal needed for conditional undo.
#[derive(Clone, Debug)]
pub struct Commit {
    /// Deterministic transaction identity.
    pub id: String,
    /// Root-relative target.
    pub path: String,
    /// Original version.
    pub before: Digest,
    /// Written version.
    pub after: Digest,
    /// Evidence of which platform-specific durability mechanism this call
    /// used. Excluded from equality: two commits sharing a transaction
    /// identity are equal regardless of which path reached durability.
    pub durability: Durability,
}
impl PartialEq for Commit {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.path == other.path
            && self.before == other.before
            && self.after == other.after
    }
}
impl Eq for Commit {}
/// One independently journaled result in a non-atomic batch.
#[derive(Debug)]
pub struct BatchOutcome {
    /// Original input position, including repeated paths.
    pub index: usize,
    /// Root-relative target supplied by the caller.
    pub path: String,
    /// Per-file durable success or explicit failure; errors are never discarded.
    pub result: Result<Commit, Error>,
}
/// Recovery observation for one prepared journal, or for a leftover
/// replacement temporary file found while recovering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// Replacement bytes are present; journal finalized.
    Finalized(String),
    /// Original bytes are present; no replacement is claimed.
    NotApplied(String),
    /// Neither version matches; caller must resolve the conflict.
    Conflict(String),
    /// The journal for this transaction id exists but is unreadable: torn,
    /// truncated, or otherwise inconsistent (invalid JSON, a digest that
    /// does not match its recorded piece, a non-UTF-8 or malformed
    /// identity). The target file is never touched before every journal
    /// piece is fully written and synced, so this never implies the target
    /// itself was left in a partial state — only that this journal cannot
    /// certify which version the target holds.
    Corrupt {
        /// Transaction identity the journal directory claims.
        id: String,
        /// Human-readable reason the journal could not be read.
        reason: String,
    },
    /// A bounded regular temporary file at its validated journal's recorded
    /// target parent contained a byte prefix of the recorded replacement and
    /// was removed as evidence-bearing cleanup.
    OrphanRemoved(String),
    /// A leftover `.purrassist-<id>.tmp` replacement temporary file matched
    /// no known journal and was left in place; the path is root-relative
    /// with forward-slash separators on every platform.
    OrphanUnknown(String),
    /// A matching temporary name was retained because its ownership could not
    /// be validated. A name alone never authorizes deletion.
    OrphanPreserved {
        /// Root-relative artifact path, with forward-slash separators.
        path: String,
        /// The journal, location, or content check that prevented deletion.
        reason: String,
    },
}

/// `prepared.json`'s current format version. Bumped whenever a field is
/// added, removed, or reinterpreted; [`Store::entry`] rejects any other
/// value explicitly rather than guessing at an older or newer shape.
///
/// # History
/// - `1`: `path`, `before` (digest), `after` (digest).
/// - `2`: adds `patch`, the wire [`Patch`] (see `crates/core/src/wire.rs`)
///   from `before`'s exact bytes to `after`'s, computed with
///   [`Patch::diff`] at commit time. [`Store::undo`] inverts this recorded
///   patch (validated against the current file's digest through the same
///   governed [`Patch::apply`] every other consumer uses) instead of an
///   independent restore path.
const JOURNAL_VERSION: u64 = 2;
// A diff has at most one replacement, whose JSON spelling takes at most six
// bytes per admitted UTF-8 byte. The remaining allowance covers the path,
// digests, version, offsets, and fixed JSON structure; commit checks the exact
// generated record against this bound before creating transaction artifacts.
const JOURNAL_METADATA_ALLOWANCE: u64 = 65_536;

impl Store {
    /// Attempt each replacement in input order and retain every outcome.
    ///
    /// Each item has its own writer lock, stale check, journal, and conditional
    /// undo identity. Other writers may interleave between items. A failure does
    /// not prevent later attempts or roll back earlier successes. An I/O failure
    /// can require `recover` to determine whether replacement reached disk.
    #[must_use]
    pub fn commit_batch(&self, replacements: &[(Snapshot, String)]) -> Vec<BatchOutcome> {
        replacements
            .iter()
            .enumerate()
            .map(|(index, (snapshot, replacement))| BatchOutcome {
                index,
                path: snapshot.path.clone(),
                result: self.commit(snapshot, replacement),
            })
            .collect()
    }
    /// Open a root and initialize its private transaction directory.
    ///
    /// # Errors
    /// Rejects non-directory roots, symlinked journal locations, and I/O failures.
    pub fn open(root: impl AsRef<Path>, max_bytes: u64) -> Result<Self, Error> {
        let root = root.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(Error::Invalid("store root is not a directory".into()));
        }
        let journal = root.join(".purrassist");
        match fs::symlink_metadata(&journal) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(Error::Invalid(
                    "journal location must be a real directory".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_private_directory(&journal)?;
            }
            Err(error) => return Err(error.into()),
        }
        Ok(Self {
            root,
            journal,
            max_bytes,
        })
    }
    /// Read a bounded UTF-8 snapshot without taking the writer lock.
    ///
    /// # Errors
    /// Rejects non-relative paths, traversal, symlinks, non-files, oversized data, and invalid UTF-8.
    pub fn read(&self, relative: &str) -> Result<Snapshot, Error> {
        let path = self.resolve(relative)?;
        let text = read_bounded(&path, self.max_bytes)?;
        Ok(Snapshot {
            path: relative.into(),
            digest: Digest::of(text.as_bytes()),
            text,
        })
    }
    /// Commit one replacement using a cooperating-writer lock and stale-source check.
    ///
    /// The original and proposed bytes are synced to a prepared journal before
    /// replacement. A sibling temporary file is synced and renamed onto the target.
    /// If a later sync/marker fails, `recover` inspects actual bytes; an error does
    /// not imply the target was untouched. On Unix, the target's original
    /// permission bits are copied onto the replacement. On Windows there are
    /// no permission bits to copy: the new file inherits its ACL from the
    /// containing directory, which is that platform's normal file-creation
    /// behavior and requires no code here (see `docs/EDIT-APPLICATION.md`).
    /// The returned `Commit::durability` records which platform-specific
    /// rename mechanism and directory-flush outcome this call actually used.
    ///
    /// # Errors
    /// Returns busy, stale, path, size, resource, or I/O errors. Both snapshot
    /// and replacement text must fit the store's file limit. The serialized
    /// journal must fit six times that limit plus 65,536 metadata bytes, accounting
    /// for JSON escaping and the root-relative path before any journal is written.
    /// Noncooperating writers can
    /// still race the final comparison and rename; callers must coordinate them.
    pub fn commit(&self, before: &Snapshot, replacement: &str) -> Result<Commit, Error> {
        if u64::try_from(before.text.len()).unwrap_or(u64::MAX) > self.max_bytes {
            return Err(Error::Invalid("snapshot exceeds file limit".into()));
        }
        if u64::try_from(replacement.len()).unwrap_or(u64::MAX) > self.max_bytes {
            return Err(Error::Invalid("replacement exceeds file limit".into()));
        }
        if Digest::of(before.text.as_bytes()) != before.digest {
            return Err(Error::Invalid(
                "snapshot text does not match its digest".into(),
            ));
        }
        let _lock = self.lock()?;
        let current = self.read(&before.path)?;
        let after = Digest::of(replacement.as_bytes());
        let patch = Patch::diff(
            &before.text,
            replacement,
            &mut Governor::new(self.patch_budget()),
        )
        .map_err(Error::from)?;
        let key = object([
            ("version", integer(JOURNAL_VERSION)),
            ("path", string(&before.path)),
            ("before", string(before.digest.to_string())),
            ("after", string(after.to_string())),
            ("patch", patch.to_node()),
        ])
        .to_json();
        if u64::try_from(key.len()).unwrap_or(u64::MAX) > self.journal_limit() {
            return Err(Error::Invalid(
                "serialized journal exceeds metadata limit".into(),
            ));
        }
        let id = Digest::of(key.as_bytes())
            .to_string()
            .replace("sha256:", "");
        let directory = self.journal.join(&id);
        if directory.exists() {
            let metadata = fs::symlink_metadata(&directory)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::Invalid("journal must be a real directory".into()));
            }
            if directory.join("committed").exists() {
                let entry = self.entry(&id)?;
                let same_path = entry.path == before.path;
                let same_versions = entry.before == before.text && entry.after == replacement;
                if !same_path || !same_versions {
                    return Err(Error::Invalid(
                        "transaction identity collision or corrupt journal".into(),
                    ));
                }
            }
            if current.digest != before.digest && current.digest != after {
                return Err(Error::Stale);
            }
        } else {
            if current.digest != before.digest {
                return Err(Error::Stale);
            }
            create_private_directory(&directory)?;
        }
        // A retry may finish a prefix left by a failed write before preparation.
        // Divergent or symlinked artifacts remain errors, never overwritten.
        prepare_piece(&directory.join("before"), before.text.as_bytes())?;
        fire(CrashPoint::AfterJournalBefore);
        prepare_piece(&directory.join("after"), replacement.as_bytes())?;
        fire(CrashPoint::AfterJournalAfter);
        prepare_piece(&directory.join("prepared.json"), key.as_bytes())?;
        fire(CrashPoint::AfterJournalPrepared);
        let mut directory_sync = sync_directory(&directory)?;
        directory_sync = worse(directory_sync, sync_directory(&self.journal)?);
        directory_sync = worse(directory_sync, sync_directory(&self.root)?);
        fire(CrashPoint::AfterJournalSync);
        let rename = if current.digest == after {
            RenameMethod::AlreadyCurrent
        } else {
            let (method, replace_sync) =
                self.replace(&before.path, before.digest, replacement, &id)?;
            directory_sync = worse(directory_sync, replace_sync);
            method
        };
        fire(CrashPoint::BeforeCommittedMarker);
        if !directory.join("committed").exists() {
            write_new(&directory.join("committed"), b"1\n")?;
        }
        directory_sync = worse(directory_sync, sync_directory(&directory)?);
        Ok(Commit {
            id,
            path: before.path.clone(),
            before: before.digest,
            after,
            durability: Durability {
                rename,
                directory: directory_sync,
            },
        })
    }
    /// Undo only if the current file still contains the exact committed output.
    /// The undo itself is journaled as a new conditional transaction.
    ///
    /// Constructs its replacement by governed-[`Patch::invert`]ing the exact
    /// patch [`Store::commit`] recorded (from the journaled `before` bytes),
    /// then governed-[`Patch::apply`]ing that inverse to the current file
    /// text; that apply's own stale-source check is what validates the
    /// current file against the inverse's expected source digest, not an
    /// independent comparison.
    ///
    /// # Errors
    /// Rejects modified output, tampered/incomplete journals, a recorded
    /// patch that does not invert against its own `before` bytes, and
    /// ordinary commit failures. Journal parsing uses allowances derived from
    /// its admitted serialized size; resource exhaustion remains [`Error::Resource`]
    /// and never becomes evidence that the current file is stale.
    pub fn undo(&self, transaction: &str) -> Result<Commit, Error> {
        let entry = self.entry(transaction)?;
        let current = self.read(&entry.path)?;
        let inverse = entry
            .patch
            .invert(&entry.before, &mut Governor::new(self.patch_budget()))
            .map_err(Error::from)?;
        let result = inverse.apply(&current.text, self.patch_budget());
        let Outcome::Resolved(applied) = result.outcome else {
            // Successful inversion constructed valid ranges against the exact
            // recorded output. Its only invalid-input refusal here is a changed
            // source digest; resource failures retain their own typed outcome.
            return Err(match result.receipt.stopped {
                StopReason::InvalidInput(_) => Error::Stale,
                reason => Error::from(reason),
            });
        };
        self.commit(&current, &applied.text)
    }
    /// A resource budget generous enough to diff, invert, or apply a patch
    /// over any file this store admits (bounded by `max_bytes`), scaled to
    /// that same limit rather than a fixed effort preset.
    const fn patch_budget(&self) -> Budget {
        Budget {
            work: self.max_bytes.saturating_mul(4).saturating_add(1_000_000),
            candidates: 1,
            input_bytes: self.max_bytes.saturating_add(1024),
            scratch_bytes: self.max_bytes.saturating_mul(8).saturating_add(1_000_000),
            depth: 8,
            evidence_entries: 1024,
            helper_calls: 0,
        }
    }
    const fn journal_limit(&self) -> u64 {
        self.max_bytes
            .saturating_mul(6)
            .saturating_add(JOURNAL_METADATA_ALLOWANCE)
    }
    const fn journal_budget(serialized_bytes: u64) -> Budget {
        Budget {
            // Parsing charges every consumed byte exactly once. The fixed
            // journal structure and geometric decoded-string capacity fit the
            // actual serialized length plus this fixed structural allowance.
            work: serialized_bytes,
            candidates: 1,
            input_bytes: serialized_bytes,
            scratch_bytes: serialized_bytes.saturating_mul(4).saturating_add(16_384),
            depth: 8,
            evidence_entries: 0,
            helper_calls: 0,
        }
    }
    /// Reconcile prepared journals with actual file versions without overwriting files.
    ///
    /// Temporary cleanup requires a validated journal, the exact temporary path
    /// beside its recorded target, and a bounded byte prefix of its replacement.
    /// Empty, partial UTF-8, and complete replacement bytes are compatible prefixes.
    /// Unknown identities remain [`Recovery::OrphanUnknown`]; invalid journals,
    /// symlinks, other locations, and incompatible bytes remain
    /// [`Recovery::OrphanPreserved`] with an explanation.
    ///
    /// # Errors
    /// Fails on store-lock contention, journal-directory enumeration, orphan
    /// scanning, orphan removal/directory sync, or governed journal resource errors. Resource
    /// exhaustion returns [`Error::Resource`] rather than classifying an otherwise
    /// unexamined transaction as corrupt. Corrupt/unreadable individual
    /// transactions and failed marker writes are retained as [`Recovery::Corrupt`]
    /// outcomes while other transactions continue.
    pub fn recover(&self) -> Result<Vec<Recovery>, Error> {
        let _lock = self.lock()?;
        let mut results = Vec::new();
        for directory in fs::read_dir(&self.journal)? {
            let directory = directory?;
            if !directory.file_type()?.is_dir() {
                continue;
            }
            let id = match directory.file_name().into_string() {
                Ok(id) => id,
                Err(raw) => {
                    results.push(Recovery::Corrupt {
                        id: raw.to_string_lossy().into_owned(),
                        reason: "journal identity is not UTF-8".into(),
                    });
                    continue;
                }
            };
            if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                results.push(Recovery::Corrupt {
                    id,
                    reason: "journal directory name is not a transaction identity".into(),
                });
                continue;
            }
            if !directory.path().join("prepared.json").exists() {
                results.push(Recovery::NotApplied(id));
                continue;
            }
            let outcome = (|| -> Result<Recovery, Error> {
                let entry = self.entry(&id)?;
                let current = self.read(&entry.path)?;
                if current.digest == Digest::of(entry.after.as_bytes()) {
                    if !directory.path().join("committed").exists() {
                        write_new(&directory.path().join("committed"), b"1\n")?;
                        sync_directory(&directory.path())?;
                    }
                    Ok(Recovery::Finalized(id.clone()))
                } else if current.digest == Digest::of(entry.before.as_bytes()) {
                    Ok(Recovery::NotApplied(id.clone()))
                } else {
                    Ok(Recovery::Conflict(id.clone()))
                }
            })();
            results.push(match outcome {
                Ok(recovery) => recovery,
                Err(error @ Error::Resource(_)) => return Err(error),
                Err(error) => Recovery::Corrupt {
                    id,
                    reason: error.to_string(),
                },
            });
        }
        for orphan in self.scan_orphans()? {
            results.push(self.recover_orphan(&orphan)?);
        }
        results.sort_by(|a, b| recovery_id(a).cmp(recovery_id(b)));
        Ok(results)
    }
    fn recover_orphan(&self, orphan: &Orphan) -> Result<Recovery, Error> {
        let relative = orphan
            .path
            .strip_prefix(&self.root)
            .map_err(|_| Error::Invalid("temporary artifact is outside the store root".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        let directory = self.journal.join(&orphan.id);
        let metadata = match fs::symlink_metadata(&directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Recovery::OrphanUnknown(relative));
            }
            Err(error) => {
                return Ok(Recovery::OrphanPreserved {
                    path: relative,
                    reason: format!("journal metadata could not be read: {error}"),
                });
            }
        };
        let checked = (|| -> Result<PathBuf, Error> {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::Invalid("journal must be a real directory".into()));
            }
            let entry = self.entry(&orphan.id)?;
            let target = self.resolve(&entry.path)?;
            let parent = target
                .parent()
                .ok_or_else(|| Error::Invalid("recorded target has no parent".into()))?;
            let expected = parent.join(format!(".purrassist-{}.tmp", orphan.id));
            if orphan.path != expected {
                return Err(Error::Invalid(
                    "temporary path does not match the recorded target parent".into(),
                ));
            }
            let pending = read_bytes_bounded(&orphan.path, self.max_bytes)?;
            if !entry.after.as_bytes().starts_with(&pending) {
                return Err(Error::Invalid(
                    "temporary contents are not a byte prefix of the journaled replacement".into(),
                ));
            }
            Ok(parent.to_path_buf())
        })();
        let parent = match checked {
            Ok(parent) => parent,
            Err(error @ Error::Resource(_)) => return Err(error),
            Err(error) => {
                return Ok(Recovery::OrphanPreserved {
                    path: relative,
                    reason: error.to_string(),
                });
            }
        };
        fs::remove_file(&orphan.path)?;
        sync_directory(&parent)?;
        Ok(Recovery::OrphanRemoved(orphan.id.clone()))
    }
    fn resolve(&self, relative: &str) -> Result<PathBuf, Error> {
        let path = Path::new(relative);
        if relative.is_empty()
            || relative.contains(['\\', '\0', ':'])
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || relative
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".." | ".purrassist"))
        {
            return Err(Error::Invalid(
                "target must be a root-relative path without traversal or reserved components"
                    .into(),
            ));
        }
        let mut current = self.root.clone();
        for component in path.components() {
            current.push(component);
            if fs::symlink_metadata(&current)?.file_type().is_symlink() {
                return Err(Error::Invalid("symlink targets are not accepted".into()));
            }
        }
        if !current.is_file() {
            return Err(Error::Invalid("target is not a regular file".into()));
        }
        Ok(current)
    }
    fn lock(&self) -> Result<WriterLock, Error> {
        let path = self.journal.join("writer.lock");
        if let Ok(metadata) = fs::symlink_metadata(&path)
            && metadata.file_type().is_symlink()
        {
            return Err(Error::Invalid("writer lock cannot be a symlink".into()));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => Error::Busy,
            std::fs::TryLockError::Error(error) => Error::Io(error),
        })?;
        Ok(WriterLock(file))
    }
    /// Write the replacement bytes into a sibling temporary file and make it
    /// visible at `target`, returning which rename mechanism completed and
    /// whether the containing directory's own flush actually succeeded.
    fn replace(
        &self,
        relative: &str,
        expected: Digest,
        replacement: &str,
        id: &str,
    ) -> Result<(RenameMethod, DirectorySync), Error> {
        let target = self.resolve(relative)?;
        let parent = target
            .parent()
            .ok_or_else(|| Error::Invalid("target has no parent".into()))?;
        let temporary = parent.join(format!(".purrassist-{id}.tmp"));
        match fs::symlink_metadata(&temporary) {
            Ok(_) => {
                let pending = read_bytes_bounded(&temporary, self.max_bytes)?;
                if !replacement.as_bytes().starts_with(&pending) {
                    return Err(Error::Invalid(
                        "temporary file differs from its prepared transaction".into(),
                    ));
                }
                fs::remove_file(&temporary)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        windows::prepare_temporary_options(&mut options);
        let mut file = options.open(&temporary)?;
        #[cfg(unix)]
        file.set_permissions(fs::metadata(&target)?.permissions())?;
        file.write_all(replacement.as_bytes())?;
        file.sync_all()?;
        fire(CrashPoint::AfterTempWrite);
        if self.read(relative)?.digest != expected {
            fs::remove_file(&temporary)?;
            return Err(Error::Stale);
        }
        fire(CrashPoint::BeforeRename);
        #[cfg(windows)]
        let method = windows::replace(file, &temporary, &target)?;
        // Every other platform, including wasm32-wasip1, uses the portable
        // `rename(2)`-equivalent; only directory durability (`sync_directory`
        // below) is unproven off Unix/Windows and stays an explicit error there.
        #[cfg(not(windows))]
        let method = {
            fs::rename(&temporary, &target)?;
            RenameMethod::PosixSemantics
        };
        fire(CrashPoint::AfterRename);
        let directory_sync = sync_directory(parent)?;
        Ok((method, directory_sync))
    }
    /// Recursively find `.purrassist-<id>.tmp` replacement temporary files
    /// under the store root without following symlinks. A matching name only
    /// identifies a candidate; [`Self::recover_orphan`] establishes ownership.
    fn scan_orphans(&self) -> Result<Vec<Orphan>, Error> {
        let mut findings = Vec::new();
        self.walk_orphans(&self.root, &mut findings)?;
        Ok(findings)
    }
    fn walk_orphans(&self, directory: &Path, findings: &mut Vec<Orphan>) -> Result<(), Error> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path == self.journal {
                continue;
            }
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                self.walk_orphans(&path, findings)?;
                continue;
            }
            if !file_type.is_file() && !file_type.is_symlink() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Some(id) = orphan_id(&name) else {
                continue;
            };
            findings.push(Orphan { id, path });
        }
        Ok(())
    }
    fn entry(&self, id: &str) -> Result<Entry, Error> {
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Invalid("invalid transaction identity".into()));
        }
        let directory = self.journal.join(id);
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            return Err(Error::Invalid("journal cannot be a symlink".into()));
        }
        let source = read_bounded(&directory.join("prepared.json"), self.journal_limit())?;
        let mut governor = Governor::new(Self::journal_budget(
            u64::try_from(source.len()).unwrap_or(u64::MAX),
        ));
        let metadata = json::parse_with(&source, &mut governor).map_err(|error| match error {
            json::ParseFailure::Syntax(error) => Error::Invalid(error.to_string()),
            json::ParseFailure::Resource(reason) => Error::Resource(reason),
        })?;
        let field = |key| {
            metadata
                .get(key)
                .and_then(Node::as_str)
                .ok_or_else(|| Error::Invalid(format!("journal missing {key}")))
        };
        match metadata.get("version").map(|node| &node.value) {
            Some(Value::Number(number))
                if number.to_usize() == usize::try_from(JOURNAL_VERSION).ok() => {}
            Some(Value::Number(number)) => {
                return Err(Error::Invalid(format!(
                    "unsupported journal version {number:?}"
                )));
            }
            _ => return Err(Error::Invalid("journal missing a supported version".into())),
        }
        let before = read_bounded(&directory.join("before"), self.max_bytes)?;
        let after = read_bounded(&directory.join("after"), self.max_bytes)?;
        if field("before")? != Digest::of(before.as_bytes()).to_string()
            || field("after")? != Digest::of(after.as_bytes()).to_string()
            || Digest::of(source.as_bytes())
                .to_string()
                .trim_start_matches("sha256:")
                != id
        {
            return Err(Error::Invalid("journal digest mismatch".into()));
        }
        let patch_node = metadata
            .get("patch")
            .ok_or_else(|| Error::Invalid("journal missing patch".into()))?;
        let patch = decode_patch(patch_node, Digest::of(before.as_bytes()))?;
        let applied = patch.apply(&before, self.patch_budget());
        match applied.outcome {
            Outcome::Resolved(applied) if applied.text == after => {}
            Outcome::Resolved(_) => {
                return Err(Error::Invalid(
                    "journal patch does not produce its recorded replacement".into(),
                ));
            }
            _ => return Err(Error::from(applied.receipt.stopped)),
        }
        Ok(Entry {
            path: field("path")?.into(),
            before,
            after,
            patch,
        })
    }
}
struct Entry {
    path: String,
    before: String,
    after: String,
    /// The recorded `before`-to-`after` patch (see `JOURNAL_VERSION`);
    /// [`Store::undo`] inverts this rather than taking an independent path.
    patch: Patch,
}
/// Decode a `prepared.json` `"patch"` member into a [`Patch`], validated to
/// be bound to `expected_source` (the journal's own `before` piece digest).
fn decode_patch(node: &Node, expected_source: Digest) -> Result<Patch, Error> {
    let source = node
        .get("source")
        .and_then(Node::as_str)
        .ok_or_else(|| Error::Invalid("journal patch missing source".into()))?;
    if source != expected_source.to_string() {
        return Err(Error::Invalid(
            "journal patch source does not match its before piece".into(),
        ));
    }
    let Some(Node {
        value: Value::Array(edit_nodes),
        ..
    }) = node.get("edits")
    else {
        return Err(Error::Invalid("journal patch missing edits".into()));
    };
    let mut edits = Vec::with_capacity(edit_nodes.len());
    for edit in edit_nodes {
        let start = patch_offset(edit, "start")?;
        let end = patch_offset(edit, "end")?;
        let replacement = edit
            .get("replacement")
            .and_then(Node::as_str)
            .ok_or_else(|| Error::Invalid("journal patch edit missing replacement".into()))?;
        if start > end {
            return Err(Error::Invalid(
                "journal patch edit has a reversed range".into(),
            ));
        }
        edits.push(TextEdit {
            range: start..end,
            replacement: replacement.into(),
        });
    }
    Ok(Patch {
        source: expected_source,
        edits,
    })
}
fn patch_offset(node: &Node, key: &str) -> Result<usize, Error> {
    match node.get(key).map(|field| &field.value) {
        Some(Value::Number(number)) => number.to_usize(),
        _ => None,
    }
    .ok_or_else(|| Error::Invalid(format!("journal patch edit {key} is invalid")))
}
/// One `.purrassist-<id>.tmp` replacement temporary file found while
/// recovering. Neither its name nor its identity alone authorizes deletion.
struct Orphan {
    id: String,
    path: PathBuf,
}
/// Parse a `.purrassist-<id>.tmp` file name into its transaction id, or
/// `None` if the name does not match that exact convention.
fn orphan_id(name: &str) -> Option<String> {
    let id = name.strip_prefix(".purrassist-")?.strip_suffix(".tmp")?;
    (id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| id.to_string())
}
fn recovery_id(recovery: &Recovery) -> &str {
    match recovery {
        Recovery::Finalized(id)
        | Recovery::NotApplied(id)
        | Recovery::Conflict(id)
        | Recovery::OrphanRemoved(id)
        | Recovery::Corrupt { id, .. } => id,
        Recovery::OrphanUnknown(name) => name,
        Recovery::OrphanPreserved { path, .. } => path,
    }
}
fn read_bounded(path: &Path, limit: u64) -> Result<String, Error> {
    String::from_utf8(read_bytes_bounded(path, limit)?).map_err(|e| Error::Invalid(e.to_string()))
}
fn read_bytes_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(Error::Invalid("expected a bounded regular file".into()));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(Error::Invalid(
            "file grew beyond the admission limit".into(),
        ));
    }
    Ok(bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn prepare_piece(path: &Path, expected: &[u8]) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > u64::try_from(expected.len()).unwrap_or(u64::MAX)
            {
                return Err(Error::Invalid("invalid partial journal artifact".into()));
            }
            let mut actual = Vec::new();
            File::open(path)?
                .take(
                    u64::try_from(expected.len())
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .read_to_end(&mut actual)?;
            if actual == expected {
                // A previous write may have completed in memory but failed before
                // fsync. Flushing needs a writable handle: Windows
                // `FlushFileBuffers` rejects read-only handles with
                // `ERROR_ACCESS_DENIED`, unlike POSIX `fsync`.
                OpenOptions::new().write(true).open(path)?.sync_all()?;
                return Ok(());
            }
            if !expected.starts_with(&actual) {
                return Err(Error::Invalid(
                    "journal artifact differs from the proposed transaction".into(),
                ));
            }
            fs::remove_file(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    write_new(path, expected)
}
#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<DirectorySync, Error> {
    File::open(path)?.sync_all()?;
    Ok(DirectorySync::Flushed)
}
#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<DirectorySync, Error> {
    windows::sync_directory(path)
}
#[cfg(not(any(unix, windows)))]
fn sync_directory(_path: &Path) -> Result<DirectorySync, Error> {
    Err(Error::Invalid(
        "durable directory sync is not implemented on this platform".into(),
    ))
}

fn create_private_directory(path: &Path) -> Result<(), Error> {
    // Windows has no permission-bit concept to set here; the new directory
    // inherits its ACL from its parent (see docs/EDIT-APPLICATION.md).
    #[cfg(unix)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

/// Holds the cooperating-writer lock and releases it explicitly when dropped.
///
/// Closing a handle releases its lock on every platform, but Windows documents
/// that release-on-close happens asynchronously, so an immediate retry by the
/// same or another writer could still observe the lock as held. Unlocking
/// before the handle closes makes release synchronous everywhere.
struct WriterLock(File);
impl Drop for WriterLock {
    fn drop(&mut self) {
        // A drop cannot report failure. If the explicit unlock fails, closing
        // the handle immediately afterwards still releases the lock (the
        // pre-existing behavior), so no lock can outlive this guard's handle.
        let _ = self.0.unlock();
    }
}
