// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Weighted, invertible grammars for model output markup.
//!
//! Every tool-call dialect is a declarative grammar written in the small
//! textual DSL below and compiled by [`Grammar::compile_governed`] into a
//! [`CompiledGrammar`]: the productions themselves (retained for printing:
//! literals, slot order and capture placement survive compilation), plus
//! byte-level recognizer tables — a program of terminal instructions per
//! recognized production, literal tries for repair spellings, and dense
//! Aho–Corasick matchers for verbatim bodies. One interpreter runs every
//! dialect of a [`DialectSet`](crate::tool_call::DialectSet) concurrently
//! over these tables (see [`Recognizer`](crate::tool_call::Recognizer)).
//!
//! # The DSL
//!
//! A grammar file is a sequence of `;`-terminated statements. `#` starts a
//! comment that runs to the end of the line. The first statement names the
//! grammar.
//!
//! | Statement | Meaning |
//! | --- | --- |
//! | `grammar "ID";` | Header; `ID` matches `[A-Za-z0-9._-]+`. |
//! | `reasoning "OPEN" "CLOSE";` | Reasoning-block markers for this dialect (at most one). |
//! | `confusable "CANONICAL" "VARIANT";` | `VARIANT` is a confusable spelling of `CANONICAL` inside any literal. |
//! | `closer NAME = "S1" \| "S2" ...;` | A closer class: `S1` is canonical, the rest are variant spellings. |
//! | `NAME = EXPR;` | A production. |
//!
//! Expressions:
//!
//! | Form | Meaning |
//! | --- | --- |
//! | `"bytes"` | A literal terminal. Escapes: `\\ \" \n \r \t \xHH \u{HEX}`. |
//! | `/pattern/` | A class terminal: a maximal run matching the ECMA-262 `u`-mode pattern as a whole (anchored, non-empty, no backreferences or lookaround; `\/` is a slash). |
//! | `until("D1", "D2", ...)` | A verbatim body up to, not including, the first occurrence of any delimiter (or the end of input). |
//! | `slot(CODEC, "D1", ...)` | Like `until`, with the body decoded by the named [`codec`]. |
//! | `NAME` | A production (inlined; recursion is rejected, since framing is regular) or a closer class. |
//! | `KIND:item` | A capture of `item`'s span; `KIND` is one of [`CaptureKind`]. |
//! | `a b` / `a \| b` | Sequence / alternation. |
//! | `item*` `item+` `item?` | Repetition. |
//!
//! Every grammar defines `call`, `result` and `declaration`. The recognizer
//! looks for `call` and `result` in model output; each must begin with a
//! literal opener (that is how a turn knows where a call may start), and
//! must not match the empty string. `declaration` is retained for printing.
//! `arguments:` and `param-value:` captures must wrap a `slot(...)`; a
//! production uses either one `arguments` slot or `param-name`/`param-value`
//! pairs (adjacent, in that order, in one sequence), never both.
//!
//! A worked example (the engine's test grammar, shaped like an
//! XML-parameter dialect: a wrapper around a function element):
//!
//! ```text
//! grammar "mini.v1";
//! reasoning "<think>" "</think>";
//! confusable "｜" "|";
//! closer tool-end = "</tool>" | "</tools>";
//! closer call-end = "</call>" | "</calls>";
//! closer arg-end = "</arg>" | "</args>";
//! ident = /[A-Za-z_][A-Za-z0-9_.:-]*/;
//! param = "<arg=" param-name:ident ">"
//!         param-value:slot(raw, "</arg>", "</args>", "<arg=", "</call>", "</calls>")
//!         arg-end;
//! call = "<tool>" "<call=" name:ident (" id=" id:ident)? ">" param* call-end tool-end;
//! result = "<｜result｜>" content:until("<｜/result｜>") "<｜/result｜>";
//! declaration = "<tool name=\"" name:ident "\"/>";
//! ```
//!
//! It recognizes `<tool><call=read id=c1><arg=path>a.rs</arg></call></tool>`
//! as a call named `read` with id `c1` and arguments `{"path":"a.rs"}`. The
//! `param-value` slot ends at the next parameter opener or call closer, so a
//! missing `</arg>` is detected at the following token; the delimiter sets
//! of `until` and `slot` also match their confusable spellings, so an ASCII
//! `<|/result|>` still ends a result body.
//!
//! # Repairs
//!
//! Recognition is deterministic on well-formed input. Where every
//! configuration of a call attempt fails, a best-first search over
//! `(position, configuration)` applies the weighted [edit
//! operators](edit::EditOperator) — insert a missing closer, delete a stray
//! closer, accept a closer variant or a confusable spelling — and keeps the
//! least-cost repairs. Repair is local to each failure configuration: the
//! search compares repairs by their cost up to the failing byte and does not
//! revisit a cheaper repair whose continuation fails later (it repairs
//! again there). The turn-level segmentation over the resulting segments is
//! global. Costs and the linear work bound are in [`cost`].
//!
//! ```
//! use purrassist_core::grammar::{CaptureKind, Expr, Grammar};
//! use purrassist_core::{Budget, Governor};
//! let mut governor = Governor::new(Budget::default());
//! let grammar = Grammar::compile_governed(
//!     r#"grammar "tiny.v1";
//!        closer end = "</c>" | "</cs>";
//!        call = "<c>" name:/[a-z]+/ end;
//!        result = "<r>" content:until("</r>") "</r>";
//!        declaration = "<d " name:/[a-z]+/ ">";"#,
//!     &mut governor,
//! )?;
//! assert_eq!(grammar.id(), "tiny.v1");
//! let Expr::Sequence(items) = grammar.call() else { unreachable!() };
//! assert!(matches!(&items[1], Expr::Capture { kind: CaptureKind::Name, .. }));
//! let Expr::Closer(end) = items[2] else { unreachable!() };
//! assert_eq!(grammar.closer_spellings(end), [b"</c>".to_vec(), b"</cs>".to_vec()]);
//! # Ok::<(), purrassist_core::grammar::GrammarError>(())
//! ```
pub(crate) mod automaton;
pub mod codec;
mod compile;
pub mod cost;
pub mod edit;
pub(crate) mod engine;
mod repair;
mod syntax;

