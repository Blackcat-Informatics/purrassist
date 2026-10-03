// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use purrassist::{
    Effort, Outcome, RecoveryRequest, Schema,
    json::{self, Member, Node, Value},
    matching::{self, Candidate, MatchRequest},
    tool_call::{DialectId, ToolCalls},
    wire::{array, integer, object, string},
};
use std::io::{self, Read};

const MAX_INPUT: u64 = 16 * 1024 * 1024;
const HELP: &str = "PurRAssist: evidence-bearing recovery for imperfect AI output\n\nUsage:\n  purrassist decode --schema FILE [--input FILE] [--effort exact|quick|standard|deep|maximum] [--no-guesses]\n  purrassist validate --schema FILE [--input FILE]\n  purrassist match QUERY CANDIDATE...\n  purrassist recover-tool-calls --tools FILE [--tools-format openai|mcp|anthropic] [--dialect ID]... [--stop stop|length|tool_calls|content_filter|unknown|stop-sequence=STR] [--tool-choice auto|none|required|NAME] [--policy per-tool|structural|tolerant] [--effects NAME=read-only|mutating]... [--reasoning separate|separate-open|content] [--ambiguity report|reject] [--emit openai|DIALECT] [--ids-source | [--ids-prefix P] [--ids-start N]] [--effort exact|quick|standard|deep|maximum] [FILE]\n  purrassist call TOOL --root DIRECTORY\n  purrassist mcp --root DIRECTORY\n\nInput defaults to stdin. Decode emits a JSON answer and receipt.\nrecover-tool-calls reads one model turn as raw bytes (invalid UTF-8 is a recorded loss) and\nemits the recover_tool_calls tool's result; built-in dialect ids for --dialect (default all)\nand --emit: {dialects}. Call markup no allowed dialect reads\n(a family not built in yet, or a built-in dialect left out of --dialect) is a rejected attempt\nwith reason unsupported_dialect, its bytes stay content, and the finish is unresolved. Call ids by default keep each id the model wrote\nand number every other call call_<12 hex of the output's SHA-256>_<index>, so\nturns never share ids (a missing or repeated model id is a recorded inference);\n--ids-source keeps the ids the output carries (deduplicated as ID#INDEX);\n--ids-prefix P/--ids-start N number every call P(N+index) (default prefix call_,\nstart 0; pass the result's next_id_start as the next round's --ids-start).\nExit: 0 resolved/best-guess, 3 ambiguous, 4 unresolved/invalid, 1 invocation error.\nrecover-tool-calls exits 0 when at least one call is usable and the tool-choice check passed\n(even if other attempts were rejected or unparsed; the JSON itemizes them) or for clean\ncontent_only; 3 when nothing is usable and a call or the operation is ambiguous; 4 when\nnothing is usable (unresolved calls, rejected or unparsed markup only, truncated, only\nunusable guesses, an exhausted budget or invalid input) or the tool-choice check failed,\neven with usable calls. The JSON output is the same either way.\n";

