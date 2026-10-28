// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Governed schema bundle compilation and typed failure propagation.
use super::{
    DRAFT_07, Dialect, Index, Schema, SchemaError, at_route, builtin, bundle_digest, compile,
    hash_schema_bytes,
};
use crate::{
    Governor, StopReason,
    contract::Resource,
    json::{self, Node, ParseFailure},
};
use std::collections::BTreeMap;

/// Compilation distinguishes invalid schemas from an incomplete resource-bounded attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompilationError {
    /// Schema syntax, vocabulary, keyword, or linkage is invalid.
    InvalidSchema(SchemaError),
    /// Compilation stopped before a complete schema was available.
    Resource(StopReason),
}
impl core::fmt::Display for CompilationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSchema(error) => error.fmt(formatter),
            Self::Resource(reason) => reason.fmt(formatter),
        }
    }
}
impl std::error::Error for CompilationError {}
impl From<SchemaError> for CompilationError {
    fn from(error: SchemaError) -> Self {
        Self::InvalidSchema(error)
    }
}
impl From<StopReason> for CompilationError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}
pub(super) struct Context<'a> {
    pub resources: &'a BTreeMap<String, String>,
    pub governor: &'a mut Governor,
}
impl Context<'_> {
    pub(super) fn parse(&mut self, source: &str, location: &str) -> Result<Node, CompilationError> {
        json::parse_with(source, self.governor).map_err(|error| match error {
            ParseFailure::Syntax(error) => SchemaError {
                location: location.into(),
                message: error.to_string(),
            }
            .into(),
            ParseFailure::Resource(reason) => reason.into(),
        })
    }
    pub(super) fn charge(
        &mut self,
        resource: Resource,
        amount: usize,
    ) -> Result<(), CompilationError> {
        self.governor
            .charge(resource, u64::try_from(amount).unwrap_or(u64::MAX))?;
        Ok(())
    }
}
/// Load the built-in or supplied document `uri` names (fragment ignored),
/// first referenced from location `referrer`. A document without `$schema`
/// inherits a Draft-07 referrer's dialect.
fn load_document(
    schema: &mut Schema,
    context: &mut Context<'_>,
    referrer: usize,
    uri: &str,
) -> Result<(), CompilationError> {
    let resource = uri.split('#').next().unwrap_or(uri);
    let text = builtin::resource(resource)
        .or_else(|| context.resources.get(resource).map(String::as_str))
        .ok_or_else(|| SchemaError {
            location: uri.to_owned(),
            message: "unresolved resource; supply it explicitly".into(),
        })?;
    let node = context.parse(text, resource)?;
    let document = schema.documents.len();
    let vocabularies = schema.index.locations[referrer].vocabularies;
    if vocabularies.dialect == Dialect::Draft07 {
        schema
            .index
            .document_dialects
            .insert(document, vocabularies);
    }
    let id = compile::walk(
        &node,
        document,
        &[],
        "",
        resource,
        &mut schema.index,
        context,
    )?;
    schema.index.resources.insert(resource.into(), id);
    schema.documents.push(node);
    Ok(())
}
pub(super) fn compile(
    source: &str,
    resources: &BTreeMap<String, String>,
    governor: &mut Governor,
) -> Result<Schema, CompilationError> {
    let mut context = Context {
        resources,
        governor,
    };
    // Even unused supplied documents must satisfy input admission. Their actual
    // hashing work is admitted separately, at each bounded hashing checkpoint.
    for text in resources.values() {
        context.charge(Resource::Work, 1)?;
        context.charge(Resource::InputBytes, text.len())?;
    }
    let root = context.parse(source, "")?;
    let mut index = Index::default();
    compile::walk(
        &root,
        0,
        &[],
        "",
        "urn:purrassist:root",
        &mut index,
        &mut context,
    )?;
    let root_digest = hash_schema_bytes(source.as_bytes(), context.governor)?;
    let mut schema = Schema {
        documents: vec![root],
        index,
        root: 0,
        // Private root identity until linkage determines the complete bundle's
        // dialect prefix. No partially prepared Schema leaves this function.
        digest: root_digest,
    };
    let mut cursor = 0;
    while cursor < schema.index.locations.len() {
        context.charge(Resource::Work, 1)?;
        for keyword in ["$ref", "$dynamicRef"] {
            let Some(reference) = schema.body(cursor).get(keyword).and_then(Node::as_str) else {
                continue;
            };
            let base = &schema.index.locations[cursor].base;
            context.charge(
                Resource::Work,
                base.len().saturating_add(reference.len()).saturating_add(1),
            )?;
            context.charge(
                Resource::ScratchBytes,
                base.len()
                    .saturating_add(reference.len())
                    .saturating_mul(8)
                    .saturating_add(256),
            )?;
            let reference = reference.to_owned();
            let base = base.clone();
            let uri = compile::resolve_uri(&base, &reference);
            let resource = uri.split('#').next().unwrap_or(&uri);
            if !schema.index.resources.contains_key(resource) {
                load_document(&mut schema, &mut context, cursor, &uri)?;
            }
            let target = schema.reference_target(&base, &reference)?;
            if !schema
                .index
                .by_route
                .contains_key(&(target.document, target.route.clone()))
            {
                // Borrow the immutable document separately from the mutable index;
                // reference-only schemas no longer clone entire subtrees.
                let node = at_route(&schema.documents[target.document], &target.route);
                compile::walk(
                    node,
                    target.document,
                    &target.route,
                    &target.pointer,
                    &target.base,
                    &mut schema.index,
                    &mut context,
                )?;
            }
            schema.resolve(&base, &reference)?;
        }
        cursor += 1;
    }
    schema.digest = bundle_digest(
        root_digest,
        resources,
        schema.dialect() == Dialect::Draft07 || schema.index.resources.contains_key(DRAFT_07),
        context.governor,
    )?;
    context.governor.check()?;
    Ok(schema)
}
