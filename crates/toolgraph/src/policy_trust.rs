// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact caller-prepared SRL contracts and immutable host grant matching.
//!
//! This boundary interprets no SRL and authenticates no issuer. Native parsing,
//! declared-edge verification, protected-head admission and evaluation remain
//! separate obligations. In particular, a matched contract grants no dispatch.
//!
//! # Version one transcripts
//!
//! All integers are unsigned 64-bit big endian. A byte field is its length
//! followed by its exact bytes. SHA-256 fields are exactly 32 bytes. Hash input
//! length is checked against `2^61 - 1`; arithmetic overflow is a typed refusal.
//!
//! A rooted contract starts with `purrassist.policy-contract.v1\0`. Traverse
//! depth first in declared import order, assigning consecutive IDs from zero
//! at first encounter. A first encounter emits byte `1`, ID, location field,
//! external-base presence byte (zero or one, followed by a field only for one),
//! issuer field, original-text SHA-256, direct-reference count, then each target
//! recursively, then byte `3`. An already encountered target emits byte `2`
//! and its ID only. Thus repeated edges, sharing and cycles remain explicit,
//! traversal is finite, and table order has no effect. Each document has its
//! own rooted transcript, even when it participates in a cycle.
//!
//! A registry starts with `purrassist.policy-registry.v1\0`, then grant count.
//! Grants sort lexicographically by document SHA, contract SHA, tool bytes,
//! opaque vocabulary bytes, issuer bytes, session-schema SHA, provenance bytes.
//! Each emits document SHA, contract SHA, tool field, vocabulary's 32 opaque
//! bytes, issuer field, session-schema SHA and provenance field, in that order.
//! Duplicate authority tuples (all fields except provenance) are refused.

use std::{cmp::Ordering, fmt};

use purrassist_core::{
    Governor, ScratchCharge, ScratchVec, StopReason,
    contract::{Resource, Usage},
    digest::{Digest, DigestBuilder},
};

/// Original caller-supplied SRL source and declared ordered direct edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicySource<'a> {
    /// Exact document location, used as the reference identity.
    pub location: &'a str,
    /// External parse base; absence and an explicitly empty base differ.
    pub external_base: Option<&'a str>,
    /// Audited grant-source identity; this text is not authentication.
    pub issuer: &'a str,
    /// Original UTF-8 SRL bytes, never rewritten by contract preparation.
    pub text: &'a str,
    /// Locations of direct imports, preserving order and repeated occurrences.
    pub imports: &'a [&'a str],
}

/// Complete table rooted at one exact document location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyContractInput<'a> {
    /// Exact root location; must resolve to one table record.
    pub root: &'a str,
    /// Complete reachable table, without duplicate or unused records.
    pub documents: &'a [PolicySource<'a>],
}

/// Exact selected authority scope, supplied independently of policy text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyScope<'a> {
    /// One exact tool identity.
    pub tool: &'a str,
    /// Host-supplied opaque vocabulary identity, not a computed vocabulary hash.
    pub vocabulary: [u8; 32],
    /// SHA-256 of the trusted session-fact schema's exact bytes.
    pub session_schema: [u8; 32],
}

/// One original host-configured grant, never inferred from a graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyGrant<'a> {
    /// Exact original SRL document SHA-256.
    pub document: [u8; 32],
    /// Approved rooted ordered import-contract SHA-256.
    pub contract: [u8; 32],
    /// Exact tool, vocabulary and session-schema scope.
    pub scope: PolicyScope<'a>,
    /// Exact audited issuer identity; not proof of authentication by itself.
    pub issuer: &'a str,
    /// Original audit label, included in registry identity but not authority.
    pub provenance: &'a str,
}

