// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Call markup no allowed parser reads: a linear, first-party detector.
//!
//! Recovery reads calls only in the dialects of the request's allow-list.
//! Call markup of another family would otherwise stay plain content with
//! nothing to tell the integrator that a call was there. [`detect_unparsed`]
//! scans the content of a recovered turn for the distinctive opener or
//! shape of each [`MarkupFamily`] and reports every hit (recovery turns each
//! into a rejected attempt with
//! [`Rejection::UnsupportedDialect`](super::Rejection::UnsupportedDialect),
//! and the bytes stay content). Two kinds of family are detected:
//!
//! - families no built-in dialect parses yet: the *stripped* renderings of
//!   built-in dialects whose special tokens a detokenizer removed and which
//!   no built-in grammar reads (Kimi-K2 `functions.NAME:IDX{...}`, Harmony
//!   `to=functions.NAME` without its tokens, DeepSeek-V3 `functionNAME` with
//!   a fenced JSON block, and DeepSeek-V3.1 / Mistral v11 `NAME{...}` at the
//!   start of a line); the other stripped renderings (Mistral v3 arrays,
//!   Llama 3 objects without `<|python_tag|>`, Hermes objects without their
//!   tags, Qwen3-Coder `<function=NAME>` elements without `<tool_call>`) are
//!   read by `bare-json.v1`, `llama3-json.v1` and `qwen3-coder-xml.v1`;
//! - built-in dialects that are not in the allow-list (`[TOOL_CALLS]` under
//!   a Hermes-only request, for example).
//!
//! The detector never parses a call. Each family is recognized by a short,
//! bounded check at an anchor byte: a literal token, or a call-shaped
//! identifier followed by `(` and a keyword argument for Pythonic lists, and
//! only at a plausible position (the start of the turn or of a line, or
//! right after `<|python_start|>` / `<|python_tag|>`), so
//! prose with parentheses, math, and Python code blocks are never flagged.
//! A built-in family whose dialect is allowed is never reported: its parser
//! read the bytes and kept them as content on evidence (a data object with
//! a `"name"` member, a pasted tool declaration). JSON call objects whose
//! arguments have a JSON Schema's shape (`"type": "object"`) are not
//! reported either.
//!
//! Only content is scanned, so a span a call, a rejected attempt or a
//! reasoning block already holds is never reported twice, and every
//! reported span lies inside one content range. After a hit the scan
//! resumes at its end; every check reads a bounded window or, to find a
//! hit's end, at most the rest of its content range, which the scan then
//! skips. The scan is therefore linear in the content length; it charges
//! one `Work` unit per byte it reads.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188).
#![allow(clippy::redundant_pub_crate)]
use super::{DialectId, DialectSet};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use core::ops::Range;

/// A family of call markup the detector recognizes.
///
/// ```
/// use purrassist_core::tool_call::{DialectId, MarkupFamily};
/// assert_eq!(MarkupFamily::Pythonic.id(), "pythonic.v1");
/// assert!(MarkupFamily::Pythonic.built_in());
/// assert_eq!(MarkupFamily::MistralV3.dialect(), Some(DialectId::MistralV3));
/// assert!(MarkupFamily::MistralV3.built_in());
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MarkupFamily {
    /// `anthropic-tool-use.v1`: a `"type": "tool_use"` block.
    AnthropicToolUse,
    /// `bare-json.v1`: a `{"name": ..., "arguments": ...}` object.
    BareJson,
    /// `deepseek-v3.1.v1`: a `DeepSeek` tool-calls section.
    DeepSeekV31,
    /// `deepseek-v3.v1`: a `DeepSeek` section whose calls have the
    /// `function` type.
    DeepSeekV3,
    /// `harmony.v1`: a Harmony message to `functions.NAME`.
    Harmony,
    /// `hermes.v1`: `<tool_call>` followed by a JSON object.
    Hermes,
    /// `kimi-k2.v1`: a Kimi-K2 tool-calls section or call token.
    KimiK2,
    /// `llama3-json.v1`: `<|python_tag|>`, or a `{"name": ...,
    /// "parameters": ...}` object.
    Llama3Json,
    /// `mistral-v11.v1`: `[TOOL_CALLS]` followed by a name.
    MistralV11,
    /// `mistral-v3.v1`: `[TOOL_CALLS]` followed by a JSON array or object.
    MistralV3,
    /// `openai-tool-calls.v1`: a `"function": {"name": ..., "arguments":
    /// ...}` member.
    OpenAiToolCalls,
    /// `qwen3-coder-xml.v1`: `<function=NAME>` in a `<tool_call>`.
    Qwen3CoderXml,
    /// `pythonic.v1`: a list of Python-style calls with
    /// keyword arguments, `[get_weather(city="Paris"), ...]`.
    Pythonic,
    /// `gemma3-tool-code.v1`: a `` ```tool_code `` fenced
    /// block holding a call.
    Gemma3ToolCode,
    /// `glm-4.5.v1`: `<arg_key>`/`<arg_value>` pairs in a
    /// `<tool_call>`.
    Glm45,
    /// `kimi-k2-stripped.v1`: a Kimi-K2 call with its
    /// special tokens removed, `functions.NAME:IDX{...}`.
    KimiK2Stripped,
    /// `harmony-stripped.v1`: a Harmony recipient
    /// `to=functions.NAME` with the message tokens removed.
    HarmonyStripped,
    /// `deepseek-v3-stripped.v1`: `functionNAME` followed by
    /// a `` ```json `` block, a DeepSeek-V3 call with its tokens removed.
    DeepSeekV3Stripped,
    /// `name-arguments-stripped.v1` category: `NAME{...}` at the start
    /// of a line, a DeepSeek-V3.1 or Mistral v11 call with its special
    /// tokens removed.
    NameArgumentsStripped,
}

