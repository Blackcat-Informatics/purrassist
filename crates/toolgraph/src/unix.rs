// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed Unix socket fragments using the existing [AppArmor](https://apparmor.net/) policy-word printer.

use core::fmt;

use purrassist_core::{
    Governor, ScratchString, StopReason,
    contract::{Resource, saturating_size as size},
};

use crate::{
    AppArmorGlob, GlobError, GlobTokenKind, LiteralPathBinding, apparmor_glob::emit, socket_access,
};

/// Condition whose original input was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnixField {
    /// Local socket address.
    LocalAddress,
    /// Peer socket address.
    PeerAddress,
    /// Peer profile label.
    PeerLabel,
}

/// Why a requested address cannot be emitted as an abstract Unix condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnixAddressProblem {
    /// A filesystem socket needs a file-access rule, not an abstract-address rule.
    Filesystem,
    /// The first source byte must be a literal `@`, independently of variables.
    MissingLiteralMarker,
    /// Autobind identity is outside this declared address surface; it is not widened.
    AutoUnsupported,
}

/// A Unix declaration or rendering refusal retaining original caller identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnixError<'a> {
    /// Unknown/empty access or socket type.
    Field {
        /// Stable field name.
        field: &'static str,
        /// Complete original field value.
        source: &'a str,
    },
    /// An address cannot be safely interpreted as the requested abstract condition.
    Address {
        /// Original caller rule identity.
        source: &'a str,
        /// Condition that failed.
        field: UnixField,
        /// Original admitted glob text.
        pattern: &'a str,
        /// Actionable address refusal.
        problem: UnixAddressProblem,
    },
    /// The shared [`AppArmorGlob`] binding/word authority refused a condition.
    Pattern {
        /// Original caller rule identity.
        source: &'a str,
        /// Condition that failed.
        field: UnixField,
        /// Original admitted glob text.
        pattern: &'a str,
        /// Exact original binding offsets or resource cause.
        reason: GlobError,
    },
    /// Local-only access cannot carry any peer address/label condition.
    PeerAccess {
        /// Original caller rule identity.
        source: &'a str,
        /// First incompatible canonical access.
        access: &'static str,
    },
    /// Input/work/depth/storage/cancellation admission failed; no artifact is returned.
    Resource {
        /// Original field value or caller rule identity.
        source: &'a str,
        /// Exact governor outcome.
        reason: StopReason,
    },
}
impl fmt::Display for UnixError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Address { pattern, problem: UnixAddressProblem::Filesystem, .. } => write!(f, "filesystem Unix socket {pattern:?} requires a file-access rule"),
            Self::Address { pattern, problem: UnixAddressProblem::MissingLiteralMarker, .. } => write!(f, "abstract Unix address {pattern:?} requires a literal initial @ before any variable; use the explicit anonymous form for addr=none"),
            Self::Address { problem: UnixAddressProblem::AutoUnsupported, .. } => f.write_str("Unix autobind identity is unsupported; declare an exact abstract or anonymous scope instead"),
            Self::PeerAccess { access, .. } => write!(f, "Unix peer conditions cannot accompany {access}; declare local-only accesses separately"),
            _ => write!(f, "Unix rule refused: {self:?}"),
        }
    }
}
impl core::error::Error for UnixError<'_> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Pattern { reason, .. } => Some(reason),
            Self::Resource { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

fn admit<'a>(source: &'a str, governor: &mut Governor) -> Result<(), UnixError<'a>> {
    let work = size(source.len())
        .checked_mul(64)
        .and_then(|work| work.checked_add(128))
        .ok_or(UnixError::Resource {
            source,
            reason: StopReason::BudgetExceeded(Resource::Work),
        })?;
    // This bound admits complete source scans and the twelve-name lookup table.
    governor
        .charge(Resource::InputBytes, size(source.len()))
        .and_then(|()| governor.charge(Resource::Work, work))
        .map_err(|reason| UnixError::Resource { source, reason })
}

/// Nonempty native Unix accesses, in shared canonical socket-access order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnixAccessSet(u16);
impl UnixAccessSet {
    /// Admit comma/ASCII-whitespace-separated create, bind, listen, accept,
    /// connect, shutdown, getattr, setattr, getopt, setopt, send and receive.
    /// Aliases `r`, `w`, `rw` mean receive, send, both; duplicates are deduplicated.
    /// See [`UnixRule`] for a successful rule and peer compatibility.
    ///
    /// # Errors
    /// Empty comma fields/sets and unknown names retain their complete source;
    /// input/work/cancellation refusal remains an exact resource error.
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, UnixError<'a>> {
        admit(source, governor)?;
        socket_access::parse(source)
            .map(Self)
            .map_err(|socket_access::InvalidAccess| UnixError::Field {
                field: "access",
                source,
            })
    }
}

