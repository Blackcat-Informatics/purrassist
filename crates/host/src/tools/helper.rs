// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `prepare_helper`/`complete_helper`: a production tool surface over
//! [`crate::helpers::Pending`], previously reachable only from tests and
//! doctests. `prepare_helper` performs no network I/O; it returns the exact
//! request the caller's own transport must send, plus a tamper-evident,
//! versioned, opaque `pending` string. `complete_helper` decodes that string,
//! re-derives the identical `Pending` value it was minted from (deterministic
//! given the same provider/model/input/schema/token limit), and resumes it
//! with the caller-supplied HTTP status and body. Each call charges exactly
//! one caller-owned governor; no state survives between the two calls except
//! the opaque string itself.
use crate::helpers::{Pending, Provider};
use purrassist::{
    Budget, CancellationToken, Schema,
    digest::Digest,
    json::{self, Node, Value},
    wire::{array, integer, object, string},
};

/// `pending` envelope format version. Bumped whenever a field is added,
/// removed, or reinterpreted; [`decode_pending`] rejects any other value.
const PENDING_VERSION: u64 = 1;

/// Every other tool leaves `Budget::default()`'s `helper_calls: 0` in place
/// (helpers are disabled unless a caller explicitly budgets one); calling
/// `prepare_helper`/`complete_helper` *is* that explicit budgeting decision,
/// so each call is admitted exactly one.
const fn helper_budget() -> Budget {
    Budget {
        helper_calls: 1,
        ..purrassist::Effort::Standard.budget()
    }
}

pub(super) fn prepare(
    args: &Node,
    cancellation: Option<&CancellationToken>,
) -> Result<Node, String> {
    let provider = provider_of(super::field(args, "provider")?)?;
    let model = super::field(args, "model")?;
    let input = super::field(args, "input")?;
    let schema_text = args.get("schema").ok_or("missing schema")?.to_json();
    let max_output_tokens = super::unsigned(args, "max_output_tokens")?;
    let mut governor = super::governor(helper_budget(), cancellation);
    let schema =
        Schema::compile_governed(&schema_text, &mut governor).map_err(|e| e.to_string())?;
    let pending = Pending::prepare(
        provider,
        model,
        input,
        &schema,
        max_output_tokens,
        &mut governor,
    )
    .map_err(|e| e.to_string())?;
    let request = pending.request();
    let envelope = encode_pending(provider, model, input, &schema_text, max_output_tokens);
    let mut receipt = purrassist::Receipt::new(
        input.as_bytes(),
        "helper.prepare.v1",
        purrassist::Effort::Standard,
        Budget::default(),
    );
    receipt.usage = governor.usage();
    Ok(object([
        ("provider", string(provider_name(provider))),
        ("path", string(request.path)),
        (
            "headers",
            array(
                request.headers.iter().map(|(name, value)| {
                    object([("name", string(name)), ("value", string(value))])
                }),
            ),
        ),
        ("body", string(&request.body)),
        ("pending", string(envelope.to_json())),
        ("receipt", receipt.to_node()),
    ]))
}

pub(super) fn complete(
    args: &Node,
    cancellation: Option<&CancellationToken>,
) -> Result<Node, String> {
    let pending_text = super::field(args, "pending")?;
    let status = super::unsigned(args, "status")?;
    let status =
        u16::try_from(status).map_err(|_| "status must fit an HTTP status code".to_string())?;
    let body = super::field(args, "body")?;
    let (provider, model, original, schema_text, max_output_tokens) = decode_pending(pending_text)?;
    let mut governor = super::governor(helper_budget(), cancellation);
    let schema =
        Schema::compile_governed(&schema_text, &mut governor).map_err(|e| e.to_string())?;
    let pending = Pending::prepare(
        provider,
        &model,
        &original,
        &schema,
        max_output_tokens,
        &mut governor,
    )
    .map_err(|e| e.to_string())?;
    Ok(pending.complete(status, body).to_node())
}

/// Serialize a tamper-evident, versioned envelope: `payload` binds every
/// field [`Pending::prepare`] needs to deterministically reconstruct the
/// exact same pending request, and `digest` binds `payload`'s exact JSON
/// bytes so [`decode_pending`] can detect a corrupted or substituted string
/// before trusting any of its fields.
fn encode_pending(
    provider: Provider,
    model: &str,
    original: &str,
    schema_text: &str,
    max_output_tokens: u64,
) -> Node {
    let payload = object([
        ("provider", string(provider_name(provider))),
        ("model", string(model)),
        ("original", string(original)),
        ("schema", string(schema_text)),
        ("max_output_tokens", integer(max_output_tokens)),
    ]);
    let digest = Digest::of(payload.to_json().as_bytes());
    object([
        ("version", integer(PENDING_VERSION)),
        ("digest", string(digest.to_string())),
        ("payload", payload),
    ])
}

/// Decode and verify an [`encode_pending`] envelope.
///
/// # Errors
/// Rejects malformed JSON, an unsupported `version`, a missing field, and a
/// `digest` that does not match the recomputed digest of `payload`'s exact
/// JSON bytes (a corrupted or substituted `pending` string).
fn decode_pending(text: &str) -> Result<(Provider, String, String, String, u64), String> {
    let envelope = json::parse(text).map_err(|e| e.to_string())?;
    let expected_version = usize::try_from(PENDING_VERSION).unwrap_or(usize::MAX);
    match envelope.get("version").map(|node| &node.value) {
        Some(Value::Number(number)) if number.to_usize() == Some(expected_version) => {}
        _ => return Err("unsupported pending-state version".into()),
    }
    let payload = envelope
        .get("payload")
        .ok_or("malformed pending state: missing payload")?;
    let expected = super::field(&envelope, "digest")?;
    if Digest::of(payload.to_json().as_bytes()).to_string() != expected {
        return Err("pending state failed tamper verification".into());
    }
    let provider = provider_of(super::field(payload, "provider")?)?;
    let model = super::field(payload, "model")?.to_string();
    let original = super::field(payload, "original")?.to_string();
    let schema_text = super::field(payload, "schema")?.to_string();
    let max_output_tokens = super::unsigned(payload, "max_output_tokens")?;
    Ok((provider, model, original, schema_text, max_output_tokens))
}

fn provider_of(name: &str) -> Result<Provider, String> {
    match name {
        "openai_responses" => Ok(Provider::OpenAiResponses),
        "anthropic_messages" => Ok(Provider::AnthropicMessages),
        "chat_completions" => Ok(Provider::ChatCompletions),
        _ => Err(format!("unknown helper provider {name:?}")),
    }
}
const fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::OpenAiResponses => "openai_responses",
        Provider::AnthropicMessages => "anthropic_messages",
        Provider::ChatCompletions => "chat_completions",
    }
}
