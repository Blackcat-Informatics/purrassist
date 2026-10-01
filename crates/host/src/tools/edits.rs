// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Decode selected plans and expose governed buffer application without file effects.
use purrassist::contract::saturating_size as size;
use purrassist::{
    AssistResult, CancellationToken, Governor, Outcome, Receipt, StopReason,
    contract::{Evidence, Resource},
    digest::Digest,
    edits::{AppliedEdit, EditPlan, TextEdit},
    json::{Node, Value},
    patch::Patch,
    syntax::{self, Language},
};

pub(super) fn apply(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let source = super::field(args, "source")?;
    let effort = super::effort(args)?;
    let mut governor = super::governor(effort.budget(), cancellation);
    let mut transformation = None;
    let plan = match decode_plan_rebased(
        source,
        args.get("plan").ok_or("missing plan")?,
        args.get("rebase_from"),
        &mut governor,
        &mut transformation,
    ) {
        Ok(plan) => plan,
        Err(reason) => {
            let mut receipt =
                Receipt::new(source.as_bytes(), "text.apply.v1", effort, effort.budget());
            receipt.stopped = receipt
                .retain_source(source, &mut governor)
                .err()
                .unwrap_or(reason);
            receipt.usage = governor.usage();
            return Ok(AssistResult::<Node> {
                outcome: Outcome::Unresolved,
                receipt,
            }
            .to_node());
        }
    };
    let language = match args
        .get("language")
        .and_then(Node::as_str)
        .unwrap_or("text")
    {
        "text" => None,
        "python" => Some(Language::Python),
        "rust" => Some(Language::Rust),
        "javascript" => Some(Language::JavaScript),
        "typescript" => Some(Language::TypeScript),
        "tsx" => Some(Language::Tsx),
        _ => return Err("unsupported language".into()),
    };
    let mut result = if let Some(language) = language {
        syntax::apply_checked_governed(source, &plan, language, &mut governor)
    } else {
        plan.apply_governed(source, &mut governor)
    };
    if let Err(reason) = admit_output(&result.outcome, source, &mut governor) {
        result.outcome = Outcome::Unresolved;
        result.receipt.stopped = reason;
    }
    if let Some(transformation) = transformation {
        result.receipt.evidence.insert(0, transformation);
    }
    result.receipt.usage = governor.usage();
    result.receipt.effort = effort;
    Ok(AssistResult {
        outcome: result.outcome.map(|value| applied(&value)),
        receipt: result.receipt,
    }
    .to_node())
}

pub(super) fn decode_plan(
    source: &str,
    plan: &Node,
    governor: &mut Governor,
) -> Result<EditPlan, StopReason> {
    governor.charge(Resource::InputBytes, size(source.len()))?;
    governor.charge(Resource::Work, size(source.len()))?;
    let digest = Digest::of(source.as_bytes());
    if super::field(plan, "source").map_err(StopReason::InvalidInput)? != digest.to_string() {
        return Err(StopReason::InvalidInput(
            "stale edit plan: source digest changed".into(),
        ));
    }
    let Some(Node {
        value: Value::Array(values),
        ..
    }) = plan.get("edits")
    else {
        return Err(StopReason::InvalidInput("plan must contain edits".into()));
    };
    governor.charge(
        Resource::ScratchBytes,
        size(values.len()).saturating_mul(128),
    )?;
    let mut edits = Vec::with_capacity(values.len());
    for value in values {
        governor.charge(Resource::Work, 1)?;
        let replacement = super::field(value, "replacement").map_err(StopReason::InvalidInput)?;
        governor.charge(Resource::ScratchBytes, size(replacement.len()))?;
        governor.charge(Resource::Work, size(replacement.len()))?;
        edits.push(TextEdit {
            range: offset(value, "start")?..offset(value, "end")?,
            replacement: replacement.into(),
        });
    }
    Ok(EditPlan {
        source: digest,
        edits,
    })
}

/// Decode a plan exactly as [`decode_plan`] does when its declared source
/// digest still matches `source`. Otherwise, if `rebase_from` supplies the
/// exact prior text the plan was made against, diff that prior text against
/// `source` into an intervening [`Patch`], and governed-[`Patch::rebase`]
/// the plan onto it: a plan whose edits commute with the intervening change
/// is rebased (recording `*transformation`), and one that does not is
/// refused as stale, exactly as an unrebased stale plan already is.
///
/// # Errors
/// Returns the same stale/malformed-plan refusals as [`decode_plan`], plus
/// a stale refusal when `rebase_from` is absent, does not match the plan's
/// declared digest, or when the rebase itself refuses a non-commuting
/// intervening change. Resource exhaustion and cancellation propagate.
pub(super) fn decode_plan_rebased(
    source: &str,
    plan: &Node,
    rebase_from: Option<&Node>,
    governor: &mut Governor,
    transformation: &mut Option<Evidence>,
) -> Result<EditPlan, StopReason> {
    let digest = Digest::of(source.as_bytes());
    let declared = super::field(plan, "source").map_err(StopReason::InvalidInput)?;
    if declared == digest.to_string() {
        return decode_plan(source, plan, governor);
    }
    let Some(rebase_from) = rebase_from else {
        return Err(StopReason::InvalidInput(
            "stale edit plan: source digest changed".into(),
        ));
    };
    let prior = super::field(rebase_from, "source").map_err(StopReason::InvalidInput)?;
    let prior_digest = Digest::of(prior.as_bytes());
    if declared != prior_digest.to_string() {
        return Err(StopReason::InvalidInput(
            "stale edit plan: plan source does not match rebase_from source".into(),
        ));
    }
    let stale_plan = decode_plan(prior, plan, governor)?;
    let intervening = Patch::diff(prior, source, governor)?;
    let rebased = stale_plan.rebase(&intervening, prior, governor)?;
    *transformation = Some(rebased.transformation);
    Ok(rebased.patch)
}

fn offset(value: &Node, name: &str) -> Result<usize, StopReason> {
    match value.get(name).map(|node| &node.value) {
        Some(Value::Number(number)) => number.to_usize(),
        _ => None,
    }
    .ok_or_else(|| StopReason::InvalidInput(format!("edit {name} exceeds address space")))
}

fn admit_output(
    outcome: &Outcome<AppliedEdit>,
    source: &str,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let values = match outcome {
        Outcome::Resolved(value) | Outcome::BestGuess(value) => std::slice::from_ref(value),
        Outcome::Ambiguous(values) => values.as_slice(),
        Outcome::Unresolved => &[],
    };
    let mut bytes = size(source.len());
    let mut entries = 0_u64;
    for value in values {
        bytes = bytes.saturating_add(size(value.text.len()));
        entries = entries.saturating_add(size(value.inverse.edits.len()));
        for edit in &value.inverse.edits {
            bytes = bytes.saturating_add(size(edit.replacement.len()));
        }
    }
    governor.charge(
        Resource::ScratchBytes,
        bytes
            .saturating_mul(4)
            .saturating_add(entries.saturating_mul(2048))
            .saturating_add(4096),
    )?;
    governor.charge(
        Resource::Work,
        bytes.saturating_add(entries.saturating_mul(16)),
    )
}

fn applied(value: &AppliedEdit) -> Node {
    value.to_node()
}
