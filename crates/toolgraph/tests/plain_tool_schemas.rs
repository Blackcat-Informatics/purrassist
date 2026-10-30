// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Downstream acceptance for the pinned public `FnO` schema API. Worked, logical
//! and loss fixtures adapted from `PurRDF` PR #806 under the same triple license.

#[path = "../examples/support/plain_schemas.rs"]
pub mod fixtures;

use fixtures::{LIMITS, PREFIXES, binding, namespaces, profile};
use purrassist_core::{Budget, Governor, json, schema::Schema};
use purrdf_shapes::json_schema::{
    ToolArgumentProfile, ToolRepresentationError, ToolSchemaError, ToolSchemaLimits, compile,
    compile_tool_arguments,
};

fn schema(source: &str) -> Schema {
    Schema::compile_governed(source, &mut Governor::new(Budget::default()))
        .expect("exact emitted schema compiles with the first-party validator")
}

fn accepts(schema: &Schema, instance: &str) -> bool {
    schema
        .validate_with(
            &json::parse(instance).expect("instance JSON"),
            &mut Governor::new(Budget::default()),
        )
        .expect("validation resources")
        .valid
}

#[test]
fn worked_bare_input_and_output_schemas_are_exact_and_keep_jsonld_neighbors() {
    let shapes =
        purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{}", fixtures::SHAPES), None)
            .expect("worked shapes");
    let ns = namespaces().expect("caller namespace");
    let legacy = compile(&shapes, &ns).expect("JSON-LD compiler");
    for case in fixtures::WORKED {
        let compiled = compile_tool_arguments(
            &shapes,
            &ns,
            &profile(case.root, case.class, case.fields),
            &LIMITS,
        )
        .expect("worked plain schema");
        assert!(
            json::parse(&compiled.schema_json)
                .unwrap()
                .equivalent(&json::parse(case.expected).unwrap()),
            "{}",
            case.root
        );
        assert_eq!(compiled.losses.entries(), []);
        let schema = schema(&compiled.schema_json);
        assert!(accepts(&schema, case.valid), "{}", case.root);
        for invalid in case.invalid {
            assert!(!accepts(&schema, invalid), "{}: {invalid}", case.root);
        }
    }
    let retained = compile(&shapes, &ns).expect("retained JSON-LD compiler");
    assert_eq!(legacy.schema_json, retained.schema_json);
    assert_eq!(legacy.openapi_json, retained.openapi_json);
    assert_eq!(legacy.losses, retained.losses);
}

const NESTED: &str = r"
ex:Input a sh:NodeShape; sh:targetClass ex:InputClass; sh:closed true;
 sh:property [sh:path ex:name; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1];
 sh:property [sh:path ex:children; sh:node ex:Child; sh:minCount 1; sh:maxCount 2];
 sh:property [sh:path ex:enabled; sh:datatype xsd:boolean; sh:maxCount 1].
ex:Output a sh:NodeShape; sh:targetClass ex:OutputClass; sh:closed true;
 sh:property [sh:path ex:child; sh:node ex:Child; sh:minCount 1; sh:maxCount 1].
ex:Child a sh:NodeShape; sh:closed true;
 sh:property [sh:path ex:name; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1].
";

