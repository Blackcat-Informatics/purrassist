// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Compilation must not reset resource budgets across bundle preparation.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    schema::{CompilationError, Schema, recovery::RecoveryRequest},
};
use std::{collections::BTreeMap, fmt::Write as _};

#[test]
fn compilation_distinguishes_invalid_schemas_from_sticky_resource_failures() {
    for (resource, budget) in [
        (
            Resource::Work,
            Budget {
                work: 0,
                ..Budget::default()
            },
        ),
        (
            Resource::ScratchBytes,
            Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
        ),
        (
            Resource::InputBytes,
            Budget {
                input_bytes: 1,
                ..Budget::default()
            },
        ),
        (
            Resource::Depth,
            Budget {
                depth: 0,
                ..Budget::default()
            },
        ),
    ] {
        let mut governor = Governor::new(budget);
        let error =
            Schema::compile_governed(r#"{"properties":{"x":true}}"#, &mut governor).unwrap_err();
        assert_eq!(
            error,
            CompilationError::Resource(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(governor.check(), Err(StopReason::BudgetExceeded(resource)));
    }
    let mut governor = Governor::new(Budget::default());
    governor.cancel();
    assert_eq!(
        Schema::compile_governed("true", &mut governor).unwrap_err(),
        CompilationError::Resource(StopReason::Cancelled)
    );
    for source in [
        "{",
        r#"{"type":false}"#,
        r#"{"required":["x","x"]}"#,
        r#"{"type":["string","string"]}"#,
        r#"{"type":"string","type":"number"}"#,
        r#"{"multipleOf":-0}"#,
        r#"{"minItems":1e-999999999999999999999999}"#,
        r#"{"maxLength":-1e999999999999999999999999}"#,
        r##"{"$ref":"#/absent"}"##,
        r#"{"$ref":"urn:absent"}"#,
        r##"{"$ref":"#absent"}"##,
        r##"{"$ref":"#/%xx"}"##,
        r##"{"$ref":"#/%ff"}"##,
        r##"{"$ref":"#/~2"}"##,
        r##"{"$ref":"#/hidden","hidden":17}"##,
    ] {
        let mut governor = Governor::new(Budget::default());
        assert!(matches!(
            Schema::compile_governed(source, &mut governor),
            Err(CompilationError::InvalidSchema(_))
        ));
        assert!(governor.check().is_ok());
    }
    // Keyword number checks must preserve exact sign/integrality even when
    // exponents exceed machine integers; negative zero remains nonnegative.
    for source in [
        r#"{"multipleOf":1e-999999999999999999999999}"#,
        r#"{"minItems":-0e-999999999999999999999999}"#,
        r#"{"maxLength":1e999999999999999999999999}"#,
        r#"{"required":["é","e"],"type":["string","number"]}"#,
    ] {
        let mut governor = Governor::new(Budget::default());
        Schema::compile_governed(source, &mut governor).unwrap();
        assert!(governor.check().is_ok());
    }
}
#[test]
fn reference_and_dialect_loading_share_the_same_governor() {
    let resources = BTreeMap::from([
        (
            "urn:external".into(),
            r##"{"$id":"urn:external","hidden":{"type":"integer"},"$ref":"#/hidden"}"##.into(),
        ),
        (
            "urn:dialect".into(),
            r#"{"$vocabulary":{"https://json-schema.org/draft/2020-12/vocab/validation":true,"urn:optional:z":false,"urn:optional:a":false}}"#
                .into(),
        ),
    ]);
    let source =
        r#"{"$schema":"urn:dialect","$ref":"urn:external","customZ":true,"customA":false}"#;
    let mut governor = Governor::new(Budget::default());
    let schema = Schema::with_resources_governed(source, &resources, &mut governor).unwrap();
    let unsupported = schema.unsupported_vocabularies();
    assert_eq!(
        unsupported
            .iter()
            .map(|item| item.uri.as_str())
            .collect::<Vec<_>>(),
        ["urn:optional:a", "urn:optional:z"]
    );
    assert!(
        unsupported
            .iter()
            .all(|item| item.keywords == ["customA", "customZ"])
    );
    let compiled_work = governor.usage().work;
    let result = schema.recover_governed("42", RecoveryRequest::default(), &mut governor);
    assert!(matches!(result.outcome, Outcome::Resolved(_)));
    assert!(result.receipt.usage.work > compiled_work);
    assert_eq!(result.receipt.usage, governor.usage());
    let mut limited = Governor::new(Budget {
        work: compiled_work - 1,
        ..Budget::default()
    });
    assert_eq!(
        Schema::with_resources_governed(source, &resources, &mut limited).unwrap_err(),
        CompilationError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    let mut limited = Governor::new(Budget {
        work: compiled_work,
        ..Budget::default()
    });
    let schema = Schema::with_resources_governed(source, &resources, &mut limited).unwrap();
    let result = schema.recover_governed("42", RecoveryRequest::default(), &mut limited);
    assert_eq!(result.outcome, Outcome::Unresolved);
    assert_eq!(
        result.receipt.stopped,
        StopReason::BudgetExceeded(Resource::Work)
    );

    // Duplicate declarations are not unique lookups: preserve Node::get's
    // behavior rather than selecting either conflicting vocabulary object.
    let duplicate = Schema::with_resources(
        r#"{"$schema":"urn:duplicate","type":"string"}"#,
        &BTreeMap::from([(
            "urn:duplicate".into(),
            r#"{"$vocabulary":{"urn:unknown":true},"$vocabulary":{}}"#.into(),
        )]),
    )
    .unwrap();
    assert!(
        !duplicate
            .validate(&purrassist_core::json::parse("42").unwrap())
            .unwrap()
            .valid
    );

    // A reference-only node inherits the effective base encountered while
    // traversing a compiled resource boundary, not the root document's base.
    let nested = Schema::with_resources(
        r##"{"$id":"https://example.test/root","$defs":{"scope":{"$id":"sub/","hidden":{"$ref":"target"}}},"$ref":"#/$defs/scope/hidden"}"##,
        &BTreeMap::from([("https://example.test/sub/target".into(), "true".into())]),
    ).unwrap();
    assert!(
        nested
            .validate(&purrassist_core::json::parse("42").unwrap())
            .unwrap()
            .valid
    );

    // A short anchor must not copy a long stored target pointer; a pointer
    // reference must admit the actual wide-object search and its owned route.
    let name = "long-name".repeat(512);
    let anchored =
        format!(r##"{{"$ref":"#x","$defs":{{"{name}":{{"$anchor":"x","type":"integer"}}}}}}"##);
    let mut hidden = String::new();
    for index in 0..96 {
        write!(hidden, "\"unused{index}\":false,").unwrap();
    }
    hidden.push_str(r#""selected":{"type":"integer"}"#);
    let pointer = format!(r##"{{"$ref":"#/hidden/selected","hidden":{{{hidden}}}}}"##);
    for source in [&anchored, &pointer] {
        assert_reference_boundaries(source);
    }
}

fn assert_reference_boundaries(source: &str) {
    let schema = Schema::compile(source).unwrap();
    let instance = purrassist_core::json::parse("42").unwrap();
    let mut measured = Governor::new(Budget::default());
    assert!(
        schema
            .validate_with(&instance, &mut measured)
            .unwrap()
            .valid
    );
    assert_eq!(
        measured.scratch_meter().live(),
        0,
        "successful reference scratch is temporary"
    );
    let usage = measured.usage();
    for (resource, budget) in [
        (
            Resource::Work,
            Budget {
                work: usage.work - 1,
                ..Budget::default()
            },
        ),
        (
            Resource::ScratchBytes,
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
        ),
    ] {
        let mut limited = Governor::new(budget);
        assert_eq!(
            schema
                .validate_with(&instance, &mut limited)
                .map(|report| report.valid),
            Err(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(
            limited.scratch_meter().live(),
            0,
            "reference owners release on refusal"
        );
    }
    let mut exact = Governor::new(Budget {
        work: usage.work,
        scratch_bytes: usage.peak_scratch_bytes,
        ..Budget::default()
    });
    assert!(schema.validate_with(&instance, &mut exact).unwrap().valid);
    let mut cancelled = Governor::new(Budget::default());
    cancelled.cancel();
    assert_eq!(
        schema
            .validate_with(&instance, &mut cancelled)
            .map(|report| report.valid),
        Err(StopReason::Cancelled)
    );
    assert_eq!(cancelled.scratch_meter().live(), 0);
}
#[test]
fn unused_resource_hashing_has_exact_work_boundaries_without_framing_storage() {
    // Cross multiple hashing checkpoints without parsing these unused bytes.
    let resources = BTreeMap::from([("urn:unused".into(), " ".repeat(8193))]);
    let mut measured = Governor::new(Budget::default());
    let schema = Schema::with_resources_governed("true", &resources, &mut measured).unwrap();
    let complete = measured.usage();
    let mut without_resources = Governor::new(Budget::default());
    Schema::compile_governed("true", &mut without_resources).unwrap();
    // Bundle identity adds no heap storage even for unused external documents.
    assert_eq!(
        complete.total_scratch_charged,
        without_resources.usage().total_scratch_charged
    );
    assert_eq!(
        complete.peak_scratch_bytes,
        without_resources.usage().peak_scratch_bytes
    );
    let mut exact = Governor::new(Budget {
        work: complete.work,
        ..Budget::default()
    });
    assert_eq!(
        Schema::with_resources_governed("true", &resources, &mut exact)
            .unwrap()
            .digest(),
        schema.digest()
    );
    let mut limited = Governor::new(Budget {
        work: complete.work - 1,
        ..Budget::default()
    });
    // Refuse at final bundle publication, never returning a root-only identity.
    assert_eq!(
        Schema::with_resources_governed("true", &resources, &mut limited).unwrap_err(),
        CompilationError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(
        limited.check(),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let mut short = Governor::new(Budget {
        work: 1000,
        ..Budget::default()
    });
    assert_eq!(
        Schema::with_resources_governed("true", &resources, &mut short).unwrap_err(),
        CompilationError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    // Original unused-document input admission still precedes parsing or hashing.
    let mut input = Governor::new(Budget {
        input_bytes: 8192,
        ..Budget::default()
    });
    assert_eq!(
        Schema::with_resources_governed("true", &resources, &mut input).unwrap_err(),
        CompilationError::Resource(StopReason::BudgetExceeded(Resource::InputBytes))
    );
    assert_eq!(input.usage().total_scratch_charged, 0);
}

/// Parsed annotation buffers belong to their document, including external and
/// temporary meta-schema documents; only the separate compiler allowance remains.
#[test]
fn schema_document_storage_releases_on_drop_and_late_refusal() {
    let annotation = format!(
        r#"[{{"text":"{}","number":1.2300e99999999999999999999}}]"#,
        "é".repeat(4096)
    );
    let document = |payload: &str| format!(r#"{{"type":"string","annotation":{payload}}}"#);
    let measure = |source: &str, resources: &BTreeMap<String, String>| {
        let mut governor = Governor::new(Budget::default());
        let schema = Schema::with_resources_governed(source, resources, &mut governor).unwrap();
        let live = governor.scratch_meter().live();
        let work = governor.usage().work;
        drop(schema);
        let remaining = governor.scratch_meter().live();
        let mut limited = Governor::new(Budget {
            work: work - 1,
            ..Budget::default()
        });
        assert_eq!(
            Schema::with_resources_governed(source, resources, &mut limited).unwrap_err(),
            CompilationError::Resource(StopReason::BudgetExceeded(Resource::Work)),
        );
        assert_eq!(
            limited.scratch_meter().live(),
            remaining,
            "late refusal releases the same owners as schema drop"
        );
        (live, remaining)
    };
    let empty = BTreeMap::new();
    let small = measure(&document("[]"), &empty);
    let large = measure(&document(&annotation), &empty);
    assert!(
        large.0 > small.0 + 8192,
        "nested strings and number/container buffers stay admitted while borrowed"
    );
    assert_eq!(
        large.1, small.1,
        "annotation storage does not leak into the legacy allowance"
    );

    let source = r#"{"$ref":"urn:external"}"#;
    let small = measure(
        source,
        &BTreeMap::from([("urn:external".into(), document("[]"))]),
    );
    let large = measure(
        source,
        &BTreeMap::from([("urn:external".into(), document(&annotation))]),
    );
    assert!(large.0 > small.0 + 8192);
    assert_eq!(
        large.1, small.1,
        "referenced document buffers release with the schema"
    );

    let source = r#"{"$schema":"urn:meta","type":"string"}"#;
    let meta = |payload: &str| {
        format!(
            r#"{{"$vocabulary":{{"https://json-schema.org/draft/2020-12/vocab/validation":true}},"annotation":{payload}}}"#
        )
    };
    let small = measure(source, &BTreeMap::from([("urn:meta".into(), meta("[]"))]));
    let large = measure(
        source,
        &BTreeMap::from([("urn:meta".into(), meta(&annotation))]),
    );
    assert_eq!(
        large, small,
        "temporary meta-schema buffers release before publication"
    );
}
