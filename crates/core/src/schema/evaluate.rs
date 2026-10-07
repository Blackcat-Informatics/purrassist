// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Schema evaluation of one instance.
//!
//! Every allocation the walk makes is charged to the caller's governor
//! before it is made: the evaluation's own collections are
//! [`ScratchVec`]/[`ScratchMap`]s (charged at portable element sizes,
//! released when the evaluation drops them), the strings they hold are
//! charged by length while held, and every transient lookup key or child
//! path is built at its exact length under a [`ScratchLease`]. Violations and
//! format evidence are the report's retained output and keep their charge.
use super::layout::Dependency;
use super::{Schema, Violation};
use crate::contract::saturating_size as size;
use crate::contract::{
    Evidence, Governor, Resource, ScratchLease, ScratchMap, ScratchMeter, ScratchVec, StopReason,
    Validation,
};
use crate::json::{Member, Node, Value};

/// Everything [`super::evaluate::run`] collects while walking one instance
/// against one schema: hard assertion failures, plus the always-on
/// evidence requirement 1 (`format`) and the Task 6 pattern-engine wiring
/// check both require, regardless of which branch's assertions passed.
pub(super) struct EvalResult {
    pub violations: Vec<Violation>,
    pub format_evidence: Vec<Validation>,
    pub pattern_evidence: Vec<Evidence>,
}

/// `ScratchBytes` charged per retained pattern-evidence entry's struct, on
/// top of its strings: at least `size_of::<Evidence>()` on every target.
const EVIDENCE_BYTES: u64 = 160;
const _: () = assert!(core::mem::size_of::<Evidence>() as u64 <= EVIDENCE_BYTES);

/// `ScratchBytes` charged per branch evaluation an applicator holds while
/// it decides (`allOf`, `anyOf`, `oneOf`): at least
/// `size_of::<Option<Evaluation>>()` on every target.
const EVALUATION_BYTES: u64 = 256;

/// `ScratchBytes` for one transient array-item child key
/// ([`super::layout::ItemLayout::keyword`]): `additionalItems`, or a
/// positional prefix (`prefixItems/` at most) and a decimal index.
const ITEM_KEY_BYTES: u64 = 32;

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
    /// Requirement 1: format annotation results, always recorded regardless
    /// of whether the enclosing branch's assertions passed.
    format_evidence: ScratchVec<Validation>,
    /// Task 6 wiring: `(location id, "pattern" | "patternProperties/<name>")`
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
    active: ScratchMap<(usize, String), ()>,
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
        active: ScratchMap::new(governor),
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
            destination: format!("{}#{}/{keyword}", location.base, location.pointer),
            ..evidence
        });
    }
    Ok(EvalResult {
        violations: errors.into_vec(),
        format_evidence: format_evidence.into_vec(),
        pattern_evidence,
    })
}

/// The byte length of `key` as an RFC 6901 pointer segment.
fn escaped_len(key: &str) -> usize {
    key.len()
        + key
            .bytes()
            .filter(|&byte| byte == b'~' || byte == b'/')
            .count()
}

