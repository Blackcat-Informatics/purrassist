// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{
    CompilationError, Dialect, Index, Location, SchemaError, Vocabularies, build::Context,
};
use super::{index, reference};
use crate::contract::Resource;
use crate::contract::format_scratch_governed;
use crate::json::{Node, Value};
use crate::pattern::{OwnedPatternCompileError, Pattern};
use crate::{Governor, ScratchString, ScratchVec};

pub(super) fn walk(
    node: &Node,
    document: usize,
    route: &[usize],
    pointer: &str,
    parent_base: &str,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<usize, CompilationError> {
    walk_at(
        node,
        Site {
            document,
            route,
            pointer,
            base: parent_base,
        },
        None,
        index,
        context,
    )
}

/// Borrowed traversal metadata stays live outside the growing index arenas.
#[derive(Clone, Copy)]
struct Site<'a> {
    document: usize,
    route: &'a [usize],
    pointer: &'a str,
    base: &'a str,
}
impl Site<'_> {
    const fn child<'a>(&'a self, route: &'a [usize], pointer: &'a str) -> Site<'a> {
        Site {
            document: self.document,
            route,
            pointer,
            base: self.base,
        }
    }
}

fn walk_at(
    node: &Node,
    site: Site<'_>,
    inherited: Option<Vocabularies>,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<usize, CompilationError> {
    let Site {
        document,
        route,
        pointer,
        base: parent_base,
    } = site;
    context.charge(Resource::Depth, route.len())?;
    reference::work(context.governor, 1)?;
    if context.initial_routes.is_none()
        && let Some(id) = index
            .by_route
            .get_governed(&(document, route), context.governor)?
    {
        return Ok(*id);
    }
    let fail = Failure {
        base: parent_base,
        pointer,
    };
    if !matches!(node.value, Value::Bool(_) | Value::Object(_)) {
        return Err(fail.error(
            format_args!("schema must be an object or boolean"),
            context.governor,
        ));
    }
    let inherited = match inherited {
        Some(vocabularies) => vocabularies,
        None => inherited_vocabularies(document, route, index, context.governor)?,
    };
    let declared = member(node, "$schema", context.governor)?.and_then(Node::as_str);
    let dialect = match declared {
        Some(uri) => declared_dialect(uri, context.governor)?.unwrap_or_default(),
        None => inherited.dialect,
    };
    let identity = identify(node, parent_base, pointer, dialect, context.governor)?;
    let base = identity
        .base
        .as_ref()
        .map_or(parent_base, ScratchString::as_str);
    let (vocabularies, unsupported) = match declared {
        Some(dialect) => Vocabularies::load(dialect, context)?,
        None => (inherited, ScratchVec::new(context.governor)),
    };
    record_unsupported_vocabularies(node, &unsupported, index, context)?;
    check_keywords(node, vocabularies, &fail, context.governor)?;
    let id = register_location(node, Site { base, ..site }, vocabularies, index, context)?;
    if (identity.resource || route.is_empty())
        && let Some(prior) = index.resources.insert(
            reference::copy(base, context.governor)?,
            id,
            context.governor,
        )?
        && prior != id
    {
        return Err(index_error(
            parent_base,
            pointer,
            format_args!("duplicate schema resource identity: {base}"),
            context.governor,
        ));
    }
    register_anchors(
        node,
        id,
        (base, parent_base, pointer),
        identity.anchor,
        index,
        context,
    )?;
    register_pattern(node, id, vocabularies, &fail, index, context)?;
    compile_children(
        node,
        id,
        Site { base, ..site },
        vocabularies,
        index,
        context,
    )?;
    Ok(id)
}

fn register_location(
    node: &Node,
    site: Site<'_>,
    vocabularies: Vocabularies,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<usize, CompilationError> {
    let Site {
        document,
        route,
        pointer,
        base,
    } = site;
    let id = index.locations.len();
    let route_key = index::Route {
        document,
        positions: index::route(route, context.governor)?,
    };
    if let Some(routes) = &mut context.initial_routes {
        reference::work(context.governor, 1)?;
        if routes.len() == routes.capacity() {
            reference::work(context.governor, routes.len())?;
        }
        routes.push((route_key, id))?;
    } else {
        index.by_route.insert(route_key, id, context.governor)?;
    }
    index.locations.push(
        Location {
            document,
            vocabularies,
            route: index::route(route, context.governor)?,
            pointer: reference::copy(pointer, context.governor)?,
            base: reference::copy(base, context.governor)?,
            children: index::Map::new(context.governor),
            patterns: index::Map::new(context.governor),
            dynamic_anchor: dynamic_anchor(node, vocabularies, context.governor)?,
        },
        context.governor,
    )?;
    Ok(id)
}

fn register_pattern(
    node: &Node,
    id: usize,
    vocabularies: Vocabularies,
    fail: &Failure<'_>,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    if vocabularies.validation
        && applies(vocabularies, node, "pattern", context.governor)?
        && let Some(pattern) = member(node, "pattern", context.governor)?.and_then(Node::as_str)
    {
        index.locations[id].patterns.insert(
            reference::copy("pattern", context.governor)?,
            compile_pattern(pattern, fail, context)?,
            context.governor,
        )?;
    }
    Ok(())
}

/// Find the closest registered ancestor or document vocabulary declaration.
fn inherited_vocabularies(
    document: usize,
    route: &[usize],
    index: &Index,
    governor: &mut Governor,
) -> Result<Vocabularies, CompilationError> {
    let mut inherited = None;
    for length in (0..route.len()).rev() {
        if let Some(id) = index
            .by_route
            .get_governed(&(document, &route[..length]), governor)?
        {
            inherited = Some(index.locations[*id].vocabularies);
            break;
        }
    }
    Ok(inherited
        .or(index
            .document_dialects
            .get_governed(&document, governor)?
            .copied())
        .unwrap_or_default())
}
/// Register draft-07 and named anchors after the resource identity.
fn register_anchors(
    node: &Node,
    id: usize,
    location: (&str, &str, &str),
    legacy_anchor: Option<ScratchString>,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    let (base, parent_base, pointer) = location;
    let vocabularies = index.locations[id].vocabularies;
    if let Some(anchor) = legacy_anchor
        && let Some(prior) = index.anchors.insert(anchor, id, context.governor)?
        && prior != id
    {
        return Err(index_error(
            parent_base,
            pointer,
            format_args!("duplicate anchor"),
            context.governor,
        ));
    }
    for keyword in ["$anchor", "$dynamicAnchor"] {
        if !applies(vocabularies, node, keyword, context.governor)? {
            continue;
        }
        if let Some(anchor) = member(node, keyword, context.governor)?.and_then(Node::as_str) {
            if anchor.is_empty() {
                return Err(index_error(
                    parent_base,
                    pointer,
                    format_args!("invalid anchor name"),
                    context.governor,
                ));
            }
            for (position, byte) in anchor.bytes().enumerate() {
                reference::work(context.governor, 1)?;
                if !(byte.is_ascii_alphabetic()
                    || byte == b'_'
                    || (position > 0 && (byte.is_ascii_digit() || matches!(byte, b'-' | b'.'))))
                {
                    return Err(index_error(
                        parent_base,
                        pointer,
                        format_args!("invalid anchor name"),
                        context.governor,
                    ));
                }
            }
            let mut key = reference::copy(base, context.governor)?;
            reference::append(&mut key, "#", context.governor)?;
            reference::append(&mut key, anchor, context.governor)?;
            if let Some(prior) = index.anchors.insert(key, id, context.governor)?
                && prior != id
            {
                return Err(index_error(
                    parent_base,
                    pointer,
                    format_args!("duplicate anchor"),
                    context.governor,
                ));
            }
        }
    }
    Ok(())
}
/// Where a schema object sits in the URI space: its base URI, whether its
/// `$id` establishes a schema resource there, and an anchor URI it declares.
struct Identity {
    /// None inherits the already-validated parent base without copying it.
    base: Option<crate::ScratchString>,
    resource: bool,
    anchor: Option<crate::ScratchString>,
}
/// The identity `$id` gives `node` under `dialect`. Draft 2020-12 `$id`
/// is a resource URI with no (or an empty) fragment.
fn identify(
    node: &Node,
    parent_base: &str,
    pointer: &str,
    dialect: Dialect,
    governor: &mut crate::Governor,
) -> Result<Identity, CompilationError> {
    if dialect == Dialect::Draft07 {
        return draft07_identity(node, parent_base, pointer, governor);
    }
    let Some(id) = member(node, "$id", governor)?.and_then(Node::as_str) else {
        return Ok(Identity {
            base: None,
            resource: false,
            anchor: None,
        });
    };
    let resolved = super::uri::resolve(parent_base, id, governor)?;
    let mut end = resolved.len();
    while end > 0 {
        super::reference::work(governor, 1)?;
        if resolved.as_bytes()[end - 1] != b'#' {
            break;
        }
        end -= 1;
    }
    let base = &resolved[..end];
    if super::uri::find(base, b'#', governor)?.is_some() {
        return Err(identity_error(
            parent_base,
            pointer,
            "$id cannot contain a nonempty fragment",
            governor,
        ));
    }
    Ok(Identity {
        base: Some(super::reference::copy(base, governor)?),
        resource: true,
        anchor: None,
    })
}
pub(super) fn identity_error(
    base: &str,
    pointer: &str,
    message: &'static str,
    governor: &mut crate::Governor,
) -> CompilationError {
    let location = (|| {
        let mut location = super::reference::copy(base, governor)?;
        super::reference::append(&mut location, "#", governor)?;
        super::reference::append(&mut location, pointer, governor)?;
        Ok::<_, crate::StopReason>(location)
    })();
    match location {
        Ok(location) => {
            super::reference::Error::Invalid { location, message }.compilation(governor)
        }
        Err(reason) => reason.into(),
    }
}
/// Draft-07 ignores `$id` beside `$ref`, and permits plain-name anchors.
fn draft07_identity(
    node: &Node,
    parent_base: &str,
    pointer: &str,
    governor: &mut crate::Governor,
) -> Result<Identity, CompilationError> {
    let id = member(node, "$id", governor)?;
    let id = if id.is_some() && member(node, "$ref", governor)?.is_some() {
        None
    } else {
        id
    };
    let Some(id) = id.and_then(Node::as_str) else {
        return Ok(Identity {
            base: None,
            resource: false,
            anchor: None,
        });
    };
    let resolved = super::uri::resolve(parent_base, id, governor)?;
    let split = super::uri::find(&resolved, b'#', governor)?;
    let (resource, fragment) = split.map_or((resolved.as_str(), ""), |i| {
        (&resolved[..i], &resolved[i + 1..])
    });
    let relocates = !id.starts_with('#');
    let anchor = if fragment.is_empty() {
        None
    } else {
        for (position, byte) in fragment.bytes().enumerate() {
            super::reference::work(governor, 1)?;
            if !(byte.is_ascii_alphabetic()
                || (position > 0
                    && (byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b':' | b'.'))))
            {
                return Err(identity_error(
                    parent_base,
                    pointer,
                    "$id fragment must be a plain name",
                    governor,
                ));
            }
        }
        let mut anchor = super::reference::copy(resource, governor)?;
        super::reference::append(&mut anchor, "#", governor)?;
        super::reference::append(&mut anchor, fragment, governor)?;
        Some(anchor)
    };
    Ok(Identity {
        base: if relocates {
            Some(super::reference::copy(resource, governor)?)
        } else {
            None
        },
        resource: relocates,
        anchor,
    })
}
/// Error spellings retain their original location while admitting every copy.
struct Failure<'a> {
    base: &'a str,
    pointer: &'a str,
}
impl Failure<'_> {
    fn error(
        &self,
        message: core::fmt::Arguments<'_>,
        governor: &mut Governor,
    ) -> CompilationError {
        index_error(self.base, self.pointer, message, governor)
    }
}

