// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Governed schema bundle compilation and typed failure propagation.
use super::{Index, Schema, SchemaError, at_route, builtin, bundle_digest, compile};
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
pub(super) fn compile(
    source: &str,
    resources: &BTreeMap<String, String>,
    governor: &mut Governor,
) -> Result<Schema, CompilationError> {
    let mut context = Context {
        resources,
        governor,
    };
    // Admission precedes bundle hashing and its temporary hash framing buffer.
    context.charge(Resource::Work, source.len())?;
    context.charge(
        Resource::ScratchBytes,
        resources.len().saturating_mul(64).saturating_add(256),
    )?;
    for (uri, text) in resources {
        context.charge(Resource::InputBytes, text.len())?;
        context.charge(Resource::Work, uri.len().saturating_add(text.len()))?;
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
    let mut schema = Schema {
        documents: vec![root],
        index,
        root: 0,
        digest: bundle_digest(source, resources),
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
                let text = builtin::resource(resource)
                    .or_else(|| resources.get(resource).map(String::as_str))
                    .ok_or_else(|| SchemaError {
                        location: uri.clone(),
                        message: "unresolved resource; supply it explicitly".into(),
                    })?;
                let node = context.parse(text, resource)?;
                let document = schema.documents.len();
                let id = compile::walk(
                    &node,
                    document,
                    &[],
                    "",
                    resource,
                    &mut schema.index,
                    &mut context,
                )?;
                schema.index.resources.insert(resource.into(), id);
                schema.documents.push(node);
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
    Ok(schema)
}
