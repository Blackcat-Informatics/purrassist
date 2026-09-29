// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! One declarative registry entry per operation: name, description, input
//! JSON Schema, output JSON Schema, write annotation, and handler. `Tools`'s
//! public `definitions`/`call`/`call_with_cancellation`, the CLI `call`
//! command, and MCP `tools/list`/`tools/call` all derive from [`ENTRIES`];
//! there is no second, hand-maintained dispatch path. Input arguments are
//! validated by this crate's own schema engine before a handler ever runs;
//! `crates/cli/tests/protocol.rs` validates every registered output schema
//! against a real fixture for every entry.
use super::Tools;
use purrassist::{
    Budget, CancellationToken,
    json::{self, Node, Value},
    schema::Schema,
    wire::{array, object, string},
};

pub(super) type Handler =
    fn(&Tools, &str, &Node, Option<&CancellationToken>) -> Result<Node, String>;

pub(super) struct Entry {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: &'static str,
    pub output_schema: &'static str,
    pub writes: bool,
    pub handler: Handler,
}

/// The wire shape every governed operation wrapped by
/// `purrassist::AssistResult<Node>::to_node()` produces: `value`/`alternatives`
/// carry the operation's own answer shape (left as `true`, "anything", since
/// it differs per operation), while `status` and `receipt` are common to all
/// of them. `receipt`'s own shape is exhaustively pinned by
/// `docs/schema/receipt.schema.json`; this loosened copy only re-asserts the
/// wire keys every receipt carries, to avoid duplicating that whole schema
/// once per tool.
const ASSIST_RESULT_OUTPUT: &str = r#"{"type":"object","additionalProperties":false,"required":["status","value","alternatives","receipt"],"properties":{"status":{"enum":["resolved","best_guess","ambiguous","unresolved"]},"value":true,"alternatives":{"type":"array"},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer","minimum":0},"source":{"type":"string"},"sources":{"type":"array"},"alternatives":{"type":"array"},"ambiguity":true,"operation":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"budget":{"type":"object"},"usage":{"type":"object"},"evidence":{"type":"array"},"losses":{"type":"array"},"inferences":{"type":"array"},"validations":{"type":"array"},"termination":{"type":"object","required":["kind","detail"],"properties":{"kind":{"enum":["completed","no_candidate","budget_exceeded","cancelled","invalid_input"]},"detail":{"type":"string"}}}}}}}"#;

/// The wire shape `super::planned` produces for exact/fuzzy/unified edit
/// planning: one or more raw [`purrassist::edits::EditPlan`] nodes instead of
/// a single `value`.
const PLANNED_OUTPUT: &str = r#"{"type":"object","additionalProperties":false,"required":["status","plans","receipt"],"properties":{"status":{"enum":["resolved","best_guess","ambiguous","unresolved"]},"plans":{"type":"array"},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer","minimum":0},"source":{"type":"string"},"sources":{"type":"array"},"alternatives":{"type":"array"},"ambiguity":true,"operation":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"budget":{"type":"object"},"usage":{"type":"object"},"evidence":{"type":"array"},"losses":{"type":"array"},"inferences":{"type":"array"},"validations":{"type":"array"},"termination":{"type":"object"}}}}}"#;

/// [`crate::files::Commit`] alone (returned directly by `undo_file`, and
/// nested under `commit`/each batch entry by `apply_file`/`commit_files`).
const COMMIT_OUTPUT: &str = r#"{"type":"object","additionalProperties":false,"required":["transaction","path","before","after","durability"],"properties":{"transaction":{"type":"string"},"path":{"type":"string"},"before":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"after":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"durability":{"type":"object","additionalProperties":false,"required":["rename","directory_sync"],"properties":{"rename":{"enum":["posix_semantics","move_file_ex_fallback","already_current"]},"directory_sync":{"enum":["flushed","access_denied_fallback"]}}}}}"#;

macro_rules! entry {
    ($name:expr, $description:expr, $input:expr, $output:expr, $writes:expr, $handler:expr) => {
        Entry {
            name: $name,
            description: $description,
            input_schema: $input,
            output_schema: $output,
            writes: $writes,
            handler: $handler,
        }
    };
}

