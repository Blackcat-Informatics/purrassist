// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Dialect identities and per-model dialect allow-lists.
//!
//! A [`DialectId`] names one compiled dialect grammar. A [`DialectSet`] is
//! the allow-list for one model: which dialects the recognizer runs,
//! concurrently, over that model's output. It is configured separately from
//! the tools a [`ToolSet`](super::ToolSet) declares, because the dialect
//! belongs to the model and its chat template while the same tools are
//! offered to many models.
//!
//! A set is canonical: its dialects are ordered by `(id, digest)` whatever
//! order the allow-list named them in, so permuting an allow-list never
//! changes a recognition result.
use crate::contract::saturating_size as size;
use crate::contract::{
    Budget, Governor, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason,
};
use crate::digest::Digest;
use crate::grammar::automaton::Matcher;
use crate::grammar::cost::K_COMPILE;
use crate::grammar::{CompiledGrammar, Grammar};
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

/// One dialect grammar the recognizer can run.
///
/// The set of variants is closed, so the recognizer dispatches statically.
/// Thirteen dialects are built in, each a first-party grammar embedded in the
/// crate (see `docs/tool-calls/dialects/` for each grammar's public sources
/// and what it deliberately does not accept):
///
/// | Variant | Versioned id | Shape |
/// | --- | --- | --- |
/// | [`DialectId::BareJson`] | `bare-json.v1` | an unwrapped `{"name": NAME, "arguments": {...}}` object, alone, in prose, fenced, or in an array |
/// | [`DialectId::Qwen3CoderXml`] | `qwen3-coder-xml.v1` | `<tool_call><function=NAME><parameter=P>VALUE</parameter></function></tool_call>`, or the bare `<function=NAME>` element |
/// | [`DialectId::AnthropicToolUse`] | `anthropic-tool-use.v1` | `tool_use` content blocks `{"type": "tool_use", "id": ID, "name": NAME, "input": {...}}` |
/// | [`DialectId::DeepSeekV31`] | `deepseek-v3.1.v1` | `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>NAME<｜tool▁sep｜>{...}<｜tool▁call▁end｜>...<｜tool▁calls▁end｜>` |
/// | [`DialectId::DeepSeekV3`] | `deepseek-v3.v1` | `<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>NAME` + a fenced `json` block + `<｜tool▁call▁end｜>...<｜tool▁calls▁end｜>` |
/// | [`DialectId::KimiK2`] | `kimi-k2.v1` | `<\|tool_calls_section_begin\|><\|tool_call_begin\|>functions.NAME:IDX<\|tool_call_argument_begin\|>{...}<\|tool_call_end\|>...<\|tool_calls_section_end\|>` |
/// | [`DialectId::Harmony`] | `harmony.v1` | `<\|channel\|>commentary to=functions.NAME <\|constrain\|>json<\|message\|>{...}<\|call\|>` |
/// | [`DialectId::Glm45`] | `glm-4.5.v1` | `<tool_call>NAME<arg_key>KEY</arg_key><arg_value>VALUE</arg_value></tool_call>` |
/// | [`DialectId::Hermes`] | `hermes.v1` | `<tool_call>{"name": NAME, "arguments": {...}}</tool_call>` |
/// | [`DialectId::Llama3Json`] | `llama3-json.v1` | `{"name": NAME, "parameters": {...}}`, optionally after `<\|python_tag\|>` |
/// | [`DialectId::MistralV11`] | `mistral-v11.v1` | `[TOOL_CALLS]NAME[CALL_ID]ID[ARGS]{...}`, the `[CALL_ID]` segment optional, repeatable |
/// | [`DialectId::MistralV3`] | `mistral-v3.v1` | `[TOOL_CALLS] [{"name": NAME, "arguments": {...}, "id": ID}, ...]` |
/// | [`DialectId::OpenAiToolCalls`] | `openai-tool-calls.v1` | `tool_calls` elements `{"id": ID, "type": "function", "function": {...}}` |
///
/// A built-in grammar is compiled once, on first use, under its own
/// governor with a default [`Budget`]; that compilation is never charged to
/// a caller's governor, so every charge a caller sees depends only on the
/// bytes it processes. A caller-supplied grammar is [`DialectId::Custom`].
/// Two ids are equal when their grammars have the same id and source
/// digest.
///
/// ```
/// use purrassist_core::grammar::Grammar;
/// use purrassist_core::tool_call::DialectId;
/// use purrassist_core::{Budget, Governor};
/// let hermes = DialectId::Hermes;
/// assert_eq!(hermes.id(), "hermes.v1");
/// assert_eq!(hermes.grammar().id(), "hermes.v1");
/// assert_eq!(DialectId::from_id("hermes.v1"), Some(DialectId::Hermes));
/// let text = "grammar \"demo.v1\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
/// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
/// let dialect = DialectId::custom(grammar);
/// assert_eq!(dialect.name(), "demo.v1");
/// assert_eq!(dialect.clone(), dialect);
/// // A custom grammar compiled from a built-in source is that dialect.
/// let source = DialectId::Hermes.source().unwrap();
/// let same = Grammar::compile_governed(source, &mut Governor::new(Budget::default())).unwrap();
/// assert_eq!(DialectId::custom(same), DialectId::Hermes);
/// ```
#[derive(Clone)]
pub enum DialectId {
    /// `qwen3-coder-xml.v1`: Qwen3-Coder XML parameter calls.
    Qwen3CoderXml,
    /// `anthropic-tool-use.v1`: Anthropic Messages API `tool_use` content
    /// blocks as text.
    AnthropicToolUse,
    /// `deepseek-v3.1.v1`: DeepSeek-V3.1 tool-calls sections.
    DeepSeekV31,
    /// `deepseek-v3.v1`: DeepSeek-V3 / R1 tool-calls sections with fenced
    /// JSON arguments.
    DeepSeekV3,
    /// `glm-4.5.v1`: GLM alternating key/value tag calls.
    Glm45,
    /// `harmony.v1`: `OpenAI` Harmony (gpt-oss) assistant messages with a
    /// recipient.
    Harmony,
    /// `hermes.v1`: Hermes JSON calls in `<tool_call>` tags.
    Hermes,
    /// `kimi-k2.v1`: Kimi-K2 tool-calls sections with `functions.NAME:IDX`
    /// ids.
    KimiK2,
    /// `llama3-json.v1`: Llama 3.x JSON calls, optionally after
    /// `<|python_tag|>`.
    Llama3Json,
    /// `mistral-v11.v1`: Mistral v11+ `[TOOL_CALLS]NAME[ARGS]{...}` calls.
    MistralV11,
    /// `mistral-v3.v1`: Mistral v3 `[TOOL_CALLS]` JSON arrays of calls.
    MistralV3,
    /// `openai-tool-calls.v1`: `OpenAI` chat-completions `tool_calls`
    /// elements as text (also the output and round-trip target).
    OpenAiToolCalls,
    /// `bare-json.v1`: unwrapped JSON call objects, alone, in prose, in a
    /// code fence, or in an array.
    BareJson,
    /// `pythonic.v1`: Python literal keyword calls inside a list.
    Pythonic,
    /// A caller-compiled grammar.
    Custom(Arc<CompiledGrammar>),
}

