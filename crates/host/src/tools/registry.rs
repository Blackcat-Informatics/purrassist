// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! One declarative registry entry per operation: name, description, input
//! JSON Schema, output JSON Schema, write annotation, and handler. `Tools`'s
//! public `definitions`/`call`/`call_with_cancellation`, the CLI `call`
//! command, and MCP `tools/list`/`tools/call` all derive from [`ENTRIES`];
//! there is no second, hand-maintained dispatch path. Input arguments are
//! validated by this crate's own schema engine before a handler ever runs;
//! complete returned nodes pass their registered output schema before release.
use super::Tools;
use purrassist::{
    Budget, CancellationToken, Effort, Governor, Receipt, StopReason,
    contract::Resource,
    json::{self, Node, Value},
    schema::Schema,
    wire::{array, null, object, string},
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

/// [`ASSIST_RESULT_OUTPUT`] with `value` (or `null`) and every alternative
/// pinned to the `purrassist::tool_call::ToolCalls` wire shape, including the
/// optional `emitted` member a requested emission adds.
const TOOL_CALLS_OUTPUT: &str = r##"{"type":"object","additionalProperties":false,"required":["status","value","alternatives","receipt"],"properties":{"status":{"enum":["resolved","best_guess","ambiguous","unresolved"]},"value":{"oneOf":[{"type":"null"},{"$ref":"#/$defs/tool_calls"}]},"alternatives":{"type":"array","items":{"$ref":"#/$defs/tool_calls"}},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer","minimum":0},"source":{"type":"string"},"sources":{"type":"array"},"alternatives":{"type":"array"},"ambiguity":true,"operation":{"type":"string"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"budget":{"type":"object"},"usage":{"type":"object"},"evidence":{"type":"array"},"losses":{"type":"array"},"inferences":{"type":"array"},"validations":{"type":"array"},"termination":{"type":"object","required":["kind","detail"],"properties":{"kind":{"enum":["completed","no_candidate","budget_exceeded","cancelled","invalid_input"]},"detail":{"type":"string"}}}}}},"$defs":{"tool_calls":{"type":"object","additionalProperties":false,"required":["calls","rejected","embedded","content","reasoning","reasoning_blocks","finish","openai_finish_reason","tool_choice","next_id_start","verdict","usable_calls","usable_calls_accepting_guesses"],"properties":{"calls":{"type":"array","items":{"$ref":"#/$defs/call"}},"rejected":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["span","dialect","candidate_dialects","reason","raw","repairs"],"properties":{"span":{"$ref":"#/$defs/range"},"dialect":{"type":["string","null"]},"candidate_dialects":{"type":"array","uniqueItems":true,"items":{"type":"string"}},"reason":{"type":"object","additionalProperties":false,"required":["code","at"],"properties":{"code":{"enum":["malformed","invalid_utf8","unterminated","overlapped","unsupported_dialect"]},"at":{"type":["integer","null"],"minimum":0},"family":{"type":"string"},"built_in":{"type":"boolean"},"detail":{"type":"string"}}},"raw":{"$ref":"#/$defs/exact"},"repairs":{"$ref":"#/$defs/ledger"}}}},"embedded":{"type":"array","items":{"$ref":"#/$defs/call"}},"content":{"type":"array","items":{"$ref":"#/$defs/range"}},"reasoning":{"type":"array","items":{"$ref":"#/$defs/range"}},"reasoning_blocks":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["span","text"],"properties":{"span":{"$ref":"#/$defs/range"},"text":{"$ref":"#/$defs/range"}}}},"finish":{"enum":["calls","content_only","truncated_with_calls","truncated","unresolved"]},"openai_finish_reason":{"enum":["tool_calls","length","stop"]},"tool_choice":{"type":"object","additionalProperties":false,"required":["constraint","passed","diagnostics"],"properties":{"constraint":{"type":"string"},"passed":{"type":"boolean"},"diagnostics":{"type":"array","items":{"type":"string"}}}},"next_id_start":{"type":"integer","minimum":0},"verdict":{"enum":["run","no_calls","ambiguous","nothing_usable"]},"usable_calls":{"type":"integer","minimum":0},"usable_calls_accepting_guesses":{"type":"integer","minimum":0},"emitted":{"$ref":"#/$defs/emitted"}}},"call":{"type":"object","additionalProperties":false,"required":["index","id","source_id","tool","name","arguments","span","dialect","candidate_dialects","effects","policy","outcome","repairs","alternatives","distinguishing","keywords"],"properties":{"index":{"type":"integer","minimum":0},"id":{"type":"string"},"source_id":{"type":["string","null"]},"tool":{"type":["string","null"]},"name":{"type":["string","null"]},"arguments":true,"span":{"$ref":"#/$defs/range"},"dialect":{"type":"string"},"candidate_dialects":{"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"string"}},"effects":{"enum":["read_only","mutating",null]},"policy":{"enum":["structural","tolerant",null]},"outcome":{"type":"object","additionalProperties":false,"required":["status","reason"],"properties":{"status":{"enum":["resolved","best_guess","ambiguous","unresolved"]},"reason":{"$ref":"#/$defs/reason"}}},"repairs":{"$ref":"#/$defs/ledger"},"alternatives":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["tool","effects","policy","arguments","cost","repairs","validations","blocker"],"properties":{"tool":{"type":["string","null"]},"effects":{"enum":["read_only","mutating",null]},"policy":{"enum":["structural","tolerant",null]},"arguments":true,"cost":{"type":"integer","minimum":0},"repairs":{"$ref":"#/$defs/ledger"},"validations":{"type":"array"},"blocker":{"$ref":"#/$defs/reason"}}}},"distinguishing":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["kind","rule"],"properties":{"kind":{"enum":["evidence","loss","inference","validation"]},"rule":{"type":"string"}}}},"keywords":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["pointer","keyword","used"],"properties":{"pointer":{"type":"string"},"keyword":{"type":"string"},"used":{"type":"boolean"}}}}}},"range":{"type":"object","additionalProperties":false,"required":["start","end"],"properties":{"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0}}},"ledger":{"type":"object","additionalProperties":false,"required":["evidence","losses","inferences"],"properties":{"evidence":{"type":"array"},"losses":{"type":"array","items":{"type":"object","required":["kind","evidence","original"],"properties":{"kind":{"enum":["markup","data"]}}}},"inferences":{"type":"array"}}},"reason":{"oneOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["code","detail"],"properties":{"code":{"type":"string"},"detail":{"type":"string"}}}]},"exact":{"type":"object","additionalProperties":false,"minProperties":1,"maxProperties":1,"properties":{"text":{"type":"string"},"bytes_hex":{"type":"string","pattern":"^([0-9a-f]{2})*$"}}},"skipped":{"type":"object","required":["kind","index","reason"],"properties":{"kind":{"enum":["call","rejected"]},"index":{"type":"integer","minimum":0},"span":{"$ref":"#/$defs/range"},"reason":{"type":"object","additionalProperties":false,"required":["code","detail"],"properties":{"code":{"type":"string"},"detail":{"type":"string"}}}}},"emitted":{"oneOf":[{"type":"object","additionalProperties":false,"required":["target","dialect","output","calls","skipped"],"properties":{"target":{"const":"dialect"},"dialect":{"type":"string"},"output":{"$ref":"#/$defs/exact"},"calls":{"type":"array","items":{"type":"integer","minimum":0}},"skipped":{"type":"array","items":{"$ref":"#/$defs/skipped"}}}},{"type":"object","additionalProperties":false,"required":["target","output","calls","skipped"],"properties":{"target":{"const":"openai_tool_calls"},"output":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","type","function"],"properties":{"id":{"type":"string"},"type":{"const":"function"},"function":{"type":"object","additionalProperties":false,"required":["name","arguments"],"properties":{"name":{"type":"string"},"arguments":{"type":"string"}}}}}},"calls":{"type":"array","items":{"type":"integer","minimum":0}},"skipped":{"type":"array","items":{"$ref":"#/$defs/skipped"}}}}]}}}"##;

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
        "recover_tool_calls",
        "Recover the tool calls of one model turn against caller tool declarations (OpenAI, MCP, or Anthropic form) over built-in call dialects, with per-call outcomes, itemized repairs and losses, rejected attempts (including call markup of dialects not built in or not allowed, reported as unsupported_dialect), the tool-choice check, the turn's finish, optional re-emission, and a receipt; never executes a call. status is the certainty of the reading of the turn, not whether anything can run: read value.verdict (run, no_calls, ambiguous, nothing_usable) and value.usable_calls for that; both usable counts are 0 whenever value.tool_choice.passed is false, so a failed tool choice never leaves a call to run",
        r#"{"type":"object","required":["output","tools"],"properties":{"output":{"type":"string","description":"Model output text of one turn; the CLI recover-tool-calls command accepts the same output as raw bytes, recording invalid UTF-8 as a loss"},"tools":{"type":["array","object","string"],"description":"Tool declarations in tools_format: the declarations value, or a string holding the raw declaration JSON, which is read byte for byte (render-back and schema digests then refer to exactly those bytes; a structured value is written back compactly, keeping member order and number spellings)"},"tools_format":{"enum":["openai","mcp","anthropic"]},"dialects":{"type":"array","minItems":1,"uniqueItems":true,"items":{"enum":["anthropic-tool-use.v1","bare-json.v1","deepseek-v3-stripped.v1","deepseek-v3.1-stripped.v1","deepseek-v3.1.v1","deepseek-v3.v1","gemma3-tool-code.v1","glm-4.5.v1","harmony-stripped.v1","harmony.v1","hermes-stripped.v1","hermes.v1","kimi-k2-stripped.v1","kimi-k2.v1","llama3-json-stripped.v1","llama3-json.v1","mistral-v11-stripped.v1","mistral-v11.v1","mistral-v3-stripped.v1","mistral-v3.v1","openai-tool-calls.v1","pythonic.v1","qwen3-coder-xml.v1"]}},"stop":{"oneOf":[{"enum":["stop","length","tool_calls","content_filter","unknown"]},{"type":"object","required":["stop_sequence"],"properties":{"stop_sequence":{"type":"string"}},"additionalProperties":false}]},"tool_choice":{"type":["string","object"],"description":"OpenAI (\"auto\", \"none\", \"required\", or {\"type\":\"function\",\"function\":{\"name\":NAME}}) or Anthropic ({\"type\":\"auto\"|\"any\"|\"none\"|\"tool\",\"name\":NAME}) form; \"disable_parallel_tool_use\":true on a \"tool\" choice forces exactly one call"},"tool_choice_strict":{"type":"object","description":"Options of a strict forced tool_choice (valid only beside a forced tool_choice): exactly_one requires exactly one call, allow_content false forbids content other than ASCII whitespace, allow_reasoning false forbids reasoning blocks (an invalid_input error beside reasoning \"content\", where reasoning is never recognized); a rejected call attempt of any reason counts against exactly_one and allow_content false; exactly_one false contradicts an Anthropic disable_parallel_tool_use true","required":["exactly_one","allow_content","allow_reasoning"],"properties":{"exactly_one":{"type":"boolean"},"allow_content":{"type":"boolean"},"allow_reasoning":{"type":"boolean"}},"additionalProperties":false},"policy":{"enum":["per-tool","structural","tolerant"]},"effects":{"type":"object","additionalProperties":{"enum":["read-only","mutating"]}},"reasoning":{"enum":["separate","separate-open","content"]},"ambiguity":{"enum":["report","reject"]},"ids":{"description":"Call ids. Default (omitted): keep each id the model wrote, and number every other call call_<12 hex digits of the output's SHA-256>_<index>, so different turns never share ids without caller state; a missing or repeated model id is numbered and recorded as a tool_call.id.dedup inference. \"source\": the output's ids, deduplicated as <id>#<index>. {\"prefix\", \"start\"}: number every call prefix+(start+index) (prefix defaults to call_, start to 0); pass the result's next_id_start as the next round's start","oneOf":[{"const":"source"},{"type":"object","properties":{"prefix":{"type":"string"},"start":{"type":"integer","minimum":0}},"additionalProperties":false}]},"emit":{"enum":["none","openai","anthropic-tool-use.v1","bare-json.v1","deepseek-v3-stripped.v1","deepseek-v3.1-stripped.v1","deepseek-v3.1.v1","deepseek-v3.v1","gemma3-tool-code.v1","glm-4.5.v1","harmony-stripped.v1","harmony.v1","hermes-stripped.v1","hermes.v1","kimi-k2-stripped.v1","kimi-k2.v1","llama3-json-stripped.v1","llama3-json.v1","mistral-v11-stripped.v1","mistral-v11.v1","mistral-v3-stripped.v1","mistral-v3.v1","openai-tool-calls.v1","pythonic.v1","qwen3-coder-xml.v1"]},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        TOOL_CALLS_OUTPUT,
        false,
        |_, _, args, cancellation| super::tool_calls::call(args, cancellation)
    ),
    entry!(
        "project_result",
        "Validate the complete original JSON result against its output schema, apply the closed context field mask, and emit an exact-byte checked JSON Lines artifact; structured refusals retain original source, requested controls, stage and typed cause. The byte limit covers only decoded artifact text, excluding protocol and receipt framing; no tool is executed or authorized",
        super::project_result::INPUT_SCHEMA,
        super::project_result::OUTPUT_SCHEMA,
        false,
        |_, _, args, cancellation| super::project_result::call(args, cancellation)
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
        "generate_json_witness",
        "Generate a witness instance for a JSON Schema location, prove unsatisfiability with refutation evidence, or report explicit search exhaustion; never claims unsatisfiability from a governor hard stop",
        r#"{"type":"object","required":["schema"],"properties":{"schema":{"type":["object","boolean"]},"resources":{"type":"object","additionalProperties":{"type":["object","boolean"]}},"effort":{"enum":["exact","quick","standard","deep","maximum"]}},"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false,"required":["outcome","witness","refutation","receipt"],"properties":{"outcome":{"enum":["witness","unsatisfiable","exhausted"]},"witness":true,"refutation":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["rule","explanation","destination","span"],"properties":{"rule":{"type":"string"},"explanation":{"type":"string"},"destination":{"type":"string"},"span":true}}},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer"},"termination":{"type":"object"}}}}}"#,
        false,
        |_, _, args, cancellation| super::witness::call(args, cancellation)
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
        "Reconcile prepared journals without overwriting targets; remove only bounded compatible temporary bytes beside a validated journal's recorded target, preserving uncertain artifacts with reasons",
        r#"{"type":"object","additionalProperties":false}"#,
        r#"{"type":"array","items":{"type":"object","additionalProperties":false,"required":["kind","id","reason"],"properties":{"kind":{"enum":["finalized","not_applied","conflict","corrupt","orphan_removed","orphan_unknown","orphan_preserved"]},"id":{"type":"string"},"reason":{"type":["string","null"]}}}}"#,
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

/// Prepare both selected schemas and validate arguments before invoking once.
/// Complete returned nodes pass their output schema before successful release.
/// This is the single dispatch path used by Rust, CLI and MCP callers.
pub(super) fn call(
    tools: &Tools,
    name: &str,
    args: &Node,
    cancellation: Option<&CancellationToken>,
) -> Result<Node, String> {
    let mut admission = super::governor(Budget::default(), cancellation);
    admission.check().map_err(|e| e.to_string())?;
    let entry = ENTRIES
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("unknown tool {name:?}"))?;
    // Once a handler returns, bounded validation completes despite late signals.
    // In particular, a completed journaled transaction must retain its outcome.
    entry.invoke(
        tools,
        args,
        cancellation,
        &mut admission,
        OutputPolicy::Automatic,
    )
}

