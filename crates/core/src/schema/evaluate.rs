// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Schema evaluation of one instance.
//!
//! Recursion keys and transient paths/lookup keys retain fallible owners and
//! admit copying, comparison, lookup and relocation through the caller governor.
//! Active keys and dynamic scopes unwind on every result; recursion obeys both
//! caller Depth and a native ceiling. Integer tests, bound comparisons and
//! transient violation messages also use admitted operations. Remaining
//! annotation/report construction, merging
//! and publication retain their separate storage contract and are not claimed
//! to have complete ownership governance here.
use super::layout::Dependency;
use super::{Schema, Violation};
use super::{index, reference};
use crate::contract::saturating_size as size;
use crate::contract::{
    Evidence, Governor, Resource, ScratchMeter, ScratchString, ScratchVec, StopReason, Validation,
    format_output_governed, format_scratch_governed,
};
use crate::json::{Member, Node, Value};

/// Everything [`super::evaluate::run`] collects while walking one instance
/// against one schema: hard assertion failures, plus the always-on
/// `format` annotation evidence and the pattern-engine wiring
/// check both require, regardless of which branch's assertions passed.
pub(super) struct EvalResult {
    pub violations: Vec<Violation>,
    pub format_evidence: Vec<Validation>,
    pub pattern_evidence: Vec<Evidence>,
}

/// Bound native evaluator nesting, with the root at depth zero.
/// Reference and applicator crossings count too: one data level can require
/// object, array and reference-target frames. This safety ceiling is separate
/// from the ordinary caller Depth preset.
const NATIVE_EVALUATION_DEPTH: u32 = 256;

/// `ScratchBytes` charged per retained pattern-evidence entry's struct, on
/// top of its strings: at least `size_of::<Evidence>()` on every target.
const EVIDENCE_BYTES: u64 = 160;
const _: () = assert!(core::mem::size_of::<Evidence>() as u64 <= EVIDENCE_BYTES);

/// `ScratchBytes` charged per branch evaluation an applicator holds while
/// it decides (`allOf`, `anyOf`, `oneOf`): at least
/// `size_of::<Option<Evaluation>>()` on every target.
const EVALUATION_BYTES: u64 = 256;

/// Bytes charged while held for the contents of strings an evaluation
/// keeps (member names, pattern keyword keys), released when dropped.
struct Held {
    meter: ScratchMeter,
    bytes: u64,
}

impl Held {
    fn new(governor: &Governor) -> Self {
        Self {
            meter: governor.scratch_meter(),
            bytes: 0,
        }
    }

    fn charge(&mut self, bytes: u64) -> Result<(), StopReason> {
        self.meter.charge(bytes)?;
        self.bytes = self.bytes.saturating_add(bytes);
        Ok(())
    }

    /// Take over `bytes` of `other`'s charge with the strings it describes.
    fn transfer(&mut self, other: &mut Self, bytes: u64) {
        let bytes = bytes.min(other.bytes);
        other.bytes -= bytes;
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.meter.release(self.bytes);
    }
}

const _: () = assert!(core::mem::size_of::<Option<Evaluation>>() as u64 <= EVALUATION_BYTES);