static FAMILY_ANTHROPICTOOLUSE: [DialectId; 1] = [DialectId::AnthropicToolUse];
static FAMILY_BAREJSON: [DialectId; 1] = [DialectId::BareJson];
static FAMILY_DEEPSEEKV31: [DialectId; 1] = [DialectId::DeepSeekV31];
static FAMILY_DEEPSEEKV3: [DialectId; 1] = [DialectId::DeepSeekV3];
static FAMILY_GLM45: [DialectId; 1] = [DialectId::Glm45];
static FAMILY_HARMONY: [DialectId; 1] = [DialectId::Harmony];
static FAMILY_HERMES: [DialectId; 1] = [DialectId::Hermes];
static FAMILY_KIMIK2: [DialectId; 1] = [DialectId::KimiK2];
static FAMILY_LLAMA3JSON: [DialectId; 1] = [DialectId::Llama3Json];
static FAMILY_MISTRALV11: [DialectId; 1] = [DialectId::MistralV11];
static FAMILY_MISTRALV3: [DialectId; 1] = [DialectId::MistralV3];
static FAMILY_OPENAITOOLCALLS: [DialectId; 1] = [DialectId::OpenAiToolCalls];
static FAMILY_QWEN3CODERXML: [DialectId; 1] = [DialectId::Qwen3CoderXml];
static FAMILY_PYTHONIC: [DialectId; 1] = [DialectId::Pythonic];
static FAMILY_GEMMA3TOOLCODE: [DialectId; 1] = [DialectId::Gemma3ToolCode];
static FAMILY_KIMIK2STRIPPED: [DialectId; 1] = [DialectId::KimiK2Stripped];
static FAMILY_HARMONYSTRIPPED: [DialectId; 1] = [DialectId::HarmonyStripped];
static FAMILY_DEEPSEEKV3STRIPPED: [DialectId; 1] = [DialectId::DeepSeekV3Stripped];
static FAMILY_NAMEARGUMENTSSTRIPPED: [DialectId; 2] = [
    DialectId::DeepSeekV31Stripped,
    DialectId::MistralV11Stripped,
];

