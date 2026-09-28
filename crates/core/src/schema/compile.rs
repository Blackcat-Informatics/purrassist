// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{Index, Location, SchemaError, Vocabularies};
use crate::json::{Node, Value, pointer_segment};
use crate::pattern::Pattern;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn walk(
    node: &Node,
    document: usize,
    route: &[usize],
    pointer: &str,
    parent_base: &str,
    index: &mut Index,
    resources: &BTreeMap<String, String>,
) -> Result<usize, SchemaError> {
    if let Some(id) = index.by_route.get(&(document, route.to_vec())) {
        return Ok(*id);
    }
    let fail = |message: String| SchemaError {
        location: format!("{parent_base}#{pointer}"),
        message,
    };
    if !matches!(node.value, Value::Bool(_) | Value::Object(_)) {
        return Err(fail("schema must be an object or boolean".into()));
    }
    let base = node
        .get("$id")
        .and_then(Node::as_str)
        .map_or_else(|| parent_base.into(), |id| resolve_uri(parent_base, id));
    let base = base.trim_end_matches('#').to_string();
    if base.contains('#') {
        return Err(fail("$id cannot contain a nonempty fragment".into()));
    }
    let inherited = (0..route.len())
        .rev()
        .find_map(|length| {
            index
                .by_route
                .get(&(document, route[..length].to_vec()))
                .map(|id| index.locations[*id].vocabularies)
        })
        .unwrap_or_default();
    let vocabularies = node
        .get("$schema")
        .and_then(Node::as_str)
        .map_or(Ok(inherited), |dialect| {
            Vocabularies::load(dialect, resources)
        })?;
    check_keywords(node, vocabularies, &fail)?;
    let id = index.locations.len();
    index.by_route.insert((document, route.to_vec()), id);
    index.locations.push(Location {
        document,
        vocabularies,
        route: route.to_vec(),
        pointer: pointer.to_string(),
        base: base.clone(),
        children: BTreeMap::new(),
        patterns: BTreeMap::new(),
        dynamic_anchor: node
            .get("$dynamicAnchor")
            .and_then(Node::as_str)
            .map(str::to_owned),
    });
    if (node.get("$id").is_some() || route.is_empty())
        && let Some(prior) = index.resources.insert(base.clone(), id)
        && prior != id
    {
        return Err(fail(format!("duplicate schema resource identity: {base}")));
    }
    for keyword in ["$anchor", "$dynamicAnchor"] {
        if let Some(anchor) = node.get(keyword).and_then(Node::as_str) {
            if anchor.is_empty()
                || !anchor.bytes().enumerate().all(|(position, byte)| {
                    byte.is_ascii_alphabetic()
                        || byte == b'_'
                        || (position > 0 && (byte.is_ascii_digit() || matches!(byte, b'-' | b'.')))
                })
            {
                return Err(fail("invalid anchor name".into()));
            }
            let key = format!("{base}#{anchor}");
            if let Some(prior) = index.anchors.insert(key, id)
                && prior != id
            {
                return Err(fail("duplicate anchor".into()));
            }
        }
    }
    if vocabularies.validation
        && let Some(pattern) = node.get("pattern").and_then(Node::as_str)
    {
        index.locations[id].patterns.insert(
            "pattern".into(),
            Pattern::compile(pattern).map_err(|error| fail(error.to_string()))?,
        );
    }
    compile_children(node, id, index, resources)?;
    Ok(id)
}
fn compile_children(
    node: &Node,
    id: usize,
    index: &mut Index,
    resources: &BTreeMap<String, String>,
) -> Result<(), SchemaError> {
    let location = index.locations[id].clone();
    let document = location.document;
    let route = location.route;
    let pointer = location.pointer;
    let base = location.base;
    let vocabularies = location.vocabularies;
    let fail = |message: String| SchemaError {
        location: format!("{base}#{pointer}"),
        message,
    };
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    for (position, member) in members.iter().enumerate() {
        if !vocabularies.enabled(&member.key) {
            continue;
        }
        let mut child_route = route.clone();
        child_route.push(position);
        let child_pointer = format!("{pointer}/{}", pointer_segment(&member.key));
        match member.key.as_str() {
            "additionalProperties"
            | "unevaluatedProperties"
            | "propertyNames"
            | "items"
            | "contains"
            | "unevaluatedItems"
            | "not"
            | "if"
            | "then"
            | "else"
            | "contentSchema" => {
                let child = walk(
                    &member.value,
                    document,
                    &child_route,
                    &child_pointer,
                    &base,
                    index,
                    resources,
                )?;
                index.locations[id]
                    .children
                    .insert(member.key.clone(), child);
            }
            "$defs" | "definitions" | "properties" | "patternProperties" | "dependentSchemas" => {
                let Value::Object(children) = &member.value.value else {
                    return Err(fail(format!("{} must be an object", member.key)));
                };
                for (child_position, child) in children.iter().enumerate() {
                    let name = format!("{}/{}", member.key, pointer_segment(&child.key));
                    if member.key == "patternProperties" {
                        index.locations[id].patterns.insert(
                            name.clone(),
                            Pattern::compile(&child.key)
                                .map_err(|error| fail(error.to_string()))?,
                        );
                    }
                    let mut next_route = child_route.clone();
                    next_route.push(child_position);
                    let child_id = walk(
                        &child.value,
                        document,
                        &next_route,
                        &format!("{child_pointer}/{}", pointer_segment(&child.key)),
                        &base,
                        index,
                        resources,
                    )?;
                    index.locations[id].children.insert(name, child_id);
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                let Value::Array(children) = &member.value.value else {
                    return Err(fail(format!("{} must be an array", member.key)));
                };
                if children.is_empty() {
                    return Err(fail(format!("{} must not be empty", member.key)));
                }
                for (child_position, child) in children.iter().enumerate() {
                    let mut next_route = child_route.clone();
                    next_route.push(child_position);
                    let child_id = walk(
                        child,
                        document,
                        &next_route,
                        &format!("{child_pointer}/{child_position}"),
                        &base,
                        index,
                        resources,
                    )?;
                    index.locations[id]
                        .children
                        .insert(format!("{}/{child_position}", member.key), child_id);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn check_keywords(
    node: &Node,
    vocabularies: Vocabularies,
    fail: &impl Fn(String) -> SchemaError,
) -> Result<(), SchemaError> {
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    let mut keys = BTreeSet::new();
    for member in members {
        if !keys.insert(&member.key) {
            return Err(fail(format!("duplicate schema keyword: {}", member.key)));
        }
        if !vocabularies.enabled(&member.key) {
            continue;
        }
        let value = &member.value.value;
        let valid = match member.key.as_str() {
            "$schema" | "$id" | "$ref" | "$dynamicRef" | "$anchor" | "$dynamicAnchor"
            | "pattern" | "format" | "title" | "description" | "contentEncoding"
            | "contentMediaType" => matches!(value, Value::String(_)),
            "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
                matches!(value, Value::Number(_))
            }
            "multipleOf" => {
                matches!(value,Value::Number(number) if number.compare(&"0".parse().expect("zero")).is_gt())
            }
            "minLength" | "maxLength" | "minItems" | "maxItems" | "minProperties"
            | "maxProperties" | "minContains" | "maxContains" => {
                matches!(value,Value::Number(number) if number.is_integer() && !number.compare(&"0".parse().expect("zero")).is_lt())
            }
            "uniqueItems" | "readOnly" | "writeOnly" | "deprecated" => {
                matches!(value, Value::Bool(_))
            }
            "required" => string_set(&member.value),
            "dependentRequired" => {
                matches!(value,Value::Object(entries) if entries.iter().all(|entry|string_set(&entry.value)))
            }
            "enum" => {
                matches!(value, Value::Array(_))
            }
            "type" => match value {
                Value::String(name) => valid_type(name),
                Value::Array(names) => {
                    !names.is_empty()
                        && string_set(&member.value)
                        && names
                            .iter()
                            .all(|name| name.as_str().is_some_and(valid_type))
                }
                _ => false,
            },
            "$vocabulary" => {
                matches!(value,Value::Object(entries) if entries.iter().all(|entry|matches!(entry.value.value,Value::Bool(_))))
            }
            _ => true,
        };
        if !valid {
            return Err(fail(format!("invalid value for {}", member.key)));
        }
    }
    if let Some(Node {
        value: Value::Object(vocabularies),
        ..
    }) = node.get("$vocabulary")
    {
        for vocabulary in vocabularies {
            if vocabulary.value.value == Value::Bool(true)
                && !matches!(
                    vocabulary.key.as_str(),
                    "https://json-schema.org/draft/2020-12/vocab/core"
                        | "https://json-schema.org/draft/2020-12/vocab/applicator"
                        | "https://json-schema.org/draft/2020-12/vocab/unevaluated"
                        | "https://json-schema.org/draft/2020-12/vocab/validation"
                        | "https://json-schema.org/draft/2020-12/vocab/meta-data"
                        | "https://json-schema.org/draft/2020-12/vocab/format-annotation"
                        | "https://json-schema.org/draft/2020-12/vocab/content"
                )
            {
                return Err(fail(format!(
                    "unsupported required vocabulary: {}",
                    vocabulary.key
                )));
            }
        }
    }
    Ok(())
}
fn valid_type(name: &str) -> bool {
    matches!(
        name,
        "null" | "boolean" | "object" | "array" | "number" | "integer" | "string"
    )
}
fn string_set(node: &Node) -> bool {
    let Value::Array(values) = &node.value else {
        return false;
    };
    let mut names = BTreeSet::new();
    values
        .iter()
        .all(|value| value.as_str().is_some_and(|name| names.insert(name)))
}
pub(super) fn resolve_uri(base: &str, reference: &str) -> String {
    super::uri::resolve(base, reference)
}
