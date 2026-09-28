// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Intent-aware assistance for imperfect AI output.
#![doc = include_str!("../README.md")]

/// Compiled schema validation and recovery entry point.
pub use purrassist_core::schema::Schema;
/// Effort, resource, and invention policy for JSON recovery.
pub use purrassist_core::schema::recovery::RecoveryRequest;
/// Pure first-party assistance engines and contracts.
pub use purrassist_core::*;

/// Isolated grammar parsing and validated code edits.
pub use purrassist_syntax as syntax;