/// The embedded source and lazily compiled grammar of one built-in dialect.
struct BuiltIn {
    id: &'static str,
    source: &'static str,
    compiled: OnceLock<CompiledGrammar>,
}

impl BuiltIn {
    const fn new(id: &'static str, source: &'static str) -> Self {
        Self {
            id,
            source,
            compiled: OnceLock::new(),
        }
    }

    fn grammar(&self) -> &CompiledGrammar {
        self.compiled.get_or_init(|| {
            Grammar::compile_governed(self.source, &mut Governor::new(Budget::default()))
                .expect("every built-in dialect grammar compiles under the default budget")
        })
    }
}

static QWEN3_CODER_XML: BuiltIn = BuiltIn::new(
    "qwen3-coder-xml.v1",
    include_str!("dialects/qwen3-coder-xml.v1.grammar"),
);
static ANTHROPIC_TOOL_USE: BuiltIn = BuiltIn::new(
    "anthropic-tool-use.v1",
    include_str!("dialects/anthropic-tool-use.v1.grammar"),
);
static DEEPSEEK_V31: BuiltIn = BuiltIn::new(
    "deepseek-v3.1.v1",
    include_str!("dialects/deepseek-v3.1.v1.grammar"),
);
static DEEPSEEK_V3: BuiltIn = BuiltIn::new(
    "deepseek-v3.v1",
    include_str!("dialects/deepseek-v3.v1.grammar"),
);
static GLM45: BuiltIn = BuiltIn::new("glm-4.5.v1", include_str!("dialects/glm-4.5.v1.grammar"));
static HARMONY: BuiltIn = BuiltIn::new("harmony.v1", include_str!("dialects/harmony.v1.grammar"));
static HERMES: BuiltIn = BuiltIn::new("hermes.v1", include_str!("dialects/hermes.v1.grammar"));
static KIMI_K2: BuiltIn = BuiltIn::new("kimi-k2.v1", include_str!("dialects/kimi-k2.v1.grammar"));
static LLAMA3_JSON: BuiltIn = BuiltIn::new(
    "llama3-json.v1",
    include_str!("dialects/llama3-json.v1.grammar"),
);
static MISTRAL_V11: BuiltIn = BuiltIn::new(
    "mistral-v11.v1",
    include_str!("dialects/mistral-v11.v1.grammar"),
);
static MISTRAL_V3: BuiltIn = BuiltIn::new(
    "mistral-v3.v1",
    include_str!("dialects/mistral-v3.v1.grammar"),
);
static BARE_JSON: BuiltIn = BuiltIn::new(
    "bare-json.v1",
    include_str!("dialects/bare-json.v1.grammar"),
);
static OPENAI_TOOL_CALLS: BuiltIn = BuiltIn::new(
    "openai-tool-calls.v1",
    include_str!("dialects/openai-tool-calls.v1.grammar"),
);

