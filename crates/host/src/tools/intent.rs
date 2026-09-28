// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Schema-validated semantic domain adapter; transport never invokes a model.
use purrassist::{
    AssistResult, Governor, Outcome, Receipt, StopReason,
    contract::Resource,
    intent::{Concept, Domain, Rule},
    json::{Node, Value},
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn resolve(
    args: &Node,
    cancellation: Option<&purrassist::CancellationToken>,
) -> Result<Node, String> {
    let input = super::field(args, "input")?;
    let effort = super::effort(args)?;
    let budget = effort.budget();
    let mut governor = super::governor(budget, cancellation);
    if let Err(reason) = admit(args, &mut governor, 0) {
        let mut receipt = Receipt::new(input.as_bytes(), "intent.resolve.v1", effort, budget);
        receipt.stopped = reason;
        receipt.usage = governor.usage();
        return Ok(AssistResult::<Node> {
            outcome: Outcome::Unresolved,
            receipt,
        }
        .to_node());
    }
    let config = args.get("domain").ok_or("missing domain")?;
    let domain = Domain {
        id: super::field(config, "id")?.into(),
        rules: values(config, "rules")?
            .iter()
            .map(|rule| {
                Ok(Rule {
                    id: super::field(rule, "id")?.into(),
                    phrase: super::field(rule, "phrase")?.into(),
                    assignments: attributes(rule, "assignments")?,
                })
            })
            .collect::<Result<_, String>>()?,
        ignored_words: if config.get("ignored_words").is_some() {
            values(config, "ignored_words")?
                .iter()
                .map(|word| {
                    word.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "ignored word must be a string".into())
                })
                .collect::<Result<_, String>>()?
        } else {
            BTreeSet::new()
        },
    };
    let concepts = values(args, "concepts")?
        .iter()
        .map(|concept| {
            Ok(Concept {
                id: super::field(concept, "id")?.into(),
                label: super::field(concept, "label")?.into(),
                attributes: attributes(concept, "attributes")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut result = domain.resolve_governed(input, &concepts, &mut governor);
    result.receipt.effort = effort;
    Ok(AssistResult {
        outcome: match result.outcome {
            Outcome::Resolved(value) => Outcome::Resolved(value.to_node()),
            Outcome::BestGuess(value) => Outcome::BestGuess(value.to_node()),
            Outcome::Ambiguous(values) => Outcome::Ambiguous(
                values
                    .iter()
                    .map(purrassist::intent::Interpretation::to_node)
                    .collect(),
            ),
            Outcome::Unresolved => Outcome::Unresolved,
        },
        receipt: result.receipt,
    }
    .to_node())
}
fn values<'a>(node: &'a Node, name: &str) -> Result<&'a [Node], String> {
    match node.get(name).map(|node| &node.value) {
        Some(Value::Array(values)) => Ok(values),
        _ => Err(format!("missing array {name}")),
    }
}
fn attributes(node: &Node, name: &str) -> Result<BTreeMap<String, String>, String> {
    let Some(Value::Object(members)) = node.get(name).map(|node| &node.value) else {
        return Err(format!("missing map {name}"));
    };
    members
        .iter()
        .map(|member| {
            Ok((
                member.key.clone(),
                member
                    .value
                    .as_str()
                    .ok_or("attribute value must be a string")?
                    .into(),
            ))
        })
        .collect()
}
fn admit(node: &Node, governor: &mut Governor, depth: u32) -> Result<(), StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(Resource::Work, 1)?;
    governor.charge(Resource::ScratchBytes, 256)?;
    match &node.value {
        Value::String(text) => {
            let size = u64::try_from(text.len()).unwrap_or(u64::MAX);
            governor.charge(Resource::InputBytes, size)?;
            governor.charge(Resource::Work, size)?;
            governor.charge(Resource::ScratchBytes, size.saturating_mul(4))?;
        }
        Value::Array(values) => {
            for value in values {
                admit(value, governor, depth + 1)?;
            }
        }
        Value::Object(members) => {
            for member in members {
                let size = u64::try_from(member.key.len()).unwrap_or(u64::MAX);
                governor.charge(Resource::Work, size)?;
                governor.charge(Resource::ScratchBytes, size.saturating_mul(4))?;
                admit(&member.value, governor, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
