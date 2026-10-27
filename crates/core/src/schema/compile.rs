// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{
    CompilationError, Dialect, Index, Location, SchemaError, Vocabularies, build::Context,
};
use crate::contract::Resource;
use crate::json::{Node, Value, pointer_segment};
use crate::pattern::{Pattern, PatternCompileError};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn walk(
    node: &Node,
    document: usize,
    route: &[usize],
    pointer: &str,
    parent_base: &str,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<usize, CompilationError> {
    admit_location(node, route, pointer, parent_base, context)?;
    if let Some(id) = index.by_route.get(&(document, route.to_vec())) {
        return Ok(*id);
    }
    let fail = failure(parent_base, pointer);
    if !matches!(node.value, Value::Bool(_) | Value::Object(_)) {
        return Err(fail("schema must be an object or boolean".into()));
    }
    let inherited = (0..route.len())
        .rev()
        .find_map(|length| {
            index
                .by_route
                .get(&(document, route[..length].to_vec()))
                .map(|id| index.locations[*id].vocabularies)
        })
        .or_else(|| index.document_dialects.get(&document).copied())
        .unwrap_or_default();
    let declared = node.get("$schema").and_then(Node::as_str);
    let dialect = declared.map_or(inherited.dialect, |uri| {
        Dialect::from_uri(uri).unwrap_or_default()
    });
    let identity = identify(node, parent_base, dialect, &fail)?;
    let base = identity.base;
    let (vocabularies, unsupported) = match declared {
        Some(dialect) => Vocabularies::load(dialect, context)?,
        None => (inherited, Vec::new()),
    };
    record_unsupported_vocabularies(node, &unsupported, index, context)?;
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
        dynamic_anchor: vocabularies
            .applies(node, "$dynamicAnchor")
            .then(|| node.get("$dynamicAnchor").and_then(Node::as_str))
            .flatten()
            .map(str::to_owned),
    });
    if (identity.resource || route.is_empty())
        && let Some(prior) = index.resources.insert(base.clone(), id)
        && prior != id
    {
        return Err(fail(format!("duplicate schema resource identity: {base}")));
    }
    if let Some(anchor) = identity.anchor
        && let Some(prior) = index.anchors.insert(anchor, id)
        && prior != id
    {
        return Err(fail("duplicate anchor".into()));
    }
    for keyword in ["$anchor", "$dynamicAnchor"] {
        if !vocabularies.applies(node, keyword) {
            continue;
        }
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
        && vocabularies.applies(node, "pattern")
        && let Some(pattern) = node.get("pattern").and_then(Node::as_str)
    {
        index.locations[id]
            .patterns
            .insert("pattern".into(), compile_pattern(pattern, &fail, context)?);
    }
    compile_children(node, id, index, context)?;
    Ok(id)
}
/// Where a schema object sits in the URI space: its base URI, whether its
/// `$id` establishes a schema resource there, and an anchor URI it declares.
struct Identity {
    base: String,
    resource: bool,
    anchor: Option<String>,
}
/// The identity `$id` gives `node` under `dialect`. Draft 2020-12 `$id`
/// is a resource URI with no (or an empty) fragment.
fn identify(
    node: &Node,
    parent_base: &str,
    dialect: Dialect,
    fail: &impl Fn(String) -> CompilationError,
) -> Result<Identity, CompilationError> {
    if dialect == Dialect::Draft07 {
        return draft07_identity(node, parent_base, fail);
    }
    let base = node
        .get("$id")
        .and_then(Node::as_str)
        .map_or_else(|| parent_base.into(), |id| resolve_uri(parent_base, id));
    let base = base.trim_end_matches('#').to_string();
    if base.contains('#') {
        return Err(fail("$id cannot contain a nonempty fragment".into()));
    }
    Ok(Identity {
        resource: node.get("$id").is_some(),
        base,
        anchor: None,
    })
}
/// Draft-07 `$id`: ignored beside `$ref`; otherwise a URI reference resolved
/// against the parent base whose non-fragment part (if any) establishes a
/// resource and whose plain-name fragment (`#foo`, alone or after a URI)
/// declares a location-independent anchor. A JSON Pointer fragment is
/// rejected, since it cannot identify the schema object that declares it.
fn draft07_identity(
    node: &Node,
    parent_base: &str,
    fail: &impl Fn(String) -> CompilationError,
) -> Result<Identity, CompilationError> {
    let Some(id) = node
        .get("$id")
        .filter(|_| node.get("$ref").is_none())
        .and_then(Node::as_str)
    else {
        return Ok(Identity {
            base: parent_base.into(),
            resource: false,
            anchor: None,
        });
    };
    let resolved = resolve_uri(parent_base, id);
    let (resource, fragment) = resolved.split_once('#').unwrap_or((&resolved, ""));
    let relocates = !id.starts_with('#');
    let anchor = if fragment.is_empty() {
        None
    } else if fragment.bytes().enumerate().all(|(position, byte)| {
        byte.is_ascii_alphabetic()
            || (position > 0
                && (byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b':' | b'.')))
    }) {
        Some(format!("{resource}#{fragment}"))
    } else {
        return Err(fail("$id fragment must be a plain name".into()));
    };
    Ok(Identity {
        base: if relocates { resource } else { parent_base }.into(),
        resource: relocates,
        anchor,
    })
}
/// An invalid-schema error constructor for the location `base#pointer`.
fn failure(base: &str, pointer: &str) -> impl Fn(String) -> CompilationError {
    move |message| {
        CompilationError::InvalidSchema(SchemaError {
            location: format!("{base}#{pointer}"),
            message,
        })
    }
}
/// Draft-07 tuple form: `items` is an array of positional schemas.
fn is_tuple(vocabularies: Vocabularies, member: &crate::json::Member) -> bool {
    vocabularies.dialect == Dialect::Draft07
        && member.key == "items"
        && matches!(member.value.value, Value::Array(_))
}
fn compile_children(
    node: &Node,
    id: usize,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    let (vocabularies, document, route, pointer, base) =
        parent_snapshot(&index.locations[id], context)?;
    let fail = failure(&base, &pointer);
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    for (position, member) in members.iter().enumerate() {
        context.charge(Resource::Work, member.key.len().saturating_add(1))?;
        if !vocabularies.applies(node, &member.key) {
            continue;
        }
        let mut child_route = route.clone();
        child_route.push(position);
        let child_pointer = format!("{pointer}/{}", pointer_segment(&member.key));
        let tuple = is_tuple(vocabularies, member);
        match member.key.as_str() {
            "additionalProperties"
            | "unevaluatedProperties"
            | "propertyNames"
            | "additionalItems"
            | "items"
            | "contains"
            | "unevaluatedItems"
            | "not"
            | "if"
            | "then"
            | "else"
            | "contentSchema"
                if !tuple =>
            {
                let child = walk(
                    &member.value,
                    document,
                    &child_route,
                    &child_pointer,
                    &base,
                    index,
                    context,
                )?;
                index.locations[id]
                    .children
                    .insert(member.key.clone(), child);
            }
            "$defs" | "definitions" | "properties" | "patternProperties" | "dependentSchemas"
            | "dependencies" => {
                let Value::Object(children) = &member.value.value else {
                    return Err(fail(format!("{} must be an object", member.key)));
                };
                for (child_position, child) in children.iter().enumerate() {
                    // Draft-07 `dependencies`: a name array is not a schema.
                    if matches!(child.value.value, Value::Array(_)) && member.key == "dependencies"
                    {
                        continue;
                    }
                    let name = format!("{}/{}", member.key, pointer_segment(&child.key));
                    if member.key == "patternProperties" {
                        index.locations[id]
                            .patterns
                            .insert(name.clone(), compile_pattern(&child.key, &fail, context)?);
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
                        context,
                    )?;
                    index.locations[id].children.insert(name, child_id);
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" | "items" => {
                let children = schema_array(&member.value, &member.key, &fail)?;
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
                        context,
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
    fail: &impl Fn(String) -> CompilationError,
) -> Result<(), CompilationError> {
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    let mut keys = BTreeSet::new();
    for member in members {
        if !keys.insert(&member.key) {
            return Err(fail(format!("duplicate schema keyword: {}", member.key)));
        }
        if !vocabularies.applies(node, &member.key) {
            continue;
        }
        let value = &member.value.value;
        let valid = match member.key.as_str() {
            "$schema" | "$id" | "$ref" | "$dynamicRef" | "$anchor" | "$dynamicAnchor"
            | "pattern" | "title" | "description" | "contentEncoding" | "contentMediaType" => {
                matches!(value, Value::String(_))
            }
            "format" => match value {
                Value::String(name) => {
                    // An unrecognized format under an active
                    // format-assertion vocabulary is a compile-time error;
                    // annotation-only use never rejects an unknown name.
                    if vocabularies.format_assert && !super::format::known(name) {
                        return Err(fail(format!(
                            "unknown format under format-assertion: {name}"
                        )));
                    }
                    true
                }
                _ => false,
            },
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
            // Draft-07: each entry is a schema or a set of property names.
            "dependencies" => {
                matches!(value,Value::Object(entries) if entries.iter().all(|entry| !matches!(entry.value.value, Value::Array(_)) || string_set(&entry.value)))
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
    }) = node
        .get("$vocabulary")
        .filter(|_| vocabularies.applies(node, "$vocabulary"))
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
/// Every keyword this engine gives defined behavior to (core linkage,
/// applicators, validation, unevaluated, content, format, and the
/// meta-data annotations); anything else is a keyword an unsupported
/// optional vocabulary might own, and the unsupported-vocabulary evidence names it.
fn is_known_keyword(key: &str) -> bool {
    matches!(
        key,
        "$schema"
            | "$id"
            | "$ref"
            | "$dynamicRef"
            | "$anchor"
            | "$dynamicAnchor"
            | "$vocabulary"
            | "$defs"
            | "definitions"
            | "type"
            | "enum"
            | "const"
            | "multipleOf"
            | "maximum"
            | "exclusiveMaximum"
            | "minimum"
            | "exclusiveMinimum"
            | "maxLength"
            | "minLength"
            | "pattern"
            | "maxItems"
            | "minItems"
            | "uniqueItems"
            | "maxContains"
            | "minContains"
            | "maxProperties"
            | "minProperties"
            | "required"
            | "dependentRequired"
            | "prefixItems"
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
            | "not"
            | "unevaluatedItems"
            | "unevaluatedProperties"
            | "contentSchema"
            | "contentEncoding"
            | "contentMediaType"
            | "format"
            | "title"
            | "description"
            | "default"
            | "deprecated"
            | "readOnly"
            | "writeOnly"
            | "examples"
    )
}

/// An unknown, optional (`false`) `$vocabulary` entry still
/// compiles, but every compile/recovery receipt must name the URI and the
/// keywords left unevaluated because of it. Scoped to the exact schema
/// resource that declared the dialect (the only schema object whose member
/// keys we can attribute to that specific, otherwise-unknown vocabulary).
fn record_unsupported_vocabularies(
    node: &Node,
    unsupported: &[String],
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    if unsupported.is_empty() {
        return Ok(());
    }
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    for uri in unsupported {
        context.charge(Resource::Work, uri.len())?;
        let keywords = index.unsupported.entry(uri.clone()).or_default();
        for member in members {
            if !is_known_keyword(&member.key) {
                context.charge(Resource::ScratchBytes, member.key.len().saturating_add(32))?;
                keywords.insert(member.key.clone());
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

fn compile_pattern(
    pattern: &str,
    fail: &impl Fn(String) -> CompilationError,
    context: &mut Context<'_>,
) -> Result<Pattern, CompilationError> {
    // Charged precisely, against the schema's own shared governor, by
    // `compile_governed` itself; a separate flat precharge here would either
    // double-count or under-bound the pattern's real chars/AST allocation.
    Pattern::compile_governed(pattern, context.governor).map_err(|error| match error {
        PatternCompileError::Syntax(error) => fail(error.to_string()),
        PatternCompileError::Resource(reason) => CompilationError::Resource(reason),
    })
}

fn admit_location(
    node: &Node,
    route: &[usize],
    pointer: &str,
    parent_base: &str,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    context.charge(Resource::Depth, route.len())?;
    // Routes and inherited vocabulary lookups are quadratic in nesting length.
    context.charge(
        Resource::Work,
        route.len().saturating_mul(route.len()).saturating_add(1),
    )?;
    context.charge(
        Resource::ScratchBytes,
        route
            .len()
            .saturating_mul(route.len())
            .saturating_mul(8)
            .saturating_add(512),
    )?;
    let spelling = match &node.value {
        Value::Object(members) => members.iter().fold(0usize, |sum, member| {
            sum.saturating_add(member.key.len())
                .saturating_add(member.value.as_str().map_or(0, str::len))
                .saturating_add(1)
        }),
        _ => 1,
    };
    context.charge(
        Resource::Work,
        spelling
            .saturating_mul(32)
            .saturating_add(pointer.len())
            .saturating_add(parent_base.len()),
    )?;
    context.charge(
        Resource::ScratchBytes,
        spelling
            .saturating_mul(16)
            .saturating_add(pointer.len().saturating_mul(8))
            .saturating_add(parent_base.len().saturating_mul(8)),
    )?;
    Ok(())
}

fn parent_snapshot(
    location: &Location,
    context: &mut Context<'_>,
) -> Result<(Vocabularies, usize, Vec<usize>, String, String), CompilationError> {
    context.charge(
        Resource::ScratchBytes,
        location
            .base
            .len()
            .saturating_add(location.pointer.len())
            .saturating_add(location.route.len().saturating_mul(8))
            .saturating_add(256),
    )?;
    Ok((
        location.vocabularies,
        location.document,
        location.route.clone(),
        location.pointer.clone(),
        location.base.clone(),
    ))
}

fn schema_array<'a>(
    node: &'a Node,
    keyword: &str,
    fail: &impl Fn(String) -> CompilationError,
) -> Result<&'a [Node], CompilationError> {
    let Value::Array(children) = &node.value else {
        return Err(fail(format!("{keyword} must be an array")));
    };
    if children.is_empty() {
        return Err(fail(format!("{keyword} must not be empty")));
    }
    Ok(children)
}
