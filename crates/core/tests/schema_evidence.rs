// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Evaluated evidence survives nested instance scopes and rejected branches.
use purrassist_core::{
    Budget, Governor,
    contract::{Resource, StopReason},
    json,
    schema::Schema,
};

#[test]
fn nested_applicators_retain_format_and_pattern_evidence() {
    let rule = r#"{"format":"email","pattern":"^x"}"#;
    let cases = [
        (
            format!(r#"{{"properties":{{"email":{rule}}}}}"#),
            r#"{"email":"bad"}"#,
            "/email",
            1,
            false,
        ),
        (
            format!(r#"{{"patternProperties":{{"^email$":{rule}}}}}"#),
            r#"{"email":"bad"}"#,
            "/email",
            2,
            false,
        ),
        (
            format!(r#"{{"additionalProperties":{rule}}}"#),
            r#"{"email":"bad"}"#,
            "/email",
            1,
            false,
        ),
        (
            format!(r#"{{"propertyNames":{rule}}}"#),
            r#"{"bad":0}"#,
            "/bad/@name",
            1,
            false,
        ),
        (
            format!(r#"{{"unevaluatedProperties":{rule}}}"#),
            r#"{"email":"bad"}"#,
            "/email",
            1,
            false,
        ),
        (
            format!(r#"{{"prefixItems":[{rule}]}}"#),
            r#"["bad"]"#,
            "/0",
            1,
            false,
        ),
        (
            format!(r#"{{"items":{rule}}}"#),
            r#"["bad"]"#,
            "/0",
            1,
            false,
        ),
        (
            format!(r#"{{"unevaluatedItems":{rule}}}"#),
            r#"["bad"]"#,
            "/0",
            1,
            false,
        ),
        (
            format!(r#"{{"contains":{rule},"minContains":0}}"#),
            r#"["bad"]"#,
            "/0",
            1,
            true,
        ),
        (
            format!(r#"{{"contains":{rule}}}"#),
            r#"["bad"]"#,
            "/0",
            1,
            false,
        ),
    ];
    for (source, instance, path, patterns, valid) in cases {
        let report = Schema::compile(&source)
            .unwrap()
            .validate(&json::parse(instance).unwrap())
            .unwrap();
        assert_eq!(report.valid, valid, "{source}");
        assert_eq!(report.format_evidence.len(), 1, "{source}");
        assert_eq!(report.format_evidence[0].constraint, path, "{source}");
        assert!(!report.format_evidence[0].passed, "{source}");
        assert_eq!(report.pattern_evidence.len(), patterns, "{source}");
    }
}

#[test]
fn combinators_retain_every_executed_branch_evidence() {
    let fail = r#"{"format":"email","pattern":"^x"}"#;
    let pass = r#"{"format":"email","pattern":"^b"}"#;
    let cases = [
        (format!(r#"{{"allOf":[{fail},true]}}"#), false),
        (format!(r#"{{"anyOf":[{fail},true]}}"#), true),
        (format!(r#"{{"anyOf":[{fail},false]}}"#), false),
        (format!(r#"{{"oneOf":[{fail},true]}}"#), true),
        (format!(r#"{{"oneOf":[{fail},false]}}"#), false),
        (format!(r#"{{"oneOf":[{pass},true]}}"#), false),
        (format!(r#"{{"not":{fail}}}"#), true),
        (format!(r#"{{"not":{pass}}}"#), false),
        (format!(r#"{{"if":{fail},"then":false,"else":true}}"#), true),
        (
            format!(r#"{{"if":{pass},"then":false,"else":true}}"#),
            false,
        ),
        (
            format!(r##"{{"$defs":{{"rule":{fail}}},"$ref":"#/$defs/rule"}}"##),
            false,
        ),
    ];
    for (source, valid) in cases {
        let report = Schema::compile(&source)
            .unwrap()
            .validate(&json::parse(r#""bad""#).unwrap())
            .unwrap();
        assert_eq!(report.valid, valid, "{source}");
        assert_eq!(report.format_evidence.len(), 1, "{source}");
        assert_eq!(report.format_evidence[0].constraint, "", "{source}");
        assert!(!report.format_evidence[0].passed, "{source}");
        assert_eq!(report.pattern_evidence.len(), 1, "{source}");
    }
}

#[test]
fn child_annotations_do_not_mark_parent_properties_or_items_evaluated() {
    for (source, instance, keyword) in [
        (
            r#"{"properties":{"outer":{"properties":{"leaked":{"format":"email"}}}},"unevaluatedProperties":false}"#,
            r#"{"outer":{"leaked":"bad"},"leaked":0}"#,
            "false",
        ),
        (
            r#"{"prefixItems":[{"prefixItems":[{"format":"email"},true]}],"unevaluatedItems":false}"#,
            r#"[["bad",0],0]"#,
            "false",
        ),
    ] {
        let report = Schema::compile(source)
            .unwrap()
            .validate(&json::parse(instance).unwrap())
            .unwrap();
        assert!(!report.valid, "{source}");
        assert!(
            report
                .violations
                .iter()
                .any(|entry| entry.keyword == keyword)
        );
        assert_eq!(report.format_evidence.len(), 1, "{source}");
    }
}

#[test]
fn nested_evidence_exhaustion_remains_an_explicit_error() {
    let schema = Schema::compile(r#"{"items":{"format":"email"}}"#).unwrap();
    let mut governor = Governor::new(Budget {
        evidence_entries: 1,
        ..Budget::default()
    });
    let result = schema.validate_with(&json::parse(r#"["bad","bad"]"#).unwrap(), &mut governor);
    assert!(matches!(
        result,
        Err(StopReason::BudgetExceeded(Resource::EvidenceEntries))
    ));
}
