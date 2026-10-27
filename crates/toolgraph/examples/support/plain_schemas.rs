// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Shared fixtures for the supported upstream plain schema profile.
//!
//! Adapted from `PurRDF` PR #806's public consumer,
//! `crates/shapes/tests/tool_argument_profile.rs` at
//! `8d0a1c6d9d094e5642ab8af3e96d4262b84a8d77`, under the same triple license.
//! Source/profile preparation is caller-owned; there is no graph loader here.

use purrdf_shapes::json_schema::{
    Namespaces, ToolArgumentProfile, ToolFieldBinding, ToolSchemaLimits,
};

pub(crate) const PREFIXES: &str = r"
@prefix ex: <https://example.org/tools/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
";

pub(crate) const SHAPES: &str = r#"
ex:QueryInput a sh:NodeShape; sh:targetClass ex:QueryArguments; sh:closed true;
 sh:property [sh:path ex:customer_id; sh:datatype xsd:string; sh:pattern "^customer-[0-9]+$"; sh:minCount 1; sh:maxCount 1];
 sh:property [sh:path ex:limit; sh:datatype xsd:integer; sh:minInclusive 1; sh:maxInclusive 100; sh:minCount 1; sh:maxCount 1].
ex:EditInput a sh:NodeShape; sh:targetClass ex:EditArguments; sh:closed true;
 sh:property [sh:path ex:path; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1];
 sh:property [sh:path ex:text; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1].
ex:QueryOutput a sh:NodeShape; sh:targetClass ex:QueryResult; sh:closed true;
 sh:property [sh:path ex:orders; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1].
ex:EditOutput a sh:NodeShape; sh:targetClass ex:EditResult; sh:closed true;
 sh:property [sh:path ex:version; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1].
"#;

pub(crate) const LIMITS: ToolSchemaLimits = ToolSchemaLimits {
    work: 1_000_000,
    bytes: 64_000_000,
    nodes: 1_000_000,
};

pub(crate) fn namespaces() -> Result<Namespaces, String> {
    Namespaces::new("ex", &[("ex".into(), "https://example.org/tools/".into())])
}

pub(crate) fn profile(root: &str, class: &str, fields: &[&str]) -> ToolArgumentProfile {
    ToolArgumentProfile {
        root_shape_iri: format!("https://example.org/tools/{root}"),
        root_class_iri: format!("https://example.org/tools/{class}"),
        fields: fields
            .iter()
            .map(|field| binding(root, field, field))
            .collect(),
    }
}

pub(crate) fn binding(parent: &str, predicate: &str, member: &str) -> ToolFieldBinding {
    ToolFieldBinding {
        parent_shape_iri: format!("https://example.org/tools/{parent}"),
        predicate_iri: format!("https://example.org/tools/{predicate}"),
        implementation_property: member.into(),
    }
}

#[derive(Debug)]
pub(crate) struct WorkedCase {
    pub(crate) root: &'static str,
    pub(crate) class: &'static str,
    pub(crate) fields: &'static [&'static str],
    pub(crate) expected: &'static str,
    pub(crate) valid: &'static str,
    pub(crate) invalid: &'static [&'static str],
}

pub(crate) const WORKED: &[WorkedCase] = &[
    WorkedCase {
        root: "QueryInput",
        class: "QueryArguments",
        fields: &["customer_id", "limit"],
        expected: r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$defs":{},"type":"object","properties":{"customer_id":{"type":"string","pattern":"^customer\\u{2d}(?:[0-9])+$"},"limit":{"type":"integer","minimum":1,"maximum":100}},"required":["customer_id","limit"],"additionalProperties":false}"#,
        valid: r#"{"customer_id":"customer-17","limit":10}"#,
        invalid: &[
            r#"{"customer_id":"customer-17","limit":101}"#,
            r#"{"customer_id":"customer-17","limit":0}"#,
            r#"{"customer_id":"customer-x","limit":10}"#,
            r#"{"customer_id":"customer-17","limit":1.5}"#,
            r#"{"customer_id":"customer-17"}"#,
            r#"{"ex:customer_id":"customer-17","ex:limit":10}"#,
            r#"{"customer_id":{"@value":"customer-17"},"limit":10}"#,
            r#"{"customer_id":"customer-17","limit":10,"@id":"x"}"#,
        ],
    },
    WorkedCase {
        root: "EditInput",
        class: "EditArguments",
        fields: &["path", "text"],
        expected: r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$defs":{},"type":"object","properties":{"path":{"type":"string"},"text":{"type":"string"}},"required":["path","text"],"additionalProperties":false}"#,
        valid: r#"{"path":"src/main.rs","text":"replacement"}"#,
        invalid: &[r#"{"path":"src/main.rs"}"#, r#"{"path":"a","text":1}"#],
    },
    WorkedCase {
        root: "QueryOutput",
        class: "QueryResult",
        fields: &["orders"],
        expected: r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$defs":{},"type":"object","properties":{"orders":{"type":"string"}},"required":["orders"],"additionalProperties":false}"#,
        valid: r#"{"orders":"order-1"}"#,
        invalid: &[r#"{"orders":["order-1"]}"#, "{}"],
    },
    WorkedCase {
        root: "EditOutput",
        class: "EditResult",
        fields: &["version"],
        expected: r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$defs":{},"type":"object","properties":{"version":{"type":"string"}},"required":["version"],"additionalProperties":false}"#,
        valid: r#"{"version":"v2"}"#,
        invalid: &[r#"{"version":"v2","extra":true}"#, "{}"],
    },
];