impl MarkupFamily {
    /// The family id: the dialect id of a built-in family, otherwise the
    /// versioned id of the format.
    ///
    /// See [`MarkupFamily`] for an example.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::AnthropicToolUse => "anthropic-tool-use.v1",
            Self::BareJson => "bare-json.v1",
            Self::DeepSeekV31 => "deepseek-v3.1.v1",
            Self::DeepSeekV3 => "deepseek-v3.v1",
            Self::Harmony => "harmony.v1",
            Self::Hermes => "hermes.v1",
            Self::KimiK2 => "kimi-k2.v1",
            Self::Llama3Json => "llama3-json.v1",
            Self::MistralV11 => "mistral-v11.v1",
            Self::MistralV3 => "mistral-v3.v1",
            Self::OpenAiToolCalls => "openai-tool-calls.v1",
            Self::Qwen3CoderXml => "qwen3-coder-xml.v1",
            Self::Pythonic => "pythonic.v1",
            Self::Gemma3ToolCode => "gemma3-tool-code.v1",
            Self::Glm45 => "glm-4.5.v1",
            Self::KimiK2Stripped => "kimi-k2-stripped.v1",
            Self::HarmonyStripped => "harmony-stripped.v1",
            Self::DeepSeekV3Stripped => "deepseek-v3-stripped.v1",
            Self::NameArgumentsStripped => "name-arguments-stripped.v1",
        }
    }

    /// The unique built-in dialect of the family, or `None` when its shape
    /// maps to several dialects. Use [`Self::dialects`] for the complete set.
    ///
    /// See [`MarkupFamily`] for an example.
    #[must_use]
    pub const fn dialect(self) -> Option<DialectId> {
        Some(match self {
            Self::AnthropicToolUse => DialectId::AnthropicToolUse,
            Self::BareJson => DialectId::BareJson,
            Self::DeepSeekV31 => DialectId::DeepSeekV31,
            Self::DeepSeekV3 => DialectId::DeepSeekV3,
            Self::Glm45 => DialectId::Glm45,
            Self::Harmony => DialectId::Harmony,
            Self::Hermes => DialectId::Hermes,
            Self::KimiK2 => DialectId::KimiK2,
            Self::Llama3Json => DialectId::Llama3Json,
            Self::MistralV11 => DialectId::MistralV11,
            Self::MistralV3 => DialectId::MistralV3,
            Self::OpenAiToolCalls => DialectId::OpenAiToolCalls,
            Self::Qwen3CoderXml => DialectId::Qwen3CoderXml,
            Self::Pythonic => DialectId::Pythonic,
            Self::Gemma3ToolCode => DialectId::Gemma3ToolCode,
            Self::KimiK2Stripped => DialectId::KimiK2Stripped,
            Self::HarmonyStripped => DialectId::HarmonyStripped,
            Self::DeepSeekV3Stripped => DialectId::DeepSeekV3Stripped,
            Self::NameArgumentsStripped => return None,
        })
    }

    /// Every built-in dialect whose grammar reads the detected shape.
    ///
    /// A bare name followed by arguments maps to both DeepSeek V3.1 and
    /// Mistral v11 projections; the detector cannot identify one from that shape.
    #[must_use]
    pub const fn dialects(self) -> &'static [DialectId] {
        match self {
            Self::AnthropicToolUse => &FAMILY_ANTHROPICTOOLUSE,
            Self::BareJson => &FAMILY_BAREJSON,
            Self::DeepSeekV31 => &FAMILY_DEEPSEEKV31,
            Self::DeepSeekV3 => &FAMILY_DEEPSEEKV3,
            Self::Glm45 => &FAMILY_GLM45,
            Self::Harmony => &FAMILY_HARMONY,
            Self::Hermes => &FAMILY_HERMES,
            Self::KimiK2 => &FAMILY_KIMIK2,
            Self::Llama3Json => &FAMILY_LLAMA3JSON,
            Self::MistralV11 => &FAMILY_MISTRALV11,
            Self::MistralV3 => &FAMILY_MISTRALV3,
            Self::OpenAiToolCalls => &FAMILY_OPENAITOOLCALLS,
            Self::Qwen3CoderXml => &FAMILY_QWEN3CODERXML,
            Self::Pythonic => &FAMILY_PYTHONIC,
            Self::Gemma3ToolCode => &FAMILY_GEMMA3TOOLCODE,
            Self::KimiK2Stripped => &FAMILY_KIMIK2STRIPPED,
            Self::HarmonyStripped => &FAMILY_HARMONYSTRIPPED,
            Self::DeepSeekV3Stripped => &FAMILY_DEEPSEEKV3STRIPPED,
            Self::NameArgumentsStripped => &FAMILY_NAMEARGUMENTSSTRIPPED,
        }
    }

    /// Whether a built-in grammar reads the family.
    ///
    /// See [`Self::dialects`] for shapes shared by more than one grammar.
    #[must_use]
    pub const fn built_in(self) -> bool {
        !self.dialects().is_empty()
    }

    /// A one-line explanation of the report: the family, and whether it is
    /// not built in yet or built in but not in the allow-list.
    ///
    /// ```
    /// use purrassist_core::tool_call::MarkupFamily;
    /// assert_eq!(
    ///     MarkupFamily::Glm45.message(),
    ///     "call markup of glm-4.5.v1, which is built in but not in the allow-list; its bytes stay content",
    /// );
    /// ```
    #[must_use]
    pub fn message(self) -> String {
        let why = if self.built_in() {
            "which is built in but not in the allow-list"
        } else {
            "which is not built in yet"
        };
        format!(
            "call markup of {}, {why}; its bytes stay content",
            self.id()
        )
    }
}

