// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{Schema, Violation};
use crate::contract::saturating_size as size;
use crate::contract::{Evidence, Governor, Resource, StopReason, Validation};
use crate::json::{Member, Node, Value, pointer_segment};
use std::collections::BTreeSet;

/// Everything [`super::evaluate::run`] collects while walking one instance
/// against one schema: hard assertion failures, plus the always-on
/// evidence requirement 1 (`format`) and the Task 6 pattern-engine wiring
/// check both require, regardless of which branch's assertions passed.
pub(super) struct EvalResult {
    pub violations: Vec<Violation>,
    pub format_evidence: Vec<Validation>,
    pub pattern_evidence: Vec<Evidence>,
}

#[derive(Default)]
struct Evaluation {
    errors: Vec<Violation>,
    properties: BTreeSet<String>,
    items: BTreeSet<usize>,
    /// Requirement 1: format annotation results, always recorded regardless
    /// of whether the enclosing branch's assertions passed.
    format_evidence: Vec<Validation>,
    /// Task 6 wiring: `(location id, "pattern" | "patternProperties/<name>")`
    /// for every compiled pattern this evaluation actually exercised.
    pattern_ids: BTreeSet<(usize, String)>,
}
impl Evaluation {
    fn merge(&mut self, other: Self) {
        if other.errors.is_empty() {
            self.properties.extend(other.properties);
            self.items.extend(other.items);
        }
        self.errors.extend(other.errors);
        self.format_evidence.extend(other.format_evidence);
        self.pattern_ids.extend(other.pattern_ids);
    }
    const fn valid(&self) -> bool {
        self.errors.is_empty()
    }
}
struct Evaluator<'s, 'g> {
    schema: &'s Schema,
    governor: &'g mut Governor,
    active: BTreeSet<(usize, String)>,
    scope: Vec<String>,
}