#[test]
fn nested_input_and_output_share_child_bindings_without_cross_parent_aliasing() {
    let shapes = purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{NESTED}"), None).unwrap();
    let ns = namespaces().unwrap();
    for (root, class, fields, valid, invalid) in [
        (
            "Input",
            "InputClass",
            &["name", "children", "enabled"][..],
            &[
                r#"{"label":"parent","children":[{"name":"Ada"}]}"#,
                r#"{"label":"parent","enabled":true,"children":[{"name":"Ada"},{"name":"Max"}]}"#,
            ][..],
            &[
                r#"{"label":"parent","children":[]}"#,
                r#"{"label":"parent","children":[{"name":"a"},{"name":"b"},{"name":"c"}]}"#,
                r#"{"label":"parent","children":{"name":"Ada"}}"#,
                r#"{"label":"parent","children":[{}]}"#,
                r#"{"label":"parent","children":[{"label":"Ada"}]}"#,
                r#"{"label":"parent","children":[{"name":"Ada","extra":1}]}"#,
                r#"{"label":"parent","enabled":"true","children":[{"name":"Ada"}]}"#,
                r#"{"children":[{"name":"Ada"}]}"#,
                r#"{"label":"parent","children":[{"name":"Ada"}],"extra":1}"#,
            ][..],
        ),
        (
            "Output",
            "OutputClass",
            &["child"][..],
            &[r#"{"child":{"name":"Ada"}}"#][..],
            &[
                r#"{"child":[{"name":"Ada"}]}"#,
                r#"{"child":{}}"#,
                r#"{"child":{"name":"Ada","extra":1}}"#,
                "{}",
            ][..],
        ),
    ] {
        let mut p = profile(root, class, fields);
        if root == "Input" {
            p.fields[0].implementation_property = "label".into();
        }
        p.fields.push(binding("Child", "name", "name"));
        let compiled = compile_tool_arguments(&shapes, &ns, &p, &LIMITS).unwrap();
        assert_eq!(compiled.losses.entries(), []);
        let schema = schema(&compiled.schema_json);
        for valid in valid {
            assert!(accepts(&schema, valid), "{root}: {valid}");
        }
        for invalid in invalid {
            assert!(!accepts(&schema, invalid), "{root}: {invalid}");
        }
        p.fields.push(binding(
            if root == "Input" { "Output" } else { "Input" },
            "unused",
            "unused",
        ));
        assert!(matches!(
            compile_tool_arguments(&shapes, &ns, &p, &LIMITS),
            Err(ToolSchemaError::AmbiguousBinding { .. })
        ));
    }
}

#[test]
fn exact_logical_constraints_remain_outside_typed_parameter_subset() {
    let body = r"
ex:Args a sh:NodeShape; sh:targetClass ex:Call; sh:closed true;
 sh:property [sh:path ex:limit; sh:datatype xsd:integer; sh:maxCount 1;
  sh:xone (
   [sh:datatype xsd:integer; sh:minInclusive 1; sh:maxInclusive 3]
   [sh:datatype xsd:integer; sh:minInclusive 3; sh:maxInclusive 5]
  );
  sh:not [sh:datatype xsd:integer; sh:in (2)];
  sh:someValue [sh:datatype xsd:integer; sh:minInclusive 1]].
";
    let shapes = purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{body}"), None).unwrap();
    let compiled = compile_tool_arguments(
        &shapes,
        &namespaces().unwrap(),
        &profile("Args", "Call", &["limit"]),
        &LIMITS,
    )
    .unwrap();
    assert_eq!(compiled.losses.entries(), []);
    let schema = schema(&compiled.schema_json);
    for (instance, valid) in [
        (r#"{"limit":1}"#, true),
        (r#"{"limit":4}"#, true),
        ("{}", false),
        (r#"{"limit":2}"#, false),
        (r#"{"limit":3}"#, false),
        (r#"{"limit":6}"#, false),
    ] {
        assert_eq!(accepts(&schema, instance), valid, "{instance}");
    }
}

#[test]
fn propagated_losses_keep_original_shacl_source_and_do_not_assert_exact_negation() {
    let body = r"
ex:Args a sh:NodeShape; sh:targetClass ex:Call; sh:closed true;
 sh:property [sh:path ex:customer_id; sh:node ex:Lossy; sh:maxCount 1];
 sh:property [sh:path ex:limit; sh:datatype xsd:integer; sh:not ex:Lossy; sh:maxCount 1].
ex:Lossy a sh:NodeShape; sh:datatype xsd:integer; sh:equals ex:other.
";
    let shapes = purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{body}"), None).unwrap();
    let compiled = compile_tool_arguments(
        &shapes,
        &namespaces().unwrap(),
        &profile("Args", "Call", &["customer_id", "limit"]),
        &LIMITS,
    )
    .unwrap();
    for (code, subject) in [
        ("sh:equals", "<https://example.org/tools/Lossy>"),
        ("sh:not", "<https://example.org/tools/Args>"),
    ] {
        assert!(
            compiled
                .losses
                .entries()
                .iter()
                .any(|entry| entry.code == code
                    && entry
                        .location
                        .as_deref()
                        .and_then(|location| location.subject.as_deref())
                        == Some(subject)),
            "{code} at {subject}"
        );
    }
    assert!(accepts(
        &schema(&compiled.schema_json),
        r#"{"customer_id":1,"limit":1}"#
    ));
}

#[test]
fn invalid_roots_bindings_and_representations_are_native_typed_refusals() {
    let shapes =
        purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{}", fixtures::SHAPES), None)
            .unwrap();
    let ns = namespaces().unwrap();
    let valid = profile("QueryInput", "QueryArguments", &["customer_id", "limit"]);
    for wrong in [
        profile("Absent", "QueryArguments", &[]),
        profile("QueryInput", "EditArguments", &["customer_id", "limit"]),
    ] {
        assert!(matches!(
            compile_tool_arguments(&shapes, &ns, &wrong, &LIMITS),
            Err(ToolSchemaError::InvalidRoot { .. })
        ));
    }
    let mut missing = valid.clone();
    missing.fields.pop();
    assert!(
        matches!(compile_tool_arguments(&shapes, &ns, &missing, &LIMITS),
        Err(ToolSchemaError::IncompleteBinding { predicate, .. })
        if predicate == "https://example.org/tools/limit")
    );
    let mut duplicate = valid.clone();
    duplicate.fields.push(duplicate.fields[0].clone());
    assert!(matches!(
        compile_tool_arguments(&shapes, &ns, &duplicate, &LIMITS),
        Err(ToolSchemaError::DuplicateBinding { .. })
    ));
    for member in ["", "@id", "limit"] {
        let mut ambiguous = valid.clone();
        ambiguous.fields[0].implementation_property = member.into();
        assert!(matches!(
            compile_tool_arguments(&shapes, &ns, &ambiguous, &LIMITS),
            Err(ToolSchemaError::AmbiguousBinding { .. })
        ));
    }
    for (datatype, expected) in [
        (
            "sh:datatype xsd:anyURI;",
            ToolRepresentationError::Constraint("sh:datatype"),
        ),
        ("", ToolRepresentationError::MissingDomain),
    ] {
        let body = format!(
            "ex:Args a sh:NodeShape; sh:targetClass ex:Call; sh:property [sh:path ex:flag; {datatype} sh:maxCount 1]."
        );
        let shapes =
            purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{body}"), None).unwrap();
        let p = ToolArgumentProfile {
            fields: vec![binding("Args", "flag", "enabled")],
            ..profile("Args", "Call", &[])
        };
        let error = compile_tool_arguments(&shapes, &ns, &p, &LIMITS).unwrap_err();
        assert!(
            matches!(error, ToolSchemaError::UnsupportedConstruct { location, predicate, reason }
            if location.contains("enabled") && predicate.as_deref() == Some("https://example.org/tools/flag")
                && reason == expected)
        );
    }
}

#[test]
fn tiny_native_ceilings_refuse_without_partial_artifacts_and_generous_neighbors_work() {
    let shapes =
        purrdf_shapes::engine::parse_shapes(&format!("{PREFIXES}{}", fixtures::SHAPES), None)
            .unwrap();
    let p = profile("QueryInput", "QueryArguments", &["customer_id", "limit"]);
    for (resource, limits) in [
        ("work", ToolSchemaLimits { work: 0, ..LIMITS }),
        ("bytes", ToolSchemaLimits { bytes: 0, ..LIMITS }),
        ("nodes", ToolSchemaLimits { nodes: 0, ..LIMITS }),
    ] {
        assert!(
            matches!(compile_tool_arguments(&shapes, &namespaces().unwrap(), &p, &limits),
            Err(ToolSchemaError::Exhausted { resource: actual, limit: 0, .. }) if actual == resource)
        );
        assert!(compile_tool_arguments(&shapes, &namespaces().unwrap(), &p, &LIMITS).is_ok());
    }
}

#[path = "support/legacy_schema.rs"]
pub mod legacy;

#[test]
fn legacy_jsonld_schema_and_openapi_goldens_from_previous_pin_are_unchanged() {
    use purrdf_shapes::json_schema::{SchemaCompileRequest, SchemaSurfaceMode, compile_schema};
    let shapes = purrdf_shapes::engine::parse_shapes(legacy::SHAPES, None).unwrap();
    let ontology =
        purrdf_shapes::text_ingest::parse_turtle_to_dataset(legacy::ONTOLOGY, None).unwrap();
    let ns = purrdf_shapes::json_schema::Namespaces::new(
        "ex",
        &[("ex".into(), "https://example.org/schema/".into())],
    )
    .unwrap();
    let request = SchemaCompileRequest::new(
        &shapes,
        &ns,
        ontology.as_ref(),
        SchemaSurfaceMode::ShapedOnly,
    );
    let compiled = compile_schema(&request).expect("original public JSON-LD compiler");
    assert_eq!(compiled.compiled.schema_json, legacy::SCHEMA);
    assert_eq!(compiled.compiled.openapi_json, legacy::OPENAPI);
}

#[expect(
    clippy::result_large_err,
    reason = "the consumer exercises the original public compiler error and native cause"
)]
fn numeric_schema(
    datatype: &str,
    constraints: &str,
) -> Result<purrdf_shapes::json_schema::CompiledSchema, ToolSchemaError> {
    let body = format!(
        "{PREFIXES}ex:Args a sh:NodeShape; sh:targetClass ex:Call; sh:closed true; \
         sh:property [sh:path ex:number; sh:datatype xsd:{datatype}; \
         sh:minCount 1; sh:maxCount 1; {constraints}]."
    );
    let shapes = purrdf_shapes::engine::parse_shapes(&body, None).expect("numeric source shapes");
    compile_tool_arguments(
        &shapes,
        &namespaces().unwrap(),
        &profile("Args", "Call", &["number"]),
        &LIMITS,
    )
}

#[test]
fn plain_decimal_boundaries_enumerations_and_constants_keep_full_precision() {
    let low = "9007199254740993.0000000000000001";
    let middle = "9007199254740993.0000000000000002";
    let high = "9007199254740993.0000000000000003";
    let below = "9007199254740993.0000000000000000";
    let above = "9007199254740993.0000000000000004";
    for (constraints, valid, invalid) in [
        (
            format!(
                "sh:minInclusive \"{low}\"^^xsd:decimal; sh:maxInclusive \"{high}\"^^xsd:decimal;"
            ),
            vec![low, middle, high],
            vec![below, above],
        ),
        (
            format!("sh:in (\"{low}\"^^xsd:decimal \"{high}\"^^xsd:decimal);"),
            vec![low, high],
            vec![middle],
        ),
        (
            format!("sh:hasValue \"{middle}\"^^xsd:decimal;"),
            vec![middle],
            vec![low, high],
        ),
    ] {
        let compiled = numeric_schema("decimal", &constraints).expect("exact decimal schema");
        assert_eq!(compiled.losses.entries(), []);
        let validator = schema(&compiled.schema_json);
        for number in valid {
            assert!(
                accepts(&validator, &format!("{{\"number\":{number}}}")),
                "{constraints}: {number}"
            );
        }
        for number in invalid {
            assert!(
                !accepts(&validator, &format!("{{\"number\":{number}}}")),
                "{constraints}: {number}"
            );
        }
    }
}

#[test]
fn exact_decimal_logic_preserves_exclusive_alternatives_and_negation() {
    let compiled = numeric_schema(
        "decimal",
        r"sh:xone (
             [sh:datatype xsd:decimal; sh:minInclusive 1.1; sh:maxInclusive 3.3]
             [sh:datatype xsd:decimal; sh:minInclusive 3.3; sh:maxInclusive 5.5]
           );
           sh:not [sh:datatype xsd:decimal; sh:in (2.2)];
           sh:someValue [sh:datatype xsd:decimal; sh:minInclusive 1.5];",
    )
    .expect("same-domain decimal logic");
    assert_eq!(compiled.losses.entries(), []);
    let validator = schema(&compiled.schema_json);
    for (number, valid) in [
        ("1.5", true),
        ("4.4", true),
        ("1.1", false),
        ("2.2", false),
        ("3.3", false),
        ("6", false),
    ] {
        assert_eq!(
            accepts(&validator, &format!("{{\"number\":{number}}}")),
            valid,
            "{number}"
        );
    }
}

#[test]
fn ieee_domain_residuals_are_registered_and_keep_the_source() {
    for datatype in ["double", "float"] {
        let compiled = numeric_schema(datatype, "sh:minInclusive 1.0;")
            .expect("finite numeric carrier with explicit residual");
        purrdf_core::loss::check_ledger_sound(&compiled.losses, "shacl", "json-schema")
            .expect("every residual has a registered native meaning");
        assert!(
            compiled.losses.entries().iter().any(|entry| {
                entry.code == "plain:ieee-representability"
                    && entry.location.as_deref().is_some_and(|location| {
                        location.subject.as_deref() == Some("<https://example.org/tools/Args>")
                    })
            }),
            "{datatype}: {:?}",
            compiled.losses
        );
        let validator = schema(&compiled.schema_json);
        assert!(accepts(&validator, r#"{"number":1.5}"#));
        assert!(!accepts(&validator, r#"{"number":0.5}"#));
        assert!(!accepts(&validator, r#"{"number":"1.5"}"#));
    }
}

#[test]
fn unsupported_numeric_encodings_keep_typed_causes_and_valid_neighbors() {
    for (datatype, invalid, reason, valid) in [
        (
            "double",
            r#"sh:hasValue "INF"^^xsd:double;"#,
            ToolRepresentationError::NumericConstant,
            r#"sh:hasValue "1.5E0"^^xsd:double;"#,
        ),
        (
            "float",
            r#"sh:hasValue "NaN"^^xsd:float;"#,
            ToolRepresentationError::NumericConstant,
            r#"sh:hasValue "1.5E0"^^xsd:float;"#,
        ),
        (
            "double",
            r#"sh:hasValue "-0.0E0"^^xsd:double;"#,
            ToolRepresentationError::SignedZero,
            r#"sh:hasValue "0.0E0"^^xsd:double;"#,
        ),
        (
            "decimal",
            r#"sh:hasValue "+01.50"^^xsd:decimal;"#,
            ToolRepresentationError::NumericIdentity,
            r#"sh:hasValue "1.5"^^xsd:decimal;"#,
        ),
    ] {
        assert!(
            matches!(
                numeric_schema(datatype, invalid),
                Err(ToolSchemaError::UnsupportedConstruct { location, predicate, reason: actual })
                    if location.contains("number")
                        && predicate.as_deref() == Some("https://example.org/tools/number")
                        && actual == reason
            ),
            "{datatype}: {invalid}"
        );
        let compiled = numeric_schema(datatype, valid).expect("canonical finite neighbor");
        assert_eq!(compiled.losses.entries(), []);
        let number = if valid.contains("0.0E0") { "0" } else { "1.5" };
        assert!(accepts(
            &schema(&compiled.schema_json),
            &format!("{{\"number\":{number}}}")
        ));
    }
}
