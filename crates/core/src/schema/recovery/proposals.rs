// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
use super::{Candidate, Search, size};
use crate::contract::{Effort, Resource, StopReason};
use crate::json::{Member, Node, ParseFailure, Value, pointer_segment};
use std::collections::BTreeSet;

impl Search<'_> {
    pub(super) fn propose(
        &mut self,
        parent: &Candidate,
        node: &Node,
        schema_id: usize,
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
    ) -> Result<(), StopReason> {
        let (route, path) = site;
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        self.governor.charge(Resource::Work, 1)?;
        if !visited.insert((schema_id, route.to_vec())) {
            return Ok(());
        }
        self.shapes(parent, node, route, path)?;
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        for keyword in ["$ref", "$dynamicRef"] {
            if let Some(reference) = body.get(keyword).and_then(Node::as_str) {
                let target = schema
                    .resolve(&schema.index.locations[schema_id].base, reference)
                    .map_err(|e| StopReason::InvalidInput(e.to_string()))?;
                self.propose(parent, node, target, (route, path), depth + 1, visited)?;
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
                    )?;
                }
            }
        }
        match &node.value {
            Value::Object(members) => {
                self.object_proposals(parent, schema_id, members, (route, path), depth, visited)?;
            }
            Value::Array(items) => {
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
                    let key = if position < prefix {
                        format!("prefixItems/{position}")
                    } else {
                        "items".into()
                    };
                    if schema.index.locations[schema_id]
                        .children
                        .contains_key(&key)
                    {
                        let mut next = route.to_vec();
                        next.push(position);
                        self.propose(
                            parent,
                            item,
                            schema.child(schema_id, &key),
                            (&next, &format!("{path}/{position}")),
                            depth + 1,
                            visited,
                        )?;
                    }
                }
            }
            _ => {}
        }
        self.local_constraints(parent, node, schema_id, (route, path))?;
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
    fn object_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        site: (&[usize], &str),
        depth: u32,
        visited: &mut BTreeSet<(usize, Vec<usize>)>,
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
        for (position, member) in members.iter().enumerate() {
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
                )?;
            } else {
                for property in properties {
                    if members.iter().any(|member| member.key == property.key) {
                        continue;
                    }
                    if let Some(cost) = self.name_cost(&member.key, &property.key)? {
                        let mut replacement = members.to_vec();
                        replacement[position].key.clone_from(&property.key);
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
            }
            if members[..position]
                .iter()
                .any(|prior| prior.key == member.key)
            {
                for remove in 0..=position {
                    if members[remove].key == member.key {
                        let mut replacement = members.to_vec();
                        replacement.remove(remove);
                        self.change(
                            parent,
                            route,
                            path,
                            (
                                Node::new(Value::Object(replacement)),
                                "json.key.resolve-duplicate",
                                10,
                                false,
                            ),
                        )?;
                    }
                }
            }
        }
        self.required_proposals(parent, schema_id, members, (route, path), depth)?;
        Ok(())
    }
    fn required_proposals(
        &mut self,
        parent: &Candidate,
        schema_id: usize,
        members: &[Member],
        site: (&[usize], &str),
        depth: u32,
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
                    let values = if let Some(&child) =
                        schema.index.locations[schema_id].children.get(&key)
                    {
                        self.witnesses(child, depth + 1, &mut BTreeSet::new())?
                    } else {
                        vec![Node::new(Value::Null)]
                    };
                    for value in values {
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
    fn witnesses(
        &mut self,
        schema_id: usize,
        depth: u32,
        visited: &mut BTreeSet<usize>,
    ) -> Result<Vec<Node>, StopReason> {
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        self.governor.charge(Resource::Work, 1)?;
        if !visited.insert(schema_id) {
            return Ok(Vec::new());
        }
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        let mut values = Vec::new();
        for keyword in ["const", "default"] {
            if let Some(value) = body.get(keyword) {
                values.push(value.clone());
            }
        }
        if let Some(Node {
            value: Value::Array(choices),
            ..
        }) = body.get("enum")
        {
            values.extend(choices.clone());
        }
        if let Some(reference) = body.get("$ref").and_then(Node::as_str) {
            let target = schema
                .resolve(&schema.index.locations[schema_id].base, reference)
                .map_err(|e| StopReason::InvalidInput(e.to_string()))?;
            values.extend(self.witnesses(target, depth + 1, visited)?);
        }
        for keyword in ["anyOf", "oneOf", "allOf"] {
            if let Some(Node {
                value: Value::Array(branches),
                ..
            }) = body.get(keyword)
            {
                for position in 0..branches.len() {
                    values.extend(self.witnesses(
                        schema.child(schema_id, &format!("{keyword}/{position}")),
                        depth + 1,
                        visited,
                    )?);
                }
            }
        }
        values.extend(self.typed_witnesses(schema_id, depth, visited)?);
        visited.remove(&schema_id);
        let mut distinct = BTreeSet::new();
        values.retain(|node| distinct.insert(node.to_json()));
        Ok(values)
    }
    fn typed_witnesses(
        &mut self,
        schema_id: usize,
        depth: u32,
        visited: &mut BTreeSet<usize>,
    ) -> Result<Vec<Node>, StopReason> {
        let schema = self.schema;
        let body = schema.keywords(schema_id);
        let mut values = Vec::new();
        match body.get("type").and_then(Node::as_str) {
            Some("boolean") => {
                values.push(Node::new(Value::Bool(false)));
                values.push(Node::new(Value::Bool(true)));
            }
            Some("number" | "integer") => {
                for keyword in ["minimum", "maximum", "multipleOf"] {
                    if let Some(value) = body.get(keyword) {
                        values.push(value.clone());
                    }
                }
                values.push(Node::new(Value::Number("0".parse().expect("zero"))));
            }
            Some("string") => {
                let count = count(body.get("minLength"))?;
                self.governor.charge(Resource::ScratchBytes, size(count))?;
                self.governor.charge(Resource::Work, size(count))?;
                values.push(Node::new(Value::String(" ".repeat(count))));
            }
            Some("object") => {
                let mut members = Vec::new();
                if let Some(Node {
                    value: Value::Array(required),
                    ..
                }) = body.get("required")
                {
                    for name in required.iter().filter_map(Node::as_str) {
                        let key = format!("properties/{}", pointer_segment(name));
                        let choices = if let Some(&child) =
                            schema.index.locations[schema_id].children.get(&key)
                        {
                            self.witnesses(child, depth + 1, visited)?
                        } else {
                            vec![Node::new(Value::Null)]
                        };
                        if let Some(value) = choices.into_iter().next() {
                            members.push(Member {
                                key: name.into(),
                                key_span: None,
                                value,
                            });
                        }
                    }
                }
                values.push(Node::new(Value::Object(members)));
            }
            Some("array") => {
                let count = count(body.get("minItems"))?;
                self.governor
                    .charge(Resource::ScratchBytes, size(count).saturating_mul(128))?;
                self.governor.charge(Resource::Work, size(count))?;
                let item =
                    if let Some(&child) = schema.index.locations[schema_id].children.get("items") {
                        self.witnesses(child, depth + 1, visited)?
                            .into_iter()
                            .next()
                            .unwrap_or_else(|| Node::new(Value::Null))
                    } else {
                        Node::new(Value::Null)
                    };
                values.push(Node::new(Value::Array(vec![item; count])));
            }
            _ => values.push(Node::new(Value::Null)),
        }
        Ok(values)
    }
}
fn count(node: Option<&Node>) -> Result<usize, StopReason> {
    match node.map(|node| &node.value) {
        None => Ok(0),
        Some(Value::Number(number)) => number
            .to_usize()
            .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes)),
        _ => Err(StopReason::InvalidInput(
            "compiled length constraint is not numeric".into(),
        )),
    }
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
