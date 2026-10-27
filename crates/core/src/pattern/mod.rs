// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Budgeted regular-expression evaluation for schema patterns.
//!
//! Implements the full ECMA-262 `u`-mode syntax (named groups, `\k<name>`,
//! `\u{...}`, surrogate-pair combination, `\p{...}`/`\P{...}` for
//! `General_Category`, `Script`, `Script_Extensions`, and every binary
//! Unicode property) over two engines, selected per pattern and recorded as
//! evidence ([`Pattern::engine_evidence`]):
//!
//! - **`pattern.engine.derivative-v1`**: Antimirov/Brzozowski derivatives
//!   over Unicode interval-set predicates (`derive.rs`) for every pattern
//!   without a backreference or a lookaround. Linear-time; used for
//!   [`Pattern::is_match`] and [`Pattern::generate`].
//! - **`pattern.engine.backtrack-v1`**: exploration-based matching
//!   (`backtrack.rs`), Work-charged per step, for backreferences and
//!   lookaround. Backreferences can describe nonregular languages because
//!   they depend on earlier captured text. Lookaround over regular components
//!   remains regular; this implementation evaluates it through bounded
//!   exploration rather than the derivative engine.
//!
//! `pattern` is unanchored search (JSON Schema semantics: found anywhere in
//! the subject), on both engines.
//!
//! [`ByteLanguage`] separately constructs full-match languages over arbitrary
//! bytes through the same derivative arena/session. Its direct postfix input
//! needs no Unicode regex parser or syntax-storage estimate; existing `Pattern`
//! and UTF-8 byte-automaton semantics are unchanged.
mod ast;
mod backtrack;
mod bytes;
pub(crate) mod derive;
mod generate;
mod parse;
mod storage;
mod unicode;

pub use bytes::{ByteCompileError, ByteInclusion, ByteInstruction, ByteLanguage};
pub use derive::{DerivativeDfa, DfaState};

use crate::contract::{
    Budget, Evidence, Governor, Resource, ScratchCharge, ScratchVec, StopReason,
};

impl ScratchCharge for char {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 4);
        4
    };
}

/// Admit the matcher's scalar buffer before collecting any input. Its owner
/// stays alive through either engine and releases the buffer on every exit.
fn match_chars(text: &str, governor: &mut Governor) -> Result<ScratchVec<char>, StopReason> {
    governor.charge(
        Resource::InputBytes,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    let mut chars = ScratchVec::new(governor);
    chars.extend(text.chars())?;
    Ok(chars)
}
use core::fmt;

/// ECMA-262 `u`-mode word-boundary class: ASCII letters, digits, and underscore.
const fn is_word(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// A compiled pattern; no ambient regex engine or platform-dependent dialect.
///
/// Patterns own their admitted storage and do not implement `Clone`. Borrow a
/// prepared pattern for matching/generation and pass borrowed exclusions through
/// [`GenerateOptions`].
#[derive(Debug)]
pub struct Pattern {
    ast: ast::Node,
    captures: usize,
    engine: Engine,
    compiled: Option<derive::Compiled>,
    _syntax_lease: crate::contract::ScratchLease,
}

/// A derivative-engine pattern's compiled automaton, detached from the
/// pattern's syntax tree by [`Pattern::into_automaton`].
#[derive(Debug)]
pub struct ByteAutomaton {
    compiled: derive::Compiled,
}

impl ByteAutomaton {
    // The same scalar/context/product search used by Pattern::generate. Grammar
    // classes already retain this exact anchored compiled arena; no syntax
    // reparse or second witness engine is needed during canonical printing.
    pub(crate) fn generate_member(
        &self,
        min_len: usize,
        max_len: usize,
        governor: &mut Governor,
    ) -> Result<Option<crate::contract::ScratchString>, StopReason> {
        match generate::generate_compiled(
            &self.compiled,
            governor,
            GenerateOptions {
                min_len,
                max_len: Some(max_len),
                avoid: &[],
            },
        )? {
            generate::Generated::Witness(text) => Ok(Some(text)),
            generate::Generated::NoCandidate { .. } => Ok(None),
        }
    }

    /// A resumable byte-level automaton over this pattern, with exactly
    /// [`Pattern::byte_dfa`]'s semantics and resource contract.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` or `Cancelled` when the governor cannot admit
    /// the automaton's working copy of the compiled term arena.
    ///
    /// See [`Pattern::into_automaton`] for an example.
    pub fn dfa(&self, governor: &mut Governor) -> Result<DerivativeDfa<'_>, StopReason> {
        DerivativeDfa::new(&self.compiled, governor)
    }
}

/// Which engine [`Pattern::is_match`]/[`Pattern::generate`] use for this
/// pattern; see the module documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// `pattern.engine.derivative-v1`.
    Derivative,
    /// `pattern.engine.backtrack-v1`.
    Backtrack,
}

