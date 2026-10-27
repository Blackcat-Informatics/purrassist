// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Named capability and unscoped mount-denial fragments without graph authority.

use std::fmt;

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, format_output_governed, saturating_size as size},
};

// Exact ABI 3.0 spellings, in native capability-number order. The newer
// checkpoint_restore spelling is recognized separately because native ABI
// compatibility lowering would silently grant sys_admin instead.
const CAPABILITIES: &[&str] = &[
    "chown",
    "dac_override",
    "dac_read_search",
    "fowner",
    "fsetid",
    "kill",
    "setgid",
    "setuid",
    "setpcap",
    "linux_immutable",
    "net_bind_service",
    "net_broadcast",
    "net_admin",
    "net_raw",
    "ipc_lock",
    "ipc_owner",
    "sys_module",
    "sys_rawio",
    "sys_chroot",
    "sys_ptrace",
    "sys_pacct",
    "sys_admin",
    "sys_boot",
    "sys_nice",
    "sys_resource",
    "sys_time",
    "sys_tty_config",
    "mknod",
    "lease",
    "audit_write",
    "audit_control",
    "setfcap",
    "mac_override",
    "mac_admin",
    "syslog",
    "wake_alarm",
    "block_suspend",
    "audit_read",
    "perfmon",
    "bpf",
];

// Normative order from TOOLS.md; every entry is explicitly denied by the baseline.
const DANGEROUS: &[&str] = &[
    "sys_admin",
    "sys_ptrace",
    "sys_module",
    "sys_rawio",
    "net_admin",
    "dac_override",
    "dac_read_search",
    "setuid",
    "setgid",
    "mknod",
    "chown",
    "fowner",
    "kill",
    "net_bind_service",
];
const MOUNT_OPERATIONS: &[&str] = &["mount", "remount", "umount", "pivot_root"];

/// Typed semantic or resource refusal retaining the caller's exact identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivilegeError<'a> {
    /// The capability is outside the exact single-name spelling table.
    UnknownCapability {
        /// Caller rule identity.
        source: &'a str,
        /// Original capability field, without trimming or normalization.
        name: &'a str,
    },
    /// The native compiler would change this capability for the selected target.
    UnsupportedCapability {
        /// Caller rule identity.
        source: &'a str,
        /// Original capability field.
        name: &'a str,
        /// Selected offline ABI.
        abi: &'static str,
    },
    /// The normative dangerous-capability deny set cannot be overridden.
    DangerousGrant {
        /// Caller rule identity.
        source: &'a str,
        /// Original requested capability.
        name: &'a str,
    },
    /// The operation is outside the four unscoped mount spellings.
    UnknownMountOperation {
        /// Caller rule identity.
        source: &'a str,
        /// Original operation field.
        operation: &'a str,
    },
    /// Mount grants are never emitted by this capability.
    MountAllow {
        /// Caller rule identity.
        source: &'a str,
        /// Original requested operation.
        operation: &'a str,
    },
    /// Exact governance refusal; no complete fragment was published.
    Resource {
        /// Caller rule identity.
        source: &'a str,
        /// Field whose admission or emission failed.
        field: &'static str,
        /// Unchanged original field text or baseline source identity.
        value: &'a str,
        /// Native input, work, storage or cancellation outcome.
        reason: StopReason,
    },
}
impl fmt::Display for PrivilegeError<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "privilege rule refused: {self:?}")
    }
}
impl std::error::Error for PrivilegeError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

const fn resource<'a>(
    source: &'a str,
    field: &'static str,
    value: &'a str,
    reason: StopReason,
) -> PrivilegeError<'a> {
    PrivilegeError::Resource {
        source,
        field,
        value,
        reason,
    }
}

fn admit<'a>(
    source: &'a str,
    field: &'static str,
    value: &'a str,
    comparisons: u64,
    governor: &mut Governor,
) -> Result<(), PrivilegeError<'a>> {
    // Conservatively admit each possible byte comparison and table visit before
    // any lookup. Checked counts retain the same typed work-budget outcome.
    let work = size(value.len())
        .checked_add(1)
        .and_then(|length| length.checked_mul(comparisons))
        .ok_or_else(|| {
            resource(
                source,
                field,
                value,
                StopReason::BudgetExceeded(Resource::Work),
            )
        })?;
    governor
        .charge(Resource::InputBytes, size(value.len()))
        .and_then(|()| governor.charge(Resource::Work, work))
        .map_err(|reason| resource(source, field, value, reason))
}

