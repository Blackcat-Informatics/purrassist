// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Direct lowering of admitted [`AppArmorGlob`] byte semantics into the core engine.

use std::fmt;

use purrassist_core::{
    Governor, ScratchVec, StopReason,
    contract::{Resource, saturating_size as size},
    pattern::{ByteCompileError, ByteInstruction as I, ByteLanguage},
};

use crate::{AppArmorGlob, GlobError, GlobTokenKind, LiteralPathBinding};

pub use purrassist_core::pattern::ByteInclusion as GlobInclusion;

/// One admitted glob and its complete caller-supplied literal bindings.
///
/// Binding validation is shared with [`AppArmorGlob::bind_literal_paths`].
/// The input is borrowed only during compilation; no reference escapes.
#[derive(Clone, Copy, Debug)]
pub struct GlobLanguageInput<'a> {
    /// Parsed semantic source, without reinterpreting its text.
    pub glob: &'a AppArmorGlob,
    /// Exactly one record per referenced variable; no extraneous records.
    pub bindings: &'a [LiteralPathBinding<'a>],
}

/// Binding/lowering or core-program compilation refusal, retaining its cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobLanguageError {
    /// Source binding or toolgraph resource failure.
    Glob(GlobError),
    /// Typed program construction or core resource failure.
    Program(ByteCompileError),
}
impl fmt::Display for GlobLanguageError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glob(reason) => reason.fmt(output),
            Self::Program(reason) => reason.fmt(output),
        }
    }
}
impl std::error::Error for GlobLanguageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Glob(reason) => Some(reason),
            Self::Program(reason) => Some(reason),
        }
    }
}
impl From<GlobError> for GlobLanguageError {
    fn from(reason: GlobError) -> Self {
        Self::Glob(reason)
    }
}
impl From<ByteCompileError> for GlobLanguageError {
    fn from(reason: ByteCompileError) -> Self {
        Self::Program(reason)
    }
}
impl From<StopReason> for GlobLanguageError {
    fn from(reason: StopReason) -> Self {
        Self::Glob(GlobError::Resource(reason))
    }
}

/// Union of admitted [`AppArmorGlob`] languages over arbitrary non-NUL pathname bytes.
///
/// Literal UTF-8 text is lowered into its individual bytes. `?` consumes one
/// non-slash byte, `*` any number of non-slash bytes and `**` any number of bytes;
/// explicit classes may contain slash. Every class excludes NUL. The original
/// parsed semantics and common literal-binding validator are used directly,
/// without rendering or reparsing a policy word or regex.
///
/// This proves language inclusion only. Permission sets, owner qualifiers,
/// allow/deny interactions, hat admission and confinement remain separate checks.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{AppArmorGlob, GlobLanguage, GlobLanguageInput, GlobInclusion};
/// let mut governor = Governor::new(Budget::default());
/// let child = AppArmorGlob::parse("/srv/*", &[], &mut governor).unwrap();
/// let parent = AppArmorGlob::parse("/srv/**", &[], &mut governor).unwrap();
/// let compile = |glob, governor: &mut Governor| GlobLanguage::compile(&[GlobLanguageInput { glob, bindings: &[] }], governor).unwrap();
/// let child = compile(&child, &mut governor);
/// let parent = compile(&parent, &mut governor);
/// assert_eq!(child.included_in(&parent, &mut governor).unwrap(), GlobInclusion::Contained);
/// ```
#[derive(Debug)]
pub struct GlobLanguage {
    program: ByteLanguage,
}

