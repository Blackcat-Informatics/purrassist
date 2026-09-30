// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Stage 1 (canonicalize) and stage 2 (`not` elimination) of the Attouche,
//! Baazizi, Colazzo, Ghelli, Sartiani witness-generation pipeline (VLDB
//! 2022): [`build`] turns one compiled schema location into a [`Canon`]
//! tree whose only combinators are conjunction ([`Canon::And`]) and
//! disjunction ([`Canon::Or`]) over leaf [`Facet`]s, pushing every `not`
//! down to a leaf by De Morgan duality (`build_negated`) until it lands on
//! a single keyword, where a closed-form dual keyword exists for every
//! keyword this stage can decide exactly (`type`, `const`/`enum`,
//! numeric bounds, string length, `pattern`, `format`, array length,
//! `uniqueItems`). A keyword whose negation has no closed dual (the
//! structural object/array applicators: `properties`, `patternProperties`,
//! `additionalProperties`, `propertyNames`, `required`,
//! `dependentRequired`/`dependentSchemas`, `contains`,
//! `prefixItems`/`items`, `unevaluatedProperties`/`unevaluatedItems`, and
//! `oneOf`) falls back to [`Canon::OpaqueNot`]: "some candidate that fails
//! that exact sub-schema", verified later by the real evaluator rather than
//! decided symbolically. Because [`Canon::Or`] only ever proves emptiness
//! when *every* branch does, mixing a precise closed-form branch with an
//! opaque fallback branch never lets an opaque, unprovable branch launder a
//! false unsatisfiability claim: see `super::generate::attempt` on
//! [`Canon::Or`].
use super::super::Schema;
use super::invalid;
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::json::{Node, Value};

/// Which numbers a merged `type` constraint admits, distinguishing a bare
/// `"number"` (any) from `"integer"` (mathematically integral) from the
/// closed dual of `"integer"` (mathematically non-integral): see the module
/// documentation on why `integer` cannot be treated as a sibling of
/// `number` under negation the way `null`/`boolean`/... can.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NumKind {
    None,
    Any,
    IntOnly,
    FracOnly,
}
impl NumKind {
    const fn intersect(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, _)
            | (_, Self::None)
            | (Self::IntOnly, Self::FracOnly)
            | (Self::FracOnly, Self::IntOnly) => Self::None,
            (Self::Any, other) => other,
            (this, Self::Any) => this,
            (Self::IntOnly, Self::IntOnly) => Self::IntOnly,
            (Self::FracOnly, Self::FracOnly) => Self::FracOnly,
        }
    }
    const fn complement(self) -> Self {
        match self {
            Self::None => Self::Any,
            Self::Any => Self::None,
            Self::IntOnly => Self::FracOnly,
            Self::FracOnly => Self::IntOnly,
        }
    }
}

