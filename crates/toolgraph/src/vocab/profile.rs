// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Versioned loss meanings, shared by every tool-graph projection.

use std::borrow::Cow;

use purrassist_core::tool_call::DeclarationForm;
use purrdf_core::loss::{LossDefinition, LossProfile, LossProfileError};

/// Identity of this version's projection code and representation contract.
pub const LOSS_PROFILE_ID: &str = "purrassist-toolgraph-v1";
/// Source representation used by every tool-graph loss entry.
pub const SOURCE_REPRESENTATION: &str = "rdf-1.2-toolgraph";

/// Model template identity, paired with its existing declaration form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFamily {
    /// Claude Anthropic tools field.
    Claude,
    /// Qwen 2.5 using its Hermes template.
    Qwen25,
    /// Qwen 3 using its Hermes template.
    Qwen3,
    /// Qwen3-Coder XML tool declarations.
    Qwen3Coder,
    /// Hermes tools block.
    Hermes,
    /// Llama 3 tool declarations.
    Llama3,
    /// Llama 4 with the admitted Llama declaration template.
    Llama4,
    /// Mistral v3 available-tools segment.
    MistralV3,
    /// Mistral v11 available-tools segment.
    MistralV11,
    /// `DeepSeek` V3 fenced declarations.
    DeepSeekV3,
    /// `DeepSeek` V3.1 tool sections.
    DeepSeekV31,
    /// Compact preamble plus `OpenAI` declaration field.
    CompactTypeScript,
}

impl ModelFamily {
    /// All specified model templates in stable order.
    pub const ALL: [Self; 12] = [
        Self::Claude,
        Self::Qwen25,
        Self::Qwen3,
        Self::Qwen3Coder,
        Self::Hermes,
        Self::Llama3,
        Self::Llama4,
        Self::MistralV3,
        Self::MistralV11,
        Self::DeepSeekV3,
        Self::DeepSeekV31,
        Self::CompactTypeScript,
    ];

    /// Exact existing declaration renderer for this template.
    ///
    /// ```
    /// use purrassist_core::tool_call::DeclarationForm;
    /// use purrassist_toolgraph::ModelFamily;
    /// assert_eq!(ModelFamily::Qwen3.declaration_form(), DeclarationForm::Hermes);
    /// ```
    #[must_use]
    pub const fn declaration_form(self) -> DeclarationForm {
        match self {
            Self::Claude => DeclarationForm::Anthropic,
            Self::Qwen25 | Self::Qwen3 | Self::Hermes => DeclarationForm::Hermes,
            Self::Qwen3Coder => DeclarationForm::Qwen3CoderXml,
            Self::Llama3 | Self::Llama4 => DeclarationForm::Llama3Json,
            Self::MistralV3 => DeclarationForm::MistralV3,
            Self::MistralV11 => DeclarationForm::MistralV11,
            Self::DeepSeekV3 => DeclarationForm::DeepSeekV3,
            Self::DeepSeekV31 => DeclarationForm::DeepSeekV31,
            Self::CompactTypeScript => DeclarationForm::OpenAi,
        }
    }

    const fn representation(self) -> &'static str {
        match self {
            Self::Claude => "prompt.claude/anthropic-tool-use.v1",
            Self::Qwen25 => "prompt.qwen-2.5/hermes.v1",
            Self::Qwen3 => "prompt.qwen-3/hermes.v1",
            Self::Qwen3Coder => "prompt.qwen3-coder/qwen3-coder-xml.v1",
            Self::Hermes => "prompt.hermes/hermes.v1",
            Self::Llama3 => "prompt.llama-3/llama3-json.v1",
            Self::Llama4 => "prompt.llama-4/llama3-json.v1",
            Self::MistralV3 => "prompt.mistral-v3/mistral-v3.v1",
            Self::MistralV11 => "prompt.mistral-v11/mistral-v11.v1",
            Self::DeepSeekV3 => "prompt.deepseek-v3/deepseek-v3.v1",
            Self::DeepSeekV31 => "prompt.deepseek-v3.1/deepseek-v3.1.v1",
            Self::CompactTypeScript => "prompt.compact-typescript/openai-tool-calls.v1",
        }
    }
}

/// A projection representation; identity includes the exact model template.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionTarget {
    /// Complete MCP Extended manifest with original RDF bundle.
    McpExtended,
    /// `AppArmor` 4.1 policy.
    AppArmor,
    /// llama.cpp grammar.
    Gbnf,
    /// Compact signature preamble.
    CompactSignature,
    /// Existing declaration rendered for one model template.
    Prompt(ModelFamily),
}

impl ProjectionTarget {
    /// Stable target identifier used in native `LossLedger` entries.
    ///
    /// ```
    /// use purrassist_toolgraph::{ModelFamily, ProjectionTarget};
    /// assert_eq!(ProjectionTarget::Prompt(ModelFamily::Hermes).representation(), "prompt.hermes/hermes.v1");
    /// ```
    #[must_use]
    pub const fn representation(self) -> &'static str {
        match self {
            Self::McpExtended => "mcp-extended-v1",
            Self::AppArmor => "apparmor-4.1",
            Self::Gbnf => "llama-gbnf",
            Self::CompactSignature => "compact-signature-v1",
            Self::Prompt(family) => family.representation(),
        }
    }
}