/// Offline target metadata, never running-kernel or loaded-policy attestation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivilegeRequirement {
    /// Selected policy ABI.
    pub abi: &'static str,
    /// Required native mediation feature class: caps or mount.
    pub mediation: &'static str,
}

const CAPABILITY_REQUIREMENT: PrivilegeRequirement = PrivilegeRequirement {
    abi: "3.0",
    mediation: "caps",
};
const MOUNT_REQUIREMENT: PrivilegeRequirement = PrivilegeRequirement {
    abi: "3.0",
    mediation: "mount",
};

/// One caller-owned named capability request, validated during governed rendering.
///
/// Only exact single names are admitted; no wildcard or empty all-capabilities
/// form exists here. Normative dangerous names may be denied but never granted.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{CapabilityRule, PrivilegeError};
/// let mut g = Governor::new(Budget::default());
/// let mut rule = CapabilityRule { source: "caller:raw", name: "net_raw", audit: true, deny: false };
/// assert_eq!(rule.render(&mut g).unwrap().text, "audit capability net_raw,\n");
/// rule.name = "sys_admin";
/// assert!(matches!(rule.render(&mut g), Err(PrivilegeError::DangerousGrant { name: "sys_admin", .. })));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct CapabilityRule<'a> {
    /// Caller rule identity retained without copying.
    pub source: &'a str,
    /// Exact original single capability name.
    pub name: &'a str,
    /// Emit the independent audit qualifier.
    pub audit: bool,
    /// Emit an explicit denial rather than a safe named grant.
    pub deny: bool,
}

/// A complete named capability fragment and its original field identity.
#[derive(Debug)]
pub struct CapabilityFragment<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Original capability field, not a substituted native compatibility name.
    pub name: &'a str,
    /// Complete comma/newline-terminated native rule.
    pub text: String,
    /// Required offline target, without host authority.
    pub requirement: PrivilegeRequirement,
}

impl<'a> CapabilityRule<'a> {
    /// Validate the exact ABI table and dangerous-grant rule, then emit atomically.
    ///
    /// Both source and field are admitted under this governor on every call.
    /// The fixed lookup bound covers forty names, one unsupported spelling and
    /// fourteen dangerous names. Borrowed preparation is caller-owned; emission
    /// uses the existing fallible governed writer and checks final cancellation.
    ///
    /// # Errors
    /// Returns original source/name with unknown, unsupported-target or dangerous
    /// grant refusals, or the unchanged governor outcome. No partial fragment is
    /// returned. The type example covers a safe grant and dangerous refusal.
    pub fn render(
        self,
        governor: &mut Governor,
    ) -> Result<CapabilityFragment<'a>, PrivilegeError<'a>> {
        admit(self.source, "source", self.source, 1, governor)?;
        admit(self.source, "capability", self.name, 55, governor)?;
        if self.name == "checkpoint_restore" {
            return Err(PrivilegeError::UnsupportedCapability {
                source: self.source,
                name: self.name,
                abi: "3.0",
            });
        }
        if !CAPABILITIES.contains(&self.name) {
            return Err(PrivilegeError::UnknownCapability {
                source: self.source,
                name: self.name,
            });
        }
        if !self.deny && DANGEROUS.contains(&self.name) {
            return Err(PrivilegeError::DangerousGrant {
                source: self.source,
                name: self.name,
            });
        }
        let failure = |reason| resource(self.source, "capability", self.name, reason);
        let text = format_output_governed(
            format_args!(
                "{}{}capability {},\n",
                if self.audit { "audit " } else { "" },
                if self.deny { "deny " } else { "" },
                self.name
            ),
            governor,
        )
        .map_err(failure)?;
        governor.check().map_err(failure)?;
        Ok(CapabilityFragment {
            source: self.source,
            name: self.name,
            text,
            requirement: CAPABILITY_REQUIREMENT,
        })
    }
}

/// Complete normative capability-denial baseline with an empty allow set.
#[derive(Debug)]
pub struct CapabilityBaseline<'a> {
    /// Original caller identity for this whole baseline request.
    pub source: &'a str,
    /// All fourteen explicit denials in the documented normative order.
    pub text: String,
    /// Required offline target, without graph-composition or host authority.
    pub requirement: PrivilegeRequirement,
}

