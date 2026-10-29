// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

use super::{Member, Node, Number, NumberParseError, OwnedDocument, Value};
use crate::contract::{
    Resource, ScratchCharge, ScratchLease, push_scratch as push, saturating_size,
};
use crate::{Governor, ScratchString, ScratchVec, StopReason};

impl OwnedDocument {
    pub(crate) const fn root(&self) -> &Node {
        &self.root
    }

    /// Change only the source span; tree storage stays under this owner.
    pub(crate) const fn set_span(&mut self, span: Option<core::ops::Range<usize>>) {
        self.root.span = span;
    }

    /// Decoded JSON has no byte coordinates in the surrounding source string.
    pub(crate) fn clear_spans(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        clear_spans(&mut self.root, governor)?;
        governor.check()
    }

    /// Admit the whole tree at its prospective nesting depth before attachment.
    pub(crate) fn admit_depth(
        &self,
        depth: u32,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        admit_depth(&self.root, depth, governor)
    }

    /// Copy retained ancestors and siblings, transferring the replacement at route.
    /// The removed subtree is never copied or charged to the new owner.
    pub(crate) fn replace_from(
        node: &Node,
        route: &[usize],
        replacement: Self,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut lease = ScratchLease::empty(governor);
        let mut replacement = Some(replacement);
        let root = copy_replacing(node, route, &mut replacement, governor, &mut lease, 0)?;
        assert!(
            replacement.is_none(),
            "INVARIANT: replacement route consumed"
        );
        governor.check()?;
        Ok(Self { root, lease })
    }

    /// Ordinary public JSON nodes retain their published storage admission.
    pub(crate) fn into_node(self) -> Node {
        let Self { root, lease } = self;
        lease.retain();
        root
    }

    /// Transfer a completed subtree into an owned parent without publication.
    pub(crate) fn into_owned_parts(self) -> (Node, ScratchLease) {
        (self.root, self.lease)
    }

    pub(crate) fn null(governor: &Governor) -> Self {
        Self {
            root: Node::new(Value::Null),
            lease: ScratchLease::empty(governor),
        }
    }

    pub(crate) fn boolean(value: bool, governor: &Governor) -> Self {
        Self {
            root: Node::new(Value::Bool(value)),
            lease: ScratchLease::empty(governor),
        }
    }

    pub(crate) fn string(text: ScratchString) -> Self {
        let (text, lease) = text.into_owned_parts();
        Self {
            root: Node::new(Value::String(text)),
            lease,
        }
    }

    pub(crate) fn number(
        text: ScratchString,
        governor: &mut Governor,
    ) -> Result<Self, NumberParseError> {
        let (number, lease) = Number::adopt_owned(text, governor)?;
        Ok(Self {
            root: Node::new(Value::Number(number)),
            lease,
        })
    }

    pub(crate) fn array(items: ScratchVec<Node>, mut lease: ScratchLease) -> Self {
        let (items, buffer) = items.into_owned_parts();
        lease.absorb(buffer);
        Self {
            root: Node::new(Value::Array(items)),
            lease,
        }
    }

    pub(crate) fn object(members: ScratchVec<Member>, mut lease: ScratchLease) -> Self {
        let (members, buffer) = members.into_owned_parts();
        lease.absorb(buffer);
        Self {
            root: Node::new(Value::Object(members)),
            lease,
        }
    }

    /// Copy an in-progress object candidate without changing its children.
    pub(crate) fn copy_object(
        members: &[Member],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut lease = ScratchLease::empty(governor);
        governor.charge(Resource::Depth, 0)?;
        governor.charge(Resource::Work, 1)?;
        let members = copy_members(members, governor, &mut lease, 0)?;
        governor.check()?;
        Ok(Self {
            root: Node::new(Value::Object(members)),
            lease,
        })
    }

    /// Copy the existing tree representation, including lexical numbers and all
    /// source spans. Temporary containers unwind before their common lease.
    pub(crate) fn copy_from(node: &Node, governor: &mut Governor) -> Result<Self, StopReason> {
        let mut lease = ScratchLease::empty(governor);
        let root = copy_node(node, governor, &mut lease, 0)?;
        governor.check()?;
        Ok(Self { root, lease })
    }
}

