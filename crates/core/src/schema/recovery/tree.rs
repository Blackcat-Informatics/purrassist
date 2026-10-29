// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Owned candidate assembly; removed values are never copied into a new tree.
use super::super::reference;
use crate::contract::{
    Governor, ScratchLease, ScratchString, ScratchVec, StopReason, push_scratch,
};
use crate::json::{Member, Node, OwnedDocument, Value};

pub(super) enum Edit {
    Rename(usize, ScratchString),
    Remove(usize),
    Relocate(usize, usize),
    Append(ScratchString, OwnedDocument),
}

pub(super) fn wrap(node: &Node, governor: &mut Governor) -> Result<OwnedDocument, StopReason> {
    let child = OwnedDocument::copy_from(node, governor)?;
    child.admit_depth(1, governor)?;
    let (node, lease) = child.into_owned_parts();
    let mut items = ScratchVec::new(governor);
    push_scratch(&mut items, node, governor)?;
    governor.check()?;
    Ok(OwnedDocument::array(items, lease))
}

pub(super) fn object(
    members: &[Member],
    edit: Edit,
    governor: &mut Governor,
) -> Result<OwnedDocument, StopReason> {
    let mut renamed = None;
    let mut removed = None;
    let mut relocated = None;
    let mut appended = None;
    match edit {
        Edit::Rename(index, key) => renamed = Some((index, key)),
        Edit::Remove(index) => removed = Some(index),
        Edit::Relocate(parent, child) => relocated = Some((parent, child)),
        Edit::Append(key, value) => appended = Some((key, value)),
    }
    let mut lease = ScratchLease::empty(governor);
    let mut output = ScratchVec::new(governor);
    for (index, member) in members.iter().enumerate() {
        reference::work(governor, 1)?;
        if removed == Some(index) {
            continue;
        }
        let (key, key_span) = if renamed
            .as_ref()
            .is_some_and(|(position, _)| *position == index)
        {
            (renamed.take().expect("INVARIANT: selected rename").1, None)
        } else {
            (
                reference::copy(&member.key, governor)?,
                member.key_span.clone(),
            )
        };
        let value = if let Some((parent, child)) = relocated
            && parent == index
        {
            let Value::Object(nested) = &member.value.value else {
                unreachable!("INVARIANT: relocation source object")
            };
            let mut value = object(nested, Edit::Remove(child), governor)?;
            value.set_span(member.value.span.clone());
            value
        } else {
            OwnedDocument::copy_from(&member.value, governor)?
        };
        push_member(&mut output, &mut lease, key, key_span, value, governor)?;
    }
    if let Some((parent, child)) = relocated {
        let Value::Object(nested) = &members[parent].value.value else {
            unreachable!("INVARIANT: relocation source object")
        };
        let member = &nested[child];
        let key = reference::copy(&member.key, governor)?;
        let value = OwnedDocument::copy_from(&member.value, governor)?;
        push_member(
            &mut output,
            &mut lease,
            key,
            member.key_span.clone(),
            value,
            governor,
        )?;
    }
    if let Some((key, value)) = appended {
        push_member(&mut output, &mut lease, key, None, value, governor)?;
    }
    assert!(renamed.is_none(), "INVARIANT: renamed member exists");
    governor.check()?;
    Ok(OwnedDocument::object(output, lease))
}

fn push_member(
    members: &mut ScratchVec<Member>,
    lease: &mut ScratchLease,
    key: ScratchString,
    key_span: Option<core::ops::Range<usize>>,
    value: OwnedDocument,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    value.admit_depth(1, governor)?;
    let (key, key_owner) = key.into_owned_parts();
    let (value, value_owner) = value.into_owned_parts();
    push_scratch(
        members,
        Member {
            key,
            key_span,
            value,
        },
        governor,
    )?;
    lease.absorb(key_owner);
    lease.absorb(value_owner);
    Ok(())
}
