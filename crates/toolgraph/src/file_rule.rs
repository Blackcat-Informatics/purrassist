// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Standalone file permission fragments, without graph or execution authority.

use std::fmt;

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, format_output_governed, saturating_size as size},
};

use crate::{AppArmorGlob, GlobError, GlobTokenKind, LiteralPathBinding};

/// A field or rule refusal retaining the original caller identity or field text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRuleError<'a> {
    /// An unknown permission byte.
    Permission {
        /// Original permission text.
        source: &'a str,
        /// Original byte offset.
        offset: usize,
    },
    /// A permission appeared twice.
    DuplicatePermission {
        /// Original permission text.
        source: &'a str,
        /// Repeated byte offset.
        offset: usize,
    },
    /// Write and append cannot be granted together.
    WriteAppend {
        /// Original permission text.
        source: &'a str,
    },
    /// The mode is outside the exact admitted spelling table.
    Execution {
        /// Original mode text.
        source: &'a str,
    },
    /// A target is outside the admitted literal ASCII grammar.
    Target {
        /// Original target text.
        source: &'a str,
        /// Invalid byte, or zero for an empty target.
        offset: usize,
    },
    /// Neither file access nor execution was requested.
    Empty {
        /// Caller rule identity.
        source: &'a str,
    },
    /// A target was supplied without a directed execution mode.
    TargetWithoutTransition {
        /// Caller rule identity.
        source: &'a str,
    },
    /// Denial cannot silently discard an explicit target.
    TargetOnDeny {
        /// Caller rule identity.
        source: &'a str,
    },
    /// Only the documented denied inherit request canonicalizes to bare x.
    DeniedTransition {
        /// Caller rule identity.
        source: &'a str,
        /// Requested transition.
        execution: FileExecution,
    },
    /// This unit cannot authenticate an exact scoped unconfined waiver.
    NeedsAuthenticatedWaiver {
        /// Caller rule identity.
        source: &'a str,
        /// Requested unconfined mode, retaining its environment distinction.
        execution: FileExecution,
    },
    /// The parsed first unit is not an unescaped slash or a bound variable.
    AbsolutePrefix {
        /// Caller rule identity.
        source: &'a str,
        /// Original glob source.
        path: &'a str,
    },
    /// Original binding/source failure, including unchanged byte offsets.
    Glob {
        /// Caller rule identity.
        source: &'a str,
        /// Original refusal.
        reason: GlobError,
    },
    /// Exact input/work/storage/cancellation refusal; no fragment is returned.
    Resource {
        /// Original field text or caller rule identity.
        source: &'a str,
        /// Original governor outcome.
        reason: StopReason,
    },
}
impl fmt::Display for FileRuleError<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "file rule refused: {self:?}")
    }
}
impl std::error::Error for FileRuleError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Glob { reason, .. } => Some(reason),
            Self::Resource { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

fn admit<'a>(source: &'a str, governor: &mut Governor) -> Result<(), FileRuleError<'a>> {
    // One scan and at most seven fixed spelling comparisons; no allocation.
    let work = size(source.len())
        .checked_mul(8)
        .and_then(|work| work.checked_add(8))
        .ok_or(FileRuleError::Resource {
            source,
            reason: StopReason::BudgetExceeded(Resource::Work),
        })?;
    governor
        .charge(Resource::InputBytes, size(source.len()))
        .and_then(|()| governor.charge(Resource::Work, work))
        .map_err(|reason| FileRuleError::Resource { source, reason })
}

