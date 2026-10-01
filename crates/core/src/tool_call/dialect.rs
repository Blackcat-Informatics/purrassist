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
use crate::contract::{Budget, Governor, Resource, StopReason};
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
/// Three dialects are built in, each a first-party grammar embedded in the
/// crate (see `docs/tool-calls/dialects/` for each grammar's public sources
/// and what it deliberately does not accept):
///
/// | Variant | Versioned id | Shape |
/// | --- | --- | --- |
/// | [`DialectId::Qwen3CoderXml`] | `qwen3-coder-xml.v1` | `<tool_call><function=NAME><parameter=P>VALUE</parameter></function></tool_call>` |
/// | [`DialectId::Hermes`] | `hermes.v1` | `<tool_call>{"name": NAME, "arguments": {...}}</tool_call>` |
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
    /// `hermes.v1`: Hermes JSON calls in `<tool_call>` tags.
    Hermes,
    /// `openai-tool-calls.v1`: `OpenAI` chat-completions `tool_calls`
    /// elements as text (also the output and round-trip target).
    OpenAiToolCalls,
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
static HERMES: BuiltIn = BuiltIn::new("hermes.v1", include_str!("dialects/hermes.v1.grammar"));
static OPENAI_TOOL_CALLS: BuiltIn = BuiltIn::new(
    "openai-tool-calls.v1",
    include_str!("dialects/openai-tool-calls.v1.grammar"),
);

impl DialectId {
    /// Every built-in dialect, ordered by id.
    ///
    /// ```
    /// use purrassist_core::tool_call::DialectId;
    /// let ids: Vec<&str> = DialectId::BUILT_IN.iter().map(DialectId::id).collect();
    /// assert_eq!(ids, ["hermes.v1", "openai-tool-calls.v1", "qwen3-coder-xml.v1"]);
    /// ```
    pub const BUILT_IN: [Self; 3] = [Self::Hermes, Self::OpenAiToolCalls, Self::Qwen3CoderXml];

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
            Self::Hermes => Some(&HERMES),
            Self::OpenAiToolCalls => Some(&OPENAI_TOOL_CALLS),
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
            Self::Hermes => HERMES.id,
            Self::OpenAiToolCalls => OPENAI_TOOL_CALLS.id,
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
            Self::Hermes => HERMES.grammar(),
            Self::OpenAiToolCalls => OPENAI_TOOL_CALLS.grammar(),
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
#[derive(Clone, Debug)]
pub struct DialectSet {
    pub(crate) dialects: Vec<DialectId>,
    pub(crate) markers: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) marker_matcher: Option<Matcher>,
    pub(crate) lookbehind: usize,
}

impl DialectSet {
    /// Compile an allow-list: order it canonically, check it, and build the
    /// combined reasoning-marker matcher (the union of the dialects'
    /// declared reasoning markers).
    ///
    /// Charges `Work` per dialect and per marker-matcher entry, and the
    /// matcher's retained `ScratchBytes`.
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
        let mut dialects: Vec<DialectId> = dialects.into_iter().collect();
        if dialects.is_empty() {
            return Err(DialectSetError::Empty);
        }
        governor.charge(
            Resource::Work,
            size(dialects.len()).saturating_mul(K_COMPILE),
        )?;
        dialects.sort();
        if let Some(pair) = dialects.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(DialectSetError::Duplicate {
                id: pair[0].name().to_owned(),
            });
        }
        let mut markers: Vec<(Vec<u8>, Vec<u8>)> = dialects
            .iter()
            .filter_map(|dialect| dialect.grammar().reasoning.clone())
            .collect();
        markers.sort();
        markers.dedup();
        let marker_matcher = if markers.is_empty() {
            None
        } else {
            let literals: Vec<Vec<u8>> = markers
                .iter()
                .flat_map(|(open, close)| [open.clone(), close.clone()])
                .collect();
            Some(Matcher::build(&literals, governor)?)
        };
        let lookbehind = dialects
            .iter()
            .map(|dialect| {
                let grammar = dialect.grammar();
                // A completed call waits for whitespace and a stray closer.
                let gap = if grammar.closers.is_empty() {
                    0
                } else {
                    crate::grammar::cost::STRAY_CLOSER_GAP
                };
                grammar.longest_literal() + gap
            })
            .chain(
                markers
                    .iter()
                    .flat_map(|(open, close)| [open.len(), close.len()]),
            )
            .max()
            .unwrap_or(0)
            + 1;
        Ok(Self {
            dialects,
            markers,
            marker_matcher,
            lookbehind,
        })
    }

    /// The allow-list of every built-in dialect ([`DialectId::BUILT_IN`]).
    ///
    /// Like a built-in grammar, the set is built once, on first use, under
    /// its own governor with a default [`Budget`], and cloned: building it is
    /// never charged to a caller's governor.
    ///
    /// # Panics
    /// Never: three built-in dialects always build under the default
    /// budget (checked by this example and the dialect conformance tests).
    ///
    /// ```
    /// use purrassist_core::tool_call::{DialectId, DialectSet};
    /// assert_eq!(DialectSet::built_in().dialects(), DialectId::BUILT_IN);
    /// ```
    #[must_use]
    pub fn built_in() -> Self {
        static BUILT_IN: OnceLock<DialectSet> = OnceLock::new();
        BUILT_IN
            .get_or_init(|| {
                Self::new(DialectId::BUILT_IN, &mut Governor::new(Budget::default()))
                    .expect("the built-in dialect set builds under the default budget")
            })
            .clone()
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
    /// assert_eq!(set.reasoning_markers(), [(b"<think>".to_vec(), b"</think>".to_vec())]);
    /// ```
    #[must_use]
    pub fn reasoning_markers(&self) -> &[(Vec<u8>, Vec<u8>)] {
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
    pub fn configuration_bound(&self) -> u64 {
        self.dialects
            .iter()
            .map(|dialect| dialect.grammar().configuration_bound())
            .sum()
    }
}
