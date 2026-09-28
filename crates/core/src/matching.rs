// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Deterministic evidence-bearing matching. Scores are ranking units, not probabilities.
//!
//! Normalization-based equivalence is an explicit lattice of refinements,
//! finest first: byte identity ⊑ NFC ⊑ case-fold ⊑ width/compatibility-fold
//! ⊑ diacritic-strip ⊑ UTS-39 confusable skeleton ⊑ depunctuation ⊑
//! phonetics. Each level a query and a candidate agree at emits one named,
//! versioned [`Signal`] whose score decreases as the level coarsens, so the
//! finest level at which a query has a unique acceptable candidate is what
//! [`SelectionPolicy`] resolves to. Coarser levels never retract a finer
//! level's evidence; they only fire when nothing finer already matched.
//!
//! Independent of that lattice, **protected token classes** — decimal digit
//! runs, negation markers, semver components, ISO dates, hex digests, and
//! units — bound every normalizer: a difference inside one of these classes
//! between the query and a candidate's primary spelling excludes that
//! candidate with explicit evidence, no matter how similar the surrounding
//! text is, unless one of the candidate's caller-declared aliases matches the
//! query exactly. See `docs/MATCHING-QUALITY.md` for the curated evidence set.
use crate::contract::{
    AssistResult, Budget, Effort, Evidence, Governor, Outcome, Receipt, Resource, StopReason,
};
use crate::text::{
    Normalization, case_fold, comparison_key, depunctuate, general_category, lexical_tokens,
    normalize, skeleton,
};
use crate::{
    digest::Digest,
    json::Node,
    wire::{array, integer, object, string},
};
use std::collections::{BTreeMap, BTreeSet};

/// A caller-owned identity with a human-readable spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Stable identity returned unchanged to the caller.
    pub id: String,
    /// The spelling to compare against.
    pub text: String,
    /// Explicit caller-supplied alternative spellings or domain equivalents.
    pub aliases: Vec<String>,
}
impl Candidate {
    /// Construct an identified candidate without aliases.
    #[must_use]
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            aliases: Vec::new(),
        }
    }
}

/// One independently interpretable comparison signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    /// Stable algorithm/profile identifier.
    pub method: &'static str,
    /// Integer ranking units between zero and 10,000.
    pub score: u32,
    /// Observable evidence supporting the score.
    pub detail: String,
}

/// A ranked identity with all supporting comparison signals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankedMatch {
    /// Original identity and spelling, never replaced with a normalized path.
    pub candidate: Candidate,
    /// Maximum supporting signal, with deterministic identity tie-breaking.
    pub score: u32,
    /// Evidence from exact, lexical, distance, phonetic, and alias comparisons.
    pub signals: Vec<Signal>,
}
impl RankedMatch {
    /// Serialize the selected identity and its signals. These exact JSON bytes
    /// identify an alternative in the receipt.
    ///
    /// ```
    /// use purrassist_core::{Outcome, matching::{self, Candidate, MatchRequest, SelectionPolicy}};
    /// let result = matching::resolve("tea", &[Candidate::new("tea-id", "tea")],
    ///     &MatchRequest::default(), SelectionPolicy::default());
    /// let Outcome::Resolved(answer) = result.outcome else { panic!("expected exact match") };
    /// assert_eq!(answer.to_node().pointer("/id").unwrap().as_str(), Some("tea-id"));
    /// ```
    #[must_use]
    pub fn to_node(&self) -> Node {
        object([
            ("id", string(&self.candidate.id)),
            ("text", string(&self.candidate.text)),
            ("score", integer(u64::from(self.score))),
            ("aliases", array(self.candidate.aliases.iter().map(string))),
            (
                "signals",
                array(self.signals.iter().map(|signal| {
                    object([
                        ("method", string(signal.method)),
                        ("score", integer(u64::from(signal.score))),
                        ("detail", string(&signal.detail)),
                    ])
                })),
            ),
        ])
    }
}

/// Explicit acceptance and separation thresholds for choosing one string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionPolicy {
    /// Minimum ranking score, from zero through 10,000.
    pub minimum_score: u32,
    /// Scores within this many units of the winner remain ambiguous. Ties
    /// remain ambiguous even at zero margin.
    pub minimum_margin: u32,
}
impl Default for SelectionPolicy {
    fn default() -> Self {
        Self {
            minimum_score: 7500,
            minimum_margin: 500,
        }
    }
}

/// Select a string under the request's resource limits.
///
/// A unique byte-exact spelling is resolved; all other accepted selections
/// are labeled guesses. A candidate whose protected tokens (decimal digit
/// runs, negation markers, semver components, ISO dates, hex digests, or
/// units) differ from the query's cannot be selected unless a caller-declared
/// alias matches the query exactly. No score is treated as a probability.
/// Invalid policy, exhaustion, cancellation, and no acceptable candidate
/// return `Unresolved` with the cause in the receipt.
#[must_use]
pub fn resolve(
    query: &str,
    candidates: &[Candidate],
    request: &MatchRequest,
    policy: SelectionPolicy,
) -> AssistResult<RankedMatch> {
    resolve_governed(
        query,
        candidates,
        request,
        policy,
        &mut Governor::new(request.budget),
    )
}