/// Preserve `Node::get`'s unique-member semantics, including duplicate rejection.
pub(super) fn member<'a>(
    node: &'a Node,
    key: &str,
    governor: &mut Governor,
) -> Result<Option<&'a Node>, crate::StopReason> {
    reference::work(governor, 1)?;
    let Value::Object(members) = &node.value else {
        return Ok(None);
    };
    let mut found = None;
    for entry in members {
        // Reject unequal lengths before admitting or comparing key bytes.
        reference::work(governor, 1)?;
        if entry.key.len() != key.len() {
            continue;
        }
        reference::work(governor, key.len())?;
        if entry.key == key {
            if found.is_some() {
                return Ok(None);
            }
            found = Some(&entry.value);
        }
    }
    Ok(found)
}

pub(super) fn applies(
    vocabularies: Vocabularies,
    node: &Node,
    keyword: &str,
    governor: &mut Governor,
) -> Result<bool, crate::StopReason> {
    reference::work(governor, keyword.len().saturating_add(1))?;
    Ok(vocabularies.enabled(keyword)
        && (vocabularies.dialect != Dialect::Draft07
            || matches!(keyword, "$ref" | "$schema")
            || member(node, "$ref", governor)?.is_none()))
}

fn declared_dialect(
    uri: &str,
    governor: &mut Governor,
) -> Result<Option<Dialect>, crate::StopReason> {
    let mut end = uri.len();
    while end > 0 {
        reference::work(governor, 1)?;
        if uri.as_bytes()[end - 1] != b'#' {
            break;
        }
        end -= 1;
    }
    let uri = &uri[..end];
    for dialect in [Dialect::Draft202012, Dialect::Draft07] {
        reference::work(
            governor,
            uri.len().min(dialect.uri().len()).saturating_add(1),
        )?;
        if uri == dialect.uri() {
            return Ok(Some(dialect));
        }
    }
    Ok(None)
}