/// Unique file permissions, emitted in canonical `rwaklm` order.
///
/// Empty access is valid only when the enclosing rule requests execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilePermissions(u8);
impl FilePermissions {
    /// Parse the exact permission letters without whitespace or aliases.
    ///
    /// # Errors
    /// Refuses unknown/duplicate bytes, write with append, or governance failure.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{FilePermissions, FileRuleError};
    /// let mut g = Governor::new(Budget::default());
    /// assert!(FilePermissions::parse("klrw", &mut g).is_ok());
    /// assert!(matches!(FilePermissions::parse("wa", &mut g), Err(FileRuleError::WriteAppend { .. })));
    /// ```
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, FileRuleError<'a>> {
        admit(source, governor)?;
        let mut bits = 0;
        for (offset, byte) in source.bytes().enumerate() {
            let bit = match byte {
                b'r' => 1,
                b'w' => 2,
                b'a' => 4,
                b'k' => 8,
                b'l' => 16,
                b'm' => 32,
                _ => return Err(FileRuleError::Permission { source, offset }),
            };
            if bits & bit != 0 {
                return Err(FileRuleError::DuplicatePermission { source, offset });
            }
            bits |= bit;
        }
        if bits & 6 == 6 {
            return Err(FileRuleError::WriteAppend { source });
        }
        Ok(Self(bits))
    }
}
impl fmt::Display for FilePermissions {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (bit, letter) in ["r", "w", "a", "k", "l", "m"].into_iter().enumerate() {
            if self.0 & (1 << bit) != 0 {
                output.write_str(letter)?;
            }
        }
        Ok(())
    }
}

/// One exact requested execution mode; unconfined modes never render here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileExecution {
    /// Inherit current confinement: ix.
    Inherit,
    /// Transition to a profile: px.
    Profile,
    /// Transition to a child: cx.
    Child,
    /// Profile transition with scrubbed environment and inherit fallback: Pix.
    ProfileInherit,
    /// Child transition with scrubbed environment and inherit fallback: Cix.
    ChildInherit,
    /// Unconfined ux; rendering requires unavailable authenticated authority.
    Unconfined,
    /// Unconfined Ux with environment scrubbing; also requires authority.
    UnconfinedScrubbed,
}
impl FileExecution {
    /// Parse only ix, px, cx, Pix, Cix, ux or Ux; this grants no authority.
    ///
    /// # Errors
    /// Unknown spellings and governance failures are typed refusals. Both
    /// unconfined variants are refused by [`FileRule::render`].
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{FileExecution, FileRuleError};
    /// let mut g = Governor::new(Budget::default());
    /// assert_eq!(FileExecution::parse("Pix", &mut g).unwrap(), FileExecution::ProfileInherit);
    /// assert!(matches!(FileExecution::parse("pix", &mut g), Err(FileRuleError::Execution { .. })));
    /// ```
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, FileRuleError<'a>> {
        admit(source, governor)?;
        match source {
            "ix" => Ok(Self::Inherit),
            "px" => Ok(Self::Profile),
            "cx" => Ok(Self::Child),
            "Pix" => Ok(Self::ProfileInherit),
            "Cix" => Ok(Self::ChildInherit),
            "ux" => Ok(Self::Unconfined),
            "Ux" => Ok(Self::UnconfinedScrubbed),
            _ => Err(FileRuleError::Execution { source }),
        }
    }
    const fn word(self) -> &'static str {
        match self {
            Self::Inherit => "ix",
            Self::Profile => "px",
            Self::Child => "cx",
            Self::ProfileInherit => "Pix",
            Self::ChildInherit => "Cix",
            Self::Unconfined => "ux",
            Self::UnconfinedScrubbed => "Ux",
        }
    }
}

/// A borrowed literal target with grammar `[A-Za-z_][A-Za-z0-9_.-]*`.
///
/// This names a requested transition; existence and authority are not checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileTarget<'a>(&'a str);
impl<'a> FileTarget<'a> {
    /// Admit a literal target without quoting, escaping or native name expansion.
    ///
    /// # Errors
    /// Refuses an empty or unsupported name at its original byte, or resources.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{FileTarget, FileRuleError};
    /// let mut g = Governor::new(Budget::default());
    /// assert!(FileTarget::parse("purrassist.tool-1", &mut g).is_ok());
    /// assert!(matches!(FileTarget::parse("target/*", &mut g), Err(FileRuleError::Target { offset: 6, .. })));
    /// ```
    pub fn parse(source: &'a str, governor: &mut Governor) -> Result<Self, FileRuleError<'a>> {
        admit(source, governor)?;
        if !source
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return Err(FileRuleError::Target { source, offset: 0 });
        }
        if let Some(offset) = source
            .bytes()
            .position(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'.' | b'-'))
        {
            return Err(FileRuleError::Target { source, offset });
        }
        Ok(Self(source))
    }

    /// Borrow the exact admitted target without interpreting emitted policy text.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::FileTarget;
    /// let target = FileTarget::parse("purrassist.worker", &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(target.as_str(), "purrassist.worker");
    /// ```
    #[must_use]
    pub const fn as_str(self) -> &'a str {
        self.0
    }
}

