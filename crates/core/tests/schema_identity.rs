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
    // Frozen before streaming identity construction; receipt identities must not drift.
    assert_eq!(
        accepting.digest().to_string(),
        "sha256:8d27ba1bcac65eac263ef9ca377b239cdcd9a76316014b4b8b637505b3b2e4c5"
    );
    assert_eq!(
        rejecting.digest().to_string(),
        "sha256:7759e30a80be266db2b3a82ceb97364cae82015fac9237b9dcf1d4adb79fa27e"
    );
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

#[test]
fn dialect_prefixes_and_unused_resources_keep_frozen_order_independent_identities() {
    let forward = BTreeMap::from([
        ("urn:unused:a".into(), "true".into()),
        ("urn:unused:b".into(), "false".into()),
    ]);
    let reverse = BTreeMap::from([
        ("urn:unused:b".into(), "false".into()),
        ("urn:unused:a".into(), "true".into()),
    ]);
    for (source, frozen) in [
        (
            "true",
            "sha256:21b4ead4bf8fe964abfa8c7edf3a495ae36fb5d5a9b91f688ecc780ef6140fac",
        ),
        (
            r#"{"$schema":"http://json-schema.org/draft-07/schema","type":"integer"}"#,
            "sha256:f72dee7e819423662650a0b2cef0145771fcc4014cc31ac298a1f1b63453c82c",
        ),
    ] {
        let original = Schema::with_resources(source, &forward).unwrap();
        assert_eq!(original.digest().to_string(), frozen);
        assert_eq!(
            Schema::with_resources(source, &reverse).unwrap().digest(),
            original.digest()
        );
        let mut changed = forward.clone();
        changed.insert("urn:unused:b".into(), "true".into());
        assert_ne!(
            Schema::with_resources(source, &changed).unwrap().digest(),
            original.digest()
        );
    }
}