pub(super) static ENTRIES: &[Entry] = &[
    entry!(
        "match_embeddings",
        "Match caller-supplied vectors in a named embedding space; unique cosine winners are best guesses, ties remain ambiguous, and no model is loaded",
        r#"{"type":"object","required":["space","query","candidates"],"properties":{"space":{"type":"string","minLength":1},"query":{"type":"array","minItems":1,"items":{"type":"number"}},"candidates":{"type":"array","items":{"type":"object","required":["id","vector"],"properties":{"id":{"type":"string","minLength":1},"vector":{"type":"array","minItems":1,"items":{"type":"number"}}},"additionalProperties":false}},"minimum_score":{"type":"number","minimum":-1,"maximum":1},"minimum_margin":{"type":"number","minimum":0,"maximum":2},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::embeddings::call(args, cancellation)
    ),
    entry!(
        "render_markdown",
        "Render exact Markdown source to HTML with a source-retaining receipt, dialect, and explicit extension profile; preserves raw HTML/URI schemes except selected GFM tag filtering and is not a sanitizer",
        r#"{"type":"object","required":["source"],"properties":{"source":{"type":"string"},"dialect":{"enum":["commonmark","gfm"]},"extensions":{"type":"array","maxItems":5,"uniqueItems":true,"items":{"enum":["autolinks","strikethrough","tables","task_lists","tag_filter"]}},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "inspect_markdown",
        "Index exact Markdown block ranges, source line maps, nested containers, and sections",
        r#"{"type":"object","required":["source"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "markdown_context",
        "Extract exact block-aligned Markdown context and enclosing sections",
        r#"{"type":"object","required":["source","start","end"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"neighbors":{"type":"integer","minimum":0}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_list_item",
        "Insert, replace, or remove a root or nested Markdown list item with structural validation and exact source identity",
        r#"{"type":"object","required":["source","path","action"],"oneOf":[{"properties":{"action":{"enum":["before","after"]}},"required":["marker","body"]},{"properties":{"action":{"const":"replace"},"marker":false},"required":["body"]},{"properties":{"action":{"const":"remove"},"marker":false,"body":false}}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"action":{"enum":["before","after","replace","remove"]},"marker":{"type":"string"},"body":{"type":"string"}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_section",
        "Plan a structurally validated Markdown section body replacement selected by root heading index or nested path",
        r#"{"type":"object","required":["source","body"],"oneOf":[{"required":["heading"],"properties":{"path":false}},{"required":["path"],"properties":{"heading":false}}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"heading":{"type":"integer","minimum":0},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"body":{"type":"string"}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_reference",
        "Plan replacement of a selected Markdown reference definition's decoded destination and title",
        r#"{"type":"object","required":["source","path","occurrence","destination","title"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"occurrence":{"type":"integer","minimum":0},"destination":{"type":"string"},"title":{"type":["string","null"]}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_table_column",
        "Plan a nested GFM column insertion, removal, or alignment change with structural validation",
        r#"{"type":"object","required":["source","path","action","index"],"oneOf":[{"properties":{"action":{"const":"insert"}},"required":["header","alignment","cells"]},{"properties":{"action":{"const":"remove"},"header":false,"alignment":false,"cells":false}},{"properties":{"action":{"const":"align"},"header":false,"cells":false},"required":["alignment"]}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"action":{"enum":["insert","remove","align"]},"index":{"type":"integer","minimum":0},"header":{"type":"string"},"alignment":{"enum":["default","left","right","center"]},"cells":{"type":"array","items":{"type":"string"}}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_table_row",
        "Plan insertion or removal of a GFM body row by nested table path and zero-based body index",
        r#"{"type":"object","required":["source","path","action","index"],"oneOf":[{"properties":{"action":{"const":"insert"}},"required":["cells"]},{"properties":{"action":{"const":"remove"}},"not":{"required":["cells"]}}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"action":{"enum":["insert","remove"]},"index":{"type":"integer","minimum":0},"cells":{"type":"array","minItems":1,"items":{"type":"string"}}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_table_cell",
        "Plan a source-preserving GFM cell edit by nested block path; row zero is the header",
        r#"{"type":"object","required":["source","path","row","column","markdown"],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"row":{"type":"integer","minimum":0},"column":{"type":"integer","minimum":0},"markdown":{"type":"string"}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_task",
        "Plan a single checkbox-state edit selected by root block index or nested block path",
        r#"{"type":"object","required":["source","checked"],"oneOf":[{"required":["block"]},{"required":["path"]}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"block":{"type":"integer","minimum":0},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"checked":{"type":"boolean"}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "plan_markdown_structure",
        "Plan a typed structural Markdown operation: section insert/remove/move/rename, list move/renumber, table row/column move, or reference add/relabel/remove/unlink",
        r#"{"type":"object","required":["source","op"],"oneOf":[{"properties":{"op":{"const":"section_insert"}},"required":["position","text"]},{"properties":{"op":{"const":"section_remove"}},"required":["heading"]},{"properties":{"op":{"const":"section_move"}},"required":["heading","position"]},{"properties":{"op":{"const":"section_rename"}},"required":["heading","title"]},{"properties":{"op":{"const":"list_move"}},"required":["path","to"]},{"properties":{"op":{"const":"list_renumber"}},"required":["path","start"]},{"properties":{"op":{"const":"table_row_move"}},"required":["path","index","to"]},{"properties":{"op":{"const":"table_column_move"}},"required":["path","index","to"]},{"properties":{"op":{"const":"reference_add"}},"required":["label","destination"]},{"properties":{"op":{"const":"reference_relabel"}},"required":["path","occurrence","new_label"]},{"properties":{"op":{"const":"reference_remove"}},"required":["path","occurrence"]},{"properties":{"op":{"const":"reference_unlink"}},"required":["path","occurrence"]}],"properties":{"source":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"op":{"enum":["section_insert","section_remove","section_move","section_rename","list_move","list_renumber","table_row_move","table_column_move","reference_add","reference_relabel","reference_remove","reference_unlink"]},"heading":{"type":"integer","minimum":0},"position":{"enum":["start","after"]},"after_heading":{"type":"integer","minimum":0},"new_level":{"type":"integer","minimum":1,"maximum":6},"title":{"type":["string","null"]},"text":{"type":"string"},"path":{"type":"array","minItems":1,"maxItems":128,"items":{"type":"integer","minimum":0}},"to":{"type":"integer","minimum":0},"start":{"type":"integer","minimum":0},"index":{"type":"integer","minimum":0},"label":{"type":"string"},"destination":{"type":"string"},"new_label":{"type":"string"},"occurrence":{"type":"integer","minimum":0}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        markdown_handler
    ),
    entry!(
        "decode_json",
        "Recover schema-valid JSON with effort bounds, ambiguity, and inference receipts",
        r#"{"type":"object","required":["input","schema"],"properties":{"input":{"type":"string"},"schema":{"type":["object","boolean"]},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"allow_guesses":{"type":"boolean"}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::decode(args, cancellation)
    ),
    entry!(
        "validate_json",
        "Validate a JSON instance against a schema, reporting violations, always-evaluated format/pattern evidence, and unsupported-vocabulary evidence",
        r#"{"type":"object","required":["schema","instance"],"properties":{"schema":{"type":["object","boolean"]},"instance":true,"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false,"required":["valid","violations","format_evidence","pattern_evidence","unsupported_vocabularies","receipt"],"properties":{"valid":{"type":"boolean"},"violations":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["instance_path","schema_path","keyword","message"],"properties":{"instance_path":{"type":"string"},"schema_path":{"type":"string"},"keyword":{"type":"string"},"message":{"type":"string"}}}},"format_evidence":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["validator","constraint","passed","diagnostics"],"properties":{"validator":{"type":"string"},"constraint":{"type":"string"},"passed":{"type":"boolean"},"diagnostics":{"type":"array","items":{"type":"string"}}}}},"pattern_evidence":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["rule","explanation","destination","span"],"properties":{"rule":{"type":"string"},"explanation":{"type":"string"},"destination":{"type":"string"},"span":true}}},"unsupported_vocabularies":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["uri","keywords"],"properties":{"uri":{"type":"string"},"keywords":{"type":"array","items":{"type":"string"}}}}},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer"},"termination":{"type":"object"}}}}}"#,
        false,
        |_, _, args, cancellation| super::validate::call(args, cancellation)
    ),
    entry!(
        "match_strings",
        "Rank caller-identified strings and aliases using exact, Unicode, lexical, distance, and optional English phonetic evidence",
        r#"{"type":"object","required":["query","candidates"],"properties":{"query":{"type":"string"},"candidates":{"type":"array","maxItems":8192,"items":{"oneOf":[{"type":"string"},{"type":"object","required":["id","text"],"properties":{"id":{"type":"string","minLength":1},"text":{"type":"string"},"aliases":{"type":"array","maxItems":256,"items":{"type":"string"}}},"additionalProperties":false}]}},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"english_phonetics":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":8192}},"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false,"required":["matches","receipt"],"properties":{"matches":{"type":"array"},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer"},"termination":{"type":"object"}}}}}"#,
        false,
        |_, _, args, cancellation| super::rank(args, cancellation)
    ),
    entry!(
        "resolve_string",
        "Select a caller-identified string with explicit exact, guess, ambiguity, and weak-match outcomes",
        r#"{"type":"object","required":["query","candidates"],"properties":{"query":{"type":"string"},"candidates":{"type":"array","maxItems":8192,"items":{"oneOf":[{"type":"string"},{"type":"object","required":["id","text"],"properties":{"id":{"type":"string","minLength":1},"text":{"type":"string"},"aliases":{"type":"array","maxItems":256,"items":{"type":"string"}}},"additionalProperties":false}]}},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"english_phonetics":{"type":"boolean"},"minimum_score":{"type":"integer","minimum":0,"maximum":10000},"minimum_margin":{"type":"integer","minimum":0,"maximum":10000}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::resolve_string(args, cancellation)
    ),
    entry!(
        "match_paths",
        "Resolve caller-supplied relative paths with context, explicit ambiguity, and guess receipts; never opens or authorizes a matched path",
        r#"{"type":"object","required":["query","paths"],"properties":{"query":{"type":"string"},"paths":{"type":"array","items":{"type":"string"}},"context":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"minimum_score":{"type":"integer","minimum":0,"maximum":10000},"minimum_margin":{"type":"integer","minimum":0,"maximum":20000}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::paths::resolve(args, cancellation)
    ),
    entry!(
        "resolve_intent",
        "Resolve explicit semantic domain rules against caller concepts; unknown words fail, conflicting interpretations remain ambiguous, and no model is contacted",
        r#"{"type":"object","required":["input","domain","concepts"],"properties":{"input":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"domain":{"type":"object","required":["id","rules"],"properties":{"id":{"type":"string"},"rules":{"type":"array","items":{"type":"object","required":["id","phrase","assignments"],"properties":{"id":{"type":"string"},"phrase":{"type":"string"},"assignments":{"type":"object","additionalProperties":{"type":"string"}}},"additionalProperties":false}},"ignored_words":{"type":"array","items":{"type":"string"}}},"additionalProperties":false},"concepts":{"type":"array","items":{"type":"object","required":["id","label","attributes"],"properties":{"id":{"type":"string"},"label":{"type":"string"},"attributes":{"type":"object","additionalProperties":{"type":"string"}}},"additionalProperties":false}}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::intent::resolve(args, cancellation)
    ),
    entry!(
        "plan_replacement",
        "Plan exact or bounded fuzzy text replacement; approximate selections remain guesses and ties remain ambiguous",
        r#"{"type":"object","required":["source","needle","replacement"],"properties":{"source":{"type":"string"},"needle":{"type":"string","minLength":1},"replacement":{"type":"string"},"max_edits":{"type":"integer","minimum":0,"maximum":64},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        PLANNED_OUTPUT,
        false,
        |_, _, args, cancellation| plan_replacement(args, cancellation)
    ),
    entry!(
        "plan_diff",
        "Plan a single-file unified diff with relocatable exact context and source-bound edit ranges",
        r#"{"type":"object","required":["source","patch"],"properties":{"source":{"type":"string"},"patch":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        PLANNED_OUTPUT,
        false,
        |_, _, args, cancellation| plan_diff(args, cancellation)
    ),
    entry!(
        "inspect_code",
        "Inspect grammar acceptance, source-bound declaration ranges, JSX rename repair options, and parse allocator accounting without executing code",
        r#"{"type":"object","required":["source","language"],"properties":{"source":{"type":"string"},"language":{"enum":["python","rust","javascript","typescript","tsx"]},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::inspect::call(args, cancellation)
    ),
    entry!(
        "read_file",
        "Read a bounded root-relative UTF-8 file and its exact digest",
        r#"{"type":"object","required":["path"],"properties":{"path":{"type":"string"}},"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false,"required":["path","source","digest"],"properties":{"path":{"type":"string"},"source":{"type":"string"},"digest":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"}}}"#,
        false,
        |tools, _, args, cancellation| tools.read_file(args, cancellation)
    ),
    entry!(
        "apply_edit",
        "Apply one caller-selected source-bound edit plan to a buffer, optionally validate code syntax, optionally rebase a stale plan onto an intervening change, and return text, exact inverse, and a receipt; no file writes",
        r#"{"type":"object","required":["source","plan"],"properties":{"source":{"type":"string"},"plan":{"type":"object","required":["source","edits"],"properties":{"source":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"edits":{"type":"array","items":{"type":"object","required":["start","end","replacement"],"properties":{"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"replacement":{"type":"string"}},"additionalProperties":false}}},"additionalProperties":false},"rebase_from":{"type":"object","required":["source"],"properties":{"source":{"type":"string"}},"additionalProperties":false},"language":{"enum":["text","python","rust","javascript","typescript","tsx"]},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::edits::apply(args, cancellation)
    ),
    entry!(
        "apply_file",
        "Conditionally apply one selected edit plan or a replacement buffer, optionally rebase a stale plan onto an intervening change, validate supported code grammar, journal it, and return undo identity",
        r#"{"type":"object","required":["path","expected_digest"],"oneOf":[{"required":["replacement"]},{"required":["plan"]}],"properties":{"path":{"type":"string"},"expected_digest":{"type":"string"},"replacement":{"type":"string"},"plan":{"type":"object","required":["source","edits"],"properties":{"source":{"type":"string","pattern":"^sha256:[0-9a-f]{64}$"},"edits":{"type":"array","items":{"type":"object","required":["start","end","replacement"],"properties":{"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0},"replacement":{"type":"string"}},"additionalProperties":false}}},"additionalProperties":false},"rebase_from":{"type":"object","required":["source"],"properties":{"source":{"type":"string"}},"additionalProperties":false},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false,"required":["commit","receipt"],"properties":{"commit":true,"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer"},"termination":{"type":"object"}}}}}"#,
        true,
        |tools, _, args, cancellation| tools.apply(args, cancellation)
    ),
    entry!(
        "undo_file",
        "Undo a journaled file replacement only if its exact output is still present",
        r#"{"type":"object","required":["transaction"],"properties":{"transaction":{"type":"string"}},"additionalProperties":false}"#,
        COMMIT_OUTPUT,
        true,
        |tools, _, args, cancellation| tools.undo_file(args, cancellation)
    ),
    entry!(
        "recover_files",
        "Reconcile prepared journals with actual file versions without overwriting files, removing evidence-bearing orphan temporaries",
        r#"{"type":"object","additionalProperties":false}"#,
        r#"{"type":"array","items":{"type":"object","additionalProperties":false,"required":["kind","id","reason"],"properties":{"kind":{"enum":["finalized","not_applied","conflict","corrupt","orphan_removed","orphan_unknown"]},"id":{"type":"string"},"reason":{"type":["string","null"]}}}}"#,
        true,
        |tools, _, args, cancellation| tools.recover_files(args, cancellation)
    ),
    entry!(
        "commit_files",
        "Independently journal and commit each of several file replacements, never as one atomic multi-file transaction",
        r#"{"type":"object","required":["files"],"properties":{"files":{"type":"array","minItems":1,"items":{"type":"object","required":["path","expected_digest","replacement"],"properties":{"path":{"type":"string"},"expected_digest":{"type":"string"},"replacement":{"type":"string"}},"additionalProperties":false}}},"additionalProperties":false}"#,
        r#"{"type":"array","items":{"type":"object","additionalProperties":false,"required":["index","path","status","commit","error"],"properties":{"index":{"type":"integer","minimum":0},"path":{"type":"string"},"status":{"enum":["committed","error"]},"commit":true,"error":{"type":["string","null"]}}}}"#,
        true,
        |tools, _, args, cancellation| tools.commit_files(args, cancellation)
    ),
    entry!(
        "prepare_helper",
        "Prepare a non-streaming completion request for a caller-selected provider/model/schema/input, performing no network I/O; the caller's own transport sends the returned request",
        r#"{"type":"object","required":["provider","model","input","schema","max_output_tokens"],"properties":{"provider":{"enum":["openai_responses","anthropic_messages","chat_completions"]},"model":{"type":"string","minLength":1},"input":{"type":"string"},"schema":{"type":["object","boolean"]},"max_output_tokens":{"type":"integer","minimum":1}},"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false,"required":["provider","path","headers","body","pending","receipt"],"properties":{"provider":{"type":"string"},"path":{"type":"string"},"headers":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["name","value"],"properties":{"name":{"type":"string"},"value":{"type":"string"}}}},"body":{"type":"string"},"pending":{"type":"string"},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer"},"termination":{"type":"object"}}}}}"#,
        false,
        |_, _, args, cancellation| super::helper::prepare(args, cancellation)
    ),
    entry!(
        "complete_helper",
        "Resume a prepared helper request with its caller-observed HTTP status and body, returning a recovered outcome with a receipt; refusals and non-completed responses remain unsuccessful",
        r#"{"type":"object","required":["pending","status","body"],"properties":{"pending":{"type":"string","minLength":1},"status":{"type":"integer","minimum":100,"maximum":599},"body":{"type":"string"}},"additionalProperties":false}"#,
        ASSIST_RESULT_OUTPUT,
        false,
        |_, _, args, cancellation| super::helper::complete(args, cancellation)
    ),
];

