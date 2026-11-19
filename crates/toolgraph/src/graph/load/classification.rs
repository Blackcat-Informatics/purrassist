// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed role and ownership classification over the retained RDF term IDs.

use purrassist_core::contract::saturating_size;
use purrdf_core::{TermId, TermValue, dataset_view::DatasetView};
use purrdf_lex::{
    allocation::{Admission, Memory, StorageError},
    control::{ControlledError, Progress, compare_bytes},
    walk::try_sort_unstable_by,
};

use super::{
    Construct, ControlledStorage, DCTERMS_IS_PART_OF, Doc, FNO, FNO_EXPECTS, FNO_RETURNS, FNOM,
    LoadError, Locations, Node, RDF_FIRST, RDF_REST, RDF_TYPE, Role, Row, SH_NODE_SHAPE,
    SH_PROPERTY, SH_PROPERTY_SHAPE, Shared, Work, control, iri_of, stop, work,
};

const FNO_SUBJECT: u8 = 1;
const FNO_CELL: u8 = 1 << 1;
const SHAPE: u8 = 1 << 2;
const SHAPE_CELL: u8 = 1 << 3;
const INITIALIZATION_CHUNK: usize = 256;

#[derive(Clone, Copy, Default)]
struct NodeState<'doc> {
    rows: &'doc [usize],
    term: Option<&'doc TermValue>,
    declared: Option<u16>,
    inferred: Option<u16>,
    rule_domain: u16,
    membership: u8,
}

impl<'doc> NodeState<'doc> {
    fn declare(&mut self, term: &'doc TermValue, role: u16) {
        self.term = Some(term);
        *self.declared.get_or_insert(0) |= role;
    }

    fn infer(&mut self, term: &'doc TermValue, role: u16) {
        self.term = Some(term);
        *self.inferred.get_or_insert(0) |= role;
        self.rule_domain |= Role::specific_rule_domain(role);
    }

    fn role(&self) -> Option<u16> {
        // Presence matters even for an abstract role with an empty mask.
        self.declared.or(self.inferred)
    }
}

#[derive(Clone, Copy)]
pub(super) struct RoleNode<'doc> {
    pub(super) identity: Node<'doc>,
    pub(super) mask: u16,
    pub(super) rule_domain: u16,
    pub(super) rows: &'doc [usize],
}

/// Classifications stay in the same admitted account through every consumer.
pub(super) struct Nodes<'doc> {
    by_id: Vec<NodeState<'doc>>,
    roles: Vec<RoleNode<'doc>>,
    pub(super) has_fno: bool,
}

impl<'doc> Nodes<'doc> {
    fn entry(&mut self, id: TermId) -> Result<&mut NodeState<'doc>, LoadError> {
        self.by_id.get_mut(id.index()).ok_or_else(|| {
            control::read_failure("dataset-read", "term ID is outside the retained dictionary")
        })
    }

    pub(super) fn roles(&self) -> impl Iterator<Item = RoleNode<'doc>> + '_ {
        self.roles.iter().copied()
    }

    pub(super) fn rows(&self, id: TermId) -> &'doc [usize] {
        self.by_id[id.index()].rows
    }

    pub(super) fn role_of(&self, id: TermId) -> Option<u16> {
        self.by_id[id.index()].role()
    }

    pub(super) fn owns_fno(&self, id: TermId) -> bool {
        self.has_membership(id, FNO_SUBJECT | FNO_CELL)
    }

    pub(super) fn owns_shape(&self, id: TermId) -> bool {
        self.has_membership(id, SHAPE)
    }

    pub(super) fn owns_shape_cell(&self, id: TermId) -> bool {
        self.has_membership(id, SHAPE_CELL)
    }

    fn has_membership(&self, id: TermId, membership: u8) -> bool {
        // Classification checks every original subject, object and reifier ID.
        self.by_id[id.index()].membership & membership != 0
    }
}

