// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Standalone Internet rule fragments, without graph admission or host authority.

use core::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use purrassist_core::{
    Governor, StopReason,
    contract::{Resource, format_output_governed, saturating_size as size},
};

/// Endpoint whose declaration failed admission or exceeded an enumeration ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternetSide {
    /// Local socket conditions.
    Local,
    /// Remote socket conditions.
    Peer,
}

/// An Internet declaration or rendering refusal with borrowed source identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InternetError<'a> {
    /// A field value outside the documented admitted table, or malformed ports.
    Field {
        /// Field's stable name.
        field: &'static str,
        /// Complete original field text.
        source: &'a str,
    },
    /// Not a numeric IP, numeric CIDR, or explicit `*`; no lookup was attempted.
    Scope {
        /// Complete original scope.
        source: &'a str,
    },
    /// An inclusive range cannot end before it starts.
    ReversedPorts {
        /// Original port text, or rule identity for a directly constructed range.
        source: &'a str,
        /// First port.
        first: u16,
        /// Last port.
        last: u16,
    },
    /// The scope has a different family from its rule.
    Family {
        /// Caller rule identity.
        source: &'a str,
        /// Endpoint identity.
        side: InternetSide,
        /// Original scope text.
        scope: &'a str,
    },
    /// The documented ICMP protocol selector applies to IPv4 only.
    SelectorFamily {
        /// Original caller rule identity.
        source: &'a str,
        /// Requested family.
        family: InternetFamily,
        /// Incompatible selector.
        selector: InternetSelector,
    },
    /// A local-only access cannot carry peer conditions.
    PeerAccess {
        /// Caller rule identity.
        source: &'a str,
        /// Incompatible canonical access.
        access: &'static str,
    },
    /// Exact endpoint cardinality exceeds its caller ceiling. Split the CIDR or raise the ceiling.
    Addresses {
        /// Caller rule identity.
        source: &'a str,
        /// Endpoint identity.
        side: InternetSide,
        /// Original scope text.
        scope: &'a str,
        /// Exact required count is two to this power, including 128.
        required_power: u8,
        /// Caller address ceiling.
        limit: u64,
    },
    /// The exact complete local-by-peer product exceeds the caller ceiling.
    Rules {
        /// Caller rule identity.
        source: &'a str,
        /// Exact required count; two `u64` counts fit in `u128`.
        required: u128,
        /// Caller rule ceiling.
        limit: u64,
    },
    /// Input/work/storage/cancellation admission failed; no fragment is returned.
    Resource {
        /// Original field text or caller rule identity.
        source: &'a str,
        /// Exact governor outcome.
        reason: StopReason,
    },
}
impl fmt::Display for InternetError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Addresses {
                scope,
                required_power,
                limit,
                ..
            } => write!(
                f,
                "scope {scope:?} needs 2^{required_power} addresses, exceeding ceiling {limit}; split the CIDR or raise a representable ceiling"
            ),
            Self::Rules {
                required, limit, ..
            } => write!(
                f,
                "Internet endpoint product needs {required} rules, exceeding ceiling {limit}; narrow the scopes or raise a representable ceiling"
            ),
            Self::Scope { source } => write!(
                f,
                "unsupported Internet scope {source:?}; supply a numeric IP, numeric CIDR or explicit *"
            ),
            Self::PeerAccess { access, .. } => write!(
                f,
                "peer conditions are incompatible with {access}; declare local-only access separately"
            ),
            _ => write!(f, "Internet rule refused: {self:?}"),
        }
    }
}
impl std::error::Error for InternetError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resource { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

fn admit<'a>(source: &'a str, governor: &mut Governor) -> Result<(), InternetError<'a>> {
    let work = size(source.len())
        .checked_mul(64)
        .and_then(|work| work.checked_add(128))
        .ok_or(InternetError::Resource {
            source,
            reason: StopReason::BudgetExceeded(Resource::Work),
        })?;
    // Admit all scans, bounded keyword comparisons and standard numeric parsing
    // before computation; no allocation or ambient resolver participates.
    governor
        .charge(Resource::InputBytes, size(source.len()))
        .and_then(|()| governor.charge(Resource::Work, work))
        .map_err(|reason| InternetError::Resource { source, reason })
}

