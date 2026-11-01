// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Schema evaluation of one instance.
//!
//! Recursion keys and transient paths/lookup keys retain fallible owners and
//! admit copying, comparison, lookup and relocation through the caller governor.
//! Active keys and dynamic scopes unwind on every result; recursion obeys
//! caller Depth. Integer tests, bound comparisons and
//! transient report records also use admitted operations. Reports and annotations
//! remain owned through merging, branch rejection and internal consumption.
//! Public validation and recovery receipts retain transferred output storage;
//! discarded internal records release it. General assertion computation keeps
//! its separate contract.
use super::layout::Dependency;
use super::report::{Owned, Records};
use super::{Schema, Violation};
use super::{index, reference, report};
use crate::contract::{
    Evidence, Governor, Resource, ScratchString, ScratchVec, StopReason, Validation,
    format_output_governed, format_scratch_governed,
};
use crate::json::{Member, Node, Value};

/// Everything [`super::evaluate::run`] collects while walking one instance
/// against one schema: hard assertion failures, plus the always-on
/// `format` annotation evidence and the pattern-engine wiring
/// check both require, regardless of which branch's assertions passed.
pub(super) struct EvalResult {
    pub violations: Records<Violation>,
    pub format_evidence: Records<Validation>,
    pub pattern_evidence: ScratchVec<Owned<Evidence>>,
}

impl EvalResult {
    pub(super) fn publish(
        self,
        schema: &Schema,
        governor: &mut Governor,
    ) -> Result<super::ValidationReport, StopReason> {
        let valid = self.violations.is_empty();
        let mut patterns = Records::new(governor);
        // Reserve before draining; any refusal drops every temporary owner.
        patterns.values.reserve(self.pattern_evidence.len())?;
        let (entries, entries_lease) = self.pattern_evidence.into_owned_parts();
        for entry in entries {
            patterns.push(entry, governor)?;
        }
        drop(entries_lease);
        governor.check()?;
        Ok(super::ValidationReport {
            schema: schema.digest(),
            dialect: schema.dialect(),
            valid,
            violations: self.violations.publish(),
            format_evidence: self.format_evidence.publish(),
            pattern_evidence: patterns.publish(),
        })
    }
}