/// A precise pattern compilation error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternError {
    /// Character offset in the pattern.
    pub offset: usize,
    /// Invalid or unsupported construct.
    pub message: String,
}
impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at pattern character {}", self.message, self.offset)
    }
}
impl std::error::Error for PatternError {}

/// Governed pattern compilation failure: invalid syntax or an explicit caller
/// budget, native resource/encoding limit, or cancellation refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternCompileError {
    /// The pattern's syntax is invalid or uses an unsupported construct.
    Syntax(PatternError),
    /// Compilation stopped before a complete pattern was available.
    Resource(StopReason),
}
impl fmt::Display for PatternCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::Resource(reason) => reason.fmt(formatter),
        }
    }
}
impl std::error::Error for PatternCompileError {}
impl From<PatternError> for PatternCompileError {
    fn from(error: PatternError) -> Self {
        Self::Syntax(error)
    }
}
impl From<StopReason> for PatternCompileError {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}

/// Bounds and exclusions for [`Pattern::generate`].
#[derive(Clone, Copy, Debug)]
pub struct GenerateOptions<'a> {
    /// Minimum witness length, in Unicode scalar values.
    pub min_len: usize,
    /// Maximum witness length; `None` searches without an upper bound
    /// (bounded in practice by the governor's `Work`/`Depth` budget).
    pub max_len: Option<usize>,
    /// Borrowed patterns the witness must not match anywhere (unanchored,
    /// exactly like [`Pattern::is_match`]); no compiled pattern is cloned.
    pub avoid: &'a [&'a Pattern],
}

/// The result of [`Pattern::generate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GenerateOutcome {
    /// A string that matches this pattern and none of the `avoid` patterns.
    Witness(String),
    /// No witness was produced. `evidence.explanation` distinguishes proven
    /// regular-language emptiness, an exhausted length bound, and a bounded
    /// backtracking search that does not prove a witness impossible.
    NoCandidate(Evidence),
}

impl Pattern {
    /// Compile the pattern, refusing unsupported constructs rather than weakening it.
    ///
    /// Delegates to [`Pattern::compile_governed`] with a default, unshared
    /// [`Governor`]. Prefer `compile_governed` to bound compilation against a
    /// caller-owned budget shared with the rest of an operation.
    ///
    /// # Errors
    /// Returns malformed syntax, excessive nesting, or an unsupported escape.
    /// A resource limit exceeded under the default budget is reported as a
    /// syntax error naming the exhausted resource, since this entry point
    /// cannot report resource usage separately.
    pub fn compile(source: &str) -> Result<Self, PatternError> {
        match Self::compile_governed(source, &mut Governor::new(Budget::default())) {
            Ok(pattern) => Ok(pattern),
            Err(PatternCompileError::Syntax(error)) => Err(error),
            Err(PatternCompileError::Resource(reason)) => Err(PatternError {
                offset: 0,
                message: format!("resource exhausted during compilation: {reason}"),
            }),
        }
    }