struct Evaluation {
    errors: ScratchVec<Violation>,
    /// Member names evaluated at this instance scope (may repeat; sorted
    /// before lookup).
    properties: ScratchVec<String>,
    properties_held: Held,
    /// Array positions evaluated at this instance scope (may repeat).
    items: ScratchVec<usize>,
    /// Format annotation results, always recorded regardless
    /// of whether the enclosing branch's assertions passed.
    format_evidence: ScratchVec<Validation>,
    /// Pattern-engine wiring: `(location id, "pattern" | "patternProperties/<name>")`
    /// for every compiled pattern this evaluation actually exercised, each
    /// once.
    pattern_ids: ScratchVec<(usize, String)>,
    patterns_held: Held,
}
impl Evaluation {
    fn new(governor: &Governor) -> Self {
        Self {
            errors: ScratchVec::new(governor),
            properties: ScratchVec::new(governor),
            properties_held: Held::new(governor),
            items: ScratchVec::new(governor),
            format_evidence: ScratchVec::new(governor),
            pattern_ids: ScratchVec::new(governor),
            patterns_held: Held::new(governor),
        }
    }
    /// Merge a schema evaluated against this same instance scope.
    fn merge(&mut self, mut other: Self) -> Result<(), StopReason> {
        if other.errors.is_empty() {
            let bytes = other.properties_held.bytes;
            self.properties.append(&mut other.properties)?;
            self.properties_held
                .transfer(&mut other.properties_held, bytes);
            self.items.append(&mut other.items)?;
        }
        self.merge_child(other)
    }
    /// Child-instance annotations cannot mark members of the parent evaluated.
    fn merge_child(&mut self, mut other: Self) -> Result<(), StopReason> {
        self.merge_evidence(&mut other)?;
        self.errors.append(&mut other.errors)
    }
    /// Keep observations even when the branch's assertions are discarded.
    fn merge_evidence(&mut self, other: &mut Self) -> Result<(), StopReason> {
        self.format_evidence.append(&mut other.format_evidence)?;
        let mut moved = 0_u64;
        while let Some(entry) = other.pattern_ids.pop() {
            if self.pattern_ids.as_slice().contains(&entry) {
                continue;
            }
            moved = moved.saturating_add(size(entry.1.len()));
            self.pattern_ids.push(entry)?;
        }
        self.patterns_held.transfer(&mut other.patterns_held, moved);
        Ok(())
    }
    /// Record that the compiled pattern `(id, key)` was exercised.
    fn pattern(&mut self, id: usize, key: &str) -> Result<(), StopReason> {
        if self
            .pattern_ids
            .as_slice()
            .iter()
            .any(|(known, name)| *known == id && name == key)
        {
            return Ok(());
        }
        self.patterns_held.charge(size(key.len()))?;
        self.pattern_ids.push((id, key.to_owned()))
    }
    /// Record that the member named `key` was evaluated.
    fn property(&mut self, key: &str) -> Result<(), StopReason> {
        self.properties_held.charge(size(key.len()))?;
        self.properties.push(key.to_owned())
    }
    const fn valid(&self) -> bool {
        self.errors.is_empty()
    }
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
    // Sorted like the set of exercised patterns it is.
    pattern_ids.as_mut_slice().sort_unstable();
    governor.charge(
        Resource::ScratchBytes,
        size(pattern_ids.len()).saturating_mul(EVIDENCE_BYTES),
    )?;
    let mut pattern_evidence = Vec::with_capacity(pattern_ids.len());
    for (location_id, keyword) in pattern_ids.as_slice() {
        governor.charge(Resource::EvidenceEntries, 1)?;
        let location = &schema.index.locations[*location_id];
        let pattern = &location.patterns[keyword];
        let evidence = pattern.engine_evidence();
        governor.charge(
            Resource::ScratchBytes,
            size(location.pointer.len())
                .saturating_add(size(keyword.len()))
                .saturating_add(64),
        )?;
        pattern_evidence.push(Evidence {
            destination: format!(
                "{}#{}/{keyword}",
                location.base.as_str(),
                location.pointer.as_str()
            ),
            ..evidence
        });
    }
    Ok(EvalResult {
        violations: errors.into_vec(),
        format_evidence: format_evidence.into_vec(),
        pattern_evidence,
    })
}

/// Report-path sizing; retained violation storage has a separate contract.
fn escaped_len(key: &str) -> usize {
    key.len()
        + key
            .bytes()
            .filter(|&byte| byte == b'~' || byte == b'/')
            .count()
}

