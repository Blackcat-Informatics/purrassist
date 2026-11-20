// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Iterative construction of the admitted execution-profile tree.

use std::cmp::Ordering;

use purrassist_core::contract::{Resource, saturating_size};
use purrdf_core::{
    RdfLocation, TermId, TermValue,
    loss::{LossEntry, LossLedger},
};
use purrdf_lex::{
    allocation::{Memory, OrderedTable, OwnedAdmission, StorageError},
    control::{ControlledError, Progress, compare_bytes, with_memory_control},
    walk::try_sort_unstable_by,
};

use super::{
    Doc, ExecutionMode, LoadError, Nodes, ProfileRule, RDF_TYPE, ResourceLimits, Role, Row, Shared,
    Storage, ToolExecutionProfile, Violation, Waiver, Work, boolean, control, iri_of, lexical_str,
    show, stop, work,
};

pub(in crate::graph) struct BuildStorage<'a, 'c, 'g> {
    memory: Memory<'a, OwnedAdmission<Storage>>,
    pub(super) governor: &'c Shared<'g>,
    rejection: control::Rejection,
}

impl<'a, 'c, 'g> BuildStorage<'a, 'c, 'g> {
    pub(super) fn new(
        account: &'a mut OwnedAdmission<Storage>,
        governor: &'c Shared<'g>,
        rejection: control::Rejection,
    ) -> Self {
        Self {
            memory: Memory::new(account),
            governor,
            rejection,
        }
    }

    fn error(&self, cause: &StorageError) -> LoadError {
        control::storage_stop(self.governor, &self.rejection, cause)
    }

    pub(in crate::graph) fn charge(&self, amount: u64) -> Result<(), LoadError> {
        work(&mut self.governor.borrow_mut(), amount)
    }

    pub(in crate::graph) fn with_governor<T>(
        &self,
        run: impl FnOnce(&mut purrassist_core::Governor) -> T,
    ) -> T {
        run(&mut self.governor.borrow_mut())
    }

    pub(super) fn reserve_next<T>(&mut self, values: &mut Vec<T>) -> Result<(), LoadError> {
        if values.len() == values.capacity() {
            self.charge(saturating_size(values.len()))?;
        }
        self.memory
            .reserve_for_push(values)
            .map_err(|cause| self.error(&cause))
    }

    pub(in crate::graph) fn push<T>(
        &mut self,
        values: &mut Vec<T>,
        value: T,
    ) -> Result<(), LoadError> {
        self.reserve_next(values)?;
        values.push(value);
        Ok(())
    }

    pub(super) fn record_loss(
        &mut self,
        ledger: &mut LossLedger,
        mut entry: LossEntry,
        location: RdfLocation,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        with_memory_control(&mut self.memory, progress, |memory| {
            entry.location = Some(memory.boxed(location)?);
            ledger.record_with_memory(entry, memory)
        })
        .map_err(|cause| match cause {
            ControlledError::Control(reason) => stop(reason),
            ControlledError::Native(cause) => self.error(&cause),
        })
    }

    pub(in crate::graph) fn release<T>(&mut self, values: Vec<T>) -> Result<(), LoadError> {
        self.memory
            .release_vec(values)
            .map_err(|cause| self.error(&cause))
    }

    pub(in crate::graph) fn string(&mut self, text: &str) -> Result<String, LoadError> {
        self.charge(saturating_size(text.len()))?;
        self.memory.string(text).map_err(|cause| self.error(&cause))
    }

    pub(super) fn lexical(&mut self, value: &TermValue) -> Result<String, LoadError> {
        match value {
            TermValue::Iri(text)
            | TermValue::Literal {
                lexical_form: text, ..
            } => self.string(text),
            TermValue::Blank { label, .. } => self.blank_label(label),
            TermValue::Triple { .. } => self.string("<<triple>>"),
        }
    }
    pub(super) fn blank_label(&mut self, label: &str) -> Result<String, LoadError> {
        self.charge(saturating_size(label.len()).saturating_add(2))?;
        self.memory
            .format(&format_args!("_:{label}"))
            .map_err(|cause| self.error(&cause))
    }