    /// Compile the pattern against a caller-owned governor.
    ///
    /// Charges `InputBytes` for the pattern's length. Decoded characters,
    /// named capture indices/alternative paths, pending numeric references and
    /// lexical text use fallible temporary scratch owners and release on every
    /// exit. Source/name/path scans, lookahead, numeric conversion and syntax
    /// diagnostics admit work before processing; names borrow fixed Unicode
    /// identifier tables. Returned diagnostic strings retain their admission.
    ///
    /// AST/interval/Box/Rc storage still uses the separate conservative
    /// `source.len() * 48 + 256` estimate, retained as a lease until syntax drops.
    /// Derivative term/index, class and cut buffers retain their actual admitted
    /// owners until the pattern or detached automaton drops. Native translation
    /// admits lookup/shift, traversal, comparison/copy and canonical union work,
    /// plus caller `Depth` and a native recursion ceiling of 128: the root is
    /// depth one and each AST child adds one. Final cancellation is checked
    /// before publication. Actual AST allocation, Unicode expansion and AST
    /// classification remain separate; parsing retains its own depth contract.
    /// Full ECMA-262 `u`-mode syntax
    /// is supported: named groups, `\k<name>`, `\u{...}`, surrogate-pair
    /// combination, and `\p{...}`/`\P{...}` for `General_Category`,
    /// `Script`, `Script_Extensions`, and every ECMA-262 binary Unicode
    /// property. A pattern containing neither a backreference nor a
    /// lookaround compiles a derivative automaton up front (charged here);
    /// see the module documentation for engine selection.
    ///
    /// # Errors
    /// Returns [`PatternCompileError::Syntax`] for malformed patterns and
    /// [`PatternCompileError::Resource`] for governor or cancellation refusal.
    /// Temporary parser buffers admit storage before allocation; the remaining
    /// AST and Unicode class expansion retain the estimate boundary above.
    /// A finite repetition maximum whose `max + 1` encoding exceeds `u64`
    /// returns sticky `BudgetExceeded(Work)` rather than changing the bound.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::{Pattern, PatternCompileError}};
    /// let mut governor = Governor::new(Budget {
    ///     scratch_bytes: 4,
    ///     ..Budget::default()
    /// });
    /// let error = Pattern::compile_governed("a+", &mut governor).unwrap_err();
    /// assert!(matches!(error, PatternCompileError::Resource(_)));
    /// ```
    pub fn compile_governed(
        source: &str,
        governor: &mut Governor,
    ) -> Result<Self, PatternCompileError> {
        let length = u64::try_from(source.len()).unwrap_or(u64::MAX);
        governor.charge(Resource::InputBytes, length)?;
        // AST nodes and interval owners retain the existing separate estimate.
        let syntax_lease = governor.lease_scratch(length.saturating_mul(48).saturating_add(256))?;
        let (ast, captures) = parse::parse(source, governor)?;
        governor.check()?;
        let backtracking = ast.requires_backtracking();
        let (engine, compiled) = if backtracking {
            (Engine::Backtrack, None)
        } else {
            (Engine::Derivative, Some(derive::compile(&ast, governor)?))
        };
        governor.check()?;
        Ok(Self {
            ast,
            captures,
            engine,
            compiled,
            _syntax_lease: syntax_lease,
        })
    }

    /// Search for a match, including unanchored matches as required by JSON Schema.
    ///
    /// # Errors
    /// Returns a governor failure; this is never converted into a non-match.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let pattern = Pattern::compile(r"^\p{Script=Greek}+$").unwrap();
    /// let mut governor = Governor::new(Budget::default());
    /// assert_eq!(pattern.is_match("Ελλάδα", &mut governor), Ok(true));
    /// assert_eq!(pattern.is_match("Hello", &mut governor), Ok(false));
    /// ```
    pub fn is_match(&self, text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
        match &self.compiled {
            Some(compiled) => {
                governor.charge(
                    Resource::InputBytes,
                    u64::try_from(text.len()).unwrap_or(u64::MAX),
                )?;
                derive::is_match(compiled, text, governor)
            }
            None => self.is_match_backtracking(text, governor),
        }
    }

