// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Crash-recovery proofs: re-executing this test binary as a child process
//! that aborts at a named checkpoint inside `Store::commit`, then asserting
//! that `Store::recover` and a retry produce exact-version outcomes.
//!
//! This file runs unchanged on Unix and Windows: crash injection uses only
//! `std::process::abort`, environment variables, and `std::process::Command`,
//! none of which need platform gating.
//!
//! Every test that touches a `Store` calls [`serialize`] first. `cargo test`
//! runs this file's tests concurrently by default, and on Unix
//! `std::process::Command::spawn` forks the whole process: a fork landing
//! while another thread is mid-`flock` on its own (unrelated) journal lock
//! file can make that unrelated `Store::recover`/`Store::commit` call
//! observe a spurious `Error::Busy`. Serializing this file's tests removes
//! that race without touching `Store`'s locking, which is correct — a real
//! concurrent writer legitimately gets `Busy`.
use purrassist::digest::Digest;
use purrassist_host::files::{CrashPoint, Recovery, Snapshot, Store, install_crash_hook};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

static SERIALIZE: Mutex<()> = Mutex::new(());
/// Serialize this file's `Store`-touching tests against each other; see the
/// module doc for why.
fn serialize() -> MutexGuard<'static, ()> {
    SERIALIZE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const FILE_NAME: &str = "target.txt";