use crate::contract::{Governor, StopReason};
use crate::digest::Digest;
use crate::pattern::ByteAutomaton;
use codec::CodecKind;
use core::fmt;

pub(crate) use compile::{Inst, LiteralTables, Program};

/// A named capture recorded into the recognized IR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaptureKind {
    /// `name:` — the tool name.
    Name,
    /// `id:` — the call identifier.
    Id,
    /// `arguments:` — the whole argument value (wraps a slot).
    Arguments,
    /// `param-name:` — one parameter's name.
    ParamName,
    /// `param-value:` — one parameter's value (wraps a slot).
    ParamValue,
    /// `reasoning:` — reasoning text inside a production.
    Reasoning,
    /// `content:` — free content (for example a tool result's body).
    Content,
}

impl CaptureKind {
    /// Every capture kind, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::Name,
        Self::Id,
        Self::Arguments,
        Self::ParamName,
        Self::ParamValue,
        Self::Reasoning,
        Self::Content,
    ];

    /// The DSL spelling of this capture.
    ///
    /// ```
    /// use purrassist_core::grammar::CaptureKind;
    /// assert_eq!(CaptureKind::ParamName.name(), "param-name");
    /// ```
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Id => "id",
            Self::Arguments => "arguments",
            Self::ParamName => "param-name",
            Self::ParamValue => "param-value",
            Self::Reasoning => "reasoning",
            Self::Content => "content",
        }
    }

    /// The capture spelled `name` in the DSL.
    ///
    /// ```
    /// use purrassist_core::grammar::CaptureKind;
    /// assert_eq!(CaptureKind::from_name("id"), Some(CaptureKind::Id));
    /// assert_eq!(CaptureKind::from_name("body"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

macro_rules! table_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub(crate) u32);

        impl $name {
            /// This id's index in its compiled table.
            #[must_use]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

table_id!(
    /// A literal terminal of one [`CompiledGrammar`]; see
    /// [`CompiledGrammar::literal`].
    LiteralId
);
table_id!(
    /// A closer class of one [`CompiledGrammar`]; see
    /// [`CompiledGrammar::closer_spellings`].
    CloserId
);
table_id!(
    /// A class terminal of one [`CompiledGrammar`]; see
    /// [`CompiledGrammar::class_source`].
    ClassId
);
table_id!(
    /// A delimiter set of one [`CompiledGrammar`]; see
    /// [`CompiledGrammar::delimiters`].
    DelimiterSetId
);
table_id!(
    /// A production of one [`CompiledGrammar`]; see
    /// [`CompiledGrammar::production`].
    ProductionId
);

/// A compiled production expression, as written (references are not
/// inlined), so a printer can interpret it in reverse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    /// A literal terminal.
    Literal(LiteralId),
    /// A closer class terminal; it prints as the canonical spelling.
    Closer(CloserId),
    /// A class terminal.
    Class(ClassId),
    /// A verbatim body up to a delimiter.
    Until(DelimiterSetId),
    /// A codec slot.
    Slot {
        /// The codec decoding the body.
        codec: CodecKind,
        /// The delimiters ending the body.
        delimiters: DelimiterSetId,
    },
    /// A named capture.
    Capture {
        /// What the capture records.
        kind: CaptureKind,
        /// The captured expression.
        inner: Box<Self>,
    },
    /// Items in order.
    Sequence(Vec<Self>),
    /// Alternatives.
    Choice(Vec<Self>),
    /// `inner` repeated between `min` and `max` (unbounded when `None`) times.
    Repeat {
        /// The repeated expression.
        inner: Box<Self>,
        /// Minimum repetitions.
        min: u32,
        /// Maximum repetitions.
        max: Option<u32>,
    },
    /// Another production.
    Reference(ProductionId),
}

