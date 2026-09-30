// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{Candidate, Search, size};
use crate::contract::{Effort, Resource, StopReason};
use crate::json::{Member, Node, ParseFailure, Value, pointer_segment};
use std::collections::BTreeSet;

impl Search<'_> {
    /// Walk the schema alongside `node`, proposing local repairs and
    /// recursing into every applicator this engine gives generation
    /// semantics to (Task 8 requirement 2): `$ref`/`$dynamicRef` (the
    /// latter resolved against the live `scope` dynamic-scope stack, the
    /// same as the evaluator and the witness algebra), `allOf`/`anyOf`/
    /// `oneOf`, `properties`/`patternProperties`/`additionalProperties`/
    /// `unevaluatedProperties`, `prefixItems`/`items`/`contains`/
    /// `unevaluatedItems`, `dependentSchemas`, and `if`/`then`/`else`
    /// (recursed into whichever branch currently applies). `not` and a
    /// `propertyNames` member key have no *local* repair (there is no
    /// single "opposite" edit for an arbitrary negated schema, and
    /// fabricating an unrelated whole-candidate replacement just to dodge
    /// one broken applicator would weaken recovery's connection to the
    /// actual input): a `not` violation is an explicit no-candidate for
    /// this rule, and a failing `propertyNames` key is repaired narrowly,
    /// by generating a replacement key from its own `pattern` (via the
    /// witness algebra's regex generation) rather than inventing a new
    /// object from scratch.
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    pub(super) fn propose(
        &mut self,
        parent: &Candidate,
        node: &Node,
        schema_id: usize,
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        self.governor.charge(Resource::Work, 1)?;
        if !visited.insert((schema_id, route.to_vec())) {
            return Ok(());
        }
        self.shapes(parent, node, route, path)?;
        let base = self.schema.index.locations[schema_id].base.clone();
        let entered = scope.last() != Some(&base);
        if entered {
            scope.push(base);
        }
        let result = self.propose_body(
            parent,
            node,
            schema_id,
            (route, path),
            depth,
            visited,
            scope,
        );
        if entered {
            scope.pop();
        }
        result
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    fn propose_body(
        &mut self,
        parent: &Candidate,
        node: &Node,
        schema_id: usize,
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        for keyword in ["$ref", "$dynamicRef"] {
            if let Some(reference) = body.get(keyword).and_then(Node::as_str) {
                let target =
                    self.resolve_reference(schema_id, keyword == "$dynamicRef", reference, scope)?;
                self.propose(
                    parent,
                    node,
                    target,
                    (route, path),
                    depth + 1,
                    visited,
                    scope,
                )?;
            }
        }
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(Node {
                value: Value::Array(branches),
                ..
            }) = body.get(keyword)
            {
                for position in 0..branches.len() {
                    self.propose(
                        parent,
                        node,
                        schema.child(schema_id, &format!("{keyword}/{position}")),
                        (route, path),
                        depth + 1,
                        visited,
                        scope,
                    )?;
                }
            }
        }
        self.if_then_else_proposal(
            parent,
            node,
            schema_id,
            (route, path),
            depth,
            visited,
            scope,
        )?;
        match &node.value {
            Value::Object(members) => {
                self.object_proposals(
                    parent,
                    schema_id,
                    members,
                    (route, path),
                    depth,
                    visited,
                    scope,
                )?;
            }
            Value::Array(items) => {
                self.array_proposals(
                    parent,
                    schema_id,
                    items,
                    (route, path),
                    depth,
                    visited,
                    scope,
                )?;
            }
            _ => {}
        }
        self.local_constraints(parent, node, schema_id, (route, path))?;
        self.format_repair(parent, node, schema_id, (route, path))?;
        Ok(())
    }
    /// Resolve `$ref`/`$dynamicRef` the same way `super::super::evaluate`
    /// and the witness algebra do: `$dynamicRef` to a fragment-only anchor
    /// is overridden by the outermost matching `$dynamicAnchor` on the live
    /// `scope` stack, never resolved purely statically.
    fn resolve_reference(
        &self,
        schema_id: usize,
        dynamic: bool,
        reference: &str,
        scope: &[String],
    ) -> Result<usize, StopReason> {
        let schema = self.schema;
        let base = schema.index.locations[schema_id].base.clone();
        let mut target = schema
            .resolve(&base, reference)
            .map_err(|error| StopReason::InvalidInput(error.to_string()))?;
        if dynamic
            && reference
                .split_once('#')
                .is_some_and(|(_, fragment)| !fragment.is_empty() && !fragment.starts_with('/'))
            && let Some(name) = &schema.index.locations[target].dynamic_anchor
        {
            for scope_base in scope {
                if let Some(&candidate) = schema.index.anchors.get(&format!("{scope_base}#{name}"))
                    && schema.index.locations[candidate].dynamic_anchor.as_ref() == Some(name)
                {
                    target = candidate;
                    break;
                }
            }
        }
        Ok(target)
    }
    /// `if`/`then`/`else` (Task 8 requirement 2): recurse into whichever of
    /// `then`/`else` currently applies to `node`, so nested local repairs
    /// inside that branch are still explored.
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    fn if_then_else_proposal(
        &mut self,
        parent: &Candidate,
        node: &Node,
        schema_id: usize,
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        if body.get("if").is_some() {
            let if_id = schema.child(schema_id, "if");
            let condition_holds = super::super::evaluate::run(schema, if_id, node, self.governor)?
                .violations
                .is_empty();
            let branch = if condition_holds { "then" } else { "else" };
            if body.get(branch).is_some() {
                self.propose(
                    parent,
                    node,
                    schema.child(schema_id, branch),
                    (route, path),
                    depth + 1,
                    visited,
                    scope,
                )?;
            }
        }
        Ok(())
    }
    /// A member whose key fails `propertyNames`: propose renaming just that
    /// key to a witness of the `propertyNames` schema's own `pattern`
    /// (Task 6's regex generation), keeping the member's value untouched.
    /// Every other shape of `propertyNames` (no `pattern`, or a failing key
    /// this pattern's generator cannot reach) is left an explicit
    /// no-candidate for this rule rather than fabricating an unrelated key.
    fn property_names_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        site: (&[usize], &str),
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        if body.get("propertyNames").is_none() {
            return Ok(());
        }
        let names_id = schema.child(schema_id, "propertyNames");
        let Some(pattern) = schema.index.locations[names_id].patterns.get("pattern") else {
            return Ok(());
        };
        for (position, member) in members.iter().enumerate() {
            let key_node = Node::new(Value::String(member.key.clone()));
            if super::super::evaluate::run(schema, names_id, &key_node, self.governor)?
                .violations
                .is_empty()
            {
                continue;
            }
            if let crate::pattern::GenerateOutcome::Witness(new_key) = pattern.generate(
                self.governor,
                crate::pattern::GenerateOptions {
                    min_len: 0,
                    max_len: Some(64),
                    avoid: &[],
                },
            )? {
                let _lease = self.governor.lease_scratch(members_weight(members))?;
                let mut replacement = members.to_vec();
                replacement[position].key = new_key;
                replacement[position].key_span = None;
                self.change(
                    parent,
                    route,
                    path,
                    (
                        Node::new(Value::Object(replacement)),
                        "json.schema.property-name-repair",
                        30,
                        true,
                    ),
                )?;
            }
        }
        Ok(())
    }
    /// Requirement 4: a `date`/`date-time`/`time` violation under an active
    /// format-assertion vocabulary is repaired as a labeled `Guess`
    /// inference (normalization only); every other failing format yields no
    /// proposal here, leaving it an explicit no-candidate for this rule
    /// (assertions are never weakened, only the search's guess is added).
    fn format_repair(
        &mut self,
        parent: &Candidate,
        node: &Node,
        schema_id: usize,
        site: (&[usize], &str),
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let body = self.schema.keywords(schema_id);
        if !body.vocabularies.format_assert {
            return Ok(());
        }
        let Some(name) = body.get("format").and_then(Node::as_str) else {
            return Ok(());
        };
        if !super::super::format::repairable(name) {
            return Ok(());
        }
        let Value::String(text) = &node.value else {
            return Ok(());
        };
        if matches!(
            super::super::format::evaluate(name, text, self.governor)?,
            super::super::format::Outcome::Fail
        ) && let Some(fixed) = super::super::format::repair(name, text, self.governor)?
        {
            self.change(
                parent,
                route,
                path,
                (
                    Node::new(Value::String(fixed)),
                    "json.schema.format-repair",
                    20,
                    true,
                ),
            )?;
        }
        Ok(())
    }
    fn local_constraints(
        &mut self,
        parent: &Candidate,
        node: &Node,
        schema_id: usize,
        site: (&[usize], &str),
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let body = self.schema.keywords(schema_id);
        if let Some(value) = body.get("const") {
            self.change(
                parent,
                route,
                path,
                (value.clone(), "json.schema.const", 100, true),
            )?;
        }
        if let Some(Node {
            value: Value::Array(values),
            ..
        }) = body.get("enum")
        {
            for value in values {
                if let (Some(actual), Some(expected)) = (node.as_str(), value.as_str())
                    && let Some(cost) = self.name_cost(actual, expected)?
                {
                    self.change(
                        parent,
                        route,
                        path,
                        (value.clone(), "json.enum.match", cost, false),
                    )?;
                }
            }
        }
        if body.get("type").and_then(Node::as_str) == Some("string")
            && !matches!(
                node.value,
                Value::String(_) | Value::Object(_) | Value::Array(_)
            )
        {
            self.change(
                parent,
                route,
                path,
                (
                    Node::new(Value::String(node.to_json())),
                    "json.type.stringify-scalar",
                    8,
                    false,
                ),
            )?;
        }
        if body.get("type").and_then(Node::as_str) == Some("array")
            && !matches!(node.value, Value::Array(_))
        {
            self.change(
                parent,
                route,
                path,
                (
                    Node::new(Value::Array(vec![node.clone()])),
                    "json.shape.wrap-array",
                    5,
                    false,
                ),
            )?;
        }
        Ok(())
    }
    fn shapes(
        &mut self,
        parent: &Candidate,
        node: &Node,
        route: &[usize],
        path: &str,
    ) -> Result<(), StopReason> {
        match &node.value {
            Value::Array(items) if items.len() == 1 => self.change(
                parent,
                route,
                path,
                (items[0].clone(), "json.shape.unwrap-array", 1, false),
            )?,
            Value::Object(members)
                if members.len() == 1
                    && matches!(members[0].value.value, Value::Array(_) | Value::Object(_)) =>
            {
                self.change(
                    parent,
                    route,
                    path,
                    (
                        members[0].value.clone(),
                        "json.shape.unwrap-object",
                        4,
                        false,
                    ),
                )?;
            }
            Value::String(text) => match crate::json::parse_with(text, self.governor) {
                Ok(mut decoded) => {
                    clear_spans(&mut decoded);
                    self.change(
                        parent,
                        route,
                        path,
                        (decoded, "json.type.decode-string", 4, false),
                    )?;
                }
                Err(ParseFailure::Resource(reason)) => return Err(reason),
                Err(ParseFailure::Syntax(_)) => {}
            },
            _ => {}
        }
        Ok(())
    }
    fn name_cost(&mut self, actual: &str, expected: &str) -> Result<Option<u64>, StopReason> {
        self.governor.charge(
            Resource::Work,
            size(actual.len().saturating_add(expected.len())),
        )?;
        if actual == expected {
            return Ok(None);
        }
        let left = crate::text::comparison_key(actual);
        let right = crate::text::comparison_key(expected);
        if left == right {
            return Ok(Some(2));
        }
        if crate::text::depunctuate(&left) == crate::text::depunctuate(&right) {
            return Ok(Some(3));
        }
        let distance = crate::matching::damerau_levenshtein(&left, &right, self.governor)?;
        let length = left.chars().count().max(right.chars().count());
        Ok(
            (distance <= 2 && distance.saturating_mul(3) <= length.max(3))
                .then_some(5 + size(distance)),
        )
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    fn object_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        let properties = body
            .get("properties")
            .and_then(|n| {
                if let Value::Object(v) = &n.value {
                    Some(v.as_slice())
                } else {
                    None
                }
            })
            .unwrap_or(&[]);
        // Key-typo rename candidates: every `properties` name (each with a
        // sub-schema to recurse into once matched) plus every `required`
        // name `properties` does not already declare (a `required` name
        // with no `properties` entry still names an expected key, so a
        // typo'd occurrence of it should still be recognized and renamed).
        let mut candidate_names: Vec<String> = properties
            .iter()
            .map(|property| property.key.clone())
            .collect();
        if let Some(Node {
            value: Value::Array(required),
            ..
        }) = body.get("required")
        {
            for name in required.iter().filter_map(Node::as_str) {
                if !candidate_names.iter().any(|candidate| candidate == name) {
                    candidate_names.push(name.to_string());
                }
            }
        }
        for (position, member) in members.iter().enumerate() {
            let matched = properties.iter().any(|property| property.key == member.key);
            if let Some(property) = properties
                .iter()
                .find(|property| property.key == member.key)
            {
                let mut next = route.to_vec();
                next.push(position);
                self.propose(
                    parent,
                    &member.value,
                    schema.child(
                        schema_id,
                        &format!("properties/{}", pointer_segment(&property.key)),
                    ),
                    (&next, &format!("{path}/{}", pointer_segment(&member.key))),
                    depth + 1,
                    visited,
                    scope,
                )?;
            } else {
                self.key_repair_proposals(
                    parent,
                    schema_id,
                    members,
                    position,
                    &candidate_names,
                    (route, path),
                )?;
            }
            self.pattern_and_additional_proposals(
                parent,
                schema_id,
                members,
                position,
                matched,
                (route, path),
                depth,
                visited,
                scope,
            )?;
            self.duplicate_key_proposals(parent, members, position, (route, path))?;
        }
        self.dependent_schema_proposals(
            parent,
            schema_id,
            members,
            (route, path),
            depth,
            visited,
            scope,
        )?;
        self.required_proposals(parent, schema_id, members, (route, path))?;
        self.property_names_proposals(parent, schema_id, members, (route, path))?;
        Ok(())
    }
    /// A member unmatched by `properties`: propose renaming its key to a
    /// close `properties` name, or dropping it when `additionalProperties`
    /// is `false`.
    fn key_repair_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        position: usize,
        candidate_names: &[String],
        site: (&[usize], &str),
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let body = self.schema.keywords(schema_id);
        for candidate_name in candidate_names {
            if members.iter().any(|member| member.key == *candidate_name) {
                continue;
            }
            if let Some(cost) = self.name_cost(&members[position].key, candidate_name)? {
                let _lease = self.governor.lease_scratch(members_weight(members))?;
                let mut replacement = members.to_vec();
                replacement[position].key.clone_from(candidate_name);
                replacement[position].key_span = None;
                self.change(
                    parent,
                    route,
                    path,
                    (
                        Node::new(Value::Object(replacement)),
                        "json.key.match",
                        cost,
                        false,
                    ),
                )?;
            }
        }
        if body
            .get("additionalProperties")
            .is_some_and(|n| n.value == Value::Bool(false))
        {
            let _lease = self.governor.lease_scratch(members_weight(members))?;
            let mut replacement = members.to_vec();
            replacement.remove(position);
            self.change(
                parent,
                route,
                path,
                (
                    Node::new(Value::Object(replacement)),
                    "json.key.remove-additional",
                    50,
                    false,
                ),
            )?;
        }
        Ok(())
    }
    /// A member whose key already occurred earlier: propose removing
    /// either occurrence, resolving the ambiguity. Costed below the
    /// cheapest possible key rename (`json.key.match`'s minimum is 6):
    /// an outright duplicate key is unambiguous JSON malformation with an
    /// obvious fix, not a guess about intent, so it must win the search
    /// before a same-shaped but speculative rename is even considered
    /// (otherwise a short, coincidentally close-by property name from an
    /// unrelated `allOf`/`oneOf` branch can look cheaper than simply
    /// dropping the duplicate).
    fn duplicate_key_proposals(
        &mut self,
        parent: &Candidate,
        members: &[Member],
        position: usize,
        site: (&[usize], &str),
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        if !members[..position]
            .iter()
            .any(|prior| prior.key == members[position].key)
        {
            return Ok(());
        }
        for remove in 0..=position {
            if members[remove].key == members[position].key {
                let _lease = self.governor.lease_scratch(members_weight(members))?;
                let mut replacement = members.to_vec();
                replacement.remove(remove);
                self.change(
                    parent,
                    route,
                    path,
                    (
                        Node::new(Value::Object(replacement)),
                        "json.key.resolve-duplicate",
                        2,
                        false,
                    ),
                )?;
            }
        }
        Ok(())
    }
    /// `patternProperties`/`additionalProperties`/`unevaluatedProperties`
    /// (Task 8 requirement 2): recurse into every matching sub-schema for
    /// this member's *value*, independently of the key-typo repairs above.
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    fn pattern_and_additional_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        position: usize,
        matched_by_properties: bool,
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        let member = &members[position];
        let mut next = route.to_vec();
        next.push(position);
        let child_path = format!("{path}/{}", pointer_segment(&member.key));
        let mut matched_pattern = false;
        if let Some(Node {
            value: Value::Object(patterns),
            ..
        }) = body.get("patternProperties")
        {
            for pattern in patterns {
                let keyword = format!("patternProperties/{}", pointer_segment(&pattern.key));
                if schema.index.locations[schema_id].patterns[&keyword]
                    .is_match(&member.key, self.governor)?
                {
                    matched_pattern = true;
                    self.propose(
                        parent,
                        &member.value,
                        schema.child(schema_id, &keyword),
                        (&next, &child_path),
                        depth + 1,
                        visited,
                        scope,
                    )?;
                }
            }
        }
        if !matched_by_properties && !matched_pattern {
            let fallback = if body.get("additionalProperties").is_some() {
                Some(schema.child(schema_id, "additionalProperties"))
            } else if body.get("unevaluatedProperties").is_some() {
                Some(schema.child(schema_id, "unevaluatedProperties"))
            } else {
                None
            };
            if let Some(target) = fallback {
                self.propose(
                    parent,
                    &member.value,
                    target,
                    (&next, &child_path),
                    depth + 1,
                    visited,
                    scope,
                )?;
            }
        }
        Ok(())
    }
    /// `dependentSchemas` (Task 8 requirement 2): when a trigger key is
    /// present, the dependent schema's own constraints apply to the whole
    /// object, so it is followed at the object's own route/path rather than
    /// one member's.
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    fn dependent_schema_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        if let Some(Node {
            value: Value::Object(dependencies),
            ..
        }) = body.get("dependentSchemas")
        {
            for dependency in dependencies {
                if members.iter().any(|member| member.key == dependency.key) {
                    self.propose(
                        parent,
                        &Node::new(Value::Object(members.to_vec())),
                        schema.child(
                            schema_id,
                            &format!("dependentSchemas/{}", pointer_segment(&dependency.key)),
                        ),
                        (route, path),
                        depth + 1,
                        visited,
                        scope,
                    )?;
                }
            }
        }
        Ok(())
    }
    fn required_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        site: (&[usize], &str),
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        if let Some(Node {
            value: Value::Array(required),
            ..
        }) = body.get("required")
        {
            for name in required.iter().filter_map(Node::as_str) {
                if members.iter().any(|member| member.key == name) {
                    continue;
                }
                // Relocation candidates retain the source value and remove its old occurrence.
                for (position, member) in members.iter().enumerate() {
                    if let Value::Object(nested) = &member.value.value {
                        for (nested_position, child) in nested
                            .iter()
                            .enumerate()
                            .filter(|(_, child)| child.key == name)
                        {
                            let _lease = self.governor.lease_scratch(
                                members_weight(members)
                                    .saturating_add(size(child.key.len()))
                                    .saturating_add(child.value.estimated_bytes()),
                            )?;
                            let mut replacement = members.to_vec();
                            if let Value::Object(children) = &mut replacement[position].value.value
                            {
                                children.remove(nested_position);
                            }
                            replacement.push(child.clone());
                            self.change(
                                parent,
                                route,
                                path,
                                (
                                    Node::new(Value::Object(replacement)),
                                    "json.key.relocate",
                                    12,
                                    false,
                                ),
                            )?;
                        }
                    }
                }
                if self.request.allow_guesses && self.request.effort >= Effort::Standard {
                    let key = format!("properties/{}", pointer_segment(name));
                    let value = if let Some(&child) =
                        schema.index.locations[schema_id].children.get(&key)
                    {
                        match super::super::algebra::generate(schema, child, self.governor)? {
                            super::super::algebra::WitnessOutcome::Witness(node) => Some(node),
                            super::super::algebra::WitnessOutcome::Unsatisfiable(_)
                            | super::super::algebra::WitnessOutcome::Exhausted => None,
                        }
                    } else {
                        Some(Node::new(Value::Null))
                    };
                    if let Some(value) = value {
                        let _lease = self.governor.lease_scratch(
                            members_weight(members).saturating_add(value.estimated_bytes()),
                        )?;
                        let mut replacement = members.to_vec();
                        replacement.push(Member {
                            key: name.into(),
                            key_span: None,
                            value,
                        });
                        self.change(
                            parent,
                            route,
                            path,
                            (
                                Node::new(Value::Object(replacement)),
                                "json.required.invent",
                                100,
                                true,
                            ),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
    /// `prefixItems`/`items`/`contains`/`unevaluatedItems` (Task 8
    /// requirement 2), each followed for every element it applies to.
    #[expect(
        clippy::too_many_arguments,
        reason = "each parameter is an independent fact this governed recursive walk needs; \
                  grouping them into a struct would only move the same facts, not reduce them"
    )]
    fn array_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        items: &[Node],
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
        scope: &mut Vec<String>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        let prefix = body
            .get("prefixItems")
            .and_then(|n| {
                if let Value::Array(v) = &n.value {
                    Some(v.len())
                } else {
                    None
                }
            })
            .unwrap_or(0);
        for (position, item) in items.iter().enumerate() {
            let mut next = route.to_vec();
            next.push(position);
            let child_path = format!("{path}/{position}");
            let key = if position < prefix {
                Some(format!("prefixItems/{position}"))
            } else if body.get("items").is_some() {
                Some("items".into())
            } else if body.get("unevaluatedItems").is_some() {
                Some("unevaluatedItems".into())
            } else {
                None
            };
            if let Some(key) = key
                && schema.index.locations[schema_id]
                    .children
                    .contains_key(&key)
            {
                self.propose(
                    parent,
                    item,
                    schema.child(schema_id, &key),
                    (&next, &child_path),
                    depth + 1,
                    visited,
                    scope,
                )?;
            }
            if body.get("contains").is_some() {
                self.propose(
                    parent,
                    item,
                    schema.child(schema_id, "contains"),
                    (&next, &child_path),
                    depth + 1,
                    visited,
                    scope,
                )?;
            }
        }
        Ok(())
    }
}
/// A conservative byte-size estimate for a member slice, used to charge
/// scratch before `.to_vec()`-ing it to build a proposal.
fn members_weight(members: &[Member]) -> u64 {
    // Portable, target-independent charge: `Member::CHARGE`, not `size_of`.
    let mut total = size(members.len())
        .saturating_mul(size(<Member as crate::contract::ScratchCharge>::CHARGE));
    for member in members {
        total = total
            .saturating_add(size(member.key.len()))
            .saturating_add(member.value.estimated_bytes());
    }
    total
}
fn clear_spans(node: &mut Node) {
    node.span = None;
    match &mut node.value {
        Value::Array(items) => {
            for item in items {
                clear_spans(item);
            }
        }
        Value::Object(members) => {
            for member in members {
                member.key_span = None;
                clear_spans(&mut member.value);
            }
        }
        _ => {}
    }
}
