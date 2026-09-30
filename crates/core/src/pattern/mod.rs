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
mod ast;
mod backtrack;
pub(crate) mod derive;
mod generate;
mod parse;
mod unicode;

use crate::contract::{Budget, Evidence, Governor, Resource, StopReason};
use core::fmt;

/// ECMA-262 `u`-mode word-boundary class: ASCII letters, digits, and underscore.
const fn is_word(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// A compiled pattern; no ambient regex engine or platform-dependent dialect.
#[derive(Clone, Debug)]
pub struct Pattern {
    ast: ast::Node,
    captures: usize,
    engine: Engine,
    compiled: Option<derive::Compiled>,
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

/// Governed pattern compilation failure: either the pattern itself is
/// malformed, or compiling it exceeded the caller's resource limits.
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
    /// Patterns the witness must not match anywhere (unanchored, exactly
    /// like [`Pattern::is_match`]).
    pub avoid: &'a [Pattern],
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
    /// Charges `InputBytes` for the pattern's own length and `ScratchBytes`
    /// for the retained character buffer and syntax tree before building
    /// them, then charges `Depth`/`Work` for each nesting level and
    /// alternation parsed. The compiled pattern is retained by the caller, so
    /// its charge is never released, matching other permanently retained
    /// compiled artifacts (schemas, receipts). Full ECMA-262 `u`-mode syntax
    /// is supported: named groups, `\k<name>`, `\u{...}`, surrogate-pair
    /// combination, and `\p{...}`/`\P{...}` for `General_Category`,
    /// `Script`, `Script_Extensions`, and every ECMA-262 binary Unicode
    /// property. A pattern containing neither a backreference nor a
    /// lookaround compiles a derivative automaton up front (charged here);
    /// see the module documentation for engine selection.
    ///
    /// # Errors
    /// Returns [`PatternCompileError::Syntax`] for malformed patterns and
    /// [`PatternCompileError::Resource`] before any allocation once the
    /// governor's limits (or a shared cancellation token) would be exceeded.
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
        let scalars = u64::try_from(source.chars().count()).unwrap_or(u64::MAX);
        governor.charge(
            Resource::ScratchBytes,
            scalars
                .saturating_mul(4) // the retained `chars: Vec<char>` buffer.
                .saturating_add(length.saturating_mul(48)) // an estimated AST.
                .saturating_add(256),
        )?;
        let (ast, captures) = parse::parse(source, governor)?;
        let backtracking = ast.requires_backtracking();
        let (engine, compiled) = if backtracking {
            (Engine::Backtrack, None)
        } else {
            governor.charge(Resource::ScratchBytes, length.saturating_mul(64))?;
            (Engine::Derivative, Some(derive::compile(&ast)))
        };
        Ok(Self {
            ast,
            captures,
            engine,
            compiled,
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
        governor.charge(
            Resource::InputBytes,
            u64::try_from(text.len()).unwrap_or(u64::MAX),
        )?;
        let chars: Vec<char> = text.chars().collect();
        match &self.compiled {
            Some(compiled) => derive::is_match(compiled, &chars, governor),
            None => backtrack::is_match(&self.ast, self.captures, &chars, governor),
        }
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
        governor.charge(
            Resource::InputBytes,
            u64::try_from(text.len()).unwrap_or(u64::MAX),
        )?;
        let chars: Vec<char> = text.chars().collect();
        backtrack::is_match(&self.ast, self.captures, &chars, governor)
    }

    /// Generate a shortest witness string that matches this pattern and none
    /// of `options.avoid`, within `[options.min_len, options.max_len]`.
    ///
    /// A pattern using [`Engine::Derivative`] is generated by a governed
    /// breadth-first search of the product of this pattern's automaton and
    /// every `avoid` pattern's own unanchored-search automaton (pruning any
    /// path the instant an `avoid` pattern's search would already have
    /// matched, since every extension of a string containing a forbidden
    /// substring still contains it); the search establishes emptiness by
    /// exhaustion, never by assumption. A pattern using [`Engine::Backtrack`]
    /// (or with any `avoid` pattern using it) instead enumerates candidates
    /// from its syntax tree and verifies each with [`Pattern::is_match`],
    /// since its language need not be regular; exhaustion of that bounded
    /// enumeration is reported explicitly, not as proof of emptiness.
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
    ///     .generate(&mut governor, GenerateOptions { min_len: 0, max_len: Some(5), avoid: &[avoid] })
    ///     .unwrap();
    /// let GenerateOutcome::Witness(text) = outcome else { panic!("expected a witness") };
    /// assert_eq!(digits.is_match(&text, &mut governor), Ok(true));
    ///
    /// let empty = Pattern::compile("abc").unwrap();
    /// let avoid_self = Pattern::compile("abc").unwrap();
    /// let outcome = empty
    ///     .generate(&mut governor, GenerateOptions { min_len: 0, max_len: Some(3), avoid: &[avoid_self] })
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

/// The Unicode `Script` value of one code point, for the Task 7
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
