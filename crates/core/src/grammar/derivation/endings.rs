// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Source-terminal proof retained before marker projection erases its evidence.

use super::{Document, GrammarError, PExpr, Statement, symbol};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, saturating_size,
};
use crate::grammar::CallEndingProof;

#[derive(Clone, Copy, Default)]
struct Endings {
    nullable: bool,
    erased: bool,
    retained: bool,
}

impl Endings {
    const EMPTY: Self = Self {
        nullable: true,
        erased: false,
        retained: false,
    };

    const fn alternative(&mut self, other: Self) {
        self.nullable |= other.nullable;
        self.erased |= other.erased;
        self.retained |= other.retained;
    }

    const fn followed_by(&mut self, other: Self) {
        self.erased = other.erased || other.nullable && self.erased;
        self.retained = other.retained || other.nullable && self.retained;
        self.nullable &= other.nullable;
    }
}

#[derive(Clone, Copy)]
enum Visit {
    Unseen,
    Active,
    Done(Endings),
}

impl ScratchCharge for Visit {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 8);
        8
    };
}

struct Proof<'a> {
    document: &'a Document,
    markers: &'a [&'a str],
    visited: ScratchVec<Visit>,
}

pub(super) fn prove(
    document: &Document,
    markers: &[&str],
    governor: &mut Governor,
) -> Result<CallEndingProof, GrammarError> {
    let Some(call) = symbol(&document.statements, "call", governor)? else {
        return Err(GrammarError::MissingProduction { name: "call" });
    };
    governor.charge(Resource::Work, saturating_size(document.statements.len()))?;
    let mut visited = ScratchVec::new(governor);
    visited.reserve(document.statements.len())?;
    visited.extend(core::iter::repeat_n(
        Visit::Unseen,
        document.statements.len(),
    ))?;
    let ends = Proof {
        document,
        markers,
        visited,
    }
    .statement(call, 1, governor)?;
    Ok(if !ends.nullable && ends.erased && !ends.retained {
        CallEndingProof::AllErased
    } else {
        CallEndingProof::Unproven
    })
}

impl<'a> Proof<'a> {
    fn statement(
        &mut self,
        index: usize,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<Endings, GrammarError> {
        admit(depth, governor)?;
        match self.visited[index] {
            Visit::Done(ends) => return Ok(ends),
            Visit::Active => {
                let document = self.document;
                let Statement::Production { name, at, .. } = &document.statements[index] else {
                    unreachable!("only production references can form a cycle")
                };
                return Err(GrammarError::Recursive {
                    offset: *at,
                    name: diagnostic(name.as_str(), governor)?,
                });
            }
            Visit::Unseen => {}
        }
        self.visited.as_mut_slice()[index] = Visit::Active;
        let document = self.document;
        let ends = match &document.statements[index] {
            Statement::Production { expr, .. } => self.expression(expr, depth + 1, governor)?,
            Statement::Closer { spellings, .. } => {
                let mut ends = Endings::default();
                for spelling in spellings {
                    ends.alternative(self.bytes(spelling, governor)?);
                }
                ends
            }
            _ => unreachable!("symbol lookup resolves only productions and closer classes"),
        };
        self.visited.as_mut_slice()[index] = Visit::Done(ends);
        Ok(ends)
    }

    fn expression(
        &mut self,
        expr: &'a PExpr,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<Endings, GrammarError> {
        admit(depth, governor)?;
        Ok(match expr {
            PExpr::Literal { bytes, .. } => self.bytes(bytes, governor)?,
            PExpr::Name { name, at } => {
                let Some(index) = symbol(&self.document.statements, name.as_str(), governor)?
                else {
                    return Err(GrammarError::UnknownName {
                        offset: *at,
                        name: diagnostic(name.as_str(), governor)?,
                    });
                };
                self.statement(index, depth + 1, governor)?
            }
            PExpr::Capture { inner, .. } => self.expression(inner.get(), depth + 1, governor)?,
            PExpr::Repeat { inner, op, .. } => {
                let mut ends = self.expression(inner.get(), depth + 1, governor)?;
                ends.nullable |= *op != b'+';
                ends
            }
            PExpr::Sequence(items, ..) => {
                let mut ends = Endings::EMPTY;
                for item in items {
                    ends.followed_by(self.expression(item, depth + 1, governor)?);
                }
                ends
            }
            PExpr::Choice(items, ..) => {
                let mut ends = Endings::default();
                for item in items {
                    ends.alternative(self.expression(item, depth + 1, governor)?);
                }
                ends
            }
            PExpr::Class { .. } => Endings {
                nullable: false,
                erased: false,
                retained: true,
            },
            PExpr::Until { .. } | PExpr::Slot { .. } => Endings {
                nullable: true,
                erased: false,
                retained: true,
            },
            PExpr::Commit | PExpr::Next { .. } | PExpr::Insert { .. } => Endings::EMPTY,
        })
    }

    fn bytes(&self, bytes: &[u8], governor: &mut Governor) -> Result<Endings, GrammarError> {
        if bytes.is_empty() {
            return Ok(Endings::EMPTY);
        }
        let mut at = 0;
        while at < bytes.len() {
            let mut longest = 0;
            for marker in self.markers {
                governor.charge(
                    Resource::Work,
                    saturating_size(marker.len().min(bytes.len() - at)).saturating_add(1),
                )?;
                if bytes[at..].starts_with(marker.as_bytes()) {
                    longest = longest.max(marker.len());
                }
            }
            if longest == 0 {
                return Ok(Endings {
                    nullable: false,
                    erased: false,
                    retained: true,
                });
            }
            at += longest;
        }
        Ok(Endings {
            nullable: false,
            erased: true,
            retained: false,
        })
    }
}

fn admit(depth: u64, governor: &mut Governor) -> Result<(), GrammarError> {
    governor.charge(Resource::Depth, depth)?;
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth).into());
    }
    governor.charge(Resource::Work, 1)?;
    Ok(())
}

fn diagnostic(name: &str, governor: &mut Governor) -> Result<String, GrammarError> {
    governor.charge(Resource::Work, saturating_size(name.len()))?;
    let mut owned = ScratchString::new(governor);
    owned.push_str(name)?;
    Ok(owned.into_string())
}