/// The help text, with the built-in dialect ids taken from
/// `DialectId::BUILT_IN` so the list cannot drift from the shipped set.
fn help() -> String {
    let ids: Vec<&str> = DialectId::BUILT_IN.iter().map(DialectId::id).collect();
    HELP.replace("{dialects}", &ids.join(", "))
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "binary entry point requires explicit parent visibility without public export"
)]
pub(super) fn run(args: &[String]) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => {
            print!("{}", help());
            Ok(0)
        }
        Some("--version") => {
            println!("purrassist {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some("decode" | "validate") => decode(args),
        Some("match") => rank(&args[1..]),
        Some("recover-tool-calls") => recover_tool_calls(&args[1..]),
        Some("call") => call(&args[1..]),
        Some("mcp") => mcp(&args[1..]),
        Some(command) => Err(format!("unknown command {command:?}; use --help")),
    }
}
fn read_input(path: Option<&str>) -> Result<String, String> {
    String::from_utf8(read_bytes(path)?).map_err(|e| format!("input is not UTF-8: {e}"))
}
fn read_bytes(path: Option<&str>) -> Result<Vec<u8>, String> {
    let mut reader: Box<dyn Read> = if let Some(path) = path {
        Box::new(std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?)
    } else {
        Box::new(io::stdin())
    };
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INPUT {
        return Err("input exceeds 16 MiB admission limit".into());
    }
    Ok(bytes)
}
fn decode(args: &[String]) -> Result<u8, String> {
    let mut schema_path = None;
    let mut input_path = None;
    let mut request = RecoveryRequest::default();
    let mut position = 1;
    while position < args.len() {
        let option = &args[position];
        if option == "--no-guesses" {
            request.allow_guesses = false;
            position += 1;
            continue;
        }
        let value = args
            .get(position + 1)
            .ok_or_else(|| format!("{option} requires a value"))?;
        match option.as_str() {
            "--schema" if schema_path.is_none() => schema_path = Some(value.as_str()),
            "--input" if input_path.is_none() => input_path = Some(value.as_str()),
            "--effort" => request.effort = effort(value)?,
            _ => return Err(format!("unknown or repeated option {option:?}")),
        }
        position += 2;
    }
    let source = read_input(Some(schema_path.ok_or("--schema is required")?))?;
    let schema = Schema::compile(&source).map_err(|e| e.to_string())?;
    let input = read_input(input_path)?;
    if args[0] == "validate" {
        // Emit exactly the `validate_json` tool's output: the schema
        // document and instance are decoded fresh here (rather than reusing
        // the `schema` compiled above only to check for a parse error early)
        // so this command's output is identical to calling that tool.
        let schema_document = json::parse(&source).map_err(|e| e.to_string())?;
        let instance = json::parse(&input).map_err(|e| e.to_string())?;
        let result = purrassist_host::tools::validate_json(&object([
            ("schema", schema_document),
            ("instance", instance),
        ]))?;
        println!("{}", result.to_json());
        let valid = matches!(
            result.get("valid").map(|node| &node.value),
            Some(Value::Bool(true))
        );
        return Ok(if valid { 0 } else { 4 });
    }
    let result = schema.recover(&input, request);
    println!("{}", result.to_node().to_json());
    Ok(exit_code(&result.outcome))
}
/// The documented exit code of a recovery outcome: 0 resolved or best
/// guess, 3 ambiguous, 4 unresolved.
const fn exit_code<T>(outcome: &Outcome<T>) -> u8 {
    match outcome {
        Outcome::Resolved(_) | Outcome::BestGuess(_) => 0,
        Outcome::Ambiguous(_) => 3,
        Outcome::Unresolved => 4,
    }
}
fn effort(value: &str) -> Result<Effort, String> {
    match value {
        "exact" => Ok(Effort::Exact),
        "quick" => Ok(Effort::Quick),
        "standard" => Ok(Effort::Standard),
        "deep" => Ok(Effort::Deep),
        "maximum" => Ok(Effort::Maximum),
        _ => Err(format!("unknown effort {value:?}")),
    }
}
fn rank(args: &[String]) -> Result<u8, String> {
    let query = args
        .first()
        .ok_or("match requires a query and candidates")?;
    if args.len() < 2 {
        return Err("match requires at least one candidate".into());
    }
    let candidates: Vec<_> = args[1..]
        .iter()
        .enumerate()
        .map(|(index, text)| Candidate::new(index.to_string(), text))
        .collect();
    let result = matching::rank(query, &candidates, &MatchRequest::default());
    let ranked = match &result.outcome {
        Outcome::Resolved(items) | Outcome::BestGuess(items) => items.as_slice(),
        _ => &[],
    };
    println!(
        "{}",
        object([
            (
                "matches",
                array(ranked.iter().map(|item| object([
                    ("id", string(&item.candidate.id)),
                    ("text", string(&item.candidate.text)),
                    ("score", integer(u64::from(item.score))),
                    (
                        "signals",
                        array(item.signals.iter().map(|signal| object([
                            ("method", string(signal.method)),
                            ("score", integer(u64::from(signal.score))),
                            ("detail", string(&signal.detail)),
                        ])))
                    ),
                ])))
            ),
            ("receipt", result.receipt.to_node()),
        ])
        .to_json()
    );
    Ok(if ranked.is_empty() { 4 } else { 0 })
}

