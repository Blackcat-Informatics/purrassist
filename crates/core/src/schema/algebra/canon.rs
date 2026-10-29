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
use super::super::index::child;
use super::super::layout::Dependency;
use super::super::{index, reference};
use super::storage::{self, CopyIn, list, push};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::contract::{ScratchCharge, ScratchVec, format_scratch_governed};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PatternRef<'s> {
    pub owner: usize,
    pub keyword: &'s str,
}

/// One leaf keyword-level assertion; every field is already resolved from
/// the compiled schema (child ids, decoded property/required name lists),
/// so stage 5 (generation) never re-reads raw JSON.
#[derive(Debug)]
pub(super) enum Facet<'s> {
    Type(TypeSet),
    Const(&'s Node),
    /// Values the instance must not equal (the closed dual of `const`/`enum`).
    Excluded(&'s [Node]),
    Enum(&'s [Node]),
    Minimum(&'s crate::json::Number, bool),
    Maximum(&'s crate::json::Number, bool),
    MultipleOf(&'s crate::json::Number),
    NotMultipleOf(&'s crate::json::Number),
    MinLength(u64),
    MaxLength(u64),
    Pattern(PatternRef<'s>),
    AvoidPattern(PatternRef<'s>),
    Format(&'s str),
    AvoidFormat(&'s str),
    MinItems(u64),
    MaxItems(u64),
    UniqueItems,
    RequireDuplicate,
    PrefixItems(ScratchVec<usize>),
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
    /// A `contains` occurrence and only that occurrence's cardinality bounds.
    /// Bounds on another schema object (or without `contains`) do not apply.
    Contains(usize, u64, Option<u64>),
    MinProperties(u64),
    MaxProperties(u64),
    Properties(ScratchVec<(&'s str, usize)>),
    PatternProperties(ScratchVec<(PatternRef<'s>, usize)>),
    AdditionalProperties(usize),
    PropertyNames(usize),
    Required(ScratchVec<&'s str>),
    DependentRequired(ScratchVec<(&'s str, ScratchVec<&'s str>)>),
    DependentSchemas(ScratchVec<(&'s str, usize)>),
    UnevaluatedProperties(usize),
    UnevaluatedItems(usize),
}

/// The output of stages 1–2: an `And`/`Or` tree over [`Facet`] leaves, plus
/// deferred reference edges (`Ref`/`DynamicRef`, resolved with the live
/// dynamic scope during generation) and [`Canon::OpaqueNot`], the fallback
/// for a structural keyword this stage cannot dualize in closed form.
#[derive(Debug)]
pub(super) enum Canon<'s> {
    True,
    False,
    Local(Facet<'s>),
    /// A candidate must fail this exact compiled schema location, checked
    /// with the real evaluator (`super::super::evaluate::run`) rather than
    /// decided symbolically.
    OpaqueNot(usize),
    And(ScratchVec<Self>),
    Or(ScratchVec<Self>),
    /// `oneOf`'s branch schema ids, resolved lazily (mirroring [`Canon::Ref`]):
    /// exactly one must hold, generated by trying branch `i`'s own positive
    /// canon while adding every *other* branch as a [`Canon::OpaqueNot`]
    /// obligation (own the closed-form `not` dual would need to fully
    /// negate an arbitrary branch, which is exactly what `OpaqueNot` already
    /// does soundly; see `super::generate::Gen::attempt_one_of`).
    OneOf(ScratchVec<usize>),
    IfThenElse {
        cond: usize,
        then: Option<usize>,
        otherwise: Option<usize>,
    },
    Ref(usize),
    DynamicRef(usize),
    NegatedRef(usize),
    NegatedDynamicRef(usize),
    /// `id`'s own base must be on the live dynamic-scope stack for exactly
    /// the duration of searching `inner`: every combinator branch
    /// (`allOf`/`anyOf`/`oneOf`/`not`, and `if`/`then`/`else` in
    /// `super::generate`) is a distinct schema location that may declare
    /// its own `$id`/`$dynamicAnchor`, and a `$dynamicRef` reached while
    /// searching it must resolve against a scope stack that includes (and,
    /// once this branch is done, excludes) that location — the same
    /// entered/left discipline `super::super::evaluate` applies to every
    /// location it evaluates, not only explicit `$ref` targets.
    Scoped(usize, ScratchVec<Self>),
}

/// Wrap `canon` (built for schema location `id`) so generation enters
/// `id`'s own dynamic scope for exactly the duration of searching it; see
/// [`Canon::Scoped`].
pub(super) fn scoped<'s>(
    id: usize,
    canon: Canon<'s>,
    governor: &mut Governor,
) -> Result<Canon<'s>, StopReason> {
    Ok(Canon::Scoped(id, storage::singleton(canon, governor)?))
}

/// Stage 1: canonicalize one compiled schema location into positive
/// [`Canon`]. Recurses through `allOf`/`anyOf`/`oneOf`/`if`/`then`/`else`
/// (a finite, already-compiled syntax DAG) but never through `$ref`/
/// `$dynamicRef`, which stay opaque edges so a self- or mutually-recursive
/// schema cannot make this stage itself recurse without bound; generation
/// (stage 5) expands those edges under its own depth/cycle guard.
fn build_at<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    depth: u32,
) -> Result<Canon<'s>, StopReason> {
    storage::depth(governor, depth)?;
    governor.charge(Resource::Work, 1)?;
    reference::work(governor, schema.index.locations[id].route.len())?;
    let body = schema.keywords(id);
    if let Value::Bool(accept) = body.value {
        return Ok(if accept { Canon::True } else { Canon::False });
    }
    storage::depth(governor, depth.saturating_add(1))?;
    let mut parts = ScratchVec::new(governor);
    local_facets(schema, id, governor, &mut parts)?;
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = get(&body, "allOf", governor)?
    {
        for position in 0..children.len() {
            let child = child(
                schema,
                id,
                &format_scratch_governed(format_args!("allOf/{position}"), governor)?,
                governor,
            )?;
            push(
                &mut parts,
                scoped(
                    child,
                    build_at(schema, child, governor, depth.saturating_add(2))?,
                    governor,
                )?,
                governor,
            )?;
        }
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = get(&body, "anyOf", governor)?
    {
        let mut branches = ScratchVec::new(governor);
        for position in 0..children.len() {
            let child = child(
                schema,
                id,
                &format_scratch_governed(format_args!("anyOf/{position}"), governor)?,
                governor,
            )?;
            push(
                &mut branches,
                scoped(
                    child,
                    build_at(schema, child, governor, depth.saturating_add(3))?,
                    governor,
                )?,
                governor,
            )?;
        }
        push(&mut parts, Canon::Or(branches), governor)?;
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = get(&body, "oneOf", governor)?
    {
        let mut ids = ScratchVec::new(governor);
        for position in 0..children.len() {
            let key = format_scratch_governed(format_args!("oneOf/{position}"), governor)?;
            let id = child(schema, id, &key, governor)?;
            push(&mut ids, id, governor)?;
        }
        push(&mut parts, Canon::OneOf(ids), governor)?;
    }
    if get(&body, "not", governor)?.is_some() {
        let not_id = child(schema, id, "not", governor)?;
        push(
            &mut parts,
            scoped(
                not_id,
                build_negated_at(schema, not_id, governor, depth.saturating_add(2))?,
                governor,
            )?,
            governor,
        )?;
    }
    if get(&body, "if", governor)?.is_some() {
        push(
            &mut parts,
            Canon::IfThenElse {
                cond: child(schema, id, "if", governor)?,
                then: get(&body, "then", governor)?
                    .map(|_| child(schema, id, "then", governor))
                    .transpose()?,
                otherwise: get(&body, "else", governor)?
                    .map(|_| child(schema, id, "else", governor))
                    .transpose()?,
            },
            governor,
        )?;
    }
    reference_facets(schema, id, &body, governor, false, &mut parts)?;
    Ok(Canon::And(parts))
}

/// Stage 2: the closed-form dual of `build` for the same location, by De
/// Morgan duality on every combinator and the per-keyword duals documented
/// on [`negate_facet`]; see the module documentation.
fn build_negated_at<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    depth: u32,
) -> Result<Canon<'s>, StopReason> {
    storage::depth(governor, depth)?;
    governor.charge(Resource::Work, 1)?;
    reference::work(governor, schema.index.locations[id].route.len())?;
    let body = schema.keywords(id);
    if let Value::Bool(accept) = body.value {
        return Ok(if accept { Canon::False } else { Canon::True });
    }
    storage::depth(governor, depth.saturating_add(2))?;
    let mut branches = ScratchVec::new(governor);
    let mut needs_fallback = false;
    let mut local = ScratchVec::new(governor);
    local_facets(schema, id, governor, &mut local)?;
    for facet in &local {
        let Canon::Local(facet) = facet else {
            unreachable!("local_facets only ever pushes Canon::Local")
        };
        match negate_facet(facet, governor)? {
            Some(negated) => push(&mut branches, negated, governor)?,
            None => needs_fallback = true,
        }
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = get(&body, "allOf", governor)?
    {
        // not(A and B) = not(A) or not(B): flatten straight into the outer Or.
        for position in 0..children.len() {
            let child = child(
                schema,
                id,
                &format_scratch_governed(format_args!("allOf/{position}"), governor)?,
                governor,
            )?;
            push(
                &mut branches,
                scoped(
                    child,
                    build_negated_at(schema, child, governor, depth.saturating_add(2))?,
                    governor,
                )?,
                governor,
            )?;
        }
    }
    if let Some(Node {
        value: Value::Array(children),
        ..
    }) = get(&body, "anyOf", governor)?
    {
        // not(A or B) = not(A) and not(B): one conjunctive branch.
        let mut inner = ScratchVec::new(governor);
        for position in 0..children.len() {
            let child = child(
                schema,
                id,
                &format_scratch_governed(format_args!("anyOf/{position}"), governor)?,
                governor,
            )?;
            push(
                &mut inner,
                scoped(
                    child,
                    build_negated_at(schema, child, governor, depth.saturating_add(3))?,
                    governor,
                )?,
                governor,
            )?;
        }
        push(&mut branches, Canon::And(inner), governor)?;
    }
    if get(&body, "oneOf", governor)?.is_some() {
        // "zero, or two-or-more, branches match" has no useful closed form
        // here; the opaque fallback is exact (never a false witness) even
        // though it forfeits an unsatisfiability proof for this branch.
        needs_fallback = true;
    }
    if get(&body, "not", governor)?.is_some() {
        // not(not(A)) = A.
        let not_id = child(schema, id, "not", governor)?;
        push(
            &mut branches,
            scoped(
                not_id,
                build_at(schema, not_id, governor, depth.saturating_add(2))?,
                governor,
            )?,
            governor,
        )?;
    }
    negated_conditional(schema, id, &body, governor, depth, &mut branches)?;
    reference_facets(schema, id, &body, governor, true, &mut branches)?;
    if needs_fallback {
        push(&mut branches, Canon::OpaqueNot(id), governor)?;
    }
    Ok(Canon::Or(branches))
}

/// Reference edges stay opaque until the branch driver enters their scope.
fn reference_facets<'s>(
    schema: &'s Schema,
    id: usize,
    body: &super::super::vocabulary::Keywords<'s>,
    governor: &mut Governor,
    negated: bool,
    output: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    if let Some(reference) = get(body, "$ref", governor)?.and_then(Node::as_str) {
        let target = super::super::reference::resolve(
            schema,
            &schema.index.locations[id].base,
            reference,
            governor,
        )?;
        let edge = if negated {
            Canon::NegatedRef(target)
        } else {
            Canon::Ref(target)
        };
        push(output, edge, governor)?;
    }
    if get(body, "$dynamicRef", governor)?
        .and_then(Node::as_str)
        .is_some()
    {
        let edge = if negated {
            Canon::NegatedDynamicRef(id)
        } else {
            Canon::DynamicRef(id)
        };
        push(output, edge, governor)?;
    }
    Ok(())
}

/// Complement an if/then/else with the same ordered two-branch expansion.
fn negated_conditional<'s>(
    schema: &'s Schema,
    id: usize,
    body: &super::super::vocabulary::Keywords<'s>,
    governor: &mut Governor,
    depth: u32,
    branches: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    if get(body, "if", governor)?.is_some() {
        let cond = child(schema, id, "if", governor)?;
        let then_id = get(body, "then", governor)?
            .map(|_| child(schema, id, "then", governor))
            .transpose()?;
        let else_id = get(body, "else", governor)?
            .map(|_| child(schema, id, "else", governor))
            .transpose()?;
        // not(if C then T else E) = (C and not T) or (not C and not E).
        let not_then = match then_id {
            Some(then_id) => scoped(
                then_id,
                build_negated_at(schema, then_id, governor, depth.saturating_add(3))?,
                governor,
            )?,
            None => Canon::False,
        };
        push(
            branches,
            Canon::And(list(
                [
                    scoped(
                        cond,
                        build_at(schema, cond, governor, depth.saturating_add(3))?,
                        governor,
                    )?,
                    not_then,
                ],
                governor,
            )?),
            governor,
        )?;
        let not_else = match else_id {
            Some(else_id) => scoped(
                else_id,
                build_negated_at(schema, else_id, governor, depth.saturating_add(3))?,
                governor,
            )?,
            None => Canon::False,
        };
        push(
            branches,
            Canon::And(list(
                [
                    scoped(
                        cond,
                        build_negated_at(schema, cond, governor, depth.saturating_add(3))?,
                        governor,
                    )?,
                    not_else,
                ],
                governor,
            )?),
            governor,
        )?;
    }
    Ok(())
}

/// The closed dual of one keyword-level [`Facet`], or `None` when this
/// stage leaves that keyword's negation to [`Canon::OpaqueNot`]: see the
/// module documentation for exactly which keywords have one.
fn negate_facet<'s>(
    facet: &Facet<'s>,
    governor: &mut Governor,
) -> Result<Option<Canon<'s>>, StopReason> {
    // A shape-specific keyword accepts every instance of another shape.
    // Its complement therefore requires the affected shape as well as the
    // complemented assertion; dualizing only the bound is not equivalent.
    let typed = |name, facet, governor: &mut Governor| {
        Ok::<_, StopReason>(Canon::And(list(
            [
                Canon::Local(Facet::Type(TypeSet::from_name(name))),
                Canon::Local(facet),
            ],
            governor,
        )?))
    };
    Ok(match facet {
        Facet::Type(set) => Some(Canon::Local(Facet::Type(set.complement()))),
        Facet::Const(value) => Some(Canon::Local(Facet::Excluded(core::slice::from_ref(*value)))),
        Facet::Enum(values) => Some(Canon::Local(Facet::Excluded(values))),
        Facet::Minimum(bound, exclusive) => Some(typed(
            "number",
            Facet::Maximum(bound, !exclusive),
            governor,
        )?),
        Facet::Maximum(bound, exclusive) => Some(typed(
            "number",
            Facet::Minimum(bound, !exclusive),
            governor,
        )?),
        Facet::MultipleOf(divisor) => {
            Some(typed("number", Facet::NotMultipleOf(divisor), governor)?)
        }
        Facet::NotMultipleOf(divisor) => {
            Some(typed("number", Facet::MultipleOf(divisor), governor)?)
        }
        Facet::MinLength(0) | Facet::MinItems(0) | Facet::MinProperties(0) => Some(Canon::False),
        Facet::MinLength(bound) => Some(typed("string", Facet::MaxLength(bound - 1), governor)?),
        Facet::MaxLength(bound) => Some(typed(
            "string",
            Facet::MinLength(bound.saturating_add(1)),
            governor,
        )?),
        Facet::Pattern(reference) => {
            Some(typed("string", Facet::AvoidPattern(*reference), governor)?)
        }
        Facet::AvoidPattern(reference) => {
            Some(typed("string", Facet::Pattern(*reference), governor)?)
        }
        Facet::Format(name) => Some(typed("string", Facet::AvoidFormat(name), governor)?),
        Facet::AvoidFormat(name) => Some(typed("string", Facet::Format(name), governor)?),
        Facet::MinItems(bound) => Some(typed("array", Facet::MaxItems(bound - 1), governor)?),
        Facet::MaxItems(bound) => Some(typed(
            "array",
            Facet::MinItems(bound.saturating_add(1)),
            governor,
        )?),
        Facet::UniqueItems => Some(typed("array", Facet::RequireDuplicate, governor)?),
        Facet::RequireDuplicate => Some(typed("array", Facet::UniqueItems, governor)?),
        Facet::MinProperties(bound) => {
            Some(typed("object", Facet::MaxProperties(bound - 1), governor)?)
        }
        Facet::MaxProperties(bound) => Some(typed(
            "object",
            Facet::MinProperties(bound.saturating_add(1)),
            governor,
        )?),
        Facet::Excluded(_)
        | Facet::PrefixItems(_)
        | Facet::Items(_, _)
        | Facet::Contains(_, _, _)
        | Facet::Properties(_)
        | Facet::PatternProperties(_)
        | Facet::AdditionalProperties(_)
        | Facet::PropertyNames(_)
        | Facet::Required(_)
        | Facet::DependentRequired(_)
        | Facet::DependentSchemas(_)
        | Facet::UnevaluatedProperties(_)
        | Facet::UnevaluatedItems(_) => None,
    })
}

/// Collect every keyword local to `id` (not an applicator) as
/// `Canon::Local` leaves, resolving child schema ids and decoding name
/// lists once so generation never re-reads raw JSON.
fn local_facets<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    scalar_facets(schema, id, governor, out)?;
    array_facets(schema, id, governor, out)?;
    object_facets(schema, id, governor, out)
}

/// The `type`/`const`/`enum`/numeric/string-shaped keywords local to `id`.
fn scalar_facets<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    reference::work(governor, schema.index.locations[id].route.len())?;
    let body = schema.keywords(id);
    if let Some(node) = get(&body, "type", governor)? {
        push(
            out,
            Canon::Local(Facet::Type({
                reference::work(
                    governor,
                    match &node.value {
                        Value::Array(items) => items.len(),
                        _ => 1,
                    },
                )?;
                TypeSet::from_node(node)
            })),
            governor,
        )?;
    }
    if let Some(value) = get(&body, "const", governor)? {
        push(out, Canon::Local(Facet::Const(value)), governor)?;
    }
    if let Some(Node {
        value: Value::Array(values),
        ..
    }) = get(&body, "enum", governor)?
    {
        push(out, Canon::Local(Facet::Enum(values)), governor)?;
    }
    for (keyword, exclusive) in [("minimum", false), ("exclusiveMinimum", true)] {
        if let Some(Node {
            value: Value::Number(bound),
            ..
        }) = get(&body, keyword, governor)?
        {
            push(
                out,
                Canon::Local(Facet::Minimum(bound, exclusive)),
                governor,
            )?;
        }
    }
    for (keyword, exclusive) in [("maximum", false), ("exclusiveMaximum", true)] {
        if let Some(Node {
            value: Value::Number(bound),
            ..
        }) = get(&body, keyword, governor)?
        {
            push(
                out,
                Canon::Local(Facet::Maximum(bound, exclusive)),
                governor,
            )?;
        }
    }
    if let Some(Node {
        value: Value::Number(divisor),
        ..
    }) = get(&body, "multipleOf", governor)?
    {
        push(out, Canon::Local(Facet::MultipleOf(divisor)), governor)?;
    }
    if let Some(count) = as_u64(get(&body, "minLength", governor)?, governor)? {
        push(out, Canon::Local(Facet::MinLength(count)), governor)?;
    }
    if let Some(count) = as_u64(get(&body, "maxLength", governor)?, governor)? {
        push(out, Canon::Local(Facet::MaxLength(count)), governor)?;
    }
    if schema.index.locations[id]
        .patterns
        .get_governed("pattern", governor)?
        .is_some()
    {
        push(
            out,
            Canon::Local(Facet::Pattern(PatternRef {
                owner: id,
                keyword: "pattern",
            })),
            governor,
        )?;
    }
    // Annotation-only formats remain observations in the real evaluator.
    // Adding them here would constrain generation and their `not` dual,
    // incorrectly turning a failed annotation into a refutation.
    if body.vocabularies.format_assert
        && let Some(name) = get(&body, "format", governor)?.and_then(Node::as_str)
    {
        push(out, Canon::Local(Facet::Format(name)), governor)?;
    }
    Ok(())
}

/// The array-shaped keywords local to `id`.
fn array_facets<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    reference::work(governor, schema.index.locations[id].route.len())?;
    let body = schema.keywords(id);
    if let Some(count) = as_u64(get(&body, "minItems", governor)?, governor)? {
        push(out, Canon::Local(Facet::MinItems(count)), governor)?;
    }
    if let Some(count) = as_u64(get(&body, "maxItems", governor)?, governor)? {
        push(out, Canon::Local(Facet::MaxItems(count)), governor)?;
    }
    if get(&body, "uniqueItems", governor)?.is_some_and(|node| node.value == Value::Bool(true)) {
        push(out, Canon::Local(Facet::UniqueItems), governor)?;
    }
    // `prefixItems` + `items`, or Draft-07 tuple `items` + `additionalItems`.
    let layout = schema.item_layout(id, governor)?;
    if layout.prefix_len > 0 {
        let mut ids = ScratchVec::new(governor);
        for position in 0..layout.prefix_len {
            push(
                &mut ids,
                child(
                    schema,
                    id,
                    &format_scratch_governed(
                        format_args!("{}/{position}", layout.prefix),
                        governor,
                    )?,
                    governor,
                )?,
                governor,
            )?;
        }
        push(out, Canon::Local(Facet::PrefixItems(ids)), governor)?;
    }
    if let Some(rest) = layout.rest {
        push(
            out,
            Canon::Local(Facet::Items(
                child(schema, id, rest, governor)?,
                layout.prefix_len,
            )),
            governor,
        )?;
    }
    if get(&body, "contains", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::Contains(
                child(schema, id, "contains", governor)?,
                as_u64(get(&body, "minContains", governor)?, governor)?.unwrap_or(1),
                as_u64(get(&body, "maxContains", governor)?, governor)?,
            )),
            governor,
        )?;
    }
    Ok(())
}

/// The `properties`/`patternProperties`/`additionalProperties`/
/// `propertyNames` keywords local to `id`.
fn object_property_facets<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    reference::work(governor, schema.index.locations[id].route.len())?;
    let body = schema.keywords(id);
    if let Some(Node {
        value: Value::Object(properties),
        ..
    }) = get(&body, "properties", governor)?
    {
        governor.charge(Resource::Work, size(properties.len()))?;
        let mut pairs = ScratchVec::new(governor);
        for property in properties {
            push(
                &mut pairs,
                (
                    property.key.as_str(),
                    child(
                        schema,
                        id,
                        &index::pointer("properties", &property.key, governor)?,
                        governor,
                    )?,
                ),
                governor,
            )?;
        }
        push(out, Canon::Local(Facet::Properties(pairs)), governor)?;
    }
    if let Some(Node {
        value: Value::Object(patterns),
        ..
    }) = get(&body, "patternProperties", governor)?
    {
        governor.charge(Resource::Work, size(patterns.len()))?;
        let mut pairs = ScratchVec::new(governor);
        for pattern in patterns {
            let keyword = index::pointer("patternProperties", &pattern.key, governor)?;
            push(
                &mut pairs,
                (
                    PatternRef {
                        owner: id,
                        keyword: pattern_key(schema, id, &keyword, governor)?,
                    },
                    child(schema, id, &keyword, governor)?,
                ),
                governor,
            )?;
        }
        push(out, Canon::Local(Facet::PatternProperties(pairs)), governor)?;
    }
    if get(&body, "additionalProperties", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::AdditionalProperties(child(
                schema,
                id,
                "additionalProperties",
                governor,
            )?)),
            governor,
        )?;
    }
    if get(&body, "propertyNames", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::PropertyNames(child(
                schema,
                id,
                "propertyNames",
                governor,
            )?)),
            governor,
        )?;
    }
    Ok(())
}