/// Why a grammar did not compile. Every syntax or semantic error carries
/// the byte offset in the grammar source where it was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrammarError {
    /// Malformed DSL syntax.
    Syntax {
        /// Byte offset of the problem.
        offset: usize,
        /// What was expected or found.
        message: String,
    },
    /// A literal (or delimiter, spelling or marker) is the empty string.
    EmptyLiteral {
        /// Byte offset of the literal.
        offset: usize,
    },
    /// A keyword used as a production or closer name.
    ReservedName {
        /// Byte offset of the name.
        offset: usize,
        /// The name.
        name: String,
    },
    /// A name defined twice (productions and closers share one namespace),
    /// or a second `reasoning` statement.
    DuplicateName {
        /// Byte offset of the second definition.
        offset: usize,
        /// The name.
        name: String,
    },
    /// A reference to an undefined production or closer.
    UnknownName {
        /// Byte offset of the reference.
        offset: usize,
        /// The name.
        name: String,
    },
    /// A production that refers to itself, directly or indirectly.
    Recursive {
        /// Byte offset of the production on the cycle.
        offset: usize,
        /// The production's name.
        name: String,
    },
    /// A capture kind that is not one of [`CaptureKind`].
    UnknownCapture {
        /// Byte offset of the capture.
        offset: usize,
        /// The capture name.
        name: String,
    },
    /// A slot naming a codec that is not one of [`CodecKind`].
    UnknownCodec {
        /// Byte offset of the codec name.
        offset: usize,
        /// The codec name.
        name: String,
    },
    /// A class pattern that is malformed, needs backtracking, or matches
    /// the empty string.
    InvalidClass {
        /// Byte offset of the class.
        offset: usize,
        /// Why it was refused.
        message: String,
    },
    /// A required production (`call`, `result`, `declaration`) is missing.
    MissingProduction {
        /// The production's name.
        name: &'static str,
    },
    /// A recognized production does not begin with a literal opener, or
    /// matches the empty string.
    InvalidOpener {
        /// Byte offset of the production.
        offset: usize,
        /// The production's name.
        production: String,
    },
    /// An `arguments:` or `param-value:` capture that does not wrap a slot.
    CaptureTarget {
        /// Byte offset of the capture.
        offset: usize,
        /// The capture name.
        capture: &'static str,
    },
    /// A `param-name`/`param-value` capture outside an adjacent pair in one
    /// sequence.
    UnpairedParameter {
        /// Byte offset of the capture.
        offset: usize,
    },
    /// A production with both an `arguments` slot and parameter pairs.
    ConflictingArguments {
        /// Byte offset of the production.
        offset: usize,
    },
    /// The governor's budget was exhausted, or the caller cancelled.
    Resource(StopReason),
}

impl GrammarError {
    /// The byte offset in the grammar source, when the error has one.
    ///
    /// ```
    /// use purrassist_core::grammar::GrammarError;
    /// assert_eq!(GrammarError::EmptyLiteral { offset: 4 }.offset(), Some(4));
    /// assert_eq!(GrammarError::MissingProduction { name: "call" }.offset(), None);
    /// ```
    #[must_use]
    pub const fn offset(&self) -> Option<usize> {
        match self {
            Self::Syntax { offset, .. }
            | Self::EmptyLiteral { offset }
            | Self::ReservedName { offset, .. }
            | Self::DuplicateName { offset, .. }
            | Self::UnknownName { offset, .. }
            | Self::Recursive { offset, .. }
            | Self::UnknownCapture { offset, .. }
            | Self::UnknownCodec { offset, .. }
            | Self::InvalidClass { offset, .. }
            | Self::InvalidOpener { offset, .. }
            | Self::CaptureTarget { offset, .. }
            | Self::UnpairedParameter { offset }
            | Self::ConflictingArguments { offset } => Some(*offset),
            Self::MissingProduction { .. } | Self::Resource(_) => None,
        }
    }
}