/// `recover-tool-calls`: translate options into the `recover_tool_calls`
/// argument object and run the host's shared request builder on the raw
/// output bytes.
fn recover_tool_calls(args: &[String]) -> Result<u8, String> {
    let mut options: Vec<(&'static str, Node)> = Vec::new();
    let mut tools_path = None;
    let mut output_path = None;
    let mut dialects = Vec::new();
    let mut effects: Vec<(String, Node)> = Vec::new();
    let (mut ids_prefix, mut ids_start, mut ids_source) = (None, None, false);
    let mut position = 0;
    while position < args.len() {
        let option = args[position].as_str();
        if !option.starts_with("--") {
            if output_path.is_some() {
                return Err(format!("unexpected argument {option:?}"));
            }
            output_path = Some(option);
            position += 1;
            continue;
        }
        if option == "--ids-source" {
            if ids_source {
                return Err(format!("unknown or repeated option {option:?}"));
            }
            ids_source = true;
            position += 1;
            continue;
        }
        let value = args
            .get(position + 1)
            .ok_or_else(|| format!("{option} requires a value"))?
            .as_str();
        let single = |key: &'static str, node: Node, options: &mut Vec<(&'static str, Node)>| {
            if options.iter().any(|(existing, _)| *existing == key) {
                return Err(format!("unknown or repeated option {option:?}"));
            }
            options.push((key, node));
            Ok(())
        };
        match option {
            "--tools" if tools_path.is_none() => tools_path = Some(value),
            "--tools-format" => single("tools_format", string(value), &mut options)?,
            "--dialect" => dialects.push(string(value)),
            "--stop" => single("stop", stop(value), &mut options)?,
            "--tool-choice" => single("tool_choice", tool_choice(value), &mut options)?,
            "--policy" => single("policy", string(value), &mut options)?,
            "--effects" => {
                let (tool, kind) = value
                    .split_once('=')
                    .ok_or("--effects takes NAME=read-only|mutating")?;
                if effects.iter().any(|(existing, _)| existing == tool) {
                    return Err(format!("--effects names tool {tool:?} twice"));
                }
                effects.push((tool.to_owned(), string(kind)));
            }
            "--reasoning" => single("reasoning", string(value), &mut options)?,
            "--ambiguity" => single("ambiguity", string(value), &mut options)?,
            "--emit" => single("emit", string(value), &mut options)?,
            "--ids-prefix" if ids_prefix.is_none() => ids_prefix = Some(value),
            "--ids-start" if ids_start.is_none() => {
                ids_start =
                    Some(value.parse::<u64>().map_err(|_| {
                        format!("--ids-start {value:?} is not a non-negative integer")
                    })?);
            }
            "--effort" => single(
                "effort",
                string(effort(value).map(|_| value)?),
                &mut options,
            )?,
            _ => return Err(format!("unknown or repeated option {option:?}")),
        }
        position += 2;
    }
    // The file's exact text: the adapters' render-back and schema digests
    // refer to the caller's bytes, not to a re-serialization.
    let declarations = read_input(Some(tools_path.ok_or("--tools is required")?))?;
    options.push(("tools", string(declarations)));
    if !dialects.is_empty() {
        options.push(("dialects", array(dialects)));
    }
    if !effects.is_empty() {
        options.push(("effects", members(effects)));
    }
    if let Some(ids) = ids(ids_source, ids_prefix, ids_start)? {
        options.push(("ids", ids));
    }
    let output = read_bytes(output_path)?;
    let result = purrassist_host::tools::recover_tool_calls(&output, &object(options))?;
    println!("{}", result.to_node().to_json());
    Ok(tool_calls_exit_code(&result.outcome))
}