/// Automatic output validation scales with returned structure, independently
/// of the handler's input budget. Explicit governors retain their exact limits.
enum OutputPolicy<'a> {
    Automatic,
    Explicit(&'a mut Governor),
}

impl<'a> From<&'a mut Governor> for OutputPolicy<'a> {
    fn from(governor: &'a mut Governor) -> Self {
        Self::Explicit(governor)
    }
}

/// Size only the default output Work allowance with a bounded, allocation-free
/// traversal. The 256 units per node are a scheduling policy for registered
/// schemas, not a guarantee that every schema or output completes validation.
/// All other limits stay at their defaults; sizing failures retain their
/// original governor so the refusal reports the actual limit and usage.
/// Each cursor step is admitted, including unwinding: a completed traversal
/// charges twice the node count minus one. The fixed stack and the automatic
/// governor both use the Standard default depth, with the root at depth zero.
fn size_output_work(original: &Node, governor: &mut Governor) -> Result<(), StopReason> {
    const DEPTH: usize = Effort::Standard.budget().depth as usize;
    let mut cursors: [Option<(&Node, usize)>; DEPTH + 1] = [None; DEPTH + 1];
    cursors[0] = Some((original, 0));
    let mut depth = 0;
    let mut maximum_depth = 0;
    let mut nodes = 1_u64;
    loop {
        governor.charge(Resource::Work, 1)?;
        let (node, next) = cursors[depth].expect("INVARIANT: active output cursor exists");
        let child = match &node.value {
            Value::Array(items) => items.get(next),
            Value::Object(members) => members.get(next).map(|member| &member.value),
            _ => None,
        };
        if let Some(child) = child {
            cursors[depth] = Some((node, next + 1));
            governor.charge(Resource::Depth, (depth + 1) as u64)?;
            depth += 1;
            maximum_depth = maximum_depth.max(depth);
            nodes = nodes.saturating_add(1);
            cursors[depth] = Some((child, 0));
        } else if depth == 0 {
            break;
        } else {
            cursors[depth] = None;
            depth -= 1;
        }
    }
    let sizing_work = governor.usage().work;
    let mut limits = *governor.limits();
    limits.work = limits.work.saturating_add(nodes.saturating_mul(256));
    *governor = Governor::new(limits);
    governor.charge(Resource::Work, sizing_work)?;
    governor.charge(Resource::Depth, maximum_depth as u64)
}

