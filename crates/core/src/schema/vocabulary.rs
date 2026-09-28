// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{DIALECT, SchemaError};
use crate::json::{Node, Value};
use std::collections::BTreeMap;

#[expect(
    clippy::struct_excessive_bools,
    reason = "JSON Schema vocabularies are independent capability flags"
)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Vocabularies {
    pub validation: bool,
    applicator: bool,
    unevaluated: bool,
    content: bool,
}
impl Default for Vocabularies {
    fn default() -> Self {
        Self {
            validation: true,
            applicator: true,
            unevaluated: true,
            content: true,
        }
    }
}
impl Vocabularies {
    pub(super) fn load(
        dialect: &str,
        resources: &BTreeMap<String, String>,
    ) -> Result<Self, SchemaError> {
        let uri = dialect.trim_end_matches('#');
        if uri == DIALECT {
            return Ok(Self::default());
        }
        let error = |message: String| SchemaError {
            location: uri.into(),
            message,
        };
        let source = resources
            .get(uri)
            .ok_or_else(|| error("unresolved dialect; supply its meta-schema explicitly".into()))?;
        let meta = crate::json::parse(source).map_err(|e| error(e.to_string()))?;
        let Some(declaration) = meta.get("$vocabulary") else {
            return Ok(Self::default());
        };
        let Value::Object(entries) = &declaration.value else {
            return Err(error("$vocabulary must be an object".into()));
        };
        let mut result = Self {
            validation: false,
            applicator: false,
            unevaluated: false,
            content: false,
        };
        for entry in entries {
            let Value::Bool(required) = entry.value.value else {
                return Err(error("vocabulary requirement must be boolean".into()));
            };
            match entry.key.as_str() {
                "https://json-schema.org/draft/2020-12/vocab/core"
                | "https://json-schema.org/draft/2020-12/vocab/meta-data"
                | "https://json-schema.org/draft/2020-12/vocab/format-annotation" => {}
                "https://json-schema.org/draft/2020-12/vocab/validation" => {
                    result.validation = true;
                }
                "https://json-schema.org/draft/2020-12/vocab/applicator" => {
                    result.applicator = true;
                }
                "https://json-schema.org/draft/2020-12/vocab/unevaluated" => {
                    result.unevaluated = true;
                }
                "https://json-schema.org/draft/2020-12/vocab/content" => result.content = true,
                unknown if required => {
                    return Err(error(format!("unsupported required vocabulary: {unknown}")));
                }
                _ => {}
            }
        }
        Ok(result)
    }
    pub(super) fn enabled(self, keyword: &str) -> bool {
        match keyword {
            "type" | "enum" | "const" | "multipleOf" | "maximum" | "exclusiveMaximum"
            | "minimum" | "exclusiveMinimum" | "maxLength" | "minLength" | "pattern"
            | "maxItems" | "minItems" | "uniqueItems" | "maxContains" | "minContains"
            | "maxProperties" | "minProperties" | "required" | "dependentRequired" => {
                self.validation
            }
            "prefixItems"
            | "items"
            | "contains"
            | "additionalProperties"
            | "properties"
            | "patternProperties"
            | "dependentSchemas"
            | "propertyNames"
            | "if"
            | "then"
            | "else"
            | "allOf"
            | "anyOf"
            | "oneOf"
            | "not" => self.applicator,
            "unevaluatedItems" | "unevaluatedProperties" => self.unevaluated,
            "contentSchema" | "contentEncoding" | "contentMediaType" => self.content,
            _ => true,
        }
    }
}
pub(super) struct Keywords<'a> {
    pub node: &'a Node,
    pub vocabularies: Vocabularies,
}
impl<'a> Keywords<'a> {
    pub(super) fn get(&self, keyword: &str) -> Option<&'a Node> {
        self.vocabularies
            .enabled(keyword)
            .then(|| self.node.get(keyword))
            .flatten()
    }
}
impl core::ops::Deref for Keywords<'_> {
    type Target = Node;
    fn deref(&self) -> &Node {
        self.node
    }
}