fn admit_depth(node: &Node, depth: u32, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(Resource::Work, 1)?;
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth));
    }
    match &node.value {
        Value::Array(items) => {
            for item in items {
                admit_depth(item, depth + 1, governor)?;
            }
        }
        Value::Object(members) => {
            for member in members {
                admit_depth(&member.value, depth + 1, governor)?;
            }
        }
        _ => {}
    }
    governor.check()
}

fn clear_spans(node: &mut Node, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(Resource::Work, 1)?;
    node.span = None;
    match &mut node.value {
        Value::Array(items) => {
            for item in items {
                clear_spans(item, governor)?;
            }
        }
        Value::Object(members) => {
            for member in members {
                member.key_span = None;
                clear_spans(&mut member.value, governor)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn copy_replacing(
    node: &Node,
    route: &[usize],
    replacement: &mut Option<OwnedDocument>,
    governor: &mut Governor,
    lease: &mut ScratchLease,
    depth: u32,
) -> Result<Node, StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(Resource::Work, 1)?;
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth));
    }
    let Some((&selected, tail)) = route.split_first() else {
        replacement
            .as_ref()
            .expect("INVARIANT: replacement exists")
            .admit_depth(depth, governor)?;
        let (root, owner) = replacement
            .take()
            .expect("INVARIANT: one replacement")
            .into_owned_parts();
        lease.absorb(owner);
        return Ok(root);
    };
    let value = match &node.value {
        Value::Array(items) => {
            assert!(selected < items.len(), "INVARIANT: replacement array route");
            let mut copied = ScratchVec::new(governor);
            for (index, item) in items.iter().enumerate() {
                let child = if index == selected {
                    copy_replacing(item, tail, replacement, governor, lease, depth + 1)?
                } else {
                    copy_node(item, governor, lease, depth + 1)?
                };
                push(&mut copied, child, governor)?;
            }
            let (items, owner) = copied.into_owned_parts();
            lease.absorb(owner);
            Value::Array(items)
        }
        Value::Object(members) => {
            assert!(
                selected < members.len(),
                "INVARIANT: replacement object route"
            );
            let mut copied = ScratchVec::new(governor);
            for (index, member) in members.iter().enumerate() {
                let (key, owner) = copy_text(&member.key, governor)?.into_owned_parts();
                lease.absorb(owner);
                let value = if index == selected {
                    copy_replacing(&member.value, tail, replacement, governor, lease, depth + 1)?
                } else {
                    copy_node(&member.value, governor, lease, depth + 1)?
                };
                push(
                    &mut copied,
                    Member {
                        key,
                        key_span: member.key_span.clone(),
                        value,
                    },
                    governor,
                )?;
            }
            let (members, owner) = copied.into_owned_parts();
            lease.absorb(owner);
            Value::Object(members)
        }
        _ => unreachable!("INVARIANT: replacement route traverses containers"),
    };
    Ok(Node {
        value,
        span: node.span.clone(),
    })
}

fn copy_text(text: &str, governor: &mut Governor) -> Result<ScratchString, StopReason> {
    governor.charge(Resource::Work, saturating_size(text.len()))?;
    let mut copied = ScratchString::new(governor);
    copied.push_str(text)?;
    Ok(copied)
}

fn copy_node(
    node: &Node,
    governor: &mut Governor,
    lease: &mut ScratchLease,
    depth: u32,
) -> Result<Node, StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(Resource::Work, 1)?;
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth));
    }
    let value = match &node.value {
        Value::Null => Value::Null,
        Value::Bool(value) => Value::Bool(*value),
        Value::String(text) => {
            let (text, storage) = copy_text(text, governor)?.into_owned_parts();
            lease.absorb(storage);
            Value::String(text)
        }
        Value::Number(number) => Value::Number(
            Number::retain_owned(number.as_str(), governor, lease).map_err(
                |error| match error {
                    NumberParseError::Resource(reason) => reason,
                    NumberParseError::Syntax(_) => {
                        unreachable!("INVARIANT: Number contains validated spelling")
                    }
                },
            )?,
        ),
        Value::Array(items) => {
            let mut copied = ScratchVec::new(governor);
            for item in items {
                let item = copy_node(item, governor, lease, depth + 1)?;
                push(&mut copied, item, governor)?;
            }
            let (copied, storage) = copied.into_owned_parts();
            lease.absorb(storage);
            Value::Array(copied)
        }
        Value::Object(members) => Value::Object(copy_members(members, governor, lease, depth)?),
    };
    Ok(Node {
        value,
        span: node.span.clone(),
    })
}