fn dynamic_anchor(
    node: &Node,
    vocabularies: Vocabularies,
    governor: &mut Governor,
) -> Result<Option<ScratchString>, crate::StopReason> {
    if !applies(vocabularies, node, "$dynamicAnchor", governor)? {
        return Ok(None);
    }
    member(node, "$dynamicAnchor", governor)?
        .and_then(Node::as_str)
        .map(|text| reference::copy(text, governor))
        .transpose()
}

/// Draft-07 tuple form: `items` is an array of positional schemas.
fn is_tuple(vocabularies: Vocabularies, member: &crate::json::Member) -> bool {
    vocabularies.dialect == Dialect::Draft07
        && member.key == "items"
        && matches!(member.value.value, Value::Array(_))
}
#[derive(Clone, Copy)]
enum Children {
    Single,
    Object,
    Array,
}

/// Classify before constructing any route or pointer for a scalar keyword.
fn child_kind(member: &crate::json::Member, vocabularies: Vocabularies) -> Option<Children> {
    match member.key.as_str() {
        "items" if is_tuple(vocabularies, member) => Some(Children::Array),
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
        | "contentSchema" => Some(Children::Single),
        "$defs" | "definitions" | "properties" | "patternProperties" | "dependentSchemas"
        | "dependencies" => Some(Children::Object),
        "allOf" | "anyOf" | "oneOf" | "prefixItems" => Some(Children::Array),
        _ => None,
    }
}