#[derive(Clone, Copy)]
enum Lane {
    AppArmor,
    Gbnf,
    Compact,
    Prompt,
}
use Lane::{AppArmor as A, Compact as C, Gbnf as G, Prompt as P};

struct Meaning {
    code: &'static str,
    description: &'static str,
    lanes: &'static [Lane],
}

const MEANINGS: &[Meaning] = &[
    Meaning {
        code: "authorization-not-expressible",
        description: "Call authorization is not enforced by this representation.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "session-binding-not-expressible",
        description: "Argument equality to authenticated session facts is not enforced by this representation.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "taint-not-expressible",
        description: "Trusted argument taint labels are not enforced by this representation.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "recovery-hints-not-expressible",
        description: "Alias, distance, coercion and folding tolerances require recovery evidence checks.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "confinement-not-expressible",
        description: "Required host confinement is not enforced by this representation.",
        lanes: &[G, C, P],
    },
    Meaning {
        code: "resource-ceiling-not-expressible",
        description: "Execution resource ceilings require separate enforcement.",
        lanes: &[G, C, P],
    },
    Meaning {
        code: "output-shape-not-expressible",
        description: "The complete output shape requires validation before release.",
        lanes: &[A, G, P],
    },
    Meaning {
        code: "input-shape-not-expressible",
        description: "The complete input shape requires validation before authorization.",
        lanes: &[A],
    },
    Meaning {
        code: "context-contract-not-expressible",
        description: "Output projection, byte bounds and history retention require separate enforcement.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "tool-annotations-not-expressible",
        description: "Tool title and descriptive hints are not represented completely.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "rdf-binding-not-expressible",
        description: "FnO bindings, RDF lists and source reifiers require the original graph bundle.",
        lanes: &[A, G, C, P],
    },
    Meaning {
        code: "description-not-expressible",
        description: "The original tool description requires retention in the immutable bundle.",
        lanes: &[A, G, C],
    },
    Meaning {
        code: "json-string-validity-not-expressible",
        description: "Exact JSON string and Unicode validity requires input validation.",
        lanes: &[G],
    },
    Meaning {
        code: "schema-keyword-not-expressible",
        description: "Schema keyword semantics require complete input and output validation.",
        lanes: &[C],
    },
    Meaning {
        code: "tool-selection-omitted",
        description: "An on-demand tool declaration was omitted; only the selected immutable tool set may be used.",
        lanes: &[P],
    },
];

pub(super) fn definitions() -> impl Iterator<Item = LossDefinition> {
    [
        ProjectionTarget::AppArmor,
        ProjectionTarget::Gbnf,
        ProjectionTarget::CompactSignature,
    ]
    .into_iter()
    .chain(ModelFamily::ALL.into_iter().map(ProjectionTarget::Prompt))
    .flat_map(|target| {
        MEANINGS
            .iter()
            .filter(move |meaning| {
                meaning.lanes.iter().any(|lane| {
                    matches!(
                        (lane, target),
                        (A, ProjectionTarget::AppArmor)
                            | (G, ProjectionTarget::Gbnf)
                            | (
                                C,
                                ProjectionTarget::CompactSignature
                                    | ProjectionTarget::Prompt(ModelFamily::CompactTypeScript)
                            )
                            | (P, ProjectionTarget::Prompt(_))
                    )
                })
            })
            .map(move |meaning| LossDefinition {
                from: Cow::Borrowed(SOURCE_REPRESENTATION),
                to: Cow::Borrowed(target.representation()),
                code: Cow::Borrowed(meaning.code),
                description: Cow::Borrowed(meaning.description),
            })
    })
}

/// Extend an explicit upstream profile with the complete versioned tool-graph meanings.
///
/// The original registry and its builtins are retained. Repeating registration
/// is idempotent. MCP's complete manifest has no invented placeholder loss.
/// The combined compact-template target registers both preamble (C) and
/// declaration (P) meanings. Its future projection uses one family-target ledger,
/// with each runtime entry identifying its source and affected component.
///
/// # Errors
/// Returns the upstream conflict with its original definition when a code has
/// an incompatible existing meaning. No partially extended profile is exposed.
///
/// ```
/// use purrdf_core::loss::LossProfile;
/// use purrassist_toolgraph::{register_loss_profile, SOURCE_REPRESENTATION};
/// let profile = register_loss_profile(LossProfile::builtin()).unwrap();
/// assert!(profile.description(SOURCE_REPRESENTATION, "llama-gbnf", "confinement-not-expressible").is_some());
/// assert_eq!(profile.codes_for(SOURCE_REPRESENTATION, "mcp-extended-v1").count(), 0);
/// ```
pub fn register_loss_profile(profile: LossProfile) -> Result<LossProfile, LossProfileError> {
    profile.extend(definitions())
}