    pub(in crate::graph) fn fill<T: Copy>(
        &mut self,
        values: &mut Vec<T>,
        count: usize,
        value: T,
    ) -> Result<(), LoadError> {
        self.charge(0)?;
        self.memory
            .reserve(values, count)
            .map_err(|cause| self.error(&cause))?;
        for start in (0..count).step_by(256) {
            let end = start.saturating_add(256).min(count);
            self.charge(saturating_size(end - start))?;
            values.resize(end, value);
        }
        Ok(())
    }

    pub(super) fn record_name<'d>(
        &mut self,
        names: &mut OrderedTable<&'d str, ()>,
        name: &'d str,
        progress: &mut Work<'_, '_>,
    ) -> Result<bool, LoadError> {
        let entry = with_memory_control(&mut self.memory, progress, |memory| {
            names.prepare_insert_with_memory(name, memory, |left, right, memory| {
                compare_bytes(left.as_bytes(), right.as_bytes(), memory)
            })
        })
        .map_err(|cause| match cause {
            ControlledError::Control(reason) => stop(reason),
            ControlledError::Native(cause) => self.error(&cause),
        })?;
        if entry.is_occupied() {
            return Ok(true);
        }
        entry.publish(());
        Ok(false)
    }
}

struct Frame<'d> {
    node: &'d Row,
    children: Vec<(usize, &'d Row)>,
    cursor: usize,
    subprofiles: Vec<ToolExecutionProfile>,
}

pub(super) struct Profiles<'n, 'd> {
    pub(super) nodes: &'n Nodes<'d>,
}

