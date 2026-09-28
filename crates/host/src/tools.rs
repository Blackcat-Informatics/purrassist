// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared Rust, CLI, and MCP operations with schema-validated arguments.
mod embeddings;
mod intent;
mod markdown;
mod paths;

use crate::files::Store;
use purrassist::{
    Budget, Effort, Governor, Outcome, RecoveryRequest, Schema,
    edits::{EditPlan, TextEdit},
    json::{self, Node, Value},
    matching::{self, Candidate, MatchRequest},
    syntax::{self, Language},
    wire::{array, integer, object, string},
};
use std::path::Path;

/// Root-scoped host tool collection. All operations share their Rust implementations.
#[derive(Clone, Debug)]
pub struct Tools {
    store: Store,
}
struct Definition {
    name: &'static str,
    description: &'static str,
    schema: &'static str,
    writes: bool,
}
const DEFINITIONS: &[Definition] = &[
    Definition {
        name: "match_embeddings",
        description: "Match caller-supplied vectors in a named embedding space; unique cosine winners are best guesses, ties remain ambiguous, and no model is loaded",
        schema: r#"{"type":"object","required":["space","query","candidates"],"properties":{"space":{"type":"string","minLength":1},"query":{"type":"array","minItems":1,"items":{"type":"number"}},"candidates":{"type":"array","items":{"type":"object","required":["id","vector"],"properties":{"id":{"type":"string","minLength":1},"vector":{"type":"array","minItems":1,"items":{"type":"number"}}},"additionalProperties":false}},"minimum_score":{"type":"number","minimum":-1,"maximum":1},"minimum_margin":{"type":"number","minimum":0,"maximum":2},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "render_markdown",
        description: "Render exact Markdown source to HTML with a source-retaining receipt, dialect, and explicit extension profile; preserves raw HTML/URI schemes except selected GFM tag filtering and is not a sanitizer",
        schema: r#"{"type":"object","required":["source"],"properties":{"source":{"type":"string"},"dialect":{"enum":["commonmark","gfm"]},"extensions":{"type":"array","maxItems":5,"uniqueItems":true,"items":{"enum":["autolinks","strikethrough","tables","task_lists","tag_filter"]}},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "inspect_markdown",
        description: "Index exact Markdown block ranges, source line maps, nested containers, and sections",
        schema: r#"{"type":"object","required":["source"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "markdown_context",
        description: "Extract exact block-aligned Markdown context and enclosing sections",
        schema: r#"{"type":"object","required":["source","start","end"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"neighbors":{"type":"integer","minimum":0}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_markdown_section",
        description: "Plan a structurally validated Markdown section body replacement selected by root heading index or nested path",
        schema: r#"{"type":"object","required":["source","body"],"oneOf":[{"required":["heading"],"properties":{"path":false}},{"required":["path"],"properties":{"heading":false}}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"heading":{"type":"integer","minimum":0},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"body":{"type":"string"}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_markdown_reference",
        description: "Plan replacement of a selected Markdown reference definition's decoded destination and title",
        schema: r#"{"type":"object","required":["source","path","occurrence","destination","title"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"occurrence":{"type":"integer","minimum":0},"destination":{"type":"string"},"title":{"type":["string","null"]}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_markdown_table_column",
        description: "Plan a nested GFM column insertion, removal, or alignment change with structural validation",
        schema: r#"{"type":"object","required":["source","path","action","index"],"oneOf":[{"properties":{"action":{"const":"insert"}},"required":["header","alignment","cells"]},{"properties":{"action":{"const":"remove"},"header":false,"alignment":false,"cells":false}},{"properties":{"action":{"const":"align"},"header":false,"cells":false},"required":["alignment"]}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"action":{"enum":["insert","remove","align"]},"index":{"type":"integer","minimum":0},"header":{"type":"string"},"alignment":{"enum":["default","left","right","center"]},"cells":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_markdown_table_row",
        description: "Plan insertion or removal of a GFM body row by nested table path and zero-based body index",
        schema: r#"{"type":"object","required":["source","path","action","index"],"oneOf":[{"properties":{"action":{"const":"insert"}},"required":["cells"]},{"properties":{"action":{"const":"remove"}},"not":{"required":["cells"]}}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"action":{"enum":["insert","remove"]},"index":{"type":"integer","minimum":0},"cells":{"type":"array","minItems":1,"items":{"type":"string"}}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_markdown_table_cell",
        description: "Plan a source-preserving GFM cell edit by nested block path; row zero is the header",
        schema: r#"{"type":"object","required":["source","path","row","column","markdown"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"row":{"type":"integer","minimum":0},"column":{"type":"integer","minimum":0},"markdown":{"type":"string"}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_markdown_task",
        description: "Plan a single checkbox-state edit selected by root block index or nested block path",
        schema: r#"{"type":"object","required":["source","checked"],"oneOf":[{"required":["block"]},{"required":["path"]}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"block":{"type":"integer","minimum":0},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"checked":{"type":"boolean"}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "decode_json",
        description: "Recover schema-valid JSON with effort bounds, ambiguity, and inference receipts",
        schema: r#"{"type":"object","required":["input","schema"],"properties":{"input":{"type":"string"},"schema":{"type":["object","boolean"]},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"allow_guesses":{"type":"boolean"}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "match_strings",
        description: "Rank candidate strings using exact, Unicode, lexical, distance, and phonetic evidence",
        schema: r#"{"type":"object","required":["query","candidates"],"properties":{"query":{"type":"string"},"candidates":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "match_paths",
        description: "Resolve caller-supplied relative paths with context, explicit ambiguity, and guess receipts; never opens or authorizes a matched path",
        schema: r#"{"type":"object","required":["query","paths"],"properties":{"query":{"type":"string"},"paths":{"type":"array","items":{"type":"string"}},"context":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"minimum_score":{"type":"integer","minimum":0,"maximum":10000},"minimum_margin":{"type":"integer","minimum":0,"maximum":20000}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "resolve_intent",
        description: "Resolve explicit semantic domain rules against caller concepts; unknown words fail, conflicting interpretations remain ambiguous, and no model is contacted",
        schema: r#"{"type":"object","required":["input","domain","concepts"],"properties":{"input":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"domain":{"type":"object","required":["id","rules"],"properties":{"id":{"type":"string"},"rules":{"type":"array","items":{"type":"object","required":["id","phrase","assignments"],"properties":{"id":{"type":"string"},"phrase":{"type":"string"},"assignments":{"type":"object","additionalProperties":{"type":"string"}}},"additionalProperties":false}},"ignored_words":{"type":"array","items":{"type":"string"}}},"additionalProperties":false},"concepts":{"type":"array","items":{"type":"object","required":["id","label","attributes"],"properties":{"id":{"type":"string"},"label":{"type":"string"},"attributes":{"type":"object","additionalProperties":{"type":"string"}}},"additionalProperties":false}}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_replacement",
        description: "Plan exact or bounded fuzzy text replacement; approximate selections remain guesses and ties remain ambiguous",
        schema: r#"{"type":"object","required":["source","needle","replacement"],"properties":{"source":{"type":"string"},"needle":{"type":"string","minLength":1},"replacement":{"type":"string"},"max_edits":{"type":"integer","minimum":0,"maximum":64}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "plan_diff",
        description: "Plan a single-file unified diff with relocatable exact context and source-bound edit ranges",
        schema: r#"{"type":"object","required":["source","patch"],"properties":{"source":{"type":"string"},"patch":{"type":"string"}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "inspect_code",
        description: "Inspect grammar acceptance and source-bound declaration ranges without executing code",
        schema: r#"{"type":"object","required":["source","language"],"properties":{"source":{"type":"string"},"language":{"enum":["python","rust","javascript","typescript","tsx"]}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "read_file",
        description: "Read a bounded root-relative UTF-8 file and its exact digest",
        schema: r#"{"type":"object","required":["path"],"properties":{"path":{"type":"string"}},"additionalProperties":false}"#,
        writes: false,
    },
    Definition {
        name: "apply_file",
        description: "Conditionally replace a file, validate supported code grammar, journal it, and return undo identity",
        schema: r#"{"type":"object","required":["path","expected_digest","replacement"],"properties":{"path":{"type":"string"},"expected_digest":{"type":"string"},"replacement":{"type":"string"}},"additionalProperties":false}"#,
        writes: true,
    },
    Definition {
        name: "undo_file",
        description: "Undo a journaled file replacement only if its exact output is still present",
        schema: r#"{"type":"object","required":["transaction"],"properties":{"transaction":{"type":"string"}},"additionalProperties":false}"#,
        writes: true,
    },
];
impl Tools {
    pub(crate) fn contains(name: &str) -> bool {
        DEFINITIONS.iter().any(|definition| definition.name == name)
    }
    /// Initialize the file store for an explicit root; creates `.purrassist`.
    ///
    /// # Errors
    /// Returns filesystem initialization errors.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, crate::files::Error> {
        Ok(Self {
            store: Store::open(root, 16 * 1024 * 1024)?,
        })
    }
    /// Tool definitions suitable for MCP discovery, including write/read annotations.
    ///
    /// # Errors
    /// Returns an error if a built-in tool schema is malformed.
    pub fn definitions() -> Result<Node, String> {
        let mut definitions = Vec::new();
        for definition in DEFINITIONS {
            definitions.push(object([
                ("name", string(definition.name)),
                ("description", string(definition.description)),
                (
                    "inputSchema",
                    json::parse(definition.schema).map_err(|e| e.to_string())?,
                ),
                (
                    "annotations",
                    object([
                        ("readOnlyHint", Node::new(Value::Bool(!definition.writes))),
                        ("destructiveHint", Node::new(Value::Bool(definition.writes))),
                        ("openWorldHint", Node::new(Value::Bool(false))),
                    ]),
                ),
            ]));
        }
        Ok(array(definitions))
    }
    /// Invoke the same operation exposed through CLI `call` and MCP `tools/call`.
    ///
    /// # Errors
    /// Rejects unknown operations, schema-invalid arguments, stale writes, and execution errors.
    pub fn call(&self, name: &str, args: &Node) -> Result<Node, String> {
        let definition = DEFINITIONS
            .iter()
            .find(|definition| definition.name == name)
            .ok_or_else(|| format!("unknown tool {name:?}"))?;
        let schema = Schema::compile(definition.schema).map_err(|e| e.to_string())?;
        let validation = schema.validate(args).map_err(|e| e.to_string())?;
        if !validation.valid {
            return Err(format!(
                "tool arguments violate schema: {:?}",
                validation.violations
            ));
        }
        match name {
            "decode_json" => decode(args),
            "match_strings" => rank(args),
            "match_paths" => paths::resolve(args),
            "resolve_intent" => intent::resolve(args),
            "match_embeddings" => embeddings::call(args),
            "render_markdown"
            | "inspect_markdown"
            | "markdown_context"
            | "plan_markdown_section"
            | "plan_markdown_table_cell"
            | "plan_markdown_table_row"
            | "plan_markdown_table_column"
            | "plan_markdown_reference"
            | "plan_markdown_task" => markdown::call(name, args),
            "plan_replacement" => {
                let max_edits = match args.get("max_edits").map(|node| &node.value) {
                    Some(Value::Number(number)) => {
                        number.to_usize().ok_or("max_edits is out of range")?
                    }
                    None => 0,
                    _ => return Err("max_edits must be an integer".into()),
                };
                Ok(planned(purrassist::edits::locate_fuzzy(
                    field(args, "source")?,
                    field(args, "needle")?,
                    field(args, "replacement")?,
                    max_edits,
                    Budget::default(),
                )))
            }
            "plan_diff" => Ok(planned(purrassist::edits::plan_unified(
                field(args, "source")?,
                field(args, "patch")?,
                Budget::default(),
            ))),
            "inspect_code" => inspect(args),
            "read_file" => {
                let snapshot = self
                    .store
                    .read(field(args, "path")?)
                    .map_err(|e| e.to_string())?;
                Ok(object([
                    ("path", string(snapshot.path)),
                    ("source", string(snapshot.text)),
                    ("digest", string(snapshot.digest.to_string())),
                ]))
            }
            "apply_file" => self.apply(args),
            "undo_file" => {
                let commit = self
                    .store
                    .undo(field(args, "transaction")?)
                    .map_err(|e| e.to_string())?;
                Ok(commit_node(commit))
            }
            _ => Err("tool dispatch invariant failed".into()),
        }
    }
    fn apply(&self, args: &Node) -> Result<Node, String> {
        let path = field(args, "path")?;
        let snapshot = self.store.read(path).map_err(|e| e.to_string())?;
        if field(args, "expected_digest")? != snapshot.digest.to_string() {
            return Err("stale expected digest".into());
        }
        let replacement = field(args, "replacement")?;
        let plan = EditPlan {
            source: snapshot.digest,
            edits: vec![TextEdit {
                range: 0..snapshot.text.len(),
                replacement: replacement.into(),
            }],
        };
        let language = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension {
                "py" => Some(Language::Python),
                "rs" => Some(Language::Rust),
                "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
                "ts" => Some(Language::TypeScript),
                "tsx" => Some(Language::Tsx),
                _ => None,
            });
        let result = language.map_or_else(
            || plan.apply(&snapshot.text, Budget::default()),
            |language| syntax::apply_checked(&snapshot.text, &plan, language, Budget::default()),
        );
        let Outcome::Resolved(applied) = result.outcome else {
            return Err(format!(
                "edit was not unambiguously validated: {}",
                result.receipt.to_node().to_json()
            ));
        };
        let commit = self
            .store
            .commit(&snapshot, &applied.text)
            .map_err(|e| e.to_string())?;
        Ok(object([
            ("commit", commit_node(commit)),
            ("receipt", result.receipt.to_node()),
        ]))
    }
}
fn field<'a>(node: &'a Node, name: &str) -> Result<&'a str, String> {
    node.get(name)
        .and_then(Node::as_str)
        .ok_or_else(|| format!("missing string argument {name}"))
}
fn effort(args: &Node) -> Result<Effort, String> {
    Ok(
        match args
            .get("effort")
            .and_then(Node::as_str)
            .unwrap_or("standard")
        {
            "exact" => Effort::Exact,
            "quick" => Effort::Quick,
            "standard" => Effort::Standard,
            "deep" => Effort::Deep,
            "maximum" => Effort::Maximum,
            _ => return Err("unknown effort".into()),
        },
    )
}
fn decode(args: &Node) -> Result<Node, String> {
    let effort = effort(args)?;
    let input = field(args, "input")?;
    let mut governor = Governor::new(effort.budget());
    let schema_text = args.get("schema").ok_or("missing schema")?.to_json();
    let schema = match Schema::compile_governed(&schema_text, &mut governor) {
        Ok(schema) => schema,
        Err(purrassist::schema::CompilationError::InvalidSchema(error)) => {
            return Err(error.to_string());
        }
        Err(purrassist::schema::CompilationError::Resource(reason)) => {
            let mut receipt = purrassist::Receipt::new(
                input.as_bytes(),
                "json.recover.v1",
                effort,
                effort.budget(),
            );
            receipt.stopped = reason;
            receipt.usage = governor.usage();
            return Ok(purrassist::AssistResult::<Node> {
                outcome: Outcome::Unresolved,
                receipt,
            }
            .to_node());
        }
    };
    let allow_guesses = args
        .get("allow_guesses")
        .is_none_or(|node| node.value == Value::Bool(true));
    Ok(schema
        .recover_governed(
            input,
            RecoveryRequest {
                effort,
                allow_guesses,
                budget: None,
            },
            &mut governor,
        )
        .to_node())
}