/// Explicit socket type; unconstrained does not imply a guessed default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnixSocketType {
    /// No type condition was requested.
    Any,
    /// Connection-oriented byte stream.
    Stream,
    /// Connectionless datagrams.
    Datagram,
    /// Connection-oriented ordered packets.
    Seqpacket,
}
impl UnixSocketType {
    /// Admit `*`, `stream`, `dgram` or `seqpacket` without OS lookup.
    /// See [`UnixRule`] for rendering through the public API.
    ///
    /// # Errors
    /// Unknown types and resource failures retain the original field value.
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, UnixError<'a>> {
        admit(source, governor)?;
        match source {
            "*" => Ok(Self::Any),
            "stream" => Ok(Self::Stream),
            "dgram" => Ok(Self::Datagram),
            "seqpacket" => Ok(Self::Seqpacket),
            _ => Err(UnixError::Field {
                field: "socket_type",
                source,
            }),
        }
    }
    const fn word(self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            Self::Stream => Some("stream"),
            Self::Datagram => Some("dgram"),
            Self::Seqpacket => Some("seqpacket"),
        }
    }
}

/// One existing admitted AARE plus its complete safe literal-path bindings.
///
/// Parsing, original source spans, unsupported escapes and exact binding refusals
/// remain owned by [`AppArmorGlob`]. No word is reparsed or matched here. In
/// particular, file-rule [`crate::GlobLanguage`] normalization is not Unix matching.
#[derive(Clone, Copy, Debug)]
pub struct UnixPattern<'a> {
    /// Existing admitted glob interior, without surrounding policy quotes.
    pub glob: &'a AppArmorGlob,
    /// Exactly the referenced literal-path records; no duplicate/unused records.
    pub bindings: &'a [LiteralPathBinding<'a>],
}

/// Distinct Unix address scopes; filesystem sockets require file-access rules.
#[derive(Clone, Copy, Debug)]
pub enum UnixAddress<'a> {
    /// Explicitly unconstrained: anonymous and abstract addresses remain possible.
    Any,
    /// Only anonymous sockets, rendered as native `addr=none`.
    Anonymous,
    /// Abstract AARE with a literal initial `@`, before any variable reference.
    /// Marker-only `@` names the empty abstract address, not an anonymous socket.
    Abstract(UnixPattern<'a>),
}

/// Peer label scope, never inferred from a socket address or caller identity.
#[derive(Clone, Copy, Debug)]
pub enum UnixLabel<'a> {
    /// Explicitly unconstrained label.
    Any,
    /// Existing admitted bound policy word; caller owns label trust.
    Pattern(UnixPattern<'a>),
}

/// Independent peer address and profile-label conditions.
#[derive(Clone, Copy, Debug)]
pub struct UnixPeer<'a> {
    /// Exact requested address form, or explicit unconstrained scope.
    pub address: UnixAddress<'a>,
    /// Exact requested label pattern, or explicit unconstrained scope.
    pub label: UnixLabel<'a>,
}
impl UnixPeer<'_> {
    const fn constrained(self) -> bool {
        !matches!(self.address, UnixAddress::Any) || !matches!(self.label, UnixLabel::Any)
    }
}

/// Required fine-grained Unix target, without host or loaded-policy attestation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnixRequirement {
    /// Standalone feature ABI needed by this output.
    pub abi: &'static str,
    /// Required mediation; a coarse network rule is no substitute.
    pub mediation: &'static str,
}

/// Complete deterministic output from one standalone Unix declaration.
#[derive(Debug)]
pub struct UnixFragment<'a> {
    /// Original caller rule identity; no admitted-graph authority is implied.
    pub source: &'a str,
    /// One complete comma/newline-terminated policy rule.
    pub text: String,
    /// Required offline target, never actual host capability evidence.
    pub requirement: UnixRequirement,
}