/// Select with cumulative caller-owned limits and cancellation. Exhaustion
/// discards partial alternatives and returns an explicit unresolved receipt.
///
/// ```
/// use purrassist_core::{Outcome, matching::{self, Candidate, MatchRequest, SelectionPolicy}};
/// let answer = matching::resolve("raeder", &[Candidate::new("reader", "reader")],
///     &MatchRequest::default(), SelectionPolicy::default());
/// assert!(matches!(answer.outcome, Outcome::BestGuess(_)));
/// ```
///
/// A protected-class difference excludes a candidate outright, even one that
/// would otherwise be the closest textual match:
///
/// ```
/// use purrassist_core::{Outcome, matching::{self, Candidate, MatchRequest, SelectionPolicy}};
/// let answer = matching::resolve("invoice-1001", &[Candidate::new("wrong", "invoice-1002")],
///     &MatchRequest::default(), SelectionPolicy::default());
/// assert_eq!(answer.outcome, Outcome::Unresolved);
/// assert_eq!(answer.receipt.evidence[0].rule, "matching.numeric-identity-v1");
/// ```
#[must_use]
pub fn resolve_governed(
    query: &str,
    candidates: &[Candidate],
    request: &MatchRequest,
    policy: SelectionPolicy,
    governor: &mut Governor,
) -> AssistResult<RankedMatch> {
    let mut receipt = Receipt::new(
        query.as_bytes(),
        "matching/resolve-v1",
        request.effort,
        *governor.limits(),
    );
    if policy.minimum_score > 10_000 || policy.minimum_margin > 10_000 {
        receipt.stopped =
            StopReason::InvalidInput("string selection thresholds exceed ranking bounds".into());
        receipt.usage = governor.usage();
        return AssistResult {
            outcome: Outcome::Unresolved,
            receipt,
        };
    }
    let mut full_request = request.clone();
    full_request.limit = candidates.len().max(1);
    let ranked = rank_governed(query, candidates, &full_request, governor);
    if ranked.receipt.stopped != StopReason::Completed
        && ranked.receipt.stopped != StopReason::NoCandidate
    {
        receipt.stopped = ranked.receipt.stopped;
        receipt.usage = governor.usage();
        return AssistResult {
            outcome: Outcome::Unresolved,
            receipt,
        };
    }
    // `rank_governed` already excluded every protected-class violation before
    // scoring; surface that exclusion evidence here even when it leaves no
    // candidate at all (the `Unresolved` branch below).
    receipt.evidence.extend(
        ranked
            .receipt
            .evidence
            .iter()
            .filter(|evidence| is_protected_class_rule(&evidence.rule))
            .cloned(),
    );
    let Outcome::Resolved(mut matches) = ranked.outcome else {
        receipt.stopped = StopReason::NoCandidate;
        receipt.usage = governor.usage();
        return AssistResult {
            outcome: Outcome::Unresolved,
            receipt,
        };
    };
    if matches.is_empty() {
        receipt.stopped = StopReason::NoCandidate;
        receipt.usage = governor.usage();
        return AssistResult {
            outcome: Outcome::Unresolved,
            receipt,
        };
    }
    let exact = matches.iter().filter(|entry| entry.score == 10_000).count();
    let best = matches[0].score;
    matches.retain(|entry| {
        if exact > 0 {
            entry.score == 10_000
        } else {
            entry.score >= policy.minimum_score
                && (entry.score == best || best - entry.score < policy.minimum_margin)
        }
    });
    let outcome = if matches.is_empty() {
        receipt.stopped = StopReason::NoCandidate;
        Outcome::Unresolved
    } else {
        let recorded = record_selection(&matches, policy, governor, &mut receipt);
        match recorded {
            Err(reason) => {
                receipt.stopped = reason;
                receipt.alternatives.clear();
                receipt.evidence.clear();
                receipt.inferences.clear();
                Outcome::Unresolved
            }
            Ok(()) if matches.len() > 1 => Outcome::Ambiguous(matches),
            Ok(()) => matches
                .into_iter()
                .next()
                .map_or(Outcome::Unresolved, |answer| {
                    if answer.score == 10_000 {
                        Outcome::Resolved(answer)
                    } else {
                        Outcome::BestGuess(answer)
                    }
                }),
        }
    };
    receipt.usage = governor.usage();
    AssistResult { outcome, receipt }
}

