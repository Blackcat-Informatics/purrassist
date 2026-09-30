// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Admitted journal serialization, replay, undo, and explicit resource failures.
use purrassist::{StopReason, contract::Resource, digest::Digest};
use purrassist_host::files::{Error, Recovery, Snapshot, Store};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "purrassist-journal-limits-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("café.txt"), "original\r\n猫\n").unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn replay_and_undo(max_bytes: u64, replacement: &str) -> u64 {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, max_bytes).unwrap();
    let original = store.read("café.txt").unwrap();
    let committed = store.commit(&original, replacement).unwrap();
    let journal = scratch.0.join(".purrassist").join(&committed.id);
    let metadata_bytes = fs::metadata(journal.join("prepared.json")).unwrap().len();
    assert_eq!(store.read("café.txt").unwrap().text, replacement);

    // Reopen and replay the exact transaction, then simulate the final-marker
    // crash checkpoint against the real serialized journal left by commit.
    let reopened = Store::open(&scratch.0, max_bytes).unwrap();
    assert_eq!(reopened.commit(&original, replacement).unwrap(), committed);
    fs::remove_file(journal.join("committed")).unwrap();
    assert_eq!(
        reopened.recover().unwrap(),
        vec![Recovery::Finalized(committed.id.clone())]
    );
    assert_eq!(reopened.read("café.txt").unwrap().text, replacement);
    assert_eq!(reopened.commit(&original, replacement).unwrap(), committed);

    let undone = reopened.undo(&committed.id).unwrap();
    assert_eq!(undone.after, original.digest);
    assert_eq!(reopened.read("café.txt").unwrap().text, original.text);
    assert!(matches!(reopened.undo(&committed.id), Err(Error::Stale)));
    assert_eq!(reopened.commit(&original, replacement).unwrap(), committed);
    reopened.undo(&committed.id).unwrap();
    assert_eq!(reopened.read("café.txt").unwrap(), original);
    metadata_bytes
}

#[test]
fn escaped_replacement_replays_recovers_and_undoes_at_the_file_limit() {
    let max_bytes = 100_000;
    let replacement = "\u{1}".repeat(usize::try_from(max_bytes).unwrap());
    let metadata_bytes = replay_and_undo(max_bytes, &replacement);
    assert!(metadata_bytes > max_bytes + 65_536);
    assert!(metadata_bytes < max_bytes * 6 + 65_536);
}

#[test]
fn large_plain_journal_uses_its_actual_size_for_parser_admission() {
    let replacement = format!("{}\r\n猫\n", "a".repeat(1_100_000));
    let metadata_bytes = replay_and_undo(2_000_000, &replacement);
    assert!(metadata_bytes > purrassist::Budget::default().work);
}

#[test]
fn oversized_text_and_serialized_metadata_fail_without_changing_the_target() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 64).unwrap();
    let original = store.read("café.txt").unwrap();
    let oversized = "x".repeat(65);
    let supplied = Snapshot {
        path: original.path.clone(),
        text: oversized.clone(),
        digest: Digest::of(oversized.as_bytes()),
    };
    assert!(matches!(
        store.commit(&supplied, "replacement"),
        Err(Error::Invalid(message)) if message == "snapshot exceeds file limit"
    ));
    assert!(matches!(
        store.commit(&original, &oversized),
        Err(Error::Invalid(message)) if message == "replacement exceeds file limit"
    ));
    assert_eq!(store.read("café.txt").unwrap(), original);
    assert_eq!(
        fs::read_dir(scratch.0.join(".purrassist")).unwrap().count(),
        0
    );

    let committed = store.commit(&original, "replacement").unwrap();
    let metadata = scratch
        .0
        .join(".purrassist")
        .join(&committed.id)
        .join("prepared.json");
    fs::write(&metadata, " ".repeat(64 * 6 + 65_536 + 1)).unwrap();
    assert!(matches!(store.undo(&committed.id), Err(Error::Invalid(_))));
    assert!(matches!(
        store.commit(&original, "replacement"),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        store.recover().unwrap().as_slice(),
        [Recovery::Corrupt { id, .. }] if id == &committed.id
    ));
    assert_eq!(store.read("café.txt").unwrap().text, "replacement");
}

#[test]
fn parser_exhaustion_is_typed_and_is_not_staleness_or_journal_corruption() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let original = store.read("café.txt").unwrap();
    let committed = store.commit(&original, "replacement").unwrap();
    let metadata = scratch
        .0
        .join(".purrassist")
        .join(&committed.id)
        .join("prepared.json");
    // This admitted malformed record requires many more JSON nodes than a
    // real one-edit journal. Reject its scratch exhaustion before trusting it.
    fs::write(&metadata, format!("[{}0]", "0,".repeat(10_000))).unwrap();
    for result in [
        store.undo(&committed.id),
        store.commit(&original, "replacement"),
    ] {
        assert!(matches!(
            result,
            Err(Error::Resource(StopReason::BudgetExceeded(
                Resource::ScratchBytes
            )))
        ));
    }
    assert!(matches!(
        store.recover(),
        Err(Error::Resource(StopReason::BudgetExceeded(
            Resource::ScratchBytes
        )))
    ));
    assert_eq!(store.read("café.txt").unwrap().text, "replacement");
}

#[test]
fn same_size_metadata_tampering_retains_original_and_committed_bytes() {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.0, 4096).unwrap();
    let original = store.read("café.txt").unwrap();
    let committed = store.commit(&original, "replacement").unwrap();
    let journal = scratch.0.join(".purrassist").join(&committed.id);
    let metadata = journal.join("prepared.json");
    let tampered =
        fs::read_to_string(&metadata)
            .unwrap()
            .replacen("\"version\":2", "\"version\":3", 1);
    fs::write(&metadata, tampered).unwrap();
    assert!(matches!(store.undo(&committed.id), Err(Error::Invalid(_))));
    assert!(matches!(
        store.recover().unwrap().as_slice(),
        [Recovery::Corrupt { .. }]
    ));
    assert_eq!(
        fs::read_to_string(journal.join("before")).unwrap(),
        original.text
    );
    assert_eq!(
        fs::read_to_string(journal.join("after")).unwrap(),
        "replacement"
    );
    assert_eq!(store.read("café.txt").unwrap().text, "replacement");
}
