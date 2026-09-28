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
//! violations a bounded number of times before reporting [`Error::Busy`], and
//! records the mechanism actually used in [`Commit::durability`]. See
//! `docs/EDIT-APPLICATION.md` for the full design.
use purrassist::{
    digest::Digest,
    json::{self, Node},
    wire::{object, string},
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
    /// Another cooperating writer holds the store lock.
    Busy,
    /// Current file differs from the expected version.
    Stale,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Invalid(message) => f.write_str(message),
            Self::Busy => f.write_str("store is locked by another writer"),
            Self::Stale => f.write_str("file changed since the expected snapshot"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
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
fn worse(a: DirectorySync, b: DirectorySync) -> DirectorySync {
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
    /// A leftover `.purrassist-<id>.tmp` replacement temporary file whose id
    /// matched a known journal was removed as evidence-bearing cleanup.
    OrphanRemoved(String),
    /// A leftover `.purrassist-<id>.tmp` replacement temporary file matched
    /// no known journal and was left in place; the path is root-relative
    /// with forward-slash separators on every platform.
    OrphanUnknown(String),
}

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
    /// Returns busy, stale, path, size, or I/O errors. Noncooperating writers can
    /// still race the final comparison and rename; callers must coordinate them.
    pub fn commit(&self, before: &Snapshot, replacement: &str) -> Result<Commit, Error> {
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
        let key = object([
            ("path", string(&before.path)),
            ("before", string(before.digest.to_string())),
            ("after", string(after.to_string())),
        ])
        .to_json();
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
        let rename = if current.digest != after {
            let (method, replace_sync) =
                self.replace(&before.path, before.digest, replacement, &id)?;
            directory_sync = worse(directory_sync, replace_sync);
            method
        } else {
            RenameMethod::AlreadyCurrent
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
    /// # Errors
    /// Rejects modified output, tampered/incomplete journals, and ordinary commit failures.
    pub fn undo(&self, transaction: &str) -> Result<Commit, Error> {
        let entry = self.entry(transaction)?;
        let current = self.read(&entry.path)?;
        if current.digest != Digest::of(entry.after.as_bytes()) {
            return Err(Error::Stale);
        }
        self.commit(&current, &entry.before)
    }
    /// Reconcile prepared journals with actual file versions without overwriting files.
    ///
    /// # Errors
    /// Fails on lock contention, tampered journals, unreadable entries, or marker writes.
    pub fn recover(&self) -> Result<Vec<Recovery>, Error> {
        let _lock = self.lock()?;
        let mut results = Vec::new();
        let mut known_ids = Vec::new();
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
                    id: id.clone(),
                    reason: "journal directory name is not a transaction identity".into(),
                });
                continue;
            }
            known_ids.push(id.clone());
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
            results.push(outcome.unwrap_or_else(|error| Recovery::Corrupt {
                id: id.clone(),
                reason: error.to_string(),
            }));
        }
        for orphan in self.scan_orphans(&known_ids)? {
            match orphan {
                Orphan::Known { id, path } => {
                    fs::remove_file(&path)?;
                    results.push(Recovery::OrphanRemoved(id));
                }
                Orphan::Unknown { name } => results.push(Recovery::OrphanUnknown(name)),
            }
        }
        results.sort_by(|a, b| recovery_id(a).cmp(recovery_id(b)));
        Ok(results)
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
    fn lock(&self) -> Result<File, Error> {
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
        Ok(file)
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
    /// under the store root, classifying each by whether its id matches a
    /// known journal directory.
    fn scan_orphans(&self, known: &[String]) -> Result<Vec<Orphan>, Error> {
        let mut findings = Vec::new();
        self.walk_orphans(&self.root, known, &mut findings)?;
        Ok(findings)
    }
    fn walk_orphans(
        &self,
        directory: &Path,
        known: &[String],
        findings: &mut Vec<Orphan>,
    ) -> Result<(), Error> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path == self.journal {
                continue;
            }
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                self.walk_orphans(&path, known, findings)?;
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Some(id) = orphan_id(&name) else {
                continue;
            };
            if known.iter().any(|candidate| candidate == &id) {
                findings.push(Orphan::Known { id, path });
            } else {
                let relative = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                findings.push(Orphan::Unknown { name: relative });
            }
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
        let source = read_bounded(&directory.join("prepared.json"), 65_536)?;
        let metadata = json::parse(&source).map_err(|e| Error::Invalid(e.to_string()))?;
        let field = |key| {
            metadata
                .get(key)
                .and_then(Node::as_str)
                .ok_or_else(|| Error::Invalid(format!("journal missing {key}")))
        };
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
        Ok(Entry {
            path: field("path")?.into(),
            before,
            after,
        })
    }
}
struct Entry {
    path: String,
    before: String,
    after: String,
}
/// One `.purrassist-<id>.tmp` replacement temporary file found while
/// recovering, classified by whether its id matches a known journal.
enum Orphan {
    /// Matches a known journal directory; safe to remove.
    Known { id: String, path: PathBuf },
    /// Matches no known journal; reported but never deleted.
    Unknown { name: String },
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
        | Recovery::OrphanRemoved(id) => id,
        Recovery::Corrupt { id, .. } => id,
        Recovery::OrphanUnknown(name) => name,
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
                // A previous write may have completed in memory but failed before fsync.
                File::open(path)?.sync_all()?;
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