impl GlobLanguage {
    /// Compile the exact union of supplied globs; no inputs means empty language.
    ///
    /// Programs and temporary frame/instruction arrays use fallible admitted
    /// capacity. Brace alternatives are lowered structurally, never expanded.
    /// Source model construction is caller-owned. See the type example.
    ///
    /// # Errors
    /// Returns shared binding validation errors or explicit resource/program refusal.
    pub fn compile(
        inputs: &[GlobLanguageInput<'_>],
        governor: &mut Governor,
    ) -> Result<Self, GlobLanguageError> {
        let mut instructions = ScratchVec::new(governor);
        for input in inputs {
            governor.charge(Resource::Work, 1)?;
            lower(input, &mut instructions, governor)?;
        }
        if inputs.is_empty() {
            instructions.push(I::Dead)?;
        } else if inputs.len() > 1 {
            instructions.push(I::Alternate(inputs.len()))?;
        }
        let program = ByteLanguage::compile(instructions.as_slice(), governor)?;
        governor.check()?;
        Ok(Self { program })
    }

    /// Match the complete pathname byte sequence, including invalid UTF-8.
    ///
    /// # Errors
    /// Returns exact resource/cancellation refusal. NUL is never accepted.
    /// The type example demonstrates compilation through actual glob semantics.
    pub fn accepts(&self, pathname: &[u8], governor: &mut Governor) -> Result<bool, StopReason> {
        self.program.accepts(pathname, governor)
    }

    /// Prove exact inclusion or return a shortest left-only byte witness.
    ///
    /// # Errors
    /// Any exhausted work, depth, scratch or cancellation limit is an error,
    /// never containment or permission. See the type example.
    pub fn included_in(
        &self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<GlobInclusion, StopReason> {
        self.program.included_in(&other.program, governor)
    }
}

fn finish_sequence(count: usize, output: &mut ScratchVec<I>) -> Result<(), StopReason> {
    if count == 0 {
        output.push(I::Empty)?;
    } else if count > 1 {
        output.push(I::Concat(count))?;
    }
    Ok(())
}

fn literal(
    bytes: &[u8],
    output: &mut ScratchVec<I>,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    governor.charge(Resource::Work, size(bytes.len()))?;
    output.extend(bytes.iter().copied().map(I::Literal))?;
    Ok(bytes.len())
}

fn lower(
    input: &GlobLanguageInput<'_>,
    output: &mut ScratchVec<I>,
    governor: &mut Governor,
) -> Result<(), GlobLanguageError> {
    governor.charge(Resource::InputBytes, size(input.glob.source().len()))?;
    let positions = input.glob.prepare_bindings(input.bindings, governor)?;
    let mut frames: ScratchVec<(usize, usize)> = ScratchVec::new(governor);
    frames.push((0, 0))?;
    let tokens = input.glob.tokens();
    let mut at = 0;
    while at < tokens.len() {
        governor.charge(Resource::Work, 1)?;
        let token = &tokens[at];
        let added = match token.kind {
            GlobTokenKind::Literal => literal(
                &input.glob.source().as_bytes()[token.span.clone()],
                output,
                governor,
            )?,
            GlobTokenKind::Byte(byte) => {
                output.push(I::Literal(byte))?;
                1
            }
            GlobTokenKind::Variable { declaration } => literal(
                input.bindings[positions.as_slice()[declaration]]
                    .path
                    .as_bytes(),
                output,
                governor,
            )?,
            GlobTokenKind::AnyByte | GlobTokenKind::Star | GlobTokenKind::RecursiveStar => {
                let mut bits = [u64::MAX; 4];
                bits[0] &= !1;
                if !matches!(token.kind, GlobTokenKind::RecursiveStar) {
                    bits[0] &= !(1 << b'/');
                }
                output.push(I::Class(bits))?;
                if !matches!(token.kind, GlobTokenKind::AnyByte) {
                    output.push(I::Star)?;
                }
                1
            }
            GlobTokenKind::ClassStart { negated, end } => {
                let bits = class_bits(&tokens[at + 1..end], negated, governor)?;
                output.push(I::Class(bits))?;
                at = end;
                1
            }
            GlobTokenKind::AlternationStart { .. } => {
                governor.charge(Resource::Depth, size(frames.len()))?;
                frames.push((0, 0))?;
                at += 1;
                continue;
            }
            GlobTokenKind::Branch { .. } => {
                // The immutable admitted model guarantees the enclosing frame.
                let frame = frames
                    .as_mut_slice()
                    .last_mut()
                    .expect("INVARIANT: branch frame");
                finish_sequence(frame.0, output)?;
                frame.0 = 0;
                frame.1 += 1;
                at += 1;
                continue;
            }
            GlobTokenKind::AlternationEnd { .. } => {
                let frame = frames.pop().expect("INVARIANT: admitted group frame");
                finish_sequence(frame.0, output)?;
                output.push(I::Alternate(frame.1 + 1))?;
                1
            }
            GlobTokenKind::ClassEnd { .. } | GlobTokenKind::ByteRange { .. } => {
                unreachable!("INVARIANT: class atoms handled by their starting token")
            }
        };
        let frame = frames
            .as_mut_slice()
            .last_mut()
            .expect("INVARIANT: root frame outlives traversal");
        frame.0 = frame
            .0
            .checked_add(added)
            .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes))?;
        at += 1;
    }
    finish_sequence(frames.as_slice()[0].0, output)?;
    Ok(())
}

fn class_bits(
    atoms: &[crate::GlobToken],
    negated: bool,
    governor: &mut Governor,
) -> Result<[u64; 4], StopReason> {
    let mut bits = [0; 4];
    for atom in atoms {
        governor.charge(Resource::Work, 1)?;
        let (first, last) = match atom.kind {
            GlobTokenKind::Byte(byte) => (byte, byte),
            GlobTokenKind::ByteRange { first, last } => (first, last),
            _ => unreachable!("INVARIANT: admitted class contains only byte atoms/ranges"),
        };
        for byte in first..=last {
            governor.charge(Resource::Work, 1)?;
            bits[usize::from(byte / 64)] |= 1 << (byte % 64);
        }
    }
    if negated {
        governor.charge(Resource::Work, 4)?;
        for word in &mut bits {
            *word = !*word;
        }
    }
    bits[0] &= !1;
    Ok(bits)
}
