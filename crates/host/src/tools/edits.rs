// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Decode selected plans and expose governed buffer application without file effects.
use purrassist::{
    AssistResult, CancellationToken, Governor, Outcome, Receipt, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{AppliedEdit, EditPlan, TextEdit},
    json::{Node, Value},
    syntax::{self, Language},
};

pub(super) fn apply(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let source = super::field(args, "source")?;
    let effort = super::effort(args)?;
    let mut governor = super::governor(effort.budget(), cancellation);
    let plan = match decode_plan(
        source,
        args.get("plan").ok_or("missing plan")?,
        &mut governor,
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
    result.receipt.usage = governor.usage();
    result.receipt.effort = effort;
    Ok(AssistResult {
        outcome: match &result.outcome {
            Outcome::Resolved(value) => Outcome::Resolved(applied(value)),
            Outcome::BestGuess(value) => Outcome::BestGuess(applied(value)),
            Outcome::Ambiguous(values) => Outcome::Ambiguous(values.iter().map(applied).collect()),
            Outcome::Unresolved => Outcome::Unresolved,
        },
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
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
