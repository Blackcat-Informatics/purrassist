// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Ledger rows and schema bookkeeping common to the projection lanes.
//!
//! Each lane uses the MCP declaration schemas and identifies the source
//! constructs it omits through the shared rows and keyword scan.

use std::borrow::Cow;

use purrassist_core::json::{self, Node, Value};
use purrdf_core::{
    diagnostic::RdfLocation,
    loss::{LossEntry, LossLedger},
};

use crate::{ProjectionTarget, SOURCE_REPRESENTATION, Tool};

/// Builds one ledger entry: a code, a note and an optional subject location.
pub(super) fn entry_for(
    target: ProjectionTarget,
    code: &'static str,
    note: String,
    subject: Option<(&str, &'static str)>,
) -> LossEntry {
    LossEntry {
        code: Cow::Borrowed(code),
        from: Cow::Borrowed(SOURCE_REPRESENTATION),
        to: Cow::Borrowed(target.representation()),
        note: Cow::Owned(note),
        location: subject.map(|(iri, member)| {
            Box::new(RdfLocation {
                subject: Some(iri.to_owned()),
                logical: Some(member.to_owned()),
                ..RdfLocation::default()
            })
        }),
    }
}

/// Carry each construct the upstream schema compiler already dropped as one
/// entry under `code`, quoting its note and keeping its source location.
pub(super) fn record_upstream(
    ledger: &mut LossLedger,
    upstream: &LossLedger,
    target: ProjectionTarget,
    code: &'static str,
) {
    for entry in upstream.entries() {
        ledger.record(upstream_loss(target, code, entry));
    }
}

/// Translate one native schema loss without losing its original source location.
pub(super) fn upstream_loss(
    target: ProjectionTarget,
    code: &'static str,
    entry: &LossEntry,
) -> LossEntry {
    let note = format!(
        "upstream schema compiler omitted a construct ({}): {}",
        entry.code, entry.note
    );
    LossEntry {
        location: entry.location.clone(),
        ..entry_for(target, code, note, None)
    }
}

/// The pointers of every keyword of `schema` that is neither `carried` nor
/// schema metadata (`$schema`, an empty `$defs`), in schema order.
pub(super) fn uncarried(schema: &Node, pointer: &str, carried: &[&str]) -> Vec<String> {
    let Value::Object(members) = &schema.value else {
        return Vec::new();
    };
    members
        .iter()
        .filter(|member| {
            let key = member.key.as_str();
            let metadata = key == "$schema"
                || (key == "$defs"
                    && matches!(&member.value.value, Value::Object(inner) if inner.is_empty()));
            !metadata && !carried.contains(&key)
        })
        .map(|member| format!("{pointer}/{}", json::pointer_segment(&member.key)))
        .collect()
}

/// The ledger rows every lane records for one tool, in the documented order:
/// authorization, session binding, taint, recovery hints, confinement, resource
/// ceilings, context contract, annotations, RDF binding and description. Hint
/// rows are located at the hint's reifier and the other rows at the tool or
/// profile IRI. Each note names the mandatory check that enforces the omitted
/// constraint.
pub(super) fn record_tool(ledger: &mut LossLedger, tool: &Tool, target: ProjectionTarget) {
    let mut row = |code, note: &str, iri: &str, member| {
        ledger.record(entry_for(
            target,
            code,
            note.to_owned(),
            Some((iri, member)),
        ));
    };
    let hints = || tool.parameters.iter().filter_map(|p| p.hint.as_ref());
    if !tool.parameter_constraints.is_empty() {
        row(
            "authorization-not-expressible",
            "parameterConstraints are not enforced by the projection; mandatory check authorize-call",
            &tool.iri,
            "parameterConstraints",
        );
    }
    for hint in hints().filter(|hint| hint.session_binding.is_some()) {
        row(
            "session-binding-not-expressible",
            "the session binding is not enforced by the projection; mandatory check bind-session",
            &hint.reifier,
            "sessionBinding",
        );
    }
    for hint in hints().filter(|hint| !hint.taints.is_empty()) {
        row(
            "taint-not-expressible",
            "taint labels are not enforced by the projection; mandatory check taint-policy",
            &hint.reifier,
            "taint",
        );
    }
    for hint in hints() {
        row(
            "recovery-hints-not-expressible",
            "recovery tolerances are not carried by the projection; mandatory check recovery-tolerance",
            &hint.reifier,
            "recoveryHint",
        );
    }
    let profile = &tool.execution_profile.iri;
    row(
        "confinement-not-expressible",
        "the execution profile is not enforced by the projection; mandatory check confine-worker",
        profile,
        "executionProfile",
    );
    row(
        "resource-ceiling-not-expressible",
        "the five resource ceilings are not enforced by the projection; mandatory check confine-worker",
        profile,
        "resourceCeilings",
    );
    row(
        "context-contract-not-expressible",
        "the output projection, byte cap and retention are not carried by the projection; mandatory check project-context",
        &tool.iri,
        "contextManagement",
    );
    let annotations = &tool.annotations;
    if annotations.title.is_some()
        || annotations.read_only_hint.is_some()
        || annotations.destructive_hint.is_some()
        || annotations.idempotent_hint.is_some()
        || annotations.open_world_hint.is_some()
    {
        row(
            "tool-annotations-not-expressible",
            "the title and behavior hints are not carried by the projection; mandatory check verify-effects",
            &tool.iri,
            "annotations",
        );
    }
    row(
        "rdf-binding-not-expressible",
        "the FnO mappings and source reifiers are not carried by the projection; mandatory check bind-manifest",
        &tool.iri,
        "fnoBinding",
    );
    if !tool.description.is_empty() {
        row(
            "description-not-expressible",
            "the description is not carried by the projection; mandatory check bind-manifest",
            &tool.iri,
            "description",
        );
    }
}