/// Numeric address family, independent of host capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternetFamily {
    /// IPv4 (`inet`).
    Inet,
    /// IPv6 (`inet6`).
    Inet6,
}
impl InternetFamily {
    /// Admit `inet` or `inet6`. See [`InternetRule`] for a complete consumer.
    ///
    /// # Errors
    /// Unknown text retains its original identity; governance errors are explicit.
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, InternetError<'a>> {
        admit(source, governor)?;
        match source {
            "inet" => Ok(Self::Inet),
            "inet6" => Ok(Self::Inet6),
            _ => Err(InternetError::Field {
                field: "family",
                source,
            }),
        }
    }
    const fn word(self) -> &'static str {
        match self {
            Self::Inet => "inet",
            Self::Inet6 => "inet6",
        }
    }
    const fn agrees(self, address: IpAddr) -> bool {
        matches!(
            (self, address),
            (Self::Inet, IpAddr::V4(_)) | (Self::Inet6, IpAddr::V6(_))
        )
    }
}

/// Exactly one native socket type or protocol, from the documented admitted table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternetSelector {
    /// Socket type `stream`.
    Stream,
    /// Socket type `dgram`.
    Datagram,
    /// Socket type `seqpacket`.
    Seqpacket,
    /// Socket type `rdm`.
    Rdm,
    /// Socket type `raw`.
    Raw,
    /// Socket type `packet`.
    Packet,
    /// Protocol `tcp`.
    Tcp,
    /// Protocol `udp`.
    Udp,
    /// IPv4 protocol `icmp`; rendering with `inet6` is refused.
    Icmp,
}
impl InternetSelector {
    /// Admit one documented type/protocol name, without OS protocol lookup.
    /// See [`InternetRule`] for rendering.
    ///
    /// # Errors
    /// Unknown values and resource refusal retain original field text.
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, InternetError<'a>> {
        admit(source, governor)?;
        match source {
            "stream" => Ok(Self::Stream),
            "dgram" => Ok(Self::Datagram),
            "seqpacket" => Ok(Self::Seqpacket),
            "rdm" => Ok(Self::Rdm),
            "raw" => Ok(Self::Raw),
            "packet" => Ok(Self::Packet),
            "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            "icmp" => Ok(Self::Icmp),
            _ => Err(InternetError::Field {
                field: "selector",
                source,
            }),
        }
    }
    const fn word(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::Datagram => "dgram",
            Self::Seqpacket => "seqpacket",
            Self::Rdm => "rdm",
            Self::Raw => "raw",
            Self::Packet => "packet",
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Icmp => "icmp",
        }
    }
}

const ACCESSES: [&str; 12] = [
    "create", "bind", "listen", "accept", "connect", "shutdown", "getattr", "setattr", "getopt",
    "setopt", "send", "receive",
];
const PEER_ACCESSES: u16 = (1 << 3) | (1 << 4) | (1 << 10) | (1 << 11);

/// Admitted nonempty access set; duplicates and aliases have one canonical order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternetAccessSet(u16);
impl InternetAccessSet {
    /// Read comma/ASCII-whitespace-separated native accesses. `r` means receive,
    /// `w` send, and `rw` both. The canonical names are create, bind, listen,
    /// accept, connect, shutdown, getattr, setattr, getopt, setopt, send, receive.
    ///
    /// # Errors
    /// Empty sets, empty comma fields, unknown names and governance fail explicitly.
    /// See [`InternetRule`] for a successful use and peer compatibility semantics.
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, InternetError<'a>> {
        admit(source, governor)?;
        let invalid = || InternetError::Field {
            field: "access",
            source,
        };
        let mut bits = 0;
        for segment in source.split(',') {
            if segment.trim_ascii().is_empty() {
                return Err(invalid());
            }
            for word in segment.split_ascii_whitespace() {
                let value = match word {
                    "r" => 1 << 11,
                    "w" => 1 << 10,
                    "rw" => (1 << 10) | (1 << 11),
                    _ => {
                        let index = ACCESSES
                            .iter()
                            .position(|candidate| *candidate == word)
                            .ok_or_else(invalid)?;
                        1 << index
                    }
                };
                bits |= value;
            }
        }
        Ok(Self(bits))
    }
}