/// A merged `type` constraint: an instance is admitted if its JSON kind's
/// flag is set (numbers separately gated by [`NumKind`]).
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent admitted-shape dimension, not a state machine"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TypeSet {
    pub null: bool,
    pub boolean: bool,
    pub object: bool,
    pub array: bool,
    pub string: bool,
    pub num: NumKind,
}
impl TypeSet {
    pub(super) const fn universal() -> Self {
        Self {
            null: true,
            boolean: true,
            object: true,
            array: true,
            string: true,
            num: NumKind::Any,
        }
    }
    const fn empty() -> Self {
        Self {
            null: false,
            boolean: false,
            object: false,
            array: false,
            string: false,
            num: NumKind::None,
        }
    }
    pub(super) const fn intersect(self, other: Self) -> Self {
        Self {
            null: self.null && other.null,
            boolean: self.boolean && other.boolean,
            object: self.object && other.object,
            array: self.array && other.array,
            string: self.string && other.string,
            num: self.num.intersect(other.num),
        }
    }
    pub(super) const fn complement(self) -> Self {
        Self {
            null: !self.null,
            boolean: !self.boolean,
            object: !self.object,
            array: !self.array,
            string: !self.string,
            num: self.num.complement(),
        }
    }
    pub(super) const fn is_empty(self) -> bool {
        !self.null
            && !self.boolean
            && !self.object
            && !self.array
            && !self.string
            && matches!(self.num, NumKind::None)
    }
    fn from_name(name: &str) -> Self {
        let mut set = Self::empty();
        match name {
            "null" => set.null = true,
            "boolean" => set.boolean = true,
            "object" => set.object = true,
            "array" => set.array = true,
            "string" => set.string = true,
            "number" => set.num = NumKind::Any,
            "integer" => set.num = NumKind::IntOnly,
            _ => {}
        }
        set
    }
    fn from_node(node: &Node) -> Self {
        match &node.value {
            Value::String(name) => Self::from_name(name),
            Value::Array(names) => {
                names
                    .iter()
                    .filter_map(Node::as_str)
                    .fold(Self::empty(), |set, name| {
                        let flags = Self::from_name(name);
                        Self {
                            null: set.null || flags.null,
                            boolean: set.boolean || flags.boolean,
                            object: set.object || flags.object,
                            array: set.array || flags.array,
                            string: set.string || flags.string,
                            num: match (set.num, flags.num) {
                                (NumKind::None, other) | (other, NumKind::None) => other,
                                (NumKind::Any, _) | (_, NumKind::Any) => NumKind::Any,
                                (a, b) if a == b => a,
                                // "integer" and non-integer both listed: any number.
                                _ => NumKind::Any,
                            },
                        }
                    })
            }
            _ => Self::universal(),
        }
    }
}

/// A location of a compiled `pattern`/`patternProperties` regular
/// expression: the schema location that owns it, plus which of its
/// keywords the compiled [`crate::pattern::Pattern`] is filed under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PatternRef {
    pub owner: usize,
    pub keyword: String,
}

/// One leaf keyword-level assertion; every field is already resolved from
/// the compiled schema (child ids, decoded property/required name lists),
/// so stage 5 (generation) never re-reads raw JSON.
#[derive(Clone, Debug)]
pub(super) enum Facet {
    Type(TypeSet),
    Const(Node),
    /// Values the instance must not equal (the closed dual of `const`/`enum`).
    Excluded(Vec<Node>),
    Enum(Vec<Node>),
    Minimum(crate::json::Number, bool),
    Maximum(crate::json::Number, bool),
    MultipleOf(crate::json::Number),
    NotMultipleOf(crate::json::Number),
    MinLength(u64),
    MaxLength(u64),
    Pattern(PatternRef),
    AvoidPattern(PatternRef),
    Format(String),
    AvoidFormat(String),
    MinItems(u64),
    MaxItems(u64),
    UniqueItems,
    RequireDuplicate,
    PrefixItems(Vec<usize>),
    /// `items`'s target, paired with how many leading elements *this same
    /// schema node's own* `prefixItems` (0 if it has none) already covers —
    /// `items` applies only beyond that count. Keeping the pairing here,
    /// rather than merging every `items`/`prefixItems` occurrence from every
    /// `allOf` branch into one shared prefix length, matters whenever
    /// `prefixItems` and `items` are declared on *different* schema objects
    /// (e.g. `items` at the root, `prefixItems` inside an `allOf` branch):
    /// `items` in that case still applies from index 0, since its own node
    /// declares no `prefixItems` of its own.
    Items(usize, usize),
    Contains(usize),
    MinContains(u64),
    MaxContains(u64),
    MinProperties(u64),
    MaxProperties(u64),
    Properties(Vec<(String, usize)>),
    PatternProperties(Vec<(PatternRef, usize)>),
    AdditionalProperties(usize),
    PropertyNames(usize),
    Required(Vec<String>),
    DependentRequired(Vec<(String, Vec<String>)>),
    DependentSchemas(Vec<(String, usize)>),
    UnevaluatedProperties(usize),
    UnevaluatedItems(usize),
}

