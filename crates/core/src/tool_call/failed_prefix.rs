// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Private structural transport for a proved failed native frame and its shadow.

use super::{DialectId, Piece, Rejection, ResolvedPiece, ResolvedSegment, ResolvedTurn, Segment};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchVec, StopReason, invalid_input, push_scratch,
    saturating_size as size,
};
use crate::grammar::engine::{FailureWitness, HeaderSpan};
use core::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Certificate {
    witness: FailureWitness,
    frame: HeaderSpan,
    native: bool,
}

impl ScratchCharge for Certificate {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 256);
        256
    };
}

impl Certificate {
    pub(super) fn new(
        witness: FailureWitness,
        span: &Range<usize>,
        dialect: &DialectId,
        frame: Option<HeaderSpan>,
        governor: &mut Governor,
    ) -> Result<Option<Self>, StopReason> {
        governor.charge(Resource::Work, 20)?;
        if witness.end != span.end || witness.header.consumed_to < span.end {
            return Ok(None);
        }
        let native = *dialect == DialectId::MistralV3;
        let Some(frame) = (if native { witness.native } else { frame }) else {
            return Ok(None);
        };
        if frame.start >= frame.end || frame.end > witness.header.first_retained {
            return Ok(None);
        }
        let framing = if native {
            frame.start == span.start && witness.header.dropped.is_none()
        } else if *dialect == DialectId::HermesStripped {
            witness.header.dropped.is_some_and(|dropped| {
                dropped.start == span.start
                    && dropped.start < frame.start
                    && frame.end <= dropped.end
                    && dropped.end <= witness.header.first_retained
            })
        } else {
            false
        };
        Ok(framing.then_some(Self {
            witness,
            frame,
            native,
        }))
    }

    fn matches(&self, span: &Range<usize>, dialect: &DialectId, reason: Rejection) -> bool {
        let start = if self.native {
            self.frame.start
        } else {
            self.witness
                .header
                .dropped
                .map_or(usize::MAX, |dropped| dropped.start)
        };
        span.start == start
            && span.end == self.witness.end
            && reason == self.witness.reason
            && if self.native {
                *dialect == DialectId::MistralV3
            } else {
                *dialect == DialectId::HermesStripped
            }
    }

    fn same_body(&self, other: &Self) -> bool {
        let (left, right) = (self.witness, other.witness);
        left.failed_at == right.failed_at
            && left.reason == right.reason
            && left.end == right.end
            && left.header.name == right.header.name
            && left.header.id == right.header.id
            && left.header.arguments == right.header.arguments
            && left.header.first_retained == right.header.first_retained
            && left.header.consumed_to == right.header.consumed_to
            && self.frame == other.frame
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SegmentProof {
    segment: usize,
    certificate: Certificate,
}

impl ScratchCharge for SegmentProof {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 256);
        256
    };
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ProofRows<T: ScratchCharge> {
    rows: ScratchVec<T>,
}

impl<T: ScratchCharge> ProofRows<T> {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            rows: ScratchVec::new(governor),
        }
    }
}

pub(super) type PathProof = ProofRows<SegmentProof>;

impl ScratchCharge for PathProof {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 64);
        64
    };
}

impl PathProof {
    pub(super) fn get(
        &self,
        segment: usize,
        governor: &mut Governor,
    ) -> Result<Option<Certificate>, StopReason> {
        if self.rows.is_empty() {
            return Ok(None);
        }
        governor.charge(
            Resource::Work,
            u64::from(self.rows.len().bit_width() + 1).saturating_add(24),
        )?;
        Ok(self
            .rows
            .binary_search_by_key(&segment, |row| row.segment)
            .ok()
            .map(|index| self.rows[index].certificate))
    }

    pub(super) fn push(
        &mut self,
        segment: usize,
        certificate: Option<Certificate>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let Some(certificate) = certificate else {
            return Ok(());
        };
        governor.charge(Resource::Work, 24)?;
        if self.rows.last().is_some_and(|last| last.segment >= segment) {
            return Err(invalid_input(
                "failed header path addresses are not increasing",
            ));
        }
        push_scratch(
            &mut self.rows,
            SegmentProof {
                segment,
                certificate,
            },
            governor,
        )
    }