/// Inclusive numeric port bounds, or explicitly unconstrained ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternetPorts {
    /// No port condition is requested.
    Any,
    /// All ports from `first` through `last`, including both endpoints.
    Range {
        /// First port.
        first: u16,
        /// Last port.
        last: u16,
    },
}
impl InternetPorts {
    /// Parse `*`, one decimal `u16`, or `first-last`. No service lookup occurs.
    ///
    /// # Errors
    /// Invalid/out-of-range text retains its source; reversed ranges are refused.
    /// Reversed directly constructed variants are also refused by rendering.
    pub fn parse<'a>(source: &'a str, governor: &mut Governor) -> Result<Self, InternetError<'a>> {
        admit(source, governor)?;
        if source == "*" {
            return Ok(Self::Any);
        }
        let invalid = || InternetError::Field {
            field: "ports",
            source,
        };
        let (first, last) = source.split_once('-').unwrap_or((source, source));
        if first.is_empty()
            || last.is_empty()
            || !first.bytes().all(|byte| byte.is_ascii_digit())
            || !last.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid());
        }
        let first = first.parse::<u16>().map_err(|_| invalid())?;
        let last = last.parse::<u16>().map_err(|_| invalid())?;
        if first > last {
            return Err(InternetError::ReversedPorts {
                source,
                first,
                last,
            });
        }
        Ok(Self::Range { first, last })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScopeKind {
    Any,
    Numeric { first: IpAddr, host_bits: u8 },
}

/// Caller scope, retaining original text and a symbolic CIDR until rendering.
///
/// `*` is explicitly unconstrained. Numeric CIDRs include every mathematical
/// address, including IPv4 network/broadcast addresses; host bits are masked.
/// No DNS, interface zones or other ambient resolution is attempted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternetScope<'a> {
    source: &'a str,
    kind: ScopeKind,
}
impl<'a> InternetScope<'a> {
    /// Admit `*`, an exact numeric IP, or numeric `IP/prefix` symbolically.
    ///
    /// # Errors
    /// Unsupported/malformed scope text and invalid prefixes retain exact input.
    /// Enumeration ceilings apply only when [`InternetRule::render`] is called.
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::InternetScope;
    /// let mut g = Governor::new(Budget::default());
    /// assert!(InternetScope::parse("2001:db8::/128", &mut g).is_ok());
    /// assert!(InternetScope::parse("database.example", &mut g).is_err());
    /// ```
    pub fn parse(source: &'a str, governor: &mut Governor) -> Result<Self, InternetError<'a>> {
        admit(source, governor)?;
        if source == "*" {
            return Ok(Self {
                source,
                kind: ScopeKind::Any,
            });
        }
        let invalid = || InternetError::Scope { source };
        let (ip, prefix) = source
            .split_once('/')
            .map_or((source, None), |(ip, prefix)| (ip, Some(prefix)));
        let address = ip.parse::<IpAddr>().map_err(|_| invalid())?;
        let width = if address.is_ipv4() { 32_u8 } else { 128 };
        let prefix = if let Some(prefix) = prefix {
            if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            prefix.parse::<u8>().map_err(|_| invalid())?
        } else {
            width
        };
        if prefix > width {
            return Err(invalid());
        }
        let host_bits = width - prefix;
        let mask = if prefix == 0 {
            0
        } else {
            u128::MAX << host_bits
        };
        let first = match address {
            IpAddr::V4(ip) => IpAddr::V4(Ipv4Addr::from(
                (u32::from(ip)
                    & u32::try_from(mask & u128::from(u32::MAX)).map_err(|_| invalid())?)
                .to_be_bytes(),
            )),
            IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from(u128::from(ip) & mask)),
        };
        Ok(Self {
            source,
            kind: ScopeKind::Numeric { first, host_bits },
        })
    }
    /// Exact original scope; parsing never replaces it with normalized text.
    #[must_use]
    pub const fn source(self) -> &'a str {
        self.source
    }
}

/// Explicit endpoint conditions. Both `*` forms mean truly unconstrained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternetEndpoint<'a> {
    /// Exact numeric address, symbolic CIDR, or explicit unconstrained scope.
    pub scope: InternetScope<'a>,
    /// Inclusive port condition or explicit unconstrained ports.
    pub ports: InternetPorts,
}
impl InternetEndpoint<'_> {
    const fn constrained(self) -> bool {
        !matches!(self.scope.kind, ScopeKind::Any) || !matches!(self.ports, InternetPorts::Any)
    }
}

/// Explicit maximum cardinalities checked before output allocation or emission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternetLimits {
    /// Maximum addresses in each endpoint; an unconstrained endpoint counts as one.
    pub addresses_per_endpoint: u64,
    /// Maximum complete local-by-peer rule product.
    pub rules: u64,
}

/// Fine-grained Internet target requirement, never host or loaded-policy evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternetRequirement {
    /// Policy ABI name required by this fragment.
    pub abi: &'static str,
    /// Mediation capability required by this fragment.
    pub mediation: &'static str,
}