/// A class of token no lattice normalizer is permitted to collapse. A
/// difference here excludes a candidate outright regardless of how similar
/// the surrounding text is, unless a caller-declared alias matches the
/// query exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProtectedClass {
    /// Maximal runs of Unicode decimal digits (`Nd`).
    Digits,
    /// `not`/`no`, `non-`/`un-`-prefixed negation words, and `!`.
    Negation,
    /// `major.minor.patch[-pre][+build]` semantic-version components.
    Semver,
    /// `YYYY-MM-DD` calendar dates.
    IsoDate,
    /// Hexadecimal runs of at least seven characters, containing a letter.
    HexDigest,
    /// A decimal quantity immediately paired with a unit abbreviation.
    Unit,
}
impl ProtectedClass {
    const ALL: [Self; 6] = [
        Self::Digits,
        Self::Negation,
        Self::Semver,
        Self::IsoDate,
        Self::HexDigest,
        Self::Unit,
    ];
    /// Stable evidence rule identifier. `Digits` keeps its original name
    /// (`matching.numeric-identity-v1`) so callers pinned to it are unaffected.
    const fn rule(self) -> &'static str {
        match self {
            Self::Digits => "matching.numeric-identity-v1",
            Self::Negation => "matching.protected-negation-v1",
            Self::Semver => "matching.protected-semver-v1",
            Self::IsoDate => "matching.protected-iso-date-v1",
            Self::HexDigest => "matching.protected-hex-digest-v1",
            Self::Unit => "matching.protected-unit-v1",
        }
    }
    const fn explanation(self) -> &'static str {
        match self {
            Self::Digits => "decimal digit runs differ from the query and no caller alias matches",
            Self::Negation => "negation markers differ from the query and no caller alias matches",
            Self::Semver => {
                "semantic-version components differ from the query and no caller alias matches"
            }
            Self::IsoDate => "ISO calendar dates differ from the query and no caller alias matches",
            Self::HexDigest => {
                "hexadecimal digests differ from the query and no caller alias matches"
            }
            Self::Unit => "quantities with units differ from the query and no caller alias matches",
        }
    }
}

const UNITS: &[&str] = &[
    "ms", "s", "min", "hr", "h", "kb", "mb", "gb", "tb", "pb", "b", "px", "pt", "em", "rem", "kg",
    "mg", "g", "km", "cm", "mm", "ft",
];
const NEGATION_WORDS: &[&str] = &["not", "no", "non", "un"];
const UN_PREFIXED_NEGATIONS: &[&str] = &[
    "unable",
    "unavailable",
    "undone",
    "unknown",
    "unresolved",
    "unnecessary",
    "unwanted",
    "untrue",
    "unsafe",
    "unsure",
    "unclear",
    "unhappy",
    "undefined",
    "unsupported",
    "unauthorized",
    "unauthorised",
    "uninstalled",
    "unsuccessful",
    "unconfirmed",
    "unpaid",
    "uninitialized",
    "unlikely",
    "unequal",
];

fn is_protected_class_rule(rule: &str) -> bool {
    ProtectedClass::ALL.iter().any(|class| class.rule() == rule)
}

/// One protected class's ordered token sequence extracted from `text`.
fn protected_tokens(text: &str, class: ProtectedClass) -> Vec<String> {
    match class {
        ProtectedClass::Digits => digit_runs(&comparison_key(text)),
        ProtectedClass::Negation => negation_tokens(text),
        ProtectedClass::Semver => semver_tokens(&comparison_key(text)),
        ProtectedClass::IsoDate => iso_date_tokens(&comparison_key(text)),
        ProtectedClass::HexDigest => hex_digest_tokens(&comparison_key(text)),
        ProtectedClass::Unit => unit_tokens(&comparison_key(text)),
    }
}

fn digit_runs(key: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    for character in key.chars() {
        if general_category(character) == "Nd" {
            current.push(character);
        } else if !current.is_empty() {
            runs.push(core::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

fn negation_tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = lexical_tokens(text)
        .into_iter()
        .filter(|token| {
            NEGATION_WORDS.contains(&token.as_str())
                || UN_PREFIXED_NEGATIONS.contains(&token.as_str())
        })
        .collect();
    if text.contains('!') {
        out.push("!".into());
    }
    out.sort();
    out
}

fn take_ascii_digits(chars: &[char], index: &mut usize) -> bool {
    let start = *index;
    while chars.get(*index).is_some_and(char::is_ascii_digit) {
        *index += 1;
    }
    *index > start
}

fn semver_tokens(key: &str) -> Vec<String> {
    let chars: Vec<char> = key.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let start = index;
        let mut cursor = index;
        let matched = take_ascii_digits(&chars, &mut cursor)
            && chars.get(cursor) == Some(&'.')
            && {
                cursor += 1;
                take_ascii_digits(&chars, &mut cursor)
            }
            && chars.get(cursor) == Some(&'.')
            && {
                cursor += 1;
                take_ascii_digits(&chars, &mut cursor)
            };
        if matched {
            if matches!(chars.get(cursor), Some('-' | '+')) {
                let mut suffix = cursor + 1;
                while chars
                    .get(suffix)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                {
                    suffix += 1;
                }
                if suffix > cursor + 1 {
                    cursor = suffix;
                }
            }
            out.push(chars[start..cursor].iter().collect());
            index = cursor;
        } else {
            index += 1;
        }
    }
    out
}

fn iso_date_tokens(key: &str) -> Vec<String> {
    let chars: Vec<char> = key.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index + 10 <= chars.len() {
        let digits = |range: std::ops::Range<usize>| chars[range].iter().all(char::is_ascii_digit);
        let boundary_before = index == 0 || !chars[index - 1].is_ascii_digit();
        let boundary_after = !chars.get(index + 10).is_some_and(char::is_ascii_digit);
        if boundary_before
            && boundary_after
            && digits(index..index + 4)
            && chars[index + 4] == '-'
            && digits(index + 5..index + 7)
            && chars[index + 7] == '-'
            && digits(index + 8..index + 10)
        {
            out.push(chars[index..index + 10].iter().collect());
            index += 10;
        } else {
            index += 1;
        }
    }
    out
}

fn hex_digest_tokens(key: &str) -> Vec<String> {
    let chars: Vec<char> = key.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_hexdigit() {
            let start = index;
            while chars.get(index).is_some_and(char::is_ascii_hexdigit) {
                index += 1;
            }
            if index - start >= 7 && chars[start..index].iter().any(char::is_ascii_alphabetic) {
                out.push(chars[start..index].iter().collect());
            }
        } else {
            index += 1;
        }
    }
    out
}

fn unit_tokens(key: &str) -> Vec<String> {
    let chars: Vec<char> = key.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let start = index;
            while chars.get(index).is_some_and(char::is_ascii_digit) {
                index += 1;
            }
            let mut letters_start = index;
            if chars.get(letters_start) == Some(&' ') {
                letters_start += 1;
            }
            let mut letters_end = letters_start;
            while chars
                .get(letters_end)
                .is_some_and(char::is_ascii_alphabetic)
            {
                letters_end += 1;
            }
            let unit: String = chars[letters_start..letters_end].iter().collect();
            let boundary_after = !chars.get(letters_end).is_some_and(|c| c.is_alphanumeric());
            if letters_end > letters_start && UNITS.contains(&unit.as_str()) && boundary_after {
                let digits: String = chars[start..index].iter().collect();
                out.push(format!("{digits}{unit}"));
                index = letters_end;
            }
        } else {
            index += 1;
        }
    }
    out
}