/// Typed atomic refusal; indices address original caller-order records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyTrustCause {
    /// A required tool, issuer or document identity is empty.
    EmptyIdentity,
    /// Two original document records name the same location.
    DuplicateDocument(usize, usize),
    /// The root location does not name a supplied record.
    MissingRoot,
    /// A direct reference does not resolve; document and edge indices.
    MissingImport(usize, usize),
    /// A supplied record is outside the root's declared closure.
    UnreachedDocument(usize),
    /// Two grants have identical authority, regardless of provenance.
    DuplicateGrant(usize, usize),
    /// No exact authority grant matches this document and selected scope.
    UntrustedDocument(usize),
    /// Checked framing, index conversion or SHA-256 length overflow.
    LengthOverflow,
    /// Original sticky governor or allocation cause.
    Resource(StopReason),
}
impl From<StopReason> for PolicyTrustCause {
    fn from(value: StopReason) -> Self {
        Self::Resource(value)
    }
}

/// Original request and host records retained by a refusal.
#[derive(Clone, Copy, Debug)]
pub struct PolicyTrustContext<'a> {
    /// Original source table, when preparing or matching a contract.
    pub request: Option<PolicyContractInput<'a>>,
    /// Original registry records; empty during source-only preparation.
    pub grants: &'a [PolicyGrant<'a>],
    /// Selected exact scope, when matching.
    pub scope: Option<PolicyScope<'a>>,
    /// Prepared registry identity, when matching.
    pub registry: Option<Digest>,
}

/// Atomic refusal retaining originals, native cause and consumed usage.
#[derive(Clone, Debug)]
pub struct PolicyTrustError<'a> {
    /// Complete original context; no partial matched handle accompanies it.
    pub context: PolicyTrustContext<'a>,
    /// Exact validation or resource refusal.
    pub cause: PolicyTrustCause,
    /// Governor usage at refusal.
    pub usage: Usage,
}
impl fmt::Display for PolicyTrustError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "policy contract refused: {:?}", self.cause)
    }
}
impl std::error::Error for PolicyTrustError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            PolicyTrustCause::Resource(e) => Some(e),
            _ => None,
        }
    }
}

/// Immutable original host grants plus their canonical complete SHA-256.
#[derive(Debug)]
pub struct TrustedPolicyRegistry<'a> {
    grants: &'a [PolicyGrant<'a>],
    digest: Digest,
    order: ScratchVec<usize>,
}
impl<'a> TrustedPolicyRegistry<'a> {
    /// Prepare host records without selecting or deduplicating a winner.
    ///
    /// # Errors
    /// Empty identities, duplicate authority, framing overflow and native stops
    /// retain the original grant slice and usage. New index storage releases.
    #[expect(
        clippy::result_large_err,
        reason = "Refusals retain complete original borrowed context, usage and native causes without ungoverned boxing."
    )]
    pub fn prepare(
        grants: &'a [PolicyGrant<'a>],
        g: &mut Governor,
    ) -> Result<Self, PolicyTrustError<'a>> {
        let context = PolicyTrustContext {
            request: None,
            grants,
            scope: None,
            registry: None,
        };
        let result = (|| {
            let mut order = ScratchVec::new(g);
            for (index, grant) in grants.iter().enumerate() {
                g.charge(Resource::Work, 1)?;
                admit(grant.scope.tool, g)?;
                admit(grant.issuer, g)?;
                admit(grant.provenance, g)?;
                if grant.scope.tool.is_empty() || grant.issuer.is_empty() {
                    return Err(PolicyTrustCause::EmptyIdentity);
                }
                order.push(index)?;
                let mut position = order.len() - 1;
                while position > 0 {
                    let earlier = order[position - 1];
                    let comparison = compare_grants(&grants[earlier], grant, g)?;
                    if comparison == Ordering::Equal {
                        return Err(PolicyTrustCause::DuplicateGrant(earlier, index));
                    }
                    if comparison == Ordering::Less {
                        break;
                    }
                    g.charge(Resource::Work, 1)?;
                    order.as_mut_slice().swap(position - 1, position);
                    position -= 1;
                }
            }
            let mut hash = Transcript::new(b"purrassist.policy-registry.v1\0", g)?;
            hash.number(grants.len(), g)?;
            for &index in &order {
                let grant = &grants[index];
                hash.bytes(&grant.document, g)?;
                hash.bytes(&grant.contract, g)?;
                hash.field(grant.scope.tool, g)?;
                hash.bytes(&grant.scope.vocabulary, g)?;
                hash.field(grant.issuer, g)?;
                hash.bytes(&grant.scope.session_schema, g)?;
                hash.field(grant.provenance, g)?;
            }
            let digest = hash.finish(g)?;
            g.check()?;
            Ok(Self {
                grants,
                digest,
                order,
            })
        })();
        result.map_err(|cause| failure(context, cause, g))
    }
    /// Exact original records, in caller order.
    #[must_use]
    pub const fn grants(&self) -> &'a [PolicyGrant<'a>] {
        self.grants
    }
    /// Complete canonical registry SHA-256 for later substitution checks.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }
}