/// Append `key` escaped as an RFC 6901 pointer segment.
fn push_segment(out: &mut String, key: &str) {
    for character in key.chars() {
        match character {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
}

/// The decimal length of `value`.
const fn digits(mut value: usize) -> usize {
    let mut count = 1;
    while value >= 10 {
        value /= 10;
        count += 1;
    }
    count
}

/// One piece of a transient key built by [`Evaluator::key`].
enum Piece<'a> {
    /// Text as it is.
    Text(&'a str),
    /// A member name, escaped as a pointer segment.
    Segment(&'a str),
    /// A decimal index.
    Index(usize),
}

impl Evaluator<'_, '_> {
    /// Build a transient key (a child lookup key or an instance path) at its
    /// exact length, charged by a lease the caller holds while the key
    /// lives.
    fn key(&mut self, pieces: &[Piece<'_>]) -> Result<(String, ScratchLease), StopReason> {
        let length: usize = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => text.len(),
                Piece::Segment(key) => escaped_len(key),
                Piece::Index(index) => digits(*index),
            })
            .sum();
        let lease = self.governor.lease_scratch(size(length))?;
        let mut key = String::with_capacity(length);
        for piece in pieces {
            match piece {
                Piece::Text(text) => key.push_str(text),
                Piece::Segment(segment) => push_segment(&mut key, segment),
                Piece::Index(index) => {
                    use core::fmt::Write as _;
                    let _ = write!(key, "{index}");
                }
            }
        }
        Ok((key, lease))
    }
    fn violation(
        &mut self,
        result: &mut Evaluation,
        id: usize,
        path: &str,
        keyword: &str,
        message: impl Into<String>,
    ) -> Result<(), StopReason> {
        self.governor.charge(Resource::EvidenceEntries, 1)?;
        let location = &self.schema.index.locations[id];
        let message = message.into();
        // A `Violation` may be a transient search diagnostic or the actual
        // output an external `Schema::validate` caller retains indefinitely.
        // Since that is not known here, its strings are charged directly and
        // never released, like `retain_source`: safe to over-count, never to
        // under-count or to release memory a caller may still be holding.
        // The message is charged by its allocated capacity (a formatted
        // message may hold more than its length); the other strings are
        // built at their exact lengths.
        let schema_path_len =
            location.base.len() + location.pointer.len() + 2 + escaped_len(keyword);
        self.governor.charge(
            Resource::ScratchBytes,
            size(path.len())
                .saturating_add(size(schema_path_len))
                .saturating_add(size(keyword.len()))
                .saturating_add(size(message.capacity()))
                .saturating_add(64),
        )?;
        let mut schema_path = String::with_capacity(schema_path_len);
        schema_path.push_str(&location.base);
        schema_path.push('#');
        schema_path.push_str(&location.pointer);
        schema_path.push('/');
        push_segment(&mut schema_path, keyword);
        result.errors.push(Violation {
            instance_path: path.into(),
            schema_path,
            keyword: keyword.into(),
            message,
        })
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
        // The active-recursion key retains two `String` copies of `path`
        // (one moved into the map, one kept here to remove it later);
        // charge before allocating either. Released when this call returns,
        // matching the unconditional removal below.
        let _lease = self
            .governor
            .lease_scratch(size(path.len()).saturating_mul(2).saturating_add(48))?;
        let key = (id, path.to_string());
        if self.active.get(&key).is_some() {
            return Err(StopReason::InvalidInput(
                "schema reference recurs without instance progress".into(),
            ));
        }
        self.active.insert(key.clone(), ())?;
        let base = &self.schema.index.locations[id].base;
        let entered = self
            .scope
            .as_slice()
            .last()
            .is_none_or(|&last| self.schema.index.locations[last].base != *base);
        let result = if entered {
            match self.scope.push(id) {
                Ok(()) => {
                    let result = self.evaluate_body(id, instance, path, depth);
                    self.scope.pop();
                    result
                }
                Err(reason) => Err(reason),
            }
        } else {
            self.evaluate_body(id, instance, path, depth)
        };
        self.active.remove(&key);
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
        let body = schema.keywords(id);
        let mut result = Evaluation::new(self.governor);
        if let Value::Bool(accept) = body.value {
            if !accept {
                self.violation(
                    &mut result,
                    id,
                    path,
                    "false",
                    "boolean schema rejects every instance",
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
        for keyword in ["$ref", "$dynamicRef"] {
            if let Some(reference) = schema.keywords(id).get(keyword).and_then(Node::as_str) {
                let base = &schema.index.locations[id].base;
                // Resolution builds the absolute URI and copies the target's
                // route, pointer, and base, each bounded by the base and the
                // reference: the same allowance compilation charges for it.
                let resolving = self.governor.lease_scratch(
                    size(base.len())
                        .saturating_add(size(reference.len()))
                        .saturating_mul(8)
                        .saturating_add(256),
                )?;
                let mut target = schema
                    .resolve(base, reference)
                    .map_err(|error| StopReason::InvalidInput(error.to_string()))?;
                drop(resolving);
                if keyword == "$dynamicRef"
                    && reference.split_once('#').is_some_and(|(_, fragment)| {
                        !fragment.is_empty() && !fragment.starts_with('/')
                    })
                    && let Some(name) = &schema.index.locations[target].dynamic_anchor
                {
                    for position in 0..self.scope.len() {
                        let scope = self.scope.as_slice()[position];
                        let (anchor, _lease) = self.key(&[
                            Piece::Text(&schema.index.locations[scope].base),
                            Piece::Text("#"),
                            Piece::Text(name),
                        ])?;
                        if let Some(&candidate) = schema.index.anchors.get(&anchor)
                            && schema.index.locations[candidate].dynamic_anchor.as_ref()
                                == Some(name)
                        {
                            target = candidate;
                            break;
                        }
                    }
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
                Value::String(kind) => matches_type(instance, kind),
                Value::Array(kinds) => kinds
                    .iter()
                    .filter_map(Node::as_str)
                    .any(|kind| matches_type(instance, kind)),
                _ => false,
            };
            if !valid {
                let expected = expected.to_json();
                let _expected = self.governor.lease_scratch(size(expected.capacity()))?;
                self.violation(result, id, path, "type", format!("expected {expected}"))?;
            }
        }
        if let Some(expected) = body.get("const")
            && !instance.equivalent(expected)
        {
            self.violation(result, id, path, "const", "instance differs from const")?;
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
            if !values.iter().any(|value| instance.equivalent(value)) {
                self.violation(result, id, path, "enum", "instance is not an enum member")?;
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
                        let comparison = number.compare(bound);
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
                                format!("{number} violates bound {bound}"),
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
                        format!("{number} is not an exact multiple of {divisor}"),
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
                            "string does not match the pattern",
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
    /// Requirement 1: `format` is always evaluated and always reported as
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
                format!("does not satisfy format {name:?}"),
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
        let number = crate::json::Number::from_count(count);
        for (keyword, is_minimum) in [(minimum, true), (maximum, false)] {
            if let Some(Node {
                value: Value::Number(bound),
                ..
            }) = self.schema.keywords(id).get(keyword)
            {
                let comparison = number.compare(bound);
                if (is_minimum && comparison.is_lt()) || (!is_minimum && comparison.is_gt()) {
                    self.violation(
                        result,
                        id,
                        path,
                        keyword,
                        format!("count {count} violates bound {bound}"),
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
        let body = schema.keywords(id);
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(Node {
                value: Value::Array(children),
                ..
            }) = body.get(keyword)
            {
                // Every branch's evaluation is held until the keyword's
                // outcome is known.
                let _branches = self
                    .governor
                    .lease_scratch(size(children.len()).saturating_mul(EVALUATION_BYTES))?;
                let mut branches: Vec<Option<Evaluation>> = Vec::with_capacity(children.len());
                for position in 0..children.len() {
                    let child = {
                        let (key, _lease) = self.key(&[
                            Piece::Text(keyword),
                            Piece::Text("/"),
                            Piece::Index(position),
                        ])?;
                        schema.child(id, &key)
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
                        format!("{passed} of {} branches passed", children.len()),
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
        if body.get("not").is_some() {
            let mut branch = self.eval(schema.child(id, "not"), instance, path, depth + 1)?;
            if branch.valid() {
                self.violation(result, id, path, "not", "negated schema matched")?;
            }
            result.merge_evidence(&mut branch)?;
        }
        if body.get("if").is_some() {
            let mut condition = self.eval(schema.child(id, "if"), instance, path, depth + 1)?;
            let branch = if condition.valid() {
                result.merge(condition)?;
                "then"
            } else {
                result.merge_evidence(&mut condition)?;
                "else"
            };
            if body.get(branch).is_some() {
                result.merge(self.eval(schema.child(id, branch), instance, path, depth + 1)?)?;
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
                "object has ambiguous duplicate member occurrences",
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
                        format!("missing required member {key:?}"),
                    )?;
                }
            }
        }
        // Each `dependentSchemas` entry's child key is built by
        // `pointer_segment` and `format!`: at most two escaping passes and a
        // formatting buffer, each at most twice the escaped key's length.
        let _dependencies = match body.get("dependentSchemas").map(|node| &node.value) {
            Some(Value::Object(entries)) => Some(
                self.governor.lease_scratch(
                    entries
                        .iter()
                        .map(|entry| size(entry.key.len()).saturating_mul(16).saturating_add(96))
                        .fold(0, u64::saturating_add),
                )?,
            ),
            _ => None,
        };
        for dependency in schema.dependencies(id) {
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
                                format!("{trigger} requires member {key:?}"),
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
        let body = schema.keywords(id);
        let mut matched = false;
        let (child_path, _child_path) = self.key(&[
            Piece::Text(path),
            Piece::Text("/"),
            Piece::Segment(&member.key),
        ])?;
        if body.get("propertyNames").is_some() {
            // The name as a string instance, and its instance path.
            let _name = self.governor.lease_scratch(size(member.key.len()))?;
            let name = Node::new(Value::String(member.key.clone()));
            let (name_path, _name_path) =
                self.key(&[Piece::Text(&child_path), Piece::Text("/@name")])?;
            result.merge_child(self.eval(
                schema.child(id, "propertyNames"),
                &name,
                &name_path,
                depth + 1,
            )?)?;
        }
        if let Some(properties) = body.get("properties")
            && properties.get(&member.key).is_some()
        {
            matched = true;
            let child = {
                let (key, _lease) =
                    self.key(&[Piece::Text("properties/"), Piece::Segment(&member.key)])?;
                schema.child(id, &key)
            };
            let branch = self.eval(child, &member.value, &child_path, depth + 1)?;
            result.merge_child(branch)?;
        }
        if let Some(Node {
            value: Value::Object(patterns),
            ..
        }) = body.get("patternProperties")
        {
            for pattern in patterns {
                let (name, _name) = self.key(&[
                    Piece::Text("patternProperties/"),
                    Piece::Segment(&pattern.key),
                ])?;
                result.pattern(id, &name)?;
                if schema.index.locations[id].patterns[&name]
                    .is_match(&member.key, self.governor)?
                {
                    matched = true;
                    let branch = self.eval(
                        schema.child(id, &name),
                        &member.value,
                        &child_path,
                        depth + 1,
                    )?;
                    result.merge_child(branch)?;
                }
            }
        }
        if !matched && body.get("additionalProperties").is_some() {
            matched = true;
            let branch = self.eval(
                schema.child(id, "additionalProperties"),
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
        let body = schema.keywords(id);
        self.bounds(id, path, items.len(), "minItems", "maxItems", result)?;
        if body
            .get("uniqueItems")
            .is_some_and(|node| node.value == Value::Bool(true))
        {
            for (position, item) in items.iter().enumerate() {
                for previous in &items[..position] {
                    self.governor.charge(Resource::Work, 1)?;
                    if previous.equivalent(item) {
                        self.violation(
                            result,
                            id,
                            path,
                            "uniqueItems",
                            format!("duplicate array value at index {position}"),
                        )?;
                        break;
                    }
                }
            }
        }
        let layout = schema.item_layout(id);
        for (position, item) in items.iter().enumerate() {
            let child = {
                let _key = self.governor.lease_scratch(ITEM_KEY_BYTES)?;
                layout
                    .keyword(position)
                    .map(|keyword| schema.child(id, &keyword))
            };
            if let Some(child) = child {
                let (item_path, _item_path) =
                    self.key(&[Piece::Text(path), Piece::Text("/"), Piece::Index(position)])?;
                let branch = self.eval(child, item, &item_path, depth + 1)?;
                result.merge_child(branch)?;
                result.items.push(position)?;
            }
        }
        if body.get("contains").is_some() {
            let mut matched: ScratchVec<usize> = ScratchVec::new(self.governor);
            for (position, item) in items.iter().enumerate() {
                let (item_path, _item_path) =
                    self.key(&[Piece::Text(path), Piece::Text("/"), Piece::Index(position)])?;
                let mut branch =
                    self.eval(schema.child(id, "contains"), item, &item_path, depth + 1)?;
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
                    "no array member satisfies contains",
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
        let body = schema.keywords(id);
        match &instance.value {
            Value::Object(members) if body.get("unevaluatedProperties").is_some() => {
                let pending = self.unevaluated_members(members, result)?;
                for &index in pending.as_slice() {
                    let member = &members[index];
                    let (member_path, _member_path) = self.key(&[
                        Piece::Text(path),
                        Piece::Text("/"),
                        Piece::Segment(&member.key),
                    ])?;
                    let branch = self.eval(
                        schema.child(id, "unevaluatedProperties"),
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
            Value::Array(items) if body.get("unevaluatedItems").is_some() => {
                result.items.as_mut_slice().sort_unstable();
                let mut pending: ScratchVec<usize> = ScratchVec::new(self.governor);
                for position in 0..items.len() {
                    self.governor.charge(Resource::Work, 1)?;
                    if result.items.as_slice().binary_search(&position).is_err() {
                        pending.push(position)?;
                    }
                }
                for &position in pending.as_slice() {
                    let (item_path, _item_path) =
                        self.key(&[Piece::Text(path), Piece::Text("/"), Piece::Index(position)])?;
                    let branch = self.eval(
                        schema.child(id, "unevaluatedItems"),
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
fn matches_type(instance: &Node, kind: &str) -> bool {
    match (kind, &instance.value) {
        ("null", Value::Null)
        | ("boolean", Value::Bool(_))
        | ("object", Value::Object(_))
        | ("array", Value::Array(_))
        | ("number", Value::Number(_))
        | ("string", Value::String(_)) => true,
        ("integer", Value::Number(number)) => number.is_integer(),
        _ => false,
    }
}