/// The documented exit code of `recover-tool-calls`: exactly
/// [`purrassist::tool_call::exit_code`], the selected reading's usability
/// verdict (`value.verdict` in the printed JSON) mapped to 0 (`run`,
/// `no_calls`), 3 (`ambiguous`, or an `Ambiguous` operation) or 4
/// (`nothing_usable`, or an `Unresolved` operation), so the exit code and
/// the JSON can never disagree. The printed JSON is the whole answer.
fn tool_calls_exit_code(outcome: &Outcome<ToolCalls>) -> u8 {
    purrassist::tool_call::exit_code(outcome)
}

/// A JSON object of owned keys, in order (the `--effects` overrides).
fn members(entries: Vec<(String, Node)>) -> Node {
    Node::new(Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| Member {
                key,
                key_span: None,
                value,
            })
            .collect(),
    ))
}
/// `--ids-source`, or `--ids-prefix`/`--ids-start`: the `ids` argument, if
/// any; the two forms are mutually exclusive.
fn ids(source: bool, prefix: Option<&str>, start: Option<u64>) -> Result<Option<Node>, String> {
    if source {
        if prefix.is_some() || start.is_some() {
            return Err("--ids-source cannot be combined with --ids-prefix or --ids-start".into());
        }
        return Ok(Some(string("source")));
    }
    let mut members = Vec::new();
    if let Some(prefix) = prefix {
        members.push(("prefix", string(prefix)));
    }
    if let Some(start) = start {
        members.push(("start", integer(start)));
    }
    Ok((!members.is_empty()).then(|| object(members)))
}
/// `--stop`: a stop name, or `stop-sequence=STR` for a matched caller stop
/// sequence.
fn stop(value: &str) -> Node {
    value.strip_prefix("stop-sequence=").map_or_else(
        || string(value),
        |sequence| object([("stop_sequence", string(sequence))]),
    )
}
/// `--tool-choice`: `auto`, `none`, `required`, or the name of the one tool
/// the turn must call (the `OpenAI` function form).
fn tool_choice(value: &str) -> Node {
    if matches!(value, "auto" | "none" | "required") {
        string(value)
    } else {
        object([
            ("type", string("function")),
            ("function", object([("name", string(value))])),
        ])
    }
}

fn call(args: &[String]) -> Result<u8, String> {
    if args.len() != 3 || args[1] != "--root" {
        return Err(
            "usage: purrassist call TOOL --root DIRECTORY (arguments JSON on stdin)".into(),
        );
    }
    let tools = purrassist_host::tools::Tools::new(&args[2]).map_err(|e| e.to_string())?;
    let input = read_input(None)?;
    let args_node = json::parse(&input).map_err(|e| e.to_string())?;
    let result = tools.call(&args[0], &args_node)?;
    println!("{}", result.to_json());
    Ok(0)
}
fn mcp(args: &[String]) -> Result<u8, String> {
    if args.len() != 2 || args[0] != "--root" {
        return Err("usage: purrassist mcp --root DIRECTORY".into());
    }
    let tools = purrassist_host::tools::Tools::new(&args[1]).map_err(|e| e.to_string())?;
    crate::mcp_stdio::run(tools)?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::tool_calls_exit_code;
    use purrassist::Outcome;

    /// An `Ambiguous` operation (readings of the turn that recover
    /// different calls, reachable with caller grammars) exits 3, like the
    /// other subcommands; an `Unresolved` one (no value) exits 4.
    #[test]
    fn operation_outcomes_without_a_single_reading() {
        assert_eq!(tool_calls_exit_code(&Outcome::Ambiguous(Vec::new())), 3);
        assert_eq!(tool_calls_exit_code(&Outcome::Unresolved), 4);
    }
}