/// The object-shaped keywords local to `id`.
fn object_facets<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
    out: &mut ScratchVec<Canon<'s>>,
) -> Result<(), StopReason> {
    reference::work(governor, schema.index.locations[id].route.len())?;
    let body = schema.keywords(id);
    if let Some(count) = as_u64(get(&body, "minProperties", governor)?, governor)? {
        push(out, Canon::Local(Facet::MinProperties(count)), governor)?;
    }
    if let Some(count) = as_u64(get(&body, "maxProperties", governor)?, governor)? {
        push(out, Canon::Local(Facet::MaxProperties(count)), governor)?;
    }
    object_property_facets(schema, id, governor, out)?;
    if let Some(Node {
        value: Value::Array(required),
        ..
    }) = get(&body, "required", governor)?
    {
        push(
            out,
            Canon::Local(Facet::Required(self_names(required, governor)?)),
            governor,
        )?;
    }
    // `dependentRequired`/`dependentSchemas`, or Draft-07 `dependencies`
    // split into the same two facets by entry form.
    let mut required_pairs = ScratchVec::new(governor);
    let mut schema_pairs = ScratchVec::new(governor);
    let dependencies = schema.dependencies(id, governor)?;
    for dependency in &dependencies {
        match dependency {
            Dependency::Required { trigger, names, .. } => push(
                &mut required_pairs,
                (*trigger, self_names(names, governor)?),
                governor,
            )?,
            Dependency::Schema { trigger, child } => {
                push(&mut schema_pairs, (*trigger, *child), governor)?;
            }
        }
    }
    let legacy = get(&body, "dependencies", governor)?.is_some();
    if legacy || get(&body, "dependentRequired", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::DependentRequired(required_pairs)),
            governor,
        )?;
    }
    if legacy || get(&body, "dependentSchemas", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::DependentSchemas(schema_pairs)),
            governor,
        )?;
    }
    if get(&body, "unevaluatedProperties", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::UnevaluatedProperties(child(
                schema,
                id,
                "unevaluatedProperties",
                governor,
            )?)),
            governor,
        )?;
    }
    if get(&body, "unevaluatedItems", governor)?.is_some() {
        push(
            out,
            Canon::Local(Facet::UnevaluatedItems(child(
                schema,
                id,
                "unevaluatedItems",
                governor,
            )?)),
            governor,
        )?;
    }
    Ok(())
}