impl<'d> Profiles<'_, 'd> {
    pub(super) fn build(
        &self,
        doc: &'d Doc<'_>,
        root: &'d Row,
        retained: &mut BuildStorage<'_, '_, '_>,
    ) -> Result<ToolExecutionProfile, LoadError> {
        let (temporary, rejection) = Storage::new(retained.governor);
        let mut account = OwnedAdmission::new(temporary);
        let mut temporary = BuildStorage::new(&mut account, retained.governor, rejection);
        let mut progress = Work::new(retained.governor);
        let mut stack = Vec::new();
        self.descend(doc, root, &mut stack, &mut temporary, &mut progress)?;
        loop {
            let frame = stack
                .last_mut()
                .expect("the root frame remains until it is returned");
            if let Some(&(_, next)) = frame.children.get(frame.cursor) {
                frame.cursor += 1;
                self.descend(doc, next, &mut stack, &mut temporary, &mut progress)?;
                continue;
            }
            let frame = stack.pop().expect("a frame was just inspected");
            temporary.release(frame.children)?;
            let profile = self.finish(
                doc,
                frame.node,
                frame.subprofiles,
                retained,
                &mut temporary,
                &mut progress,
            )?;
            let Some(parent) = stack.last_mut() else {
                temporary.release(stack)?;
                return Ok(profile);
            };
            retained.push(&mut parent.subprofiles, profile)?;
        }
    }

    fn descend(
        &self,
        doc: &'d Doc<'_>,
        node: &'d Row,
        stack: &mut Vec<Frame<'d>>,
        temporary: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        temporary.charge(1)?;
        for ancestor in stack.iter() {
            temporary.charge(1)?;
            if ancestor.node.object_id == node.object_id {
                return Err(LoadError::InvalidExecutionProfile {
                    subject: show(&node.o),
                    predicate: doc.term("subprofile").to_owned(),
                    at: None,
                    violation: Violation::Cycle,
                });
            }
        }
        temporary
            .governor
            .borrow_mut()
            .charge(
                Resource::Depth,
                saturating_size(stack.len()).saturating_add(1),
            )
            .map_err(stop)?;
        let mut children = self.edges(doc, node.object_id, "subprofile", temporary, progress)?;
        try_sort_unstable_by(&mut children, |&(_, left), &(_, right)| {
            left.o
                .try_cmp_with_control(&right.o, &mut temporary.memory, &mut |units| {
                    progress.charge(units).map_err(stop)
                })
                .map_err(|error| match error {
                    ControlledError::Control(error) => error,
                    ControlledError::Native(cause) => temporary.error(&cause),
                })
        })?;
        temporary.push(
            stack,
            Frame {
                node,
                children,
                cursor: 0,
                subprofiles: Vec::new(),
            },
        )
    }

    fn edges(
        &self,
        doc: &'d Doc<'_>,
        subject: TermId,
        name: &'static str,
        temporary: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<Vec<(usize, &'d Row)>, LoadError> {
        let predicate = doc.term(name);
        let mut edges = Vec::new();
        for &index in self.nodes.rows(subject) {
            let row = &doc.rows[index];
            if matches_predicate(row, predicate, progress)? {
                temporary.push(&mut edges, (index, row))?;
            }
        }
        Ok(edges)
    }

    fn rows(&self, doc: &'d Doc<'_>, subject: TermId) -> impl Iterator<Item = &'d Row> {
        self.nodes
            .rows(subject)
            .iter()
            .map(|&index| &doc.rows[index])
    }

    fn finish(
        &self,
        doc: &'d Doc<'_>,
        node: &'d Row,
        subprofiles: Vec<ToolExecutionProfile>,
        retained: &mut BuildStorage<'_, '_, '_>,
        temporary: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<ToolExecutionProfile, LoadError> {
        let id = node.object_id;
        let mut rules = Vec::new();
        let mut rule_edges = self.edges(doc, id, "rule", temporary, progress)?;
        sort_display_edges(&mut rule_edges, progress)?;
        for &(_, rule) in &rule_edges {
            let rule = self.rule(doc, rule, retained, progress)?;
            retained.push(&mut rules, rule)?;
        }
        temporary.release(rule_edges)?;
        let mut waivers = Vec::new();
        let mut waiver_edges = self.edges(doc, id, "waiver", temporary, progress)?;
        sort_display_edges(&mut waiver_edges, progress)?;
        for &(_, waiver) in &waiver_edges {
            let subject = waiver.object_id;
            let waived_rule = doc
                .typed_value(self.nodes, subject, "waivedRule", progress)?
                .map(|value| retained.lexical(value))
                .transpose()?
                .unwrap_or_default();
            let value = Waiver {
                iri: retained.lexical(&waiver.o)?,
                waived_rule,
                reviewer: doc.retained_text(self.nodes, subject, "reviewer", retained, progress)?,
                rationale: doc.retained_text(
                    self.nodes,
                    subject,
                    "rationale",
                    retained,
                    progress,
                )?,
                source: doc.retained_text(
                    self.nodes,
                    subject,
                    "waiverSource",
                    retained,
                    progress,
                )?,
            };
            retained.push(&mut waivers, value)?;
        }
        temporary.release(waiver_edges)?;
        let mode = if doc
            .typed_value(self.nodes, id, "confinementMode", progress)?
            .map(lexical_str)
            == Some("aa-exec")
        {
            ExecutionMode::AaExec
        } else {
            ExecutionMode::ChangeHat
        };
        let egress_allowed = doc
            .typed_value(self.nodes, id, "egressAllowed", progress)?
            .is_some_and(boolean);
        Ok(ToolExecutionProfile {
            iri: retained.lexical(&node.o)?,
            mode,
            profile_name: doc.retained_text(self.nodes, id, "profileName", retained, progress)?,
            hat_name: doc.retained_text(self.nodes, id, "hatName", retained, progress)?,
            egress_allowed,
            limits: ResourceLimits {
                address_space_bytes: doc.typed_number(
                    self.nodes,
                    id,
                    "addressSpaceBytes",
                    progress,
                )?,
                file_size_bytes: doc.typed_number(self.nodes, id, "fileSizeBytes", progress)?,
                processes: doc.typed_number(self.nodes, id, "processCount", progress)?,
                cpu_seconds: doc.typed_number(self.nodes, id, "cpuSeconds", progress)?,
                open_files: doc.typed_number(self.nodes, id, "openFileCount", progress)?,
            },
            rules,
            subprofiles,
            waivers,
        })
    }

    fn rule(
        &self,
        doc: &'d Doc<'_>,
        node: &'d Row,
        retained: &mut BuildStorage<'_, '_, '_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<ProfileRule, LoadError> {
        let mut mask = Role::RULE_ANY;
        let mut inferred = 0;
        let mut properties = Vec::new();
        for row in self.rows(doc, node.object_id) {
            progress
                .charge(control::NATIVE_UNITS_PER_WORK)
                .map_err(stop)?;
            if row.p == RDF_TYPE
                && let Some(class) = iri_of(&row.o)
                && let Some(class) = bound(doc, class, progress)?
            {
                mask |= Role::of_class(class);
            }
            if let Some(name) = bound(doc, &row.p, progress)? {
                let domain = Role::domain(name) & Role::RULES;
                if domain != Role::RULES {
                    inferred |= domain;
                }
                let text = retained.lexical(&row.o)?;
                retained.push(&mut properties, (name, text))?;
            }
        }
        let class =
            Role::label_class(doc.resolve_concrete(&node.o, mask, || inferred)? & Role::RULES);
        try_sort_unstable_by(&mut properties, |left, right| {
            let order =
                compare_bytes(left.0.as_bytes(), right.0.as_bytes(), progress).map_err(stop)?;
            if order.is_eq() {
                compare_bytes(left.1.as_bytes(), right.1.as_bytes(), progress).map_err(stop)
            } else {
                Ok(order)
            }
        })?;
        Ok(ProfileRule {
            iri: retained.lexical(&node.o)?,
            class,
            properties,
        })
    }
}