/// Protected classes where `query` and `text` differ, unless `bypass` (an
/// exact caller alias match) waives the check entirely.
///
/// # Errors
/// Returns governor exhaustion before allocating the extraction scratch.
fn protected_class_violations(
    query: &str,
    text: &str,
    bypass: bool,
    governor: &mut Governor,
) -> Result<Vec<ProtectedClass>, StopReason> {
    if bypass || query == text {
        return Ok(Vec::new());
    }
    governor.charge(
        Resource::Work,
        size(query.len().saturating_add(text.len())).saturating_mul(8),
    )?;
    // Every class's extraction allocates a handful of Strings proportional to
    // input length (the negation class additionally tokenizes both strings,
    // each tokenization allocating a comparison key, a token vector, and a
    // dedup set); charge a conservative bound before allocating any of them,
    // released automatically once this function returns.
    let reservation = size(query.len().saturating_add(text.len()))
        .saturating_mul(160)
        .saturating_add(512);
    let _lease = governor.lease_scratch(reservation)?;
    let violations = ProtectedClass::ALL
        .into_iter()
        .filter(|class| protected_tokens(query, *class) != protected_tokens(text, *class))
        .collect();
    Ok(violations)
}

fn record_selection(
    matches: &[RankedMatch],
    policy: SelectionPolicy,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<(), StopReason> {
    let ambiguous = matches.len() > 1;
    let mut forest = ambiguous.then(|| crate::provenance::Forest::new(governor));
    let mut derivations = Vec::with_capacity(matches.len());
    for entry in matches {
        let serialized = entry.to_node().to_json();
        let storage = size(serialized.len()).saturating_mul(4).saturating_add(512);
        governor.charge(Resource::Work, storage)?;
        governor.charge(Resource::ScratchBytes, storage)?;
        governor.charge(
            Resource::EvidenceEntries,
            1 + u64::from(entry.score != 10_000),
        )?;
        let evidence = Evidence {
            rule: "matching.selection-v1".into(),
            explanation: format!(
                "score {}; signals {:?}; selection policy {policy:?}",
                entry.score,
                entry
                    .signals
                    .iter()
                    .map(|signal| signal.method)
                    .collect::<Vec<_>>()
            ),
            span: None,
            destination: entry.candidate.id.clone(),
        };
        let inferences = if entry.score == 10_000 {
            Vec::new()
        } else {
            vec![crate::contract::Inference {
                evidence: evidence.clone(),
                kind: crate::contract::InferenceKind::Guess,
                value: entry.candidate.id.clone(),
            }]
        };
        if let Some(forest) = forest.as_mut() {
            let mut derivation =
                crate::provenance::Derivation::new(Digest::of(serialized.as_bytes()), 0);
            derivation.push(forest.evidence(governor, evidence)?);
            for inference in inferences {
                derivation.push(forest.inference(governor, inference)?);
            }
            derivations.push(derivation);
        } else {
            receipt.evidence.push(evidence);
            receipt.inferences.extend(inferences);
        }
    }
    if let Some(forest) = forest {
        let published = forest.publish(&derivations, governor)?;
        // `receipt.evidence` may already carry protected-class exclusion
        // evidence recorded before selection; extend rather than replace it.
        receipt.evidence.extend(published.evidence);
        receipt.inferences.extend(published.inferences);
        receipt.alternatives = published.alternatives;
    }
    Ok(())
}

/// Caller-selected matching behavior.
#[derive(Clone, Debug)]
pub struct MatchRequest {
    /// Effort preset.
    pub effort: Effort,
    /// Effective resource bounds.
    pub budget: Budget,
    /// Include the explicitly English Soundex phonetic signal.
    pub english_phonetics: bool,
    /// Maximum number of ranked results retained.
    pub limit: usize,
}
impl Default for MatchRequest {
    fn default() -> Self {
        Self {
            effort: Effort::Standard,
            budget: Budget::default(),
            english_phonetics: true,
            limit: 10,
        }
    }
}

/// Rank candidates while preserving resource termination and comparison evidence.
/// Duplicate candidate identities are a configuration error, recorded in the receipt.
#[must_use]
pub fn rank(
    query: &str,
    candidates: &[Candidate],
    request: &MatchRequest,
) -> AssistResult<Vec<RankedMatch>> {
    rank_governed(
        query,
        candidates,
        request,
        &mut Governor::new(request.budget),
    )
}

/// Rank with caller-owned cumulative limits and cancellation.
///
/// The governor overrides `request.budget`. Exhaustion or cancellation returns
/// `Unresolved` with the cumulative usage and termination reason in its receipt.
///
/// ```
/// use purrassist_core::{Budget, Governor, Outcome, matching::{self, Candidate, MatchRequest}};
/// let mut governor = Governor::new(Budget::default());
/// let result = matching::rank_governed("tea", &[Candidate::new("1", "tea")],
///     &MatchRequest::default(), &mut governor);
/// assert!(matches!(result.outcome, Outcome::Resolved(_)));
/// assert_eq!(result.receipt.usage, governor.usage());
/// ```
#[must_use]
pub fn rank_governed(
    query: &str,
    candidates: &[Candidate],
    request: &MatchRequest,
    governor: &mut Governor,
) -> AssistResult<Vec<RankedMatch>> {
    let mut receipt = Receipt::new(
        query.as_bytes(),
        "matching/rank-v1",
        request.effort,
        *governor.limits(),
    );
    let mut identities = BTreeSet::new();
    let mut ranked = Vec::new();
    let processing = (|| -> Result<(), StopReason> {
        governor.charge(Resource::InputBytes, size(query.len()))?;
        if request.limit == 0 {
            return Err(StopReason::InvalidInput(
                "result limit must be positive".into(),
            ));
        }
        for candidate in candidates {
            let alias_bytes = admit_candidate(candidate, governor)?;
            if !identities.insert(&candidate.id) {
                return Err(StopReason::InvalidInput(format!(
                    "duplicate candidate identity: {}",
                    candidate.id
                )));
            }
            if request.effort != Effort::Exact {
                let bypass = candidate
                    .aliases
                    .iter()
                    .any(|alias| comparison_key(alias) == comparison_key(query));
                let violations =
                    protected_class_violations(query, &candidate.text, bypass, governor)?;
                if !violations.is_empty() {
                    governor.charge(Resource::EvidenceEntries, size(violations.len()))?;
                    for class in violations {
                        receipt.evidence.push(Evidence {
                            rule: class.rule().into(),
                            explanation: class.explanation().into(),
                            span: None,
                            destination: candidate.id.clone(),
                        });
                    }
                    continue;
                }
            }
            let mut signals = signals(query, &candidate.text, request, governor)?;
            let alias_lease =
                append_alias_signals(query, candidate, request, governor, &mut signals)?;
            let score = signals.iter().map(|signal| signal.score).max().unwrap_or(0);
            if score > 0 {
                // The alias reservation now backs a kept, ranked match:
                // retain it permanently instead of releasing it.
                alias_lease.retain();
                reserve_ranked(candidate, &signals, alias_bytes, governor)?;
                for signal in &signals {
                    receipt.evidence.push(Evidence {
                        rule: signal.method.into(),
                        explanation: signal.detail.clone(),
                        span: None,
                        destination: candidate.id.clone(),
                    });
                }
                ranked.push(RankedMatch {
                    candidate: candidate.clone(),
                    score,
                    signals,
                });
            } // else: `alias_lease` drops here, releasing its charge.
        }
        Ok(())
    })();
    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.candidate.id.cmp(&right.candidate.id))
    });
    ranked.truncate(request.limit);
    receipt.usage = governor.usage();
    receipt.stopped = processing.err().unwrap_or(if ranked.is_empty() {
        StopReason::NoCandidate
    } else {
        StopReason::Completed
    });
    let outcome = if receipt.stopped != StopReason::Completed || ranked.is_empty() {
        Outcome::Unresolved
    } else {
        Outcome::Resolved(ranked)
    };
    AssistResult { outcome, receipt }
}

