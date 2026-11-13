// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed source identity for ordered JSON trees with exact lexical numbers.
// `unreachable_pub` rejects `pub` helpers reexported only within the crate,
// while `redundant_pub_crate` rejects `pub(crate)` inside this private module
// (https://github.com/rust-lang/rust-clippy/issues/6188). Retain the compiler's
// crate-only visibility contract, as in `grammar::engine`.
#![allow(clippy::redundant_pub_crate)]

use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason, push_scratch};

/// Compare exact spellings after admitting both byte lengths; resource refusal
/// propagates without allocating or publishing a partial comparison.
pub(crate) fn same_source_text(
    left: &str,
    right: &str,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        size(left.len())
            .saturating_add(size(right.len()))
            .saturating_add(1),
    )?;
    Ok(left == right)
}

struct SourceNode<'a> {
    left: &'a crate::json::Node,
    right: &'a crate::json::Node,
    depth: u64,
}

impl ScratchCharge for SourceNode<'_> {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<SourceNode<'_>>() <= SourceNode::CHARGE);

/// Source identity is stricter than semantic JSON equality: preserve every
/// occurrence, source range and exact number spelling without numeric work.
pub(crate) fn same_source_tree(
    left: &crate::json::Node,
    right: &crate::json::Node,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    use crate::json::Value;
    let mut stack = ScratchVec::new(governor);
    push_scratch(
        &mut stack,
        SourceNode {
            left,
            right,
            depth: 0,
        },
        governor,
    )?;
    while let Some(SourceNode { left, right, depth }) = stack.pop() {
        governor.charge(Resource::Depth, depth)?;
        if left.span != right.span {
            return Ok(false);
        }
        match (&left.value, &right.value) {
            (Value::Null, Value::Null) => {}
            (Value::Bool(a), Value::Bool(b)) if a == b => {}
            (Value::String(a), Value::String(b)) => {
                if !same_source_text(a, b, governor)? {
                    return Ok(false);
                }
            }
            (Value::Number(a), Value::Number(b)) => {
                if !same_source_text(a.as_str(), b.as_str(), governor)? {
                    return Ok(false);
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (left, right) in a.iter().zip(b).rev() {
                    governor.charge(Resource::Depth, depth.saturating_add(1))?;
                    push_scratch(
                        &mut stack,
                        SourceNode {
                            left,
                            right,
                            depth: depth.saturating_add(1),
                        },
                        governor,
                    )?;
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (left, right) in a.iter().zip(b).rev() {
                    if left.key_span != right.key_span
                        || !same_source_text(&left.key, &right.key, governor)?
                    {
                        return Ok(false);
                    }
                    governor.charge(Resource::Depth, depth.saturating_add(1))?;
                    push_scratch(
                        &mut stack,
                        SourceNode {
                            left: &left.value,
                            right: &right.value,
                            depth: depth.saturating_add(1),
                        },
                        governor,
                    )?;
                }
            }
            _ => return Ok(false),
        }
    }
    governor.check()?;
    Ok(true)
}
