// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Standalone signal and ptrace declarations over explicit native peer labels.

use crate::{AppArmorGlob, GlobError, LiteralPathBinding, apparmor_glob::emit};
use core::fmt;
use purrassist_core::{
    Governor, ScratchString, StopReason,
    contract::{Resource, saturating_size as size},
};

const SIGNAL_ACCESS: [&str; 2] = ["send", "receive"];
const PTRACE_ACCESS: [&str; 4] = ["trace", "read", "readby", "tracedby"];
// AppArmor's declared vocabulary, not ambient platform signal numbers.
const SIGNALS: [&str; 66] = [
    "hup", "int", "quit", "ill", "trap", "abrt", "bus", "fpe", "kill", "usr1", "segv", "usr2",
    "pipe", "alrm", "term", "stkflt", "chld", "cont", "stop", "stp", "ttin", "ttou", "urg", "xcpu",
    "xfsz", "vtalrm", "prof", "winch", "io", "pwr", "sys", "emt", "exists", "rtmin+0", "rtmin+1",
    "rtmin+2", "rtmin+3", "rtmin+4", "rtmin+5", "rtmin+6", "rtmin+7", "rtmin+8", "rtmin+9",
    "rtmin+10", "rtmin+11", "rtmin+12", "rtmin+13", "rtmin+14", "rtmin+15", "rtmin+16", "rtmin+17",
    "rtmin+18", "rtmin+19", "rtmin+20", "rtmin+21", "rtmin+22", "rtmin+23", "rtmin+24", "rtmin+25",
    "rtmin+26", "rtmin+27", "rtmin+28", "rtmin+29", "rtmin+30", "rtmin+31", "rtmin+32",
];

/// A closed-set or rendering refusal with unchanged original identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IpcError<'a> {
    /// An empty comma field/set or a word outside the frozen vocabulary.
    Field {
        /// Stable input field name.
        field: &'static str,
        /// Complete original field text.
        source: &'a str,
        /// Original unknown word, or None for an empty field.
        word: Option<&'a str>,
    },
    /// Original peer binding/source failure, without rewriting its offsets.
    Peer {
        /// Caller rule identity.
        source: &'a str,
        /// Original native peer pattern.
        pattern: &'a str,
        /// Original binding or governance refusal.
        reason: GlobError,
    },
    /// Exact input/work/storage/cancellation outcome; no partial artifact escapes.
    Resource {
        /// Original field text or caller rule identity.
        source: &'a str,
        /// Original governor outcome.
        reason: StopReason,
    },
}
impl fmt::Display for IpcError<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "IPC rule refused: {self:?}")
    }
}
impl core::error::Error for IpcError<'_> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Peer { reason, .. } => Some(reason),
            Self::Resource { reason, .. } => Some(reason),
            Self::Field { .. } => None,
        }
    }
}

fn admit<'a>(source: &'a str, comparisons: usize, g: &mut Governor) -> Result<(), IpcError<'a>> {
    // The actual closed lookup table plus splitting, trimming and RT digit scans.
    let work = size(source.len())
        .checked_mul(size(comparisons).saturating_add(8))
        .and_then(|work| work.checked_add(128))
        .ok_or(IpcError::Resource {
            source,
            reason: StopReason::BudgetExceeded(Resource::Work),
        })?;
    g.charge(Resource::InputBytes, size(source.len()))
        .and_then(|()| g.charge(Resource::Work, work))
        .map_err(|reason| IpcError::Resource { source, reason })
}

fn realtime(word: &str) -> Option<usize> {
    let digits = word.strip_prefix("rtmin+")?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let digits = digits.trim_start_matches('0');
    let value = if digits.is_empty() {
        0
    } else {
        if digits.len() > 2 {
            return None;
        }
        digits.parse::<u8>().ok()?
    };
    (value <= 32).then_some(33 + usize::from(value))
}

