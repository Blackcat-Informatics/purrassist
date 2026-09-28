// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Pure candidate-path resolution. Matching never opens or authorizes a file.
use crate::{
    AssistResult, Effort, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Inference, InferenceKind, Resource},
    matching::{self, Candidate, MatchRequest},
    text::comparison_key,
};
use std::collections::{BTreeMap, BTreeSet};

/// Immutable index of caller-supplied relative paths; original spelling is preserved.
#[derive(Clone, Debug)]
pub struct PathIndex {
    paths: Vec<String>,
    exact: BTreeMap<String, usize>,
    basenames: BTreeMap<String, Vec<usize>>,
}
/// Evidence-bearing path candidate. The path is a name, never an I/O capability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathMatch {
    /// Original indexed path.
    pub path: String,
    /// Combined spelling and context ranking units, not a probability.
    pub score: u32,
    /// Matching algorithms and contextual rules supporting this result.
    pub methods: Vec<String>,
}
/// Explicit acceptance thresholds for selecting among fuzzy path spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionPolicy {
    /// Minimum spelling score before parent/extension bonuses (0 through 10,000).
    pub minimum_score: u32,
    /// Alternatives closer than this many combined ranking units remain ambiguous.
    /// Equal scores always remain ambiguous, including when the margin is zero.
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
impl PathIndex {
    /// Build exact-path and normalized-basename indexes under explicit limits.
    /// Supplied identities consume input, work, and storage budgets; candidate
    /// proposals are charged when resolving, rather than during index admission.
    ///
    /// # Errors
    /// Rejects duplicate identities, absolute paths, traversal, empty components,
    /// NULs, backslashes, and exhausted construction budgets. Use `/` separators.
    pub fn new(
        paths: impl IntoIterator<Item = String>,
        governor: &mut Governor,
    ) -> Result<Self, StopReason> {
        let mut index = Self {
            paths: Vec::new(),
            exact: BTreeMap::new(),
            basenames: BTreeMap::new(),
        };
        for path in paths {
            portable(&path)?;
            governor.charge(Resource::InputBytes, size(path.len()))?;
            governor.charge(Resource::Work, size(path.len()))?;
            governor.charge(Resource::ScratchBytes, size(path.len()).saturating_mul(128))?;
            let position = index.paths.len();
            if index.exact.insert(path.clone(), position).is_some() {
                return Err(StopReason::InvalidInput("duplicate indexed path".into()));
            }
            index
                .basenames
                .entry(comparison_key(basename(&path)))
                .or_default()
                .push(position);
            index.paths.push(path);
        }
        Ok(index)
    }
    /// Number of indexed caller paths.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.paths.len()
    }
    /// Whether the index has no candidates.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
    /// Match a path using exact identity, basename spelling, and parent context.
    ///
    /// `context` is a relative directory, or an empty string for the root. Exact
    /// paths take precedence. For fuzzy resolution, every candidate is ranked;
    /// normalized basename indexes identify equivalent spellings without dropping
    /// fuzzy alternatives. Equal top scores remain ambiguous. No disk access occurs.
    #[must_use]
    pub fn resolve(
        &self,
        query: &str,
        context: &str,
        request: &MatchRequest,
    ) -> AssistResult<PathMatch> {
        self.resolve_with_policy(query, context, request, SelectionPolicy::default())
    }
    /// Resolve with explicit spelling and separation thresholds.
    ///
    /// Only a byte-exact indexed path is `Resolved`. A unique accepted fuzzy
    /// interpretation is `BestGuess`; weak matches are unsuccessful and close
    /// alternatives remain ambiguous. Thresholds are ranking policy, not calibrated
    /// probabilities. Invalid policy bounds produce an invalid-input receipt.
    #[must_use]
    pub fn resolve_with_policy(
        &self,
        query: &str,
        context: &str,
        request: &MatchRequest,
        policy: SelectionPolicy,
    ) -> AssistResult<PathMatch> {
        self.resolve_governed(
            query,
            context,
            request,
            policy,
            &mut Governor::new(request.budget),
        )
    }
    /// Resolve using a governor shared with index construction or other operations.
    ///
    /// The governor's limits are authoritative and its cumulative usage is reported.
    /// Exhaustion or cancellation returns `Unresolved`, including when an exact
    /// indexed path exists. No filesystem access or authorization is performed.
    ///
    /// ```
    /// use purrassist_core::{Governor, Outcome, matching::MatchRequest,
    ///     paths::{PathIndex, SelectionPolicy}};
    /// let request = MatchRequest::default();
    /// let mut governor = Governor::new(request.budget);
    /// let index = PathIndex::new(["src/reader.rs".into()], &mut governor).unwrap();
    /// let answer = index.resolve_governed("src/reader.rs", "", &request,
    ///     SelectionPolicy::default(), &mut governor);
    /// assert!(matches!(answer.outcome, Outcome::Resolved(_)));
    /// assert_eq!(answer.receipt.usage, governor.usage());
    /// ```
    #[must_use]
    pub fn resolve_governed(
        &self,
        query: &str,
        context: &str,
        request: &MatchRequest,
        policy: SelectionPolicy,
        governor: &mut Governor,
    ) -> AssistResult<PathMatch> {
        let mut receipt = Receipt::new(
            query.as_bytes(),
            "paths.resolve.v1",
            request.effort,
            *governor.limits(),
        );
        let result = self.resolve_with(query, context, request, policy, governor);
        let outcome = match result {
            Ok(matches) => {
                for entry in &matches {
                    let evidence = Evidence {
                        rule: "paths.rank.v1".into(),
                        explanation: format!(
                            "score {}; signals {:?}; context {context:?}; selection policy {policy:?}",
                            entry.score, entry.methods
                        ),
                        span: None,
                        destination: entry.path.clone(),
                    };
                    receipt.evidence.push(evidence.clone());
                    if entry.score != 20_000 {
                        receipt.inferences.push(Inference {
                            evidence,
                            kind: InferenceKind::Guess,
                            value: entry.path.clone(),
                        });
                    }
                }
                match matches.as_slice() {
                    [] => {
                        receipt.stopped = StopReason::NoCandidate;
                        Outcome::Unresolved
                    }
                    [answer] if answer.score == 20_000 => Outcome::Resolved(answer.clone()),
                    [answer] => Outcome::BestGuess(answer.clone()),
                    _ => Outcome::Ambiguous(matches),
                }
            }
            Err(reason) => {
                receipt.stopped = reason;
                Outcome::Unresolved
            }
        };
        receipt.usage = governor.usage();
        AssistResult { outcome, receipt }
    }
    fn resolve_with(
        &self,
        query: &str,
        context: &str,
        request: &MatchRequest,
        policy: SelectionPolicy,
        governor: &mut Governor,
    ) -> Result<Vec<PathMatch>, StopReason> {
        if policy.minimum_score > 10_000 || policy.minimum_margin > 20_000 {
            return Err(StopReason::InvalidInput(
                "path selection thresholds exceed ranking bounds".into(),
            ));
        }
        portable(query)?;
        if !context.is_empty() {
            portable(context)?;
        }
        governor.charge(Resource::InputBytes, size(query.len()))?;
        if let Some(&position) = self.exact.get(query) {
            governor.charge(Resource::Candidates, 1)?;
            governor.charge(Resource::Work, size(query.len()))?;
            governor.charge(Resource::EvidenceEntries, 1)?;
            return Ok(vec![PathMatch {
                path: self.paths[position].clone(),
                score: 20_000,
                methods: vec!["exact-path".into()],
            }]);
        }
        if request.effort == Effort::Exact {
            return Ok(Vec::new());
        }
        let normalized = comparison_key(basename(query));
        let indexed: BTreeSet<_> = self
            .basenames
            .get(&normalized)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        let storage = self
            .paths
            .iter()
            .try_fold(0_u64, |sum, path| {
                sum.checked_add(size(path.len()).saturating_mul(4).saturating_add(128))
            })
            .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes))?;
        governor.charge(Resource::ScratchBytes, storage)?;
        let candidates: Vec<_> = self
            .paths
            .iter()
            .map(|path| Candidate::new(path, basename(path)))
            .collect();
        let mut matching_request = request.clone();
        matching_request.limit = self.paths.len().max(1);
        let result =
            matching::rank_governed(basename(query), &candidates, &matching_request, governor);
        if result.receipt.stopped != StopReason::Completed
            && result.receipt.stopped != StopReason::NoCandidate
        {
            return Err(result.receipt.stopped);
        }
        let Outcome::Resolved(ranked) = result.outcome else {
            return Ok(Vec::new());
        };
        let mut results = Vec::new();
        let wanted_parent = query.rsplit_once('/').map_or(context, |(parent, _)| parent);
        for entry in ranked {
            if entry.score < policy.minimum_score {
                continue;
            }
            let path = entry.candidate.id;
            let mut methods: Vec<_> = entry
                .signals
                .iter()
                .map(|signal| signal.method.into())
                .collect();
            let mut score = entry.score;
            if parent(&path) == wanted_parent {
                score += 1000;
                methods.push("same-parent".into());
            }
            if extension(&path) == extension(query) && extension(query).is_some() {
                score += 500;
                methods.push("same-extension".into());
            }
            if self
                .exact
                .get(&path)
                .is_some_and(|position| indexed.contains(position))
            {
                methods.push("normalized-basename-index".into());
            }
            results.push(PathMatch {
                path,
                score,
                methods,
            });
        }
        results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
        if let Some(best) = results.first().map(|entry| entry.score) {
            results
                .retain(|entry| entry.score == best || best - entry.score < policy.minimum_margin);
        }
        governor.charge(Resource::EvidenceEntries, size(results.len()))?;
        Ok(results)
    }
}
fn portable(path: &str) -> Result<(), StopReason> {
    if path.is_empty()
        || path.contains(['\0', '\\', ':'])
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(StopReason::InvalidInput(
            "expected a nonempty portable relative path without traversal".into(),
        ));
    }
    Ok(())
}
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn extension(path: &str) -> Option<&str> {
    basename(path)
        .rsplit_once('.')
        .map(|(_, extension)| extension)
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