pub(super) fn run(
    schema: &Schema,
    id: usize,
    instance: &Node,
    governor: &mut Governor,
) -> Result<EvalResult, StopReason> {
    let mut evaluator = Evaluator {
        schema,
        governor,
        active: BTreeSet::new(),
        scope: Vec::new(),
    };
    let evaluation = evaluator.eval(id, instance, "", 0)?;
    let mut pattern_evidence = Vec::with_capacity(evaluation.pattern_ids.len());
    for (location_id, keyword) in &evaluation.pattern_ids {
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
        violations: evaluation.errors,
        format_evidence: evaluation.format_evidence,
        pattern_evidence,
    })
}
impl Evaluator<'_, '_> {
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
        let schema_path = format!(
            "{}#{}/{}",
            location.base,
            location.pointer,
            pointer_segment(keyword)
        );
        let message = message.into();
        // A `Violation` may be a transient search diagnostic or the actual
        // output an external `Schema::validate` caller retains indefinitely.
        // Since that is not known here, its strings are charged directly and
        // never released, like `retain_source`: safe to over-count, never to
        // under-count or to release memory a caller may still be holding.
        self.governor.charge(
            Resource::ScratchBytes,
            size(path.len())
                .saturating_add(size(schema_path.len()))
                .saturating_add(size(keyword.len()))
                .saturating_add(size(message.len()))
                .saturating_add(64),
        )?;
        result.errors.push(Violation {
            instance_path: path.into(),
            schema_path,
            keyword: keyword.into(),
            message,
        });
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
        // The active-recursion key retains two `String` copies of `path`
        // (one moved into the set, one kept here to remove it later);
        // charge before allocating either. Released when this call returns,
        // matching the unconditional `self.active.remove(&key)` below.
        let _lease = self
            .governor
            .lease_scratch(size(path.len()).saturating_mul(2).saturating_add(48))?;
        let key = (id, path.to_string());
        if !self.active.insert(key.clone()) {
            return Err(StopReason::InvalidInput(
                "schema reference recurs without instance progress".into(),
            ));
        }
        let base = &self.schema.index.locations[id].base;
        let entered = self.scope.last() != Some(base);
        if entered {
            self.scope.push(base.clone());
        }
        let result = self.evaluate_body(id, instance, path, depth);
        if entered {
            self.scope.pop();
        }
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
        let mut result = Evaluation::default();
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
                let mut target = schema
                    .resolve(&schema.index.locations[id].base, reference)
                    .map_err(|error| StopReason::InvalidInput(error.to_string()))?;
                if keyword == "$dynamicRef"
                    && reference.split_once('#').is_some_and(|(_, fragment)| {
                        !fragment.is_empty() && !fragment.starts_with('/')
                    })
                    && let Some(name) = &schema.index.locations[target].dynamic_anchor
                {
                    for base in &self.scope {
                        if let Some(&candidate) =
                            schema.index.anchors.get(&format!("{base}#{name}"))
                            && schema.index.locations[candidate].dynamic_anchor.as_ref()
                                == Some(name)
                        {
                            target = candidate;
                            break;
                        }
                    }
                }
                result.merge(self.eval(target, instance, path, depth + 1)?);
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
                self.violation(
                    result,
                    id,
                    path,
                    "type",
                    format!("expected {}", expected.to_json()),
                )?;
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
                    result.pattern_ids.insert((id, "pattern".into()));
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
        });
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
                let mut passed = Vec::new();
                let mut failed = Vec::new();
                for position in 0..children.len() {
                    let branch = self.eval(
                        schema.child(id, &format!("{keyword}/{position}")),
                        instance,
                        path,
                        depth + 1,
                    )?;
                    if branch.valid() {
                        passed.push(branch);
                    } else {
                        failed.push(branch);
                    }
                }
                let valid = match keyword {
                    "allOf" => failed.is_empty(),
                    "anyOf" => !passed.is_empty(),
                    _ => passed.len() == 1,
                };
                if valid {
                    for branch in passed {
                        result.merge(branch);
                    }
                } else {
                    self.violation(
                        result,
                        id,
                        path,
                        keyword,
                        format!("{} of {} branches passed", passed.len(), children.len()),
                    )?;
                    for branch in failed {
                        result.errors.extend(branch.errors);
                    }
                }
            }
        }
        if body.get("not").is_some()
            && self
                .eval(schema.child(id, "not"), instance, path, depth + 1)?
                .valid()
        {
            self.violation(result, id, path, "not", "negated schema matched")?;
        }
        if body.get("if").is_some() {
            let condition = self.eval(schema.child(id, "if"), instance, path, depth + 1)?;
            let branch = if condition.valid() {
                result.merge(condition);
                "then"
            } else {
                "else"
            };
            if body.get(branch).is_some() {
                result.merge(self.eval(schema.child(id, branch), instance, path, depth + 1)?);
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
        let names: BTreeSet<_> = members.iter().map(|member| member.key.as_str()).collect();
        if names.len() != members.len() {
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
                if !names.contains(key) {
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
        if let Some(Node {
            value: Value::Object(dependencies),
            ..
        }) = body.get("dependentRequired")
        {
            for dependency in dependencies {
                if names.contains(dependency.key.as_str())
                    && let Value::Array(required) = &dependency.value.value
                {
                    for key in required.iter().filter_map(Node::as_str) {
                        if !names.contains(key) {
                            self.violation(
                                result,
                                id,
                                path,
                                "dependentRequired",
                                format!("{} requires member {key:?}", dependency.key),
                            )?;
                        }
                    }
                }
            }
        }
        if let Some(Node {
            value: Value::Object(dependencies),
            ..
        }) = body.get("dependentSchemas")
        {
            for dependency in dependencies {
                if names.contains(dependency.key.as_str()) {
                    result.merge(self.eval(
                        schema.child(
                            id,
                            &format!("dependentSchemas/{}", pointer_segment(&dependency.key)),
                        ),
                        instance,
                        path,
                        depth + 1,
                    )?);
                }
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
        let child_path = format!("{path}/{}", pointer_segment(&member.key));
        if body.get("propertyNames").is_some() {
            let name = Node::new(Value::String(member.key.clone()));
            result.errors.extend(
                self.eval(
                    schema.child(id, "propertyNames"),
                    &name,
                    &format!("{child_path}/@name"),
                    depth + 1,
                )?
                .errors,
            );
        }
        if let Some(properties) = body.get("properties")
            && properties.get(&member.key).is_some()
        {
            matched = true;
            let branch = self.eval(
                schema.child(id, &format!("properties/{}", pointer_segment(&member.key))),
                &member.value,
                &child_path,
                depth + 1,
            )?;
            result.errors.extend(branch.errors);
            result.properties.insert(member.key.clone());
        }
        if let Some(Node {
            value: Value::Object(patterns),
            ..
        }) = body.get("patternProperties")
        {
            for pattern in patterns {
                let name = format!("patternProperties/{}", pointer_segment(&pattern.key));
                result.pattern_ids.insert((id, name.clone()));
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
                    result.errors.extend(branch.errors);
                    result.properties.insert(member.key.clone());
                }
            }
        }
        if !matched && body.get("additionalProperties").is_some() {
            let branch = self.eval(
                schema.child(id, "additionalProperties"),
                &member.value,
                &child_path,
                depth + 1,
            )?;
            result.errors.extend(branch.errors);
            result.properties.insert(member.key.clone());
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
        let prefix = if let Some(Node {
            value: Value::Array(prefix),
            ..
        }) = body.get("prefixItems")
        {
            prefix.len()
        } else {
            0
        };
        for (position, item) in items.iter().enumerate() {
            let keyword = if position < prefix {
                Some(format!("prefixItems/{position}"))
            } else if body.get("items").is_some() {
                Some("items".into())
            } else {
                None
            };
            if let Some(keyword) = keyword {
                let branch = self.eval(
                    schema.child(id, &keyword),
                    item,
                    &format!("{path}/{position}"),
                    depth + 1,
                )?;
                result.errors.extend(branch.errors);
                result.items.insert(position);
            }
        }
        if body.get("contains").is_some() {
            let mut matched = BTreeSet::new();
            for (position, item) in items.iter().enumerate() {
                if self
                    .eval(
                        schema.child(id, "contains"),
                        item,
                        &format!("{path}/{position}"),
                        depth + 1,
                    )?
                    .valid()
                {
                    matched.insert(position);
                }
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
            result.items.extend(matched);
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
                for member in members {
                    if !result.properties.contains(&member.key) {
                        let branch = self.eval(
                            schema.child(id, "unevaluatedProperties"),
                            &member.value,
                            &format!("{path}/{}", pointer_segment(&member.key)),
                            depth + 1,
                        )?;
                        result.errors.extend(branch.errors);
                        result.properties.insert(member.key.clone());
                    }
                }
            }
            Value::Array(items) if body.get("unevaluatedItems").is_some() => {
                for (position, item) in items.iter().enumerate() {
                    if result.items.insert(position) {
                        let branch = self.eval(
                            schema.child(id, "unevaluatedItems"),
                            item,
                            &format!("{path}/{position}"),
                            depth + 1,
                        )?;
                        result.errors.extend(branch.errors);
                    }
                }
            }
            _ => {}
        }
        Ok(())
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
