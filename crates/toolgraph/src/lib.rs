// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Validated caller-owned tool-graph vocabulary and context contracts.
#![doc = include_str!("../README.md")]

mod apparmor_glob;
mod binding;
mod context;
mod field_mask;
mod glob_language;
mod vocab;

pub use context::{
    ContextCodecError, ContextManagement, LoadingStrategy, MCP_CONTEXT_FORMAT, OutputByteLimit,
    OutputEnforcement, OutputLimitError, OutputMeasurement, OutputUnit,
};
pub use field_mask::{FieldMask, FieldMaskError, FieldMaskKind};
pub use vocab::{
    BindingProblem, BindingSection, LOSS_PROFILE_ID, ModelFamily, PrefixTable, ProjectionTarget,
    SOURCE_REPRESENTATION, TERM_NAMES, TermBindings, Vocabulary, VocabularyError,
    register_loss_profile,
};

pub use apparmor_glob::{
    AppArmorGlob, GlobBindingProblem, GlobError, GlobToken, GlobTokenKind, LiteralPathBinding,
    literal_path_word,
};
pub use binding::{BindingError, CallableBinding, ParameterBinding, ReturnBinding};
pub use glob_language::{GlobInclusion, GlobLanguage, GlobLanguageError, GlobLanguageInput};