/// Report-path encoding; traversal keys use the shared admitted pointer builder.
fn push_segment(out: &mut String, key: &str) {
    for character in key.chars() {
        match character {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
}

impl Evaluator<'_, '_> {
    /// Resolve the compiled child before entering its evaluation frame.
    fn eval_child(
        &mut self,
        id: usize,
        key: &str,
        instance: &Node,
        path: &str,
        depth: u32,
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
        let message = format_scratch_governed(message, self.governor)?;
        // A `Violation` may be a transient search diagnostic or the actual
        // output an external `Schema::validate` caller retains indefinitely.
        // Since that is not known here, its strings are charged directly and
        // never released, like `retain_source`: safe to over-count, never to
        // under-count or to release memory a caller may still be holding.
        // The message retains its temporary owner until transfer below; the
        // other strings are built at their exact lengths.
        let schema_path_len =
            location.base.len() + location.pointer.len() + 2 + escaped_len(keyword);
        self.governor.charge(
            Resource::ScratchBytes,
            size(path.len())
                .saturating_add(size(schema_path_len))
                .saturating_add(size(keyword.len()))
                .saturating_add(64),
        )?;
        let mut schema_path = String::with_capacity(schema_path_len);
        schema_path.push_str(&location.base);
        schema_path.push('#');
        schema_path.push_str(&location.pointer);
        schema_path.push('/');
        push_segment(&mut schema_path, keyword);
        let (message, message_lease) = message.into_owned_parts();
        result.errors.push(Violation {
            instance_path: path.into(),
            schema_path,
            keyword: keyword.into(),
            message,
        })?;
        message_lease.retain();
        Ok(())
    }
    fn eval(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u32,
    ) -> Result<Evaluation, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        if depth > NATIVE_EVALUATION_DEPTH {
            return Err(self.governor.stop_resource(Resource::Depth));
        }
        for (known_id, known_path) in &self.active {
            reference::work(self.governor, 1)?;
            if *known_id == id {
                reference::work(
                    self.governor,
                    path.len().min(known_path.len()).saturating_add(1),
                )?;
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
        depth: u32,
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
        depth: u32,
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
                result.merge(self.eval(target, instance, path, depth + 1)?)?;
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
        let body = schema.keywords(id);
        if let Some(expected) = body.get("type") {
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
        if let Some(expected) = body.get("const")
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
        }) = body.get("enum")
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
        let body = schema.keywords(id);
        match &instance.value {
            Value::Number(number) => {
                for keyword in ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"] {
                    if let Some(Node {
                        value: Value::Number(bound),
                        ..
                    }) = body.get(keyword)
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
                }) = body.get("multipleOf")
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
                    text.chars().count(),
                    "minLength",
                    "maxLength",
                    result,
                )?;
                if let Some(pattern) = schema.index.locations[id].patterns.get("pattern") {
                    result.pattern(id, "pattern")?;
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
                if let Some(name) = body.get("format").and_then(Node::as_str) {
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
        let (validator, passed, diagnostics) = match outcome {
            super::format::Outcome::Pass => (super::format::rule(name), true, Vec::new()),
            super::format::Outcome::Fail => (
                super::format::rule(name),
                false,
                vec![format!("{text:?} does not satisfy format {name:?}")],
            ),
            super::format::Outcome::Unknown => (
                "schema.format.unknown-v1".into(),
                true,
                vec![format!("format {name:?} is not evaluated")],
            ),
        };
        self.governor.charge(
            Resource::ScratchBytes,
            size(validator.len())
                .saturating_add(size(path.len()))
                .saturating_add(
                    diagnostics
                        .iter()
                        .fold(0, |sum, line| sum.saturating_add(size(line.len()))),
                )
                .saturating_add(64),
        )?;
        let assert = !passed && self.schema.keywords(id).vocabularies.format_assert;
        result.format_evidence.push(Validation {
            validator,
            constraint: path.into(),
            passed,
            diagnostics,
        })?;
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
        count: usize,
        minimum: &str,
        maximum: &str,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        for (keyword, is_minimum) in [(minimum, true), (maximum, false)] {
            if let Some(Node {
                value: Value::Number(bound),
                ..
            }) = self.schema.keywords(id).get(keyword)
            {
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
        depth: u32,
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
                let _branches = self
                    .governor
                    .lease_scratch(size(children.len()).saturating_mul(EVALUATION_BYTES))?;
                let mut branches: Vec<Option<Evaluation>> = Vec::with_capacity(children.len());
                for position in 0..children.len() {
                    let child = {
                        let key = format_scratch_governed(
                            format_args!("{keyword}/{position}"),
                            self.governor,
                        )?;
                        index::child(schema, id, &key, self.governor)?
                    };
                    branches.push(Some(self.eval(child, instance, path, depth + 1)?));
                }
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
                for slot in &mut branches {
                    if slot.as_ref().is_some_and(|branch| branch.valid() == valid)
                        && let Some(branch) = slot.take()
                    {
                        if valid {
                            result.merge(branch)?;
                        } else {
                            result.merge_child(branch)?;
                        }
                    }
                }
                for mut branch in branches.into_iter().flatten() {
                    result.merge_evidence(&mut branch)?;
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
            result.merge_evidence(&mut branch)?;
        }
        if body.get_governed("if", self.governor)?.is_some() {
            let mut condition = self.eval_child(id, "if", instance, path, depth + 1)?;
            let branch = if condition.valid() {
                result.merge(condition)?;
                "then"
            } else {
                result.merge_evidence(&mut condition)?;
                "else"
            };
            if body.get_governed(branch, self.governor)?.is_some() {
                result.merge(self.eval_child(id, branch, instance, path, depth + 1)?)?;
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
        depth: u32,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        let body = schema.keywords(id);
        self.bounds(
            id,
            path,
            members.len(),
            "minProperties",
            "maxProperties",
            result,
        )?;
        // The member names, sorted for lookup: `&str` is two words on a
        // 64-bit target.
        let _names = self
            .governor
            .lease_scratch(size(members.len()).saturating_mul(16))?;
        let mut names: Vec<&str> = Vec::with_capacity(members.len());
        names.extend(members.iter().map(|member| member.key.as_str()));
        names.sort_unstable();
        let has = |key: &str| names.binary_search(&key).is_ok();
        if names.windows(2).any(|pair| pair[0] == pair[1]) {
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
        }) = body.get("required")
        {
            for key in required.iter().filter_map(Node::as_str) {
                if !has(key) {
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
                } if has(trigger) => {
                    for key in required.iter().filter_map(Node::as_str) {
                        if !has(key) {
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
                Dependency::Schema { trigger, child } if has(trigger) => {
                    result.merge(self.eval(child, instance, path, depth + 1)?)?;
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
        depth: u32,
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
            result.merge_child(self.eval_child(
                id,
                "propertyNames",
                &name,
                &name_path,
                depth + 1,
            )?)?;
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
            result.merge_child(branch)?;
        }
        if let Some(Node {
            value: Value::Object(patterns),
            ..
        }) = body.get_governed("patternProperties", self.governor)?
        {
            for pattern in patterns {
                let name = index::pointer("patternProperties", &pattern.key, self.governor)?;
                result.pattern(id, &name)?;
                if schema.index.locations[id]
                    .patterns
                    .get_governed(name.as_str(), self.governor)?
                    .expect("compiled pattern exists")
                    .is_match(&member.key, self.governor)?
                {
                    matched = true;
                    let branch =
                        self.eval_child(id, &name, &member.value, &child_path, depth + 1)?;
                    result.merge_child(branch)?;
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
            result.merge_child(branch)?;
        }
        if matched {
            result.property(&member.key)?;
        }
        Ok(())
    }
    fn array(
        &mut self,
        id: usize,
        items: &[Node],
        path: &str,
        depth: u32,
        result: &mut Evaluation,
    ) -> Result<(), StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let body = schema.keywords(id);
        self.bounds(id, path, items.len(), "minItems", "maxItems", result)?;
        if body
            .get("uniqueItems")
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
                result.merge_child(branch)?;
                result.items.push(position)?;
            }
        }
        if body.get_governed("contains", self.governor)?.is_some() {
            let mut matched: ScratchVec<usize> = ScratchVec::new(self.governor);
            for (position, item) in items.iter().enumerate() {
                let item_path =
                    format_scratch_governed(format_args!("{path}/{position}"), self.governor)?;
                let mut branch = self.eval_child(id, "contains", item, &item_path, depth + 1)?;
                if branch.valid() {
                    matched.push(position)?;
                }
                result.merge_evidence(&mut branch)?;
            }
            if body.get("minContains").is_none() && matched.is_empty() {
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
                matched.len(),
                "minContains",
                "maxContains",
                result,
            )?;
            result.items.append(&mut matched)?;
        }
        Ok(())
    }
    fn unevaluated(
        &mut self,
        id: usize,
        instance: &Node,
        path: &str,
        depth: u32,
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
                    result.merge_child(branch)?;
                }
                for &index in pending.as_slice() {
                    result.property(&members[index].key)?;
                }
            }
            Value::Array(items)
                if body
                    .get_governed("unevaluatedItems", self.governor)?
                    .is_some() =>
            {
                result.items.as_mut_slice().sort_unstable();
                let mut pending: ScratchVec<usize> = ScratchVec::new(self.governor);
                for position in 0..items.len() {
                    self.governor.charge(Resource::Work, 1)?;
                    if result.items.as_slice().binary_search(&position).is_err() {
                        pending.push(position)?;
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
                    result.merge_child(branch)?;
                }
                result.items.append(&mut pending)?;
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
        result.properties.as_mut_slice().sort_unstable();
        let mut pending: ScratchVec<usize> = ScratchVec::new(self.governor);
        for (index, member) in members.iter().enumerate() {
            self.governor.charge(Resource::Work, 1)?;
            if result
                .properties
                .as_slice()
                .binary_search(&member.key)
                .is_err()
            {
                pending.push(index)?;
            }
        }
        // Keep the first member of each name, then restore member order.
        pending.as_mut_slice().sort_unstable_by(|&left, &right| {
            members[left]
                .key
                .cmp(&members[right].key)
                .then(left.cmp(&right))
        });
        let mut first: ScratchVec<usize> = ScratchVec::new(self.governor);
        let mut previous: Option<&str> = None;
        for &index in pending.as_slice() {
            let key = members[index].key.as_str();
            if previous != Some(key) {
                first.push(index)?;
            }
            previous = Some(key);
        }
        first.as_mut_slice().sort_unstable();
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
    fn control_paths_release_on_success_and_late_admission_refusal() {
        let schema = Schema::compile(
            r##"{"$defs":{"node":{"properties":{"雪/~":{"$ref":"#/$defs/node"}}}},"$ref":"#/$defs/node"}"##,
        ).unwrap();
        let instance = json::parse(r#"{"雪/~":{"雪/~":{}}}"#).unwrap();
        let mut governor = Governor::new(Budget::default());
        assert_eq!(
            run(&schema, schema.root, &instance, &mut governor)
                .unwrap()
                .violations,
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
                    result.unwrap().violations,
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