fn admit_candidate(candidate: &Candidate, governor: &mut Governor) -> Result<u64, StopReason> {
    governor.charge(Resource::Candidates, 1)?;
    governor.charge(Resource::InputBytes, size(candidate.id.len()))?;
    governor.charge(Resource::InputBytes, size(candidate.text.len()))?;
    let mut alias_bytes = 0_u64;
    for alias in &candidate.aliases {
        governor.charge(Resource::InputBytes, size(alias.len()))?;
        alias_bytes = alias_bytes.saturating_add(size(alias.len()));
    }
    governor.charge(Resource::ScratchBytes, 128)?;
    Ok(alias_bytes)
}

fn append_alias_signals(
    query: &str,
    candidate: &Candidate,
    request: &MatchRequest,
    governor: &mut Governor,
    signals: &mut Vec<Signal>,
) -> Result<crate::contract::ScratchLease, StopReason> {
    if request.effort == Effort::Exact {
        return Ok(governor.scratch_lease_for(0));
    }
    let mut alias_storage = 0_u64;
    for alias in &candidate.aliases {
        let reservation = size(alias.len()).saturating_mul(6).saturating_add(128);
        governor.charge(Resource::ScratchBytes, reservation)?;
        alias_storage = alias_storage.saturating_add(reservation);
        let alias_signals = signals_for_alias(query, alias, request, governor)?;
        if let Some(score) = alias_signals.iter().map(|signal| signal.score).max() {
            signals.push(Signal {
                method: "caller-alias-v1",
                score: score.min(9900),
                detail: format!("caller supplied alias {alias:?}"),
            });
        }
    }
    Ok(governor.scratch_lease_for(alias_storage))
}