/// The output of stages 1–2: an `And`/`Or` tree over [`Facet`] leaves, plus
/// deferred reference edges (`Ref`/`DynamicRef`, resolved with the live
/// dynamic scope during generation) and [`Canon::OpaqueNot`], the fallback
/// for a structural keyword this stage cannot dualize in closed form.
#[derive(Clone, Debug)]
pub(super) enum Canon {
    True,
    False,
    Local(Facet),
    /// A candidate must fail this exact compiled schema location, checked
    /// with the real evaluator (`super::super::evaluate::run`) rather than
    /// decided symbolically.
    OpaqueNot(usize),
    And(Vec<Self>),
    Or(Vec<Self>),
    /// `oneOf`'s branch schema ids, resolved lazily (mirroring [`Canon::Ref`]):
    /// exactly one must hold, generated by trying branch `i`'s own positive
    /// canon while adding every *other* branch as a [`Canon::OpaqueNot`]
    /// obligation (own the closed-form `not` dual would need to fully
    /// negate an arbitrary branch, which is exactly what `OpaqueNot` already
    /// does soundly; see `super::generate::Gen::attempt_one_of`).
    OneOf(Vec<usize>),
    IfThenElse {
        cond: usize,
        then: Option<usize>,
        otherwise: Option<usize>,
    },
    Ref(usize),
    DynamicRef(String, String),
    NegatedRef(usize),
    NegatedDynamicRef(String, String),
    /// `id`'s own base must be on the live dynamic-scope stack for exactly
    /// the duration of searching `inner`: every combinator branch
    /// (`allOf`/`anyOf`/`oneOf`/`not`, and `if`/`then`/`else` in
    /// `super::generate`) is a distinct schema location that may declare
    /// its own `$id`/`$dynamicAnchor`, and a `$dynamicRef` reached while
    /// searching it must resolve against a scope stack that includes (and,
    /// once this branch is done, excludes) that location — the same
    /// entered/left discipline `super::super::evaluate` applies to every
    /// location it evaluates, not only explicit `$ref` targets.
    Scoped(usize, Box<Self>),
}

/// Wrap `canon` (built for schema location `id`) so generation enters
/// `id`'s own dynamic scope for exactly the duration of searching it; see
/// [`Canon::Scoped`].
fn scoped(id: usize, canon: Canon) -> Canon {
    Canon::Scoped(id, Box::new(canon))
}