/// Standalone Unix socket rule for the declared tool-graph field surface.
///
/// Any peer constraint permits only accept, connect, send and receive. The
/// eight local-only accesses are refused rather than silently removed. Protocol,
/// local-label and attr/opt rules are outside this surface. This is emission,
/// not graph admission, permission analysis or confinement.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{AppArmorGlob, LiteralPathBinding, UnixAccessSet,
///     UnixAddress, UnixLabel, UnixPattern, UnixPeer, UnixRule, UnixSocketType};
/// let mut g = Governor::new(Budget::default());
/// let address = AppArmorGlob::parse("@@{ROOT}/ipc", &["ROOT"], &mut g).unwrap();
/// let bindings = [LiteralPathBinding { name: "ROOT", path: "/run/example" }];
/// let rule = UnixRule {
///     source: "caller:ipc", accesses: UnixAccessSet::parse("send,receive", &mut g).unwrap(),
///     socket_type: UnixSocketType::Stream, audit: false, deny: false,
///     local: UnixAddress::Abstract(UnixPattern { glob: &address, bindings: &bindings }),
///     peer: UnixPeer { address: UnixAddress::Anonymous, label: UnixLabel::Any },
/// };
/// let output = rule.render(&mut g).unwrap();
/// assert_eq!(output.text, "unix (send, receive) type=stream addr=\"@/run/example/ipc\" peer=(addr=none),\n");
/// assert_eq!(output.requirement.mediation, "network/af_unix");
/// let mut invalid = rule;
/// invalid.accesses = UnixAccessSet::parse("bind", &mut g).unwrap();
/// assert!(matches!(invalid.render(&mut g), Err(purrassist_toolgraph::UnixError::PeerAccess { access: "bind", .. })));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct UnixRule<'a> {
    /// Borrowed caller rule identity, retained on artifacts and failures.
    pub source: &'a str,
    /// Nonempty validated access set.
    pub accesses: UnixAccessSet,
    /// Explicit type or explicit unconstrained selection.
    pub socket_type: UnixSocketType,
    /// Emit the audit qualifier.
    pub audit: bool,
    /// Emit the deny qualifier.
    pub deny: bool,
    /// Requested local address scope.
    pub local: UnixAddress<'a>,
    /// Requested peer address and label scopes.
    pub peer: UnixPeer<'a>,
}
impl<'a> UnixRule<'a> {
    /// Emit a complete rule atomically through the existing governed policy printer.
    ///
    /// Borrowed models/bindings are caller-owned. Address/peer checks precede
    /// output; the same existing bound-word operation owns all quoting and
    /// binding validation. One grammar nesting level and every emitted chunk
    /// are admitted; brace patterns are retained, never expanded.
    ///
    /// # Errors
    /// Semantic refusals retain rule/field/pattern identities. Binding errors
    /// retain original [`GlobError`] offsets; resource refusal can arise from
    /// this rule or a bound word. No partial successful artifact escapes. See
    /// the type example for successful output and a peer-access refusal.
    pub fn render(self, governor: &mut Governor) -> Result<UnixFragment<'a>, UnixError<'a>> {
        admit(self.source, governor)?;
        governor
            .charge(Resource::Depth, 1)
            .map_err(|reason| UnixError::Resource {
                source: self.source,
                reason,
            })?;
        let patterns = [
            (
                UnixField::LocalAddress,
                match self.local {
                    UnixAddress::Abstract(pattern) => Some(pattern),
                    _ => None,
                },
            ),
            (
                UnixField::PeerAddress,
                match self.peer.address {
                    UnixAddress::Abstract(pattern) => Some(pattern),
                    _ => None,
                },
            ),
            (
                UnixField::PeerLabel,
                match self.peer.label {
                    UnixLabel::Pattern(pattern) => Some(pattern),
                    UnixLabel::Any => None,
                },
            ),
        ];
        for (field, pattern) in patterns {
            if let Some(pattern) = pattern {
                governor
                    .charge(Resource::InputBytes, size(pattern.glob.source().len()))
                    .map_err(|reason| UnixError::Pattern {
                        source: self.source,
                        field,
                        pattern: pattern.glob.source(),
                        reason: GlobError::Resource(reason),
                    })?;
            }
        }
        validate_address(self.source, UnixField::LocalAddress, self.local)?;
        validate_address(self.source, UnixField::PeerAddress, self.peer.address)?;
        if self.peer.constrained()
            && let Some(access) = socket_access::incompatible_peer(self.accesses.0)
        {
            return Err(UnixError::PeerAccess {
                source: self.source,
                access,
            });
        }
        let mut output = ScratchString::new(governor);
        self.append(&mut output, governor)?;
        governor.check().map_err(|reason| UnixError::Resource {
            source: self.source,
            reason,
        })?;
        Ok(UnixFragment {
            source: self.source,
            text: output.into_string(),
            requirement: UnixRequirement {
                abi: "3.0",
                mediation: "network/af_unix",
            },
        })
    }