fn parse_set<'a>(
    source: &'a str,
    field: &'static str,
    names: &[&str],
    rt: bool,
    g: &mut Governor,
) -> Result<u128, IpcError<'a>> {
    admit(source, names.len(), g)?;
    let mut bits = 0;
    for segment in source.split(',') {
        if segment.trim_ascii().is_empty() {
            return Err(IpcError::Field {
                field,
                source,
                word: None,
            });
        }
        for word in segment.split_ascii_whitespace() {
            let index = names
                .iter()
                .position(|name| *name == word)
                .or_else(|| if rt { realtime(word) } else { None })
                .ok_or(IpcError::Field {
                    field,
                    source,
                    word: Some(word),
                })?;
            // Both fixed access tables and the 66-signal table fit u128.
            bits |= 1_u128 << index;
        }
    }
    Ok(bits)
}

/// Nonempty send/receive permissions, in that canonical order; aliases are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalAccessSet(u128);
impl SignalAccessSet {
    /// Parse comma/ASCII-whitespace lists; duplicates have ordinary set semantics.
    ///
    /// # Errors
    /// Empty fields/sets, unknown words and original governor refusals remain typed.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{SignalAccessSet, IpcError};
    /// let mut g = Governor::new(Budget::default());
    /// assert!(SignalAccessSet::parse("receive send send", &mut g).is_ok());
    /// assert!(matches!(SignalAccessSet::parse("rw", &mut g), Err(IpcError::Field { .. })));
    /// ```
    pub fn parse<'a>(source: &'a str, g: &mut Governor) -> Result<Self, IpcError<'a>> {
        parse_set(source, "signal.access", &SIGNAL_ACCESS, false, g).map(Self)
    }
}

/// Nonempty trace/read/readby/tracedby permissions, with no implicit reverse grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtraceAccessSet(u128);
impl PtraceAccessSet {
    /// Parse the four exact names in comma/ASCII-whitespace lists; deduplicate them.
    ///
    /// # Errors
    /// Empty fields/sets, native aliases outside this subset, unknown words and
    /// governance refusal are explicit. See [`PtraceRule`] for complete output.
    pub fn parse<'a>(source: &'a str, g: &mut Governor) -> Result<Self, IpcError<'a>> {
        parse_set(source, "ptrace.access", &PTRACE_ACCESS, false, g).map(Self)
    }
}

/// A nonempty native signal set, retaining its original field text.
///
/// The named vocabulary is `hup int quit ill trap abrt bus fpe kill usr1 segv
/// usr2 pipe alrm term stkflt chld cont stop stp ttin ttou urg xcpu xfsz vtalrm
/// prof winch io pwr sys emt exists`.
/// Canonical order is this vocabulary then rtmin+0 through rtmin+32.
/// Leading-zero RT offsets are normalized without ambient signal lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalSet<'a> {
    source: &'a str,
    bits: u128,
}
impl<'a> SignalSet<'a> {
    /// Parse the closed 33-name table plus inclusive RT offsets 0..=32.
    ///
    /// # Errors
    /// Refuses unsupported names (including tstp/lost), numeric OS values,
    /// rtmax, malformed/out-of-range RT offsets, empty fields and governance.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor};
    /// use purrassist_toolgraph::{SignalSet, IpcError};
    /// let mut g = Governor::new(Budget::default());
    /// assert!(SignalSet::parse("term int rtmin+00032", &mut g).is_ok());
    /// assert!(matches!(SignalSet::parse("rtmin+33", &mut g), Err(IpcError::Field { .. })));
    /// ```
    pub fn parse(source: &'a str, g: &mut Governor) -> Result<Self, IpcError<'a>> {
        parse_set(source, "signal.set", &SIGNALS, true, g).map(|bits| Self { source, bits })
    }
    /// Borrow the exact original field, including order, duplicates and RT spelling.
    #[must_use]
    pub const fn source(self) -> &'a str {
        self.source
    }
}

/// Selected native mediation requirement, never running-kernel or host authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcRequirement {
    /// Selected offline feature ABI.
    pub abi: &'static str,
    /// Exact native class: signal or ptrace.
    pub mediation: &'static str,
}

