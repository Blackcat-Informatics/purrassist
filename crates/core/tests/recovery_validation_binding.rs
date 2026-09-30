// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Competing repairs reference whole-schema validation, independently of annotations.
use purrassist_core::{
    Outcome,
    digest::Digest,
    schema::{Schema, recovery::RecoveryRequest},
};

#[test]
fn ambiguous_repairs_bind_passing_schema_validations_before_format_annotations() {
    let schema =
        Schema::compile(r#"{"type":"string","enum":["bad","bar"],"format":"email"}"#).unwrap();
    let input = r#""baf""#;
    let result = schema.recover(input, RecoveryRequest::default());
    let Outcome::Ambiguous(choices) = &result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(choices.len(), 2);
    assert_eq!(result.receipt.alternatives.len(), choices.len());
    assert!(
        result
            .receipt
            .validations
            .iter()
            .any(|entry| entry.validator == "schema.format.email-v1" && !entry.passed)
    );
    for (choice, ledger) in choices.iter().zip(&result.receipt.alternatives) {
        assert_eq!(ledger.output, Digest::of(choice.to_json().as_bytes()));
        assert!(schema.validate(choice).unwrap().valid);
        assert_eq!(ledger.validations.len(), 1);
        let validation = &result.receipt.validations[ledger.validations[0]];
        assert_eq!(validation.validator, "purrassist.schema.draft2020-12.v1");
        assert_eq!(validation.constraint, schema.digest().to_string());
        assert!(validation.passed);
    }
    assert_eq!(result.receipt.sources[0].text, input);
}
