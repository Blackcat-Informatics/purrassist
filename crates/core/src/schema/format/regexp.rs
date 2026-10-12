// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `regex`: the value must compile as an ECMA-262 pattern (the derivative/backtracking
//! pattern engine), never evaluated against any instance.
use crate::contract::{Governor, Resource, StopReason};
use crate::pattern::Pattern;

pub(super) fn is_regex(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    governor.charge(Resource::Work, 1)?;
    match Pattern::compile_governed(text, governor) {
        Ok(_) => Ok(true),
        Err(crate::pattern::PatternCompileError::Syntax(_)) => Ok(false),
        Err(crate::pattern::PatternCompileError::Resource(reason)) => Err(reason),
    }
}
