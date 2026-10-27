// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Caller-owned catalog preparation, outside callable-binding governance.

use purrdf_core::{
    BlankScope, RdfDatasetBuilder, TermValue,
    fno::{
        FnBlank, FnFunction, FnImpl, FnMapping, FnOutput, FnParam, FnParamMapping, FnReturnMapping,
        FnoCatalog, FnoCatalogInput, to_quads,
    },
};

pub(crate) fn input() -> FnoCatalogInput {
    let text = |value: &str| TermValue::Literal {
        lexical_form: value.into(),
        datatype: "http://www.w3.org/1999/02/22-rdf-syntax-ns#langString".into(),
        language: Some("de".into()),
        direction: None,
    };
    let blank = |label: &str| FnBlank {
        label: label.into(),
        scope: BlankScope(42),
    };
    FnoCatalogInput {
        ontology_iri: "urn:example:tools".into(),
        document_iri: "urn:example:catalog".into(),
        doc_label: text("Werkzeuge"),
        banner: text("Beispiel"),
        functions: vec![FnFunction {
            iri: "urn:example:read".into(),
            label: text("Lesen"),
            description: Some(text("Datei lesen")),
            kind_types: vec![],
            see_also: None,
            expects: vec!["urn:example:z".into(), "urn:example:a".into()],
            output: FnOutput {
                iri: "urn:example:result".into(),
                predicate: Some("urn:example:value".into()),
                r#type: "http://www.w3.org/2001/XMLSchema#string".into(),
                label: None,
                description: None,
            },
        }],
        params: ["a", "z"]
            .into_iter()
            .map(|name| FnParam {
                iri: format!("urn:example:{name}"),
                predicate: Some(format!("urn:example:{name}-value")),
                r#type: "http://www.w3.org/2001/XMLSchema#string".into(),
                required: true,
                label: Some(text(name)),
                description: None,
            })
            .collect(),
        implementations: vec![FnImpl {
            iri: "urn:example:reader".into(),
            format: "application/vnd.purrassist.host-tool".into(),
            see_also: "urn:example:host:reader".into(),
        }],
        mappings: vec![FnMapping {
            node: blank("read"),
            label: text("Bindung"),
            function: "urn:example:read".into(),
            implementation: "urn:example:reader".into(),
            parameter_mappings: ["a", "z"]
                .into_iter()
                .map(|name| FnParamMapping {
                    node: blank(name),
                    label: text(name),
                    function_parameter: format!("urn:example:{name}"),
                    implementation_property: format!("{name} value"),
                })
                .collect(),
            return_mappings: vec![FnReturnMapping {
                node: blank("result"),
                label: text("Ergebnis"),
                function_output: "urn:example:result".into(),
                implementation_property: "z value".into(),
            }],
        }],
    }
}

pub(crate) fn read() -> Result<FnoCatalog, Box<dyn std::error::Error>> {
    let catalog = FnoCatalog::try_new(input())?;
    let mut builder = RdfDatasetBuilder::new();
    for quad in to_quads(&catalog) {
        builder.push_owned_quad(&quad)?;
    }
    let dataset = builder.freeze()?;
    Ok(FnoCatalog::from_dataset(&dataset, "urn:example:tools")?)
}