impl fmt::Display for GrammarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { offset, message } => write!(formatter, "byte {offset}: {message}"),
            Self::EmptyLiteral { offset } => write!(formatter, "byte {offset}: empty literal"),
            Self::ReservedName { offset, name } => {
                write!(formatter, "byte {offset}: `{name}` is a keyword")
            }
            Self::DuplicateName { offset, name } => {
                write!(formatter, "byte {offset}: `{name}` is defined twice")
            }
            Self::UnknownName { offset, name } => {
                write!(formatter, "byte {offset}: `{name}` is not defined")
            }
            Self::Recursive { offset, name } => {
                write!(formatter, "byte {offset}: production `{name}` is recursive")
            }
            Self::UnknownCapture { offset, name } => {
                write!(formatter, "byte {offset}: unknown capture `{name}`")
            }
            Self::UnknownCodec { offset, name } => {
                write!(formatter, "byte {offset}: unknown codec `{name}`")
            }
            Self::InvalidClass { offset, message } => {
                write!(formatter, "byte {offset}: invalid class: {message}")
            }
            Self::MissingProduction { name } => {
                write!(formatter, "missing required production `{name}`")
            }
            Self::InvalidOpener { offset, production } => write!(
                formatter,
                "byte {offset}: production `{production}` must begin with a literal opener and must not match the empty string"
            ),
            Self::CaptureTarget { offset, capture } => {
                write!(
                    formatter,
                    "byte {offset}: `{capture}:` must wrap a slot(...)"
                )
            }
            Self::UnpairedParameter { offset } => write!(
                formatter,
                "byte {offset}: param-name/param-value captures must be adjacent pairs in one sequence"
            ),
            Self::ConflictingArguments { offset } => write!(
                formatter,
                "byte {offset}: a production uses either an arguments slot or parameter pairs"
            ),
            Self::Resource(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for GrammarError {}

impl From<StopReason> for GrammarError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// The grammar compiler.
///
/// See the [module documentation](self) for the DSL.
#[derive(Clone, Copy, Debug)]
pub struct Grammar;

impl Grammar {
    /// Compile a grammar under a caller-owned governor.
    ///
    /// Charges `InputBytes` for the source; `Work` per source byte, syntax
    /// node, instruction, and compiled table entry (see [`cost::K_COMPILE`]);
    /// `Depth` per nesting level; and `ScratchBytes` for every retained table
    /// (literal tries, Aho–Corasick matchers, class automata), which stay
    /// charged for the life of the operation like other compiled artifacts.
    ///
    /// # Errors
    /// Returns a typed [`GrammarError`] with the source offset for malformed
    /// or semantically invalid grammars, and [`GrammarError::Resource`] when
    /// the governor's budget is exhausted or the caller cancelled.
    ///
    /// ```
    /// use purrassist_core::grammar::{Grammar, GrammarError};
    /// use purrassist_core::{Budget, Governor, StopReason, contract::Resource};
    /// let text = "grammar \"g\";\ncall = \"<c>\" undefined;";
    /// let error = Grammar::compile_governed(text, &mut Governor::new(Budget::default()));
    /// assert_eq!(
    ///     error.unwrap_err(),
    ///     GrammarError::UnknownName { offset: 26, name: "undefined".into() }
    /// );
    /// let mut starved = Governor::new(Budget { work: 0, ..Budget::default() });
    /// assert_eq!(
    ///     Grammar::compile_governed(text, &mut starved).unwrap_err(),
    ///     GrammarError::Resource(StopReason::BudgetExceeded(Resource::Work))
    /// );
    /// ```
    pub fn compile_governed(
        text: &str,
        governor: &mut Governor,
    ) -> Result<CompiledGrammar, GrammarError> {
        let document = syntax::parse(text, governor)?;
        compile::compile(document, Digest::of(text.as_bytes()), governor)
    }
}

/// The literal spellings a closer class accepts, canonical first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CloserClass {
    pub(crate) name: String,
    pub(crate) spellings: Vec<Vec<u8>>,
}

