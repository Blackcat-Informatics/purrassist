// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Deterministic evidence-bearing matching. Scores are ranking units, not probabilities.
use crate::contract::{
    AssistResult, Budget, Effort, Evidence, Governor, Outcome, Receipt, Resource, StopReason,
};
use crate::text::{comparison_key, depunctuate, lexical_tokens};
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
    rank_with(
        query,
        candidates,
        request,
        &mut Governor::new(request.budget),
    )
}

pub(crate) fn rank_with(
    query: &str,
    candidates: &[Candidate],
    request: &MatchRequest,
    governor: &mut Governor,
) -> AssistResult<Vec<RankedMatch>> {
    let mut receipt = Receipt::new(
        query.as_bytes(),
        "matching/rank-v1",
        request.effort,
        request.budget,
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
            governor.charge(Resource::Candidates, 1)?;
            governor.charge(Resource::InputBytes, size(candidate.text.len()))?;
            if !identities.insert(&candidate.id) {
                return Err(StopReason::InvalidInput(format!(
                    "duplicate candidate identity: {}",
                    candidate.id
                )));
            }
            let mut signals = signals(query, &candidate.text, request, governor)?;
            if request.effort != Effort::Exact {
                for alias in &candidate.aliases {
                    let alias_signals = signals_for_alias(query, alias, request, governor)?;
                    if let Some(score) = alias_signals.iter().map(|signal| signal.score).max() {
                        signals.push(Signal {
                            method: "caller-alias-v1",
                            score: score.min(9900),
                            detail: format!("caller supplied alias {alias:?}"),
                        });
                    }
                }
            }
            let score = signals.iter().map(|signal| signal.score).max().unwrap_or(0);
            if score > 0 {
                governor.charge(Resource::EvidenceEntries, size(signals.len()))?;
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
            }
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
    // Reserve a conservative bound for decomposition, folding, and comparison keys.
    let reservation = size(query.len().saturating_add(text.len())).saturating_mul(128);
    governor.charge(Resource::ScratchBytes, reservation)?;
    let left = comparison_key(query);
    let right = comparison_key(text);
    if left == right {
        output.push(Signal {
            method: "unicode-17-caseless-v1",
            score: 9900,
            detail: "equivalent after pinned Unicode normalization and full case folding".into(),
        });
    }
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
    governor.release_scratch(reservation)?;
    Ok(output)
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
    let scratch = size(cells)
        .saturating_mul(size(core::mem::size_of::<usize>()))
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
}