/// Decode a compiled non-negative length/count bound. A value too large for
/// `usize` (an astronomically large `minLength`/`minItems`/... spelling)
/// clamps to `u64::MAX` rather than silently dropping the bound: generation
/// still tries to honor it and fails with an explicit resource exhaustion
/// once it actually attempts to build something that large, instead of
/// quietly treating an unrepresentable bound as no constraint at all.
fn as_u64(node: Option<&Node>, governor: &mut Governor) -> Result<Option<u64>, StopReason> {
    let Some(Node {
        value: Value::Number(number),
        ..
    }) = node
    else {
        return Ok(None);
    };
    let result = match number.integer_magnitude_governed(governor)? {
        crate::json::IntegerReading::Exact {
            negative: false,
            magnitude,
        } => usize::try_from(magnitude).ok().map_or(u64::MAX, size),
        _ => u64::MAX,
    };
    Ok(Some(result))
}

fn get<'s>(
    body: &super::super::vocabulary::Keywords<'s>,
    key: &str,
    governor: &mut Governor,
) -> Result<Option<&'s Node>, StopReason> {
    body.get_governed(key, governor)
}
fn pattern_key<'s>(
    schema: &'s Schema,
    id: usize,
    key: &str,
    governor: &mut Governor,
) -> Result<&'s str, StopReason> {
    for (stored, _) in schema.index.locations[id]
        .patterns
        .iter_governed(governor)?
    {
        reference::work(governor, key.len().min(stored.len()).saturating_add(1))?;
        if stored.as_str() == key {
            return Ok(stored.as_str());
        }
    }
    unreachable!("compiled pattern exists")
}
fn self_names<'s>(
    nodes: &'s [Node],
    governor: &mut Governor,
) -> Result<ScratchVec<&'s str>, StopReason> {
    let mut names = ScratchVec::new(governor);
    for node in nodes {
        reference::work(governor, 1)?;
        if let Some(name) = node.as_str() {
            push(&mut names, name, governor)?;
        }
    }
    Ok(names)
}