/// Complete deterministic output from one admitted standalone declaration.
#[derive(Debug)]
pub struct InternetFragment<'a> {
    /// Original caller rule identity, not a graph admission claim.
    pub source: &'a str,
    /// Complete fragment, ending every rule with comma and newline.
    pub text: String,
    /// Exact generated rule count.
    pub rules: u64,
    /// Required offline target capability; the host must separately attest enforcement.
    pub requirement: InternetRequirement,
}

/// A standalone Internet declaration, without graph admission or dispatch authority.
///
/// The access and selector order follows the native grammar. Peer conditions
/// require accept/connect/send/receive accesses exclusively. Unconstrained local
/// or peer endpoints omit only conditions explicitly declared as `*`.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{InternetAccessSet, InternetEndpoint, InternetFamily,
///     InternetLimits, InternetPorts, InternetRule, InternetScope, InternetSelector};
/// let mut g = Governor::new(Budget::default());
/// let any = InternetEndpoint { scope: InternetScope::parse("*", &mut g).unwrap(), ports: InternetPorts::Any };
/// let rule = InternetRule {
///     source: "caller:database", family: InternetFamily::Inet, selector: InternetSelector::Tcp,
///     accesses: InternetAccessSet::parse("connect", &mut g).unwrap(), audit: false, deny: false,
///     local: any, peer: InternetEndpoint { scope: InternetScope::parse("192.0.2.1", &mut g).unwrap(), ports: InternetPorts::Range { first: 5432, last: 5432 } },
/// };
/// let output = rule.render(InternetLimits { addresses_per_endpoint: 1, rules: 1 }, &mut g).unwrap();
/// assert_eq!(output.text, "network (connect) inet tcp peer=(ip=192.0.2.1 port=5432),\n");
/// assert_eq!(output.requirement.abi, "4.0-ip");
/// let mut broad = rule;
/// broad.peer.scope = InternetScope::parse("10.0.0.7/8", &mut g).unwrap();
/// assert!(matches!(broad.render(InternetLimits { addresses_per_endpoint: 1, rules: 1 }, &mut g),
///     Err(purrassist_toolgraph::InternetError::Addresses { scope: "10.0.0.7/8", required_power: 24, .. })));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct InternetRule<'a> {
    /// Caller-provided identity retained in artifacts and rule errors.
    pub source: &'a str,
    /// Explicit numeric address family.
    pub family: InternetFamily,
    /// One socket type or protocol, never both.
    pub selector: InternetSelector,
    /// Nonempty validated access set.
    pub accesses: InternetAccessSet,
    /// Emit the audit qualifier.
    pub audit: bool,
    /// Emit the deny qualifier.
    pub deny: bool,
    /// Explicit local endpoint conditions.
    pub local: InternetEndpoint<'a>,
    /// Explicit peer endpoint conditions; `*`/`Any` omits the peer clause.
    pub peer: InternetEndpoint<'a>,
}
impl<'a> InternetRule<'a> {
    /// Return the complete governed fragment atomically, never a partial rule list.
    ///
    /// CIDR counts and the complete Cartesian product are checked before output.
    /// Display computation is bounded and admitted separately from the shared
    /// formatter's per-output-byte work and fallible storage. No address vector
    /// or brace product is built; numeric local order precedes numeric peer order.
    ///
    /// # Errors
    /// Family, port and peer/access mismatches retain rule/scope identity.
    /// Excessive address counts report exact powers of two (including IPv6 `/0`);
    /// split the CIDR or raise a representable ceiling. Product limits report the
    /// exact count. Governance errors return no artifact. See the type example.
    pub fn render(
        self,
        limits: InternetLimits,
        governor: &mut Governor,
    ) -> Result<InternetFragment<'a>, InternetError<'a>> {
        admit(self.source, governor)?;
        if self.family == InternetFamily::Inet6 && self.selector == InternetSelector::Icmp {
            return Err(InternetError::SelectorFamily {
                source: self.source,
                family: self.family,
                selector: self.selector,
            });
        }
        let local = self.count(self.local, InternetSide::Local, limits, governor)?;
        let peer = self.count(self.peer, InternetSide::Peer, limits, governor)?;
        let required = u128::from(local) * u128::from(peer);
        if required > u128::from(limits.rules) {
            return Err(InternetError::Rules {
                source: self.source,
                required,
                limit: limits.rules,
            });
        }
        if self.peer.constrained() {
            for (index, access) in ACCESSES.iter().enumerate() {
                if self.accesses.0 & !PEER_ACCESSES & (1 << index) != 0 {
                    return Err(InternetError::PeerAccess {
                        source: self.source,
                        access,
                    });
                }
            }
        }
        let rules = u64::try_from(required).map_err(|_| InternetError::Rules {
            source: self.source,
            required,
            limit: limits.rules,
        })?;
        // Each row has <=12 access names, two <=39-byte standard IP displays,
        // four u16 displays, and bounded syntax. 4096 units conservatively admit
        // their loops/formatting before invoking Display; output bytes are charged
        // separately by the existing governed formatter, which checks cancellation.
        let work = rules.checked_mul(4096).ok_or(InternetError::Resource {
            source: self.source,
            reason: StopReason::BudgetExceeded(Resource::Work),
        })?;
        governor
            .charge(Resource::Work, work)
            .map_err(|reason| InternetError::Resource {
                source: self.source,
                reason,
            })?;
        let text = format_output_governed(
            format_args!(
                "{}",
                FragmentDisplay {
                    rule: self,
                    local,
                    peer
                }
            ),
            governor,
        )
        .map_err(|reason| InternetError::Resource {
            source: self.source,
            reason,
        })?;
        Ok(InternetFragment {
            source: self.source,
            text,
            rules,
            requirement: InternetRequirement {
                abi: "4.0-ip",
                mediation: "network_v8/af_inet",
            },
        })
    }

