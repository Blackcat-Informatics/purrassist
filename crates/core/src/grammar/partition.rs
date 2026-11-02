// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Compact adjacent-capture relations and native-DFA boundary validation.

use super::{CaptureKind, CompiledGrammar, Inst, Program, compile};
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason, saturating_size};
use crate::pattern::{ByteAutomaton, DfaState};

/// A name class whose suppressed separator made an optional ID class adjacent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct JointCapture {
    name: u32,
    id: u32,
}
impl ScratchCharge for JointCapture {
    const CHARGE: usize = 8;
}

/// Keep the maximal name reading and its compact source-boundary relation.
/// No speculative parser configurations are introduced for its partitions.
pub(super) fn relations(
    program: &Program,
    governor: &mut Governor,
) -> Result<ScratchVec<JointCapture>, StopReason> {
    let mut relations = ScratchVec::new(governor);
    for inst in &program.insts {
        governor.charge(Resource::Work, 1)?;
        let Inst::Class { class: name, next } = *inst else {
            continue;
        };
        let Inst::Close {
            kind: CaptureKind::Name,
            next,
        } = program.insts[next as usize]
        else {
            continue;
        };
        let mut seen = ScratchVec::new(governor);
        seen.extend(core::iter::repeat_n(false, program.insts.len()))?;
        let mut pending = ScratchVec::new(governor);
        pending.push(next)?;
        while let Some(pc) = pending.pop() {
            governor.charge(Resource::Work, 1)?;
            if core::mem::replace(&mut seen.as_mut_slice()[pc as usize], true) {
                continue;
            }
            match program.insts[pc as usize] {
                Inst::Open {
                    kind: CaptureKind::Id,
                    next,
                } => {
                    if let Inst::Class { class: id, .. } = program.insts[next as usize] {
                        let relation = JointCapture { name, id };
                        governor.charge(Resource::Work, saturating_size(relations.len()))?;
                        if !relations.contains(&relation) {
                            relations.push(relation)?;
                        }
                    }
                }
                Inst::Split(..) | Inst::Close { .. } | Inst::Commit { .. } => {
                    pending.extend(compile::successors(&program.insts[pc as usize]))?;
                }
                _ => {}
            }
        }
    }
    Ok(relations)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NativeState(DfaState);
impl ScratchCharge for NativeState {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 40);
        40
    };
}

const DEAD: u32 = u32::MAX;

/// A finite table extracted through the original class automaton, over just the
/// bytes present in the joint capture. Other bytes cannot occur in its suffixes.
struct SuffixTable {
    columns: [usize; 256],
    width: usize,
    transitions: ScratchVec<u32>,
    accepting: ScratchVec<bool>,
}
impl SuffixTable {
    fn compile(
        automaton: &ByteAutomaton,
        bytes: &[u8],
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut alphabet = ScratchVec::new(governor);
        let mut columns = [usize::MAX; 256];
        for &byte in bytes {
            governor.charge(Resource::Work, 1)?;
            if columns[usize::from(byte)] == usize::MAX {
                columns[usize::from(byte)] = alphabet.len();
                alphabet.push(byte)?;
            }
        }
        let mut dfa = automaton.dfa(governor)?;
        let mut states = ScratchVec::new(governor);
        states.push(NativeState(dfa.start()))?;
        let mut transitions = ScratchVec::new(governor);
        let mut accepting = ScratchVec::new(governor);
        let mut index = 0;
        while index < states.len() {
            let state = states[index].0;
            accepting.push(dfa.accepting(state, governor)?)?;
            for &byte in &alphabet {
                governor.charge(Resource::Work, 1)?;
                let next = dfa.step(state, byte, governor)?;
                if dfa.is_dead(next, governor)? {
                    transitions.push(DEAD)?;
                    continue;
                }
                governor.charge(Resource::Work, saturating_size(states.len()))?;
                let found = states.iter().position(|state| state.0 == next);
                let target = if let Some(found) = found {
                    found
                } else {
                    let target = states.len();
                    states.push(NativeState(next))?;
                    target
                };
                let target = u32::try_from(target)
                    .map_err(|_| governor.stop_resource(Resource::ScratchBytes))?;
                transitions.push(target)?;
            }
            index += 1;
        }
        Ok(Self {
            columns,
            width: alphabet.len(),
            transitions,
            accepting,
        })
    }

    fn boundaries(
        &self,
        bytes: &[u8],
        prefixes: &[bool],
        governor: &mut Governor,
    ) -> Result<ScratchVec<usize>, StopReason> {
        let mut current = ScratchVec::new(governor);
        current.extend(self.accepting.iter().copied())?;
        let mut next = ScratchVec::new(governor);
        next.extend(core::iter::repeat_n(false, self.accepting.len()))?;
        let mut boundaries = ScratchVec::new(governor);
        for index in (1..bytes.len()).rev() {
            let column = self.columns[usize::from(bytes[index])];
            for state in 0..self.accepting.len() {
                governor.charge(Resource::Work, 1)?;
                let target = self.transitions[state * self.width + column];
                next.as_mut_slice()[state] = target != DEAD && current[target as usize];
            }
            core::mem::swap(&mut current, &mut next);
            if prefixes[index] && current[0] {
                boundaries.push(index)?;
            }
        }
        governor.charge(Resource::Work, saturating_size(boundaries.len()))?;
        boundaries.as_mut_slice().reverse();
        Ok(boundaries)
    }
}

/// Validate all nonempty NAME/ID splits in two source passes plus a fixed-class
/// state DP. Materialization of the admitted alternatives is charged separately.
pub(super) fn boundaries(
    grammar: &CompiledGrammar,
    relation: JointCapture,
    bytes: &[u8],
    governor: &mut Governor,
) -> Result<ScratchVec<usize>, StopReason> {
    let mut prefixes = ScratchVec::new(governor);
    prefixes.push(false)?;
    let mut dfa = grammar.classes[relation.name as usize]
        .automaton
        .dfa(governor)?;
    let mut state = dfa.start();
    for &byte in bytes {
        governor.charge(Resource::Work, 1)?;
        state = dfa.step(state, byte, governor)?;
        prefixes.push(dfa.accepting(state, governor)?)?;
    }
    SuffixTable::compile(
        &grammar.classes[relation.id as usize].automaton,
        bytes,
        governor,
    )?
    .boundaries(bytes, &prefixes, governor)
}