const BEFORE_TEXT: &str = "before state\r\n";
const AFTER_TEXT: &str = "after café state\r\n";

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "purrassist-crash-recovery-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join(FILE_NAME), BEFORE_TEXT).unwrap();
        Self(path)
    }
    fn original_before() -> Snapshot {
        Snapshot {
            path: FILE_NAME.to_string(),
            text: BEFORE_TEXT.to_string(),
            digest: Digest::of(BEFORE_TEXT.as_bytes()),
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

/// The child-process worker: aborts exactly at the checkpoint named by
/// `PURRASSIST_ABORT_POINT`, operating on the store rooted at
/// `PURRASSIST_SCRATCH`. Run normally (no env vars set) this is a no-op, so
/// it is a harmless, ordinary passing test in every other `cargo test` run.
#[test]
fn crash_point_worker() {
    let Ok(point_name) = std::env::var("PURRASSIST_ABORT_POINT") else {
        return;
    };
    let scratch = std::env::var("PURRASSIST_SCRATCH").expect("PURRASSIST_SCRATCH must be set");
    let point = CrashPoint::ALL
        .into_iter()
        .find(|candidate| format!("{candidate:?}") == point_name)
        .unwrap_or_else(|| panic!("unknown crash point name: {point_name}"));
    TARGET.set(point).expect("target installed exactly once");
    install_crash_hook(abort_at_target);
    let store = Store::open(&scratch, 4096).expect("open store");
    let before = Snapshot {
        path: FILE_NAME.to_string(),
        text: BEFORE_TEXT.to_string(),
        digest: Digest::of(BEFORE_TEXT.as_bytes()),
    };
    // If the requested checkpoint never fires, this simply completes; the
    // driver test asserts the child process did not exit successfully, so a
    // checkpoint that fails to fire is caught there, not swallowed here.
    let _ = store.commit(&before, AFTER_TEXT);
}
static TARGET: OnceLock<CrashPoint> = OnceLock::new();
fn abort_at_target(point: CrashPoint) {
    if TARGET.get() == Some(&point) {
        std::process::abort();
    }
}

struct Expected {
    text: &'static str,
    finalized: bool,
    orphan: bool,
}
const fn expected_outcome(point: CrashPoint) -> Expected {
    match point {
        CrashPoint::AfterJournalBefore
        | CrashPoint::AfterJournalAfter
        | CrashPoint::AfterJournalPrepared
        | CrashPoint::AfterJournalSync => Expected {
            text: BEFORE_TEXT,
            finalized: false,
            orphan: false,
        },
        CrashPoint::AfterTempWrite | CrashPoint::BeforeRename => Expected {
            text: BEFORE_TEXT,
            finalized: false,
            orphan: true,
        },
        CrashPoint::AfterRename | CrashPoint::BeforeCommittedMarker => Expected {
            text: AFTER_TEXT,
            finalized: true,
            orphan: false,
        },
    }
}

#[test]
fn every_crash_point_yields_exact_version_and_recovers() {
    let _serial = serialize();
    for point in CrashPoint::ALL {
        run_crash_point(point);
    }
}

fn run_crash_point(point: CrashPoint) {
    let scratch = Scratch::new();
    let exe = std::env::current_exe().expect("current test binary path");
    let status = Command::new(&exe)
        .args(["crash_point_worker", "--exact"])
        .env("PURRASSIST_ABORT_POINT", format!("{point:?}"))
        .env("PURRASSIST_SCRATCH", &scratch.0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("spawn child worker");
    assert!(
        !status.success(),
        "worker did not abort at checkpoint {point:?} (status: {status:?})"
    );

    let expected = expected_outcome(point);
    let contents = std::fs::read(scratch.0.join(FILE_NAME)).unwrap();
    assert!(
        contents == expected.text.as_bytes(),
        "checkpoint {point:?} left a torn write: {contents:?}"
    );

    let store = Store::open(&scratch.0, 4096).unwrap();
    let recovered = store
        .recover()
        .unwrap_or_else(|error| panic!("checkpoint {point:?}: recovery failed: {error}"));
    assert!(
        !recovered
            .iter()
            .any(|r| matches!(r, Recovery::Corrupt { .. } | Recovery::Conflict(_))),
        "checkpoint {point:?}: unexpected classification: {recovered:?}"
    );
    if expected.finalized {
        assert!(
            recovered
                .iter()
                .any(|r| matches!(r, Recovery::Finalized(_))),
            "checkpoint {point:?}: expected Finalized, got {recovered:?}"
        );
    } else {
        assert!(
            recovered
                .iter()
                .any(|r| matches!(r, Recovery::NotApplied(_))),
            "checkpoint {point:?}: expected NotApplied, got {recovered:?}"
        );
    }
    if expected.orphan {
        assert!(
            recovered
                .iter()
                .any(|r| matches!(r, Recovery::OrphanRemoved(_))),
            "checkpoint {point:?}: expected an orphan to be removed, got {recovered:?}"
        );
    } else {
        assert!(
            !recovered
                .iter()
                .any(|r| matches!(r, Recovery::OrphanRemoved(_) | Recovery::OrphanUnknown(_))),
            "checkpoint {point:?}: unexpected orphan finding: {recovered:?}"
        );
    }

    // Retrying the original transaction (regardless of which partial state
    // the crash left) must complete idempotently and land on exactly AFTER.
    let original_before = Scratch::original_before();
    let commit = store
        .commit(&original_before, AFTER_TEXT)
        .unwrap_or_else(|error| panic!("checkpoint {point:?}: retry commit failed: {error}"));
    assert_eq!(
        std::fs::read(scratch.0.join(FILE_NAME)).unwrap(),
        AFTER_TEXT.as_bytes(),
        "checkpoint {point:?}: retry did not converge on the exact after version"
    );
    let commit_again = store
        .commit(&original_before, AFTER_TEXT)
        .unwrap_or_else(|error| panic!("checkpoint {point:?}: repeated commit failed: {error}"));
    assert_eq!(
        commit, commit_again,
        "checkpoint {point:?}: retry was not idempotent"
    );
}

#[test]
fn missing_prepared_json_reports_not_applied() {
    let _serial = serialize();
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read(FILE_NAME).unwrap();
    let commit = store.commit(&before, AFTER_TEXT).unwrap();
    let commit_dir = scratch.0.join(".purrassist").join(&commit.id);
    std::fs::remove_file(commit_dir.join("prepared.json")).unwrap();
    // A crash before `prepared.json` exists can never have replaced the
    // target (replace only runs after all three journal pieces are
    // written and synced), so this reconstructs a faithful before-state.
    std::fs::write(scratch.0.join(FILE_NAME), BEFORE_TEXT).unwrap();
    let recovered = store.recover().unwrap();
    assert!(
        recovered
            .iter()
            .any(|r| matches!(r, Recovery::NotApplied(id) if id == &commit.id)),
        "{recovered:?}"
    );
}

#[test]
fn torn_prepared_json_reports_corrupt() {
    let _serial = serialize();
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read(FILE_NAME).unwrap();
    let commit = store.commit(&before, AFTER_TEXT).unwrap();
    let commit_dir = scratch.0.join(".purrassist").join(&commit.id);
    let prepared = std::fs::read(commit_dir.join("prepared.json")).unwrap();
    std::fs::write(
        commit_dir.join("prepared.json"),
        &prepared[..prepared.len() / 2],
    )
    .unwrap();
    let recovered = store.recover().unwrap();
    assert!(
        recovered
            .iter()
            .any(|r| matches!(r, Recovery::Corrupt { id, .. } if id == &commit.id)),
        "{recovered:?}"
    );
}

#[test]
fn truncated_before_piece_reports_corrupt() {
    let _serial = serialize();
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read(FILE_NAME).unwrap();
    let commit = store.commit(&before, AFTER_TEXT).unwrap();
    let commit_dir = scratch.0.join(".purrassist").join(&commit.id);
    let original = std::fs::read(commit_dir.join("before")).unwrap();
    std::fs::write(commit_dir.join("before"), &original[..original.len() - 1]).unwrap();
    let recovered = store.recover().unwrap();
    assert!(
        recovered
            .iter()
            .any(|r| matches!(r, Recovery::Corrupt { id, .. } if id == &commit.id)),
        "{recovered:?}"
    );
}

#[test]
fn truncated_after_piece_reports_corrupt() {
    let _serial = serialize();
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read(FILE_NAME).unwrap();
    let commit = store.commit(&before, AFTER_TEXT).unwrap();
    let commit_dir = scratch.0.join(".purrassist").join(&commit.id);
    let original = std::fs::read(commit_dir.join("after")).unwrap();
    std::fs::write(commit_dir.join("after"), &original[..original.len() - 1]).unwrap();
    let recovered = store.recover().unwrap();
    assert!(
        recovered
            .iter()
            .any(|r| matches!(r, Recovery::Corrupt { id, .. } if id == &commit.id)),
        "{recovered:?}"
    );
}

#[test]
fn known_orphan_is_removed_and_reported() {
    let _serial = serialize();
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let before = store.read(FILE_NAME).unwrap();
    let commit = store.commit(&before, AFTER_TEXT).unwrap();
    let orphan_path = scratch.0.join(format!(".purrassist-{}.tmp", commit.id));
    std::fs::write(&orphan_path, "leftover").unwrap();
    let recovered = store.recover().unwrap();
    assert!(
        recovered
            .iter()
            .any(|r| matches!(r, Recovery::OrphanRemoved(id) if id == &commit.id)),
        "{recovered:?}"
    );
    assert!(!orphan_path.exists(), "known orphan must be removed");
}

#[test]
fn unknown_orphan_is_reported_and_not_removed() {
    let _serial = serialize();
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let fake_id = "0".repeat(64);
    let orphan_name = format!(".purrassist-{fake_id}.tmp");
    let orphan_path = scratch.0.join(&orphan_name);
    std::fs::write(&orphan_path, "leftover").unwrap();
    let recovered = store.recover().unwrap();
    assert!(
        recovered
            .iter()
            .any(|r| matches!(r, Recovery::OrphanUnknown(name) if name == &orphan_name)),
        "{recovered:?}"
    );
    assert!(orphan_path.exists(), "unknown orphan must not be removed");
}