impl ScratchCharge for Facet<'_> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 128);
        128
    };
}
impl CopyIn for Facet<'_> {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        reference::work(governor, 1)?;
        Ok(match self {
            Self::Type(a) => Self::Type(a.copy_in(governor)?),
            Self::Const(a) => Self::Const(a.copy_in(governor)?),
            Self::Excluded(a) => Self::Excluded(a.copy_in(governor)?),
            Self::Enum(a) => Self::Enum(a.copy_in(governor)?),
            Self::Minimum(a, b) => Self::Minimum(a.copy_in(governor)?, b.copy_in(governor)?),
            Self::Maximum(a, b) => Self::Maximum(a.copy_in(governor)?, b.copy_in(governor)?),
            Self::MultipleOf(a) => Self::MultipleOf(a.copy_in(governor)?),
            Self::NotMultipleOf(a) => Self::NotMultipleOf(a.copy_in(governor)?),
            Self::MinLength(a) => Self::MinLength(a.copy_in(governor)?),
            Self::MaxLength(a) => Self::MaxLength(a.copy_in(governor)?),
            Self::Pattern(a) => Self::Pattern(a.copy_in(governor)?),
            Self::AvoidPattern(a) => Self::AvoidPattern(a.copy_in(governor)?),
            Self::Format(a) => Self::Format(a.copy_in(governor)?),
            Self::AvoidFormat(a) => Self::AvoidFormat(a.copy_in(governor)?),
            Self::MinItems(a) => Self::MinItems(a.copy_in(governor)?),
            Self::MaxItems(a) => Self::MaxItems(a.copy_in(governor)?),
            Self::UniqueItems => Self::UniqueItems,
            Self::RequireDuplicate => Self::RequireDuplicate,
            Self::PrefixItems(a) => Self::PrefixItems(a.copy_in(governor)?),
            Self::Items(a, b) => Self::Items(a.copy_in(governor)?, b.copy_in(governor)?),
            Self::Contains(a, b, c) => Self::Contains(
                a.copy_in(governor)?,
                b.copy_in(governor)?,
                c.copy_in(governor)?,
            ),
            Self::MinProperties(a) => Self::MinProperties(a.copy_in(governor)?),
            Self::MaxProperties(a) => Self::MaxProperties(a.copy_in(governor)?),
            Self::Properties(a) => Self::Properties(a.copy_in(governor)?),
            Self::PatternProperties(a) => Self::PatternProperties(a.copy_in(governor)?),
            Self::AdditionalProperties(a) => Self::AdditionalProperties(a.copy_in(governor)?),
            Self::PropertyNames(a) => Self::PropertyNames(a.copy_in(governor)?),
            Self::Required(a) => Self::Required(a.copy_in(governor)?),
            Self::DependentRequired(a) => Self::DependentRequired(a.copy_in(governor)?),
            Self::DependentSchemas(a) => Self::DependentSchemas(a.copy_in(governor)?),
            Self::UnevaluatedProperties(a) => Self::UnevaluatedProperties(a.copy_in(governor)?),
            Self::UnevaluatedItems(a) => Self::UnevaluatedItems(a.copy_in(governor)?),
        })
    }
}