fn markdown_handler(
    _: &Tools,
    name: &str,
    args: &Node,
    cancellation: Option<&CancellationToken>,
) -> Result<Node, String> {
    super::markdown::call(name, args, cancellation)
}

fn plan_replacement(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let max_edits = match args.get("max_edits").map(|node| &node.value) {
        Some(Value::Number(number)) => number.to_usize().ok_or("max_edits is out of range")?,
        None => 0,
        _ => return Err("max_edits must be an integer".into()),
    };
    let effort = super::effort(args)?;
    let mut result = purrassist::edits::locate_fuzzy_governed(
        super::field(args, "source")?,
        super::field(args, "needle")?,
        super::field(args, "replacement")?,
        max_edits,
        &mut super::governor(effort.budget(), cancellation),
    );
    result.receipt.effort = effort;
    Ok(super::planned(result))
}
fn plan_diff(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let effort = super::effort(args)?;
    let mut result = purrassist::edits::plan_unified_governed(
        super::field(args, "source")?,
        super::field(args, "patch")?,
        &mut super::governor(effort.budget(), cancellation),
    );
    result.receipt.effort = effort;
    Ok(super::planned(result))
}

/// Tool definitions suitable for MCP discovery, including write/read
/// annotations and this registry's exact input/output schemas.
///
/// # Errors
/// Returns an error if a registered schema is malformed.
pub(super) fn definitions() -> Result<Node, String> {
    let mut definitions = Vec::with_capacity(ENTRIES.len());
    for entry in ENTRIES {
        definitions.push(object([
            ("name", string(entry.name)),
            ("description", string(entry.description)),
            (
                "inputSchema",
                json::parse(entry.input_schema).map_err(|e| e.to_string())?,
            ),
            (
                "outputSchema",
                json::parse(entry.output_schema).map_err(|e| e.to_string())?,
            ),
            (
                "annotations",
                object([
                    ("readOnlyHint", Node::new(Value::Bool(!entry.writes))),
                    ("destructiveHint", Node::new(Value::Bool(entry.writes))),
                    ("openWorldHint", Node::new(Value::Bool(false))),
                ]),
            ),
        ]));
    }
    Ok(array(definitions))
}

pub(super) fn contains(name: &str) -> bool {
    ENTRIES.iter().any(|entry| entry.name == name)
}

/// Validate `args` against `name`'s registered input schema, then run its
/// handler. The single dispatch path shared by `Tools::call`,
/// `Tools::call_with_cancellation`, and (through those) the CLI and MCP.
///
/// # Errors
/// Rejects unknown operations, schema-invalid arguments, and every error the
/// selected handler itself returns.
pub(super) fn call(
    tools: &Tools,
    name: &str,
    args: &Node,
    cancellation: Option<&CancellationToken>,
) -> Result<Node, String> {
    let mut validation_governor = super::governor(Budget::default(), cancellation);
    validation_governor.check().map_err(|e| e.to_string())?;
    let entry = ENTRIES
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("unknown tool {name:?}"))?;
    let schema = Schema::compile_governed(entry.input_schema, &mut validation_governor)
        .map_err(|e| e.to_string())?;
    let validation = schema
        .validate_with(args, &mut validation_governor)
        .map_err(|e| e.to_string())?;
    if !validation.valid {
        return Err(format!(
            "tool arguments violate schema: {:?}",
            validation.violations
        ));
    }
    (entry.handler)(tools, name, args, cancellation)
}