struct DangerousDenials {
    audit: bool,
}
impl fmt::Display for DangerousDenials {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        for name in DANGEROUS {
            if self.audit {
                output.write_str("audit ")?;
            }
            output.write_str("deny capability ")?;
            output.write_str(name)?;
            output.write_str(",\n")?;
        }
        Ok(())
    }
}

/// Emit the complete dangerous-capability deny baseline, with no capability grants.
///
/// One bounded formatter pass traverses the fourteen static names; traversal is
/// admitted before the shared sink admits every emitted byte and buffer growth.
/// Caller-owned graph-wide composition is outside this standalone operation.
///
/// # Errors
/// Returns original source identity and exact input/work/storage/cancellation
/// outcome, without a partial baseline. Final cancellation is checked before
/// publishing the complete artifact.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::dangerous_capability_denials;
/// let baseline = dangerous_capability_denials("caller:baseline", false,
///     &mut Governor::new(Budget::default())).unwrap();
/// assert!(baseline.text.starts_with("deny capability sys_admin,\n"));
/// assert!(baseline.text.ends_with("deny capability net_bind_service,\n"));
/// ```
pub fn dangerous_capability_denials<'a>(
    source: &'a str,
    audit: bool,
    governor: &mut Governor,
) -> Result<CapabilityBaseline<'a>, PrivilegeError<'a>> {
    admit(source, "source", source, 1, governor)?;
    let failure = |reason| resource(source, "source", source, reason);
    governor
        .charge(Resource::Work, size(DANGEROUS.len()))
        .map_err(failure)?;
    let text = format_output_governed(format_args!("{}", DangerousDenials { audit }), governor)
        .map_err(failure)?;
    governor.check().map_err(failure)?;
    Ok(CapabilityBaseline {
        source,
        text,
        requirement: CAPABILITY_REQUIREMENT,
    })
}

/// One exact unscoped mount request; only explicit denials can render.
///
/// No source path, mountpoint, option or transition field is accepted or dropped.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{MountRule, PrivilegeError};
/// let mut g = Governor::new(Budget::default());
/// let mut rule = MountRule { source: "caller:mount", operation: "remount", audit: false, deny: true };
/// assert_eq!(rule.render(&mut g).unwrap().text, "deny remount,\n");
/// rule.deny = false;
/// assert!(matches!(rule.render(&mut g), Err(PrivilegeError::MountAllow { operation: "remount", .. })));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct MountRule<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Exactly `mount`, `remount`, `umount` or `pivot_root`.
    pub operation: &'a str,
    /// Emit the independent audit qualifier.
    pub audit: bool,
    /// Must be true; a requested allow is an explicit typed refusal.
    pub deny: bool,
}

/// A complete unscoped denial and its original operation field.
#[derive(Debug)]
pub struct MountFragment<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Exact original operation text.
    pub operation: &'a str,
    /// Complete comma/newline-terminated native rule.
    pub text: String,
    /// Required offline target, without host authority.
    pub requirement: PrivilegeRequirement,
}
impl<'a> MountRule<'a> {
    /// Validate the four exact operations and explicit denial, then emit atomically.
    ///
    /// # Errors
    /// Refuses unknown operation text, requested allows, or exact current-governor
    /// input/work/storage/cancellation outcomes. Every refusal keeps original
    /// identities and returns no partial fragment. See the type's successful
    /// denial and requested-allow example.
    pub fn render(self, governor: &mut Governor) -> Result<MountFragment<'a>, PrivilegeError<'a>> {
        admit(self.source, "source", self.source, 1, governor)?;
        admit(self.source, "mountOperation", self.operation, 4, governor)?;
        if !MOUNT_OPERATIONS.contains(&self.operation) {
            return Err(PrivilegeError::UnknownMountOperation {
                source: self.source,
                operation: self.operation,
            });
        }
        if !self.deny {
            return Err(PrivilegeError::MountAllow {
                source: self.source,
                operation: self.operation,
            });
        }
        let failure = |reason| resource(self.source, "mountOperation", self.operation, reason);
        let text = format_output_governed(
            format_args!(
                "{}deny {},\n",
                if self.audit { "audit " } else { "" },
                self.operation
            ),
            governor,
        )
        .map_err(failure)?;
        governor.check().map_err(failure)?;
        Ok(MountFragment {
            source: self.source,
            operation: self.operation,
            text,
            requirement: MOUNT_REQUIREMENT,
        })
    }
}
