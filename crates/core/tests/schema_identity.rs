// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Schema receipts bind their external constraints and dialect configuration.
use purrassist_core::{json::parse, schema::Schema};
use std::collections::BTreeMap;

#[test]
fn external_constraint_changes_cannot_share_receipt_identity() {
    let root = r#"{"$ref":"https://example.test/value"}"#;
    let mut resources = BTreeMap::from([("https://example.test/value".into(), "true".into())]);
    let accepting = Schema::with_resources(root, &resources).unwrap();
    resources.insert("https://example.test/value".into(), "false".into());
    let rejecting = Schema::with_resources(root, &resources).unwrap();
    let value = parse("42").unwrap();
    assert!(accepting.validate(&value).unwrap().valid);
    assert!(!rejecting.validate(&value).unwrap().valid);
    assert_ne!(accepting.digest(), rejecting.digest());
    assert_eq!(
        rejecting.digest(),
        Schema::with_resources(root, &resources).unwrap().digest()
    );
}

#[test]
fn custom_dialect_bytes_are_bound_even_without_ref() {
    let root = r#"{"$schema":"https://example.test/dialect","type":"string"}"#;
    let mut resources = BTreeMap::from([("https://example.test/dialect".into(), "{}".into())]);
    let first = Schema::with_resources(root, &resources).unwrap();
    resources.insert(
        "https://example.test/dialect".into(),
        r#"{"$vocabulary":{}}"#.into(),
    );
    let second = Schema::with_resources(root, &resources).unwrap();
    assert_ne!(first.digest(), second.digest());
    assert!(!first.validate(&parse("1").unwrap()).unwrap().valid);
    assert!(second.validate(&parse("1").unwrap()).unwrap().valid);
}
