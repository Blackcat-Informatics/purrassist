// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Pure path resolution shared by host, CLI, and MCP dispatch.
use purrassist::{
    AssistResult, Outcome, Receipt,
    json::{Node, Value},
    matching::MatchRequest,
    paths::{PathIndex, SelectionPolicy},
};

pub(super) fn resolve(
    args: &Node,
    cancellation: Option<&purrassist::CancellationToken>,
) -> Result<Node, String> {
    let query = super::field(args, "query")?;
    let effort = super::effort(args)?;
    let request = MatchRequest {
        effort,
        budget: effort.budget(),
        ..MatchRequest::default()
    };
    let mut governor = super::governor(request.budget, cancellation);
    let Some(Node {
        value: Value::Array(paths),
        ..
    }) = args.get("paths")
    else {
        return Err("missing paths".into());
    };
    let context = args.get("context").and_then(Node::as_str).unwrap_or("");
    let policy = SelectionPolicy {
        minimum_score: threshold(args, "minimum_score", 7500)?,
        minimum_margin: threshold(args, "minimum_margin", 500)?,
    };
    let result = match PathIndex::new(
        paths
            .iter()
            .map(|path| path.as_str().expect("schema-validated path").to_owned()),
        &mut governor,
    ) {
        Ok(index) => index.resolve_governed(query, context, &request, policy, &mut governor),
        Err(reason) => {
            let mut receipt =
                Receipt::new(query.as_bytes(), "paths.resolve.v1", effort, request.budget);
            receipt.stopped = reason;
            receipt.usage = governor.usage();
            AssistResult {
                outcome: Outcome::Unresolved,
                receipt,
            }
        }
    };
    Ok(AssistResult {
        outcome: result.outcome.map(|value| value.to_node()),
        receipt: result.receipt,
    }
    .to_node())
}
fn threshold(args: &Node, name: &str, default: u32) -> Result<u32, String> {
    match args.get(name) {
        Some(Node {
            value: Value::Number(value),
            ..
        }) => value
            .to_usize()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("invalid {name}")),
        None => Ok(default),
        _ => Err(format!("invalid {name}")),
    }
}
