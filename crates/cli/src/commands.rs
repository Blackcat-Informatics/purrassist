// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use purrassist::{
    Effort, Outcome, RecoveryRequest, Schema,
    json::{self, Value},
    matching::{self, Candidate, MatchRequest},
    wire::{array, integer, object, string},
};
use std::io::{self, Read};

const MAX_INPUT: u64 = 16 * 1024 * 1024;
const HELP: &str = "PurRAssist: evidence-bearing recovery for imperfect AI output\n\nUsage:\n  purrassist decode --schema FILE [--input FILE] [--effort exact|quick|standard|deep|maximum] [--no-guesses]\n  purrassist validate --schema FILE [--input FILE]\n  purrassist match QUERY CANDIDATE...\n  purrassist call TOOL --root DIRECTORY\n  purrassist mcp --root DIRECTORY\n\nInput defaults to stdin. Decode emits a JSON answer and receipt.\nExit: 0 resolved/best-guess, 3 ambiguous, 4 unresolved/invalid, 1 invocation error.\n";

#[expect(
    clippy::redundant_pub_crate,
    reason = "binary entry point requires explicit parent visibility without public export"
)]
pub(super) fn run(args: &[String]) -> Result<u8, String> {
    match args.first().map(String::as_str) {
        None | Some("--help" | "-h" | "help") => {
            print!("{HELP}");
            Ok(0)
        }
        Some("--version") => {
            println!("purrassist {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Some("decode" | "validate") => decode(args),
        Some("match") => rank(&args[1..]),
        Some("call") => call(&args[1..]),
        Some("mcp") => mcp(&args[1..]),
        Some(command) => Err(format!("unknown command {command:?}; use --help")),
    }
}
fn read_input(path: Option<&str>) -> Result<String, String> {
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
    String::from_utf8(bytes).map_err(|e| format!("input is not UTF-8: {e}"))
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
    Ok(match result.outcome {
        Outcome::Resolved(_) | Outcome::BestGuess(_) => 0,
        Outcome::Ambiguous(_) => 3,
        Outcome::Unresolved => 4,
    })
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