fn reserve_ranked(
    candidate: &Candidate,
    signals: &[Signal],
    alias_bytes: u64,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let signal_bytes = signals.iter().fold(0_u64, |sum, signal| {
        sum.saturating_add(size(signal.detail.len()))
    });
    let retained = size(candidate.id.len())
        .saturating_mul(size(signals.len()).saturating_add(1))
        .saturating_add(size(candidate.text.len()))
        .saturating_add(alias_bytes)
        .saturating_add(signal_bytes)
        .saturating_add(size(signals.len()).saturating_mul(256))
        .saturating_add(256);
    governor.charge(Resource::Work, retained)?;
    governor.charge(Resource::ScratchBytes, retained)?;
    governor.charge(Resource::EvidenceEntries, size(signals.len()))
}

fn signals_for_alias(
    query: &str,
    alias: &str,
    request: &MatchRequest,
    governor: &mut Governor,
) -> Result<Vec<Signal>, StopReason> {
    governor.charge(Resource::Candidates, 1)?;
    governor.charge(Resource::InputBytes, size(alias.len()))?;
    signals(query, alias, request, governor)
}
fn signals(
    query: &str,
    text: &str,
    request: &MatchRequest,
    governor: &mut Governor,
) -> Result<Vec<Signal>, StopReason> {
    governor.charge(Resource::Work, size(query.len().saturating_add(text.len())))?;
    let mut output = Vec::new();
    if query == text {
        output.push(Signal {
            method: "exact-v1",
            score: 10_000,
            detail: "byte-exact spelling".into(),
        });
        return Ok(output);
    }
    if request.effort == Effort::Exact {
        return Ok(output);
    }
    // Reserve a conservative bound covering every normalized key this
    // refinement lattice allocates for both strings, before allocating any
    // of them: NFC, case-fold, the width/compatibility-fold comparison key,
    // diacritic-stripped keys, UTS-39 skeletons, depunctuated keys, and (at
    // `Standard` effort and above) the lexical-Jaccard token sets, each of
    // which tokenizes both strings into a vector and a dedup set.
    let reservation = size(query.len().saturating_add(text.len()))
        .saturating_mul(320)
        .saturating_add(512);
    let _lease = governor.lease_scratch(reservation)?;
    governor.charge(
        Resource::Work,
        size(query.len().saturating_add(text.len())).saturating_mul(8),
    )?;
    let left = comparison_key(query);
    let right = comparison_key(text);
    lattice_signal(query, text, &left, &right, &mut output);
    if depunctuate(query) == depunctuate(text) {
        output.push(Signal {
            method: "unicode-17-depunctuated-v1",
            score: 9600,
            detail: "equivalent after removing punctuation and separators".into(),
        });
    }
    let distance = levenshtein(&left, &right, governor)?;
    let length = left.chars().count().max(right.chars().count());
    let score = fraction(length.saturating_sub(distance), length, 9500);
    output.push(Signal {
        method: "levenshtein-unicode-v1",
        score,
        detail: format!("{distance} scalar edits across {length} scalars"),
    });
    if request.effort >= Effort::Standard {
        let distance = damerau_levenshtein(&left, &right, governor)?;
        output.push(Signal {
            method: "damerau-levenshtein-unicode-v1",
            score: fraction(length.saturating_sub(distance), length, 9500),
            detail: format!("{distance} edits including unrestricted adjacent transpositions"),
        });
        let left_tokens: BTreeSet<_> = lexical_tokens(query).into_iter().collect();
        let right_tokens: BTreeSet<_> = lexical_tokens(text).into_iter().collect();
        output.push(Signal {
            method: "lexical-jaccard-v1",
            score: fraction(
                left_tokens.intersection(&right_tokens).count(),
                left_tokens.union(&right_tokens).count(),
                8500,
            ),
            detail: "Jaccard overlap of Unicode letter/number/mark runs".into(),
        });
    }
    if request.english_phonetics
        && let Some(left_sound) = soundex(&left)
        && Some(left_sound) == soundex(&right)
    {
        output.push(Signal {
            method: "soundex-en-v1",
            score: 6000,
            detail: format!(
                "matching English Soundex code {}",
                String::from_utf8_lossy(&left_sound)
            ),
        });
    }
    Ok(output)
}
/// Test the query/text pair against the refinement lattice finer than the
/// width/compatibility-fold comparison key (`left`/`right`), finest first,
/// emitting exactly one named signal for the finest level that matches.
/// Coarser levels are only tested once every finer level has failed, so a
/// finer match's evidence is never shadowed by a coarser, less specific one.
fn lattice_signal(query: &str, text: &str, left: &str, right: &str, output: &mut Vec<Signal>) {
    let nfc_query = normalize(query, Normalization::Nfc);
    let nfc_text = normalize(text, Normalization::Nfc);
    if nfc_query == nfc_text {
        output.push(Signal {
            method: "unicode-17-nfc-v1",
            score: 9950,
            detail: "equivalent after canonical composition, before case or width folding".into(),
        });
        return;
    }
    if case_fold(&nfc_query) == case_fold(&nfc_text) {
        output.push(Signal {
            method: "unicode-17-case-fold-v1",
            score: 9925,
            detail: "equivalent after canonical composition and full case folding, before width or compatibility folding"
                .into(),
        });
        return;
    }
    if left == right {
        output.push(Signal {
            method: "unicode-17-caseless-v1",
            score: 9900,
            detail: "equivalent after pinned Unicode normalization and full case folding".into(),
        });
        return;
    }
    let left_unmarked = diacritic_key(left);
    let right_unmarked = diacritic_key(right);
    if !left_unmarked.is_empty() && left_unmarked == right_unmarked {
        output.push(Signal {
            method: "unicode-17-diacritic-insensitive-v1",
            score: 9700,
            detail: "equivalent after Unicode decomposition and removal of combining marks".into(),
        });
        return;
    }
    let left_skeleton = skeleton(&left_unmarked);
    let right_skeleton = skeleton(&right_unmarked);
    if !left_skeleton.is_empty() && left_skeleton == right_skeleton {
        output.push(Signal {
            method: "unicode-17-skeleton-v1",
            score: 9650,
            detail:
                "equivalent UTS-39 confusable skeletons after case, width, and diacritic folding"
                    .into(),
        });
    }
}
fn diacritic_key(key: &str) -> String {
    normalize(key, Normalization::Nfd)
        .chars()
        .filter(|character| !general_category(*character).starts_with('M'))
        .collect()
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
fn fraction(numerator: usize, denominator: usize, scale: u32) -> u32 {
    if denominator == 0 {
        return 0;
    }
    u32::try_from(
        u128::try_from(numerator).unwrap_or(u128::MAX) * u128::from(scale)
            / u128::try_from(denominator).unwrap_or(u128::MAX),
    )
    .unwrap_or(scale)
}

/// Unicode-scalar Levenshtein distance with linear scratch storage.
///
/// # Errors
/// Returns governor exhaustion before starting an unaffordable matrix traversal.
pub fn levenshtein(left: &str, right: &str, governor: &mut Governor) -> Result<usize, StopReason> {
    let rows = left.chars().count();
    let columns = right.chars().count();
    governor.charge(
        Resource::Work,
        size(rows)
            .saturating_mul(size(columns))
            .max(size(rows + columns)),
    )?;
    let scratch = size(rows + columns)
        .saturating_mul(4)
        .saturating_add(size(columns + 1).saturating_mul(16));
    governor.charge(Resource::ScratchBytes, scratch)?;
    let right: Vec<_> = right.chars().collect();
    let mut previous: Vec<_> = (0..=columns).collect();
    let mut current = vec![0; columns + 1];
    for (row, character) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, &other) in right.iter().enumerate() {
            current[column + 1] = (current[column] + 1)
                .min(previous[column + 1] + 1)
                .min(previous[column] + usize::from(character != other));
        }
        core::mem::swap(&mut previous, &mut current);
    }
    let answer = previous[columns];
    governor.release_scratch(scratch)?;
    Ok(answer)
}