    /// Every collapsed source origin must prove the same certificate.
    pub(super) fn intersect(
        &mut self,
        other: &Self,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(
            Resource::Work,
            size(self.rows.len())
                .saturating_mul(u64::from(other.rows.len().bit_width() + 1).saturating_add(32)),
        )?;
        let mut write = 0;
        for read in 0..self.rows.len() {
            let row = self.rows[read];
            let same = other
                .rows
                .binary_search_by_key(&row.segment, |row| row.segment)
                .is_ok_and(|index| other.rows[index].certificate == row.certificate);
            if same {
                self.rows.as_mut_slice()[write] = row;
                write += 1;
            }
        }
        while self.rows.len() > write {
            self.rows.pop();
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) enum PieceProof {
    Shared(PathProof),
    Choice(ScratchVec<PathProof>),
}

impl ScratchCharge for PieceProof {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 80);
        80
    };
}

impl PieceProof {
    pub(super) fn validate_choice(
        &self,
        length: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Work, 1)?;
        if !matches!(self,Self::Choice(paths) if paths.len()==length) {
            return Err(invalid_input("failed header choice option count changed"));
        }
        Ok(())
    }
    pub(super) fn shared(
        certificate: Option<Certificate>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut path = PathProof::new(governor);
        path.push(0, certificate, governor)?;
        Ok(Self::Shared(path))
    }

    pub(super) fn path(
        &mut self,
        option: usize,
        governor: &mut Governor,
    ) -> Result<PathProof, StopReason> {
        governor.charge(Resource::Work, 1)?;
        let Self::Choice(paths) = self else {
            return Err(invalid_input("failed header choice shape changed"));
        };
        let path = paths
            .as_mut_slice()
            .get_mut(option)
            .ok_or_else(|| invalid_input("failed header option address is missing"))?;
        Ok(core::mem::replace(path, PathProof::new(governor)))
    }

    pub(super) fn shared_path(self) -> Result<PathProof, StopReason> {
        match self {
            Self::Shared(path) => Ok(path),
            Self::Choice(_) => Err(invalid_input("failed header shared-piece shape changed")),
        }
    }
}

/// This companion is inactive until a stored rejection has a certificate.
pub(super) struct BuildProofs {
    active: bool,
    rows: ScratchVec<PieceProof>,
}

