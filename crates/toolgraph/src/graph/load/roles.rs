// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The vocabulary's node roles and per-role property table, keyed by term name.
//!
//! This table is the loader's executable approximation of the structural shapes in
//! `docs/tool-graph/TOOLS.md`, not an equivalent of them: the four tool
//! annotation hints are optional here but required there, `filePermissions` has
//! no pattern check, integers are not distinguished by `xsd:positiveInteger`,
//! `xsd:anyURI` is not validated, and `FnO` parameter agreement and `sh:*`
//! constraint content are unchecked (remaining #33 work). It names only [`TERM_NAMES`](crate::TERM_NAMES)
//! identifiers, never IRIs; the caller's `Vocabulary` supplies those.

/// How a property value is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// An `xsd:string` literal.
    Str,
    /// A nonempty `xsd:string` literal.
    NonEmptyStr,
    /// A nonempty tool name of `[A-Za-z0-9_.:-]`.
    Name,
    /// An `xsd:boolean` literal.
    Bool,
    /// An integer literal that is positive and fits `u64`.
    Positive,
    /// An integer literal that is non-negative and fits `u64`.
    NonNegative,
    /// An IRI or blank node.
    Node,
    /// An IRI.
    Iri,
    /// An `xsd:anyURI` literal.
    AnyUri,
    /// An `xsd:string` literal from a closed set.
    Choice(&'static [&'static str]),
}

/// One property of a role: inclusive cardinality and value kind.
#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub(super) name: &'static str,
    pub(super) min: u8,
    pub(super) max: u8,
    pub(super) kind: Kind,
}

const MANY: u8 = u8::MAX;

const fn one(name: &'static str, kind: Kind) -> Spec {
    Spec {
        name,
        min: 1,
        max: 1,
        kind,
    }
}
const fn opt(name: &'static str, kind: Kind) -> Spec {
    Spec {
        name,
        min: 0,
        max: 1,
        kind,
    }
}
const fn any(name: &'static str, kind: Kind) -> Spec {
    Spec {
        name,
        min: 0,
        max: MANY,
        kind,
    }
}

const TOOL: &[Spec] = &[
    one("name", Kind::Name),
    one("description", Kind::Str),
    one("inputShape", Kind::Node),
    one("outputShape", Kind::Node),
    one("executionProfile", Kind::Node),
    one("contextManagement", Kind::Node),
    one("parameterConstraints", Kind::Str),
    opt("title", Kind::Str),
    opt("readOnlyHint", Kind::Bool),
    opt("destructiveHint", Kind::Bool),
    opt("idempotentHint", Kind::Bool),
    opt("openWorldHint", Kind::Bool),
    any("subtool", Kind::Node),
];
const PROFILE: &[Spec] = &[
    one("confinementMode", Kind::Choice(&["aa-exec", "change_hat"])),
    one("profileName", Kind::Str),
    one("hatName", Kind::Str),
    one("egressAllowed", Kind::Bool),
    one("addressSpaceBytes", Kind::Positive),
    one("fileSizeBytes", Kind::Positive),
    one("processCount", Kind::Positive),
    one("cpuSeconds", Kind::Positive),
    one("openFileCount", Kind::Positive),
    any("rule", Kind::Node),
    any("subprofile", Kind::Node),
    any("waiver", Kind::Node),
];
const WAIVER: &[Spec] = &[
    one("waivedRule", Kind::Node),
    one("reviewer", Kind::NonEmptyStr),
    one("rationale", Kind::NonEmptyStr),
    one("waiverSource", Kind::AnyUri),
];
const CONTEXT: &[Spec] = &[
    one("outputProjectionJq", Kind::Str),
    one("maxOutputBytes", Kind::Positive),
    one("historyRetentionTurns", Kind::NonNegative),
    any("intentTags", Kind::Str),
    one("loadingStrategy", Kind::Choice(&["always", "on_demand"])),
];
const HINT: &[Spec] = &[
    one("maxEditDistance", Kind::NonNegative),
    any("alias", Kind::Str),
    Spec {
        name: "typeCoercion",
        min: 1,
        max: MANY,
        kind: Kind::Choice(&["none", "string-to-integer", "integer-to-string"]),
    },
    one("caseFold", Kind::Bool),
    one("unicodeFold", Kind::Bool),
    any("taint", Kind::Str),
    opt("sessionBinding", Kind::Iri),
];
const FILE: &[Spec] = &[
    opt("audit", Kind::Bool),
    opt("deny", Kind::Bool),
    one("pathGlob", Kind::Str),
    opt("filePermissions", Kind::Str),
    opt(
        "execTransition",
        Kind::Choice(&["ix", "px", "cx", "ux", "Ux", "Pix", "Cix"]),
    ),
    opt("transitionTarget", Kind::Str),
    opt("owner", Kind::Bool),
];
const CAPABILITY: &[Spec] = &[
    opt("audit", Kind::Bool),
    opt("deny", Kind::Bool),
    one("capability", Kind::Str),
];
const NETWORK: &[Spec] = &[
    opt("audit", Kind::Bool),
    opt("deny", Kind::Bool),
    one("domain", Kind::Choice(&["inet", "inet6"])),
    one("access", Kind::Str),
    opt("socketType", Kind::Str),
    opt("protocol", Kind::Str),
    opt("localIp", Kind::Str),
    opt("localPort", Kind::Str),
    opt("peerIp", Kind::Str),
    opt("peerPort", Kind::Str),
];
const UNIX: &[Spec] = &[
    opt("audit", Kind::Bool),
    opt("deny", Kind::Bool),
    one("access", Kind::Str),
    opt("socketType", Kind::Str),
    opt("address", Kind::Str),
    opt("peerAddress", Kind::Str),
    opt("peerLabel", Kind::Str),
];
const SIGNAL: &[Spec] = &[
    opt("audit", Kind::Bool),
    opt("deny", Kind::Bool),
    one("access", Kind::Str),
    one("signalSet", Kind::Str),
    one("peerLabel", Kind::Str),
];
const PTRACE: &[Spec] = &[
    opt("audit", Kind::Bool),
    opt("deny", Kind::Bool),
    one("access", Kind::Str),
    one("peerLabel", Kind::Str),
];
const MOUNT: &[Spec] = &[
    opt("audit", Kind::Bool),
    one("deny", Kind::Bool),
    one(
        "mountOperation",
        Kind::Choice(&["mount", "remount", "umount", "pivot_root"]),
    ),
];

