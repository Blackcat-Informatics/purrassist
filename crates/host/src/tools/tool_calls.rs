// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The `recover_tool_calls` operation: one request builder shared by the
//! registry handler (MCP `tools/call` and CLI `call`) and the CLI
//! `recover-tool-calls` command, so every surface runs the same request.
use purrassist::{
    AssistResult, CancellationToken, Effort, Governor, Outcome, Receipt, StopReason,
    json::{Node, Value},
    tool_call::{
        AmbiguityPolicy, DEFAULT_ID_PREFIX, DialectId, DialectSet, DialectSetError, Effects,
        EmitTarget, GenerationStop, IdPolicy, PolicySelection, RECOVER_OPERATION, ReasoningPolicy,
        RepairPolicy, StrictForced, ToolCalls, ToolChoice, ToolRecoveryRequest, ToolSet,
        ToolSetError,
    },
};

/// Every member a `recover_tool_calls` argument object may carry.
const MEMBERS: [&str; 14] = [
    "output",
    "tools",
    "tools_format",
    "dialects",
    "stop",
    "tool_choice",
    "tool_choice_strict",
    "policy",
    "effects",
    "reasoning",
    "ambiguity",
    "ids",
    "emit",
    "effort",
];

pub(super) fn call(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let output = super::field(args, "output")?;
    Ok(recover(output.as_bytes(), args, cancellation)?.to_node())
}

