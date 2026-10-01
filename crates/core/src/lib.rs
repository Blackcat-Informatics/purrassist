// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Pure intent recovery engines and auditable assistance contracts.
#![doc = include_str!("../README.md")]

pub mod contract;
pub mod digest;
pub mod json;
pub mod matching;
pub mod pattern;
pub mod schema;
pub mod text;
pub mod tool_call;

pub use contract::{
    AssistResult, Budget, CancellationToken, Effort, Governor, Outcome, RECEIPT_VERSION, Receipt,
    ScratchCharge, ScratchLease, ScratchMap, ScratchMeter, ScratchString, ScratchVec, StopReason,
};

pub mod wire;

pub mod embeddings;
pub mod intent;

pub mod paths;

pub mod edits;

pub mod patch;

pub mod provenance;

pub mod scan;

pub mod search;

pub mod markdown;