/// Node roles as bit masks; a node may hold several until concretised.
pub(super) struct Role;

impl Role {
    pub(super) const TOOL: u16 = 1;
    pub(super) const PROFILE: u16 = 1 << 1;
    pub(super) const CONTEXT: u16 = 1 << 2;
    pub(super) const HINT: u16 = 1 << 3;
    pub(super) const WAIVER: u16 = 1 << 4;
    pub(super) const FILE: u16 = 1 << 5;
    pub(super) const CAPABILITY: u16 = 1 << 6;
    pub(super) const NETWORK: u16 = 1 << 7;
    pub(super) const UNIX: u16 = 1 << 8;
    pub(super) const SIGNAL: u16 = 1 << 9;
    pub(super) const PTRACE: u16 = 1 << 10;
    pub(super) const MOUNT: u16 = 1 << 11;
    /// Marker: the node is a rule whose concrete class is not yet known.
    pub(super) const RULE_ANY: u16 = 1 << 12;
    /// Marker: the node is an argument or result shape.
    pub(super) const SHAPE: u16 = 1 << 13;
    pub(super) const RULES: u16 = Self::FILE
        | Self::CAPABILITY
        | Self::NETWORK
        | Self::UNIX
        | Self::SIGNAL
        | Self::PTRACE
        | Self::MOUNT;

    /// Runtime call, session and decision facts a static tool graph never asserts.
    pub(super) fn is_runtime(name: &str) -> bool {
        matches!(
            name,
            "Call"
                | "Session"
                | "session"
                | "tool"
                | "userId"
                | "tenant"
                | "active"
                | "customerId"
                | "candidateAllowed"
                | "candidateDenied"
                | "permits"
                | "denies"
                | "conflict"
        )
    }

    pub(super) fn of_class(class: &str) -> u16 {
        match class {
            "Tool" => Self::TOOL,
            "ExecutionProfile" => Self::PROFILE,
            "Context" => Self::CONTEXT,
            "Hint" => Self::HINT,
            "Waiver" => Self::WAIVER,
            "FileRule" => Self::FILE,
            "CapabilityRule" => Self::CAPABILITY,
            "NetworkRule" => Self::NETWORK,
            "UnixRule" => Self::UNIX,
            "SignalRule" => Self::SIGNAL,
            "PtraceRule" => Self::PTRACE,
            "MountRule" => Self::MOUNT,
            "Rule" => Self::RULE_ANY,
            _ => 0,
        }
    }