/// Unrestricted Damerau-Levenshtein distance, including edits around transpositions.
///
/// # Errors
/// Returns exhaustion before allocating the full distance matrix.
pub fn damerau_levenshtein(
    left: &str,
    right: &str,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let rows = left.chars().count();
    let columns = right.chars().count();
    let cells = (rows + 2)
        .checked_mul(columns + 2)
        .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes))?;
    governor.charge(Resource::Work, size(cells))?;
    // Charge at least 64-bit cells so the same request has the same governor
    // decision on native 64-bit and WASM 32-bit targets.
    let cell_bytes = core::mem::size_of::<usize>().max(8);
    let scratch = size(cells)
        .saturating_mul(size(cell_bytes))
        .saturating_add(size(rows + columns).saturating_mul(128));
    governor.charge(Resource::ScratchBytes, scratch)?;
    let left: Vec<_> = left.chars().collect();
    let right: Vec<_> = right.chars().collect();
    let width = columns + 2;
    let infinity = rows + columns;
    let mut matrix = vec![0; cells];
    matrix[0] = infinity;
    for row in 0..=rows {
        matrix[(row + 1) * width] = infinity;
        matrix[(row + 1) * width + 1] = row;
    }
    for column in 0..=columns {
        matrix[column + 1] = infinity;
        matrix[width + column + 1] = column;
    }
    let mut seen = BTreeMap::new();
    for row in 1..=rows {
        let mut matched_column = 0;
        for column in 1..=columns {
            let previous_row = seen.get(&right[column - 1]).copied().unwrap_or(0);
            let previous_column = matched_column;
            let cost = usize::from(left[row - 1] != right[column - 1]);
            if cost == 0 {
                matched_column = column;
            }
            matrix[(row + 1) * width + column + 1] = (matrix[row * width + column] + cost)
                .min(matrix[(row + 1) * width + column] + 1)
                .min(matrix[row * width + column + 1] + 1)
                .min(
                    matrix[previous_row * width + previous_column]
                        + (row - previous_row - 1)
                        + 1
                        + (column - previous_column - 1),
                );
        }
        seen.insert(left[row - 1], row);
    }
    let answer = matrix[(rows + 1) * width + columns + 1];
    governor.release_scratch(scratch)?;
    Ok(answer)
}

