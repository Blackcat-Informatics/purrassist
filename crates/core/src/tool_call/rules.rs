// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The tool-call rule registry: every named, costed rule a recovered call
//! can record (R3.1–R3.2).
//!
//! [`RULES`] holds one [`Rule`] per rule id. Its first rows are the grammar
//! edit operators, built at compile time from
//! [`grammar::edit::OPERATORS`](crate::grammar::edit::OPERATORS) (the single
//! source of their ids, costs, guess flags, readings, grounding and loss
//! kinds; see that module for their table); the rest are the resolution
//! rules applied by [`resolve_forest`](super::resolve::resolve_forest),
//! including the schema-recovery rules tool calls reuse (`json.key.match`,
//! `json.key.relocate`, `json.enum.match`).
//!
//! # Costs and grounding
//!
//! A rule's [`Rule::cost`] is its base cost times its grounding's
//! multiplier: [`DEFINITION_COST_MULTIPLIER`] (1) for a rule grounded in the
//! tool definition, [`SYNTAX_COST_MULTIPLIER`] (2) for one grounded in the
//! dialect grammar alone. Definition-grounded repairs therefore beat
//! syntax-only repairs of equal base cost when the readings of a call are
//! ranked (R3.2): ending a value at a declared parameter's opener
//! (`tool_call.markup.missing-parameter-close`, 2) is cheaper than ending it
//! at an opener of a call naming no declared tool
//! (`tool_call.markup.undeclared-parameter-boundary`, 4). The rules whose
//! cost depends on the names compared (`json.key.match`, `json.enum.match`,
//! `tool_call.value.enum-normalize`, `tool_call.name.match`) charge the
//! shared approximate-name cost of schema recovery per application (2 for
//! equal comparison keys, 3 without punctuation and separators, 5 plus the
//! edit distance otherwise); their row lists its minimum.
//!
//! # Policies
//!
//! A rule is admitted under the [`RepairPolicy`]s it lists.
//! [`RepairPolicy::Structural`] admits only rules with
//! [`Readings::Single`]: markup repairs that leave no other reading.
//! [`RepairPolicy::Tolerant`] admits every rule. Guesses (fuzzy tool and
//! parameter names, approximate enumeration values, guessed JSON repairs,
//! calls inside reasoning or examples) are therefore never part of a
//! `Structural` call. Ambiguity is reported whatever the policy: a call with
//! several equally good readings is per-call `Ambiguous`, never a usable
//! call.
//!
//! # Losses
//!
//! Every rule that discards bytes declares the [`LossKind`] it records.
//! [`LossKind::Markup`] losses (stray, repeated or variant closers, a
//! closers-only body, code-fence markers, framing newlines) are itemized but
//! never demote a call; [`LossKind::Data`] losses (a dropped unknown
//! parameter, a dropped repeated occurrence, dropped prose inside a call,
//! truncation, a dangling opener, an unterminated reasoning block) demote
//! the call that records them to at most `BestGuess`
//! ([`CallOutcome::classify`](super::CallOutcome::classify)). Text *outside*
//! every call (prose around a call, a fence wrapped around a call) is never
//! dropped: it stays content, so it records no loss; a call written inside
//! fenced code or quoted prose is instead flagged as a possible example
//! (`tool_call.context.fenced-example`, a guess).
//!
//! # Resolution rules
//!
//! | Rule id | Cost | Guess | Readings | Grounding | Loss | Applied when |
//! | --- | --- | --- | --- | --- | --- | --- |
//! | `tool_call.name.alias` | 0 | no | single | definition | none | the call names a tool by an explicit alias |
//! | `tool_call.name.match` | 2+ | yes | multiple | definition | none | a near-miss tool name accepted by [`matching::resolve_governed`](crate::matching::resolve_governed) |
//! | `tool_call.parameter.alias` | 0 | no | single | definition | none | a parameter named by an explicit alias |
//! | `json.key.match` | 2+ | yes | multiple | definition | none | a near-miss parameter name renamed to a declared one |
//! | `json.key.relocate` | 12 | yes | multiple | definition | none | a required parameter moved out of a nested object |
//! | `tool_call.parameter.drop-unknown` | 50 | no | single | definition | data | an undeclared parameter dropped under `DropWithLoss` |
//! | `tool_call.parameter.pass-through` | 0 | no | single | definition | none | an undeclared parameter kept under `PassThrough` |
//! | `tool_call.parameter.repeated` | 0 | yes | multiple | definition | data | one occurrence of a repeated non-repeatable parameter kept (first or last) |
//! | `tool_call.parameter.collect` | 0 | no | single | definition | none | a repeatable parameter's occurrences collected into an array |
//! | `tool_call.value.enum-normalize` | 2+ | no | single | definition | none | an enumeration value equal to one literal after case folding and whitespace normalization |
//! | `json.enum.match` | 2+ | yes | multiple | definition | none | an enumeration value matched approximately |
//! | `tool_call.value.decode-typed` | 0 | no | single | definition | none | XML text of a typed parameter read by its declared type after the call's name was resolved |
//! | `tool_call.truncated` | 0 | no | single | syntax | data | a call cut off by a length stop |
//! | `tool_call.markup.dangling-opener` | 0 | no | single | syntax | data | a call attempt still open at the end of output under a non-length stop |
//! | `tool_call.id.dedup` | 0 | no | single | syntax | none | a missing or duplicate source call id renumbered (recorded by the recovery operation's id policy) |
//!
//! [`DISABLED`] lists the schema-recovery rules tool calls never apply:
//! inventing a required value, stringifying a scalar, and decoding a JSON
//! string into a value. No rule synthesizes a parameter name from
//! positional text, accepts trailing free text as an argument, or invents a
//! value (R3.5).
//!
//! ```
//! use purrassist_core::grammar::edit::{EditOperator, Grounding, LossKind, Readings};
//! use purrassist_core::tool_call::RepairPolicy;
//! use purrassist_core::tool_call::rules::{self, RULES};
//! let close = rules::rule("tool_call.markup.missing-parameter-close").unwrap();
//! assert_eq!((close.cost, close.readings, close.grounding), (2, Readings::Single, Grounding::Definition));
//! assert!(close.admits(RepairPolicy::Structural));
//! let name = rules::rule("tool_call.name.match").unwrap();
//! assert!(name.guess && !name.admits(RepairPolicy::Structural));
//! assert_eq!(rules::of_operator(EditOperator::DeleteStrayCloser).loss, Some(LossKind::Markup));
//! assert!(RULES.iter().all(|rule| rule.readings == Readings::Single || !rule.admits(RepairPolicy::Structural)));
//! ```
use super::RepairPolicy;
use crate::grammar::edit::{
    EditOperator, Grounding, LossKind, OPERATOR_COUNT, OPERATOR_TABLE, OperatorRule, Readings,
};
use crate::schema::recovery::{FOLDED_NAME_COST, KEY_RELOCATE_COST, KEY_REMOVE_COST};

