// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One source-borrowing semantic comparison walk for ordinary and governed APIs.
use super::{Node, Value};
use crate::contract::{Operation, Storage};
use crate::contract::{ScratchCharge, ScratchVec, StopReason};

struct Frame<'a> {
    left: &'a Node,
    right: &'a Node,
    depth: u64,
}
impl ScratchCharge for Frame<'_> {
    const CHARGE: usize = 24;
}
const _: () = assert!(core::mem::size_of::<Frame<'_>>() <= Frame::CHARGE);
type Stack<'a> = Storage<Vec<Frame<'a>>, ScratchVec<Frame<'a>>>;
impl<'a> Stack<'a> {
    fn push(&mut self, frame: Frame<'a>) -> Result<(), StopReason> {
        self.append(frame, Vec::push, ScratchVec::push)
    }
    fn pop(&mut self) -> Option<Frame<'a>> {
        match self {
            Self::Plain(stack) => stack.pop(),
            Self::Admitted(stack) => stack.pop(),
        }
    }
}

pub(super) fn compare(
    left: &Node,
    right: &Node,
    operation: &mut Operation<'_>,
) -> Result<bool, StopReason> {
    operation.check()?;
    operation.depth(0)?;
    let mut stack = operation.0.as_ref().map_or_else(
        || Stack::Plain(Vec::new()),
        |governor| Stack::Admitted(ScratchVec::new(governor)),
    );
    stack.push(Frame {
        left,
        right,
        depth: 0,
    })?;
    while let Some(Frame { left, right, depth }) = stack.pop() {
        operation.work(1)?;
        operation.depth(depth)?;
        match (&left.value, &right.value) {
            (Value::Null, Value::Null) => {}
            (Value::Bool(a), Value::Bool(b)) if a == b => {}
            (Value::String(a), Value::String(b)) => {
                if !operation.compare(a, b)?.is_eq() {
                    return Ok(false);
                }
            }
            (Value::Number(a), Value::Number(b)) => {
                if !a.compare_admitted(b, operation)?.is_eq() {
                    return Ok(false);
                }
            }
            (Value::Array(a), Value::Array(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (left, right) in a.iter().zip(b).rev() {
                    operation.work(1)?;
                    operation.depth(depth.saturating_add(1))?;
                    stack.push(Frame {
                        left,
                        right,
                        depth: depth.saturating_add(1),
                    })?;
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                // Match the nth occurrence of a key with the nth occurrence on
                // the other side. Interleaving distinct keys never changes it.
                for (index, member) in a.iter().enumerate().rev() {
                    operation.work(1)?;
                    let mut ordinal = 0_usize;
                    for previous in &a[..index] {
                        if operation.compare(&previous.key, &member.key)?.is_eq() {
                            ordinal += 1;
                        }
                    }
                    let mut found = None;
                    for candidate in b {
                        if operation.compare(&candidate.key, &member.key)?.is_eq() {
                            if ordinal == 0 {
                                found = Some(&candidate.value);
                                break;
                            }
                            ordinal -= 1;
                        }
                    }
                    let Some(right) = found else {
                        return Ok(false);
                    };
                    operation.depth(depth.saturating_add(1))?;
                    stack.push(Frame {
                        left: &member.value,
                        right,
                        depth: depth.saturating_add(1),
                    })?;
                }
            }
            _ => return Ok(false),
        }
    }
    operation.check()?;
    Ok(true)
}