    fn append(
        self,
        output: &mut ScratchString,
        governor: &mut Governor,
    ) -> Result<(), UnixError<'a>> {
        let mut write = |text| {
            emit(output, text, governor).map_err(|reason| UnixError::Resource {
                source: self.source,
                reason,
            })
        };
        if self.audit {
            write("audit ")?;
        }
        if self.deny {
            write("deny ")?;
        }
        write("unix (")?;
        let mut separator = "";
        for access in socket_access::names(self.accesses.0) {
            write(separator)?;
            write(access)?;
            separator = ", ";
        }
        write(")")?;
        if let Some(socket_type) = self.socket_type.word() {
            write(" type=")?;
            write(socket_type)?;
        }
        append_address(
            self.source,
            UnixField::LocalAddress,
            self.local,
            " addr=",
            output,
            governor,
        )?;
        if self.peer.constrained() {
            emit(output, " peer=(", governor).map_err(|reason| UnixError::Resource {
                source: self.source,
                reason,
            })?;
            let address_prefix = if let UnixLabel::Pattern(pattern) = self.peer.label {
                emit(output, "label=", governor).map_err(|reason| UnixError::Resource {
                    source: self.source,
                    reason,
                })?;
                append_pattern(self.source, UnixField::PeerLabel, pattern, output, governor)?;
                " addr="
            } else {
                "addr="
            };
            append_address(
                self.source,
                UnixField::PeerAddress,
                self.peer.address,
                address_prefix,
                output,
                governor,
            )?;
            emit(output, ")", governor).map_err(|reason| UnixError::Resource {
                source: self.source,
                reason,
            })?;
        }
        emit(output, ",\n", governor).map_err(|reason| UnixError::Resource {
            source: self.source,
            reason,
        })?;
        Ok(())
    }
}

fn validate_address<'a>(
    source: &'a str,
    field: UnixField,
    address: UnixAddress<'a>,
) -> Result<(), UnixError<'a>> {
    let UnixAddress::Abstract(pattern) = address else {
        return Ok(());
    };
    let text = pattern.glob.source();
    let problem = if text == "auto" {
        UnixAddressProblem::AutoUnsupported
    } else if text.starts_with('/')
        || pattern
            .glob
            .tokens()
            .first()
            .is_some_and(|token| matches!(token.kind, GlobTokenKind::Byte(b'/')))
    {
        UnixAddressProblem::Filesystem
    } else if matches!(
        pattern.glob.tokens().first().map(|token| &token.kind),
        Some(GlobTokenKind::Literal)
    ) && text.starts_with('@')
    {
        return Ok(());
    } else {
        UnixAddressProblem::MissingLiteralMarker
    };
    Err(UnixError::Address {
        source,
        field,
        pattern: text,
        problem,
    })
}

fn append_address<'a>(
    source: &'a str,
    field: UnixField,
    address: UnixAddress<'a>,
    prefix: &str,
    output: &mut ScratchString,
    governor: &mut Governor,
) -> Result<(), UnixError<'a>> {
    if matches!(address, UnixAddress::Any) {
        return Ok(());
    }
    emit(output, prefix, governor).map_err(|reason| UnixError::Resource { source, reason })?;
    match address {
        UnixAddress::Any => {
            unreachable!("INVARIANT: explicit unconstrained address returned before emission")
        }
        UnixAddress::Anonymous => {
            emit(output, "none", governor).map_err(|reason| UnixError::Resource { source, reason })
        }
        UnixAddress::Abstract(pattern) => append_pattern(source, field, pattern, output, governor),
    }
}

fn append_pattern<'a>(
    source: &'a str,
    field: UnixField,
    pattern: UnixPattern<'a>,
    output: &mut ScratchString,
    governor: &mut Governor,
) -> Result<(), UnixError<'a>> {
    pattern
        .glob
        .append_bound_policy_word(pattern.bindings, output, governor)
        .map_err(|reason| UnixError::Pattern {
            source,
            field,
            pattern: pattern.glob.source(),
            reason,
        })
}