static PYTHONIC: BuiltIn =
    BuiltIn::new("pythonic.v1", include_str!("dialects/pythonic.v1.grammar"));

impl DialectId {
    /// Every built-in dialect, ordered by id.
    ///
    /// ```
    /// use purrassist_core::tool_call::DialectId;
    /// let ids: Vec<&str> = DialectId::BUILT_IN.iter().map(DialectId::id).collect();
    /// assert_eq!(
    ///     ids,
    ///     [
    ///         "anthropic-tool-use.v1",
    ///         "bare-json.v1",
    ///         "deepseek-v3.1.v1",
    ///         "deepseek-v3.v1",
    ///         "glm-4.5.v1",
    ///         "harmony.v1",
    ///         "hermes.v1",
    ///         "kimi-k2.v1",
    ///         "llama3-json.v1",
    ///         "mistral-v11.v1",
    ///         "mistral-v3.v1",
    ///         "openai-tool-calls.v1",
    ///         "pythonic.v1",
    ///         "qwen3-coder-xml.v1",
    ///     ]
    /// );
    /// ```
    pub const BUILT_IN: [Self; 14] = [
        Self::AnthropicToolUse,
        Self::BareJson,
        Self::DeepSeekV31,
        Self::DeepSeekV3,
        Self::Glm45,
        Self::Harmony,
        Self::Hermes,
        Self::KimiK2,
        Self::Llama3Json,
        Self::MistralV11,
        Self::MistralV3,
        Self::OpenAiToolCalls,
        Self::Pythonic,
        Self::Qwen3CoderXml,
    ];

    /// Wrap a compiled grammar.
    ///
    /// See [`DialectId`] for an example.
    #[must_use]
    pub fn custom(grammar: CompiledGrammar) -> Self {
        Self::Custom(Arc::new(grammar))
    }

