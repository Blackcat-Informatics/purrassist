// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Validated caller-owned tool-graph vocabulary and context contracts.
#![doc = include_str!("../README.md")]

mod apparmor_glob;
mod binding;
mod file_rule;
mod glob_language;
mod internet;
mod ipc;
mod policy_document;
mod policy_trust;
mod privilege_rule;
mod prompt_bytes;
mod resource_limits;
mod socket_access;
mod unix;
mod vocab;

pub use purrassist_core::output::{
    ContextCodecError, ContextManagement, LoadingStrategy, MCP_CONTEXT_FORMAT, NullSelection,
    OutputByteLimit, OutputEnforcement, OutputLimitError, OutputMeasurement, OutputTransform,
    OutputUnit, ProjectedResult, ProjectionFailure, ProjectionStage, ProjectionTrace,
    ResultProjectionError,
};
pub use purrassist_core::output::{FieldMask, FieldMaskError, FieldMaskKind};
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

pub use internet::{
    InternetAccessSet, InternetEndpoint, InternetError, InternetFamily, InternetFragment,
    InternetLimits, InternetPorts, InternetRequirement, InternetRule, InternetScope,
    InternetSelector, InternetSide,
};

pub use unix::{
    UnixAccessSet, UnixAddress, UnixAddressProblem, UnixError, UnixField, UnixFragment, UnixLabel,
    UnixPattern, UnixPeer, UnixRequirement, UnixRule, UnixSocketType,
};

pub use file_rule::{
    FileCanonicalization, FileExecution, FileFragment, FilePermissions, FileRequirement, FileRule,
    FileRuleError, FileTarget,
};

pub use ipc::{
    IpcError, IpcFragment, IpcRequirement, PtraceAccessSet, PtraceRule, SignalAccessSet,
    SignalRule, SignalSet,
};

pub use privilege_rule::{
    CapabilityBaseline, CapabilityFragment, CapabilityRule, MountFragment, MountRule,
    PrivilegeError, PrivilegeRequirement, dangerous_capability_denials,
};

pub use resource_limits::{
    ResourceLimitError, ResourceLimitField, ResourceLimitFragment, ResourceLimitProblem,
    ResourceLimitRequirement, ResourceLimits,
};

pub use prompt_bytes::{
    FramedPrompt, FramedPromptBytes, PromptByteError, PromptByteFailure, PromptBytePart,
};

pub use policy_document::{
    BoundPolicyDependency, CompiledPolicy, PolicyBaseline, PolicyBody, PolicyCause,
    PolicyDependency, PolicyDocument, PolicyDuplicateScope, PolicyError, PolicyImport, PolicyInput,
    PolicyOrigin, PolicyPrepareError, PolicyProfile, PolicyRequirements, PolicyRule, PolicySpan,
    PolicyTarget, UnresolvedPolicyImports,
};

pub use policy_trust::{
    MatchedPolicyContract, PolicyContractInput, PolicyGrant, PolicyScope, PolicySource,
    PolicyTrustCause, PolicyTrustContext, PolicyTrustError, PreparedPolicyContract,
    PreparedPolicyDocument, TrustedPolicyRegistry,
};
