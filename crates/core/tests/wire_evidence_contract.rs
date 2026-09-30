// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Public evidence serialization keeps the receipt's source and absent-span contract.
use purrassist_core::{
    Budget, Effort, Receipt,
    contract::{Evidence, SourceSpan},
    digest::Digest,
    json, wire,
};

#[test]
fn standalone_evidence_matches_receipt_with_exact_source_offsets_and_null_span() {
    let source = "Tea café";
    let digest = Digest::of(source.as_bytes());
    for span in [
        Some(SourceSpan {
            source: digest,
            bytes: 4..9,
        }),
        None,
    ] {
        let evidence = Evidence {
            rule: "test.literal.v1".into(),
            explanation: "Preserve café and \"quotes\".".into(),
            destination: "/properties/café".into(),
            span,
        };
        let expected_span = evidence.span.as_ref().map_or_else(
            || "null".to_owned(),
            |_| format!(r#"{{"source":"{digest}","start":4,"end":9}}"#),
        );
        let expected = json::parse(&format!(
            r#"{{"rule":"test.literal.v1","explanation":"Preserve café and \"quotes\".","destination":"/properties/café","span":{expected_span}}}"#
        )).unwrap();
        let standalone = wire::evidence(&evidence);
        assert!(standalone.equivalent(&expected));
        let mut receipt = Receipt::new(
            source.as_bytes(),
            "test",
            Effort::Standard,
            Budget::default(),
        );
        receipt.evidence.push(evidence);
        assert_eq!(
            standalone.to_json(),
            receipt.to_node().pointer("/evidence/0").unwrap().to_json(),
        );
    }
}