impl BuildProofs {
    pub(super) fn new(active: bool, governor: &Governor) -> Self {
        Self {
            active,
            rows: ScratchVec::new(governor),
        }
    }
    pub(super) const fn active(&self) -> bool {
        self.active
    }
    pub(super) fn push(
        &mut self,
        proof: PieceProof,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.active {
            push_scratch(&mut self.rows, proof, governor)?;
        }
        Ok(())
    }
    pub(super) fn shared(
        &mut self,
        certificate: Option<Certificate>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if self.active {
            self.push(PieceProof::shared(certificate, governor)?, governor)?;
        }
        Ok(())
    }
    pub(super) fn take(
        &mut self,
        index: usize,
        governor: &mut Governor,
    ) -> Result<PieceProof, StopReason> {
        if !self.active {
            return Ok(PieceProof::Shared(PathProof::new(governor)));
        }
        governor.charge(Resource::Work, 1)?;
        let row = self
            .rows
            .as_mut_slice()
            .get_mut(index)
            .ok_or_else(|| invalid_input("failed header piece address is missing"))?;
        Ok(core::mem::replace(
            row,
            PieceProof::Shared(PathProof::new(governor)),
        ))
    }
    pub(super) fn pop(&mut self, governor: &mut Governor) -> Result<PieceProof, StopReason> {
        if !self.active {
            return Ok(PieceProof::Shared(PathProof::new(governor)));
        }
        governor.charge(Resource::Work, 1)?;
        self.rows
            .pop()
            .ok_or_else(|| invalid_input("failed header reverse address is missing"))
    }
    pub(super) fn reorder(
        &mut self,
        piece: usize,
        order: &[usize],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if !self.active {
            return Ok(());
        }
        let PieceProof::Choice(paths) = &mut self.rows.as_mut_slice()[piece] else {
            return Err(invalid_input(
                "failed header permutation targets a shared piece",
            ));
        };
        governor.charge(Resource::Work, size(order.len()).saturating_mul(4))?;
        if paths.len() != order.len() {
            return Err(invalid_input("failed header permutation length changed"));
        }
        let mut seen = ScratchVec::new(governor);
        seen.reserve(order.len())?;
        seen.extend(core::iter::repeat_n(false, order.len()))?;
        for &old in order {
            let mark = seen.as_mut_slice().get_mut(old).ok_or_else(|| {
                invalid_input("failed header permutation index is outside its options")
            })?;
            if core::mem::replace(mark, true) {
                return Err(invalid_input("failed header permutation repeats an option"));
            }
        }
        let mut reordered = ScratchVec::new(governor);
        reordered.reserve(order.len())?;
        for &old in order {
            // Capacity and the complete movement bound were admitted above.
            reordered.push(core::mem::replace(
                &mut paths.as_mut_slice()[old],
                PathProof::new(governor),
            ))?;
        }
        *paths = reordered;
        Ok(())
    }
    pub(super) fn finish(
        self,
        pieces: &[Piece],
        governor: &mut Governor,
    ) -> Result<FailureProofs, StopReason> {
        let mut result = FailureProofs::new(governor);
        if !self.active {
            return Ok(result);
        }
        if self.rows.len() != pieces.len() {
            return Err(invalid_input("failed header final piece count changed"));
        }
        for (piece, (proof, source)) in self.rows.iter().zip(pieces).enumerate() {
            governor.charge(Resource::Work, 1)?;
            match (proof, source) {
                (PieceProof::Shared(path), Piece::Segment(segment)) => {
                    result.append(piece, None, path, core::slice::from_ref(segment), governor)?;
                }
                (PieceProof::Choice(paths), Piece::Choice(choice))
                    if paths.len() == choice.readings.len() =>
                {
                    for (option, path) in paths.iter().enumerate() {
                        result.append(
                            piece,
                            Some(option),
                            path,
                            &choice.readings[option],
                            governor,
                        )?;
                    }
                }
                _ => return Err(invalid_input("failed header final piece shape changed")),
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Address {
    piece: usize,
    option: Option<usize>,
    segment: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Located {
    address: Address,
    certificate: Certificate,
}

impl ScratchCharge for Located {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 256);
        256
    };
}

pub(super) type FailureProofs = ProofRows<Located>;

pub(super) struct FailureChoice<'a> {
    proofs: &'a FailureProofs,
    piece: usize,
    span: &'a Range<usize>,
    active: bool,
}

impl<'a> FailureChoice<'a> {
    pub(super) fn new(
        proofs: &'a FailureProofs,
        piece: usize,
        span: &'a Range<usize>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let active = if proofs.rows.is_empty() {
            false
        } else {
            governor.charge(Resource::Work, u64::from(proofs.rows.len().bit_width() + 1))?;
            let at = proofs.rows.partition_point(|row| row.address.piece < piece);
            proofs
                .rows
                .get(at)
                .is_some_and(|row| row.address.piece == piece)
        };
        Ok(Self {
            proofs,
            piece,
            span,
            active,
        })
    }
    pub(super) fn dominated(
        &self,
        member: usize,
        members: &[usize],
        options: &[Vec<ResolvedSegment>],
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if !self.active {
            return Ok(false);
        }
        self.proofs
            .dominated(self.piece, member, members, options, self.span, governor)
    }
}

impl FailureProofs {
    fn append(
        &mut self,
        piece: usize,
        option: Option<usize>,
        path: &PathProof,
        segments: &[Segment],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for row in &path.rows {
            governor.charge(Resource::Work, 48)?;
            let address = Address {
                piece,
                option,
                segment: row.segment,
            };
            if self.rows.last().is_some_and(|last| last.address >= address) {
                return Err(invalid_input(
                    "failed header final addresses are not increasing",
                ));
            }
            if !matches!(segments.get(row.segment), Some(Segment::Rejected { span,dialect,reason })
                if row.certificate.matches(span,dialect,*reason))
            {
                return Err(invalid_input(
                    "failed header recognized structural address changed",
                ));
            }
            push_scratch(
                &mut self.rows,
                Located {
                    address,
                    certificate: row.certificate,
                },
                governor,
            )?;
        }
        Ok(())
    }
    fn get(
        &self,
        piece: usize,
        option: Option<usize>,
        segment: usize,
        governor: &mut Governor,
    ) -> Result<Option<Certificate>, StopReason> {
        if self.rows.is_empty() {
            return Ok(None);
        }
        governor.charge(
            Resource::Work,
            u64::from(self.rows.len().bit_width() + 1)
                .saturating_mul(3)
                .saturating_add(24),
        )?;
        let address = Address {
            piece,
            option,
            segment,
        };
        Ok(self
            .rows
            .binary_search_by_key(&address, |row| row.address)
            .ok()
            .map(|index| self.rows[index].certificate))
    }
    pub(super) fn validate(
        &self,
        turn: &ResolvedTurn,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        for row in &self.rows {
            governor.charge(Resource::Work, 20)?;
            let address = row.address;
            let segment = match (turn.pieces.get(address.piece), address.option) {
                (Some(ResolvedPiece::Segment(segment)), None) if address.segment == 0 => {
                    Some(segment)
                }
                (Some(ResolvedPiece::Choice(choice)), Some(option)) => choice
                    .readings
                    .get(option)
                    .and_then(|segments| segments.get(address.segment)),
                _ => None,
            };
            if !matches!(segment, Some(ResolvedSegment::Rejected {span,dialect,reason,..})
                if row.certificate.matches(span,dialect,*reason))
            {
                return Err(invalid_input(
                    "failed header resolved structural address changed",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn dominated(
        &self,
        piece: usize,
        member: usize,
        members: &[usize],
        options: &[Vec<ResolvedSegment>],
        span: &Range<usize>,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        if self.rows.is_empty() {
            return Ok(false);
        }
        let Some(weak) = self.option(piece, member, &options[member], span, governor)? else {
            return Ok(false);
        };
        if weak.native {
            return Ok(false);
        }
        for &candidate in members {
            if candidate == member {
                continue;
            }
            let Some(native) =
                self.option(piece, candidate, &options[candidate], span, governor)?
            else {
                continue;
            };
            governor.charge(Resource::Work, 24)?;
            if native.native
                && native.frame.start
                    > weak
                        .witness
                        .header
                        .dropped
                        .map_or(usize::MAX, |dropped| dropped.start)
                && native.same_body(&weak)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Exact partitions contain only one rejection and exterior content.
    fn option(
        &self,
        piece: usize,
        option: usize,
        segments: &[ResolvedSegment],
        span: &Range<usize>,
        governor: &mut Governor,
    ) -> Result<Option<Certificate>, StopReason> {
        let mut at = span.start;
        let mut certificate = None;
        for (index, segment) in segments.iter().enumerate() {
            governor.charge(Resource::Work, 4)?;
            let range = match segment {
                ResolvedSegment::Content(range) => range,
                ResolvedSegment::Rejected {
                    span,
                    dialect,
                    reason,
                    repairs,
                } if certificate.is_none() => {
                    governor.charge(Resource::Work, 3)?;
                    if !repairs.evidence.is_empty()
                        || !repairs.losses.is_empty()
                        || !repairs.inferences.is_empty()
                    {
                        return Ok(None);
                    }
                    let Some(proof) = self.get(piece, Some(option), index, governor)? else {
                        return Ok(None);
                    };
                    governor.charge(Resource::Work, 16)?;
                    if !proof.matches(span, dialect, *reason) {
                        return Ok(None);
                    }
                    certificate = Some(proof);
                    span
                }
                _ => return Ok(None),
            };
            if range.start != at || range.end <= range.start || range.end > span.end {
                return Ok(None);
            }
            at = range.end;
        }
        Ok(if at == span.end { certificate } else { None })
    }
}
