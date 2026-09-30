// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Orphan cleanup requires validated ownership, location, and compatible bytes.
use purrassist::{
    Schema, StopReason,
    contract::Resource,
    digest::Digest,
    json::Value,
    wire::{array, integer, object, string},
};
use purrassist_host::{
    files::{Commit, Error, Recovery, Snapshot, Store},
    tools::Tools,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const TARGET: &str = "nested/target.txt";
const AFTER: &str = "猫 committed\r\n";
static SERIAL: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    store: Store,
    original: Snapshot,
    commit: Commit,
}
impl Fixture {
    fn new(replacement: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "purrassist-orphan-preservation-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::create_dir(root.join("other")).unwrap();
        fs::write(root.join(TARGET), "original\r\n").unwrap();
        let store = Store::open(&root, 4096).unwrap();
        let original = store.read(TARGET).unwrap();
        let commit = store.commit(&original, replacement).unwrap();
        Self {
            root,
            store,
            original,
            commit,
        }
    }
    fn journal(&self) -> PathBuf {
        self.root.join(".purrassist").join(&self.commit.id)
    }
    fn temporary(&self, parent: &str, id: &str) -> PathBuf {
        self.root.join(parent).join(format!(".purrassist-{id}.tmp"))
    }
    fn relative(&self, path: &std::path::Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn compatible_empty_partial_utf8_and_complete_temporaries_recover_and_retry() {
    for replacement in [AFTER, ""] {
        let lengths = if replacement.is_empty() {
            vec![0]
        } else {
            vec![0, 1, 2, replacement.len() - 1, replacement.len()]
        };
        for length in lengths {
            let fixture = Fixture::new(replacement);
            fs::write(fixture.root.join(TARGET), &fixture.original.text).unwrap();
            fs::remove_file(fixture.journal().join("committed")).unwrap();
            let temporary = fixture.temporary("nested", &fixture.commit.id);
            fs::write(&temporary, &replacement.as_bytes()[..length]).unwrap();
            let recovered = fixture.store.recover().unwrap();
            assert!(recovered.contains(&Recovery::NotApplied(fixture.commit.id.clone())));
            assert!(recovered.contains(&Recovery::OrphanRemoved(fixture.commit.id.clone())));
            assert!(!temporary.exists());
            assert_eq!(fixture.store.read(TARGET).unwrap(), fixture.original);
            assert_eq!(
                fixture
                    .store
                    .commit(&fixture.original, replacement)
                    .unwrap(),
                fixture.commit
            );
            assert_eq!(fixture.store.read(TARGET).unwrap().text, replacement);
            fixture.store.undo(&fixture.commit.id).unwrap();
            assert_eq!(fixture.store.read(TARGET).unwrap(), fixture.original);
        }
    }
}

#[test]
fn matching_names_in_other_parents_survive_even_with_compatible_bytes() {
    let fixture = Fixture::new(AFTER);
    let expected = fixture.temporary("nested", &fixture.commit.id);
    let unrelated = fixture.temporary("other", &fixture.commit.id);
    let root_artifact = fixture.temporary("", &fixture.commit.id);
    for path in [&expected, &unrelated, &root_artifact] {
        fs::write(path, AFTER).unwrap();
    }
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.contains(&Recovery::OrphanRemoved(fixture.commit.id.clone())));
    assert!(!expected.exists());
    for path in [unrelated, root_artifact] {
        assert_eq!(fs::read(&path).unwrap(), AFTER.as_bytes());
        assert!(recovered.iter().any(|outcome| {
            matches!(outcome, Recovery::OrphanPreserved { path: name, reason }
                if name == &fixture.relative(&path) && reason.contains("target parent"))
        }));
    }
    assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
fn divergent_extra_and_oversized_bytes_are_preserved_with_reasons() {
    for contents in [
        b"leftover".to_vec(),
        format!("{AFTER}extra").into_bytes(),
        vec![b'x'; 4097],
    ] {
        let fixture = Fixture::new(AFTER);
        let temporary = fixture.temporary("nested", &fixture.commit.id);
        fs::write(&temporary, &contents).unwrap();
        for _ in 0..2 {
            let recovered = fixture.store.recover().unwrap();
            assert!(
                !recovered
                    .iter()
                    .any(|outcome| { matches!(outcome, Recovery::OrphanRemoved(_)) })
            );
            assert!(recovered.iter().any(|outcome| {
                matches!(outcome, Recovery::OrphanPreserved { path, reason }
                    if path == &fixture.relative(&temporary) && !reason.is_empty())
            }));
            assert_eq!(fs::read(&temporary).unwrap(), contents);
            assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
        }
    }
}

#[test]
fn unknown_identity_is_retained_as_unknown_and_empty_journal_cannot_admit_cleanup() {
    let fixture = Fixture::new(AFTER);
    let id = "0".repeat(64);
    let temporary = fixture.temporary("nested", &id);
    fs::write(&temporary, AFTER).unwrap();
    assert!(
        fixture
            .store
            .recover()
            .unwrap()
            .contains(&Recovery::OrphanUnknown(fixture.relative(&temporary)))
    );
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
    fs::create_dir(fixture.root.join(".purrassist").join(&id)).unwrap();
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.contains(&Recovery::NotApplied(id)));
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::OrphanPreserved { path, reason }
            if path == &fixture.relative(&temporary) && !reason.is_empty())
    }));
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
}