/// Exact original document and both independently relevant identities.
#[derive(Clone, Copy, Debug)]
pub struct PreparedPolicyDocument<'a> {
    /// Borrowed original text, location, base, issuer and ordered references.
    pub source: &'a PolicySource<'a>,
    /// SHA-256 of original text bytes only.
    pub document: Digest,
    /// SHA-256 of this document's finite rooted ordered graph transcript.
    pub contract: Digest,
}
impl ScratchCharge for PreparedPolicyDocument<'_> {
    const CHARGE: usize = 72;
}
const _: () = assert!(core::mem::size_of::<PreparedPolicyDocument<'_>>() <= 72);

/// Complete immutable source table, validated structurally but not parsed as SRL.
///
/// Preparing original bytes never supplies an implicit host grant:
///
/// ```
/// use purrassist_core::{Budget, Governor};
/// use purrassist_toolgraph::{PolicySource, PolicyContractInput, PolicyScope,
///     PreparedPolicyContract, TrustedPolicyRegistry, PolicyTrustCause};
/// let documents = [PolicySource { location: "urn:policy:root", external_base: None,
///     issuer: "host", text: "", imports: &[] }];
/// let mut governor = Governor::new(Budget::default());
/// let prepared = PreparedPolicyContract::prepare(PolicyContractInput {
///     root: "urn:policy:root", documents: &documents }, &mut governor).unwrap();
/// let registry = TrustedPolicyRegistry::prepare(&[], &mut governor).unwrap();
/// let scope = PolicyScope { tool: "urn:tool:read", vocabulary: [7; 32],
///     session_schema: [42; 32] };
/// let refused = prepared.bind(&registry, scope, &mut governor).unwrap_err();
/// assert_eq!(refused.cause, PolicyTrustCause::UntrustedDocument(0));
/// assert_eq!(prepared.root().source.text, "");
/// ```
#[derive(Debug)]
pub struct PreparedPolicyContract<'a> {
    input: PolicyContractInput<'a>,
    documents: ScratchVec<PreparedPolicyDocument<'a>>,
    root: usize,
}
impl<'a> PreparedPolicyContract<'a> {
    /// Admit exact originals and every document's finite rooted graph identity.
    ///
    /// # Errors
    /// Duplicate/missing/unreached documents, empty identities, checked overflow
    /// and native stops retain the original complete request and usage.
    /// Temporary traversal and retained manifest storage release on refusal.
    #[expect(
        clippy::result_large_err,
        reason = "Refusals retain complete original borrowed context, usage and native causes without ungoverned boxing."
    )]
    pub fn prepare(
        input: PolicyContractInput<'a>,
        g: &mut Governor,
    ) -> Result<Self, PolicyTrustError<'a>> {
        let context = PolicyTrustContext {
            request: Some(input),
            grants: &[],
            scope: None,
            registry: None,
        };
        let result = (|| {
            admit(input.root, g)?;
            let mut documents = ScratchVec::new(g);
            for (index, source) in input.documents.iter().enumerate() {
                g.charge(Resource::Work, 1)?;
                admit(source.location, g)?;
                admit(source.issuer, g)?;
                admit(source.text, g)?;
                if let Some(base) = source.external_base {
                    admit(base, g)?;
                }
                if source.location.is_empty() || source.issuer.is_empty() {
                    return Err(PolicyTrustCause::EmptyIdentity);
                }
                for (earlier, previous) in input.documents[..index].iter().enumerate() {
                    if equal(previous.location, source.location, g)? {
                        return Err(PolicyTrustCause::DuplicateDocument(earlier, index));
                    }
                }
                for reference in source.imports {
                    admit(reference, g)?;
                }
                let mut hash = Transcript::new(b"", g)?;
                hash.bytes(source.text.as_bytes(), g)?;
                let document = hash.finish(g)?;
                documents.push(PreparedPolicyDocument {
                    source,
                    document,
                    contract: document,
                })?;
            }
            let root =
                lookup(input.documents, input.root, g)?.ok_or(PolicyTrustCause::MissingRoot)?;
            for (index, source) in input.documents.iter().enumerate() {
                g.charge(Resource::Work, 1)?;
                for (edge, reference) in source.imports.iter().enumerate() {
                    if lookup(input.documents, reference, g)?.is_none() {
                        return Err(PolicyTrustCause::MissingImport(index, edge));
                    }
                }
            }
            let (digest, reached) = rooted(input.documents, &documents, root, g)?;
            for index in 0..input.documents.len() {
                g.charge(Resource::Work, 1)?;
                let mut found = false;
                for &visited in &reached {
                    g.charge(Resource::Work, 1)?;
                    if visited == index {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(PolicyTrustCause::UnreachedDocument(index));
                }
            }
            drop(reached);
            documents.as_mut_slice()[root].contract = digest;
            for index in 0..documents.len() {
                g.charge(Resource::Work, 1)?;
                if index != root {
                    documents.as_mut_slice()[index].contract =
                        rooted(input.documents, &documents, index, g)?.0;
                }
            }
            g.check()?;
            Ok(Self {
                input,
                documents,
                root,
            })
        })();
        result.map_err(|cause| failure(context, cause, g))
    }
    /// Complete original request, in caller table order.
    #[must_use]
    pub const fn input(&self) -> PolicyContractInput<'a> {
        self.input
    }
    /// Each original document with its text and rooted graph SHA-256.
    #[must_use]
    pub fn documents(&self) -> &[PreparedPolicyDocument<'a>] {
        &self.documents
    }
    /// Original root and its exact identities.
    #[must_use]
    pub fn root(&self) -> &PreparedPolicyDocument<'a> {
        &self.documents[self.root]
    }

    /// Match every reachable document under one exact host-selected scope.
    ///
    /// # Errors
    /// A missing exact grant refuses the complete request, even after earlier
    /// documents match. All new match storage releases; original request and
    /// registry records, selected scope, identity, usage and native stops remain.
    /// Final cancellation is checked before publishing any matched handle.
    #[expect(
        clippy::result_large_err,
        reason = "Refusals retain complete original borrowed context, usage and native causes without ungoverned boxing."
    )]
    pub fn bind<'b>(
        &'b self,
        registry: &'b TrustedPolicyRegistry<'_>,
        scope: PolicyScope<'b>,
        g: &mut Governor,
    ) -> Result<MatchedPolicyContract<'b>, PolicyTrustError<'b>> {
        let context = PolicyTrustContext {
            request: Some(self.input),
            grants: registry.grants,
            scope: Some(scope),
            registry: Some(registry.digest),
        };
        let result = (|| {
            admit(scope.tool, g)?;
            if scope.tool.is_empty() {
                return Err(PolicyTrustCause::EmptyIdentity);
            }
            let mut matches = ScratchVec::new(g);
            for (index, document) in self.documents.iter().enumerate() {
                let mut selected_grant = None;
                for &grant_index in &registry.order {
                    let grant = &registry.grants[grant_index];
                    g.charge(Resource::Work, 128)?;
                    let issuer = equal(document.source.issuer, grant.issuer, g)?;
                    let tool = equal(scope.tool, grant.scope.tool, g)?;
                    if document.document.as_bytes() == &grant.document
                        && document.contract.as_bytes() == &grant.contract
                        && issuer
                        && tool
                        && scope.vocabulary == grant.scope.vocabulary
                        && scope.session_schema == grant.scope.session_schema
                    {
                        selected_grant = Some(grant_index);
                        break;
                    }
                }
                matches.push(selected_grant.ok_or(PolicyTrustCause::UntrustedDocument(index))?)?;
            }
            g.check()?;
            Ok(MatchedPolicyContract {
                input: self.input,
                documents: &self.documents,
                grants: registry.grants,
                registry: registry.digest,
                scope,
                matches,
            })
        })();
        result.map_err(|cause| failure(context, cause, g))
    }
}