impl ScratchCharge for Canon<'_> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 128);
        128
    };
}
impl Canon<'_> {
    fn copy_at(&self, governor: &mut Governor, depth: u32) -> Result<Self, StopReason> {
        storage::depth(governor, depth)?;
        reference::work(governor, 1)?;
        Ok(match self {
            Self::True => Self::True,
            Self::False => Self::False,
            Self::Local(a) => Self::Local(a.copy_in(governor)?),
            Self::OpaqueNot(a) => Self::OpaqueNot(a.copy_in(governor)?),
            Self::And(a) => Self::And(copy_canons(a, governor, depth.saturating_add(1))?),
            Self::Or(a) => Self::Or(copy_canons(a, governor, depth.saturating_add(1))?),
            Self::OneOf(a) => Self::OneOf(a.copy_in(governor)?),
            Self::Ref(a) => Self::Ref(a.copy_in(governor)?),
            Self::DynamicRef(a) => Self::DynamicRef(a.copy_in(governor)?),
            Self::NegatedRef(a) => Self::NegatedRef(a.copy_in(governor)?),
            Self::NegatedDynamicRef(a) => Self::NegatedDynamicRef(a.copy_in(governor)?),
            Self::Scoped(a, b) => {
                Self::Scoped(*a, copy_canons(b, governor, depth.saturating_add(1))?)
            }
            Self::IfThenElse {
                cond,
                then,
                otherwise,
            } => Self::IfThenElse {
                cond: *cond,
                then: *then,
                otherwise: *otherwise,
            },
        })
    }
}