fn copy_members(
    members: &[Member],
    governor: &mut Governor,
    lease: &mut ScratchLease,
    depth: u32,
) -> Result<Vec<Member>, StopReason> {
    let mut copied = ScratchVec::new(governor);
    for member in members {
        let (key, storage) = copy_text(&member.key, governor)?.into_owned_parts();
        lease.absorb(storage);
        let value = copy_node(&member.value, governor, lease, depth + 1)?;
        push(
            &mut copied,
            Member {
                key,
                key_span: member.key_span.clone(),
                value,
            },
            governor,
        )?;
    }
    let (copied, storage) = copied.into_owned_parts();
    lease.absorb(storage);
    Ok(copied)
}

impl ScratchCharge for OwnedDocument {
    const CHARGE: usize = {
        let charge = Node::CHARGE + 16;
        assert!(size_of::<Self>() <= charge);
        charge
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Budget;

    #[test]
    fn replacement_skips_removed_storage_and_admits_combined_depth() {
        let source = super::super::parse(&format!(
            r#"{{"remove":"{}","keep":1.00e+2}}"#,
            "x".repeat(8192)
        ))
        .unwrap();
        let mut governor = Governor::new(Budget {
            scratch_bytes: 1024,
            ..Budget::default()
        });
        let replacement = OwnedDocument::null(&governor);
        let copied =
            OwnedDocument::replace_from(&source, &[0], replacement, &mut governor).unwrap();
        assert_eq!(copied.root().to_json(), r#"{"remove":null,"keep":1.00e+2}"#);
        assert_eq!(copied.root().span, source.span);
        assert_eq!(copied.root().get("keep"), source.get("keep"));
        drop(copied);
        assert_eq!(governor.scratch_meter().live(), 0);

        let parent = super::super::parse("[null]").unwrap();
        let replacement = super::super::parse("[0]").unwrap();
        let mut governor = Governor::new(Budget {
            depth: 1,
            ..Budget::default()
        });
        let replacement = OwnedDocument::copy_from(&replacement, &mut governor).unwrap();
        assert!(matches!(
            OwnedDocument::replace_from(&parent, &[0], replacement, &mut governor),
            Err(StopReason::BudgetExceeded(Resource::Depth))
        ));
        assert_eq!(governor.scratch_meter().live(), 0);
    }

    #[test]
    fn copied_literals_preserve_provenance_and_release_partial_trees() {
        let source = super::super::parse(r#" {"n":1.00e+2,"n":[true,"雪"],"tail":null} "#).unwrap();
        let mut governor = Governor::new(Budget::default());
        let copied = OwnedDocument::copy_from(&source, &mut governor).unwrap();
        assert_eq!(copied.root(), &source, "all node and member spans survive");
        assert_eq!(
            copied.root().to_json(),
            source.to_json(),
            "number spelling and duplicate member order survive"
        );
        assert!(governor.scratch_meter().live() > 0);
        let usage = governor.usage();
        drop(copied);
        assert_eq!(governor.scratch_meter().live(), 0);
        for (budget, resource) in [
            (
                Budget {
                    work: usage.work - 1,
                    ..Budget::default()
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..Budget::default()
                },
                Resource::ScratchBytes,
            ),
        ] {
            let mut governor = Governor::new(budget);
            assert!(
                matches!(OwnedDocument::copy_from(&source, &mut governor), Err(StopReason::BudgetExceeded(actual)) if actual == resource)
            );
            assert!(governor.usage().total_scratch_charged > 0);
            assert_eq!(governor.scratch_meter().live(), 0);
        }
    }
}