#[test]
fn corrupt_and_incomplete_journals_never_authorize_prefix_deletion() {
    for piece in ["missing-prepared", "prepared.json", "before", "after"] {
        let fixture = Fixture::new(AFTER);
        let journal = fixture.journal();
        if piece == "missing-prepared" {
            fs::remove_file(journal.join("prepared.json")).unwrap();
        } else {
            fs::write(journal.join(piece), "{").unwrap();
        }
        let temporary = fixture.temporary("nested", &fixture.commit.id);
        fs::write(&temporary, &AFTER.as_bytes()[..2]).unwrap();
        let recovered = fixture.store.recover().unwrap();
        assert!(recovered.iter().any(|outcome| {
            matches!(outcome, Recovery::OrphanPreserved { path, reason }
                if path == &fixture.relative(&temporary) && !reason.is_empty())
        }));
        assert!(
            !recovered
                .iter()
                .any(|outcome| matches!(outcome, Recovery::OrphanRemoved(_)))
        );
        assert_eq!(fs::read(&temporary).unwrap(), &AFTER.as_bytes()[..2]);
        assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
    }
}

fn rewrite_journal(fixture: &Fixture, target: &str, empty_patch: bool) -> String {
    let old = purrassist::json::parse(
        &fs::read_to_string(fixture.journal().join("prepared.json")).unwrap(),
    )
    .unwrap();
    let patch = if empty_patch {
        object([
            ("source", string(fixture.original.digest.to_string())),
            ("edits", array([])),
        ])
    } else {
        old.get("patch").unwrap().clone()
    };
    let metadata = object([
        ("version", integer(2)),
        ("path", string(target)),
        ("before", string(fixture.original.digest.to_string())),
        ("after", string(fixture.commit.after.to_string())),
        ("patch", patch),
    ])
    .to_json();
    let id = Digest::of(metadata.as_bytes())
        .to_string()
        .trim_start_matches("sha256:")
        .to_string();
    let directory = fixture.root.join(".purrassist").join(&id);
    fs::rename(fixture.journal(), &directory).unwrap();
    fs::write(directory.join("prepared.json"), metadata).unwrap();
    id
}

#[test]
fn checksum_consistent_but_incoherent_patch_cannot_admit_cleanup() {
    let fixture = Fixture::new(AFTER);
    let id = rewrite_journal(&fixture, TARGET, true);
    let temporary = fixture.temporary("nested", &id);
    fs::write(&temporary, AFTER).unwrap();
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::Corrupt { id: found, reason }
            if found == &id && reason.contains("patch does not produce"))
    }));
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::OrphanPreserved { path, reason }
            if path == &fixture.relative(&temporary) && reason.contains("patch does not produce"))
    }));
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
    assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
fn checksum_consistent_escaped_target_is_not_cleanup_authority() {
    let fixture = Fixture::new(AFTER);
    let id = rewrite_journal(&fixture, "../outside.txt", false);
    let temporary = fixture.temporary("", &id);
    fs::write(&temporary, AFTER).unwrap();
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::OrphanPreserved { path, reason }
            if path == &fixture.relative(&temporary) && reason.contains("root-relative"))
    }));
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
    assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
