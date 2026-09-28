// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Governed embedding admission, selection, and independent alternative ledgers.
use purrassist::{
    AssistResult, Governor, Outcome, Receipt, StopReason,
    contract::{AlternativeReceipt, Evidence, Inference, InferenceKind, Resource, Validation},
    digest::Digest,
    embeddings::{Embedding, EmbeddingIndex, EmbeddingMatch, EmbeddingPolicy},
    json::{Node, Value},
    wire::{object, string},
};

pub(super) fn call(args: &Node) -> Result<Node, String> {
    let effort = super::effort(args)?;
    let mut governor = Governor::new(effort.budget());
    let bound = admission(args, &mut governor).map_err(|error| error.to_string())?;
    governor
        .charge(Resource::InputBytes, bound)
        .map_err(|error| error.to_string())?;
    governor
        .charge(Resource::ScratchBytes, bound.saturating_mul(2))
        .map_err(|error| error.to_string())?;
    let source = args.to_json();
    let mut receipt = Receipt::new(
        source.as_bytes(),
        "embeddings.match.v1",
        effort,
        effort.budget(),
    );
    let result = receipt
        .retain_source(&source, &mut governor)
        .and_then(|()| select(args, &mut governor, &mut receipt));
    let outcome = match result {
        Ok(outcome) => {
            if matches!(outcome, Outcome::Unresolved) {
                receipt.stopped = StopReason::NoCandidate;
            }
            outcome
        }
        Err(reason) => {
            receipt.stopped = reason;
            receipt.alternatives.clear();
            receipt.inferences.clear();
            receipt.evidence.clear();
            Outcome::Unresolved
        }
    };
    receipt.usage = governor.usage();
    Ok(AssistResult { outcome, receipt }.to_node())
}
fn select(
    args: &Node,
    governor: &mut Governor,
    receipt: &mut Receipt,
) -> Result<Outcome<Node>, StopReason> {
    let space = super::field(args, "space").map_err(StopReason::InvalidInput)?;
    let query = vector(args.get("query").expect("validated query"), governor)?;
    let Value::Array(candidates) = &args.get("candidates").expect("validated candidates").value
    else {
        unreachable!()
    };
    let mut entries = Vec::new();
    for candidate in candidates {
        let id = super::field(candidate, "id").map_err(StopReason::InvalidInput)?;
        governor.charge(Resource::ScratchBytes, size(id.len()).saturating_add(64))?;
        entries.push(Embedding {
            id: id.into(),
            vector: vector(candidate.get("vector").expect("validated vector"), governor)?,
        });
    }
    let policy = EmbeddingPolicy {
        minimum_score: args.get("minimum_score").map_or(Ok(0.75), number)?,
        minimum_margin: args.get("minimum_margin").map_or(Ok(0.05), number)?,
    };
    let index = EmbeddingIndex::new(space, &entries, governor)?;
    let selected = index.select(&query, policy, governor)?;
    governor.charge(Resource::EvidenceEntries, 1)?;
    governor.charge(Resource::ScratchBytes, 512)?;
    receipt.validations.push(Validation {
        validator: "embeddings.binary64.v1".into(),
        constraint: "finite nonzero equal-dimension vectors; unique identities; bounded thresholds"
            .into(),
        passed: true,
        diagnostics: Vec::new(),
    });
    match selected {
        Outcome::BestGuess(value) | Outcome::Resolved(value) => {
            let (output, evidence, inferences) = ledger(space, value, governor)?;
            receipt.evidence.push(evidence);
            receipt.inferences.extend(inferences);
            Ok(Outcome::BestGuess(output))
        }
        Outcome::Ambiguous(values) => {
            let mut outputs = Vec::new();
            let mut alternatives = Vec::new();
            for value in values {
                let (output, evidence, inferences) = ledger(space, value, governor)?;
                alternatives.push(AlternativeReceipt {
                    output: Digest::of(output.to_json().as_bytes()),
                    cost: 0,
                    evidence: vec![evidence],
                    losses: Vec::new(),
                    inferences,
                    validations: vec![0],
                });
                outputs.push(output);
            }
            receipt.alternatives = alternatives;
            Ok(Outcome::Ambiguous(outputs))
        }
        Outcome::Unresolved => Ok(Outcome::Unresolved),
    }
}
fn ledger(
    space: &str,
    value: EmbeddingMatch,
    governor: &mut Governor,
) -> Result<(Node, Evidence, Vec<Inference>), StopReason> {
    governor.charge(Resource::EvidenceEntries, 3)?;
    governor.charge(
        Resource::Work,
        size(space.len() + value.id.len())
            .saturating_mul(8)
            .saturating_add(256),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        size(space.len() + value.id.len())
            .saturating_mul(8)
            .saturating_add(2048),
    )?;
    let score = value.score.to_string();
    let evidence = Evidence {
        rule: "embeddings.cosine.binary64.v1".into(),
        explanation: format!(
            "Cosine {score} in caller-declared space {space}; JSON decimals converted to finite binary64 before normalization. Scores are not probabilities."
        ),
        span: None,
        destination: "/score".into(),
    };
    let inference = Inference {
        evidence: Evidence {
            rule: "embeddings.semantic-selection.v1".into(),
            explanation:
                "Numerical proximity proposes this identity; semantic equivalence is not validated."
                    .into(),
            span: None,
            destination: "/id".into(),
        },
        kind: InferenceKind::Guess,
        value: value.id.clone(),
    };
    let output = object([
        ("space", string(space)),
        ("id", string(value.id)),
        (
            "score",
            Node::new(Value::Number(score.parse().expect("finite cosine"))),
        ),
    ]);
    Ok((
        output,
        evidence.clone(),
        vec![
            Inference {
                evidence,
                kind: InferenceKind::Derived,
                value: score,
            },
            inference,
        ],
    ))
}
fn vector(node: &Node, governor: &mut Governor) -> Result<Vec<f64>, StopReason> {
    let Value::Array(values) = &node.value else {
        return Err(invalid("embedding must be an array"));
    };
    governor.charge(Resource::ScratchBytes, size(values.len()).saturating_mul(8))?;
    values
        .iter()
        .map(|node| {
            governor.charge(Resource::Work, 1)?;
            number(node)
        })
        .collect()
}
fn number(node: &Node) -> Result<f64, StopReason> {
    let Value::Number(number) = &node.value else {
        return Err(invalid("embedding coordinate must be numeric"));
    };
    let value: f64 = number
        .as_str()
        .parse()
        .map_err(|_| invalid("coordinate is outside binary64 range"))?;
    if !value.is_finite()
        || value == 0.0
            && number
                .as_str()
                .bytes()
                .take_while(|byte| !matches!(byte, b'e' | b'E'))
                .any(|byte| matches!(byte, b'1'..=b'9'))
    {
        return Err(invalid("coordinate overflows or underflows binary64"));
    }
    Ok(value)
}
fn admission(node: &Node, governor: &mut Governor) -> Result<u64, StopReason> {
    let size = match &node.value {
        Value::Null | Value::Bool(_) => 5,
        Value::Number(number) => size(number.as_str().len()),
        Value::String(value) => size(value.len()).saturating_mul(6).saturating_add(2),
        Value::Array(values) => {
            let mut total = 2;
            for value in values {
                total = admission(value, governor)?
                    .saturating_add(total)
                    .saturating_add(1);
            }
            return Ok(total);
        }
        Value::Object(values) => {
            let mut total = 2;
            for value in values {
                let key = size(value.key.len()).saturating_mul(6).saturating_add(4);
                governor.charge(Resource::Work, key)?;
                total = admission(&value.value, governor)?
                    .saturating_add(total)
                    .saturating_add(key);
            }
            return Ok(total);
        }
    };
    governor.charge(Resource::Work, size.saturating_add(1))?;
    Ok(size)
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