/// One detected piece of call markup: its family, where it was recognized
/// (`anchor`), and the span it covers (within one content range).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// The markup's family.
    pub family: MarkupFamily,
    /// The bytes the markup covers, inside one content range.
    pub span: Range<usize>,
}

/// Detect the call markup no dialect of `dialects` reads in the `content`
/// ranges of `output`.
///
/// It looks for the distinctive opener or shape of each
/// [`MarkupFamily`], skipping a built-in family whose dialect `dialects`
/// holds (its parser read those bytes and kept them as content on
/// evidence) and JSON call objects whose arguments are a JSON Schema. The
/// ranges are disjoint and ordered; the detections are ordered by span, and
/// each lies inside one range.
///
/// Nothing is parsed. Each family is recognized by a bounded check at an
/// anchor byte; a Pythonic list needs a call-shaped `NAME(` with a keyword
/// argument (or `()`) at the start of the turn or of a line, or right after
/// `<|python_start|>` / `<|python_tag|>`, so prose with parentheses, math
/// and Python code blocks are never flagged. After a hit the scan resumes
/// at its end, so the scan is linear in the content; it charges one `Work`
/// unit per byte read and [`DETECTION_BYTES`] scratch per detection.
///
/// # Errors
/// The governor's exhaustion or cancellation.
///
/// ```
/// use purrassist_core::tool_call::{DialectId, DialectSet, MarkupFamily, detect_unparsed};
/// use purrassist_core::{Budget, Governor};
/// let mut governor = Governor::new(Budget::default());
/// let hermes = DialectSet::new([DialectId::Hermes], &mut governor).unwrap();
/// let output = b"[TOOL_CALLS] [{\"name\": \"ls\", \"arguments\": {}}]";
/// let found = detect_unparsed(output, &[0..output.len()], &hermes, &mut governor)?;
/// assert_eq!(found[0].family, MarkupFamily::MistralV3);
/// // Prose with parentheses is never call markup.
/// let prose = b"Use f(x) = 2x (see [1]).";
/// assert!(detect_unparsed(prose, &[0..prose.len()], &hermes, &mut governor)?.is_empty());
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
pub fn detect_unparsed(
    output: &[u8],
    content: &[Range<usize>],
    dialects: &DialectSet,
    governor: &mut Governor,
) -> Result<Vec<Detection>, StopReason> {
    let turn_start = output
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(output.len());
    let mut found = Vec::new();
    for range in content {
        let mut scan = Scan {
            bytes: output,
            lo: range.start,
            hi: range.end.min(output.len()),
            turn_start,
            dialects,
            read: 0,
        };
        let mut at = scan.lo;
        while at < scan.hi {
            scan.read += 1;
            if let Some((family, span)) = scan.check(at) {
                governor.charge(Resource::ScratchBytes, DETECTION_BYTES)?;
                at = span.end.max(at + 1);
                found.push(Detection { family, span });
            } else {
                at += 1;
            }
        }
        governor.charge(Resource::Work, scan.read)?;
    }
    Ok(found)
}

/// Scratch bytes charged per detection.
pub const DETECTION_BYTES: u64 = 64;

/// How far a bounded check looks around its anchor.
const WINDOW: usize = 256;

/// How far the Harmony check looks for a message token around a
/// recipient (the tokens sit right beside it in a message header).
const TOKEN_WINDOW: usize = 64;

/// One content range being scanned.
struct Scan<'a> {
    bytes: &'a [u8],
    lo: usize,
    hi: usize,
    turn_start: usize,
    dialects: &'a DialectSet,
    /// Bytes read so far (the `Work` charged).
    read: u64,
}

