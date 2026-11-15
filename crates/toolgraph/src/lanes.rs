// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Projection lanes over a loaded [`ToolGraph`](crate::ToolGraph).
//!
//! Each lane returns its artifact together with a `LossLedger` that names every
//! constraint the artifact does not carry. The compact signature lane and the
//! GBNF argument grammar lane are implemented here.

mod compact;
mod gbnf;
mod shared;

pub use compact::{CompactError, CompactSignature, compact};
pub use gbnf::{GbnfError, ToolGrammar, gbnf};