    const fn built_in(&self) -> Option<&'static BuiltIn> {
        match self {
            Self::Qwen3CoderXml => Some(&QWEN3_CODER_XML),
            Self::Pythonic => Some(&PYTHONIC),
            Self::AnthropicToolUse => Some(&ANTHROPIC_TOOL_USE),
            Self::DeepSeekV31 => Some(&DEEPSEEK_V31),
            Self::DeepSeekV3 => Some(&DEEPSEEK_V3),
            Self::Glm45 => Some(&GLM45),
            Self::Harmony => Some(&HARMONY),
            Self::Hermes => Some(&HERMES),
            Self::KimiK2 => Some(&KIMI_K2),
            Self::Llama3Json => Some(&LLAMA3_JSON),
            Self::MistralV11 => Some(&MISTRAL_V11),
            Self::MistralV3 => Some(&MISTRAL_V3),
            Self::OpenAiToolCalls => Some(&OPENAI_TOOL_CALLS),
            Self::BareJson => Some(&BARE_JSON),
            Self::Custom(_) => None,
        }
    }

    /// The built-in dialect with versioned id `id`, or `None` for any other
    /// string (custom grammars are not registered by id).
    ///
    /// ```
    /// use purrassist_core::tool_call::DialectId;
    /// assert_eq!(DialectId::from_id("qwen3-coder-xml.v1"), Some(DialectId::Qwen3CoderXml));
    /// assert_eq!(DialectId::from_id("openai-tool-calls.v1"), Some(DialectId::OpenAiToolCalls));
    /// assert_eq!(DialectId::from_id("hermes"), None);
    /// ```
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::BUILT_IN
            .into_iter()
            .find(|dialect| dialect.built_in().is_some_and(|built| built.id == id))
    }

    /// The dialect's versioned id: the built-in id, or a custom grammar's
    /// declared id. For a built-in dialect this never compiles its grammar.
    ///
    /// ```
    /// use purrassist_core::tool_call::DialectId;
    /// for dialect in DialectId::BUILT_IN {
    ///     assert_eq!(DialectId::from_id(dialect.id()), Some(dialect.clone()));
    ///     assert_eq!(dialect.grammar().id(), dialect.id());
    /// }
    /// ```
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Qwen3CoderXml => QWEN3_CODER_XML.id,
            Self::Pythonic => PYTHONIC.id,
            Self::AnthropicToolUse => ANTHROPIC_TOOL_USE.id,
            Self::DeepSeekV31 => DEEPSEEK_V31.id,
            Self::DeepSeekV3 => DEEPSEEK_V3.id,
            Self::Glm45 => GLM45.id,
            Self::Harmony => HARMONY.id,
            Self::Hermes => HERMES.id,
            Self::KimiK2 => KIMI_K2.id,
            Self::Llama3Json => LLAMA3_JSON.id,
            Self::MistralV11 => MISTRAL_V11.id,
            Self::MistralV3 => MISTRAL_V3.id,
            Self::OpenAiToolCalls => OPENAI_TOOL_CALLS.id,
            Self::BareJson => BARE_JSON.id,
            Self::Custom(grammar) => grammar.id(),
        }
    }

    /// The embedded grammar source of a built-in dialect; `None` for a
    /// custom grammar. Compiling it with [`Grammar::compile_governed`]
    /// yields a grammar equal to [`DialectId::grammar`], charged to the
    /// caller's governor.
    ///
    /// See [`DialectId`] for an example.
    #[must_use]
    pub fn source(&self) -> Option<&'static str> {
        self.built_in().map(|built| built.source)
    }

    /// The dialect's grammar; a built-in grammar is compiled on first use.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::tool_call::DialectId;
    /// # use purrassist_core::{Budget, Governor};
    /// # let text = "grammar \"demo.v1\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// # let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// let dialect = DialectId::custom(grammar);
    /// assert_eq!(dialect.grammar().id(), "demo.v1");
    /// assert_eq!(DialectId::Qwen3CoderXml.grammar().reasoning(), Some((&b"<think>"[..], &b"</think>"[..])));
    /// ```
    #[must_use]
    pub fn grammar(&self) -> &CompiledGrammar {
        match self {
            Self::Qwen3CoderXml => QWEN3_CODER_XML.grammar(),
            Self::Pythonic => PYTHONIC.grammar(),
            Self::AnthropicToolUse => ANTHROPIC_TOOL_USE.grammar(),
            Self::DeepSeekV31 => DEEPSEEK_V31.grammar(),
            Self::DeepSeekV3 => DEEPSEEK_V3.grammar(),
            Self::Glm45 => GLM45.grammar(),
            Self::Harmony => HARMONY.grammar(),
            Self::Hermes => HERMES.grammar(),
            Self::KimiK2 => KIMI_K2.grammar(),
            Self::Llama3Json => LLAMA3_JSON.grammar(),
            Self::MistralV11 => MISTRAL_V11.grammar(),
            Self::MistralV3 => MISTRAL_V3.grammar(),
            Self::OpenAiToolCalls => OPENAI_TOOL_CALLS.grammar(),
            Self::BareJson => BARE_JSON.grammar(),
            Self::Custom(grammar) => grammar,
        }
    }

    /// The dialect's versioned id (its grammar's id); the same as
    /// [`DialectId::id`].
    ///
    /// See [`DialectId`] for an example.
    #[must_use]
    pub fn name(&self) -> &str {
        self.id()
    }

    fn identity(&self) -> (&str, Digest) {
        (self.name(), self.grammar().digest())
    }
}

impl PartialEq for DialectId {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
    }
}

impl Eq for DialectId {}

impl PartialOrd for DialectId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DialectId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.identity().cmp(&other.identity())
    }
}

impl Hash for DialectId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity().hash(state);
    }
}

impl fmt::Debug for DialectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, digest) = self.identity();
        write!(formatter, "DialectId({name} {digest})")
    }
}

/// Why a [`DialectSet`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialectSetError {
    /// The allow-list names no dialect.
    Empty,
    /// The allow-list names the same dialect twice.
    Duplicate {
        /// The repeated dialect's id.
        id: String,
    },
    /// The governor's budget was exhausted, or the caller cancelled.
    Resource(StopReason),
}

