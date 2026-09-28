// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Filesystem transaction, stale-write, retry, recovery, and conditional-undo tests.
use purrassist_host::files::{Error, Recovery, Store};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "purrassist-files-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("source.txt"), "original\r\n").unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn commit_retry_undo_and_redo_preserve_exact_versions() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read("source.txt").unwrap();
    let commit = store.commit(&before, "changed\r\n").unwrap();
    assert_eq!(store.commit(&before, "changed\r\n").unwrap(), commit);
    assert_eq!(store.read("source.txt").unwrap().text, "changed\r\n");
    store.undo(&commit.id).unwrap();
    assert_eq!(store.read("source.txt").unwrap().text, before.text);
    store.commit(&before, "changed\r\n").unwrap();
    fs::write(scratch.0.join("source.txt"), "user edit").unwrap();
    assert!(matches!(store.undo(&commit.id), Err(Error::Stale)));
    assert!(matches!(store.commit(&before, "other"), Err(Error::Stale)));
    assert_eq!(store.read("source.txt").unwrap().text, "user edit");
}
#[test]
fn interrupted_final_marker_is_recovered_without_rewriting_file() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let commit = store
        .commit(&store.read("source.txt").unwrap(), "new")
        .unwrap();
    fs::remove_file(
        scratch
            .0
            .join(".purrassist")
            .join(&commit.id)
            .join("committed"),
    )
    .unwrap();
    assert_eq!(
        store.recover().unwrap(),
        vec![Recovery::Finalized(commit.id.clone())]
    );
    assert_eq!(store.read("source.txt").unwrap().text, "new");
    fs::write(scratch.0.join("source.txt"), "external").unwrap();
    assert_eq!(
        store.recover().unwrap(),
        vec![Recovery::Conflict(commit.id)]
    );
}
#[test]
fn batch_reports_success_and_stale_independently() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    fs::write(scratch.0.join("second.txt"), "second").unwrap();
    let first = store.read("source.txt").unwrap();
    let second = store.read("second.txt").unwrap();
    let results = store.commit_batch(&[
        (first.clone(), "first update".into()),
        (first, "stale update".into()),
        (second, "second update".into()),
    ]);
    assert!(results[0].result.is_ok());
    assert!(matches!(results[1].result, Err(Error::Stale)));
    assert!(results[2].result.is_ok());
    assert_eq!(store.read("source.txt").unwrap().text, "first update");
    assert_eq!(store.read("second.txt").unwrap().text, "second update");
}
#[test]
fn cooperating_lock_and_path_guards_fail_explicitly() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    for path in [
        "../escape",
        "/absolute",
        "./source.txt",
        ".purrassist/writer.lock",
    ] {
        assert!(store.read(path).is_err());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(scratch.0.join(".purrassist/writer.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(matches!(
        store.commit(&store.read("source.txt").unwrap(), "blocked"),
        Err(Error::Busy)
    ));
}
#[test]
fn retry_completes_partial_journal_and_partial_utf8_temporary() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read("source.txt").unwrap();
    let replacement = "猫 changed\r\n";
    let commit = store.commit(&before, replacement).unwrap();
    let journal = scratch.0.join(".purrassist").join(&commit.id);
    let prepared = fs::read(journal.join("prepared.json")).unwrap();
    // Recreate the persistent states of interruption at every journal write.
    for part in ["before", "after", "prepared.json"] {
        fs::write(scratch.0.join("source.txt"), &before.text).unwrap();
        fs::remove_file(journal.join("committed")).unwrap();
        let full = fs::read(journal.join(part)).unwrap();
        fs::write(journal.join(part), &full[..1]).unwrap();
        fs::write(
            scratch.0.join(format!(".purrassist-{}.tmp", commit.id)),
            &replacement.as_bytes()[..1],
        )
        .unwrap();
        assert_eq!(store.commit(&before, replacement).unwrap(), commit);
        assert_eq!(fs::read(journal.join("prepared.json")).unwrap(), prepared);
        assert_eq!(store.read("source.txt").unwrap().text, replacement);
    }
    fs::write(scratch.0.join("source.txt"), &before.text).unwrap();
    let temporary = scratch.0.join(format!(".purrassist-{}.tmp", commit.id));
    fs::write(&temporary, "unrelated content").unwrap();
    assert!(matches!(
        store.commit(&before, replacement),
        Err(Error::Invalid(_))
    ));
    assert_eq!(fs::read_to_string(&temporary).unwrap(), "unrelated content");
    assert_eq!(store.read("source.txt").unwrap().text, before.text);
}
#[test]
#[cfg(unix)]
fn symlinks_are_rejected_and_permissions_survive_replacement() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    symlink("source.txt", scratch.0.join("alias.txt")).unwrap();
    assert!(store.read("alias.txt").is_err());
    fs::set_permissions(
        scratch.0.join("source.txt"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    store
        .commit(&store.read("source.txt").unwrap(), "new")
        .unwrap();
    assert_eq!(
        fs::metadata(scratch.0.join("source.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(scratch.0.join(".purrassist"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn writer_lock_is_released_before_the_next_writer_arrives() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    for attempt in 0..500 {
        assert!(
            store.recover().is_ok(),
            "lock still held on back-to-back acquisition {attempt}"
        );
    }
}