/// Immutable exact matches; this is neither native admission nor dispatch authority.
#[derive(Debug)]
pub struct MatchedPolicyContract<'a> {
    input: PolicyContractInput<'a>,
    documents: &'a [PreparedPolicyDocument<'a>],
    grants: &'a [PolicyGrant<'a>],
    registry: Digest,
    scope: PolicyScope<'a>,
    matches: ScratchVec<usize>,
}
impl<'a> MatchedPolicyContract<'a> {
    /// Original complete table, including exact external bases and SRL text.
    #[must_use]
    pub const fn input(&self) -> PolicyContractInput<'a> {
        self.input
    }
    /// Original documents and their exact identities, in caller order.
    #[must_use]
    pub const fn documents(&self) -> &'a [PreparedPolicyDocument<'a>] {
        self.documents
    }
    /// Complete registry SHA-256 bound by this handle.
    #[must_use]
    pub const fn registry_digest(&self) -> Digest {
        self.registry
    }
    /// Exact selected tool, opaque vocabulary and session-schema scope.
    #[must_use]
    pub const fn scope(&self) -> PolicyScope<'a> {
        self.scope
    }
    /// Exact original grant for a caller-order document index.
    #[must_use]
    pub fn grant(&self, document: usize) -> Option<&'a PolicyGrant<'a>> {
        self.matches.get(document).map(|&index| &self.grants[index])
    }
}

