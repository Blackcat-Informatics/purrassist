// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Combined validation, four-form projection, framing, byte and refusal contract.

use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason, contract::Resource, json, schema::Schema,
};
use purrassist_toolgraph::{
    ContextManagement, FieldMask, LoadingStrategy, NullSelection, OutputByteLimit,
    OutputEnforcement, OutputLimitError, OutputMeasurement, OutputTransform, OutputUnit,
    ProjectionFailure, ProjectionStage,
};

fn context(mask: &str, limit: u64) -> ContextManagement {
    ContextManagement::new(
        FieldMask::parse(mask).unwrap(),
        OutputByteLimit::new(limit).unwrap(),
        0,
        vec![],
        LoadingStrategy::Always,
    )
}

#[test]
fn all_masks_preserve_order_and_exact_values_in_zero_one_many_lines() {
    let schema = Schema::compile("true").unwrap();
    for (mask, source, expected) in [
        (".a.b", r#"{"a":{"b":1.00e2},"other":9}"#, "1.00e2\n"),
        (".a.b", "{}", "null\n"),
        (".a.b", r#"{"a":null}"#, "null\n"),
        (
            "{b,a}",
            r#"{"a":1,"b":{"x":2,"x":3},"c":4}"#,
            "{\"b\":{\"x\":2,\"x\":3},\"a\":1}\n",
        ),
        ("{b,a}", r#"{"a":1}"#, "{\"b\":null,\"a\":1}\n"),
        ("{b,a}", "null", "{\"b\":null,\"a\":null}\n"),
        (
            ".[]",
            r#"[1.00e2,{"x":1,"x":2},"é"]"#,
            "1.00e2\n{\"x\":1,\"x\":2}\n\"é\"\n",
        ),
        (".[]", "[]", ""),
        (".a[]?", r#"{"a":[true,false]}"#, "true\nfalse\n"),
        (".a[]?", r#"{"a":[]}"#, ""),
        (".a[]?", r#"{"a":1}"#, ""),
        (".a[]?", r#"{"a":null}"#, ""),
        (".a[]?", "{}", ""),
        (".a[]?", "null", ""),
    ] {
        let context = context(mask, 1024);
        let original = json::parse(source).unwrap();
        let mut governor = Governor::new(Budget::default());
        let artifact = context
            .project_result(&schema, &original, &mut governor)
            .unwrap();
        assert_eq!(artifact.bytes.as_str(), expected, "{mask} on {source}");
        assert_eq!(
            artifact.measurement.measured,
            u64::try_from(expected.len()).unwrap()
        );
        assert!(core::ptr::eq(artifact.trace.original, &raw const original));
        assert!(core::ptr::eq(
            artifact.trace.mask,
            context.output_projection()
        ));
        assert_eq!(artifact.trace.stage, ProjectionStage::Complete);
        assert!(artifact.trace.validation.as_ref().unwrap().valid);
        assert_eq!(
            artifact.trace.transformations.last(),
            Some(&OutputTransform::MaskApplied)
        );
        assert_eq!(original.to_json(), source);
    }
}

#[test]
fn schema_validation_precedes_masking_and_selected_duplicates_never_choose_a_winner() {
    let schema = Schema::compile(
        r#"{"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}"#,
    )
    .unwrap();
    let context = context(".visible", 1024);
    let original = json::parse(r#"{"id":"bad","visible":1}"#).unwrap();
    let error = context
        .project_result(&schema, &original, &mut Governor::new(Budget::default()))
        .unwrap_err();
    assert_eq!(error.reason, ProjectionFailure::Schema);
    assert_eq!(error.trace.stage, ProjectionStage::Validation);
    assert!(!error.trace.validation.as_ref().unwrap().valid);
    assert!(error.trace.transformations.is_empty());
    assert!(core::ptr::eq(error.trace.original, &raw const original));
    let schema = Schema::compile("true").unwrap();
    for (mask, source, expected) in [
        (
            ".a",
            r#"{"a":1,"a":2}"#,
            ProjectionFailure::AmbiguousMember { field: "a" },
        ),
        (
            "{a,b}",
            r#"{"a":1,"a":2}"#,
            ProjectionFailure::AmbiguousMember { field: "a" },
        ),
        (
            ".a[]?",
            r#"{"a":[],"a":[]}"#,
            ProjectionFailure::AmbiguousMember { field: "a" },
        ),
        (
            ".a.b",
            r#"{"a":1}"#,
            ProjectionFailure::WrongType { field: Some("b") },
        ),
        (
            "{a,b}",
            "[]",
            ProjectionFailure::WrongType { field: Some("a") },
        ),
        (".[]", "{}", ProjectionFailure::WrongType { field: None }),
        (
            ".a[]?",
            "true",
            ProjectionFailure::WrongType { field: Some("a") },
        ),
    ] {
        let context = self::context(mask, 1024);
        let original = json::parse(source).unwrap();
        let error = context
            .project_result(&schema, &original, &mut Governor::new(Budget::default()))
            .unwrap_err();
        assert_eq!(error.reason, expected);
        assert_eq!(error.trace.stage, ProjectionStage::Selection);
        assert!(error.trace.validation.as_ref().unwrap().valid);
        assert!(core::ptr::eq(error.trace.original, &raw const original));
        assert_eq!(original.to_json(), source);
    }
}

#[test]
fn framing_escaping_and_utf8_are_counted_before_any_result_is_released() {
    let schema = Schema::compile("true").unwrap();
    let original = json::parse(r#"{"x":"é\n\""}"#).unwrap();
    let context = context(".x", 9);
    let artifact = context
        .project_result(&schema, &original, &mut Governor::new(Budget::default()))
        .unwrap();
    assert_eq!(artifact.bytes.as_str(), "\"é\\n\\\"\"\n");
    assert_eq!(
        artifact.measurement,
        OutputMeasurement {
            limit: 9,
            measured: 9,
            unit: OutputUnit::Bytes,
            enforcement: OutputEnforcement::ExactBytes
        }
    );
    let context = self::context(".x", 8);
    let error = context
        .project_result(&schema, &original, &mut Governor::new(Budget::default()))
        .unwrap_err();
    assert_eq!(
        error.reason,
        ProjectionFailure::OutputLimit(OutputLimitError::Exceeded(OutputMeasurement {
            limit: 8,
            measured: 9,
            unit: OutputUnit::Bytes,
            enforcement: OutputEnforcement::ExactBytes
        }))
    );
    assert_eq!(error.trace.stage, ProjectionStage::ByteLimit);
    assert!(core::ptr::eq(error.trace.original, &raw const original));
    assert_eq!(
        error.trace.transformations.last(),
        Some(&OutputTransform::MaskApplied)
    );
}

#[test]
fn null_and_optional_transforms_describe_derived_values_without_claiming_source_facts() {
    let schema = Schema::compile("true").unwrap();
    let original = json::parse("{}").unwrap();
    let context = context(".a.b", 16);
    let artifact = context
        .project_result(&schema, &original, &mut Governor::new(Budget::default()))
        .unwrap();
    assert_eq!(
        artifact.trace.transformations.as_slice(),
        [
            OutputTransform::NullInserted {
                field: "a",
                index: 0,
                cause: NullSelection::Missing
            },
            OutputTransform::NullInserted {
                field: "b",
                index: 1,
                cause: NullSelection::Propagated
            },
            OutputTransform::MaskApplied,
        ]
    );
    let context = self::context(".a[]?", 1);
    let artifact = context
        .project_result(&schema, &original, &mut Governor::new(Budget::default()))
        .unwrap();
    assert_eq!(artifact.bytes.as_str(), "");
    assert_eq!(
        artifact.trace.transformations.as_slice(),
        [
            OutputTransform::NullInserted {
                field: "a",
                index: 0,
                cause: NullSelection::Missing
            },
            OutputTransform::OptionalOmitted { field: "a" },
            OutputTransform::MaskApplied,
        ]
    );
}

#[test]
fn native_refusals_keep_original_stage_and_release_unpublished_projection_storage() {
    let schema = Schema::compile("true").unwrap();
    let original = json::parse(&format!(r#"{{"x":"{}"}}"#, "é\\\"".repeat(128))).unwrap();
    let context = context("{x,missing}", 8192);
    let mut baseline = Governor::new(Budget::default());
    drop(schema.validate_with(&original, &mut baseline).unwrap());
    let validation_live = baseline.scratch_meter().live();
    let mut measured = Governor::new(Budget::default());
    let artifact = context
        .project_result(&schema, &original, &mut measured)
        .unwrap();
    let usage = measured.usage();
    assert!(measured.scratch_meter().live() > validation_live);
    drop(artifact);
    assert_eq!(measured.scratch_meter().live(), validation_live);
    for (budget, resource) in [
        (
            Budget {
                work: 0,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                work: usage.work / 2,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                work: usage.work - 1,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
        (
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
        (
            Budget {
                input_bytes: 0,
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
    ] {
        let mut governor = Governor::new(budget);
        let error = context
            .project_result(&schema, &original, &mut governor)
            .unwrap_err();
        let reason = StopReason::BudgetExceeded(resource);
        assert_eq!(error.reason, ProjectionFailure::Resource(reason.clone()));
        assert!(core::ptr::eq(error.trace.original, &raw const original));
        if resource == Resource::Work && budget.work == 0 {
            assert_eq!(error.trace.stage, ProjectionStage::Validation);
        }
        if resource == Resource::Work && budget.work == usage.work / 2 {
            assert_eq!(error.trace.stage, ProjectionStage::Serialization);
        }
        if resource == Resource::Work && budget.work == usage.work - 1 {
            assert_eq!(error.trace.stage, ProjectionStage::ByteLimit);
        }
        drop(error);
        assert_eq!(governor.check(), Err(reason));
        assert_eq!(governor.scratch_meter().live(), validation_live);
    }
    let token = CancellationToken::new();
    token.cancel();
    let mut governor = Governor::with_cancellation(Budget::default(), token);
    let error = context
        .project_result(&schema, &original, &mut governor)
        .unwrap_err();
    assert_eq!(
        error.reason,
        ProjectionFailure::Resource(StopReason::Cancelled)
    );
    assert_eq!(error.trace.stage, ProjectionStage::Validation);
    assert!(core::ptr::eq(error.trace.original, &raw const original));
}