pub(super) fn matches_predicate(
    row: &Row,
    predicate: &str,
    progress: &mut Work<'_, '_>,
) -> Result<bool, LoadError> {
    progress
        .charge(control::NATIVE_UNITS_PER_WORK)
        .map_err(stop)?;
    if row.p.len() != predicate.len() {
        return Ok(false);
    }
    compare_bytes(row.p.as_bytes(), predicate.as_bytes(), progress)
        .map(Ordering::is_eq)
        .map_err(stop)
}

fn bound(
    doc: &Doc<'_>,
    iri: &str,
    progress: &mut Work<'_, '_>,
) -> Result<Option<&'static str>, LoadError> {
    doc.find_bound(|key| compare_bytes(key.as_bytes(), iri.as_bytes(), progress).map_err(stop))
}

fn sort_display_edges(
    edges: &mut [(usize, &Row)],
    progress: &mut Work<'_, '_>,
) -> Result<(), LoadError> {
    try_sort_unstable_by(edges, |&(left_index, left), &(right_index, right)| {
        let order = compare_display(&left.o, &right.o, progress)?;
        // A repeated displayed blank label keeps the original encounter order.
        Ok(order.then(left_index.cmp(&right_index)))
    })
}

fn compare_display(
    left: &TermValue,
    right: &TermValue,
    progress: &mut Work<'_, '_>,
) -> Result<Ordering, LoadError> {
    let compare = |left: &str, right: &str, progress: &mut Work<'_, '_>| {
        compare_bytes(left.as_bytes(), right.as_bytes(), progress).map_err(stop)
    };
    match (left, right) {
        (TermValue::Iri(left), TermValue::Iri(right))
        | (TermValue::Blank { label: left, .. }, TermValue::Blank { label: right, .. }) => {
            compare(left, right, progress)
        }
        (TermValue::Blank { label, .. }, TermValue::Iri(iri)) => {
            compare_blank_to_iri(label, iri, progress)
        }
        (TermValue::Iri(iri), TermValue::Blank { label, .. }) => {
            compare_blank_to_iri(label, iri, progress).map(Ordering::reverse)
        }
        _ => unreachable!("a validated profile edge names an RDF node"),
    }
}

fn compare_blank_to_iri(
    label: &str,
    iri: &str,
    progress: &mut Work<'_, '_>,
) -> Result<Ordering, LoadError> {
    if let Some(rest) = iri.strip_prefix("_:") {
        compare_bytes(label.as_bytes(), rest.as_bytes(), progress).map_err(stop)
    } else {
        compare_bytes(b"_:", iri.as_bytes(), progress).map_err(stop)
    }
}