/// A location's child table is not queried until all of its children are compiled.
struct ChildEntries {
    location: usize,
    entries: ScratchVec<(ScratchString, usize)>,
}
impl ChildEntries {
    fn push(
        &mut self,
        name: ScratchString,
        child: usize,
        governor: &mut Governor,
    ) -> Result<(), crate::StopReason> {
        reference::work(governor, 1)?;
        if self.entries.len() == self.entries.capacity() {
            reference::work(governor, self.entries.len())?;
        }
        self.entries.push((name, child))
    }
}

fn compile_children(
    node: &Node,
    id: usize,
    site: Site<'_>,
    vocabularies: Vocabularies,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    let fail = Failure {
        base: site.base,
        pointer: site.pointer,
    };
    let mut pending = ChildEntries {
        location: id,
        entries: ScratchVec::new(context.governor),
    };
    for (position, member) in members.iter().enumerate() {
        context.charge(Resource::Work, member.key.len().saturating_add(1))?;
        if !applies(vocabularies, node, &member.key, context.governor)? {
            continue;
        }
        let Some(kind) = child_kind(member, vocabularies) else {
            continue;
        };
        let mut child_route = index::route(site.route, context.governor)?;
        index::push(&mut child_route, position, context.governor)?;
        let child_pointer = index::pointer(site.pointer, &member.key, context.governor)?;
        let child_site = site.child(&child_route, &child_pointer);
        match kind {
            Children::Single => {
                let child = walk_at(
                    &member.value,
                    child_site,
                    Some(vocabularies),
                    index,
                    context,
                )?;
                pending.push(
                    reference::copy(&member.key, context.governor)?,
                    child,
                    context.governor,
                )?;
            }
            Children::Object => compile_object_children(
                member,
                &mut pending,
                child_site,
                vocabularies,
                &fail,
                index,
                context,
            )?,
            Children::Array => compile_array_children(
                member,
                &mut pending,
                child_site,
                vocabularies,
                &fail,
                index,
                context,
            )?,
        }
    }
    index.locations[id].children = index::Map::from_entries(pending.entries, context.governor)?;
    Ok(())
}

