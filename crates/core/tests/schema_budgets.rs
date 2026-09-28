// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Compilation must not reset resource budgets across bundle preparation.
use purrassist_core::{
    Budget, Governor, Outcome, StopReason,
    contract::Resource,
    schema::{CompilationError, Schema, recovery::RecoveryRequest},
};
use std::collections::BTreeMap;

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
    for source in ["{", r#"{"type":false}"#, r##"{"$ref":"#/absent"}"##] {
        let mut governor = Governor::new(Budget::default());
        assert!(matches!(
            Schema::compile_governed(source, &mut governor),
            Err(CompilationError::InvalidSchema(_))
        ));
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
            r#"{"$vocabulary":{"https://json-schema.org/draft/2020-12/vocab/validation":true}}"#
                .into(),
        ),
    ]);
    let source = r#"{"$schema":"urn:dialect","$ref":"urn:external"}"#;
    let mut governor = Governor::new(Budget::default());
    let schema = Schema::with_resources_governed(source, &resources, &mut governor).unwrap();
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
}
#[test]
fn unused_resources_are_admitted_before_identity_hashing() {
    let resources = BTreeMap::from([("urn:unused".into(), " ".repeat(2000))]);
    let mut governor = Governor::new(Budget {
        work: 1000,
        ..Budget::default()
    });
    assert_eq!(
        Schema::with_resources_governed("true", &resources, &mut governor).unwrap_err(),
        CompilationError::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
}