/// Complete deterministic output retaining the caller's source and peer identity.
#[derive(Debug)]
pub struct IpcFragment<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Original unexpanded native peer pattern.
    pub peer_source: &'a str,
    /// Original exact safe literal bindings; no ambient lookup was performed.
    pub bindings: &'a [LiteralPathBinding<'a>],
    /// Original signal field where applicable; canonical output may reorder/deduplicate.
    pub signal_source: Option<&'a str>,
    /// Complete comma/newline-terminated native fragment.
    pub text: String,
    /// Selected ABI/class; both peers still need independently applicable permissions.
    pub requirement: IpcRequirement,
}

fn start<'a>(
    source: &'a str,
    peer: &AppArmorGlob,
    audit: bool,
    deny: bool,
    keyword: &str,
    g: &mut Governor,
) -> Result<ScratchString, IpcError<'a>> {
    admit(source, 0, g)?;
    g.charge(Resource::InputBytes, size(peer.source().len()))
        .map_err(|reason| IpcError::Resource { source, reason })?;
    let mut output = ScratchString::new(g);
    for text in [
        if audit { "audit " } else { "" },
        if deny { "deny " } else { "" },
        keyword,
        " ",
    ] {
        emit(&mut output, text, g).map_err(|reason| IpcError::Resource { source, reason })?;
    }
    Ok(output)
}
fn append_set(
    bits: u128,
    names: &[&str],
    output: &mut ScratchString,
    g: &mut Governor,
) -> Result<(), StopReason> {
    g.charge(Resource::Work, size(names.len()))?;
    emit(output, "(", g)?;
    let mut first = true;
    for (index, name) in names.iter().enumerate() {
        if bits & (1_u128 << index) != 0 {
            if !first {
                emit(output, ", ", g)?;
            }
            emit(output, name, g)?;
            first = false;
        }
    }
    emit(output, ")", g)
}
fn finish<'a>(
    source: &'a str,
    peer: &'a AppArmorGlob,
    bindings: &'a [LiteralPathBinding<'a>],
    mut output: ScratchString,
    g: &mut Governor,
) -> Result<String, IpcError<'a>> {
    emit(&mut output, " peer=", g).map_err(|reason| IpcError::Resource { source, reason })?;
    peer.append_bound_policy_word(bindings, &mut output, g)
        .map_err(|reason| IpcError::Peer {
            source,
            pattern: peer.source(),
            reason,
        })?;
    emit(&mut output, ",\n", g).map_err(|reason| IpcError::Resource { source, reason })?;
    g.check()
        .map_err(|reason| IpcError::Resource { source, reason })?;
    Ok(output.into_string())
}