    /// A resumable byte-level automaton for this pattern, or `None` when the
    /// pattern uses [`Engine::Backtrack`] (backreferences and lookaround are
    /// not finite-state over this engine).
    ///
    /// The automaton has exactly [`Pattern::is_match`]'s unanchored search
    /// semantics, which is itself implemented by stepping it; see
    /// [`DerivativeDfa`] for the state and resource contract.
    ///
    /// # Errors
    /// Returns `BudgetExceeded` or `Cancelled` when the governor cannot admit
    /// the automaton's working copy of the compiled term arena (`Work` and
    /// `ScratchBytes` proportional to the compiled pattern).
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let mut governor = Governor::new(Budget::default());
    /// let digits = Pattern::compile(r"^\d+$").unwrap();
    /// let mut dfa = digits.byte_dfa(&mut governor)?.expect("derivative pattern");
    /// let mut state = dfa.start();
    /// for &byte in b"2026" {
    ///     state = dfa.step(state, byte, &mut governor)?;
    /// }
    /// assert!(dfa.accepting(state, &mut governor)?);
    /// assert!(Pattern::compile(r"(a)\1").unwrap().byte_dfa(&mut governor)?.is_none());
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    pub fn byte_dfa(
        &self,
        governor: &mut Governor,
    ) -> Result<Option<DerivativeDfa<'_>>, StopReason> {
        self.compiled
            .as_ref()
            .map(|compiled| DerivativeDfa::new(compiled, governor))
            .transpose()
    }

    /// Detach this pattern's derivative automaton from its syntax tree;
    /// `None` when the pattern uses [`Engine::Backtrack`], which has no
    /// finite automaton.
    ///
    /// The automaton owns only the compiled, read-only term arena and class
    /// tables (already charged by [`Pattern::compile_governed`] and retained
    /// as-is, with no copy). Discarding syntax releases its estimate lease;
    /// dropping the automaton releases its own buffers. Unlike [`Pattern`] it is `Send + Sync` and
    /// can live in shared, compiled artifacts.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let mut governor = Governor::new(Budget::default());
    /// let automaton = Pattern::compile(r"^[a-z]+$").unwrap().into_automaton().unwrap();
    /// let mut dfa = automaton.dfa(&mut governor)?;
    /// let mut state = dfa.start();
    /// for &byte in b"abc" {
    ///     state = dfa.step(state, byte, &mut governor)?;
    /// }
    /// assert!(dfa.accepting(state, &mut governor)?);
    /// assert!(Pattern::compile(r"(a)\1").unwrap().into_automaton().is_none());
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    #[must_use]
    pub fn into_automaton(self) -> Option<ByteAutomaton> {
        self.compiled.map(|compiled| ByteAutomaton { compiled })
    }

    /// Which engine this pattern uses; see the module documentation.
    #[must_use]
    pub const fn engine(&self) -> Engine {
        self.engine
    }

    /// Evidence naming the engine [`Pattern::is_match`]/[`Pattern::generate`]
    /// use for this pattern (`pattern.engine.derivative-v1` or
    /// `pattern.engine.backtrack-v1`), for a caller that publishes a receipt
    /// carrying it.
    ///
    /// ```
    /// use purrassist_core::pattern::Pattern;
    /// let plain = Pattern::compile("[a-z]+").unwrap();
    /// assert_eq!(plain.engine_evidence().rule, "pattern.engine.derivative-v1");
    /// let backref = Pattern::compile(r"(a)\1").unwrap();
    /// assert_eq!(backref.engine_evidence().rule, "pattern.engine.backtrack-v1");
    /// ```
    #[must_use]
    pub fn engine_evidence(&self) -> Evidence {
        let (rule, explanation) = match self.engine {
            Engine::Derivative => (
                "pattern.engine.derivative-v1",
                "no backreference or lookaround; matched by the derivative automaton",
            ),
            Engine::Backtrack => (
                "pattern.engine.backtrack-v1",
                "a backreference or lookaround requires exploration-based matching",
            ),
        };
        Evidence {
            rule: rule.into(),
            explanation: explanation.into(),
            span: None,
            destination: String::new(),
        }
    }

    /// Match using the backtracking evaluator regardless of [`Pattern::engine`].
    ///
    /// The backtracking evaluator supports every construct this crate can
    /// compile (it is what [`Pattern::is_match`] itself uses for
    /// [`Engine::Backtrack`] patterns), so this never fails to *support* a
    /// pattern the way [`Pattern::generate`]'s derivative path can decline
    /// one; it can still return a governor failure. For an
    /// [`Engine::Backtrack`] pattern this is identical to
    /// [`Pattern::is_match`]. Exists for differential testing between the
    /// two engines and for a caller that specifically wants
    /// Work-charged-per-step semantics regardless of which engine this
    /// pattern would otherwise use.
    ///
    /// # Errors
    /// Returns a governor failure; this is never converted into a non-match.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::Pattern};
    /// let pattern = Pattern::compile("[a-z]+[0-9]").unwrap();
    /// let mut governor = Governor::new(Budget::default());
    /// assert_eq!(
    ///     pattern.is_match("ab1", &mut governor),
    ///     pattern.is_match_backtracking("ab1", &mut governor),
    /// );
    /// ```
    pub fn is_match_backtracking(
        &self,
        text: &str,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let chars = match_chars(text, governor)?;
        backtrack::is_match(&self.ast, self.captures, chars.as_slice(), governor)
    }

    /// Generate a shortest witness string that matches this pattern and none
    /// of `options.avoid`, within `[options.min_len, options.max_len]`.
    ///
    /// A pattern using [`Engine::Derivative`] is generated by a governed
    /// breadth-first search of the product of this pattern's automaton and
    /// every `avoid` pattern's own unanchored-search automaton. Acceptance and
    /// state identity retain start and word-boundary context. Matches that depend
    /// on the end of the string exclude only complete candidates; matches already
    /// established before a next character exclude all their extensions.
    /// The search establishes emptiness by
    /// exhaustion, never by assumption. A pattern using [`Engine::Backtrack`]
    /// (or with any `avoid` pattern using it) instead enumerates candidates
    /// from its syntax tree and verifies each with [`Pattern::is_match`],
    /// since its language need not be regular; exhaustion of that bounded
    /// enumeration is reported explicitly, not as proof of emptiness.
    /// Frontier text, product states, cycle keys, captures, verification
    /// buffers, and the returned witness or evidence are admitted against
    /// `ScratchBytes` before allocation. Temporary ownership releases its
    /// charge on every exit; returned plain strings retain their charges.
    /// Copying candidate text and scanning it for verification also consumes
    /// `Work`, so forcing a long minimum length can exhaust either resource.
    ///
    /// # Errors
    /// Returns a governor failure (`BudgetExceeded`/`Cancelled`) if the
    /// search is stopped before it can return a witness or establish
    /// [`GenerateOutcome::NoCandidate`].
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, pattern::{GenerateOptions, GenerateOutcome, Pattern}};
    /// let mut governor = Governor::new(Budget::default());
    /// let digits = Pattern::compile(r"^\d{3}$").unwrap();
    /// let avoid = Pattern::compile("000").unwrap();
    /// let outcome = digits
    ///     .generate(&mut governor, GenerateOptions { min_len: 0, max_len: Some(5), avoid: &[&avoid] })
    ///     .unwrap();
    /// let GenerateOutcome::Witness(text) = outcome else { panic!("expected a witness") };
    /// assert_eq!(digits.is_match(&text, &mut governor), Ok(true));
    ///
    /// let empty = Pattern::compile("abc").unwrap();
    /// let avoid_self = Pattern::compile("abc").unwrap();
    /// let outcome = empty
    ///     .generate(&mut governor, GenerateOptions { min_len: 0, max_len: Some(3), avoid: &[&avoid_self] })
    ///     .unwrap();
    /// assert!(matches!(outcome, GenerateOutcome::NoCandidate(_)));
    /// ```
    pub fn generate(
        &self,
        governor: &mut Governor,
        options: GenerateOptions<'_>,
    ) -> Result<GenerateOutcome, StopReason> {
        generate::generate(self, governor, options)
    }
}

