// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Example normalization moves and intersects its rejection-proof companion.

use super::{
    Builder, Choice, EXAMPLE_RECORD_BYTES, Piece, Segment, push_merged, same_example_reading,
};
use crate::contract::{
    Governor, Resource, ScratchVec, StopReason, invalid_input, push_scratch,
    saturating_size as size,
};
use crate::tool_call::failed_prefix::{BuildProofs, PathProof, PieceProof};
use core::ops::Range;

impl Builder<'_> {
    pub(super) fn normalize_examples(
        &self,
        pieces: &mut Vec<Piece>,
        proofs: &mut BuildProofs,
        fenced: &[Range<usize>],
        inline: &[Range<usize>],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(
            Resource::ScratchBytes,
            size(pieces.len()).saturating_mul(EXAMPLE_RECORD_BYTES),
        )?;
        let active = proofs.active();
        let mut old = core::mem::replace(proofs, BuildProofs::new(active, governor));
        let mut normalized = Vec::new();
        let mut normalized_proofs = BuildProofs::new(active, governor);
        for (piece_index, piece) in core::mem::take(pieces).into_iter().enumerate() {
            let proof = old.take(piece_index, governor)?;
            match piece {
                Piece::Segment(mut segment) => {
                    let certificate = proof.shared_path()?.get(0, governor)?;
                    self.suppress_segment(&mut segment, fenced, inline, governor)?;
                    normalized_proofs.shared(
                        if matches!(segment, Segment::Rejected { .. }) {
                            certificate
                        } else {
                            None
                        },
                        governor,
                    )?;
                    normalized.push(Piece::Segment(segment));
                }
                Piece::Choice(choice) => {
                    let (mut choice, mut paths) = self.normalize_example_choice(
                        choice, proof, active, fenced, inline, governor,
                    )?;
                    if choice.readings.len() == 1 {
                        let path = if active {
                            paths.pop().ok_or_else(|| {
                                invalid_input("failed header collapsed choice has no path")
                            })?
                        } else {
                            PathProof::new(governor)
                        };
                        let segments = choice.readings.pop().ok_or_else(|| {
                            invalid_input("normalized choice lost its sole reading")
                        })?;
                        for (index, segment) in segments.into_iter().enumerate() {
                            normalized_proofs.shared(path.get(index, governor)?, governor)?;
                            normalized.push(Piece::Segment(segment));
                        }
                    } else {
                        normalized_proofs.push(PieceProof::Choice(paths), governor)?;
                        normalized.push(Piece::Choice(choice));
                    }
                }
            }
        }
        for (index, piece) in normalized.into_iter().enumerate() {
            let proof = normalized_proofs.take(index, governor)?;
            if let (
                Some(Piece::Segment(Segment::Content(before))),
                Piece::Segment(Segment::Content(after)),
            ) = (pieces.last_mut(), &piece)
                && before.end == after.start
            {
                before.end = after.end;
            } else {
                pieces.push(piece);
                proofs.push(proof, governor)?;
            }
        }
        Ok(())
    }

    fn normalize_example_choice(
        &self,
        mut choice: Choice,
        mut proof: PieceProof,
        active: bool,
        fenced: &[Range<usize>],
        inline: &[Range<usize>],
        governor: &mut Governor,
    ) -> Result<(Choice, ScratchVec<PathProof>), StopReason> {
        if active {
            proof.validate_choice(choice.readings.len(), governor)?;
        }
        let segment_count: usize = choice.readings.iter().map(Vec::len).sum();
        governor.charge(
            Resource::ScratchBytes,
            size(segment_count)
                .saturating_mul(EXAMPLE_RECORD_BYTES.saturating_mul(2))
                .saturating_add(size(choice.readings.len()).saturating_mul(64)),
        )?;
        if choice.readings.len() != choice.derivations.len() {
            return Err(invalid_input("normalized choice derivation count changed"));
        }
        let mut readings: Vec<Vec<Segment>> = Vec::new();
        let mut derivations = Vec::new();
        let mut paths: ScratchVec<PathProof> = ScratchVec::new(governor);
        for (option, (reading, derivation)) in choice
            .readings
            .into_iter()
            .zip(choice.derivations)
            .enumerate()
        {
            let path = if active {
                proof.path(option, governor)?
            } else {
                PathProof::new(governor)
            };
            let normalized =
                self.normalize_example_reading(reading, &path, fenced, inline, governor);
            // Preserve the consumed owner's old return/refusal drop boundary.
            drop(path);
            let (segments, path) = normalized?;
            let mut duplicate = None;
            for (index, previous) in readings.iter().enumerate() {
                if same_example_reading(&segments, previous, governor)? {
                    duplicate = Some(index);
                    break;
                }
            }
            if let Some(index) = duplicate {
                if active {
                    paths
                        .as_mut_slice()
                        .get_mut(index)
                        .ok_or_else(|| invalid_input("failed header duplicate option is missing"))?
                        .intersect(&path, governor)?;
                }
            } else {
                readings.push(segments);
                derivations.push(derivation);
                if active {
                    push_scratch(&mut paths, path, governor)?;
                }
            }
        }
        choice.readings = readings;
        choice.derivations = derivations;
        Ok((choice, paths))
    }

    fn normalize_example_reading(
        &self,
        reading: Vec<Segment>,
        path: &PathProof,
        fenced: &[Range<usize>],
        inline: &[Range<usize>],
        governor: &mut Governor,
    ) -> Result<(Vec<Segment>, PathProof), StopReason> {
        let mut segments = Vec::new();
        let mut proofs = PathProof::new(governor);
        for (index, mut segment) in reading.into_iter().enumerate() {
            let certificate = path.get(index, governor)?;
            self.suppress_segment(&mut segment, fenced, inline, governor)?;
            let certificate = if matches!(segment, Segment::Rejected { .. }) {
                certificate
            } else {
                None
            };
            proofs.push(segments.len(), certificate, governor)?;
            push_merged(&mut segments, segment);
        }
        Ok((segments, proofs))
    }
}
