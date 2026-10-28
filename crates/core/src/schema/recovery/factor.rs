// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Certified independent mandatory wraps; unproved alternatives keep joint search.
//!
//! States pair a compiled location with an original input occurrence, preserving
//! spans and duplicate/order identity. Product edges partition original children;
//! reference and wrap edges are unary. All costs are nonnegative, so an optimum
//! has a cycle-free dependency path: at most V full relaxation rounds suffice
//! for V states. A global least-cost fixed point therefore handles recursive
//! references without treating a pending recursive state as an impossibility.
//!
//! Certification requires every minimum-cost edge to be a product, reference or
//! mandatory wrap. An equally cheap unwrap/coercion, coupled constraints, name
//! repair, relocation or unproved occurrence construction keeps joint search.
//! Mandatory wraps all have the same cost. Ancestor wraps precede dependent
//! children; independent wraps follow the original proposal preorder. This is
//! the first equal-cost derivation of the existing search, retaining intermediate
//! replacement bytes, source spans and evidence order. Its normal validators and
//! receipt publication remain authoritative.
//!
//! Chart preparation and replay are polynomial for this certified family,
//! including arbitrarily many independent wraps. No polynomial guarantee is made
//! for joint search, arbitrary ambiguity, guesses or unsupported schema families.
use super::{Schema, proposals::name_cost};
use crate::contract::{Resource, ScratchCharge, format_scratch_governed, saturating_size as size};
use crate::json::{Member, Node, ParseFailure, Value};
use crate::{Governor, ScratchString, ScratchVec, StopReason};

pub(super) const WRAP_COST: u64 = 5;
pub(super) const WRAP_RULE: &str = "json.shape.wrap-array";

pub(super) struct Edit {
    pub(super) route: ScratchVec<usize>,
    pub(super) path: ScratchString,
}
impl ScratchCharge for Edit {
    const CHARGE: usize = 80;
}
struct Occurrence<'a> {
    node: &'a Node,
    children: ScratchVec<usize>,
}
impl ScratchCharge for Occurrence<'_> {
    const CHARGE: usize = 48;
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Product,
    Reference,
    Wrap,
    Other,
}
struct Edge {
    kind: Kind,
    cost: u64,
    children: ScratchVec<usize>,
    positions: ScratchVec<usize>,
}
impl Edge {
    fn new(kind: Kind, cost: u64, governor: &Governor) -> Self {
        Self {
            kind,
            cost,
            children: ScratchVec::new(governor),
            positions: ScratchVec::new(governor),
        }
    }
}
impl ScratchCharge for Edge {
    const CHARGE: usize = 96;
}
struct State {
    schema: usize,
    occurrence: usize,
    edges: ScratchVec<Edge>,
    cost: Option<u64>,
}
impl ScratchCharge for State {
    const CHARGE: usize = 72;
}
struct Chart<'a> {
    schema: &'a Schema,
    occurrences: ScratchVec<Occurrence<'a>>,
    states: ScratchVec<State>,
    lookup: ScratchVec<usize>,
}

fn work(governor: &mut Governor, amount: usize) -> Result<(), StopReason> {
    governor.charge(Resource::Work, size(amount).saturating_add(1))
}
/// Admit the queried bytes and one control step before each comparison.
fn member_position(
    members: &[Member],
    query: &str,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    governor.check()?;
    for (position, member) in members.iter().enumerate() {
        work(governor, query.len())?;
        if member.key == query {
            return Ok(Some(position));
        }
    }
    Ok(None)
}
fn child_path(
    parent: &str,
    segment: &str,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    let mut path = format_scratch_governed(format_args!("{parent}/"), governor)?;
    work(governor, segment.len().saturating_mul(2))?;
    for character in segment.chars() {
        match character {
            '~' => path.push_str("~0")?,
            '/' => path.push_str("~1")?,
            character => path.push(character)?,
        }
    }
    Ok(path)
}
fn overflow() -> StopReason {
    StopReason::InvalidInput("schema repair chart size or cost overflow".into())
}