fn rank(args: &Node) -> Result<Node, String> {
    let Some(Node {
        value: Value::Array(values),
        ..
    }) = args.get("candidates")
    else {
        return Err("missing candidates".into());
    };
    let candidates: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .map(|text| Candidate::new(index.to_string(), text))
                .ok_or("candidate must be a string")
        })
        .collect::<Result<_, _>>()?;
    let result = matching::rank(field(args, "query")?, &candidates, &MatchRequest::default());
    let values = match result.outcome {
        Outcome::Resolved(values) | Outcome::BestGuess(values) => values,
        _ => Vec::new(),
    };
    Ok(object([
        (
            "matches",
            array(values.iter().map(|value| {
                object([
                    ("id", string(&value.candidate.id)),
                    ("text", string(&value.candidate.text)),
                    ("score", integer(u64::from(value.score))),
                ])
            })),
        ),
        ("receipt", result.receipt.to_node()),
    ]))
}
fn inspect(args: &Node) -> Result<Node, String> {
    let language = match field(args, "language")? {
        "python" => Language::Python,
        "rust" => Language::Rust,
        "javascript" => Language::JavaScript,
        "typescript" => Language::TypeScript,
        "tsx" => Language::Tsx,
        _ => return Err("unsupported language".into()),
    };
    let report = syntax::inspect(
        field(args, "source")?,
        language,
        &mut Governor::new(Budget::default()),
    )
    .map_err(|e| e.to_string())?;
    Ok(object([
        ("valid", Node::new(Value::Bool(report.valid))),
        ("source", string(report.source.to_string())),
        ("parser", string(report.parser)),
        (
            "issues",
            array(report.issues.iter().map(|issue| {
                object([
                    ("kind", string(&issue.kind)),
                    (
                        "start",
                        integer(u64::try_from(issue.span.start).unwrap_or(u64::MAX)),
                    ),
                    (
                        "end",
                        integer(u64::try_from(issue.span.end).unwrap_or(u64::MAX)),
                    ),
                    ("missing", Node::new(Value::Bool(issue.missing))),
                ])
            })),
        ),
        (
            "symbols",
            array(report.symbols.iter().map(|symbol| {
                object([
                    ("name", string(&symbol.name)),
                    ("kind", string(&symbol.kind)),
                    (
                        "start",
                        integer(u64::try_from(symbol.span.start).unwrap_or(u64::MAX)),
                    ),
                    (
                        "end",
                        integer(u64::try_from(symbol.span.end).unwrap_or(u64::MAX)),
                    ),
                ])
            })),
        ),
    ]))
}
fn commit_node(commit: crate::files::Commit) -> Node {
    object([
        ("transaction", string(commit.id)),
        ("path", string(commit.path)),
        ("before", string(commit.before.to_string())),
        ("after", string(commit.after.to_string())),
    ])
}
fn planned(result: purrassist::AssistResult<EditPlan>) -> Node {
    let (status, plans) = match result.outcome {
        Outcome::Resolved(plan) => ("resolved", vec![plan]),
        Outcome::BestGuess(plan) => ("best_guess", vec![plan]),
        Outcome::Ambiguous(plans) => ("ambiguous", plans),
        Outcome::Unresolved => ("unresolved", Vec::new()),
    };
    object([
        ("status", string(status)),
        (
            "plans",
            array(plans.iter().map(|plan| {
                object([
                    ("source", string(plan.source.to_string())),
                    (
                        "edits",
                        array(plan.edits.iter().map(|edit| {
                            object([
                                (
                                    "start",
                                    integer(u64::try_from(edit.range.start).unwrap_or(u64::MAX)),
                                ),
                                (
                                    "end",
                                    integer(u64::try_from(edit.range.end).unwrap_or(u64::MAX)),
                                ),
                                ("replacement", string(&edit.replacement)),
                            ])
                        })),
                    ),
                ])
            })),
        ),
        ("receipt", result.receipt.to_node()),
    ])
}
