// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Coordinated filesystem transactions with persistent journals and conditional undo.
//!
//! The lock coordinates users of this store. It does not lock out arbitrary editors.
//! Atomic replacement covers one file; a batch is not an atomic multi-file commit.
//! Canonical root and symlink checks prevent accidental escapes; this portable API
//! is not a sandbox against malicious concurrent directory replacement.
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Deterministic transaction identity.
    pub id: String,
    /// Root-relative target.
    pub path: String,
    /// Original version.
    pub before: Digest,
    /// Written version.
    pub after: Digest,
}
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
/// Recovery observation for one prepared journal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// Replacement bytes are present; journal finalized.
    Finalized(String),
    /// Original bytes are present; no replacement is claimed.
    NotApplied(String),
    /// Neither version matches; caller must resolve the conflict.
    Conflict(String),
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
    /// not imply the target was untouched. Original permissions are preserved.
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
        prepare_piece(&directory.join("after"), replacement.as_bytes())?;
        prepare_piece(&directory.join("prepared.json"), key.as_bytes())?;
        sync_directory(&directory)?;
        sync_directory(&self.journal)?;
        sync_directory(&self.root)?;
        if current.digest != after {
            self.replace(&before.path, before.digest, replacement, &id)?;
        }
        if !directory.join("committed").exists() {
            write_new(&directory.join("committed"), b"1\n")?;
        }
        sync_directory(&directory)?;
        Ok(Commit {
            id,
            path: before.path.clone(),
            before: before.digest,
            after,
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
        for directory in fs::read_dir(&self.journal)? {
            let directory = directory?;
            if !directory.file_type()?.is_dir() {
                continue;
            }
            let id = directory
                .file_name()
                .into_string()
                .map_err(|_| Error::Invalid("journal identity is not UTF-8".into()))?;
            if !directory.path().join("prepared.json").exists() {
                results.push(Recovery::NotApplied(id));
                continue;
            }
            let entry = self.entry(&id)?;
            let current = self.read(&entry.path)?;
            if current.digest == Digest::of(entry.after.as_bytes()) {
                if !directory.path().join("committed").exists() {
                    write_new(&directory.path().join("committed"), b"1\n")?;
                    sync_directory(&directory.path())?;
                }
                results.push(Recovery::Finalized(id));
            } else if current.digest == Digest::of(entry.before.as_bytes()) {
                results.push(Recovery::NotApplied(id));
            } else {
                results.push(Recovery::Conflict(id));
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
    fn replace(
        &self,
        relative: &str,
        expected: Digest,
        replacement: &str,
        id: &str,
    ) -> Result<(), Error> {
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
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.set_permissions(fs::metadata(&target)?.permissions())?;
        let mut file = file;
        file.write_all(replacement.as_bytes())?;
        file.sync_all()?;
        if self.read(relative)?.digest != expected {
            fs::remove_file(&temporary)?;
            return Err(Error::Stale);
        }
        fs::rename(&temporary, &target)?;
        sync_directory(parent)?;
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
fn recovery_id(recovery: &Recovery) -> &str {
    match recovery {
        Recovery::Finalized(id) | Recovery::NotApplied(id) | Recovery::Conflict(id) => id,
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
fn sync_directory(path: &Path) -> Result<(), Error> {
    File::open(path)?.sync_all()?;
    Ok(())
}
#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), Error> {
    Err(Error::Invalid(
        "durable directory sync is not implemented on this platform".into(),
    ))
}

fn create_private_directory(path: &Path) -> Result<(), Error> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