/// A certificate is optional; a resource refusal is never a certificate miss.
pub(super) fn prepare(
    schema: &Schema,
    node: &Node,
    governor: &mut Governor,
) -> Result<Option<ScratchVec<Edit>>, StopReason> {
    if !supported(schema, governor)? {
        return Ok(None);
    }
    let mut chart = Chart {
        schema,
        occurrences: ScratchVec::new(governor),
        states: ScratchVec::new(governor),
        lookup: ScratchVec::new(governor),
    };
    chart.occurrence(node, 0, governor)?;
    let cells = schema
        .index
        .locations
        .len()
        .checked_mul(chart.occurrences.len())
        .ok_or_else(overflow)?;
    work(governor, cells)?;
    chart.lookup.reserve(cells)?;
    chart
        .lookup
        .extend(core::iter::repeat_n(usize::MAX, cells))?;
    let root = chart.state(schema.root, 0, governor)?;
    let mut next = 0;
    while next < chart.states.len() {
        if !chart.expand(next, governor)? {
            return Ok(None);
        }
        next += 1;
    }
    chart.solve(governor)?;
    if chart.states[root].cost.is_none_or(|cost| cost == 0) {
        return Ok(None);
    }
    let proved = chart.prove(governor)?;
    if !proved[root] {
        return Ok(None);
    }
    let mut edits = ScratchVec::new(governor);
    chart.emit(root, &[], "", 0, &mut edits, governor)?;
    if chart.states[root].cost
        != Some(
            size(edits.len())
                .checked_mul(WRAP_COST)
                .ok_or_else(overflow)?,
        )
    {
        return Err(StopReason::InvalidInput(
            "mandatory-wrap proof disagrees with its edit sequence".into(),
        ));
    }
    governor.check()?;
    Ok(Some(edits))
}