fn non_directory_journal_never_authorizes_temporary_deletion() {
    let fixture = Fixture::new(AFTER);
    let temporary = fixture.temporary("nested", &fixture.commit.id);
    fs::write(&temporary, AFTER).unwrap();
    fs::remove_dir_all(fixture.journal()).unwrap();
    fs::write(fixture.journal(), AFTER).unwrap();
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::OrphanPreserved { path, reason }
            if path == &fixture.relative(&temporary) && reason.contains("real directory"))
    }));
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
    assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
#[cfg(unix)]
fn symlinked_journal_is_preserved_without_borrowing_outside_authority() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(AFTER);
    let outside = Fixture::new(AFTER);
    assert_eq!(fixture.commit.id, outside.commit.id);
    let temporary = fixture.temporary("nested", &fixture.commit.id);
    fs::write(&temporary, AFTER).unwrap();
    fs::remove_dir_all(fixture.journal()).unwrap();
    symlink(outside.journal(), fixture.journal()).unwrap();
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::OrphanPreserved { path, reason }
            if path == &fixture.relative(&temporary) && reason.contains("real directory"))
    }));
    assert!(
        fs::symlink_metadata(fixture.journal())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
    assert_eq!(outside.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
#[cfg(unix)]
fn symlink_temporaries_and_symlinked_directories_are_never_followed_or_removed() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(AFTER);
    let outside = Fixture::new(AFTER);
    let temporary = fixture.temporary("nested", &fixture.commit.id);
    symlink(outside.root.join(TARGET), &temporary).unwrap();
    let outside_artifact = outside.temporary("other", &fixture.commit.id);
    fs::write(&outside_artifact, AFTER).unwrap();
    symlink(outside.root.join("other"), fixture.root.join("linked")).unwrap();
    let recovered = fixture.store.recover().unwrap();
    assert!(recovered.iter().any(|outcome| {
        matches!(outcome, Recovery::OrphanPreserved { path, reason }
            if path == &fixture.relative(&temporary) && reason.contains("regular file"))
    }));
    assert!(
        fs::symlink_metadata(&temporary)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&outside_artifact).unwrap(), AFTER.as_bytes());
    assert_eq!(outside.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
fn resource_exhaustion_preserves_temporary_bytes_and_its_typed_outcome() {
    let fixture = Fixture::new(AFTER);
    let temporary = fixture.temporary("nested", &fixture.commit.id);
    fs::write(&temporary, AFTER).unwrap();
    fs::write(
        fixture.journal().join("prepared.json"),
        format!("[{}0]", "0,".repeat(10_000)),
    )
    .unwrap();
    assert!(matches!(
        fixture.store.recover(),
        Err(Error::Resource(StopReason::BudgetExceeded(
            Resource::ScratchBytes
        )))
    ));
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
    assert_eq!(fixture.store.read(TARGET).unwrap().text, AFTER);
}

#[test]
fn shared_recovery_tool_publishes_preservation_reason_against_its_output_schema() {
    let fixture = Fixture::new(AFTER);
    let temporary = fixture.temporary("other", &fixture.commit.id);
    fs::write(&temporary, AFTER).unwrap();
    let tools = Tools::new(&fixture.root).unwrap();
    let arguments = object(core::iter::empty::<(&str, purrassist::json::Node)>());
    let result = tools.call("recover_files", &arguments).unwrap();
    let Value::Array(outcomes) = &result.value else {
        panic!("recovery must return each observable outcome")
    };
    assert!(outcomes.iter().any(|outcome| {
        outcome.get("kind").and_then(purrassist::json::Node::as_str) == Some("orphan_preserved")
            && outcome.get("id").and_then(purrassist::json::Node::as_str)
                == Some(fixture.relative(&temporary).as_str())
            && outcome
                .get("reason")
                .and_then(purrassist::json::Node::as_str)
                .is_some_and(|reason| reason.contains("target parent"))
    }));
    let definitions = Tools::definitions().unwrap();
    let Value::Array(definitions) = definitions.value else {
        panic!("tool definitions must be an array")
    };
    let definition = definitions
        .iter()
        .find(|definition| {
            definition
                .get("name")
                .and_then(purrassist::json::Node::as_str)
                == Some("recover_files")
        })
        .unwrap();
    let schema = Schema::compile(&definition.get("outputSchema").unwrap().to_json()).unwrap();
    assert!(schema.validate(&result).unwrap().valid);
    assert_eq!(fs::read(&temporary).unwrap(), AFTER.as_bytes());
}