/// Standalone signal policy with explicit nonempty access, signal set and peer.
///
/// Peer labels use native AARE semantics, without filename normalization or Unix
/// address markers. Both sender and receiver profiles need applicable native
/// permissions; this one fragment does not authenticate or configure either host.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{AppArmorGlob, SignalAccessSet, SignalSet, SignalRule};
/// let mut g = Governor::new(Budget::default());
/// let peer = AppArmorGlob::parse("purrassist.peer", &[], &mut g).unwrap();
/// let rule = SignalRule { source: "caller:signal", peer: &peer, bindings: &[], audit: false, deny: false,
///     access: SignalAccessSet::parse("receive", &mut g).unwrap(), signals: SignalSet::parse("term,int", &mut g).unwrap() };
/// assert_eq!(rule.render(&mut g).unwrap().text, "signal (receive) set=(int, term) peer=\"purrassist.peer\",\n");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct SignalRule<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Explicit native peer-label expression; an explicit ** is broad by request.
    pub peer: &'a AppArmorGlob,
    /// Exact declared literal bindings.
    pub bindings: &'a [LiteralPathBinding<'a>],
    /// Validated nonempty access set.
    pub access: SignalAccessSet,
    /// Validated signal membership plus unchanged original field text.
    pub signals: SignalSet<'a>,
    /// Emit audit independently of deny.
    pub audit: bool,
    /// Emit denial; this is not a host admission check.
    pub deny: bool,
}
impl<'a> SignalRule<'a> {
    /// Produce one complete fragment through the shared bound-word writer.
    ///
    /// # Errors
    /// Current-governor source/peer/signal input, work/storage/cancellation and
    /// original binding/source refusals return their exact causes, with no partial
    /// artifact. Original source preparation remains a separate caller operation.
    pub fn render(self, g: &mut Governor) -> Result<IpcFragment<'a>, IpcError<'a>> {
        g.charge(Resource::InputBytes, size(self.signals.source.len()))
            .map_err(|reason| IpcError::Resource {
                source: self.source,
                reason,
            })?;
        let mut output = start(self.source, self.peer, self.audit, self.deny, "signal", g)?;
        append_set(self.access.0, &SIGNAL_ACCESS, &mut output, g).map_err(|reason| {
            IpcError::Resource {
                source: self.source,
                reason,
            }
        })?;
        emit(&mut output, " set=", g).map_err(|reason| IpcError::Resource {
            source: self.source,
            reason,
        })?;
        append_set(self.signals.bits, &SIGNALS, &mut output, g).map_err(|reason| {
            IpcError::Resource {
                source: self.source,
                reason,
            }
        })?;
        let text = finish(self.source, self.peer, self.bindings, output, g)?;
        Ok(IpcFragment {
            source: self.source,
            peer_source: self.peer.source(),
            bindings: self.bindings,
            signal_source: Some(self.signals.source),
            text,
            requirement: IpcRequirement {
                abi: "3.0",
                mediation: "signal",
            },
        })
    }
}

/// Standalone ptrace policy with explicit access and mandatory peer-label scope.
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{AppArmorGlob, PtraceAccessSet, PtraceRule};
/// let mut g = Governor::new(Budget::default());
/// let peer = AppArmorGlob::parse("**", &[], &mut g).unwrap();
/// let rule = PtraceRule { source: "caller:ptrace", peer: &peer, bindings: &[], audit: false, deny: true,
///     access: PtraceAccessSet::parse("tracedby,read,trace,readby", &mut g).unwrap() };
/// assert_eq!(rule.render(&mut g).unwrap().text, "deny ptrace (trace, read, readby, tracedby) peer=\"**\",\n");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct PtraceRule<'a> {
    /// Original caller rule identity.
    pub source: &'a str,
    /// Explicit native peer-label expression, without inferred defaults.
    pub peer: &'a AppArmorGlob,
    /// Exact declared literal bindings.
    pub bindings: &'a [LiteralPathBinding<'a>],
    /// Validated nonempty access set; no reverse permission is implied.
    pub access: PtraceAccessSet,
    /// Emit audit independently of deny.
    pub audit: bool,
    /// Emit a deny rule without granting dispatch authority.
    pub deny: bool,
}
impl<'a> PtraceRule<'a> {
    /// Produce one complete fragment through the existing bound-word authority.
    ///
    /// # Errors
    /// Current-governor input, work/storage/cancellation and original binding
    /// failures retain their source identities and publish no partial artifact.
    pub fn render(self, g: &mut Governor) -> Result<IpcFragment<'a>, IpcError<'a>> {
        let mut output = start(self.source, self.peer, self.audit, self.deny, "ptrace", g)?;
        append_set(self.access.0, &PTRACE_ACCESS, &mut output, g).map_err(|reason| {
            IpcError::Resource {
                source: self.source,
                reason,
            }
        })?;
        let text = finish(self.source, self.peer, self.bindings, output, g)?;
        Ok(IpcFragment {
            source: self.source,
            peer_source: self.peer.source(),
            bindings: self.bindings,
            signal_source: None,
            text,
            requirement: IpcRequirement {
                abi: "3.0",
                mediation: "ptrace",
            },
        })
    }
}