impl fmt::Display for DialectSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a dialect allow-list names at least one dialect"),
            Self::Duplicate { id } => write!(formatter, "dialect `{id}` is listed twice"),
            Self::Resource(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for DialectSetError {}

impl From<StopReason> for DialectSetError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// A compiled, canonical dialect allow-list for one model.
#[derive(Debug)]
pub struct DialectSet {
    pub(crate) dialects: ScratchVec<DialectId>,
    pub(crate) markers: ScratchVec<(ScratchVec<u8>, ScratchVec<u8>)>,
    /// Per marker pair, whether every dialect declaring it declares it
    /// `explicit` (see [`CompiledGrammar::reasoning_explicit`](crate::grammar::CompiledGrammar::reasoning_explicit)).
    pub(crate) explicit: ScratchVec<bool>,
    pub(crate) marker_matcher: Option<Matcher>,
    pub(crate) lookbehind: usize,
    configuration_bound: u64,
}

impl DialectSet {
    /// Compile an allow-list: order it canonically, check it, and build the
    /// combined reasoning-marker matcher (the union of the dialects'
    /// declared reasoning markers).
    ///
    /// Admits collection, identity comparisons, duplicate diagnostics, marker
    /// copies/order/deduplication, explicit-marker scans, matcher preparation and
    /// checked lookbehind/configuration bounds before processing. All retained
    /// lists and byte buffers release their scratch charge when the set drops;
    /// temporaries also release on refusal. A returned duplicate diagnostic is
    /// an owned String with a retained admission charge. The input iterator and custom
    /// grammar/Arc preparation belong to the caller. Lazy built-in grammars
    /// retain their separate default governor; native pattern preparation and
    /// recognizer/recovery runtime allocations are separate contracts.
    ///
    /// # Errors
    /// [`DialectSetError::Empty`] for an empty list,
    /// [`DialectSetError::Duplicate`] for a dialect listed twice, and
    /// [`DialectSetError::Resource`] on exhaustion or cancellation.
    ///
    /// ```
    /// use purrassist_core::grammar::Grammar;
    /// use purrassist_core::tool_call::{DialectId, DialectSet, DialectSetError};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let compile = |id: &str, governor: &mut Governor| {
    ///     let text = format!("grammar \"{id}\"; call = \"<{id}>\"; result = \"<r>\"; declaration = \"<d>\";");
    ///     DialectId::custom(Grammar::compile_governed(&text, governor).unwrap())
    /// };
    /// let (a, b) = (compile("a", &mut governor), compile("b", &mut governor));
    /// let set = DialectSet::new([b.clone(), a.clone()], &mut governor)?;
    /// assert_eq!(set.dialects(), [a.clone(), b]);
    /// assert_eq!(
    ///     DialectSet::new([a.clone(), a], &mut governor).unwrap_err(),
    ///     DialectSetError::Duplicate { id: "a".into() }
    /// );
    /// assert_eq!(DialectSet::new([], &mut governor).unwrap_err(), DialectSetError::Empty);
    /// # Ok::<(), DialectSetError>(())
    /// ```
    pub fn new(
        dialects: impl IntoIterator<Item = DialectId>,
        governor: &mut Governor,
    ) -> Result<Self, DialectSetError> {
        governor.check()?;
        let mut collected = ScratchVec::new(governor);
        for dialect in dialects {
            governor.charge(Resource::Work, K_COMPILE)?;
            collected.push(dialect)?;
        }
        if collected.is_empty() {
            return Err(DialectSetError::Empty);
        }
        canonical_dialects(collected.as_mut_slice(), governor)?;
        let markers = marker_pairs(&collected, governor)?;
        let explicit = marker_flags(&collected, &markers, governor)?;
        let marker_matcher = if markers.is_empty() {
            None
        } else {
            let mut literals = ScratchVec::new(governor);
            for (open, close) in &markers {
                governor.charge(Resource::Work, 2)?;
                literals.push(open.as_slice())?;
                literals.push(close.as_slice())?;
            }
            Some(Matcher::build(&literals, governor)?)
        };
        let (lookbehind, configuration_bound) = set_bounds(&collected, &markers, governor)?;
        governor.check()?;
        Ok(Self {
            dialects: collected,
            markers,
            explicit,
            marker_matcher,
            lookbehind,
            configuration_bound,
        })
    }

    /// The allow-list of every built-in dialect ([`DialectId::BUILT_IN`]).
    ///
    /// Like a built-in grammar, the set is built once, on first use, under
    /// its own governor with a default [`Budget`], and borrowed: building it is
    /// never charged to a caller's governor.
    ///
    /// # Panics
    /// Never: the built-in dialects always build under the default
    /// budget (checked by this example and the dialect conformance tests).
    ///
    /// ```
    /// use purrassist_core::tool_call::{DialectId, DialectSet};
    /// assert_eq!(DialectSet::built_in().dialects(), DialectId::BUILT_IN);
    /// ```
    #[must_use]
    pub fn built_in() -> &'static Self {
        static BUILT_IN: OnceLock<DialectSet> = OnceLock::new();
        BUILT_IN.get_or_init(|| {
            Self::new(DialectId::BUILT_IN, &mut Governor::new(Budget::default()))
                .expect("the built-in dialect set builds under the default budget")
        })
    }

    /// The dialects, in canonical order.
    ///
    /// See [`DialectSet::new`] for an example.
    #[must_use]
    pub fn dialects(&self) -> &[DialectId] {
        &self.dialects
    }

    /// The reasoning markers `(open, close)` the set recognizes, sorted.
    ///
    /// ```
    /// use purrassist_core::grammar::Grammar;
    /// use purrassist_core::tool_call::{DialectId, DialectSet};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let text = "grammar \"g\"; reasoning \"<think>\" \"</think>\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let dialect = DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap());
    /// let set = DialectSet::new([dialect], &mut governor).unwrap();
    /// assert_eq!(set.reasoning_markers()[0].0.as_slice(), b"<think>");
    /// assert_eq!(set.reasoning_markers()[0].1.as_slice(), b"</think>");
    /// ```
    #[must_use]
    pub fn reasoning_markers(&self) -> &[(ScratchVec<u8>, ScratchVec<u8>)] {
        &self.markers
    }

    /// How many bytes behind the input a repair or delimiter replay can
    /// reach: one more than the longest literal, delimiter or marker of any
    /// dialect, plus [`STRAY_CLOSER_GAP`](crate::grammar::cost::STRAY_CLOSER_GAP)
    /// for a dialect that declares closers (a completed call absorbs stray
    /// closers separated from it by that much whitespace). The turn-level
    /// segmentation settles each offset once the input is this far past it.
    ///
    /// ```
    /// use purrassist_core::grammar::Grammar;
    /// use purrassist_core::tool_call::{DialectId, DialectSet};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let text = "grammar \"g\"; call = \"<call>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let dialect = DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap());
    /// assert_eq!(DialectSet::new([dialect], &mut governor).unwrap().lookbehind(), 7);
    /// ```
    #[must_use]
    pub const fn lookbehind(&self) -> usize {
        self.lookbehind
    }

    /// The sum of the dialects' configuration bounds: `K_THREAD` times this
    /// bounds the configuration steps per input byte (see
    /// [`crate::grammar::cost`]).
    ///
    /// ```
    /// use purrassist_core::grammar::Grammar;
    /// use purrassist_core::tool_call::{DialectId, DialectSet};
    /// use purrassist_core::{Budget, Governor};
    /// let mut governor = Governor::new(Budget::default());
    /// let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let dialect = DialectId::custom(Grammar::compile_governed(text, &mut governor).unwrap());
    /// let bound = dialect.grammar().configuration_bound();
    /// assert_eq!(DialectSet::new([dialect], &mut governor).unwrap().configuration_bound(), bound);
    /// ```
    #[must_use]
    pub const fn configuration_bound(&self) -> u64 {
        self.configuration_bound
    }

    /// The call openers of `output` summed over the dialects
    /// ([`CompiledGrammar::call_openers`]): every byte offset where a
    /// dialect's `call` production can open counts once for that dialect,
    /// so a shared opener (`<tool_call>` in Qwen3-Coder XML, Hermes and GLM)
    /// counts once per competing attempt. A dialect whose
    /// attempts commit after their opener (`llama3-json.v1`,
    /// `anthropic-tool-use.v1` and `bare-json.v1` open at `{` and commit on
    /// call-shaped evidence) counts every opener, committed or not. It bounds the call attempts recovery can read or reject, and sizes the candidate
    /// budget of [`ToolRecoveryRequest::sized_for`](super::ToolRecoveryRequest::sized_for).
    /// A pure, allocation-free scan; nothing is charged.
    ///
    /// ```
    /// use purrassist_core::tool_call::DialectSet;
    /// let output = b"<tool_call>\n{\"name\": \"ls\", \"arguments\": {}}\n</tool_call>";
    /// // Qwen3-Coder XML, Hermes and GLM open with `<tool_call>`; a Llama 3
    /// // JSON, an Anthropic `tool_use` and a bare JSON attempt may open at
    /// // each of the two `{`.
    /// assert_eq!(DialectSet::built_in().call_openers(output), 9);
    /// assert_eq!(DialectSet::built_in().call_openers(b"no calls here"), 0);
    /// ```
    #[must_use]
    pub fn call_openers(&self, output: &[u8]) -> u64 {
        self.dialects
            .iter()
            .map(|dialect| dialect.grammar().call_openers(output))
            .fold(0, u64::saturating_add)
    }
}

