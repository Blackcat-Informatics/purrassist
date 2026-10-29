// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Shared Rust, CLI, and MCP operations with schema-validated arguments and outputs.
mod edits;
mod embeddings;
mod files;
mod helper;
mod inspect;
mod intent;
mod markdown;
mod paths;
mod project_result;
mod registry;
mod tool_calls;
mod validate;
mod witness;

use crate::files::Store;
use purrassist::{
    AssistResult, Budget, CancellationToken, Effort, Governor, Outcome, Receipt, RecoveryRequest,
    Schema, StopReason,
    contract::Resource,
    digest::Digest,
    edits::{EditPlan, TextEdit},
    json::{Node, Value},
    matching::{self, Candidate, MatchRequest, SelectionPolicy as StringSelectionPolicy},
    syntax::{self, Language},
    wire::{WireValue, array, object, string},
};
use std::path::Path;

/// Root-scoped host tool collection. All operations share their Rust implementations.
#[derive(Clone, Debug)]
pub struct Tools {
    store: Store,
}
impl Tools {
    pub(crate) fn contains(name: &str) -> bool {
        registry::contains(name)
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
        registry::definitions()
    }
    /// Invoke the same operation exposed through CLI `call` and MCP `tools/call`.
    ///
    /// # Errors
    /// Rejects unknown operations, schema preparation or validation failures,
    /// stale writes and execution errors. Both selected schemas are prepared
    /// before the handler runs. The complete returned node must satisfy the
    /// registered output contract before successful release.
    /// Output validation has a separate default Work allowance: the standard
    /// base plus 256 units per returned node. Bounded, allocation-free sizing
    /// carries its consumed work into validation; other limits stay unchanged.
    ///
    /// A post-handler refusal is a JSON diagnostic in the returned error string,
    /// with code `registered_output_refused`, original output, tool/schema
    /// identity, reason/report, separate boundary receipts and effect context.
    /// The handler has already returned; a file mutation may have completed.
    /// Refusal neither retries nor rolls back it. Successful bytes and engine
    /// receipts remain unchanged; those receipts exclude registry validation.
    pub fn call(&self, name: &str, args: &Node) -> Result<Node, String> {
        registry::call(self, name, args, None)
    }

    /// Invoke with a caller-owned cooperative cancellation signal.
    ///
    /// Argument validation and governed engines observe cancellation at checks
    /// and charges. File reads may finish their current OS operation. Mutations
    /// check before entering the journaled commit/undo; once entered, they finish
    /// that transaction and report its actual outcome rather than imply rollback.
    /// Once a handler returns, output validation runs under its own bounded
    /// governor without observing late cancellation. A completed transaction
    /// therefore retains either its validated result or an explicit output
    /// refusal containing its original result and commit/undo identity.
    /// This method starts no thread or listener.
    ///
    /// ```no_run
    /// use purrassist::{CancellationToken, json};
    /// use purrassist_host::tools::Tools;
    /// let tools = Tools::new(".")?;
    /// let token = CancellationToken::new();
    /// token.cancel();
    /// let args = json::parse(r#"{"path":"example.txt"}"#)?;
    /// assert_eq!(tools.call_with_cancellation("read_file", &args, &token),
    ///     Err("cancelled".into()));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    /// Returns the same errors as `call`, plus cancellation during argument
    /// validation or before a filesystem operation. Engine receipts preserve
    /// their typed cancellation outcome. Post-handler schema or resource
    /// refusals retain complete output and phase usage in the JSON diagnostic
    /// described by [`Self::call`]; they never masquerade as cancellation.
    pub fn call_with_cancellation(
        &self,
        name: &str,
        args: &Node,
        cancellation: &CancellationToken,
    ) -> Result<Node, String> {
        registry::call(self, name, args, Some(cancellation))
    }