/// The Unicode `Script` value of one code point, for the
/// idn-hostname format validator's RFC 5892 Appendix A CONTEXTO rules; not
/// part of the public API.
pub(crate) fn script_of(code: u32) -> &'static str {
    unicode::script_of(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assertions_classes_and_repetition() {
        for (pattern, text, expected) in [
            ("^[a-z]+[0-9]{2}$", "ab12", true),
            ("^[a-z]+[0-9]{2}$", "ab1", false),
            ("foo|bar", "xxbarxx", true),
            ("(?=abc)ab", "abc", true),
            ("a(?!b)", "ab", false),
            ("(?<=ab)c", "abc", true),
            (r"^(ab)\1$", "abab", true),
            (r"^\d+$", "\u{ff11}\u{ff12}", false),
            (r"^\s$", "\u{a0}", true),
            (".", "\u{2028}", false),
            ("[^]", "\n", true),
            ("^(a?)*$", "", true),
            ("^a$", "a\n", false),
        ] {
            let compiled = Pattern::compile(pattern).expect(pattern);
            assert_eq!(
                compiled.is_match(text, &mut Governor::new(Budget::default())),
                Ok(expected),
                "{pattern:?} {text:?}"
            );
        }
    }
    #[test]
    fn word_boundaries_agree_for_ascii_and_non_ascii_neighbours() {
        for character in ['a', 'Z', '0', '9', '_'] {
            assert!(is_word(character));
        }
        for character in ['é', '猫', '０', '\u{212a}', ' ', '-', '\n', '\u{0301}'] {
            assert!(!is_word(character));
        }
        for (source, text, expected) in [
            (r"\ba\b", "a", true),
            (r"\ba\b", "_a", false),
            (r"\ba\b", "a9", false),
            (r"\ba\b", "éa猫", true),
            (r"\B_\B", "a_9", true),
            (r"\B_\B", "é_猫", false),
        ] {
            let pattern = Pattern::compile(source).expect("boundary pattern");
            assert_eq!(
                pattern.is_match(text, &mut Governor::new(Budget::default())),
                Ok(expected),
                "derivative {source:?} {text:?}"
            );
            assert_eq!(
                pattern.is_match_backtracking(text, &mut Governor::new(Budget::default())),
                Ok(expected),
                "backtracking {source:?} {text:?}"
            );
        }
    }
    #[test]
    fn exhaustion_is_not_false() {
        let pattern = Pattern::compile("(a+)+$").expect("pattern");
        assert!(
            pattern
                .is_match(
                    "aaaaaaaaaaaa!",
                    &mut Governor::new(Budget {
                        work: 10,
                        ..Budget::default()
                    })
                )
                .is_err()
        );
    }
    #[test]
    fn pathological_nested_quantifier_is_linear_not_exponential() {
        let pattern = Pattern::compile("(a+)+b").expect("pattern");
        assert_eq!(pattern.engine(), Engine::Derivative);
        let text = "a".repeat(10_000);
        assert_eq!(
            pattern.is_match(&text, &mut Governor::new(Budget::default())),
            Ok(false)
        );
    }
}