/// Cost multiplier of a rule grounded in the tool definition.
pub const DEFINITION_COST_MULTIPLIER: u64 = 1;

/// Cost multiplier of a rule grounded in the dialect grammar alone: twice a
/// definition-grounded rule's, so knowing the tool always wins a tie.
pub const SYNTAX_COST_MULTIPLIER: u64 = 2;

/// The cost multiplier of `grounding`.
///
/// ```
/// use purrassist_core::grammar::edit::Grounding;
/// use purrassist_core::tool_call::rules::multiplier;
/// assert!(multiplier(Grounding::Definition) < multiplier(Grounding::Syntax));
/// ```
#[must_use]
pub const fn multiplier(grounding: Grounding) -> u64 {
    match grounding {
        Grounding::Definition => DEFINITION_COST_MULTIPLIER,
        Grounding::Syntax => SYNTAX_COST_MULTIPLIER,
    }
}

/// Every policy: what a single-reading rule is admitted under.
const EVERY_POLICY: &[RepairPolicy] = &[RepairPolicy::Structural, RepairPolicy::Tolerant];
/// Only [`RepairPolicy::Tolerant`]: what a multiple-reading rule is
/// admitted under.
const TOLERANT_ONLY: &[RepairPolicy] = &[RepairPolicy::Tolerant];

/// One named rule of tool-call recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Stable rule identifier recorded as evidence; unique in [`RULES`].
    pub id: &'static str,
    /// Ranking cost per application: base cost times the grounding's
    /// [`multiplier`] (the minimum, for rules costed by the names they
    /// compare).
    pub cost: u64,
    /// Whether an application is a guess (recorded as a guess inference).
    pub guess: bool,
    /// Whether other readings of the same input remain possible.
    pub readings: Readings,
    /// What the rule's decision rests on.
    pub grounding: Grounding,
    /// The kind of loss an application records, if it discards bytes.
    pub loss: Option<LossKind>,
    /// The repair policies that admit the rule: every policy for a
    /// single-reading rule, only `Tolerant` otherwise.
    pub policies: &'static [RepairPolicy],
    /// The grammar edit operator this rule is, for engine and codec rules.
    pub operator: Option<EditOperator>,
}

impl Rule {
    /// Whether `policy` admits this rule.
    ///
    /// ```
    /// use purrassist_core::tool_call::{RepairPolicy, rules};
    /// let stray = rules::rule("tool_call.markup.stray-closer").unwrap();
    /// assert!(stray.admits(RepairPolicy::Structural) && stray.admits(RepairPolicy::Tolerant));
    /// ```
    #[must_use]
    pub fn admits(&self, policy: RepairPolicy) -> bool {
        self.policies.contains(&policy)
    }
}

const fn policies(readings: Readings) -> &'static [RepairPolicy] {
    match readings {
        Readings::Single => EVERY_POLICY,
        Readings::Multiple => TOLERANT_ONLY,
    }
}

/// The rule of one grammar edit operator.
const fn from_operator(operator: &OperatorRule) -> Rule {
    Rule {
        id: operator.id,
        cost: operator.cost * multiplier(operator.grounding),
        guess: operator.guess,
        readings: operator.readings,
        grounding: operator.grounding,
        loss: operator.loss,
        policies: policies(operator.readings),
        operator: Some(operator.operator),
    }
}