/// The explicitly recorded transformation of a requested execution mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileCanonicalization {
    /// The emitted mode equals the requested mode, or no mode was requested.
    Unchanged,
    /// Requested denied ix emits native bare x, with no executable grant.
    DeniedInheritToExecute,
}

/// Selected file-mediation target, without kernel or loaded-policy attestation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileRequirement {
    /// Policy ABI selected for offline qualification.
    pub abi: &'static str,
    /// Required native mediation class.
    pub mediation: &'static str,
}

/// Complete standalone fragment and its unchanged source/transition identities.
#[derive(Debug)]
pub struct FileFragment<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Original unexpanded glob source.
    pub path_source: &'a str,
    /// Original explicit literal bindings; caller owns canonicalization and trust.
    pub bindings: &'a [LiteralPathBinding<'a>],
    /// Original execution request, even when canonicalized for denial.
    pub requested_execution: Option<FileExecution>,
    /// Original literal target; never silently removed.
    pub target: Option<FileTarget<'a>>,
    /// The actual execution transformation.
    pub canonicalization: FileCanonicalization,
    /// Complete deterministic rule, ending in comma and newline.
    pub text: String,
    /// Selected offline ABI and file mediation requirement.
    pub requirement: FileRequirement,
}

/// One caller-prepared file rule, without graph, waiver or dispatch authority.
///
/// Paths must start with an unescaped literal slash or a first variable whose
/// exact binding is an absolute literal. Brace/class/escaped leading forms are
/// explicitly refused. Qualifiers print in audit, deny, owner order. Directed
/// targets apply only to px/cx/Pix/Cix; denied ix emits x and records the change.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{AppArmorGlob, FilePermissions, FileRule};
/// let mut g = Governor::new(Budget::default());
/// let path = AppArmorGlob::parse("/srv/**", &[], &mut g).unwrap();
/// let rule = FileRule { source: "caller:read", path: &path, bindings: &[],
///     permissions: FilePermissions::parse("r", &mut g).unwrap(), execution: None,
///     target: None, audit: false, deny: false, owner: true };
/// assert_eq!(rule.render(&mut g).unwrap().text, "owner \"/srv/**\" r,\n");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct FileRule<'a> {
    /// Original caller rule identity, retained without copying.
    pub source: &'a str,
    /// Already parsed source; parsing is caller-owned.
    pub path: &'a AppArmorGlob,
    /// Exact binding set for the source; no ambient variable resolution occurs.
    pub bindings: &'a [LiteralPathBinding<'a>],
    /// Validated file permissions; may be empty only with execution.
    pub permissions: FilePermissions,
    /// At most one requested execution mode.
    pub execution: Option<FileExecution>,
    /// Optional literal directed profile or child target.
    pub target: Option<FileTarget<'a>>,
    /// Emit the audit qualifier.
    pub audit: bool,
    /// Emit a deny rule, never an execution grant.
    pub deny: bool,
    /// Limit the rule to the native file-owner condition.
    pub owner: bool,
}
impl<'a> FileRule<'a> {
    // Validate the closed execution/target table before producing any word.
    fn execution_word(self) -> Result<&'static str, FileRuleError<'a>> {
        if self.deny && self.target.is_some() {
            return Err(FileRuleError::TargetOnDeny {
                source: self.source,
            });
        }
        match self.execution {
            Some(execution @ (FileExecution::Unconfined | FileExecution::UnconfinedScrubbed)) => {
                Err(FileRuleError::NeedsAuthenticatedWaiver {
                    source: self.source,
                    execution,
                })
            }
            Some(execution) if self.deny && execution != FileExecution::Inherit => {
                Err(FileRuleError::DeniedTransition {
                    source: self.source,
                    execution,
                })
            }
            Some(FileExecution::Inherit) | None if self.target.is_some() => {
                Err(FileRuleError::TargetWithoutTransition {
                    source: self.source,
                })
            }
            Some(_) if self.deny => Ok("x"),
            Some(execution) => Ok(execution.word()),
            None => Ok(""),
        }
    }