fn compile_object_children(
    member: &crate::json::Member,
    pending: &mut ChildEntries,
    site: Site<'_>,
    vocabularies: Vocabularies,
    fail: &Failure<'_>,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    let Value::Object(children) = &member.value.value else {
        return Err(fail.error(
            format_args!("{} must be an object", member.key),
            context.governor,
        ));
    };
    // A pointer may have compiled this object container before its containing
    // schema was discovered. Preserve that nearer recorded dialect, if present.
    let vocabularies = if context.initial_routes.is_some() {
        vocabularies
    } else {
        match index
            .by_route
            .get_governed(&(site.document, site.route), context.governor)?
        {
            Some(container) => index.locations[*container].vocabularies,
            None => vocabularies,
        }
    };
    for (position, child) in children.iter().enumerate() {
        reference::work(context.governor, member.key.len().saturating_add(1))?;
        // Draft-07 dependencies can be name arrays rather than schemas.
        if matches!(child.value.value, Value::Array(_)) && member.key == "dependencies" {
            continue;
        }
        let name = index::pointer(&member.key, &child.key, context.governor)?;
        if member.key == "patternProperties" {
            let pattern_name = reference::copy(&name, context.governor)?;
            let pattern = compile_pattern(&child.key, fail, context)?;
            index.locations[pending.location].patterns.insert(
                pattern_name,
                pattern,
                context.governor,
            )?;
        }
        let mut next_route = index::route(site.route, context.governor)?;
        index::push(&mut next_route, position, context.governor)?;
        let next_pointer = index::pointer(site.pointer, &child.key, context.governor)?;
        let child_id = walk_at(
            &child.value,
            site.child(&next_route, &next_pointer),
            Some(vocabularies),
            index,
            context,
        )?;
        pending.push(name, child_id, context.governor)?;
    }
    Ok(())
}

/// Positional schemas borrow the containing walk's live metadata.
fn compile_array_children(
    member: &crate::json::Member,
    pending: &mut ChildEntries,
    site: Site<'_>,
    vocabularies: Vocabularies,
    fail: &Failure<'_>,
    index: &mut Index,
    context: &mut Context<'_>,
) -> Result<(), CompilationError> {
    let children = schema_array(&member.value, &member.key, fail, context.governor)?;
    for (position, child) in children.iter().enumerate() {
        let mut next_route = index::route(site.route, context.governor)?;
        index::push(&mut next_route, position, context.governor)?;
        let next_pointer = format_scratch_governed(
            format_args!("{}/{position}", site.pointer),
            context.governor,
        )?;
        let child_id = walk_at(
            child,
            site.child(&next_route, &next_pointer),
            Some(vocabularies),
            index,
            context,
        )?;
        pending.push(
            format_scratch_governed(format_args!("{}/{position}", member.key), context.governor)?,
            child_id,
            context.governor,
        )?;
    }
    Ok(())
}
fn check_keywords(
    node: &Node,
    vocabularies: Vocabularies,
    fail: &Failure<'_>,
    governor: &mut Governor,
) -> Result<(), CompilationError> {
    governor.check()?;
    let Value::Object(members) = &node.value else {
        return Ok(());
    };
    let mut keys = index::Map::new(governor);
    for entry in members {
        reference::work(governor, entry.key.len().saturating_add(1))?;
        if keys
            .insert(reference::copy(&entry.key, governor)?, (), governor)?
            .is_some()
        {
            return Err(fail.error(
                format_args!("duplicate schema keyword: {}", entry.key),
                governor,
            ));
        }
        if !applies(vocabularies, node, &entry.key, governor)? {
            continue;
        }
        if !valid_keyword(entry, vocabularies, fail, governor)? {
            return Err(fail.error(format_args!("invalid value for {}", entry.key), governor));
        }
    }
    // Lookup precedes applicability in the original diagnostic path.
    let declaration = member(node, "$vocabulary", governor)?;
    if declaration.is_some()
        && applies(vocabularies, node, "$vocabulary", governor)?
        && let Some(Node {
            value: Value::Object(entries),
            ..
        }) = declaration
    {
        for entry in entries {
            reference::work(governor, 1)?;
            if matches!(entry.value.value, Value::Bool(true))
                && !known_required_vocabulary(&entry.key, governor)?
            {
                return Err(fail.error(
                    format_args!("unsupported required vocabulary: {}", entry.key),
                    governor,
                ));
            }
        }
    }
    Ok(())
}