impl Doc<'_> {
    pub(super) fn with_classification<V: DatasetView<Id = TermId>, T>(
        &self,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        read: impl FnOnce(&Nodes<'_>) -> Result<T, LoadError>,
    ) -> Result<T, LoadError> {
        let (mut account, rejection) = ControlledStorage::owned(governor);
        let mut memory = Memory::new(&mut account);
        let storage_error = |cause| control::storage_stop(governor, &rejection, &cause);
        let term_count = locations.view.term_count();
        let count =
            usize::try_from(term_count).map_err(|_| storage_error(StorageError::SizeOverflow))?;
        let mut nodes = Nodes {
            by_id: Vec::new(),
            roles: Vec::new(),
            has_fno: false,
        };
        work(&mut governor.borrow_mut(), 0)?;
        memory
            .reserve(&mut nodes.by_id, count)
            .map_err(storage_error)?;
        for start in (0..count).step_by(INITIALIZATION_CHUNK) {
            let end = start.saturating_add(INITIALIZATION_CHUNK).min(count);
            work(&mut governor.borrow_mut(), saturating_size(end - start))?;
            nodes.by_id.resize(end, NodeState::default());
        }

        let mut progress = Work::new(governor);
        for row in &self.rows {
            work(&mut governor.borrow_mut(), 1)?;
            self.classify_row(row, &mut nodes, locations, governor, &mut progress)?;
        }
        for reifier in &self.reifiers {
            work(&mut governor.borrow_mut(), 1)?;
            nodes
                .entry(reifier.term_id)?
                .declare(&reifier.id, Role::HINT);
        }
        self.classify_lists(&mut nodes, governor)?;
        collect_roles(&mut nodes, governor, &mut memory, storage_error)?;
        read(&nodes)
    }

    fn classify_row<'doc, V: DatasetView<Id = TermId>>(
        &'doc self,
        row: &'doc Row,
        nodes: &mut Nodes<'doc>,
        locations: &Locations<'_, V>,
        governor: &Shared<'_>,
        progress: &mut Work<'_, '_>,
    ) -> Result<(), LoadError> {
        let class = if row.p == RDF_TYPE {
            iri_of(&row.o)
        } else {
            None
        };
        let mut bound = |iri: &str| {
            self.find_bound(|key| compare_bytes(key.as_bytes(), iri.as_bytes(), progress))
                .map_err(stop)
        };
        let bound_class = class.map(&mut bound).transpose()?.flatten();
        let name = bound(&row.p)?;
        if name.or(bound_class).is_some_and(Role::is_runtime) {
            return Err(locations.unsupported_row(
                row,
                Construct::RuntimeFact,
                &mut governor.borrow_mut(),
            )?);
        }

        let is_fno = row.p == DCTERMS_IS_PART_OF
            || class.is_some_and(|iri| iri.starts_with(FNO) || iri.starts_with(FNOM))
            || bound_class == Some("Tool");
        let is_property = row.p == SH_PROPERTY;
        nodes.has_fno |= is_fno;
        let subject = nodes.entry(row.subject_id)?;
        if is_fno {
            subject.membership |= FNO_SUBJECT;
        }
        if is_property || matches!(class, Some(SH_NODE_SHAPE | SH_PROPERTY_SHAPE)) {
            subject.membership |= SHAPE;
        }
        if let Some(name) = name {
            subject.infer(&row.s, Role::domain(name));
        } else if let Some(class) = bound_class {
            subject.declare(&row.s, Role::of_class(class));
        }

        let object = nodes.entry(row.object_id)?;
        let linked = name.map_or(0, Role::link);
        if linked != 0 {
            object.declare(&row.o, linked);
        }
        if is_property || linked & Role::SHAPE != 0 {
            object.membership |= SHAPE;
        }
        Ok(())
    }

    fn classify_lists<'doc>(
        &'doc self,
        nodes: &mut Nodes<'doc>,
        governor: &Shared<'_>,
    ) -> Result<(), LoadError> {
        self.register_groups(nodes, governor)?;
        let mut offset = 0;
        while let Some((id, group)) = self.next_group(nodes, &mut offset) {
            work(&mut governor.borrow_mut(), 1)?;
            if nodes.owns_shape(id) {
                for &index in group {
                    let row = &self.rows[index];
                    work(&mut governor.borrow_mut(), 1)?;
                    self.claim_list(row, nodes, SHAPE_CELL, governor)?;
                }
            }
        }
        offset = 0;
        while let Some((id, group)) = self.next_group(nodes, &mut offset) {
            work(&mut governor.borrow_mut(), 1)?;
            if nodes.has_membership(id, FNO_SUBJECT) {
                for predicate in [FNO_EXPECTS, FNO_RETURNS] {
                    for &index in group {
                        let row = &self.rows[index];
                        work(&mut governor.borrow_mut(), 1)?;
                        if row.p == predicate {
                            self.claim_list(row, nodes, FNO_CELL, governor)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn register_groups<'doc>(
        &'doc self,
        nodes: &mut Nodes<'doc>,
        governor: &Shared<'_>,
    ) -> Result<(), LoadError> {
        let mut start = 0;
        let mut previous = None;
        for (offset, &index) in self.by_subject.iter().enumerate() {
            work(&mut governor.borrow_mut(), 1)?;
            let id = self.rows[index].subject_id;
            if previous == Some(id) {
                continue;
            }
            if let Some(previous) = previous {
                nodes.entry(previous)?.rows = &self.by_subject[start..offset];
            }
            previous = Some(id);
            start = offset;
        }
        if let Some(previous) = previous {
            nodes.entry(previous)?.rows = &self.by_subject[start..];
        }
        Ok(())
    }

    fn next_group<'doc>(
        &self,
        nodes: &Nodes<'doc>,
        offset: &mut usize,
    ) -> Option<(TermId, &'doc [usize])> {
        let id = self.rows[*self.by_subject.get(*offset)?].subject_id;
        let group = nodes.by_id[id.index()].rows;
        *offset += group.len();
        Some((id, group))
    }

    /// Mark the list cells reached through this object's native identity.
    fn claim_list<'doc>(
        &'doc self,
        mut head: &'doc Row,
        nodes: &mut Nodes<'_>,
        membership: u8,
        governor: &Shared<'_>,
    ) -> Result<(), LoadError> {
        while matches!(head.o, TermValue::Blank { .. }) {
            work(&mut governor.borrow_mut(), 1)?;
            let cell = &mut nodes.by_id[head.object_id.index()];
            if cell.membership & membership != 0 {
                break;
            }
            let mut has_first = false;
            let mut next = None;
            for &index in cell.rows {
                work(&mut governor.borrow_mut(), 1)?;
                let row = &self.rows[index];
                if row.p == RDF_FIRST {
                    has_first = true;
                } else if row.p == RDF_REST {
                    next.get_or_insert(row);
                }
                if has_first && next.is_some() {
                    break;
                }
            }
            if !has_first {
                break;
            }
            cell.membership |= membership;
            let Some(rest) = next else {
                break;
            };
            head = rest;
        }
        Ok(())
    }
}

fn collect_roles<S: Admission>(
    nodes: &mut Nodes<'_>,
    governor: &Shared<'_>,
    memory: &mut Memory<'_, S>,
    storage_error: impl Fn(StorageError) -> LoadError,
) -> Result<(), LoadError> {
    for (index, node) in nodes.by_id.iter().enumerate() {
        work(&mut governor.borrow_mut(), 1)?;
        if let (Some(term), Some(role)) = (node.term, node.role()) {
            // INVARIANT: the node table has the retained dataset's checked term count.
            let index = u32::try_from(index).expect("index fits in a retained term table");
            let id = TermId::from_index(index).expect("index addresses a retained term");
            memory
                .push(
                    &mut nodes.roles,
                    RoleNode {
                        identity: Node { id, term },
                        mask: role,
                        rule_domain: node.rule_domain,
                        rows: node.rows,
                    },
                )
                .map_err(&storage_error)?;
        }
    }
    // Native term IDs follow acquisition order. Diagnostics use RDF value order.
    let mut progress = Work::new(governor);
    try_sort_unstable_by(&mut nodes.roles, |left, right| {
        left.identity
            .term
            .try_cmp_with_control(right.identity.term, memory, &mut |units| {
                progress.charge(units).map_err(stop)
            })
            .map_err(|failure| match failure {
                ControlledError::Native(cause) => storage_error(cause),
                ControlledError::Control(error) => error,
            })
    })
}