    /// Render one complete fragment against the caller governor.
    ///
    /// Source/bindings are validated before output. Rendering reuses the actual
    /// glob binding/policy-word operation and the shared fallible formatter.
    /// Intermediate word charges remain retained as documented by those APIs.
    ///
    /// # Errors
    /// Refuses empty rules, misplaced targets, unsupported denied transitions,
    /// unconfined modes requiring authenticated waivers, nonabsolute lexical
    /// prefixes, original binding errors and exact governance failures. A refusal
    /// publishes no partial fragment. Target existence and host confinement are
    /// not established by success.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{AppArmorGlob, FileExecution, FilePermissions, FileRule, FileRuleError};
    /// let mut g = Governor::new(Budget::default());
    /// let path = AppArmorGlob::parse("/bin/tool", &[], &mut g).unwrap();
    /// let rule = FileRule { source: "caller:unconfined", path: &path, bindings: &[],
    ///     permissions: FilePermissions::parse("", &mut g).unwrap(), execution: Some(FileExecution::Unconfined),
    ///     target: None, audit: false, deny: false, owner: false };
    /// assert!(matches!(rule.render(&mut g), Err(FileRuleError::NeedsAuthenticatedWaiver { .. })));
    /// ```
    pub fn render(self, governor: &mut Governor) -> Result<FileFragment<'a>, FileRuleError<'a>> {
        admit(self.source, governor)?;
        let resource = |reason| FileRuleError::Resource {
            source: self.source,
            reason,
        };
        governor
            .charge(Resource::InputBytes, size(self.path.source().len()))
            .map_err(resource)?;
        if let Some(target) = self.target {
            governor
                .charge(Resource::InputBytes, size(target.0.len()))
                .map_err(resource)?;
        }
        let execution = self.execution_word()?;
        if self.permissions.0 == 0 && self.execution.is_none() {
            return Err(FileRuleError::Empty {
                source: self.source,
            });
        }
        let positions = self
            .path
            .prepare_bindings(self.bindings, governor)
            .map_err(|reason| FileRuleError::Glob {
                source: self.source,
                reason,
            })?;
        governor.charge(Resource::Work, 1).map_err(resource)?;
        let absolute = self
            .path
            .tokens()
            .first()
            .is_some_and(|token| match token.kind {
                GlobTokenKind::Literal => self.path.source().as_bytes().first() == Some(&b'/'),
                GlobTokenKind::Variable { declaration } => self.bindings
                    [positions.as_slice()[declaration]]
                    .path
                    .starts_with('/'),
                _ => false,
            });
        if !absolute {
            return Err(FileRuleError::AbsolutePrefix {
                source: self.source,
                path: self.path.source(),
            });
        }
        drop(positions);
        let word = self
            .path
            .bind_literal_paths(self.bindings, governor)
            .map_err(|reason| FileRuleError::Glob {
                source: self.source,
                reason,
            })?;
        // Permission Display performs six bounded tests. Other arguments are
        // borrowed str values or constants; output copies are admitted by the
        // shared formatter before every write and actual buffer growth.
        governor.charge(Resource::Work, 6).map_err(resource)?;
        let text = format_output_governed(
            format_args!(
                "{}{}{}{} {}{}{}{},\n",
                if self.audit { "audit " } else { "" },
                if self.deny { "deny " } else { "" },
                if self.owner { "owner " } else { "" },
                word,
                self.permissions,
                execution,
                if self.target.is_some() { " -> " } else { "" },
                self.target.map_or("", |target| target.0),
            ),
            governor,
        )
        .map_err(resource)?;
        governor.check().map_err(resource)?;
        Ok(FileFragment {
            source: self.source,
            path_source: self.path.source(),
            bindings: self.bindings,
            requested_execution: self.execution,
            target: self.target,
            canonicalization: if self.deny && self.execution.is_some() {
                FileCanonicalization::DeniedInheritToExecute
            } else {
                FileCanonicalization::Unchanged
            },
            text,
            requirement: FileRequirement {
                abi: "3.0",
                mediation: "file",
            },
        })
    }
}