/// Build the request `options` describes and recover `output` under one
/// governor shared by declaration compilation, dialect allow-list
/// construction, and recovery.
pub(super) fn recover(
    output: &[u8],
    options: &Node,
    cancellation: Option<&CancellationToken>,
) -> Result<AssistResult<ToolCalls>, String> {
    let Value::Object(members) = &options.value else {
        return Err("recover_tool_calls arguments must be an object".into());
    };
    if let Some(member) = members
        .iter()
        .find(|member| !MEMBERS.contains(&member.key.as_str()))
    {
        return Err(format!(
            "unknown recover_tool_calls argument {:?}",
            member.key
        ));
    }
    let effort = super::effort(options)?;
    let dialects = dialect_ids(options.get("dialects"))?;
    // Two phases: count the call openers of the dialects that will read the
    // output (a pure pre-scan), then size the one governor's budget from
    // them and the output length.
    let openers = dialects.as_ref().map_or_else(
        || DialectSet::built_in().call_openers(output),
        |ids| {
            ids.iter()
                .map(|dialect| dialect.grammar().call_openers(output))
                .fold(0, u64::saturating_add)
        },
    );
    let mut request = ToolRecoveryRequest::new(effort)
        .budget(ToolRecoveryRequest::sized_budget(
            effort,
            output.len(),
            openers,
        ))
        .stop(stop(options.get("stop"))?)
        .tool_choice(tool_choice(
            options.get("tool_choice"),
            options.get("tool_choice_strict"),
        )?)
        .policy(policy(options.get("policy"))?)
        .reasoning(reasoning(options.get("reasoning"))?)
        .ambiguity(ambiguity(options.get("ambiguity"))?)
        .ids(ids(options.get("ids"))?)
        .emit(emit(options.get("emit"))?);
    for (tool, effects) in effects(options.get("effects"))? {
        request = request.effects(tool, effects);
    }
    let declarations = declarations(options.get("tools").ok_or("missing argument tools")?);
    let format = text(options.get("tools_format"), "tools_format")?.unwrap_or("openai");
    let mut governor = super::governor(request.budget, cancellation);
    let compiled = match format {
        "openai" => ToolSet::from_openai_governed(&declarations, &mut governor),
        "mcp" => ToolSet::from_mcp_governed(&declarations, &mut governor),
        "anthropic" => ToolSet::from_anthropic_governed(&declarations, &mut governor),
        other => return Err(format!("unknown tools_format {other:?}")),
    };
    let tools = match compiled {
        Ok(tools) => tools,
        Err(ToolSetError::Resource(reason)) => {
            return Ok(stopped(output, effort, reason, &governor));
        }
        Err(error) => return Err(error.to_string()),
    };
    if let Some(dialects) = dialects {
        match DialectSet::new(dialects, &mut governor) {
            Ok(set) => request = request.dialects(set),
            Err(DialectSetError::Resource(reason)) => {
                return Ok(stopped(output, effort, reason, &governor));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(tools.recover_governed(output, &request, &mut governor))
}

/// The declaration text the adapters read, whose exact bytes their
/// render-back and schema digests refer to. A `tools` string is the
/// caller's raw declaration JSON, passed through byte for byte (the CLI
/// passes its `--tools` file this way, and MCP callers may too). A
/// structured `tools` value (an array or object, which JSON-RPC has already
/// parsed) is written back compactly: its member order, duplicate members
/// and number spellings (`1.50e3` stays `1.50e3`) are kept, but the
/// caller's whitespace and string escape spellings are not.
fn declarations(tools: &Node) -> String {
    match &tools.value {
        Value::String(text) => text.clone(),
        _ => tools.to_json(),
    }
}

/// The `Unresolved` result of a stop before recovery began: the receipt
/// names the output's digest and records the governor's limits and usage.
fn stopped(
    output: &[u8],
    effort: Effort,
    reason: StopReason,
    governor: &Governor,
) -> AssistResult<ToolCalls> {
    let mut receipt = Receipt::new(output, RECOVER_OPERATION, effort, *governor.limits());
    receipt.stopped = reason;
    receipt.usage = governor.usage();
    AssistResult {
        outcome: Outcome::Unresolved,
        receipt,
    }
}

fn stop(node: Option<&Node>) -> Result<GenerationStop, String> {
    let Some(node) = node else {
        return Ok(GenerationStop::Stop);
    };
    if let Some(text) = node.as_str() {
        return match text {
            "stop" => Ok(GenerationStop::Stop),
            "length" => Ok(GenerationStop::Length),
            "tool_calls" => Ok(GenerationStop::ToolCalls),
            "content_filter" => Ok(GenerationStop::ContentFilter),
            "unknown" => Ok(GenerationStop::Unknown),
            other => Err(format!("unknown stop {other:?}")),
        };
    }
    match &node.value {
        Value::Object(members) if members.len() == 1 && members[0].key == "stop_sequence" => {
            members[0]
                .value
                .as_str()
                .map(|sequence| GenerationStop::StopSequence(sequence.as_bytes().to_vec()))
                .ok_or_else(|| "stop_sequence must be a string".into())
        }
        _ => Err("stop must be a stop name or {\"stop_sequence\": STRING}".into()),
    }
}

/// The tool choice, with the strict options applied to a forced choice.
///
/// `strict` is `{exactly_one, allow_content, allow_reasoning}` (all
/// booleans, all required). It is an error beside a choice that is not
/// forced, and `exactly_one: false` is an error beside an Anthropic
/// `disable_parallel_tool_use: true`, which it contradicts.
fn tool_choice(node: Option<&Node>, strict: Option<&Node>) -> Result<ToolChoice, String> {
    let choice = base_choice(node)?;
    let Some(strict) = strict else {
        return Ok(choice);
    };
    let Value::Object(members) = &strict.value else {
        return Err("tool_choice_strict must be an object".into());
    };
    if let Some(member) = members.iter().find(|member| {
        !["exactly_one", "allow_content", "allow_reasoning"].contains(&member.key.as_str())
    }) {
        return Err(format!(
            "tool_choice_strict has an unknown member {:?}",
            member.key
        ));
    }
    let flag = |key: &str| -> Result<bool, String> {
        match strict.get(key).map(|node| &node.value) {
            Some(Value::Bool(flag)) => Ok(*flag),
            Some(_) => Err(format!("tool_choice_strict.{key} must be a boolean")),
            None => Err(format!("tool_choice_strict.{key} is required")),
        }
    };
    let (exactly_one, allow_content, allow_reasoning) = (
        flag("exactly_one")?,
        flag("allow_content")?,
        flag("allow_reasoning")?,
    );
    let name = match choice {
        ToolChoice::Forced(name) => name,
        ToolChoice::ForcedStrict(existing) => {
            if existing.exactly_one && !exactly_one {
                return Err(
                    "tool_choice_strict.exactly_one is false, but the Anthropic tool_choice \
                     sets disable_parallel_tool_use: true, which means exactly one call"
                        .into(),
                );
            }
            existing.name
        }
        _ => return Err("tool_choice_strict requires a forced tool_choice".into()),
    };
    Ok(ToolChoice::ForcedStrict(StrictForced {
        name,
        exactly_one,
        allow_content,
        allow_reasoning,
    }))
}

/// An `OpenAI` choice (any string, or an object whose `type` is
/// `"function"`), otherwise an Anthropic choice.
fn base_choice(node: Option<&Node>) -> Result<ToolChoice, String> {
    let Some(node) = node else {
        return Ok(ToolChoice::Auto);
    };
    let anthropic = matches!(node.value, Value::Object(_))
        && node.get("type").and_then(Node::as_str) != Some("function");
    if anthropic {
        ToolChoice::from_anthropic(node)
    } else {
        ToolChoice::from_openai(node)
    }
    .map_err(|error| error.to_string())
}

fn named<T: Copy>(
    node: Option<&Node>,
    name: &str,
    default: T,
    choices: &[(&str, T)],
) -> Result<T, String> {
    let Some(node) = node else {
        return Ok(default);
    };
    let text = node
        .as_str()
        .ok_or_else(|| format!("{name} must be a string"))?;
    choices
        .iter()
        .find(|(choice, _)| *choice == text)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("unknown {name} {text:?}"))
}

fn policy(node: Option<&Node>) -> Result<PolicySelection, String> {
    named(
        node,
        "policy",
        PolicySelection::PerTool,
        &[
            ("per-tool", PolicySelection::PerTool),
            ("structural", PolicySelection::All(RepairPolicy::Structural)),
            ("tolerant", PolicySelection::All(RepairPolicy::Tolerant)),
        ],
    )
}

fn reasoning(node: Option<&Node>) -> Result<ReasoningPolicy, String> {
    named(
        node,
        "reasoning",
        ReasoningPolicy::default(),
        &[
            ("separate", ReasoningPolicy::Separate { starts_open: false }),
            (
                "separate-open",
                ReasoningPolicy::Separate { starts_open: true },
            ),
            ("content", ReasoningPolicy::AsContent),
        ],
    )
}

fn ambiguity(node: Option<&Node>) -> Result<AmbiguityPolicy, String> {
    named(
        node,
        "ambiguity",
        AmbiguityPolicy::Report,
        &[
            ("report", AmbiguityPolicy::Report),
            ("reject", AmbiguityPolicy::Reject),
        ],
    )
}

fn effects(node: Option<&Node>) -> Result<Vec<(String, Effects)>, String> {
    let Some(node) = node else {
        return Ok(Vec::new());
    };
    let Value::Object(members) = &node.value else {
        return Err("effects must be an object".into());
    };
    let mut overrides: Vec<(String, Effects)> = Vec::with_capacity(members.len());
    for member in members {
        if overrides.iter().any(|(tool, _)| *tool == member.key) {
            return Err(format!("effects names tool {:?} twice", member.key));
        }
        let effects = named(
            Some(&member.value),
            "effects",
            Effects::Mutating,
            &[
                ("read-only", Effects::ReadOnly),
                ("mutating", Effects::Mutating),
            ],
        )?;
        overrides.push((member.key.clone(), effects));
    }
    Ok(overrides)
}

fn ids(node: Option<&Node>) -> Result<IdPolicy, String> {
    let Some(node) = node else {
        return Ok(IdPolicy::default());
    };
    match &node.value {
        Value::String(text) if text == "source" => Ok(IdPolicy::Source),
        Value::Object(members) => {
            let (mut prefix, mut start) = (DEFAULT_ID_PREFIX.to_owned(), 0);
            for member in members {
                match (member.key.as_str(), &member.value.value) {
                    ("prefix", Value::String(text)) => text.clone_into(&mut prefix),
                    ("start", Value::Number(number)) => {
                        start = number
                            .to_usize()
                            .and_then(|value| u64::try_from(value).ok())
                            .ok_or("ids start must be a non-negative integer")?;
                    }
                    (key, _) => return Err(format!("invalid ids member {key:?}")),
                }
            }
            Ok(IdPolicy::Prefix { prefix, start })
        }
        _ => Err("ids must be \"source\" or {\"prefix\": STRING, \"start\": INTEGER}".into()),
    }
}

fn dialect(id: &str) -> Result<DialectId, String> {
    DialectId::from_id(id).ok_or_else(|| format!("unknown dialect {id:?}"))
}

fn dialect_ids(node: Option<&Node>) -> Result<Option<Vec<DialectId>>, String> {
    let Some(node) = node else {
        return Ok(None);
    };
    let Value::Array(items) = &node.value else {
        return Err("dialects must be an array of dialect ids".into());
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or_else(|| "dialects must be an array of dialect ids".to_owned())
                .and_then(dialect)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn emit(node: Option<&Node>) -> Result<EmitTarget, String> {
    match text(node, "emit")? {
        None | Some("none") => Ok(EmitTarget::None),
        Some("openai") => Ok(EmitTarget::OpenAiToolCalls),
        Some(id) => dialect(id).map(EmitTarget::Dialect),
    }
}

fn text<'a>(node: Option<&'a Node>, name: &str) -> Result<Option<&'a str>, String> {
    node.map_or(Ok(None), |node| {
        node.as_str()
            .map(Some)
            .ok_or_else(|| format!("{name} must be a string"))
    })
}