/// One literal terminal: its bytes and, for a closer reference, its class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Literal {
    pub(crate) bytes: Vec<u8>,
    pub(crate) closer: Option<CloserId>,
}

/// One class terminal: its source and compiled automaton.
#[derive(Clone, Debug)]
pub(crate) struct ClassTerminal {
    pub(crate) source: String,
    pub(crate) automaton: ByteAutomaton,
}

/// One delimiter set: the declared literals, and the matcher over them and
/// their confusable spellings.
#[derive(Clone, Debug)]
pub(crate) struct DelimiterSet {
    pub(crate) declared: Vec<Vec<u8>>,
    pub(crate) matcher: automaton::Matcher,
}

/// One named production.
#[derive(Clone, Debug)]
pub(crate) struct Production {
    pub(crate) name: String,
    pub(crate) expr: Expr,
}

/// A compiled, invertible dialect grammar.
///
/// Holds the productions as written ([`Expr`]), the terminal tables they
/// reference, and the byte-level recognizer tables for `call` and `result`.
/// Grammars are identified by their id and the SHA-256 digest of their
/// source text.
#[derive(Clone, Debug)]
pub struct CompiledGrammar {
    pub(crate) id: String,
    pub(crate) digest: Digest,
    pub(crate) literals: Vec<Literal>,
    pub(crate) literal_tables: Vec<LiteralTables>,
    pub(crate) closers: Vec<CloserClass>,
    pub(crate) confusables: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) classes: Vec<ClassTerminal>,
    pub(crate) delimiters: Vec<DelimiterSet>,
    pub(crate) productions: Vec<Production>,
    pub(crate) call: ProductionId,
    pub(crate) result: ProductionId,
    pub(crate) declaration: ProductionId,
    pub(crate) reasoning: Option<(Vec<u8>, Vec<u8>)>,
    pub(crate) programs: [Program; 2],
    pub(crate) stray: automaton::Trie,
    pub(crate) longest: usize,
}

// Compiled grammars are shared across threads (`DialectId::Custom` holds an
// `Arc`): every table they hold is plain, owned data.
const _: fn() = || {
    const fn shareable<T: Send + Sync>() {}
    shareable::<CompiledGrammar>();
};