    fn count(
        self,
        endpoint: InternetEndpoint<'a>,
        side: InternetSide,
        limits: InternetLimits,
        governor: &mut Governor,
    ) -> Result<u64, InternetError<'a>> {
        governor
            .charge(Resource::Work, 1)
            .map_err(|reason| InternetError::Resource {
                source: self.source,
                reason,
            })?;
        if let InternetPorts::Range { first, last } = endpoint.ports
            && first > last
        {
            return Err(InternetError::ReversedPorts {
                source: self.source,
                first,
                last,
            });
        }
        let power = match endpoint.scope.kind {
            ScopeKind::Any => 0,
            ScopeKind::Numeric { first, host_bits } => {
                if !self.family.agrees(first) {
                    return Err(InternetError::Family {
                        source: self.source,
                        side,
                        scope: endpoint.scope.source,
                    });
                }
                host_bits
            }
        };
        match 1_u64.checked_shl(u32::from(power)) {
            Some(count) if count <= limits.addresses_per_endpoint => Ok(count),
            _ => Err(InternetError::Addresses {
                source: self.source,
                side,
                scope: endpoint.scope.source,
                required_power: power,
                limit: limits.addresses_per_endpoint,
            }),
        }
    }
}

struct FragmentDisplay<'a> {
    rule: InternetRule<'a>,
    local: u64,
    peer: u64,
}
impl fmt::Display for FragmentDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for local in 0..self.local {
            for peer in 0..self.peer {
                if self.rule.audit {
                    f.write_str("audit ")?;
                }
                if self.rule.deny {
                    f.write_str("deny ")?;
                }
                f.write_str("network (")?;
                let mut separator = "";
                for (index, access) in ACCESSES.iter().enumerate() {
                    if self.rule.accesses.0 & (1 << index) != 0 {
                        write!(f, "{separator}{access}")?;
                        separator = ", ";
                    }
                }
                write!(
                    f,
                    ") {} {}",
                    self.rule.family.word(),
                    self.rule.selector.word()
                )?;
                write_endpoint(f, self.rule.local, local, false)?;
                if self.rule.peer.constrained() {
                    f.write_str(" peer=(")?;
                    write_endpoint(f, self.rule.peer, peer, true)?;
                    f.write_str(")")?;
                }
                f.write_str(",\n")?;
            }
        }
        Ok(())
    }
}

fn write_endpoint(
    f: &mut fmt::Formatter<'_>,
    endpoint: InternetEndpoint<'_>,
    offset: u64,
    peer: bool,
) -> fmt::Result {
    let mut separator = if peer { "" } else { " " };
    if let ScopeKind::Numeric { first, .. } = endpoint.scope.kind {
        // A masked CIDR base plus an admitted offset cannot overflow its family.
        let address = match first {
            IpAddr::V4(ip) => IpAddr::V4(Ipv4Addr::from(
                u32::try_from(u128::from(u32::from(ip)) + u128::from(offset))
                    .map_err(|_| fmt::Error)?,
            )),
            IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from(
                u128::from(ip)
                    .checked_add(u128::from(offset))
                    .ok_or(fmt::Error)?,
            )),
        };
        write!(f, "{separator}ip={address}")?;
        separator = " ";
    }
    if let InternetPorts::Range { first, last } = endpoint.ports {
        write!(f, "{separator}port={first}")?;
        if first != last {
            write!(f, "-{last}")?;
        }
    }
    Ok(())
}