    /// The roles a predicate is declared on.
    pub(super) fn domain(name: &str) -> u16 {
        match name {
            "name"
            | "description"
            | "inputShape"
            | "outputShape"
            | "title"
            | "readOnlyHint"
            | "destructiveHint"
            | "idempotentHint"
            | "openWorldHint"
            | "subtool"
            | "executionProfile"
            | "contextManagement"
            | "parameterConstraints" => Self::TOOL,
            "waiver" | "confinementMode" | "profileName" | "hatName" | "egressAllowed" | "rule"
            | "addressSpaceBytes" | "fileSizeBytes" | "processCount" | "cpuSeconds"
            | "openFileCount" | "subprofile" => Self::PROFILE,
            "waivedRule" | "reviewer" | "rationale" | "waiverSource" => Self::WAIVER,
            "maxEditDistance" | "alias" | "typeCoercion" | "caseFold" | "unicodeFold" | "taint"
            | "sessionBinding" => Self::HINT,
            "outputProjectionJq"
            | "maxOutputBytes"
            | "historyRetentionTurns"
            | "intentTags"
            | "loadingStrategy" => Self::CONTEXT,
            "pathGlob" | "filePermissions" | "execTransition" | "transitionTarget" | "owner" => {
                Self::FILE
            }
            "capability" => Self::CAPABILITY,
            "domain" | "protocol" | "localIp" | "localPort" | "peerIp" | "peerPort" => {
                Self::NETWORK
            }
            "address" | "peerAddress" => Self::UNIX,
            "signalSet" => Self::SIGNAL,
            "mountOperation" => Self::MOUNT,
            "audit" | "deny" | "socketType" | "access" | "peerLabel" => Self::RULES,
            _ => 0,
        }
    }

    /// The role a predicate's object receives from the link.
    pub(super) fn link(name: &str) -> u16 {
        match name {
            "inputShape" | "outputShape" => Self::SHAPE,
            "executionProfile" | "subprofile" => Self::PROFILE,
            "contextManagement" => Self::CONTEXT,
            "waiver" => Self::WAIVER,
            "waivedRule" => Self::FILE,
            "rule" => Self::RULE_ANY,
            "subtool" => Self::TOOL,
            _ => 0,
        }
    }

    /// The lowest set role bit.
    pub(super) const fn first(mask: u16) -> u16 {
        mask.isolate_lowest_one()
    }

    /// Each concrete role bit of `mask`.
    pub(super) fn bits(mask: u16) -> impl Iterator<Item = u16> {
        (0..12).map(|at| 1 << at).filter(move |bit| mask & bit != 0)
    }

    /// The vocabulary class name of a single concrete role bit.
    pub(super) const fn label(role: u16) -> &'static str {
        match role {
            Self::TOOL => "Tool",
            Self::PROFILE => "ExecutionProfile",
            Self::CONTEXT => "Context",
            Self::HINT => "Hint",
            Self::WAIVER => "Waiver",
            Self::FILE => "FileRule",
            Self::CAPABILITY => "CapabilityRule",
            Self::NETWORK => "NetworkRule",
            Self::UNIX => "UnixRule",
            Self::SIGNAL => "SignalRule",
            Self::PTRACE => "PtraceRule",
            Self::MOUNT => "MountRule",
            _ => "Node",
        }
    }

    pub(super) const fn label_class(role: u16) -> &'static str {
        Self::label(role)
    }
}

impl Spec {
    pub(super) const fn table(role: u16) -> &'static [Self] {
        match role {
            Role::TOOL => TOOL,
            Role::PROFILE => PROFILE,
            Role::CONTEXT => CONTEXT,
            Role::HINT => HINT,
            Role::WAIVER => WAIVER,
            Role::FILE => FILE,
            Role::CAPABILITY => CAPABILITY,
            Role::NETWORK => NETWORK,
            Role::UNIX => UNIX,
            Role::SIGNAL => SIGNAL,
            Role::PTRACE => PTRACE,
            Role::MOUNT => MOUNT,
            _ => &[],
        }
    }
}