/// A resolution rule from its table columns; guesses admit other readings.
const fn resolution(
    id: &'static str,
    base: u64,
    guess: bool,
    grounding: Grounding,
    loss: Option<LossKind>,
) -> Rule {
    let readings = if guess {
        Readings::Multiple
    } else {
        Readings::Single
    };
    Rule {
        id,
        cost: base * multiplier(grounding),
        guess,
        readings,
        grounding,
        loss,
        policies: policies(readings),
        operator: None,
    }
}

const RESOLUTION: [Rule; 15] = [
    resolution(
        "tool_call.name.alias",
        0,
        false,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.name.match",
        FOLDED_NAME_COST,
        true,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.parameter.alias",
        0,
        false,
        Grounding::Definition,
        None,
    ),
    resolution(
        "json.key.match",
        FOLDED_NAME_COST,
        true,
        Grounding::Definition,
        None,
    ),
    resolution(
        "json.key.relocate",
        KEY_RELOCATE_COST,
        true,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.parameter.drop-unknown",
        KEY_REMOVE_COST,
        false,
        Grounding::Definition,
        Some(LossKind::Data),
    ),
    resolution(
        "tool_call.parameter.pass-through",
        0,
        false,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.parameter.repeated",
        0,
        true,
        Grounding::Definition,
        Some(LossKind::Data),
    ),
    resolution(
        "tool_call.parameter.collect",
        0,
        false,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.value.enum-normalize",
        FOLDED_NAME_COST,
        false,
        Grounding::Definition,
        None,
    ),
    resolution(
        "json.enum.match",
        FOLDED_NAME_COST,
        true,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.value.decode-typed",
        0,
        false,
        Grounding::Definition,
        None,
    ),
    resolution(
        "tool_call.truncated",
        0,
        false,
        Grounding::Syntax,
        Some(LossKind::Data),
    ),
    resolution(
        "tool_call.markup.dangling-opener",
        0,
        false,
        Grounding::Syntax,
        Some(LossKind::Data),
    ),
    resolution("tool_call.id.dedup", 0, false, Grounding::Syntax, None),
];

/// The number of rules in [`RULES`].
pub const RULE_COUNT: usize = OPERATOR_COUNT + RESOLUTION.len();

const RULE_TABLE: [Rule; RULE_COUNT] = {
    let mut table = [RESOLUTION[0]; RULE_COUNT];
    let mut index = 0;
    while index < OPERATOR_COUNT {
        table[index] = from_operator(&OPERATOR_TABLE[index]);
        index += 1;
    }
    let mut extra = 0;
    while extra < RESOLUTION.len() {
        table[OPERATOR_COUNT + extra] = RESOLUTION[extra];
        extra += 1;
    }
    table
};

/// Every tool-call rule: the grammar edit operators, in
/// [`EditOperator`] order, then the resolution rules.
pub static RULES: &[Rule] = &RULE_TABLE;

/// Schema-recovery rules that tool-call recovery never applies.
///
/// A missing required argument is never invented, a scalar is never
/// stringified for a string parameter, and a JSON string is never decoded
/// into a value.
pub static DISABLED: &[&str] = &[
    "json.required.invent",
    "json.type.stringify-scalar",
    "json.type.decode-string",
];

/// The rule with id `id`, if any.
///
/// ```
/// use purrassist_core::tool_call::rules;
/// assert!(rules::rule("tool_call.markup.closers-only-empty").is_some());
/// assert!(rules::rule("json.type.decode-string").is_none());
/// ```
#[must_use]
pub fn rule(id: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.id == id)
}

/// The rule of a grammar edit operator.
///
/// ```
/// use purrassist_core::grammar::edit::EditOperator;
/// use purrassist_core::tool_call::rules;
/// let rule = rules::of_operator(EditOperator::CloserVariant);
/// assert_eq!(rule.id, EditOperator::CloserVariant.rule().id);
/// assert_eq!(rule.cost, 2); // base 1, syntax-grounded
/// ```
#[must_use]
pub fn of_operator(operator: EditOperator) -> &'static Rule {
    &RULES[operator as usize]
}

/// The resolution rule with id `id`; a registry invariant.
pub(crate) fn resolution_rule(id: &str) -> &'static Rule {
    RULES[OPERATOR_COUNT..]
        .iter()
        .find(|rule| rule.id == id)
        .unwrap_or_else(|| unreachable!("`{id}` is a registered resolution rule"))
}

#[cfg(test)]
mod tests {
    use super::{OPERATOR_COUNT, RULES, of_operator, resolution_rule};
    use crate::grammar::edit::OPERATORS;

    #[test]
    fn operators_come_first_in_order() {
        for (index, operator) in OPERATORS.iter().enumerate() {
            assert_eq!(RULES[index].operator, Some(operator.operator));
            assert_eq!(of_operator(operator.operator).id, operator.id);
        }
        for rule in &RULES[OPERATOR_COUNT..] {
            assert_eq!(rule.operator, None);
            assert_eq!(resolution_rule(rule.id), rule);
        }
    }
}
