// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Governed schema bundle compilation and typed failure propagation.
use super::{
    DRAFT_07, Dialect, Index, Schema, SchemaError, at_route, builtin, bundle_digest, compile,
    hash_schema_bytes, index, reference, uri,
};
use crate::{
    Governor, StopReason,
    contract::{Resource, format_scratch_governed},
    json::{Node, OwnedDocument, OwnedParseFailure},
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
    /// Only the initial document walk emits a complete, strictly ordered route set.
    pub initial_routes: Option<crate::ScratchVec<(index::Route, usize)>>,
}
impl Context<'_> {
    pub(super) fn parse(
        &mut self,
        source: &str,
        location: &str,
    ) -> Result<OwnedDocument, CompilationError> {
        match OwnedDocument::parse(source, self.governor) {
            Ok(document) => Ok(document),
            Err(OwnedParseFailure::Resource(reason)) => Err(reason.into()),
            Err(OwnedParseFailure::Syntax(error)) => {
                let location = reference::copy(location, self.governor)?;
                let message = format_scratch_governed(format_args!("{error}"), self.governor)?;
                self.governor.check()?;
                Err(SchemaError {
                    location: location.into_string(),
                    message: message.into_string(),
                }
                .into())
            }
        }
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
    let resource = &uri[..super::uri::find(uri, b'#', context.governor)?.unwrap_or(uri.len())];
    context.governor.charge(
        Resource::Work,
        u64::try_from(resource.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1)
            .saturating_mul(10),
    )?;
    reference::lookup_work(context.resources.len(), resource.len(), context.governor)?;
    let text = builtin::resource(resource)
        .or_else(|| context.resources.get(resource).map(String::as_str))
        .ok_or_else(|| {
            reference::invalid(
                uri,
                "unresolved resource; supply it explicitly",
                context.governor,
            )
            .compilation(context.governor)
        })?;
    let node = context.parse(text, resource)?;
    let document = schema.documents.len();
    let vocabularies = schema.index.locations[referrer].vocabularies;
    if vocabularies.dialect == Dialect::Draft07 {
        schema
            .index
            .document_dialects
            .insert(document, vocabularies, context.governor)?;
    }
    let id = compile::walk(
        node.root(),
        document,
        &[],
        "",
        resource,
        &mut schema.index,
        context,
    )?;
    schema.index.resources.insert(
        reference::copy(resource, context.governor)?,
        id,
        context.governor,
    )?;
    schema.documents.push(node, context.governor)?;
    Ok(())
}
pub(super) fn compile(
    source: &str,
    resources: &BTreeMap<String, String>,
    governor: &mut Governor,
) -> Result<Schema, CompilationError> {
    let mut context = Context {
        resources,
        initial_routes: Some(crate::ScratchVec::new(governor)),
        governor,
    };
    // Even unused supplied documents must satisfy input admission. Their actual
    // hashing work is admitted separately, at each bounded hashing checkpoint.
    for text in resources.values() {
        context.charge(Resource::Work, 1)?;
        context.charge(Resource::InputBytes, text.len())?;
    }
    let root = context.parse(source, "")?;
    let mut index = Index::new(context.governor);
    compile::walk(
        root.root(),
        0,
        &[],
        "",
        "urn:purrassist:root",
        &mut index,
        &mut context,
    )?;
    index.by_route = index::Map::from_sorted(
        context
            .initial_routes
            .take()
            .expect("initial traversal retains its route entries until assembly"),
        context.governor,
    )?;
    let root_digest = hash_schema_bytes(source.as_bytes(), context.governor)?;
    let mut documents = index::Arena::new(context.governor);
    documents.push(root, context.governor)?;
    let mut schema = Schema {
        documents,
        index,
        root: 0,
        // Private root identity until linkage determines the complete bundle's
        // dialect prefix. No partially prepared Schema leaves this function.
        digest: root_digest,
    };
    link_references(&mut schema, &mut context)?;
    schema.digest = bundle_digest(
        root_digest,
        resources,
        schema.dialect() == Dialect::Draft07
            || schema
                .index
                .resources
                .get_governed(DRAFT_07, context.governor)?
                .is_some(),
        context.governor,
    )?;
    context.governor.check()?;
    Ok(schema)
}

/// Link each discovered schema location once, including reference-only targets.
fn link_references(schema: &mut Schema, context: &mut Context<'_>) -> Result<(), CompilationError> {
    let mut cursor = 0;
    while cursor < schema.index.locations.len() {
        context.charge(Resource::Work, 1)?;
        for keyword in ["$ref", "$dynamicRef"] {
            reference::work(context.governor, schema.index.locations[cursor].route.len())?;
            let Some(reference) = compile::member(schema.body(cursor), keyword, context.governor)?
                .and_then(Node::as_str)
            else {
                continue;
            };
            let base = &schema.index.locations[cursor].base;
            let uri = uri::resolve(base, reference, context.governor)?;
            let resource =
                &uri[..super::uri::find(&uri, b'#', context.governor)?.unwrap_or(uri.len())];
            if reference::lookup(&schema.index.resources, resource, context.governor)?.is_none() {
                load_document(schema, context, cursor, &uri)?;
            }
            let target = reference::target(schema, &uri, context.governor)
                .map_err(|error| error.compilation(context.governor))?;
            if let reference::Target::Pointer(target) = target {
                if target.compiled(schema, context.governor)?.is_none() {
                    let base = reference::copy(
                        &schema.index.locations[target.base_id].base,
                        context.governor,
                    )?;
                    reference::work(context.governor, target.key.1.len())?;
                    let node = at_route(schema.documents[target.key.0].root(), &target.key.1);
                    if !matches!(
                        node.value,
                        crate::json::Value::Bool(_) | crate::json::Value::Object(_)
                    ) {
                        return Err(compile::identity_error(
                            &base,
                            &target.pointer,
                            "schema must be an object or boolean",
                            context.governor,
                        ));
                    }
                    compile::walk(
                        node,
                        target.key.0,
                        &target.key.1,
                        &target.pointer,
                        &base,
                        &mut schema.index,
                        context,
                    )?;
                }
                if target.compiled(schema, context.governor)?.is_none() {
                    return Err(reference::invalid(
                        &uri,
                        "reference does not identify a compiled schema",
                        context.governor,
                    )
                    .compilation(context.governor));
                }
            }
        }
        cursor += 1;
    }
    Ok(())
}
