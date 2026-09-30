// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `recover_files`/`commit_files`: [`crate::files::Store::recover`] and
//! [`crate::files::Store::commit_batch`] as tools, alongside the existing
//! single-file `apply_file`/`undo_file`.
use crate::files::{BatchOutcome, Recovery, Store};
use purrassist::{
    json::{Node, Value},
    wire::{array, integer, object, string},
};

pub(super) fn recover(store: &Store) -> Result<Node, String> {
    let recoveries = store.recover().map_err(|e| e.to_string())?;
    Ok(array(recoveries.iter().map(recovery_node)))
}

fn recovery_node(recovery: &Recovery) -> Node {
    let (kind, id, reason) = match recovery {
        Recovery::Finalized(id) => ("finalized", id.as_str(), None),
        Recovery::NotApplied(id) => ("not_applied", id.as_str(), None),
        Recovery::Conflict(id) => ("conflict", id.as_str(), None),
        Recovery::Corrupt { id, reason } => ("corrupt", id.as_str(), Some(reason.as_str())),
        Recovery::OrphanRemoved(id) => ("orphan_removed", id.as_str(), None),
        Recovery::OrphanUnknown(name) => ("orphan_unknown", name.as_str(), None),
        Recovery::OrphanPreserved { path, reason } => {
            ("orphan_preserved", path.as_str(), Some(reason.as_str()))
        }
    };
    object([
        ("kind", string(kind)),
        ("id", string(id)),
        (
            "reason",
            reason.map_or_else(|| Node::new(Value::Null), string),
        ),
    ])
}

pub(super) fn commit(store: &Store, args: &Node) -> Result<Node, String> {
    let Some(Node {
        value: Value::Array(items),
        ..
    }) = args.get("files")
    else {
        return Err("missing files".into());
    };
    let mut replacements = Vec::with_capacity(items.len());
    for item in items {
        let path = super::field(item, "path")?;
        let expected_digest = super::field(item, "expected_digest")?;
        let replacement = super::field(item, "replacement")?;
        let snapshot = store.read(path).map_err(|e| e.to_string())?;
        if snapshot.digest.to_string() != expected_digest {
            return Err(format!("stale expected digest for {path}"));
        }
        replacements.push((snapshot, replacement.to_string()));
    }
    let outcomes = store.commit_batch(&replacements);
    Ok(array(outcomes.iter().map(batch_outcome_node)))
}

fn batch_outcome_node(outcome: &BatchOutcome) -> Node {
    let (status, commit, error) = match &outcome.result {
        Ok(commit) => ("committed", Some(commit), None),
        Err(error) => ("error", None, Some(error.to_string())),
    };
    object([
        (
            "index",
            integer(u64::try_from(outcome.index).unwrap_or(u64::MAX)),
        ),
        ("path", string(&outcome.path)),
        ("status", string(status)),
        (
            "commit",
            commit.map_or_else(
                || Node::new(Value::Null),
                |commit| super::commit_node(commit.clone()),
            ),
        ),
        (
            "error",
            error.map_or_else(|| Node::new(Value::Null), string),
        ),
    ])
}