fn valid_keyword(
    entry: &crate::json::Member,
    vocabularies: Vocabularies,
    fail: &Failure<'_>,
    governor: &mut Governor,
) -> Result<bool, CompilationError> {
    let value = &entry.value.value;
    Ok(match entry.key.as_str() {
        "$schema" | "$id" | "$ref" | "$dynamicRef" | "$anchor" | "$dynamicAnchor" | "pattern"
        | "title" | "description" | "contentEncoding" | "contentMediaType" => {
            matches!(value, Value::String(_))
        }
        "format" => match value {
            Value::String(name) => {
                if vocabularies.format_assert && !known_name(name, super::format::NAMES, governor)?
                {
                    return Err(fail.error(
                        format_args!("unknown format under format-assertion: {name}"),
                        governor,
                    ));
                }
                true
            }
            _ => false,
        },
        "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum" => {
            matches!(value, Value::Number(_))
        }
        "multipleOf" => match value {
            Value::Number(number) => positive_number(number.as_str(), governor)?,
            _ => false,
        },
        "minLength" | "maxLength" | "minItems" | "maxItems" | "minProperties" | "maxProperties"
        | "minContains" | "maxContains" => match value {
            Value::Number(number) => nonnegative_integer(number, governor)?,
            _ => false,
        },
        "uniqueItems" | "readOnly" | "writeOnly" | "deprecated" => matches!(value, Value::Bool(_)),
        "required" => string_set(&entry.value, governor)?,
        "dependentRequired" => dependent_sets(value, false, governor)?,
        "dependencies" => dependent_sets(value, true, governor)?,
        "enum" => matches!(value, Value::Array(_)),
        "type" => valid_types(&entry.value, governor)?,
        "$vocabulary" => match value {
            Value::Object(entries) => {
                for entry in entries {
                    reference::work(governor, 1)?;
                    if !matches!(entry.value.value, Value::Bool(_)) {
                        return Ok(false);
                    }
                }
                true
            }
            _ => false,
        },
        _ => true,
    })
}

fn known_name(
    name: &str,
    alternatives: &[&str],
    governor: &mut Governor,
) -> Result<bool, crate::StopReason> {
    for alternative in alternatives {
        reference::work(
            governor,
            name.len().min(alternative.len()).saturating_add(1),
        )?;
        if name == *alternative {
            return Ok(true);
        }
    }
    Ok(false)
}

fn known_required_vocabulary(
    name: &str,
    governor: &mut Governor,
) -> Result<bool, crate::StopReason> {
    known_name(
        name,
        &[
            "https://json-schema.org/draft/2020-12/vocab/core",
            "https://json-schema.org/draft/2020-12/vocab/applicator",
            "https://json-schema.org/draft/2020-12/vocab/unevaluated",
            "https://json-schema.org/draft/2020-12/vocab/validation",
            "https://json-schema.org/draft/2020-12/vocab/meta-data",
            "https://json-schema.org/draft/2020-12/vocab/format-annotation",
            "https://json-schema.org/draft/2020-12/vocab/content",
        ],
        governor,
    )
}