impl CompiledGrammar {
    /// The grammar's declared id.
    ///
    /// See [`Grammar::compile_governed`]'s module example.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The SHA-256 digest of the grammar source text.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::{Budget, Governor, digest::Digest};
    /// let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(grammar.digest(), Digest::of(text.as_bytes()));
    /// ```
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }

    /// The `call` production.
    ///
    /// See the [module documentation](self) for an example.
    #[must_use]
    pub fn call(&self) -> &Expr {
        &self.productions[self.call.index()].expr
    }

    /// The `result` production.
    ///
    /// ```
    /// # use purrassist_core::grammar::{Expr, Grammar};
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// let Expr::Literal(opener) = grammar.result() else { unreachable!() };
    /// assert_eq!(grammar.literal(*opener), b"<r>");
    /// ```
    #[must_use]
    pub fn result(&self) -> &Expr {
        &self.productions[self.result.index()].expr
    }

    /// The `declaration` production, retained for printing.
    ///
    /// ```
    /// # use purrassist_core::grammar::{Expr, Grammar};
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// assert!(matches!(grammar.declaration(), Expr::Literal(_)));
    /// ```
    #[must_use]
    pub fn declaration(&self) -> &Expr {
        &self.productions[self.declaration.index()].expr
    }

    /// The production named `name`, with its id.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; w = /[a-z]+/; call = \"<c>\" w; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// let (id, _) = grammar.production("w").unwrap();
    /// assert_eq!(grammar.production_name(id), "w");
    /// assert!(grammar.production("missing").is_none());
    /// ```
    #[must_use]
    pub fn production(&self, name: &str) -> Option<(ProductionId, &Expr)> {
        self.productions
            .iter()
            .enumerate()
            .find(|(_, production)| production.name == name)
            .map(|(index, production)| (ProductionId(index_u32(index)), &production.expr))
    }

    /// The name of production `id`.
    ///
    /// See [`CompiledGrammar::production`].
    #[must_use]
    pub fn production_name(&self, id: ProductionId) -> &str {
        &self.productions[id.index()].name
    }

    /// The expression of production `id`.
    ///
    /// ```
    /// # use purrassist_core::grammar::{Expr, Grammar};
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; w = /[a-z]+/; call = \"<c>\" w; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// let (id, _) = grammar.production("w").unwrap();
    /// let Expr::Class(class) = grammar.production_expr(id) else { unreachable!() };
    /// assert_eq!(grammar.class_source(*class), "[a-z]+");
    /// ```
    #[must_use]
    pub fn production_expr(&self, id: ProductionId) -> &Expr {
        &self.productions[id.index()].expr
    }

    /// The bytes of literal `id`.
    ///
    /// See [`CompiledGrammar::result`].
    #[must_use]
    pub fn literal(&self, id: LiteralId) -> &[u8] {
        &self.literals[id.index()].bytes
    }

    /// The spellings of closer class `id`, canonical first.
    ///
    /// See the [module documentation](self) for an example.
    #[must_use]
    pub fn closer_spellings(&self, id: CloserId) -> &[Vec<u8>] {
        &self.closers[id.index()].spellings
    }

    /// The declared name of closer class `id`.
    ///
    /// ```
    /// # use purrassist_core::grammar::{Expr, Grammar};
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; closer end = \"</c>\"; call = \"<c>\" end; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// let Expr::Sequence(items) = grammar.call() else { unreachable!() };
    /// let Expr::Closer(end) = items[1] else { unreachable!() };
    /// assert_eq!(grammar.closer_name(end), "end");
    /// ```
    #[must_use]
    pub fn closer_name(&self, id: CloserId) -> &str {
        &self.closers[id.index()].name
    }

    /// The pattern source of class terminal `id`, as written.
    ///
    /// See [`CompiledGrammar::production_expr`].
    #[must_use]
    pub fn class_source(&self, id: ClassId) -> &str {
        &self.classes[id.index()].source
    }

    /// The declared delimiters of delimiter set `id` (confusable spellings
    /// the matcher also accepts are not listed).
    ///
    /// ```
    /// # use purrassist_core::grammar::{Expr, Grammar};
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; call = \"<c>\" until(\"</c>\", \"<c>\"); result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// let Expr::Sequence(items) = grammar.call() else { unreachable!() };
    /// let Expr::Until(set) = items[1] else { unreachable!() };
    /// assert_eq!(grammar.delimiters(set), [b"</c>".to_vec(), b"<c>".to_vec()]);
    /// ```
    #[must_use]
    pub fn delimiters(&self, id: DelimiterSetId) -> &[Vec<u8>] {
        &self.delimiters[id.index()].declared
    }

    /// The declared confusable pairs `(canonical, variant)`.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; confusable \"｜\" \"|\"; call = \"<｜c｜>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(grammar.confusables(), [("｜".as_bytes().to_vec(), b"|".to_vec())]);
    /// ```
    #[must_use]
    pub fn confusables(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.confusables
    }

    /// The reasoning markers `(open, close)`, when declared.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; reasoning \"<think>\" \"</think>\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(grammar.reasoning(), Some((&b"<think>"[..], &b"</think>"[..])));
    /// ```
    #[must_use]
    pub fn reasoning(&self) -> Option<(&[u8], &[u8])> {
        self.reasoning
            .as_ref()
            .map(|(open, close)| (open.as_slice(), close.as_slice()))
    }

    /// The maximum number of simultaneously live recognizer configurations
    /// for this grammar's `call` and `result` productions: one per program
    /// location, plus one per byte position of each literal (canonical,
    /// variant, confusable and stray-closer spellings). `K_THREAD` times
    /// this bounds the configuration-step work per input byte; see
    /// [`cost`].
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; call = \"<c>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(grammar.configuration_bound(), 6); // two 3-byte literals
    /// ```
    #[must_use]
    pub fn configuration_bound(&self) -> u64 {
        self.programs.iter().map(|program| program.bound).sum()
    }

    /// The longest byte length of any literal spelling, delimiter, or
    /// reasoning marker in this grammar: how far a repair or a delimiter
    /// replay may look back.
    ///
    /// ```
    /// # use purrassist_core::grammar::Grammar;
    /// # use purrassist_core::{Budget, Governor};
    /// let text = "grammar \"g\"; call = \"<call>\"; result = \"<r>\"; declaration = \"<d>\";";
    /// let grammar = Grammar::compile_governed(text, &mut Governor::new(Budget::default())).unwrap();
    /// assert_eq!(grammar.longest_literal(), 6);
    /// ```
    #[must_use]
    pub const fn longest_literal(&self) -> usize {
        self.longest
    }
}

/// A table index as a compact `u32` id (tables never approach `u32::MAX`
/// entries under any governed budget; an overflow saturates).
pub(crate) fn index_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}