impl Scan<'_> {
    /// Whether `family` is reported: not a built-in family whose dialect
    /// the allow-list holds. Checked before a hit's end is searched, so a
    /// family that is not reported costs only its bounded check.
    fn reported(&self, family: MarkupFamily) -> bool {
        !family
            .dialects()
            .iter()
            .any(|dialect| self.dialects.dialects().contains(dialect))
    }

    const fn byte(&mut self, at: usize) -> Option<u8> {
        if at < self.hi {
            self.read += 1;
            Some(self.bytes[at])
        } else {
            None
        }
    }

    fn lit(&mut self, at: usize, text: &[u8]) -> bool {
        let end = at + text.len();
        if end > self.hi {
            return false;
        }
        self.read += size(text.len());
        &self.bytes[at..end] == text
    }

    fn ws(&mut self, mut at: usize) -> usize {
        while self.byte(at).is_some_and(|byte| byte.is_ascii_whitespace()) {
            at += 1;
        }
        at
    }

    /// The end of an identifier (`[A-Za-z_][A-Za-z0-9_.-]*`) at `at`.
    fn ident(&mut self, at: usize) -> Option<usize> {
        if !self
            .byte(at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        {
            return None;
        }
        let mut end = at + 1;
        while self
            .byte(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        {
            end += 1;
        }
        Some(end)
    }

    /// The end of a JSON string at `at` (its closing quote included).
    fn string(&mut self, at: usize) -> Option<usize> {
        if self.byte(at) != Some(b'"') {
            return None;
        }
        let mut pos = at + 1;
        while let Some(byte) = self.byte(pos) {
            match byte {
                b'\\' => pos += 2,
                b'"' => return Some(pos + 1),
                _ => pos += 1,
            }
        }
        None
    }

    /// The end of the bracketed value opening at `at` (`{`, `[` or `(`),
    /// strings in `"` or `'` skipped; the content end when it never closes.
    const fn close(&mut self, at: usize) -> usize {
        let mut depth = 0_usize;
        let mut pos = at;
        let mut quote = None;
        while let Some(byte) = self.byte(pos) {
            pos += 1;
            match (quote, byte) {
                (Some(_), b'\\') => pos += 1,
                (Some(open), _) if byte == open => quote = None,
                (None, b'"' | b'\'') => quote = Some(byte),
                (None, b'{' | b'[' | b'(') => depth += 1,
                (None, b'}' | b']' | b')') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return pos;
                    }
                }
                (Some(_) | None, _) => {}
            }
        }
        self.hi
    }

    /// The end of the first occurrence of `needle` at or after `at`, or the
    /// content end.
    fn through(&mut self, at: usize, needle: &[u8]) -> usize {
        let mut pos = at;
        while let Some(byte) = self.byte(pos) {
            if byte == needle[0] && self.lit(pos, needle) {
                return pos + needle.len();
            }
            pos += 1;
        }
        self.hi
    }

    /// The first occurrence of `needle` in `at..at + WINDOW`.
    fn near(&mut self, at: usize, needle: &[u8]) -> Option<usize> {
        let stop = (at + WINDOW).min(self.hi);
        (at..stop).find(|&pos| self.byte(pos) == Some(needle[0]) && self.lit(pos, needle))
    }

    /// The end of the line holding `at` (its newline excluded).
    fn line_end(&mut self, at: usize) -> usize {
        let mut pos = at;
        while self.byte(pos).is_some_and(|byte| byte != b'\n') {
            pos += 1;
        }
        pos
    }

    /// Whether `at` begins a line: the content's start, or after a newline
    /// and optional spaces or tabs.
    const fn line_start(&mut self, at: usize) -> bool {
        let mut pos = at;
        while pos > self.lo {
            self.read += 1;
            match self.bytes[pos - 1] {
                b'\n' => return true,
                b' ' | b'\t' if at - pos < WINDOW => pos -= 1,
                _ => return false,
            }
        }
        self.lo == 0 || self.bytes[self.lo - 1] == b'\n'
    }

    /// The start of the line holding `at`, at most [`WINDOW`] bytes back
    /// and never before the content start.
    fn line_begin(&mut self, at: usize) -> usize {
        let floor = at.saturating_sub(WINDOW).max(self.lo);
        let mut pos = at;
        while pos > floor && self.bytes[pos - 1] != b'\n' {
            self.read += 1;
            pos -= 1;
        }
        pos
    }

    /// Whether one of Harmony's message tokens is within
    /// [`TOKEN_WINDOW`] bytes of `at`.
    fn harmony_tokens(&mut self, at: usize) -> bool {
        let from = at.saturating_sub(TOKEN_WINDOW).max(self.lo);
        let to = (at + TOKEN_WINDOW).min(self.hi);
        (from..to).any(|pos| {
            self.read += 1;
            self.bytes[pos..].starts_with(b"<|")
                && [
                    &b"<|channel|>"[..],
                    b"<|message|>",
                    b"<|constrain|>",
                    b"<|start|>",
                ]
                .iter()
                .any(|token| self.lit(pos, token))
        })
    }

    /// The family and span of call markup anchored at `at`, if any.
    fn check(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        let found = match self.bytes[at] {
            b'<' => self.tag(at),
            b'[' => self.bracket(at),
            b'{' => self.object(at),
            b'"' => self.member(at),
            b't' => self.recipient(at),
            b'f' => self.function(at),
            b'`' => self.tool_code(at),
            _ => None,
        };
        found.or_else(|| self.name_arguments(at))
    }

    /// A Harmony recipient `to=functions.NAME`: the built-in dialect's
    /// markup when a message token is near, otherwise its stripped form.
    fn recipient(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if !self.lit(at, b"to=functions.") {
            return None;
        }
        let family = if self.harmony_tokens(at) {
            MarkupFamily::Harmony
        } else {
            MarkupFamily::HarmonyStripped
        };
        if !self.reported(family) {
            return None;
        }
        let start = self.line_begin(at);
        let end = self.arguments_end(at);
        Some((family, start..end))
    }

    /// The end of the JSON arguments object that follows `at` within the
    /// window, or the end of the line.
    fn arguments_end(&mut self, at: usize) -> usize {
        match self.near(at, b"{") {
            Some(open) => self.close(open),
            None => self.line_end(at),
        }
    }

    /// Markup opening with `<`: the special tokens and tags of the
    /// built-in dialects, GLM-4.5's `<arg_key>`, and Pythonic lists after
    /// a Python tag.
    fn tag(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if self.lit(at, b"<tool_call>") {
            let body = self.ws(at + b"<tool_call>".len());
            let family = if self.lit(body, b"<function=") {
                MarkupFamily::Qwen3CoderXml
            } else if self.byte(body) == Some(b'{') {
                MarkupFamily::Hermes
            } else {
                let name = self.ident(body)?;
                let next = self.ws(name);
                if self.lit(next, b"<arg_key>") || self.lit(next, b"</tool_call>") {
                    MarkupFamily::Glm45
                } else {
                    return None;
                }
            };
            if !self.reported(family) {
                return None;
            }
            return Some((family, at..self.through(body, b"</tool_call>")));
        }
        if self.lit(at, b"<arg_key>") {
            let start = self.line_begin(at);
            return Some((
                MarkupFamily::Glm45,
                start..self.through(at, b"</tool_call>"),
            ));
        }
        if self.lit(at, b"<function=") {
            // A Qwen3-Coder XML function element, wrapped or bare (its
            // `<tool_call>` stripped): an allowed Qwen3-Coder XML attempts
            // every one, so it is reported only when the dialect is not
            // allowed, `>` or not.
            self.ident(at + b"<function=".len())?;
            if !self.reported(MarkupFamily::Qwen3CoderXml) {
                return None;
            }
            let family = MarkupFamily::Qwen3CoderXml;
            let mut end = self.through(at, b"</function>");
            let after = self.ws(end);
            if self.lit(after, b"</tool_call>") {
                end = after + b"</tool_call>".len();
            }
            return Some((family, at..end));
        }
        for tag in [&b"<|python_tag|>"[..], b"<|python_start|>"] {
            if self.lit(at, tag) {
                let body = self.ws(at + tag.len());
                if let Some(end) = self.pythonic(body) {
                    return self
                        .reported(MarkupFamily::Pythonic)
                        .then_some((MarkupFamily::Pythonic, at..end));
                }
                if tag == b"<|python_tag|>" && self.reported(MarkupFamily::Llama3Json) {
                    let end = self.arguments_end(body);
                    return Some((MarkupFamily::Llama3Json, at..end));
                }
                return None;
            }
        }
        for token in ["<｜tool▁calls▁begin｜>", "<｜tool▁call▁begin｜>"] {
            if self.lit(at, token.as_bytes()) {
                let family = if self.near(at, "function<｜tool▁sep｜>".as_bytes()).is_some() {
                    MarkupFamily::DeepSeekV3
                } else {
                    MarkupFamily::DeepSeekV31
                };
                if !self.reported(family) {
                    return None;
                }
                let end = self.through(at, "<｜tool▁calls▁end｜>".as_bytes());
                return Some((family, at..end));
            }
        }
        for token in [
            &b"<|tool_calls_section_begin|>"[..],
            b"<|tool_call_section_begin|>",
            b"<|tool_call_begin|>",
        ] {
            if self.lit(at, token) {
                if !self.reported(MarkupFamily::KimiK2) {
                    return None;
                }
                let end = self.through(at, b"_end|>");
                let end = if self.lit(end, b"<|tool_calls_section_end|>") {
                    end + b"<|tool_calls_section_end|>".len()
                } else {
                    end
                };
                return Some((MarkupFamily::KimiK2, at..end));
            }
        }
        None
    }

    /// Markup opening with `[`: Mistral's `[TOOL_CALLS]`, and Pythonic
    /// call lists at the start of the turn or as a whole line.
    fn bracket(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if self.lit(at, b"[TOOL_CALLS]") {
            let body = self.ws(at + b"[TOOL_CALLS]".len());
            return match self.byte(body)? {
                b'[' | b'{' => self
                    .reported(MarkupFamily::MistralV3)
                    .then(|| (MarkupFamily::MistralV3, at..self.close(body))),
                _ => {
                    self.ident(body)?;
                    if !self.reported(MarkupFamily::MistralV11) {
                        return None;
                    }
                    let end = match self.near(body, b"[ARGS]") {
                        Some(args) => self.arguments_end(args),
                        None => self.line_end(body),
                    };
                    Some((MarkupFamily::MistralV11, at..end))
                }
            };
        }
        // At the start of the turn or of a line only.
        if !self.reported(MarkupFamily::Pythonic) || (at != self.turn_start && !self.line_start(at))
        {
            return None;
        }
        let end = self.pythonic(at)?;
        Some((MarkupFamily::Pythonic, at..end))
    }

    /// The end of a Pythonic call list at `at`: `[`, a call-shaped
    /// identifier, `(`, and either `)` or a keyword argument `NAME=`.
    fn pythonic(&mut self, at: usize) -> Option<usize> {
        if self.byte(at) != Some(b'[') {
            return None;
        }
        let name = self.ws(at + 1);
        let open = self.ident(name)?;
        let open = self.ws(open);
        if self.byte(open) != Some(b'(') {
            return None;
        }
        let first = self.ws(open + 1);
        if self.byte(first) != Some(b')') {
            let key = self.ident(first)?;
            let equals = self.ws(key);
            if self.byte(equals) != Some(b'=') || self.byte(equals + 1) == Some(b'=') {
                return None;
            }
        }
        Some(self.close(at))
    }

    /// Markup opening with `{`: a JSON call object of Llama 3 JSON
    /// (`"parameters"`) or bare JSON (`"arguments"`), name first.
    fn object(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        let key = self.ws(at + 1);
        if !self.lit(key, b"\"name\"") {
            return None;
        }
        let colon = self.ws(key + b"\"name\"".len());
        if self.byte(colon) != Some(b':') {
            return None;
        }
        let name = self.ws(colon + 1);
        let comma = self.string(name)?;
        let comma = self.ws(comma);
        if self.byte(comma) != Some(b',') {
            return None;
        }
        let member = self.ws(comma + 1);
        let (family, key_end) = if self.lit(member, b"\"parameters\"") {
            (MarkupFamily::Llama3Json, member + b"\"parameters\"".len())
        } else if self.lit(member, b"\"arguments\"") {
            (MarkupFamily::BareJson, member + b"\"arguments\"".len())
        } else {
            return None;
        };
        let colon = self.ws(key_end);
        if self.byte(colon) != Some(b':') {
            return None;
        }
        let value = self.ws(colon + 1);
        if !matches!(self.byte(value), Some(b'{' | b'"'))
            || self.schema_shaped(value)
            || !self.reported(family)
        {
            return None;
        }
        Some((family, at..self.close(at)))
    }

    /// Whether the value at `at` is an object whose first member is
    /// `"type": "object"`: a JSON Schema, the declaration counter-evidence.
    fn schema_shaped(&mut self, at: usize) -> bool {
        if self.byte(at) != Some(b'{') {
            return false;
        }
        let key = self.ws(at + 1);
        if !self.lit(key, b"\"type\"") {
            return false;
        }
        let colon = self.ws(key + b"\"type\"".len());
        let value = self.ws(colon + 1);
        self.byte(colon) == Some(b':') && self.lit(value, b"\"object\"")
    }

    /// Markup opening with `"`: an Anthropic `"type": "tool_use"` member, or
    /// an `OpenAI` `"function": {"name": ..., "arguments": ...}` member.
    fn member(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if self.lit(at, b"\"type\"") {
            let colon = self.ws(at + b"\"type\"".len());
            let value = self.ws(colon + 1);
            if self.byte(colon) == Some(b':')
                && self.lit(value, b"\"tool_use\"")
                && self.reported(MarkupFamily::AnthropicToolUse)
            {
                let start = (at.saturating_sub(WINDOW).max(self.lo)..at)
                    .rev()
                    .find(|&pos| self.bytes[pos] == b'{')
                    .unwrap_or(at);
                return Some((MarkupFamily::AnthropicToolUse, start..self.close(start)));
            }
            return None;
        }
        if !self.lit(at, b"\"function\"") {
            return None;
        }
        let colon = self.ws(at + b"\"function\"".len());
        if self.byte(colon) != Some(b':') {
            return None;
        }
        let open = self.ws(colon + 1);
        if self.byte(open) != Some(b'{') {
            return None;
        }
        let key = self.ws(open + 1);
        let arguments = if self.lit(key, b"\"arguments\"") {
            true
        } else if self.lit(key, b"\"name\"") {
            let colon = self.ws(key + b"\"name\"".len());
            let name = self.ws(colon + 1);
            let comma = self.string(name).map(|end| self.ws(end));
            comma.is_some_and(|comma| {
                self.byte(comma) == Some(b',') && {
                    let next = self.ws(comma + 1);
                    self.lit(next, b"\"arguments\"")
                }
            })
        } else {
            false
        };
        (arguments && self.reported(MarkupFamily::OpenAiToolCalls))
            .then(|| (MarkupFamily::OpenAiToolCalls, at..self.close(open)))
    }

    /// Markup opening with `f`: a Kimi-K2 call id `functions.NAME:IDX`
    /// followed by its arguments with the tokens removed, or a DeepSeek-V3
    /// `functionNAME` line followed by a `` ```json `` block.
    fn function(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if self.reported(MarkupFamily::KimiK2Stripped) && self.lit(at, b"functions.") {
            let name = self.ident(at + b"functions.".len())?;
            let name = if self.byte(name) == Some(b':') {
                name
            } else {
                // A name holding `:`: read through the final `:IDX`.
                (name..(name + WINDOW).min(self.hi))
                    .take_while(|&pos| self.bytes[pos] != b'{')
                    .filter(|&pos| self.bytes[pos] == b':')
                    .last()?
            };
            let mut digits = name + 1;
            while self.byte(digits).is_some_and(|byte| byte.is_ascii_digit()) {
                digits += 1;
            }
            let open = self.ws(digits);
            if digits == name + 1 || self.byte(open) != Some(b'{') {
                return None;
            }
            return Some((MarkupFamily::KimiK2Stripped, at..self.close(open)));
        }
        if self.reported(MarkupFamily::DeepSeekV3Stripped)
            && self.lit(at, b"function")
            && self.line_start(at)
        {
            let name = self.ident(at + b"function".len())?;
            let fence = self.ws(name);
            if !self.bytes[name..fence].contains(&b'\n') || !self.lit(fence, b"```json") {
                return None;
            }
            let end = self.through(fence + b"```json".len(), b"```");
            return Some((MarkupFamily::DeepSeekV3Stripped, at..end));
        }
        None
    }

    /// A Gemma 3 `` ```tool_code `` fence whose first line is a call.
    fn tool_code(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if !self.lit(at, b"```tool_code")
            || !self.line_start(at)
            || !self.reported(MarkupFamily::Gemma3ToolCode)
        {
            return None;
        }
        let info = at + b"```tool_code".len();
        let line = self.line_end(info);
        if !self.bytes[info..line].iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        let call = self.ws(line);
        let open = self.ident(call)?;
        if self.byte(open) != Some(b'(') {
            return None;
        }
        Some((MarkupFamily::Gemma3ToolCode, at..self.through(open, b"```")))
    }

    /// `NAME{"...` at the start of a line: a DeepSeek-V3.1 or Mistral v11
    /// call whose special tokens were removed.
    fn name_arguments(&mut self, at: usize) -> Option<(MarkupFamily, Range<usize>)> {
        if !self.line_start(at) || !self.reported(MarkupFamily::NameArgumentsStripped) {
            return None;
        }
        let name_end = self.ident(at)?;
        let open = self.ws(name_end);
        if self.byte(open) != Some(b'{') {
            return None;
        }
        let key = self.ws(open + 1);
        if !matches!(self.byte(key), Some(b'"' | b'}')) {
            return None;
        }
        Some((MarkupFamily::NameArgumentsStripped, at..self.close(open)))
    }
}