fn dependent_sets(
    value: &Value,
    schemas: bool,
    governor: &mut Governor,
) -> Result<bool, crate::StopReason> {
    let Value::Object(entries) = value else {
        return Ok(false);
    };
    for entry in entries {
        reference::work(governor, 1)?;
        if (!schemas || matches!(entry.value.value, Value::Array(_)))
            && !string_set(&entry.value, governor)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn valid_types(node: &Node, governor: &mut Governor) -> Result<bool, crate::StopReason> {
    const TYPES: &[&str] = &[
        "null", "boolean", "object", "array", "number", "integer", "string",
    ];
    match &node.value {
        Value::String(name) => known_name(name, TYPES, governor),
        Value::Array(names) => {
            if names.is_empty() || !string_set(node, governor)? {
                return Ok(false);
            }
            for name in names {
                reference::work(governor, 1)?;
                if !known_name(
                    name.as_str().expect("string set checked every element"),
                    TYPES,
                    governor,
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// A parsed JSON number is positive exactly when its mantissa is nonzero
/// and its sign is positive; exponent magnitude cannot change either fact.
fn positive_number(spelling: &str, governor: &mut Governor) -> Result<bool, crate::StopReason> {
    for byte in spelling.bytes() {
        reference::work(governor, 1)?;
        match byte {
            b'-' | b'e' | b'E' => return Ok(false),
            b'1'..=b'9' => return Ok(true),
            _ => {}
        }
    }
    Ok(false)
}

fn nonnegative_integer(
    number: &crate::json::Number,
    governor: &mut Governor,
) -> Result<bool, crate::StopReason> {
    use crate::json::IntegerReading;
    let reading = number.integer_magnitude_governed(governor)?;
    reference::work(governor, 1)?;
    Ok(match reading {
        IntegerReading::Exact { negative, .. } => !negative,
        IntegerReading::OutOfRange => !number.as_str().starts_with('-'),
        IntegerReading::Fractional => false,
    })
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
    unsupported: &[ScratchString],
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
        if index
            .unsupported
            .get_governed(uri.as_str(), context.governor)?
            .is_none()
        {
            index.unsupported.insert(
                reference::copy(uri, context.governor)?,
                index::Map::new(context.governor),
                context.governor,
            )?;
        }
        let keywords = index
            .unsupported
            .get_mut_governed(uri.as_str(), context.governor)?
            .expect("optional vocabulary was inserted above");
        for member in members {
            reference::work(context.governor, member.key.len().saturating_add(1))?;
            if !is_known_keyword(&member.key) {
                keywords.insert(
                    reference::copy(&member.key, context.governor)?,
                    (),
                    context.governor,
                )?;
            }
        }
    }
    Ok(())
}

fn string_set(node: &Node, governor: &mut Governor) -> Result<bool, crate::StopReason> {
    governor.check()?;
    let Value::Array(values) = &node.value else {
        return Ok(false);
    };
    let mut names = index::Map::new(governor);
    for value in values {
        reference::work(governor, 1)?;
        let Some(name) = value.as_str() else {
            return Ok(false);
        };
        if names
            .insert(reference::copy(name, governor)?, (), governor)?
            .is_some()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compile_pattern(
    pattern: &str,
    fail: &Failure<'_>,
    context: &mut Context<'_>,
) -> Result<Pattern, CompilationError> {
    // Charged precisely, against the schema's own shared governor, by
    // `compile_owned` itself; a separate flat precharge here would either
    // double-count or under-bound the pattern's real chars/AST allocation.
    match Pattern::compile_owned(pattern, context.governor) {
        Ok(pattern) => Ok(pattern),
        Err(OwnedPatternCompileError::Syntax(error)) => {
            Err(fail.error(format_args!("{error}"), context.governor))
        }
        Err(OwnedPatternCompileError::Resource(reason)) => Err(CompilationError::Resource(reason)),
    }
}

fn index_error(
    base: &str,
    pointer: &str,
    message: core::fmt::Arguments<'_>,
    governor: &mut Governor,
) -> CompilationError {
    let result = (|| {
        let message = format_scratch_governed(message, governor)?;
        let mut location = reference::copy(base, governor)?;
        reference::append(&mut location, "#", governor)?;
        reference::append(&mut location, pointer, governor)?;
        governor.check()?;
        Ok::<_, crate::StopReason>(SchemaError {
            location: location.into_string(),
            message: message.into_string(),
        })
    })();
    match result {
        Ok(error) => CompilationError::InvalidSchema(error),
        Err(reason) => reason.into(),
    }
}

fn schema_array<'a>(
    node: &'a Node,
    keyword: &str,
    fail: &Failure<'_>,
    governor: &mut Governor,
) -> Result<&'a [Node], CompilationError> {
    reference::work(governor, 1)?;
    let Value::Array(children) = &node.value else {
        return Err(fail.error(format_args!("{keyword} must be an array"), governor));
    };
    if children.is_empty() {
        return Err(fail.error(format_args!("{keyword} must not be empty"), governor));
    }
    Ok(children)
}