/// English census Soundex; non-ASCII names return no phonetic claim.
#[must_use]
pub fn soundex(input: &str) -> Option<[u8; 4]> {
    if !input.is_ascii() {
        return None;
    }
    let mut letters = input
        .bytes()
        .filter(u8::is_ascii_alphabetic)
        .map(|byte| byte.to_ascii_uppercase());
    let first = letters.next()?;
    let mut output = [first, b'0', b'0', b'0'];
    let mut index = 1;
    let mut previous = soundex_class(first);
    for letter in letters {
        if matches!(letter, b'H' | b'W') {
            continue;
        }
        let class = soundex_class(letter);
        if class != b'0' && class != previous {
            output[index] = class;
            index += 1;
            if index == 4 {
                break;
            }
        }
        previous = class;
    }
    Some(output)
}
const fn soundex_class(letter: u8) -> u8 {
    match letter {
        b'B' | b'F' | b'P' | b'V' => b'1',
        b'C' | b'G' | b'J' | b'K' | b'Q' | b'S' | b'X' | b'Z' => b'2',
        b'D' | b'T' => b'3',
        b'L' => b'4',
        b'M' | b'N' => b'5',
        b'R' => b'6',
        _ => b'0',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrestricted_transposition_and_unicode() {
        let mut governor = Governor::new(Budget::default());
        assert_eq!(damerau_levenshtein("CA", "ABC", &mut governor), Ok(2));
        assert_eq!(levenshtein("CA", "ABC", &mut governor), Ok(3));
        assert_eq!(levenshtein("猫咪", "猫猫", &mut governor), Ok(1));
        assert_eq!(damerau_levenshtein("", "abc", &mut governor), Ok(3));
    }
    #[test]
    fn exact_identity_outranks_normalization_and_ties_are_stable() {
        let candidates = [
            Candidate::new("b", "UserName"),
            Candidate::new("a", "user_name"),
            Candidate::new("exact", "user_name"),
        ];
        let result = rank("user_name", &candidates, &MatchRequest::default());
        let Outcome::Resolved(matches) = result.outcome else {
            panic!("expected ranking")
        };
        assert_eq!(matches[0].candidate.id, "a");
        assert_eq!(matches[1].candidate.id, "exact");
        assert!(matches[2].score < matches[1].score);
        assert_eq!(matches[2].candidate.text, "UserName");
    }
    #[test]
    fn census_soundex_vectors() {
        assert_eq!(soundex("Robert"), Some(*b"R163"));
        assert_eq!(soundex("Rupert"), Some(*b"R163"));
        assert_eq!(soundex("Ashcraft"), Some(*b"A261"));
        assert_eq!(soundex("Tymczak"), Some(*b"T522"));
        assert_eq!(soundex("Pfister"), Some(*b"P236"));
        assert_eq!(soundex("猫"), None);
    }
    #[test]
    fn candidate_id_and_alias_admission_precedes_ranking() {
        for candidate in [
            Candidate::new("identity", "tea"),
            Candidate {
                id: "id".into(),
                text: "tea".into(),
                aliases: vec!["alternate".into()],
            },
        ] {
            let request = MatchRequest {
                budget: Budget {
                    input_bytes: 4,
                    ..Budget::default()
                },
                ..MatchRequest::default()
            };
            let answer = rank("tea", &[candidate], &request);
            assert_eq!(answer.outcome, Outcome::Unresolved);
            assert_eq!(
                answer.receipt.stopped,
                StopReason::BudgetExceeded(Resource::InputBytes)
            );
        }
        let request = MatchRequest {
            budget: Budget {
                scratch_bytes: 127,
                ..Budget::default()
            },
            ..MatchRequest::default()
        };
        let answer = rank("tea", &[Candidate::new("id", "tea")], &request);
        assert_eq!(answer.outcome, Outcome::Unresolved);
        assert_eq!(
            answer.receipt.stopped,
            StopReason::BudgetExceeded(Resource::ScratchBytes)
        );
        assert_eq!(answer.receipt.evidence, Vec::new());
        let request = MatchRequest {
            effort: Effort::Exact,
            budget: Budget {
                scratch_bytes: 512,
                ..Budget::default()
            },
            ..MatchRequest::default()
        };
        let candidate = Candidate {
            id: "tea".into(),
            text: "tea".into(),
            aliases: vec!["a".repeat(1024)],
        };
        let answer = rank("tea", &[candidate], &request);
        assert_eq!(answer.outcome, Outcome::Unresolved);
        assert_eq!(
            answer.receipt.stopped,
            StopReason::BudgetExceeded(Resource::ScratchBytes)
        );
    }
}