    fn read_file(
        &self,
        args: &Node,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Node, String> {
        governor(Budget::default(), cancellation)
            .check()
            .map_err(|e| e.to_string())?;
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
    fn undo_file(
        &self,
        args: &Node,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Node, String> {
        governor(Budget::default(), cancellation)
            .check()
            .map_err(|e| e.to_string())?;
        let commit = self
            .store
            .undo(field(args, "transaction")?)
            .map_err(|e| e.to_string())?;
        Ok(commit_node(commit))
    }
    fn recover_files(
        &self,
        _args: &Node,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Node, String> {
        governor(Budget::default(), cancellation)
            .check()
            .map_err(|e| e.to_string())?;
        files::recover(&self.store)
    }
    fn commit_files(
        &self,
        args: &Node,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Node, String> {
        governor(Budget::default(), cancellation)
            .check()
            .map_err(|e| e.to_string())?;
        files::commit(&self.store, args)
    }
    fn apply(&self, args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
        let requested_effort = effort(args)?;
        let mut governor = governor(requested_effort.budget(), cancellation);
        governor.check().map_err(|e| e.to_string())?;
        let path = field(args, "path")?;
        let snapshot = self.store.read(path).map_err(|e| e.to_string())?;
        let rebase_from = args.get("rebase_from");
        if rebase_from.is_some() && args.get("plan").is_none() {
            return Err("rebase_from requires plan".into());
        }
        // `expected_digest` names the version the caller's plan (or, for
        // `rebase_from`, its prior text) actually targets; the live file
        // may have moved on since, exactly the case `rebase_from` exists to
        // reconcile, so the live digest is checked by the rebase itself
        // rather than gating admission here.
        let baseline_digest = match rebase_from {
            Some(rebase_from) => Digest::of(field(rebase_from, "source")?.as_bytes()).to_string(),
            None => snapshot.digest.to_string(),
        };
        if field(args, "expected_digest")? != baseline_digest {
            return Err("stale expected digest".into());
        }
        let mut transformation = None;
        let plan = if let Some(plan) = args.get("plan") {
            edits::decode_plan_rebased(
                &snapshot.text,
                plan,
                rebase_from,
                &mut governor,
                &mut transformation,
            )
            .map_err(|error| error.to_string())?
        } else {
            let replacement = field(args, "replacement")?;
            EditPlan {
                source: snapshot.digest,
                edits: vec![TextEdit {
                    range: 0..snapshot.text.len(),
                    replacement: replacement.into(),
                }],
            }
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
        let mut result = if let Some(language) = language {
            syntax::apply_checked_governed(&snapshot.text, &plan, language, &mut governor)
        } else {
            plan.apply_governed(&snapshot.text, &mut governor)
        };
        result.receipt.effort = requested_effort;
        if let Some(transformation) = transformation {
            result.receipt.evidence.insert(0, transformation);
        }
        let Outcome::Resolved(applied) = result.outcome else {
            return Err(format!(
                "edit was not unambiguously validated: {}",
                result.receipt.to_node().to_json()
            ));
        };
        governor.check().map_err(|e| e.to_string())?;
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

/// Validate a JSON instance against a schema: the exact operation the
/// `validate_json` tool exposes, shared by the CLI `purrassist validate`
/// command so both surfaces emit identical output.
///
/// # Errors
/// Returns the same errors `Tools::call("validate_json", args)` would.
pub fn validate_json(args: &Node) -> Result<Node, String> {
    validate::call(args, None)
}

/// Validate a complete original JSON result, apply its context mask, and check
/// the resulting compact JSON Lines artifact's exact UTF-8 byte ceiling.
///
/// This is the same read-only `project_result` operation exposed by CLI `call`
/// and MCP. Accepted processing refusals are structured replies retaining the
/// original source, controls, stage and typed cause. Protocol and receipt bytes
/// are separate from the artifact measurement.
///
/// # Errors
/// This direct helper checks required processing fields, rather than validating
/// the full registry envelope. Use `Tools::call` for registry schema validation.
/// Missing required fields are invocation errors. Domain and native resource
/// refusals are returned in the structured processing reply.
///
/// ```
/// use purrassist::json;
/// let args = json::parse(r#"{"result_json":"{\"items\":[1.00e2,true]}","output_schema":{"type":"object"},"format":"purrassist-toolgraph-v2","context":{"outputProjectionJq":".items[]?","maxOutputBytes":64,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":[],"loadingStrategy":"always"}}"#)?;
/// let reply = purrassist_host::tools::project_result(&args)?;
/// assert_eq!(reply.pointer("/artifact/text").and_then(json::Node::as_str), Some("1.00e2\ntrue\n"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn project_result(args: &Node) -> Result<Node, String> {
    project_result::call(args, None)
}

/// Recover the tool calls of one model turn from raw output bytes.
///
/// This is the exact operation the `recover_tool_calls` tool exposes, shared
/// by the CLI `purrassist recover-tool-calls` command so every surface builds
/// the same request.
///
/// `options` is the tool's argument object without `output` (an `output`
/// member, if present, is ignored in favour of `output`): `tools` (required:
/// the declarations, as JSON), `tools_format` (`openai`, the default, `mcp`,
/// or `anthropic`), `dialects` (built-in dialect ids; default every
/// built-in), `stop` (`stop`, `length`, `tool_calls`, `content_filter`,
/// `unknown`, or `{"stop_sequence": STRING}`), `tool_choice` (an `OpenAI`
/// form: a string or `{"type":"function",..}`; or an Anthropic object form,
/// where `{"type":"tool","name":N,"disable_parallel_tool_use":true}` forces
/// exactly one call), `tool_choice_strict` (`{"exactly_one": BOOL,
/// "allow_content": BOOL, "allow_reasoning": BOOL}`, all required: the
/// options of a strict forced choice, valid only beside a forced
/// `tool_choice`; `exactly_one: false` contradicts an Anthropic
/// `disable_parallel_tool_use: true`),
/// `policy` (`per-tool`, `structural`, `tolerant`), `effects` (tool name to
/// `read-only` or `mutating`), `reasoning` (`separate`, `separate-open`,
/// `content`), `ambiguity` (`report`, `reject`), `ids` (`"source"` or
/// `{"prefix": STRING, "start": INTEGER}`), `emit` (`none`, `openai`, or a
/// dialect id), and `effort`.
///
/// `output` need not be UTF-8: each invalid run is recorded as a
/// `tool_call.input.invalid-utf8` loss and the receipt retains the exact
/// bytes; a run inside a call's argument, id or parameter name is also
/// read as U+FFFD and recorded on the call as a
/// `tool_call.value.invalid-utf8` data loss (the call is kept, at most a
/// best guess). The budget is the effort preset sized for the output's length
/// (`ToolRecoveryRequest::sized`), and one governor is charged for
/// declaration compilation, the dialect allow-list, and recovery.
///
/// ```
/// use purrassist::{Outcome, json};
/// let options = json::parse(
///     r#"{"tools":[{"type":"function","function":{"name":"ls","parameters":{"type":"object"}}}]}"#,
/// )?;
/// let output = b"<tool_call>{\"name\": \"ls\", \"arguments\": {}}</tool_call>";
/// let result = purrassist_host::tools::recover_tool_calls(output, &options)?;
/// let Outcome::Resolved(calls) = result.outcome else { panic!("resolved") };
/// assert_eq!(calls.calls[0].tool.as_deref(), Some("ls"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
/// Returns a message for options that are malformed (an unknown member, an
/// unknown enumeration value, a malformed `tool_choice`, a
/// `tool_choice_strict` that is malformed, beside a choice that is not
/// forced, or contradicting `disable_parallel_tool_use`, an unknown or
/// repeated dialect) and for declarations that are not valid (not strict
/// JSON, an invalid definition, a name collision). Exhaustion and
/// cancellation are never errors: they are an `Unresolved` result whose
/// receipt records the stop, as are request errors core recovery itself
/// reports (an effects override or forced choice naming an undeclared tool).
pub fn recover_tool_calls(
    output: &[u8],
    options: &Node,
) -> Result<AssistResult<purrassist::tool_call::ToolCalls>, String> {
    tool_calls::recover(output, options, None)
}

fn governor(budget: Budget, cancellation: Option<&CancellationToken>) -> Governor {
    cancellation.map_or_else(
        || Governor::new(budget),
        |token| Governor::with_cancellation(budget, token.clone()),
    )
}

fn field<'a>(node: &'a Node, name: &str) -> Result<&'a str, String> {
    node.get(name)
        .and_then(Node::as_str)
        .ok_or_else(|| format!("missing string argument {name}"))
}
fn unsigned(node: &Node, name: &str) -> Result<u64, String> {
    match node.get(name).map(|value| &value.value) {
        Some(Value::Number(value)) => value
            .to_usize()
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| format!("integer argument {name} is out of range")),
        _ => Err(format!("missing integer argument {name}")),
    }
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
fn decode(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let effort = effort(args)?;
    let input = field(args, "input")?;
    let mut governor = governor(effort.budget(), cancellation);
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

fn rank(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let query = field(args, "query")?;
    let effort = effort(args)?;
    let limit = match args.get("limit").map(|value| &value.value) {
        Some(Value::Number(value)) => value.to_usize().ok_or("invalid match limit")?,
        None => MatchRequest::default().limit,
        _ => return Err("invalid match limit".into()),
    };
    let english_phonetics = english_phonetics(args)?;
    let request = MatchRequest {
        effort,
        budget: effort.budget(),
        english_phonetics,
        limit,
    };
    let mut governor = governor(request.budget, cancellation);
    let Some(Node {
        value: Value::Array(values),
        ..
    }) = args.get("candidates")
    else {
        return Err("missing candidates".into());
    };
    let prepared = prepare_match_candidates(values, &mut governor);
    let result = match prepared {
        Ok(candidates) => matching::rank_governed(query, &candidates, &request, &mut governor),
        Err(reason) => {
            let mut receipt =
                Receipt::new(query.as_bytes(), "matching/rank-v1", effort, request.budget);
            receipt.stopped = reason;
            receipt.usage = governor.usage();
            AssistResult {
                outcome: Outcome::Unresolved,
                receipt,
            }
        }
    };
    let values = match result.outcome {
        Outcome::Resolved(values) | Outcome::BestGuess(values) => values,
        _ => Vec::new(),
    };
    Ok(object([
        (
            "matches",
            array(values.iter().map(matching::RankedMatch::to_node)),
        ),
        ("receipt", result.receipt.to_node()),
    ]))
}
fn english_phonetics(args: &Node) -> Result<bool, String> {
    match args.get("english_phonetics").map(|value| &value.value) {
        Some(Value::Bool(value)) => Ok(*value),
        None => Ok(true),
        _ => Err("invalid english_phonetics".into()),
    }
}
fn resolve_string(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let query = field(args, "query")?;
    let effort = effort(args)?;
    let request = MatchRequest {
        effort,
        budget: effort.budget(),
        english_phonetics: english_phonetics(args)?,
        ..MatchRequest::default()
    };
    let threshold = |name: &str, default: u32| -> Result<u32, String> {
        match args.get(name).map(|value| &value.value) {
            Some(Value::Number(value)) => value
                .to_usize()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| format!("invalid {name}")),
            None => Ok(default),
            _ => Err(format!("invalid {name}")),
        }
    };
    let policy = StringSelectionPolicy {
        minimum_score: threshold("minimum_score", 7500)?,
        minimum_margin: threshold("minimum_margin", 500)?,
    };
    let mut governor = governor(request.budget, cancellation);
    let Some(Node {
        value: Value::Array(values),
        ..
    }) = args.get("candidates")
    else {
        return Err("missing candidates".into());
    };
    let result = match prepare_match_candidates(values, &mut governor) {
        Ok(candidates) => {
            matching::resolve_governed(query, &candidates, &request, policy, &mut governor)
        }
        Err(reason) => {
            let mut receipt = Receipt::new(
                query.as_bytes(),
                "matching/resolve-v1",
                effort,
                request.budget,
            );
            receipt.stopped = reason;
            receipt.usage = governor.usage();
            AssistResult {
                outcome: Outcome::Unresolved,
                receipt,
            }
        }
    };
    Ok(result.to_node())
}
fn prepare_match_candidates(
    values: &[Node],
    governor: &mut Governor,
) -> Result<Vec<Candidate>, StopReason> {
    let mut candidates = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let (id, text, aliases): (&str, &str, Vec<&str>) = match &value.value {
            Value::String(text) => {
                let id = index.to_string();
                let storage = u64::try_from(id.len().saturating_add(text.len()))
                    .unwrap_or(u64::MAX)
                    .saturating_mul(2)
                    .saturating_add(256);
                governor.charge(Resource::Work, storage)?;
                governor.charge(Resource::ScratchBytes, storage)?;
                candidates.push(Candidate::new(id, text));
                continue;
            }
            Value::Object(_) => {
                let id = field(value, "id").map_err(StopReason::InvalidInput)?;
                let text = field(value, "text").map_err(StopReason::InvalidInput)?;
                let aliases = match value.get("aliases") {
                    Some(Node {
                        value: Value::Array(aliases),
                        ..
                    }) => aliases
                        .iter()
                        .map(|alias| {
                            alias.as_str().ok_or_else(|| {
                                StopReason::InvalidInput("candidate alias must be a string".into())
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    None => Vec::new(),
                    _ => {
                        return Err(StopReason::InvalidInput("invalid candidate aliases".into()));
                    }
                };
                (id, text, aliases)
            }
            _ => return Err(StopReason::InvalidInput("invalid candidate".into())),
        };
        let bytes = aliases
            .iter()
            .fold(id.len().saturating_add(text.len()), |sum, alias| {
                sum.saturating_add(alias.len())
            });
        let storage = u64::try_from(bytes)
            .unwrap_or(u64::MAX)
            .saturating_mul(2)
            .saturating_add(
                u64::try_from(aliases.len())
                    .unwrap_or(u64::MAX)
                    .saturating_mul(64),
            )
            .saturating_add(256);
        governor.charge(Resource::Work, storage)?;
        governor.charge(Resource::ScratchBytes, storage)?;
        candidates.push(Candidate {
            id: id.into(),
            text: text.into(),
            aliases: aliases.into_iter().map(str::to_owned).collect(),
        });
    }
    Ok(candidates)
}
fn commit_node(commit: crate::files::Commit) -> Node {
    object([
        ("transaction", string(commit.id)),
        ("path", string(commit.path)),
        ("before", string(commit.before.to_string())),
        ("after", string(commit.after.to_string())),
        ("durability", durability_node(commit.durability)),
    ])
}
fn durability_node(durability: crate::files::Durability) -> Node {
    use crate::files::{DirectorySync, RenameMethod};
    object([
        (
            "rename",
            string(match durability.rename {
                RenameMethod::PosixSemantics => "posix_semantics",
                RenameMethod::MoveFileExFallback => "move_file_ex_fallback",
                RenameMethod::AlreadyCurrent => "already_current",
            }),
        ),
        (
            "directory_sync",
            string(match durability.directory {
                DirectorySync::Flushed => "flushed",
                DirectorySync::AccessDeniedFallback => "access_denied_fallback",
            }),
        ),
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
        ("plans", array(plans.iter().map(EditPlan::to_node))),
        ("receipt", result.receipt.to_node()),
    ])
}