/// Stage 1: canonicalize one compiled schema location into positive
/// [`Canon`]. Recurses through `allOf`/`anyOf`/`oneOf`/`if`/`then`/`else`
/// (a finite, already-compiled syntax DAG) but never through `$ref`/
/// `$dynamicRef`, which stay opaque edges so a self- or mutually-recursive
/// schema cannot make this stage itself recurse without bound; generation
/// (stage 5) expands those edges under its own depth/cycle guard.
pub(super) fn build(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Canon, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let body = schema.keywords(id);
    if let Value::Bool(accept) = body.value {
        return Ok(if accept { Canon::True } else { Canon::False });
    }
    let mut parts = Vec::new();
    local_facets(schema, id, governor, &mut parts)?;
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = body.get("allOf")
    {
        for position in 0..children.len() {
            let child = schema.child(id, &format!("allOf/{position}"));
            parts.push(scoped(child, build(schema, child, governor)?));
        }
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = body.get("anyOf")
    {
        let mut branches = Vec::with_capacity(children.len());
        for position in 0..children.len() {
            let child = schema.child(id, &format!("anyOf/{position}"));
            branches.push(scoped(child, build(schema, child, governor)?));
        }
        parts.push(Canon::Or(branches));
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = body.get("oneOf")
    {
        let ids = (0..children.len())
            .map(|position| schema.child(id, &format!("oneOf/{position}")))
            .collect();
        parts.push(Canon::OneOf(ids));
    }
    if body.get("not").is_some() {
        let not_id = schema.child(id, "not");
        parts.push(scoped(not_id, build_negated(schema, not_id, governor)?));
    }
    if body.get("if").is_some() {
        parts.push(Canon::IfThenElse {
            cond: schema.child(id, "if"),
            then: body.get("then").map(|_| schema.child(id, "then")),
            otherwise: body.get("else").map(|_| schema.child(id, "else")),
        });
    }
    if let Some(reference) = body.get("$ref").and_then(Node::as_str) {
        let target = schema
            .resolve(&schema.index.locations[id].base, reference)
            .map_err(|error| invalid(&error))?;
        parts.push(Canon::Ref(target));
    }
    if let Some(reference) = body.get("$dynamicRef").and_then(Node::as_str) {
        parts.push(Canon::DynamicRef(
            reference.to_string(),
            schema.index.locations[id].base.clone(),
        ));
    }
    Ok(Canon::And(parts))
}

/// Stage 2: the closed-form dual of `build` for the same location, by De
/// Morgan duality on every combinator and the per-keyword duals documented
/// on [`Facet::negate`]; see the module documentation.
pub(super) fn build_negated(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Canon, StopReason> {
    governor.charge(Resource::Work, 1)?;
    let body = schema.keywords(id);
    if let Value::Bool(accept) = body.value {
        return Ok(if accept { Canon::False } else { Canon::True });
    }
    let mut branches = Vec::new();
    let mut needs_fallback = false;
    let mut local = Vec::new();
    local_facets(schema, id, governor, &mut local)?;
    for facet in local {
        let Canon::Local(facet) = facet else {
            unreachable!("local_facets only ever pushes Canon::Local")
        };
        match negate_facet(&facet) {
            Some(negated) => branches.push(negated),
            None => needs_fallback = true,
        }
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = body.get("allOf")
    {
        // not(A and B) = not(A) or not(B): flatten straight into the outer Or.
        for position in 0..children.len() {
            let child = schema.child(id, &format!("allOf/{position}"));
            branches.push(scoped(child, build_negated(schema, child, governor)?));
        }
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = body.get("anyOf")
    {
        // not(A or B) = not(A) and not(B): one conjunctive branch.
        let mut inner = Vec::with_capacity(children.len());
        for position in 0..children.len() {
            let child = schema.child(id, &format!("anyOf/{position}"));
            inner.push(scoped(child, build_negated(schema, child, governor)?));
        }
        branches.push(Canon::And(inner));
    }
    if body.get("oneOf").is_some() {
        // "zero, or two-or-more, branches match" has no useful closed form
        // here; the opaque fallback is exact (never a false witness) even
        // though it forfeits an unsatisfiability proof for this branch.
        needs_fallback = true;
    }
    if body.get("not").is_some() {
        // not(not(A)) = A.
        let not_id = schema.child(id, "not");
        branches.push(scoped(not_id, build(schema, not_id, governor)?));
    }
    if body.get("if").is_some() {
        let cond = schema.child(id, "if");
        let then_id = body.get("then").map(|_| schema.child(id, "then"));
        let else_id = body.get("else").map(|_| schema.child(id, "else"));
        // not(if C then T else E) = (C and not T) or (not C and not E).
        let not_then = match then_id {
            Some(then_id) => scoped(then_id, build_negated(schema, then_id, governor)?),
            None => Canon::False,
        };
        branches.push(Canon::And(vec![
            scoped(cond, build(schema, cond, governor)?),
            not_then,
        ]));
        let not_else = match else_id {
            Some(else_id) => scoped(else_id, build_negated(schema, else_id, governor)?),
            None => Canon::False,
        };
        branches.push(Canon::And(vec![
            scoped(cond, build_negated(schema, cond, governor)?),
            not_else,
        ]));
    }
    if let Some(reference) = body.get("$ref").and_then(Node::as_str) {
        let target = schema
            .resolve(&schema.index.locations[id].base, reference)
            .map_err(|error| invalid(&error))?;
        branches.push(Canon::NegatedRef(target));
    }
    if let Some(reference) = body.get("$dynamicRef").and_then(Node::as_str) {
        branches.push(Canon::NegatedDynamicRef(
            reference.to_string(),
            schema.index.locations[id].base.clone(),
        ));
    }
    if needs_fallback {
        branches.push(Canon::OpaqueNot(id));
    }
    Ok(Canon::Or(branches))
}

/// The closed dual of one keyword-level [`Facet`], or `None` when this
/// stage leaves that keyword's negation to [`Canon::OpaqueNot`]: see the
/// module documentation for exactly which keywords have one.
fn negate_facet(facet: &Facet) -> Option<Canon> {
    match facet {
        Facet::Type(set) => Some(Canon::Local(Facet::Type(set.complement()))),
        Facet::Const(value) => Some(Canon::Local(Facet::Excluded(vec![value.clone()]))),
        Facet::Enum(values) => Some(Canon::Local(Facet::Excluded(values.clone()))),
        Facet::Minimum(bound, exclusive) => {
            Some(Canon::Local(Facet::Maximum(bound.clone(), !exclusive)))
        }
        Facet::Maximum(bound, exclusive) => {
            Some(Canon::Local(Facet::Minimum(bound.clone(), !exclusive)))
        }
        Facet::MultipleOf(divisor) => Some(Canon::Local(Facet::NotMultipleOf(divisor.clone()))),
        Facet::NotMultipleOf(divisor) => Some(Canon::Local(Facet::MultipleOf(divisor.clone()))),
        Facet::MinLength(0) | Facet::MinItems(0) | Facet::MinProperties(0) => Some(Canon::False),
        Facet::MinLength(bound) => Some(Canon::Local(Facet::MaxLength(bound - 1))),
        Facet::MaxLength(bound) => Some(Canon::Local(Facet::MinLength(bound.saturating_add(1)))),
        Facet::Pattern(reference) => Some(Canon::Local(Facet::AvoidPattern(reference.clone()))),
        Facet::AvoidPattern(reference) => Some(Canon::Local(Facet::Pattern(reference.clone()))),
        Facet::Format(name) => Some(Canon::Local(Facet::AvoidFormat(name.clone()))),
        Facet::AvoidFormat(name) => Some(Canon::Local(Facet::Format(name.clone()))),
        Facet::MinItems(bound) => Some(Canon::Local(Facet::MaxItems(bound - 1))),
        Facet::MaxItems(bound) => Some(Canon::Local(Facet::MinItems(bound.saturating_add(1)))),
        Facet::UniqueItems => Some(Canon::Local(Facet::RequireDuplicate)),
        Facet::RequireDuplicate => Some(Canon::Local(Facet::UniqueItems)),
        Facet::MinProperties(bound) => Some(Canon::Local(Facet::MaxProperties(bound - 1))),
        Facet::MaxProperties(bound) => {
            Some(Canon::Local(Facet::MinProperties(bound.saturating_add(1))))
        }
        Facet::Excluded(_)
        | Facet::PrefixItems(_)
        | Facet::Items(_, _)
        | Facet::Contains(_)
        | Facet::MinContains(_)
        | Facet::MaxContains(_)
        | Facet::Properties(_)
        | Facet::PatternProperties(_)
        | Facet::AdditionalProperties(_)
        | Facet::PropertyNames(_)
        | Facet::Required(_)
        | Facet::DependentRequired(_)
        | Facet::DependentSchemas(_)
        | Facet::UnevaluatedProperties(_)
        | Facet::UnevaluatedItems(_) => None,
    }
}

/// Collect every keyword local to `id` (not an applicator) as
/// `Canon::Local` leaves, resolving child schema ids and decoding name
/// lists once so generation never re-reads raw JSON.
fn local_facets(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut Vec<Canon>,
) -> Result<(), StopReason> {
    scalar_facets(schema, id, out);
    array_facets(schema, id, out);
    object_facets(schema, id, governor, out)
}

/// The `type`/`const`/`enum`/numeric/string-shaped keywords local to `id`.
fn scalar_facets(schema: &Schema, id: usize, out: &mut Vec<Canon>) {
    let body = schema.keywords(id);
    if let Some(node) = body.get("type") {
        out.push(Canon::Local(Facet::Type(TypeSet::from_node(node))));
    }
    if let Some(value) = body.get("const") {
        out.push(Canon::Local(Facet::Const(value.clone())));
    }
    if let Some(Node {
        value: Value::Array(values),
        ..
    }) = body.get("enum")
    {
        out.push(Canon::Local(Facet::Enum(values.clone())));
    }
    for (keyword, exclusive) in [("minimum", false), ("exclusiveMinimum", true)] {
        if let Some(Node {
            value: Value::Number(bound),
            ..
        }) = body.get(keyword)
        {
            out.push(Canon::Local(Facet::Minimum(bound.clone(), exclusive)));
        }
    }
    for (keyword, exclusive) in [("maximum", false), ("exclusiveMaximum", true)] {
        if let Some(Node {
            value: Value::Number(bound),
            ..
        }) = body.get(keyword)
        {
            out.push(Canon::Local(Facet::Maximum(bound.clone(), exclusive)));
        }
    }
    if let Some(Node {
        value: Value::Number(divisor),
        ..
    }) = body.get("multipleOf")
    {
        out.push(Canon::Local(Facet::MultipleOf(divisor.clone())));
    }
    if let Some(count) = as_u64(body.get("minLength")) {
        out.push(Canon::Local(Facet::MinLength(count)));
    }
    if let Some(count) = as_u64(body.get("maxLength")) {
        out.push(Canon::Local(Facet::MaxLength(count)));
    }
    if schema.index.locations[id].patterns.contains_key("pattern") {
        out.push(Canon::Local(Facet::Pattern(PatternRef {
            owner: id,
            keyword: "pattern".into(),
        })));
    }
    if let Some(name) = body.get("format").and_then(Node::as_str) {
        out.push(Canon::Local(Facet::Format(name.to_string())));
    }
}

/// The array-shaped keywords local to `id`.
fn array_facets(schema: &Schema, id: usize, out: &mut Vec<Canon>) {
    let body = schema.keywords(id);
    if let Some(count) = as_u64(body.get("minItems")) {
        out.push(Canon::Local(Facet::MinItems(count)));
    }
    if let Some(count) = as_u64(body.get("maxItems")) {
        out.push(Canon::Local(Facet::MaxItems(count)));
    }
    if body
        .get("uniqueItems")
        .is_some_and(|node| node.value == Value::Bool(true))
    {
        out.push(Canon::Local(Facet::UniqueItems));
    }
    let mut own_prefix_len = 0;
    if let Some(Node {
        value: Value::Array(prefix),
        ..
    }) = body.get("prefixItems")
    {
        own_prefix_len = prefix.len();
        let mut ids = Vec::with_capacity(prefix.len());
        for position in 0..prefix.len() {
            ids.push(schema.child(id, &format!("prefixItems/{position}")));
        }
        out.push(Canon::Local(Facet::PrefixItems(ids)));
    }
    if body.get("items").is_some() {
        out.push(Canon::Local(Facet::Items(
            schema.child(id, "items"),
            own_prefix_len,
        )));
    }
    if body.get("contains").is_some() {
        out.push(Canon::Local(Facet::Contains(schema.child(id, "contains"))));
    }
    if let Some(count) = as_u64(body.get("minContains")) {
        out.push(Canon::Local(Facet::MinContains(count)));
    }
    if let Some(count) = as_u64(body.get("maxContains")) {
        out.push(Canon::Local(Facet::MaxContains(count)));
    }
}

/// The `properties`/`patternProperties`/`additionalProperties`/
/// `propertyNames` keywords local to `id`.
fn object_property_facets(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut Vec<Canon>,
) -> Result<(), StopReason> {
    let body = schema.keywords(id);
    if let Some(Node {
        value: Value::Object(properties),
        ..
    }) = body.get("properties")
    {
        governor.charge(Resource::Work, size(properties.len()))?;
        let mut pairs = Vec::with_capacity(properties.len());
        for property in properties {
            pairs.push((
                property.key.clone(),
                schema.child(
                    id,
                    &format!("properties/{}", crate::json::pointer_segment(&property.key)),
                ),
            ));
        }
        out.push(Canon::Local(Facet::Properties(pairs)));
    }
    if let Some(Node {
        value: Value::Object(patterns),
        ..
    }) = body.get("patternProperties")
    {
        governor.charge(Resource::Work, size(patterns.len()))?;
        let mut pairs = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            let keyword = format!(
                "patternProperties/{}",
                crate::json::pointer_segment(&pattern.key)
            );
            pairs.push((
                PatternRef {
                    owner: id,
                    keyword: keyword.clone(),
                },
                schema.child(id, &keyword),
            ));
        }
        out.push(Canon::Local(Facet::PatternProperties(pairs)));
    }
    if body.get("additionalProperties").is_some() {
        out.push(Canon::Local(Facet::AdditionalProperties(
            schema.child(id, "additionalProperties"),
        )));
    }
    if body.get("propertyNames").is_some() {
        out.push(Canon::Local(Facet::PropertyNames(
            schema.child(id, "propertyNames"),
        )));
    }
    Ok(())
}

/// The object-shaped keywords local to `id`.
fn object_facets(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut Vec<Canon>,
) -> Result<(), StopReason> {
    let body = schema.keywords(id);
    if let Some(count) = as_u64(body.get("minProperties")) {
        out.push(Canon::Local(Facet::MinProperties(count)));
    }
    if let Some(count) = as_u64(body.get("maxProperties")) {
        out.push(Canon::Local(Facet::MaxProperties(count)));
    }
    object_property_facets(schema, id, governor, out)?;
    if let Some(Node {
        value: Value::Array(required),
        ..
    }) = body.get("required")
    {
        out.push(Canon::Local(Facet::Required(
            required
                .iter()
                .filter_map(Node::as_str)
                .map(str::to_string)
                .collect(),
        )));
    }
    if let Some(Node {
        value: Value::Object(dependencies),
        ..
    }) = body.get("dependentRequired")
    {
        let mut pairs = Vec::with_capacity(dependencies.len());
        for dependency in dependencies {
            if let Value::Array(required) = &dependency.value.value {
                pairs.push((
                    dependency.key.clone(),
                    required
                        .iter()
                        .filter_map(Node::as_str)
                        .map(str::to_string)
                        .collect(),
                ));
            }
        }
        out.push(Canon::Local(Facet::DependentRequired(pairs)));
    }
    if let Some(Node {
        value: Value::Object(dependencies),
        ..
    }) = body.get("dependentSchemas")
    {
        let mut pairs = Vec::with_capacity(dependencies.len());
        for dependency in dependencies {
            pairs.push((
                dependency.key.clone(),
                schema.child(
                    id,
                    &format!(
                        "dependentSchemas/{}",
                        crate::json::pointer_segment(&dependency.key)
                    ),
                ),
            ));
        }
        out.push(Canon::Local(Facet::DependentSchemas(pairs)));
    }
    if body.get("unevaluatedProperties").is_some() {
        out.push(Canon::Local(Facet::UnevaluatedProperties(
            schema.child(id, "unevaluatedProperties"),
        )));
    }
    if body.get("unevaluatedItems").is_some() {
        out.push(Canon::Local(Facet::UnevaluatedItems(
            schema.child(id, "unevaluatedItems"),
        )));
    }
    Ok(())
}

/// Decode a compiled non-negative length/count bound. A value too large for
/// `usize` (an astronomically large `minLength`/`minItems`/... spelling)
/// clamps to `u64::MAX` rather than silently dropping the bound: generation
/// still tries to honor it and fails with an explicit resource exhaustion
/// once it actually attempts to build something that large, instead of
/// quietly treating an unrepresentable bound as no constraint at all.
fn as_u64(node: Option<&Node>) -> Option<u64> {
    let Node {
        value: Value::Number(number),
        ..
    } = node?
    else {
        return None;
    };
    Some(number.to_usize().map_or(u64::MAX, size))
}