struct Evaluation {
    errors: Records<Violation>,
    /// Member names evaluated at this instance scope (may repeat; sorted
    /// before lookup).
    properties: ScratchVec<ScratchString>,
    /// Array positions evaluated at this instance scope (may repeat).
    items: ScratchVec<usize>,
    /// Format annotation results, always recorded regardless
    /// of whether the enclosing branch's assertions passed.
    format_evidence: Records<Validation>,
    /// Pattern-engine wiring: `(location id, "pattern" | "patternProperties/<name>")`
    /// for every compiled pattern this evaluation actually exercised, each
    /// once.
    pattern_ids: ScratchVec<(usize, ScratchString)>,
}
impl Evaluation {
    fn new(governor: &Governor) -> Self {
        Self {
            errors: Records::new(governor),
            properties: ScratchVec::new(governor),
            items: ScratchVec::new(governor),
            format_evidence: Records::new(governor),
            pattern_ids: ScratchVec::new(governor),
        }
    }
    /// Merge a schema evaluated against this same instance scope.
    fn merge(&mut self, mut other: Self, governor: &mut Governor) -> Result<(), StopReason> {
        if other.errors.is_empty() {
            report::append(&mut self.properties, &mut other.properties, governor)?;
            report::append(&mut self.items, &mut other.items, governor)?;
        }
        self.merge_child(other, governor)
    }
    fn merge_child(&mut self, mut other: Self, governor: &mut Governor) -> Result<(), StopReason> {
        self.merge_evidence(&mut other, governor)?;
        self.errors.append(&mut other.errors, governor)
    }
    fn merge_evidence(
        &mut self,
        other: &mut Self,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        self.format_evidence
            .append(&mut other.format_evidence, governor)?;
        while let Some(entry) = other.pattern_ids.pop() {
            if !self.has_pattern(entry.0, &entry.1, governor)? {
                index::push(&mut self.pattern_ids, entry, governor)?;
            }
        }
        Ok(())
    }
    fn has_pattern(
        &self,
        id: usize,
        key: &str,
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        for (known, name) in &self.pattern_ids {
            reference::work(governor, 1)?;
            if *known == id {
                reference::work(governor, 1)?;
                if name.len() == key.len() {
                    reference::work(governor, key.len())?;
                    if name.as_str() == key {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
    fn pattern(&mut self, id: usize, key: &str, governor: &mut Governor) -> Result<(), StopReason> {
        if !self.has_pattern(id, key, governor)? {
            let name = reference::copy(key, governor)?;
            index::push(&mut self.pattern_ids, (id, name), governor)?;
        }
        Ok(())
    }
    fn property(&mut self, key: &str, governor: &mut Governor) -> Result<(), StopReason> {
        let key = reference::copy(key, governor)?;
        index::push(&mut self.properties, key, governor)
    }
    fn valid(&self) -> bool {
        self.errors.is_empty()
    }
}

impl crate::ScratchCharge for Option<Evaluation> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 256);
        256
    };
}

struct Evaluator<'s, 'g> {
    schema: &'s Schema,
    governor: &'g mut Governor,
    /// The `(location, instance path)` pairs being evaluated: the current
    /// recursion, so a reference that recurs without instance progress is
    /// detected.
    active: ScratchVec<(usize, ScratchString)>,
    /// The dynamic scope: the locations whose base URIs were entered.
    scope: ScratchVec<usize>,
}

pub(super) fn run(
    schema: &Schema,
    id: usize,
    instance: &Node,
    governor: &mut Governor,
) -> Result<EvalResult, StopReason> {
    let mut evaluator = Evaluator {
        schema,
        active: ScratchVec::new(governor),
        scope: ScratchVec::new(governor),
        governor,
    };
    let evaluation = evaluator.eval(id, instance, "", 0)?;
    drop(evaluator);
    let Evaluation {
        errors,
        format_evidence,
        mut pattern_ids,
        ..
    } = evaluation;
    // Preserve the established location/key ordering of pattern observations.
    report::sort_by(
        pattern_ids.as_mut_slice(),
        governor,
        |left, right, governor| {
            reference::work(governor, 1)?;
            let order = left.0.cmp(&right.0);
            if order.is_eq() {
                report::compare(&left.1, &right.1, governor)
            } else {
                Ok(order)
            }
        },
    )?;
    let mut pattern_evidence = ScratchVec::new(governor);
    for (location_id, keyword) in pattern_ids.as_slice() {
        governor.charge(Resource::EvidenceEntries, 1)?;
        let location = &schema.index.locations[*location_id];
        let pattern = location
            .patterns
            .get_governed(keyword.as_str(), governor)?
            .expect("INVARIANT: evaluated pattern is compiled");
        let (rule, explanation) = pattern.engine_metadata();
        let evidence = Owned::evidence(
            reference::copy(rule, governor)?,
            reference::copy(explanation, governor)?,
            format_scratch_governed(
                format_args!(
                    "{}#{}/{}",
                    location.base.as_str(),
                    location.pointer.as_str(),
                    keyword.as_str()
                ),
                governor,
            )?,
        );
        index::push(&mut pattern_evidence, evidence, governor)?;
    }
    governor.check()?;
    Ok(EvalResult {
        violations: errors,
        format_evidence,
        pattern_evidence,
    })
}

impl Evaluator<'_, '_> {
    /// Resolve the compiled child before entering its evaluation frame.
    fn eval_child(
        &mut self,
        id: usize,
        key: &str,
        instance: &Node,
        path: &str,
        depth: u64,
    ) -> Result<Evaluation, StopReason> {
        let child = index::child(self.schema, id, key, self.governor)?;
        self.eval(child, instance, path, depth)
    }
    fn violation(
        &mut self,
        result: &mut Evaluation,
        id: usize,
        path: &str,
        keyword: &str,
        message: core::fmt::Arguments<'_>,
    ) -> Result<(), StopReason> {
        self.governor.charge(Resource::EvidenceEntries, 1)?;
        let location = &self.schema.index.locations[id];
        let message = Owned::string(format_scratch_governed(message, self.governor)?);
        let instance_path = Owned::string(reference::copy(path, self.governor)?);
        let keyword_text = Owned::string(reference::copy(keyword, self.governor)?);
        let prefix = format_scratch_governed(
            format_args!("{}#{}", location.base.as_str(), location.pointer.as_str()),
            self.governor,
        )?;
        let schema_path = Owned::string(index::pointer(&prefix, keyword, self.governor)?);
        let mut lease = message.lease;
        lease.absorb(instance_path.lease);
        lease.absorb(keyword_text.lease);
        lease.absorb(schema_path.lease);
        result.errors.push(
            Owned {
                value: Violation {
                    instance_path: instance_path.value,
                    schema_path: schema_path.value,
                    keyword: keyword_text.value,
                    message: message.value,
                },
                lease,
            },
            self.governor,
        )
    }

    fn eval(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u64,
    ) -> Result<Evaluation, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        self.governor.charge(Resource::Depth, depth)?;
        for (known_id, known_path) in &self.active {
            reference::work(self.governor, 1)?;
            if *known_id == id {
                reference::work(self.governor, 1)?;
                // Deeper instance paths cannot match; avoid repeated prefix scans.
                if known_path.len() != path.len() {
                    continue;
                }
                reference::work(self.governor, path.len())?;
                if known_path.as_str() == path {
                    return Err(StopReason::InvalidInput(format_output_governed(
                        format_args!("schema reference recurs without instance progress"),
                        self.governor,
                    )?));
                }
            }
        }
        let key = reference::copy(path, self.governor)?;
        index::push(&mut self.active, (id, key), self.governor)?;
        let entered = super::index::enter_scope(self.schema, id, &mut self.scope, self.governor);
        let result = match entered {
            Ok(true) => {
                let result = self.evaluate_body(id, instance, path, depth);
                self.scope.pop();
                result
            }
            Ok(false) => self.evaluate_body(id, instance, path, depth),
            Err(reason) => Err(reason),
        };
        self.active.pop();
        result
    }
    fn evaluate_body(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u64,
    ) -> Result<Evaluation, StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        let mut result = Evaluation::new(self.governor);
        if let Value::Bool(accept) = body.value {
            if !accept {
                self.violation(
                    &mut result,
                    id,
                    path,
                    "false",
                    format_args!("boolean schema rejects every instance"),
                )?;
            }
            return Ok(result);
        }
        self.references(id, instance, path, depth, &mut result)?;
        self.basic(id, instance, path, &mut result)?;
        self.combinators(id, instance, path, depth, &mut result)?;
        match &instance.value {
            Value::Object(members) => {
                self.object(id, instance, members, path, depth, &mut result)?;
            }
            Value::Array(items) => self.array(id, items, path, depth, &mut result)?,
            _ => {}
        }
        self.unevaluated(id, instance, path, depth, &mut result)?;
        Ok(result)
    }
    fn references(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u64,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        for keyword in ["$ref", "$dynamicRef"] {
            if let Some(reference) = body
                .get_governed(keyword, self.governor)?
                .and_then(Node::as_str)
            {
                let base = &schema.index.locations[id].base;
                let mut target = super::reference::resolve(schema, base, reference, self.governor)?;
                if keyword == "$dynamicRef" {
                    target = super::reference::dynamic(
                        schema,
                        target,
                        reference,
                        self.scope
                            .as_slice()
                            .iter()
                            .map(|&id| schema.index.locations[id].base.as_str()),
                        self.governor,
                    )?;
                }
                result.merge(self.eval(target, instance, path, depth + 1)?, self.governor)?;
            }
        }
        Ok(())
    }
    fn basic(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        if let Some(expected) = body.get_governed("type", self.governor)? {
            let valid = match &expected.value {
                Value::String(kind) => matches_type(instance, kind, self.governor)?,
                Value::Array(kinds) => {
                    let mut matched = false;
                    for kind in kinds.iter().filter_map(Node::as_str) {
                        if matches_type(instance, kind, self.governor)? {
                            matched = true;
                            break;
                        }
                    }
                    matched
                }
                _ => false,
            };
            if !valid {
                let expected = expected.to_json_scratch(self.governor)?;
                self.violation(
                    result,
                    id,
                    path,
                    "type",
                    format_args!("expected {}", expected.as_str()),
                )?;
            }
        }
        if let Some(expected) = body.get_governed("const", self.governor)?
            && !instance.equivalent_governed(expected, self.governor)?
        {
            self.violation(
                result,
                id,
                path,
                "const",
                format_args!("instance differs from const"),
            )?;
        }
        if let Some(Node {
            value: Value::Array(values),
            ..
        }) = body.get_governed("enum", self.governor)?
        {
            self.governor.charge(
                Resource::Work,
                u64::try_from(values.len()).unwrap_or(u64::MAX),
            )?;
            if !crate::json::equivalent_any(values, instance, self.governor)? {
                self.violation(
                    result,
                    id,
                    path,
                    "enum",
                    format_args!("instance is not an enum member"),
                )?;
            }
        }
        self.scalar(id, instance, path, result)
    }
    fn scalar(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        match &instance.value {
            Value::Number(number) => {
                for keyword in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
                    if let Some(Node {
                        value: Value::Number(bound),
                        ..
                    }) = body.get_governed(keyword, self.governor)?
                    {
                        let comparison = number.compare_governed(bound, self.governor)?;
                        let valid = match keyword {
                            "minimum" => !comparison.is_lt(),
                            "maximum" => !comparison.is_gt(),
                            "exclusiveMinimum" => comparison.is_gt(),
                            _ => comparison.is_lt(),
                        };
                        if !valid {
                            self.violation(
                                result,
                                id,
                                path,
                                keyword,
                                format_args!("{number} violates bound {bound}"),
                            )?;
                        }
                    }
                }
                if let Some(Node {
                    value: Value::Number(divisor),
                    ..
                }) = body.get_governed("multipleOf", self.governor)?
                    && !number.is_multiple_of(divisor, self.governor)?
                {
                    self.violation(
                        result,
                        id,
                        path,
                        "multipleOf",
                        format_args!("{number} is not an exact multiple of {divisor}"),
                    )?;
                }
            }
            Value::String(text) => {
                self.bounds(
                    id,
                    path,
                    |governor| {
                        reference::work(governor, text.len())?;
                        Ok(text.chars().count())
                    },
                    "minLength",
                    "maxLength",
                    result,
                )?;
                if let Some(pattern) = schema.index.locations[id]
                    .patterns
                    .get_governed("pattern", self.governor)?
                {
                    result.pattern(id, "pattern", self.governor)?;
                    if !pattern.is_match(text, self.governor)? {
                        self.violation(
                            result,
                            id,
                            path,
                            "pattern",
                            format_args!("string does not match the pattern"),
                        )?;
                    }
                }
                if let Some(name) = body
                    .get_governed("format", self.governor)?
                    .and_then(Node::as_str)
                {
                    self.format(id, path, name, text, result)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// `format` is always evaluated and always reported as
    /// annotation evidence (`schema.format.<name>-v1`, pass/fail, per
    /// instance location); it becomes an assertion `Violation` only when
    /// this location's dialect declared the format-assertion vocabulary
    /// (`Vocabularies::format_assert`), never from a caller switch.
    fn format(
        &mut self,
        id: usize,
        path: &str,
        name: &str,
        text: &str,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let outcome = super::format::evaluate(name, text, self.governor)?;
        self.governor.charge(Resource::EvidenceEntries, 1)?;
        let passed = !matches!(outcome, super::format::Outcome::Fail);
        let validator = if matches!(outcome, super::format::Outcome::Unknown) {
            reference::copy("schema.format.unknown-v1", self.governor)?
        } else {
            format_scratch_governed(format_args!("schema.format.{name}-v1"), self.governor)?
        };
        let mut diagnostics = Records::new(self.governor);
        let diagnostic = match outcome {
            super::format::Outcome::Pass => None,
            super::format::Outcome::Fail => Some(format_scratch_governed(
                format_args!("{text:?} does not satisfy format {name:?}"),
                self.governor,
            )?),
            super::format::Outcome::Unknown => Some(format_scratch_governed(
                format_args!("format {name:?} is not evaluated"),
                self.governor,
            )?),
        };
        if let Some(line) = diagnostic {
            diagnostics.push(Owned::string(line), self.governor)?;
        }
        let constraint = reference::copy(path, self.governor)?;
        let assert = !passed && self.schema.index.locations[id].vocabularies.format_assert;
        result.format_evidence.push(
            Owned::validation(validator, constraint, passed, diagnostics),
            self.governor,
        )?;
        if assert {
            self.violation(
                result,
                id,
                path,
                "format",
                format_args!("does not satisfy format {name:?}"),
            )?;
        }
        Ok(())
    }
    fn bounds(
        &mut self,
        id: usize,
        path: &str,
        mut measure: impl FnMut(&mut Governor) -> Result<usize, StopReason>,
        minimum: &str,
        maximum: &str,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        let mut measured = None;
        for (keyword, is_minimum) in [(minimum, true), (maximum, false)] {
            if let Some(Node {
                value: Value::Number(bound),
                ..
            }) = body.get_governed(keyword, self.governor)?
            {
                // Count only when an applicable bound needs it, at most once.
                let count = if let Some(count) = measured {
                    count
                } else {
                    let count = measure(self.governor)?;
                    measured = Some(count);
                    count
                };
                let comparison = bound.compare_count_governed(count, self.governor)?;
                if (is_minimum && comparison.is_gt()) || (!is_minimum && comparison.is_lt()) {
                    self.violation(
                        result,
                        id,
                        path,
                        keyword,
                        format_args!("count {count} violates bound {bound}"),
                    )?;
                }
            }
        }
        Ok(())
    }
    fn combinators(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u64,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(Node {
                value: Value::Array(children),
                ..
            }) = body.get_governed(keyword, self.governor)?
            {
                // Every branch's evaluation is held until the keyword's
                // outcome is known.
                let mut branches = ScratchVec::new(self.governor);
                for position in 0..children.len() {
                    let child = {
                        let key = format_scratch_governed(
                            format_args!("{keyword}/{position}"),
                            self.governor,
                        )?;
                        index::child(schema, id, &key, self.governor)?
                    };
                    let branch = self.eval(child, instance, path, depth + 1)?;
                    index::push(&mut branches, Some(branch), self.governor)?;
                }
                reference::work(self.governor, branches.len())?;
                let passed = branches
                    .iter()
                    .flatten()
                    .filter(|branch| branch.valid())
                    .count();
                let valid = match keyword {
                    "allOf" => passed == branches.len(),
                    "anyOf" => passed > 0,
                    _ => passed == 1,
                };
                if !valid {
                    self.violation(
                        result,
                        id,
                        path,
                        keyword,
                        format_args!("{passed} of {} branches passed", children.len()),
                    )?;
                }
                // The branches whose annotations count come first: the
                // passed ones when the keyword holds, else the failed ones
                // (whose errors explain the failure); each in order.
                reference::work(self.governor, branches.len())?;
                for slot in branches.as_mut_slice() {
                    if slot.as_ref().is_some_and(|branch| branch.valid() == valid)
                        && let Some(branch) = slot.take()
                    {
                        if valid {
                            result.merge(branch, self.governor)?;
                        } else {
                            result.merge_child(branch, self.governor)?;
                        }
                    }
                }
                reference::work(self.governor, branches.len())?;
                for slot in branches.as_mut_slice() {
                    if let Some(mut branch) = slot.take() {
                        result.merge_evidence(&mut branch, self.governor)?;
                    }
                }
            }
        }
        if body.get_governed("not", self.governor)?.is_some() {
            let mut branch = self.eval_child(id, "not", instance, path, depth + 1)?;
            if branch.valid() {
                self.violation(
                    result,
                    id,
                    path,
                    "not",
                    format_args!("negated schema matched"),
                )?;
            }
            result.merge_evidence(&mut branch, self.governor)?;
        }
        if body.get_governed("if", self.governor)?.is_some() {
            let mut condition = self.eval_child(id, "if", instance, path, depth + 1)?;
            let branch = if condition.valid() {
                result.merge(condition, self.governor)?;
                "then"
            } else {
                result.merge_evidence(&mut condition, self.governor)?;
                "else"
            };
            if body.get_governed(branch, self.governor)?.is_some() {
                result.merge(
                    self.eval_child(id, branch, instance, path, depth + 1)?,
                    self.governor,
                )?;
            }
        }
        Ok(())
    }
    fn object(
        &mut self,
        id: usize,
        instance: &Node,
        members: &[Member],
        path: &str,
        depth: u64,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        self.bounds(
            id,
            path,
            |_| Ok(members.len()),
            "minProperties",
            "maxProperties",
            result,
        )?;
        let mut names = ScratchVec::new(self.governor);
        for member in members {
            index::push(&mut names, member.key.as_str(), self.governor)?;
        }
        report::sort_by(
            names.as_mut_slice(),
            self.governor,
            |left, right, governor| report::compare(left, right, governor),
        )?;
        let has = |key: &str, governor: &mut Governor| -> Result<bool, StopReason> {
            Ok(
                report::search_by(names.as_slice(), governor, |known, governor| {
                    report::compare(known, key, governor)
                })?
                .is_ok(),
            )
        };
        let mut duplicate = false;
        for pair in names.windows(2) {
            if report::compare(pair[0], pair[1], self.governor)?.is_eq() {
                duplicate = true;
                break;
            }
        }
        if duplicate {
            self.violation(
                result,
                id,
                path,
                "duplicate-members",
                format_args!("object has ambiguous duplicate member occurrences"),
            )?;
        }
        if let Some(Node {
            value: Value::Array(required),
            ..
        }) = body.get_governed("required", self.governor)?
        {
            for key in required.iter().filter_map(Node::as_str) {
                if !has(key, self.governor)? {
                    self.violation(
                        result,
                        id,
                        path,
                        "required",
                        format_args!("missing required member {key:?}"),
                    )?;
                }
            }
        }
        for dependency in schema.dependencies(id, self.governor)?.iter().copied() {
            match dependency {
                Dependency::Required {
                    keyword,
                    trigger,
                    names: required,
                } if has(trigger, self.governor)? => {
                    for key in required.iter().filter_map(Node::as_str) {
                        if !has(key, self.governor)? {
                            self.violation(
                                result,
                                id,
                                path,
                                keyword,
                                format_args!("{trigger} requires member {key:?}"),
                            )?;
                        }
                    }
                }
                Dependency::Schema { trigger, child } if has(trigger, self.governor)? => {
                    result.merge(self.eval(child, instance, path, depth + 1)?, self.governor)?;
                }
                _ => {}
            }
        }
        for member in members {
            self.member(id, member, path, depth, result)?;
        }
        Ok(())
    }
    fn member(
        &mut self,
        id: usize,
        member: &Member,
        path: &str,
        depth: u64,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        let mut matched = false;
        let child_path = index::pointer(path, &member.key, self.governor)?;
        if body.get_governed("propertyNames", self.governor)?.is_some() {
            // The name as a string instance, and its instance path.
            let name = reference::copy(&member.key, self.governor)?;
            let (name, _name_lease) = name.into_owned_parts();
            let name = Node::new(Value::String(name));
            let name_path = index::pointer(&child_path, "@name", self.governor)?;
            result.merge_child(
                self.eval_child(id, "propertyNames", &name, &name_path, depth + 1)?,
                self.governor,
            )?;
        }
        if let Some(properties) = body.get_governed("properties", self.governor)?
            && super::compile::member(properties, &member.key, self.governor)?.is_some()
        {
            matched = true;
            let child = {
                let key = index::pointer("properties", &member.key, self.governor)?;
                index::child(schema, id, &key, self.governor)?
            };
            let branch = self.eval(child, &member.value, &child_path, depth + 1)?;
            result.merge_child(branch, self.governor)?;
        }
        if let Some(Node {
            value: Value::Object(patterns),
            ..
        }) = body.get_governed("patternProperties", self.governor)?
        {
            for pattern in patterns {
                let name = index::pointer("patternProperties", &pattern.key, self.governor)?;
                result.pattern(id, &name, self.governor)?;
                if schema.index.locations[id]
                    .patterns
                    .get_governed(name.as_str(), self.governor)?
                    .expect("compiled pattern exists")
                    .is_match(&member.key, self.governor)?
                {
                    matched = true;
                    let branch =
                        self.eval_child(id, &name, &member.value, &child_path, depth + 1)?;
                    result.merge_child(branch, self.governor)?;
                }
            }
        }
        if !matched
            && body
                .get_governed("additionalProperties", self.governor)?
                .is_some()
        {
            matched = true;
            let branch = self.eval_child(
                id,
                "additionalProperties",
                &member.value,
                &child_path,
                depth + 1,
            )?;
            result.merge_child(branch, self.governor)?;
        }
        if matched {
            result.property(&member.key, self.governor)?;
        }
        Ok(())
    }
    fn array(
        &mut self,
        id: usize,
        items: &[Node],
        path: &str,
        depth: u64,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        self.bounds(
            id,
            path,
            |_| Ok(items.len()),
            "minItems",
            "maxItems",
            result,
        )?;
        if body
            .get_governed("uniqueItems", self.governor)?
            .is_some_and(|node| node.value == Value::Bool(true))
        {
            for (position, item) in items.iter().enumerate() {
                for previous in &items[..position] {
                    self.governor.charge(Resource::Work, 1)?;
                    if previous.equivalent_governed(item, self.governor)? {
                        self.violation(
                            result,
                            id,
                            path,
                            "uniqueItems",
                            format_args!("duplicate array value at index {position}"),
                        )?;
                        break;
                    }
                }
            }
        }
        let layout = schema.item_layout(id, self.governor)?;
        for (position, item) in items.iter().enumerate() {
            let child = layout.child(schema, id, position, self.governor)?;
            if let Some(child) = child {
                let item_path =
                    format_scratch_governed(format_args!("{path}/{position}"), self.governor)?;
                let branch = self.eval(child, item, &item_path, depth + 1)?;
                result.merge_child(branch, self.governor)?;
                index::push(&mut result.items, position, self.governor)?;
            }
        }
        if body.get_governed("contains", self.governor)?.is_some() {
            let mut matched: ScratchVec<usize> = ScratchVec::new(self.governor);
            for (position, item) in items.iter().enumerate() {
                let item_path =
                    format_scratch_governed(format_args!("{path}/{position}"), self.governor)?;
                let mut branch = self.eval_child(id, "contains", item, &item_path, depth + 1)?;
                if branch.valid() {
                    index::push(&mut matched, position, self.governor)?;
                }
                result.merge_evidence(&mut branch, self.governor)?;
            }
            if body.get_governed("minContains", self.governor)?.is_none() && matched.is_empty() {
                self.violation(
                    result,
                    id,
                    path,
                    "contains",
                    format_args!("no array member satisfies contains"),
                )?;
            }
            self.bounds(
                id,
                path,
                |_| Ok(matched.len()),
                "minContains",
                "maxContains",
                result,
            )?;
            report::append(&mut result.items, &mut matched, self.governor)?;
        }
        Ok(())
    }
    fn unevaluated(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u64,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        match &instance.value {
            Value::Object(members)
                if body
                    .get_governed("unevaluatedProperties", self.governor)?
                    .is_some() =>
            {
                let pending = self.unevaluated_members(members, result)?;
                for &index in pending.as_slice() {
                    let member = &members[index];
                    let member_path = index::pointer(path, &member.key, self.governor)?;
                    let branch = self.eval_child(
                        id,
                        "unevaluatedProperties",
                        &member.value,
                        &member_path,
                        depth + 1,
                    )?;
                    result.merge_child(branch, self.governor)?;
                }
                for &index in pending.as_slice() {
                    result.property(&members[index].key, self.governor)?;
                }
            }
            Value::Array(items)
                if body
                    .get_governed("unevaluatedItems", self.governor)?
                    .is_some() =>
            {
                report::sort_by(
                    result.items.as_mut_slice(),
                    self.governor,
                    |left, right, governor| {
                        reference::work(governor, 1)?;
                        Ok(left.cmp(right))
                    },
                )?;
                let mut pending: ScratchVec<usize> = ScratchVec::new(self.governor);
                for position in 0..items.len() {
                    self.governor.charge(Resource::Work, 1)?;
                    if report::search_by(
                        result.items.as_slice(),
                        self.governor,
                        |known, governor| {
                            reference::work(governor, 1)?;
                            Ok(known.cmp(&position))
                        },
                    )?
                    .is_err()
                    {
                        index::push(&mut pending, position, self.governor)?;
                    }
                }
                for &position in pending.as_slice() {
                    let item_path =
                        format_scratch_governed(format_args!("{path}/{position}"), self.governor)?;
                    let branch = self.eval_child(
                        id,
                        "unevaluatedItems",
                        &items[position],
                        &item_path,
                        depth + 1,
                    )?;
                    result.merge_child(branch, self.governor)?;
                }
                report::append(&mut result.items, &mut pending, self.governor)?;
            }
            _ => {}
        }
        Ok(())
    }
    /// The indices of the members `unevaluatedProperties` applies to, in
    /// member order: each member whose name no evaluation marked, the first
    /// occurrence of a repeated name only.
    fn unevaluated_members(
        &mut self,
        members: &[Member],
        result: &mut Evaluation,
    ) -> Result<ScratchVec<usize>, StopReason> {
        report::sort_by(
            result.properties.as_mut_slice(),
            self.governor,
            |left, right, governor| report::compare(left, right, governor),
        )?;
        let mut pending: ScratchVec<usize> = ScratchVec::new(self.governor);
        for (index, member) in members.iter().enumerate() {
            self.governor.charge(Resource::Work, 1)?;
            if report::search_by(
                result.properties.as_slice(),
                self.governor,
                |key, governor| report::compare(key, &member.key, governor),
            )?
            .is_err()
            {
                index::push(&mut pending, index, self.governor)?;
            }
        }
        // Keep the first member of each name, then restore member order.
        report::sort_by(
            pending.as_mut_slice(),
            self.governor,
            |&left, &right, governor| {
                let order = report::compare(&members[left].key, &members[right].key, governor)?;
                reference::work(governor, 1)?;
                Ok(order.then(left.cmp(&right)))
            },
        )?;
        let mut first: ScratchVec<usize> = ScratchVec::new(self.governor);
        let mut previous: Option<&str> = None;
        for &index in pending.as_slice() {
            let key = members[index].key.as_str();
            if previous.is_none()
                || !report::compare(previous.expect("previous name exists"), key, self.governor)?
                    .is_eq()
            {
                index::push(&mut first, index, self.governor)?;
            }
            previous = Some(key);
        }
        report::sort_by(
            first.as_mut_slice(),
            self.governor,
            |left, right, governor| {
                reference::work(governor, 1)?;
                Ok(left.cmp(right))
            },
        )?;
        Ok(first)
    }
}
fn matches_type(instance: &Node, kind: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    Ok(match (kind, &instance.value) {
        ("null", Value::Null)
        | ("boolean", Value::Bool(_))
        | ("object", Value::Object(_))
        | ("array", Value::Array(_))
        | ("number", Value::Number(_))
        | ("string", Value::String(_)) => true,
        ("integer", Value::Number(number)) => !matches!(
            number.integer_magnitude_governed(governor)?,
            crate::json::IntegerReading::Fractional
        ),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::{Evaluator, run};
    use crate::{
        Budget, Governor, ScratchVec, StopReason, contract::Resource, json, schema::Schema,
    };

    #[test]
    fn string_length_scan_requires_work_before_counting() {
        let schema = Schema::compile(r#"{"minLength":1,"maxLength":4096}"#).unwrap();
        let instance = json::Node::new(json::Value::String("é".repeat(4096)));
        let mut governor = Governor::new(Budget::default());
        let result = run(&schema, schema.root, &instance, &mut governor).unwrap();
        assert!(result.violations.is_empty());
        drop(result);
        assert_eq!(governor.scratch_meter().live(), 0);
        let mut refused = Governor::new(Budget {
            work: 4096,
            ..Budget::default()
        });
        assert!(matches!(
            run(&schema, schema.root, &instance, &mut refused),
            Err(StopReason::BudgetExceeded(Resource::Work))
        ));
        assert_eq!(refused.scratch_meter().live(), 0);
    }

    #[test]
    fn report_owners_release_rejected_branches_and_refused_publication() {
        let schema = Schema::compile(
            r#"{"allOf":[{"anyOf":[{"format":"email","pattern":"^x"},true]},false]}"#,
        )
        .unwrap();
        let instance = json::parse(r#""bad""#).unwrap();
        let mut governor = Governor::new(Budget::default());
        let result = run(&schema, schema.root, &instance, &mut governor).unwrap();
        assert_eq!(result.format_evidence.len(), 1);
        assert_eq!(result.pattern_evidence.len(), 1);
        assert!(!result.format_evidence[0].passed);
        assert!(
            result
                .violations
                .iter()
                .all(|entry| entry.keyword != "pattern")
        );
        assert!(!result.violations.is_empty());
        let usage = governor.usage();
        assert!(governor.scratch_meter().live() > 0);
        drop(result);
        assert_eq!(governor.scratch_meter().live(), 0);

        for (resource, limit) in [
            (Resource::Work, usage.work - 1),
            (Resource::ScratchBytes, usage.peak_scratch_bytes - 1),
            (Resource::EvidenceEntries, usage.evidence_entries - 1),
        ] {
            let mut budget = Budget::default();
            match resource {
                Resource::Work => budget.work = limit,
                Resource::ScratchBytes => budget.scratch_bytes = limit,
                Resource::EvidenceEntries => budget.evidence_entries = limit,
                _ => unreachable!(),
            }
            let mut governor = Governor::new(budget);
            assert!(
                matches!(run(&schema, schema.root, &instance, &mut governor), Err(StopReason::BudgetExceeded(actual)) if actual == resource)
            );
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        let token = crate::CancellationToken::new();
        let mut governor = Governor::with_cancellation(Budget::default(), token.clone());
        let result = run(&schema, schema.root, &instance, &mut governor).unwrap();
        token.cancel();
        assert!(matches!(
            result.publish(&schema, &mut governor),
            Err(StopReason::Cancelled)
        ));
        assert_eq!(governor.scratch_meter().live(), 0);

        let mut governor = Governor::new(Budget::default());
        let report = schema.validate_with(&instance, &mut governor).unwrap();
        let retained = governor.scratch_meter().live();
        assert!(retained > 0);
        drop(report);
        assert_eq!(governor.scratch_meter().live(), retained);
    }

    #[test]
    fn control_paths_release_on_success_and_late_admission_refusal() {
        let schema = Schema::compile(
            r##"{"$defs":{"node":{"properties":{"雪/~":{"$ref":"#/$defs/node"}}}},"$ref":"#/$defs/node"}"##,
        ).unwrap();
        let instance = json::parse(r#"{"雪/~":{"雪/~":{}}}"#).unwrap();
        let mut governor = Governor::new(Budget::default());
        assert_eq!(
            run(&schema, schema.root, &instance, &mut governor)
                .unwrap()
                .violations
                .values
                .as_slice(),
            [] as [crate::schema::Violation; 0],
        );
        let usage = governor.usage();
        assert_eq!(governor.scratch_meter().live(), 0);
        for (work, scratch_bytes, reason) in [
            (usage.work, usage.peak_scratch_bytes, None),
            (
                usage.work - 1,
                usage.peak_scratch_bytes,
                Some(Resource::Work),
            ),
            (
                usage.work,
                usage.peak_scratch_bytes - 1,
                Some(Resource::ScratchBytes),
            ),
        ] {
            let mut governor = Governor::new(Budget {
                work,
                scratch_bytes,
                ..Budget::default()
            });
            let result = run(&schema, schema.root, &instance, &mut governor);
            if let Some(resource) = reason {
                assert!(
                    matches!(result, Err(StopReason::BudgetExceeded(actual)) if actual == resource)
                );
            } else {
                assert_eq!(
                    result.unwrap().violations.values.as_slice(),
                    [] as [crate::schema::Violation; 0]
                );
            }
            assert_eq!(governor.scratch_meter().live(), 0);
        }

        // Reserve active capacity but leave no room for the first dynamic
        // scope slot. Refusal occurs after the active key has been pushed.
        let schema = Schema::compile("true").unwrap();
        let mut governor = Governor::new(Budget::default());
        let mut active = ScratchVec::new(&governor);
        active.reserve(1).unwrap();
        let remaining = governor.limits().scratch_bytes - governor.scratch_meter().live();
        let occupied = governor.lease_scratch(remaining).unwrap();
        let mut evaluator = Evaluator {
            schema: &schema,
            active,
            scope: ScratchVec::new(&governor),
            governor: &mut governor,
        };
        assert!(matches!(
            evaluator.eval(schema.root, &instance, "", 0),
            Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
        ));
        assert!(evaluator.active.is_empty());
        assert!(evaluator.scope.is_empty());
        drop(evaluator);
        assert_eq!(governor.scratch_meter().live(), remaining);
        drop(occupied);
        assert_eq!(governor.scratch_meter().live(), 0);

        // Report strings are a separate retained-output contract; only their
        // exact path bytes, not zero total output admission, are asserted here.
        let schema = Schema::compile(r#"{"properties":{"雪/~":{"prefixItems":[false]}}}"#).unwrap();
        let report = schema
            .validate(&json::parse(r#"{"雪/~":[1]}"#).unwrap())
            .unwrap();
        assert_eq!(report.violations[0].instance_path, "/雪~1~0/0");
    }
}