impl Entry {
    fn invoke<'a>(
        &self,
        tools: &Tools,
        args: &Node,
        cancellation: Option<&CancellationToken>,
        admission: &mut Governor,
        output: impl Into<OutputPolicy<'a>>,
    ) -> Result<Node, String> {
        admission.check().map_err(|e| e.to_string())?;
        let input_schema = Schema::compile_governed(self.input_schema, admission)
            .map_err(|error| error.to_string())?;
        let output_schema = Schema::compile_governed(self.output_schema, admission)
            .map_err(|error| error.to_string())?;
        let validation = input_schema
            .validate_with(args, admission)
            .map_err(|error| error.to_string())?;
        if !validation.valid {
            return Err(format!(
                "tool arguments violate schema: {:?}",
                validation.violations
            ));
        }
        admission.check().map_err(|error| error.to_string())?;
        let original = (self.handler)(tools, self.name, args, cancellation)?;
        let mut automatic = Governor::new(Budget::default());
        let (output, sized) = match output.into() {
            OutputPolicy::Automatic => {
                let sized = size_output_work(&original, &mut automatic);
                (&mut automatic, sized)
            }
            OutputPolicy::Explicit(output) => (output, Ok(())),
        };
        match sized.and_then(|()| output_schema.validate_with(&original, output)) {
            Ok(report) if report.valid => Ok(original),
            verdict => {
                let (reason, report, stop) = match verdict {
                    Ok(report) => (
                        object([("kind", string("schema_violation"))]),
                        super::validate::report_node(&report),
                        StopReason::InvalidInput(
                            "registered output schema rejected returned result".into(),
                        ),
                    ),
                    Err(reason) => (
                        super::validate::resource(&reason, "output_validation"),
                        null(),
                        reason,
                    ),
                };
                let mut admitted = Receipt::new(
                    self.input_schema.as_bytes(),
                    "host.registry.admission.v1",
                    Effort::Standard,
                    *admission.limits(),
                );
                admitted.usage = admission.usage();
                let mut checked = Receipt::new(
                    self.output_schema.as_bytes(),
                    "host.registry.output-validation.v1",
                    Effort::Standard,
                    *output.limits(),
                );
                checked.usage = output.usage();
                checked.stopped = stop;
                // Host error materialization remains outside these phase budgets,
                // including after a native stop. Move the original node intact:
                // no serialize/parse round trip can collapse duplicate members.
                Err(object([
                    ("code", string("registered_output_refused")),
                    ("stage", string("output_validation")),
                    ("tool", string(self.name)),
                    ("schema", string(output_schema.digest().to_string())),
                    ("handler_returned", Node::new(Value::Bool(true))),
                    ("effects_may_have_occurred", Node::new(Value::Bool(self.writes))),
                    ("message", string("handler returned successfully; result refused; effects may already have occurred; no retry or rollback was performed")),
                    ("reason", reason),
                    ("validation", report),
                    ("admission", admitted.to_node()),
                    ("output_validation", checked.to_node()),
                    ("original_output", original),
                ]).to_json())
            }
        }
    }
}

#[cfg(test)]
mod tests;