impl crate::contract::ScratchCharge for &Pattern {
    const CHARGE: usize = 8;
}
const _: () = assert!(
    core::mem::size_of::<&Pattern>() <= <&Pattern as crate::contract::ScratchCharge>::CHARGE
);

#[cfg(test)]
mod ownership_tests {
    use super::{ByteInstruction, ByteLanguage, Pattern, PatternCompileError};
    use crate::contract::{Budget, CancellationToken, Governor, Resource, StopReason};

    /// Compiled term/class owners and the existing syntax estimate follow the
    /// artifact lifetime, including detachment and later compiler refusal.
    #[test]
    fn compiled_artifacts_release_on_drop_detachment_and_refusal() {
        for source in ["^(a|b)+$", r"\p{Script=Greek}+", r"(a)\1"] {
            let mut governor = Governor::new(Budget::default());
            let pattern = Pattern::compile_governed(source, &mut governor).unwrap();
            assert!(governor.scratch_meter().live() > 0);
            drop(pattern);
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        let source = "^(a|b)+$";
        let mut governor = Governor::new(Budget::default());
        let pattern = Pattern::compile_governed(source, &mut governor).unwrap();
        let usage = governor.usage();
        let before = governor.scratch_meter().live();
        let automaton = pattern.into_automaton().unwrap();
        assert!(governor.scratch_meter().live() > 0);
        assert!(governor.scratch_meter().live() < before);
        drop(automaton);
        assert_eq!(governor.scratch_meter().live(), 0);
        for (budget, resource) in [
            (
                Budget {
                    work: usage.work / 2,
                    ..Budget::default()
                },
                Resource::Work,
            ),
            (
                Budget {
                    work: usage.work - 1,
                    ..Budget::default()
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..Budget::default()
                },
                Resource::ScratchBytes,
            ),
        ] {
            let mut governor = Governor::new(budget);
            let reason = StopReason::BudgetExceeded(resource);
            assert_eq!(
                Pattern::compile_governed(source, &mut governor).unwrap_err(),
                PatternCompileError::Resource(reason.clone())
            );
            assert_eq!(governor.check(), Err(reason));
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut governor = Governor::with_cancellation(Budget::default(), token);
        assert!(matches!(
            Pattern::compile_governed(source, &mut governor),
            Err(PatternCompileError::Resource(StopReason::Cancelled))
        ));
        assert_eq!(governor.scratch_meter().live(), 0);
        // A finite upper bound uses max+1; the unbounded sentinel is zero.
        let source = format!("a{{0,{}}}", usize::MAX);
        let mut governor = Governor::new(Budget::default());
        let result = Pattern::compile_governed(&source, &mut governor);
        if usize::BITS == 64 {
            let reason = StopReason::BudgetExceeded(Resource::Work);
            assert_eq!(
                result.unwrap_err(),
                PatternCompileError::Resource(reason.clone())
            );
            assert_eq!(governor.check(), Err(reason));
        } else {
            drop(result.unwrap());
        }
        assert_eq!(governor.scratch_meter().live(), 0);
        let mut governor = Governor::new(Budget::default());
        let language = ByteLanguage::compile(
            &[ByteInstruction::Literal(b'a'), ByteInstruction::Star],
            &mut governor,
        )
        .unwrap();
        assert!(governor.scratch_meter().live() > 0);
        drop(language);
        assert_eq!(governor.scratch_meter().live(), 0);
    }

    /// Parser group nesting and derivative AST translation have independent
    /// depth contracts: root depth one and a native lowering ceiling of 128.
    #[test]
    fn translation_native_depth_boundary_is_sticky_and_releases_owners() {
        for (groups, allowed) in [(127, true), (128, false)] {
            let source = format!("{}a{}", "(".repeat(groups), ")".repeat(groups));
            let mut governor = Governor::new(Budget {
                depth: 256,
                ..Budget::default()
            });
            let result = Pattern::compile_governed(&source, &mut governor);
            if allowed {
                drop(result.unwrap());
            } else {
                assert!(matches!(
                    result,
                    Err(PatternCompileError::Resource(StopReason::BudgetExceeded(
                        Resource::Depth
                    )))
                ));
                assert_eq!(
                    governor.check(),
                    Err(StopReason::BudgetExceeded(Resource::Depth))
                );
            }
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        let mut governor = Governor::new(Budget {
            depth: 0,
            ..Budget::default()
        });
        assert!(matches!(
            Pattern::compile_governed("a", &mut governor),
            Err(PatternCompileError::Resource(StopReason::BudgetExceeded(
                Resource::Depth
            )))
        ));
        assert_eq!(governor.scratch_meter().live(), 0);
    }
}