impl ScratchCharge for DialectId {
    const CHARGE: usize = 16;
}
impl ScratchCharge for &[u8] {
    const CHARGE: usize = 16;
}
const _: () = assert!(core::mem::size_of::<DialectId>() <= DialectId::CHARGE);
const _: () = assert!(core::mem::size_of::<&[u8]>() <= <&[u8] as ScratchCharge>::CHARGE);

type MarkerPair = (ScratchVec<u8>, ScratchVec<u8>);

fn compare_bytes(
    left: &[u8],
    right: &[u8],
    governor: &mut Governor,
) -> Result<Ordering, StopReason> {
    governor.charge(
        Resource::Work,
        size(left.len().min(right.len())).saturating_add(1),
    )?;
    Ok(left.cmp(right))
}

fn compare_dialects(
    left: &DialectId,
    right: &DialectId,
    governor: &mut Governor,
) -> Result<Ordering, StopReason> {
    let order = compare_bytes(left.name().as_bytes(), right.name().as_bytes(), governor)?;
    if order.is_eq() {
        governor.charge(Resource::Work, 33)?;
        Ok(left.grammar().digest().cmp(&right.grammar().digest()))
    } else {
        Ok(order)
    }
}

fn canonical_dialects(
    dialects: &mut [DialectId],
    governor: &mut Governor,
) -> Result<(), DialectSetError> {
    for index in 1..dialects.len() {
        let mut at = index;
        while at > 0 {
            if !compare_dialects(&dialects[at - 1], &dialects[at], governor)?.is_gt() {
                break;
            }
            governor.charge(Resource::Work, 1)?;
            dialects.swap(at - 1, at);
            at -= 1;
        }
    }
    for pair in dialects.windows(2) {
        if compare_dialects(&pair[0], &pair[1], governor)?.is_eq() {
            let name = pair[0].name();
            governor.charge(Resource::InputBytes, size(name.len()))?;
            governor.charge(Resource::Work, size(name.len()).saturating_add(1))?;
            let mut id = ScratchString::new(governor);
            id.push_str(name)?;
            governor.check()?;
            return Err(DialectSetError::Duplicate {
                id: id.into_string(),
            });
        }
    }
    Ok(())
}