/// Only a product of typed open objects, homogeneous arrays and static refs.
fn supported(schema: &Schema, governor: &mut Governor) -> Result<bool, StopReason> {
    for id in 0..schema.index.locations.len() {
        let Value::Object(members) = &schema.body(id).value else {
            return Ok(false);
        };
        let mut assertions = 0;
        let mut reference = false;
        for member in members {
            work(governor, member.key.len())?;
            if matches!(
                member.key.as_str(),
                "$ref" | "type" | "properties" | "required" | "items"
            ) && schema.keywords(id).get(&member.key).is_none()
            {
                return Ok(false);
            }
            match member.key.as_str() {
                "$schema" | "$id" | "$defs" | "definitions" => {}
                "$ref" => reference = true,
                "type" | "properties" | "required" | "items" => assertions += 1,
                _ => return Ok(false),
            }
        }
        let kind = schema.body(id).get("type").and_then(Node::as_str);
        if (kind != Some("object")
            && members
                .iter()
                .any(|member| matches!(member.key.as_str(), "properties" | "required")))
            || (kind != Some("array") && members.iter().any(|member| member.key == "items"))
        {
            return Ok(false);
        }
        if reference {
            if assertions != 0 {
                return Ok(false);
            }
        } else if !matches!(
            schema.body(id).get("type").and_then(Node::as_str),
            Some("object" | "array" | "string")
        ) {
            return Ok(false);
        }
    }
    Ok(true)
}
impl<'a> Chart<'a> {
    fn occurrence(
        &mut self,
        node: &'a Node,
        depth: u64,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        governor.charge(Resource::Depth, depth)?;
        work(governor, 1)?;
        let id = self.occurrences.len();
        self.occurrences.push(Occurrence {
            node,
            children: ScratchVec::new(governor),
        })?;
        match &node.value {
            Value::Array(items) => {
                for child in items {
                    let child = self.occurrence(child, depth + 1, governor)?;
                    self.occurrences.as_mut_slice()[id].children.push(child)?;
                }
            }
            Value::Object(members) => {
                for member in members {
                    let child = self.occurrence(&member.value, depth + 1, governor)?;
                    self.occurrences.as_mut_slice()[id].children.push(child)?;
                }
            }
            _ => {}
        }
        Ok(id)
    }
    fn state(
        &mut self,
        schema: usize,
        occurrence: usize,
        governor: &mut Governor,
    ) -> Result<usize, StopReason> {
        work(governor, 1)?;
        let slot = schema * self.occurrences.len() + occurrence;
        let known = self.lookup[slot];
        if known != usize::MAX {
            return Ok(known);
        }
        let id = self.states.len();
        self.states.push(State {
            schema,
            occurrence,
            edges: ScratchVec::new(governor),
            cost: None,
        })?;
        self.lookup.as_mut_slice()[slot] = id;
        Ok(id)
    }
    fn unary(
        &mut self,
        id: usize,
        kind: Kind,
        cost: u64,
        schema: usize,
        occurrence: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let mut edge = Edge::new(kind, cost, governor);
        edge.children
            .push(self.state(schema, occurrence, governor)?)?;
        self.states.as_mut_slice()[id].edges.push(edge)
    }
    fn expand(&mut self, id: usize, governor: &mut Governor) -> Result<bool, StopReason> {
        work(governor, 1)?;
        let schema_id = self.states[id].schema;
        let occurrence = self.states[id].occurrence;
        let node = self.occurrences[occurrence].node;
        match &node.value {
            Value::Array(items) if items.len() == 1 => {
                self.unary(
                    id,
                    Kind::Other,
                    1,
                    schema_id,
                    self.occurrences[occurrence].children[0],
                    governor,
                )?;
            }
            Value::Object(members)
                if members.len() == 1
                    && matches!(members[0].value.value, Value::Object(_) | Value::Array(_)) =>
            {
                self.unary(
                    id,
                    Kind::Other,
                    4,
                    schema_id,
                    self.occurrences[occurrence].children[0],
                    governor,
                )?;
            }
            Value::String(text) => {
                // Decoded strings introduce new occurrence identities. They keep
                // the exact joint path until that family has a separate proof.
                match crate::json::parse_with(text, governor) {
                    Ok(_) => return Ok(false),
                    Err(ParseFailure::Resource(reason)) => return Err(reason),
                    Err(ParseFailure::Syntax(_)) => {}
                }
            }
            _ => {}
        }
        let body = self.schema.body(schema_id);
        if let Some(reference) = body.get("$ref").and_then(Node::as_str) {
            let base = &self.schema.index.locations[schema_id].base;
            work(governor, base.len().saturating_add(reference.len()))?;
            let target = self
                .schema
                .resolve(base, reference)
                .map_err(|error| StopReason::InvalidInput(error.to_string()))?;
            self.unary(id, Kind::Reference, 0, target, occurrence, governor)?;
            return Ok(true);
        }
        match body.get("type").and_then(Node::as_str) {
            Some("object") => self.object(id, governor),
            Some("array") => {
                self.array(id, governor)?;
                Ok(true)
            }
            Some("string") => {
                let cost = match node.value {
                    Value::String(_) => Some((Kind::Product, 0)),
                    Value::Null | Value::Bool(_) | Value::Number(_) => Some((Kind::Other, 8)),
                    _ => None,
                };
                if let Some((kind, cost)) = cost {
                    let edge = Edge::new(kind, cost, governor);
                    self.states.as_mut_slice()[id].edges.push(edge)?;
                }
                Ok(true)
            }
            _ => unreachable!("certified type"),
        }
    }
    fn object(&mut self, id: usize, governor: &mut Governor) -> Result<bool, StopReason> {
        let schema_id = self.states[id].schema;
        let occurrence = self.states[id].occurrence;
        let Value::Object(members) = &self.occurrences[occurrence].node.value else {
            return Ok(true);
        };
        let body = self.schema.body(schema_id);
        let properties = match body.get("properties").map(|node| &node.value) {
            Some(Value::Object(properties)) => properties.as_slice(),
            _ => &[],
        };
        // Duplicates, rename and relocation can couple otherwise independent
        // members. Certify their absence using the existing name-cost authority.
        for (position, member) in members.iter().enumerate() {
            if member_position(&members[..position], &member.key, governor)?.is_some() {
                return Ok(false);
            }
            if member_position(properties, &member.key, governor)?.is_none() {
                for property in properties {
                    if member_position(members, &property.key, governor)?.is_none()
                        && name_cost(&member.key, &property.key, governor)?.is_some()
                    {
                        return Ok(false);
                    }
                }
            }
        }
        let mut required_present = true;
        if let Some(Value::Array(required)) = body.get("required").map(|node| &node.value) {
            for name in required.iter().filter_map(Node::as_str) {
                if member_position(members, name, governor)?.is_some() {
                    continue;
                }
                required_present = false;
                for member in members {
                    work(governor, 0)?;
                    if name_cost(&member.key, name, governor)?.is_some() {
                        return Ok(false);
                    }
                    if let Value::Object(nested) = &member.value.value
                        && member_position(nested, name, governor)?.is_some()
                    {
                        return Ok(false);
                    }
                }
            }
        }
        if !required_present {
            // A child unwrap could expose a future relocation source. Do not
            // normalize that coupled ordering into a product certificate.
            for member in members {
                work(governor, 0)?;
                if matches!(member.value.value, Value::Array(_) | Value::Object(_)) {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        let mut edge = Edge::new(Kind::Product, 0, governor);
        for (position, member) in members.iter().enumerate() {
            if let Some(property) = member_position(properties, &member.key, governor)? {
                let keyword = child_path("properties", &properties[property].key, governor)?;
                let child = self.schema.child(schema_id, &keyword);
                edge.children.push(self.state(
                    child,
                    self.occurrences[occurrence].children[position],
                    governor,
                )?)?;
                edge.positions.push(position)?;
            }
        }
        self.states.as_mut_slice()[id].edges.push(edge)?;
        Ok(true)
    }
    fn array(&mut self, id: usize, governor: &mut Governor) -> Result<(), StopReason> {
        let schema_id = self.states[id].schema;
        let occurrence = self.states[id].occurrence;
        let node = self.occurrences[occurrence].node;
        let item_schema = self
            .schema
            .body(schema_id)
            .get("items")
            .map(|_| self.schema.child(schema_id, "items"));
        let mut edge = Edge::new(Kind::Product, 0, governor);
        if let Value::Array(items) = &node.value {
            if let Some(child) = item_schema {
                for position in 0..items.len() {
                    edge.children.push(self.state(
                        child,
                        self.occurrences[occurrence].children[position],
                        governor,
                    )?)?;
                    edge.positions.push(position)?;
                }
            }
        } else {
            edge.kind = Kind::Wrap;
            edge.cost = WRAP_COST;
            if let Some(child) = item_schema {
                edge.children
                    .push(self.state(child, occurrence, governor)?)?;
            }
        }
        self.states.as_mut_slice()[id].edges.push(edge)
    }
    fn edge_cost(&self, edge: &Edge, governor: &mut Governor) -> Result<Option<u64>, StopReason> {
        let mut cost = edge.cost;
        for &child in &edge.children {
            work(governor, 1)?;
            let Some(child) = self.states[child].cost else {
                return Ok(None);
            };
            cost = cost.checked_add(child).ok_or_else(overflow)?;
        }
        Ok(Some(cost))
    }
    fn solve(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        loop {
            let mut changed = false;
            for id in (0..self.states.len()).rev() {
                work(governor, 1)?;
                let mut best = self.states[id].cost;
                for edge in &self.states[id].edges {
                    if let Some(cost) = self.edge_cost(edge, governor)?
                        && best.is_none_or(|prior| cost < prior)
                    {
                        best = Some(cost);
                    }
                }
                if best != self.states[id].cost {
                    self.states.as_mut_slice()[id].cost = best;
                    changed = true;
                }
            }
            if !changed {
                return Ok(());
            }
        }
    }
    fn prove(&self, governor: &mut Governor) -> Result<ScratchVec<bool>, StopReason> {
        let mut proved = ScratchVec::new(governor);
        proved.extend(core::iter::repeat_n(false, self.states.len()))?;
        loop {
            let mut changed = false;
            for id in (0..self.states.len()).rev() {
                work(governor, 1)?;
                if proved[id] || self.states[id].cost.is_none() {
                    continue;
                }
                let mut all = true;
                let mut any = false;
                for edge in &self.states[id].edges {
                    if self.edge_cost(edge, governor)? == self.states[id].cost {
                        any = true;
                        all &= edge.kind != Kind::Other
                            && edge.children.iter().all(|&child| proved[child]);
                    }
                }
                if any && all {
                    proved.as_mut_slice()[id] = true;
                    changed = true;
                }
            }
            if !changed {
                return Ok(proved);
            }
        }
    }
    fn emit(
        &self,
        id: usize,
        route: &[usize],
        path: &str,
        depth: u64,
        edits: &mut ScratchVec<Edit>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::Depth, depth)?;
        work(governor, 1)?;
        let state = &self.states[id];
        let mut selected = None;
        for edge in &state.edges {
            if self.edge_cost(edge, governor)? == state.cost {
                selected = Some(edge);
                break;
            }
        }
        let edge = selected.expect("proved state has a productive edge");
        if edge.kind == Kind::Wrap {
            let mut positions = ScratchVec::new(governor);
            work(governor, route.len())?;
            positions.extend(route.iter().copied())?;
            let text = format_scratch_governed(format_args!("{path}"), governor)?;
            edits.push(Edit {
                route: positions,
                path: text,
            })?;
        }
        for (position, &child) in edge.children.iter().enumerate() {
            if edge.kind == Kind::Reference {
                self.emit(child, route, path, depth + 1, edits, governor)?;
                continue;
            }
            let position = if edge.kind == Kind::Wrap {
                0
            } else {
                edge.positions[position]
            };
            let mut next = ScratchVec::new(governor);
            work(governor, route.len())?;
            next.extend(route.iter().copied())?;
            next.push(position)?;
            let next_path = match &self.occurrences[state.occurrence].node.value {
                Value::Object(members) if edge.kind != Kind::Wrap => {
                    child_path(path, &members[position].key, governor)?
                }
                _ => format_scratch_governed(format_args!("{path}/{position}"), governor)?,
            };
            self.emit(child, &next, &next_path, depth + 1, edits, governor)?;
        }
        Ok(())
    }
}

const _: () = {
    assert!(core::mem::size_of::<Edit>() <= Edit::CHARGE);
    assert!(core::mem::size_of::<Occurrence<'_>>() <= Occurrence::CHARGE);
    assert!(core::mem::size_of::<Edge>() <= Edge::CHARGE);
    assert!(core::mem::size_of::<State>() <= State::CHARGE);
};