impl CopyIn for Canon<'_> {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        self.copy_at(governor, 0)
    }
}
fn copy_canons<'s>(
    items: &[Canon<'s>],
    governor: &mut Governor,
    depth: u32,
) -> Result<ScratchVec<Canon<'s>>, StopReason> {
    let mut copied = ScratchVec::new(governor);
    copied.reserve(items.len())?;
    for item in items {
        let item = item.copy_at(governor, depth)?;
        push(&mut copied, item, governor)?;
    }
    Ok(copied)
}
impl ScratchCharge for PatternRef<'_> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 24);
        24
    };
}
impl ScratchCharge for TypeSet {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 8);
        8
    };
}
pub(super) fn build<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Canon<'s>, StopReason> {
    build_at(schema, id, governor, 0)
}
pub(super) fn build_negated<'s>(
    schema: &'s Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Canon<'s>, StopReason> {
    build_negated_at(schema, id, governor, 0)
}

pub(super) fn build_scoped<'s>(
    schema: &'s Schema,
    id: usize,
    negated: bool,
    governor: &mut Governor,
) -> Result<Canon<'s>, StopReason> {
    let inner = if negated {
        build_negated_at(schema, id, governor, 1)?
    } else {
        build_at(schema, id, governor, 1)?
    };
    scoped(id, inner, governor)
}