fn copy_marker(bytes: &[u8], governor: &mut Governor) -> Result<ScratchVec<u8>, StopReason> {
    governor.charge(Resource::InputBytes, size(bytes.len()))?;
    governor.charge(Resource::Work, size(bytes.len()).saturating_add(1))?;
    let mut owned = ScratchVec::new(governor);
    owned.extend(bytes.iter().copied())?;
    Ok(owned)
}

fn compare_markers(
    left: &MarkerPair,
    right: &MarkerPair,
    governor: &mut Governor,
) -> Result<Ordering, StopReason> {
    let order = compare_bytes(&left.0, &right.0, governor)?;
    if order.is_eq() {
        compare_bytes(&left.1, &right.1, governor)
    } else {
        Ok(order)
    }
}

fn marker_pairs(
    dialects: &[DialectId],
    governor: &mut Governor,
) -> Result<ScratchVec<MarkerPair>, StopReason> {
    let mut markers = ScratchVec::new(governor);
    for dialect in dialects {
        governor.charge(Resource::Work, 1)?;
        for (open, close) in &dialect.grammar().reasoning {
            governor.charge(Resource::Work, 1)?;
            markers.push((copy_marker(open, governor)?, copy_marker(close, governor)?))?;
        }
    }
    for index in 1..markers.len() {
        let mut at = index;
        while at > 0 {
            if !compare_markers(&markers[at - 1], &markers[at], governor)?.is_gt() {
                break;
            }
            governor.charge(Resource::Work, 1)?;
            markers.as_mut_slice().swap(at - 1, at);
            at -= 1;
        }
    }
    let mut unique = 0;
    for read in 0..markers.len() {
        governor.charge(Resource::Work, 1)?;
        if unique == 0 || !compare_markers(&markers[unique - 1], &markers[read], governor)?.is_eq()
        {
            governor.charge(Resource::Work, 1)?;
            markers.as_mut_slice().swap(unique, read);
            unique += 1;
        }
    }
    while markers.len() > unique {
        governor.charge(Resource::Work, 1)?;
        markers.pop();
    }
    Ok(markers)
}

fn marker_flags(
    dialects: &[DialectId],
    markers: &[MarkerPair],
    governor: &mut Governor,
) -> Result<ScratchVec<bool>, StopReason> {
    let mut flags = ScratchVec::new(governor);
    for pair in markers {
        governor.charge(Resource::Work, 1)?;
        let mut all_explicit = true;
        for dialect in dialects {
            governor.charge(Resource::Work, 1)?;
            let grammar = dialect.grammar();
            for (declared, explicit) in grammar.reasoning.iter().zip(&grammar.explicit_reasoning) {
                governor.charge(Resource::Work, 1)?;
                if compare_markers(declared, pair, governor)?.is_eq() && !explicit {
                    all_explicit = false;
                    break;
                }
            }
            if !all_explicit {
                break;
            }
        }
        flags.push(all_explicit)?;
    }
    Ok(flags)
}