fn failure<'a>(
    context: PolicyTrustContext<'a>,
    cause: PolicyTrustCause,
    g: &Governor,
) -> PolicyTrustError<'a> {
    PolicyTrustError {
        context,
        cause,
        usage: g.usage(),
    }
}
fn admit(text: &str, g: &mut Governor) -> Result<(), PolicyTrustCause> {
    g.charge(Resource::Work, 1)?;
    g.charge(Resource::InputBytes, length(text.len())?)?;
    Ok(())
}
fn length(value: usize) -> Result<u64, PolicyTrustCause> {
    u64::try_from(value).map_err(|_| PolicyTrustCause::LengthOverflow)
}
fn equal(left: &str, right: &str, g: &mut Governor) -> Result<bool, PolicyTrustCause> {
    g.charge(
        Resource::Work,
        length(left.len().min(right.len()))?
            .checked_add(1)
            .ok_or(PolicyTrustCause::LengthOverflow)?,
    )?;
    Ok(left == right)
}
fn lookup(
    table: &[PolicySource<'_>],
    location: &str,
    g: &mut Governor,
) -> Result<Option<usize>, PolicyTrustCause> {
    for (index, document) in table.iter().enumerate() {
        if equal(document.location, location, g)? {
            return Ok(Some(index));
        }
    }
    Ok(None)
}
fn compare_grants(
    left: &PolicyGrant<'_>,
    right: &PolicyGrant<'_>,
    g: &mut Governor,
) -> Result<Ordering, PolicyTrustCause> {
    let mut work = 128u64;
    for (a, b) in [
        (left.scope.tool, right.scope.tool),
        (left.issuer, right.issuer),
    ] {
        work = work
            .checked_add(length(a.len().min(b.len()))?)
            .and_then(|n| n.checked_add(1))
            .ok_or(PolicyTrustCause::LengthOverflow)?;
    }
    g.charge(Resource::Work, work)?;
    Ok((
        left.document,
        left.contract,
        left.scope.tool,
        left.scope.vocabulary,
        left.issuer,
        left.scope.session_schema,
    )
        .cmp(&(
            right.document,
            right.contract,
            right.scope.tool,
            right.scope.vocabulary,
            right.issuer,
            right.scope.session_schema,
        )))
}

struct Transcript {
    hash: DigestBuilder,
    bytes: u64,
}
impl Transcript {
    fn new(domain: &[u8], g: &mut Governor) -> Result<Self, PolicyTrustCause> {
        let mut result = Self {
            hash: DigestBuilder::new(),
            bytes: 0,
        };
        result.bytes(domain, g)?;
        Ok(result)
    }
    fn bytes(&mut self, bytes: &[u8], g: &mut Governor) -> Result<(), PolicyTrustCause> {
        let count = length(bytes.len())?;
        let total = self
            .bytes
            .checked_add(count)
            .filter(|n| *n <= (u64::MAX >> 3))
            .ok_or(PolicyTrustCause::LengthOverflow)?;
        g.charge(
            Resource::Work,
            count
                .checked_add(1)
                .ok_or(PolicyTrustCause::LengthOverflow)?,
        )?;
        self.hash.update(bytes);
        self.bytes = total;
        Ok(())
    }
    fn number(&mut self, value: usize, g: &mut Governor) -> Result<(), PolicyTrustCause> {
        self.bytes(&length(value)?.to_be_bytes(), g)
    }
    fn field(&mut self, text: &str, g: &mut Governor) -> Result<(), PolicyTrustCause> {
        self.number(text.len(), g)?;
        self.bytes(text.as_bytes(), g)
    }
    fn finish(self, g: &mut Governor) -> Result<Digest, PolicyTrustCause> {
        g.charge(Resource::Work, 128)?;
        Ok(self.hash.finish())
    }
}

fn rooted(
    table: &[PolicySource<'_>],
    documents: &[PreparedPolicyDocument<'_>],
    root: usize,
    g: &mut Governor,
) -> Result<(Digest, ScratchVec<usize>), PolicyTrustCause> {
    let mut seen = ScratchVec::new(g);
    let mut pending = ScratchVec::new(g);
    pending.push((root, 0usize))?;
    let mut hash = Transcript::new(b"purrassist.policy-contract.v1\0", g)?;
    while let Some((index, next)) = pending.pop() {
        g.charge(Resource::Work, 1)?;
        let source = &table[index];
        if next == 0 {
            let mut earlier = None;
            for (id, &visited) in seen.iter().enumerate() {
                g.charge(Resource::Work, 1)?;
                if visited == index {
                    earlier = Some(id);
                    break;
                }
            }
            if let Some(id) = earlier {
                hash.bytes(&[2], g)?;
                hash.number(id, g)?;
                continue;
            }
            hash.bytes(&[1], g)?;
            hash.number(seen.len(), g)?;
            seen.push(index)?;
            hash.field(source.location, g)?;
            hash.bytes(&[u8::from(source.external_base.is_some())], g)?;
            if let Some(base) = source.external_base {
                hash.field(base, g)?;
            }
            hash.field(source.issuer, g)?;
            hash.bytes(documents[index].document.as_bytes(), g)?;
            hash.number(source.imports.len(), g)?;
        }
        let edge = next.saturating_sub(1);
        if next == 0 || edge < source.imports.len() {
            let edge = if next == 0 { 0 } else { edge };
            if edge < source.imports.len() {
                pending.push((
                    index,
                    edge.checked_add(2)
                        .ok_or(PolicyTrustCause::LengthOverflow)?,
                ))?;
                let target = lookup(table, source.imports[edge], g)?
                    .ok_or(PolicyTrustCause::MissingImport(index, edge))?;
                pending.push((target, 0))?;
                continue;
            }
        }
        hash.bytes(&[3], g)?;
    }
    Ok((hash.finish(g)?, seen))
}