fn set_bounds(
    dialects: &[DialectId],
    markers: &[MarkerPair],
    governor: &mut Governor,
) -> Result<(usize, u64), StopReason> {
    let mut lookbehind = 0;
    let mut bound = 0_u64;
    for dialect in dialects {
        governor.charge(Resource::Work, 1)?;
        let grammar = dialect.grammar();
        let gap = if grammar.closers.is_empty() {
            0
        } else {
            crate::grammar::cost::STRAY_CLOSER_GAP
        };
        let reach = grammar
            .longest_literal()
            .checked_add(gap)
            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
        lookbehind = lookbehind.max(reach);
        bound = bound
            .checked_add(grammar.configuration_bound())
            .ok_or_else(|| governor.stop_resource(Resource::Work))?;
    }
    for (open, close) in markers {
        governor.charge(Resource::Work, 1)?;
        lookbehind = lookbehind.max(open.len()).max(close.len());
    }
    let lookbehind = lookbehind
        .checked_add(1)
        .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
    Ok((lookbehind, bound))
}

#[cfg(test)]
mod tests {
    use super::{DialectId, DialectSet, DialectSetError};
    use crate::contract::{Budget, CancellationToken, Governor, Resource, StopReason};
    use crate::grammar::Grammar;

    fn prepared(name: &str, explicit: bool) -> DialectId {
        let mode = if explicit { " explicit" } else { "" };
        let source = format!(
            "grammar \"{name}\"; reasoning \"<t>\" \"</t>\"{mode}; reasoning \"[t]\" \"[/t]\" explicit; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";"
        );
        DialectId::custom(
            Grammar::compile_governed(&source, &mut Governor::new(Budget::default())).unwrap(),
        )
    }

    /// Constructor refusal releases partial and completed buffers; successful
    /// sets retain their own preparation charges until dropped.
    #[test]
    fn set_preparation_retains_owners_and_releases_every_refused_stage() {
        let dialects = [prepared("b", false), prepared("a", true)];
        let mut measured = Governor::new(Budget::default());
        let set = DialectSet::new(dialects.clone(), &mut measured).unwrap();
        let usage = measured.usage();
        assert!(measured.scratch_meter().live() > 0);
        drop(set);
        assert_eq!(measured.scratch_meter().live(), 0);
        let exact = Budget {
            work: usage.work,
            input_bytes: 4,
            scratch_bytes: usage.peak_scratch_bytes,
            ..Budget::default()
        };
        let mut governor = Governor::new(exact);
        drop(DialectSet::new(dialects.clone(), &mut governor).unwrap());
        assert_eq!(governor.scratch_meter().live(), 0);
        for (budget, resource) in [
            (Budget { work: 0, ..exact }, Resource::Work),
            (
                Budget {
                    work: usage.work / 2,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    work: usage.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: 0,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    input_bytes: 3,
                    ..exact
                },
                Resource::InputBytes,
            ),
            (Budget { depth: 0, ..exact }, Resource::Depth),
        ] {
            let mut governor = Governor::new(budget);
            let reason = StopReason::BudgetExceeded(resource);
            assert_eq!(
                DialectSet::new(dialects.clone(), &mut governor).unwrap_err(),
                DialectSetError::Resource(reason.clone())
            );
            assert_eq!(governor.check(), Err(reason));
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        // The iterator cancels after yielding a prepared dialect: construction
        // cannot publish even when iteration ends without another item.
        let token = CancellationToken::new();
        let mut governor = Governor::with_cancellation(exact, token.clone());
        let mut emitted = false;
        let input = core::iter::from_fn(|| {
            if emitted {
                token.cancel();
                None
            } else {
                emitted = true;
                Some(dialects[0].clone())
            }
        });
        assert_eq!(
            DialectSet::new(input, &mut governor).unwrap_err(),
            DialectSetError::Resource(StopReason::Cancelled)
        );
        assert_eq!(governor.scratch_meter().live(), 0);
    }

    /// Deduplication combines declaration policy and preserves the flattened
    /// open/close labels the recognizer uses to pair reasoning blocks.
    #[test]
    fn shared_marker_policy_and_matcher_pair_labels_survive_canonicalization() {
        for dialects in [
            [prepared("b", false), prepared("a", true)],
            [prepared("a", true), prepared("b", false)],
        ] {
            let set = DialectSet::new(dialects, &mut Governor::new(Budget::default())).unwrap();
            assert_eq!(set.markers.len(), 2);
            assert_eq!(set.explicit.as_slice(), [false, true]);
            let matcher = set.marker_matcher.as_ref().unwrap();
            for (pair, (open, close)) in set.markers.iter().enumerate() {
                for (offset, marker) in [open, close].into_iter().enumerate() {
                    let mut state = 0;
                    for &byte in marker {
                        state = matcher.step(state, byte);
                    }
                    assert_eq!(
                        matcher.found(state),
                        Some((
                            u32::try_from(pair * 2 + offset).unwrap(),
                            u32::try_from(marker.len()).unwrap()
                        ))
                    );
                }
            }
        }
    }
}
