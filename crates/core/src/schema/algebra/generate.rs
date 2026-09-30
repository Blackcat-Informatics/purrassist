// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Stage 5 (generation), and the recursion driver connecting it to stages
//! 1–4 (`canon`/`merge`): [`Gen::attempt`] walks one compiled schema
//! location's [`Canon`] tree, folding every [`Facet`] it reaches into a
//! [`Requirements`] accumulator (the `and`-merge/preparation stages) and
//! backtracking over `Canon::Or`/`Canon::IfThenElse`/reference edges,
//! before [`Gen::finish`] builds a concrete value from the merged
//! obligations. `$ref`/`$dynamicRef` (positive and negated) are expanded
//! here, under the same non-progressing-cycle guard `(schema id, instance
//! path)` and dynamic-scope stack `super::super::evaluate` uses, so a
//! self- or mutually-recursive schema either makes progress (the path
//! deepens) or is reported as [`Attempt::Exhausted`] rather than looping.
use super::super::Schema;
use super::Refutation;
use super::canon::{self, Canon, NumKind, PatternRef};
use super::invalid;
use super::merge::Requirements;
use crate::contract::saturating_size as size;
use crate::contract::{Evidence, Governor, Resource, StopReason};
use crate::json::{Member, Node, Value, pointer_segment};
use crate::pattern::{Engine, GenerateOptions, GenerateOutcome, Pattern};
use std::collections::BTreeSet;

/// Every `items` occurrence in `req` that applies at array index `index` —
/// i.e. every `(id, own_prefix_len)` pair whose own `prefixItems` (if any)
/// does not already cover `index`. See [`super::canon::Facet::Items`]'s doc
/// comment for why this is a per-occurrence test, not a single shared
/// prefix length computed across every `prefixItems` in `req`.
fn applicable_items(req: &Requirements, index: usize) -> Vec<usize> {
    req.items
        .iter()
        .filter(|&&(_, own_len)| index >= own_len)
        .map(|&(id, _)| id)
        .collect()
}

pub(super) enum Attempt {
    Witness(Node),
    Unsatisfiable(Refutation),
    Exhausted,
}

/// Outcome of [`Gen::try_simple_merge`]: either the merge succeeded (with
/// the updated [`Requirements`]), proved a direct contradiction, or the
/// canon contained a choice point too complex for a merge-only pass.
enum SimpleMerge {
    Merged(Box<Requirements>),
    Contradiction,
    TooComplex,
}

fn refute(reason: &str) -> Refutation {
    Refutation {
        reason: reason.to_string(),
        evidence: vec![Evidence {
            rule: "schema.algebra.unsatisfiable-v1".into(),
            explanation: reason.to_string(),
            span: None,
            destination: String::new(),
        }],
    }
}

pub(super) fn run(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Attempt, StopReason> {
    let mut driver = Gen {
        schema,
        governor,
        scope: Vec::new(),
        active: BTreeSet::new(),
    };
    driver.attempt(id, "", 0)
}

struct Gen<'s, 'g> {
    schema: &'s Schema,
    governor: &'g mut Governor,
    scope: Vec<String>,
    active: BTreeSet<(usize, String, bool)>,
}

impl Gen<'_, '_> {
    /// Enter one schema location under the non-progressing-cycle guard and
    /// dynamic-scope stack, build its [`Canon`], and search it.
    fn attempt(&mut self, id: usize, path: &str, depth: u32) -> Result<Attempt, StopReason> {
        self.enter(id, path, depth, false, |driver, canon, depth| {
            driver.attempt_and(&[canon], Requirements::default(), path, depth)
        })
    }
    /// Shared guard/scope/build wrapper for entering schema location `id`
    /// (positively or, when `negated`, via `build_negated`) at `path`.
    fn enter(
        &mut self,
        id: usize,
        path: &str,
        depth: u32,
        negated: bool,
        body: impl FnOnce(&mut Self, Canon, u32) -> Result<Attempt, StopReason>,
    ) -> Result<Attempt, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        let key = (id, path.to_string(), negated);
        if !self.active.insert(key.clone()) {
            return Ok(Attempt::Exhausted);
        }
        let base = self.schema.index.locations[id].base.clone();
        let entered = self.scope.last() != Some(&base);
        if entered {
            self.scope.push(base);
        }
        let canon = if negated {
            canon::build_negated(self.schema, id, self.governor)
        } else {
            canon::build(self.schema, id, self.governor)
        };
        let result = canon.and_then(|canon| body(self, canon, depth + 1));
        if entered {
            self.scope.pop();
        }
        self.active.remove(&key);
        result
    }
    /// Fold every id in `ids` into one merged conjunction at the same
    /// generation path (used for and-merging `properties`/`items`/
    /// `prefixItems` contributions from multiple `allOf` branches): reuses
    /// [`Canon::Ref`]'s own guard/scope handling for each id in turn.
    fn attempt_many(
        &mut self,
        ids: &[usize],
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        self.attempt_many_excluding(ids, &[], path, depth)
    }
    /// Like [`Gen::attempt_many`], but seeding the merged `Requirements`
    /// with `excluded` values already committed to (a `uniqueItems` array's
    /// earlier elements): otherwise a deterministic generator would
    /// happily repeat the exact same value for every slot sharing one
    /// `items`/`contains` sub-schema, which the array's own `uniqueItems`
    /// facet would then reject wholesale.
    fn attempt_many_excluding(
        &mut self,
        ids: &[usize],
        excluded: &[Node],
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        let items: Vec<Canon> = ids.iter().copied().map(Canon::Ref).collect();
        let mut req = Requirements::default();
        req.excluded.extend(excluded.iter().cloned());
        self.attempt_and(&items, req, path, depth)
    }
    fn attempt_and(
        &mut self,
        items: &[Canon],
        mut req: Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        self.governor.check()?;
        self.governor.charge(Resource::Work, 1)?;
        let Some((first, rest)) = items.split_first() else {
            return self.finish(&req, path, depth);
        };
        match first {
            Canon::True => self.attempt_and(rest, req, path, depth),
            Canon::False => Ok(Attempt::Unsatisfiable(refute("boolean false schema"))),
            Canon::Local(facet) => {
                req.apply(facet);
                if req.is_contradictory() {
                    return Ok(Attempt::Unsatisfiable(refute(
                        req.contradiction.unwrap_or("contradiction"),
                    )));
                }
                self.attempt_and(rest, req, path, depth)
            }
            Canon::OpaqueNot(id) => {
                req.opaque_nots.push(*id);
                self.attempt_and(rest, req, path, depth)
            }
            Canon::And(inner) => {
                let mut combined = inner.clone();
                combined.extend_from_slice(rest);
                self.attempt_and(&combined, req, path, depth)
            }
            Canon::Or(branches) => self.attempt_or(branches, rest, &req, path, depth),
            Canon::OneOf(ids) => self.attempt_one_of(ids, rest, &req, path, depth),
            Canon::IfThenElse {
                cond,
                then,
                otherwise,
            } => self.attempt_if(*cond, *then, *otherwise, rest, req, path, depth),
            Canon::Scoped(id, inner) => {
                let base = self.schema.index.locations[*id].base.clone();
                let entered = self.scope.last() != Some(&base);
                if entered {
                    self.scope.push(base);
                }
                let mut combined = vec![(**inner).clone()];
                combined.extend_from_slice(rest);
                let result = self.attempt_and(&combined, req, path, depth);
                if entered {
                    self.scope.pop();
                }
                result
            }
            Canon::Ref(target) => self.attempt_ref(*target, false, rest, req, path, depth),
            Canon::NegatedRef(target) => self.attempt_ref(*target, true, rest, req, path, depth),
            Canon::DynamicRef(reference, base) => {
                let target = self.resolve_dynamic(reference, base)?;
                self.attempt_ref(target, false, rest, req, path, depth)
            }
            Canon::NegatedDynamicRef(reference, base) => {
                let target = self.resolve_dynamic(reference, base)?;
                self.attempt_ref(target, true, rest, req, path, depth)
            }
        }
    }
    fn attempt_ref(
        &mut self,
        target: usize,
        negated: bool,
        rest: &[Canon],
        req: Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        self.enter(target, path, depth, negated, |driver, canon, depth| {
            let mut combined = vec![canon];
            combined.extend_from_slice(rest);
            driver.attempt_and(&combined, req, path, depth)
        })
    }
    fn resolve_dynamic(&self, reference: &str, base: &str) -> Result<usize, StopReason> {
        let mut target = self
            .schema
            .resolve(base, reference)
            .map_err(|error| invalid(&error))?;
        if reference
            .split_once('#')
            .is_some_and(|(_, fragment)| !fragment.is_empty() && !fragment.starts_with('/'))
            && let Some(name) = &self.schema.index.locations[target].dynamic_anchor
        {
            for scope_base in &self.scope {
                if let Some(&candidate) = self
                    .schema
                    .index
                    .anchors
                    .get(&format!("{scope_base}#{name}"))
                    && self.schema.index.locations[candidate]
                        .dynamic_anchor
                        .as_ref()
                        == Some(name)
                {
                    target = candidate;
                    break;
                }
            }
        }
        Ok(target)
    }
    fn attempt_or(
        &mut self,
        branches: &[Canon],
        rest: &[Canon],
        req: &Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        if branches.is_empty() {
            return Ok(Attempt::Unsatisfiable(refute("empty disjunction")));
        }
        let mut any_exhausted = false;
        for branch in branches {
            self.governor.check()?;
            let mut combined = vec![branch.clone()];
            combined.extend_from_slice(rest);
            match self.attempt_and(&combined, req.clone(), path, depth)? {
                Attempt::Witness(node) => return Ok(Attempt::Witness(node)),
                Attempt::Unsatisfiable(_) => {}
                Attempt::Exhausted => any_exhausted = true,
            }
        }
        if any_exhausted {
            Ok(Attempt::Exhausted)
        } else {
            Ok(Attempt::Unsatisfiable(refute(
                "every disjunct is unsatisfiable",
            )))
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn attempt_if(
        &mut self,
        cond: usize,
        then: Option<usize>,
        otherwise: Option<usize>,
        rest: &[Canon],
        req: Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        let branch_a = self.if_branch(cond, false, then, rest, req.clone(), path, depth)?;
        if let Attempt::Witness(node) = branch_a {
            return Ok(Attempt::Witness(node));
        }
        let branch_b = self.if_branch(cond, true, otherwise, rest, req, path, depth)?;
        Ok(match (branch_a, branch_b) {
            (Attempt::Witness(node), _) | (_, Attempt::Witness(node)) => Attempt::Witness(node),
            (Attempt::Unsatisfiable(_), Attempt::Unsatisfiable(_)) => Attempt::Unsatisfiable(
                refute("if/then and its else-complement are both unsatisfiable"),
            ),
            _ => Attempt::Exhausted,
        })
    }
    /// One `if`/`then`/`else` half (`cond`+`then`, or `!cond`+`else`).
    ///
    /// `if`/`then`/`else` are each their own schema location (possibly with
    /// their own `$id`/`$dynamicAnchor`), and — critically — `evaluate.rs`
    /// enters and *fully leaves* `cond`'s dynamic scope (one complete
    /// `self.eval(...)` call) before `then`/`else` is entered at all, so a
    /// `$dynamicRef` reached while searching `then`/`else` must *not* still
    /// see `cond`'s scope on the stack. Folding `cond` and `then` into one
    /// flat `Canon` list under a single `Canon::Scoped` wrapper cannot
    /// express that: `Canon::Scoped`'s scope pop is deferred until the
    /// *entire* nested `attempt_and` recursion returns, which — since that
    /// recursion also walks whatever follows `cond` in the list — means
    /// `then` would be searched while `cond`'s scope is still active.
    ///
    /// So `cond` is merged to completion (via [`Gen::try_simple_merge`],
    /// which pushes/pops `cond`'s scope within one Rust stack frame, hence
    /// unconditionally on every return path) *before* `then`/`else` is even
    /// built, exactly mirroring `evaluate.rs`'s two separate, sequential
    /// `eval()` calls. `try_simple_merge` only understands non-branching
    /// canon (no `Or`/`OneOf`/`IfThenElse`/`Ref`/`DynamicRef`): if `cond` (or
    /// `then`/`else`) needs backtracking search to resolve, this falls back
    /// to the previous flat-list combination for that half, which remains
    /// correct whenever the branch never resolves a `$dynamicRef` against a
    /// scope that should already have been left.
    #[allow(clippy::too_many_arguments)]
    fn if_branch(
        &mut self,
        cond: usize,
        negate_cond: bool,
        then_or_else: Option<usize>,
        rest: &[Canon],
        req: Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        let cond_canon = self.scoped_build(cond, negate_cond)?;
        match self.try_simple_merge(&cond_canon, req.clone()) {
            SimpleMerge::Contradiction => Ok(Attempt::Unsatisfiable(refute(
                "if-condition branch is unsatisfiable",
            ))),
            SimpleMerge::Merged(merged_req) => match then_or_else {
                Some(id) => {
                    let branch_canon = self.scoped_build(id, false)?;
                    match self.try_simple_merge(&branch_canon, (*merged_req).clone()) {
                        SimpleMerge::Contradiction => {
                            Ok(Attempt::Unsatisfiable(refute("if-branch is unsatisfiable")))
                        }
                        SimpleMerge::Merged(final_req) => {
                            self.attempt_and(rest, *final_req, path, depth)
                        }
                        SimpleMerge::TooComplex => {
                            let mut combined = vec![branch_canon];
                            combined.extend_from_slice(rest);
                            self.attempt_and(&combined, *merged_req, path, depth)
                        }
                    }
                }
                None => self.attempt_and(rest, *merged_req, path, depth),
            },
            SimpleMerge::TooComplex => {
                let mut combined = vec![cond_canon];
                if let Some(id) = then_or_else {
                    combined.push(self.scoped_build(id, false)?);
                }
                combined.extend_from_slice(rest);
                self.attempt_and(&combined, req, path, depth)
            }
        }
    }
    /// Merge `canon`'s facets into `req` without any backtracking search or
    /// call to [`Gen::finish`] — a strict subset of what [`Gen::attempt_and`]
    /// does, covering only conjunctive/scoped structure with no choice
    /// point. Every recursive step (including the `Canon::Scoped` push/pop)
    /// happens within this one Rust call stack, so cleanup is unconditional
    /// on every return path with no list-marker or leak concerns.
    /// `Canon::Or`/`OneOf`/`IfThenElse`/`Ref`/`DynamicRef` need a real
    /// backtracking search to resolve (trying one branch can depend on what
    /// happens afterward), so those report [`SimpleMerge::TooComplex`]
    /// rather than guessing.
    fn try_simple_merge(&mut self, canon: &Canon, mut req: Requirements) -> SimpleMerge {
        match canon {
            Canon::True => SimpleMerge::Merged(Box::new(req)),
            Canon::False => SimpleMerge::Contradiction,
            Canon::Local(facet) => {
                req.apply(facet);
                if req.is_contradictory() {
                    SimpleMerge::Contradiction
                } else {
                    SimpleMerge::Merged(Box::new(req))
                }
            }
            Canon::OpaqueNot(id) => {
                req.opaque_nots.push(*id);
                SimpleMerge::Merged(Box::new(req))
            }
            Canon::And(items) => {
                for item in items {
                    match self.try_simple_merge(item, req) {
                        SimpleMerge::Merged(updated) => req = *updated,
                        other => return other,
                    }
                }
                SimpleMerge::Merged(Box::new(req))
            }
            Canon::Scoped(id, inner) => {
                let base = self.schema.index.locations[*id].base.clone();
                let entered = self.scope.last() != Some(&base);
                if entered {
                    self.scope.push(base);
                }
                let result = self.try_simple_merge(inner, req);
                if entered {
                    self.scope.pop();
                }
                result
            }
            Canon::Or(_)
            | Canon::OneOf(_)
            | Canon::IfThenElse { .. }
            | Canon::Ref(_)
            | Canon::NegatedRef(_)
            | Canon::DynamicRef(_, _)
            | Canon::NegatedDynamicRef(_, _) => SimpleMerge::TooComplex,
        }
    }
    /// Build `id`'s (positive, or negated) canon and wrap it in
    /// `Canon::Scoped(id, ..)`, for a branch (`if`/`then`/`else`) canon
    /// itself builds eagerly rather than deferring to [`Canon::Ref`].
    fn scoped_build(&mut self, id: usize, negated: bool) -> Result<Canon, StopReason> {
        let inner = if negated {
            canon::build_negated(self.schema, id, self.governor)?
        } else {
            canon::build(self.schema, id, self.governor)?
        };
        Ok(Canon::Scoped(id, Box::new(inner)))
    }
    /// `oneOf`'s exactly-one semantics: try each branch `i` in turn as the
    /// one that must hold, adding every *other* branch as a
    /// [`Canon::OpaqueNot`] obligation so the generated candidate is
    /// checked (with the real evaluator) to actually fail them — the sound,
    /// general dual of an arbitrary branch, since a fully symbolic `not`
    /// dual would have to handle whatever structural keywords that branch
    /// contains. Unsatisfiable only when every branch, tried this way, is;
    /// an `OpaqueNot` branch can never itself prove that, so this can never
    /// falsely claim `oneOf` is unsatisfiable merely because the exclusion
    /// search was inconclusive.
    fn attempt_one_of(
        &mut self,
        ids: &[usize],
        rest: &[Canon],
        req: &Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        if ids.is_empty() {
            return Ok(Attempt::Unsatisfiable(refute("empty disjunction")));
        }
        let mut any_exhausted = false;
        for (position, &id) in ids.iter().enumerate() {
            self.governor.check()?;
            let branch_canon = self.scoped_build(id, false)?;
            let mut combined = vec![branch_canon];
            for (other_position, &other_id) in ids.iter().enumerate() {
                if other_position != position {
                    combined.push(Canon::OpaqueNot(other_id));
                }
            }
            combined.extend_from_slice(rest);
            match self.attempt_and(&combined, req.clone(), path, depth)? {
                Attempt::Witness(node) => return Ok(Attempt::Witness(node)),
                Attempt::Unsatisfiable(_) => {}
                Attempt::Exhausted => any_exhausted = true,
            }
        }
        if any_exhausted {
            Ok(Attempt::Exhausted)
        } else {
            Ok(Attempt::Unsatisfiable(refute(
                "every oneOf branch, tried exclusively, is unsatisfiable",
            )))
        }
    }

    // ---- stage 5: generation from a merged `Requirements` ----

    fn finish(
        &mut self,
        req: &Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        if let Some(value) = req.const_value.clone() {
            let outcome = self.check(&value, req)?;
            return Ok(outcome.map_or(Attempt::Witness(value), |reason| {
                Attempt::Unsatisfiable(refute(reason))
            }));
        }
        if let Some(values) = req.enum_values.clone() {
            let mut any_checked = false;
            for value in &values {
                if req
                    .excluded
                    .iter()
                    .any(|excluded| excluded.equivalent(value))
                {
                    continue;
                }
                any_checked = true;
                if self.check(value, req)?.is_none() {
                    return Ok(Attempt::Witness(value.clone()));
                }
            }
            return Ok(if any_checked {
                Attempt::Unsatisfiable(refute("no enum member satisfies the merged constraints"))
            } else {
                Attempt::Unsatisfiable(refute("every enum member is excluded"))
            });
        }
        let mut any_exhausted = false;
        for kind in type_preference(req) {
            match self.build_kind(kind, req, path, depth)? {
                Attempt::Witness(node) => return Ok(Attempt::Witness(node)),
                Attempt::Unsatisfiable(_) => {}
                Attempt::Exhausted => any_exhausted = true,
            }
        }
        if req.types.is_empty() {
            return Ok(Attempt::Unsatisfiable(refute("no admitted type")));
        }
        Ok(if any_exhausted {
            Attempt::Exhausted
        } else {
            Attempt::Unsatisfiable(refute(
                "no admitted type can satisfy the merged constraints",
            ))
        })
    }
    fn build_kind(
        &mut self,
        kind: Kind,
        req: &Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        match kind {
            Kind::Null => self.finalize(Node::new(Value::Null), req),
            Kind::Boolean => {
                for value in [false, true] {
                    if self.check(&Node::new(Value::Bool(value)), req)?.is_none() {
                        return Ok(Attempt::Witness(Node::new(Value::Bool(value))));
                    }
                }
                Ok(Attempt::Exhausted)
            }
            Kind::String => self.build_string(req),
            Kind::Number => self.build_number(req),
            Kind::Array => self.build_array(req, path, depth),
            Kind::Object => self.build_object(req, path, depth),
        }
    }
    fn finalize(&mut self, node: Node, req: &Requirements) -> Result<Attempt, StopReason> {
        Ok(match self.check(&node, req)? {
            None => Attempt::Witness(node),
            Some(_) => Attempt::Exhausted,
        })
    }
    fn build_string(&mut self, req: &Requirements) -> Result<Attempt, StopReason> {
        // Any witness for this facet set needs at least `min_length` bytes;
        // charge that before attempting any construction, so an
        // unrepresentably large `minLength` (clamped to `u64::MAX` by
        // `canon::as_u64`) fails with an explicit `BudgetExceeded` instead
        // of this function trying to materialize such a string.
        self.governor
            .charge(Resource::ScratchBytes, req.min_length.saturating_add(1))?;
        if req.patterns.is_empty() {
            if let Some(name) = &req.format {
                let candidate = super::super::format::witness(name).to_string();
                let node = Node::new(Value::String(candidate));
                if self.check(&node, req)?.is_none() {
                    return Ok(Attempt::Witness(node));
                }
            }
        } else {
            let primary = req.patterns[0].clone();
            let pattern = self.pattern_of(&primary).clone();
            let avoid: Vec<Pattern> = req
                .avoid_patterns
                .iter()
                .map(|reference| self.pattern_of(reference).clone())
                .collect();
            let min_len = usize::try_from(req.min_length).unwrap_or(usize::MAX);
            let max_len = req
                .max_length
                .map(|bound| usize::try_from(bound).unwrap_or(usize::MAX));
            match pattern.generate(
                self.governor,
                GenerateOptions {
                    min_len,
                    max_len,
                    avoid: &avoid,
                },
            )? {
                GenerateOutcome::Witness(text) => {
                    let node = Node::new(Value::String(text));
                    return Ok(match self.check(&node, req)? {
                        None => Attempt::Witness(node),
                        Some(_) => Attempt::Exhausted,
                    });
                }
                GenerateOutcome::NoCandidate(_) => {
                    let all_derivative = pattern.engine() == Engine::Derivative
                        && avoid
                            .iter()
                            .all(|other| other.engine() == Engine::Derivative);
                    if all_derivative {
                        let unbounded = pattern.generate(
                            self.governor,
                            GenerateOptions {
                                min_len: 0,
                                max_len: None,
                                avoid: &avoid,
                            },
                        )?;
                        if matches!(unbounded, GenerateOutcome::NoCandidate(_)) {
                            return Ok(Attempt::Unsatisfiable(refute(
                                "pattern intersected with its avoided patterns is empty",
                            )));
                        }
                    }
                    return Ok(Attempt::Exhausted);
                }
            }
        }
        for base_char in ['a', 'b', '0', '_'] {
            let text: String =
                std::iter::repeat_n(base_char, usize::try_from(req.min_length).unwrap_or(0))
                    .collect();
            let node = Node::new(Value::String(text));
            if self.check(&node, req)?.is_none() {
                return Ok(Attempt::Witness(node));
            }
        }
        Ok(Attempt::Exhausted)
    }
    fn pattern_of(&self, reference: &PatternRef) -> &Pattern {
        &self.schema.index.locations[reference.owner].patterns[&reference.keyword]
    }
    fn build_number(&mut self, req: &Requirements) -> Result<Attempt, StopReason> {
        let want_integer = matches!(req.types.num, NumKind::IntOnly);
        let want_fraction_only = matches!(req.types.num, NumKind::FracOnly);
        let mut candidates: Vec<crate::json::Number> = Vec::new();
        for n in 0i64..=64 {
            candidates.push(int_number(n));
            if n != 0 {
                candidates.push(int_number(-n));
            }
        }
        if let Some((bound, _)) = &req.minimum {
            candidates.push(bound.clone());
            if let Some(text) = step_integer(bound.as_str(), 1)
                && let Ok(value) = text.parse()
            {
                candidates.push(value);
            }
        }
        if let Some((bound, _)) = &req.maximum {
            candidates.push(bound.clone());
            if let Some(text) = step_integer(bound.as_str(), -1)
                && let Ok(value) = text.parse()
            {
                candidates.push(value);
            }
        }
        if req.multiples_of.len() == 1
            && let Some(divisor) = req.multiples_of[0].to_usize()
            && divisor > 0
        {
            for k in 0u64..2000 {
                self.governor.charge(Resource::Work, 1)?;
                let magnitude = k.saturating_mul(size(divisor));
                if let Ok(value) = magnitude.to_string().parse() {
                    candidates.push(value);
                }
                if k > 0
                    && let Ok(value) = format!("-{magnitude}").parse()
                {
                    candidates.push(value);
                }
            }
        }
        // Always seed fractional variants too (filtered below by
        // `want_integer`/`want_fraction_only`, same as every other
        // candidate): an admitted-but-not-integer-only numeric range can
        // still exclude every integer (e.g. `exclusiveMinimum: 0,
        // exclusiveMaximum: 1`), so integer candidates alone are not enough
        // whenever `NumKind::Any` is in play, not only `NumKind::FracOnly`.
        let bases: Vec<_> = candidates.clone();
        for base in bases {
            if let Ok(value) = format!("{base}.5").parse() {
                candidates.push(value);
            }
        }
        for candidate in candidates {
            self.governor.charge(Resource::Work, 1)?;
            if want_integer && !candidate.is_integer() {
                continue;
            }
            if want_fraction_only && candidate.is_integer() {
                continue;
            }
            let node = Node::new(Value::Number(candidate));
            if self.check(&node, req)?.is_none() {
                return Ok(Attempt::Witness(node));
            }
        }
        Ok(Attempt::Exhausted)
    }
    fn build_array(
        &mut self,
        req: &Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        let mut items: Vec<Node> = Vec::new();
        let minimum = req.min_items.max(if req.require_duplicate { 2 } else { 0 });
        // Preserve the established preference for materializing a usable
        // prefix, while retaining a shorter valid array when an optional
        // position cannot be constructed. Contains construction already
        // selects its needed positions and must not add gratuitous matches.
        let preferred = if req.contains.is_empty() {
            size(req.prefix_items.iter().map(Vec::len).max().unwrap_or(0))
                .min(req.max_items.unwrap_or(u64::MAX))
        } else {
            0
        };
        // Each contains occurrence has its own minimum; one value can
        // satisfy several occurrences. This sum is a construction bound,
        // not a claim that an array needs disjoint matching elements.
        let construction_bound = req
            .contains
            .iter()
            .fold(minimum.max(preferred), |count, (_, min, _)| {
                count.saturating_add(*min)
            });
        self.governor.charge(
            Resource::ScratchBytes,
            construction_bound.saturating_mul(24).saturating_add(1),
        )?;
        loop {
            let deficient = self.deficient_contains(req, &items)?;
            let optional = size(items.len()) >= minimum && deficient.is_empty();
            if optional && size(items.len()) >= preferred {
                break;
            }
            if size(items.len()) >= construction_bound {
                return Ok(Attempt::Exhausted);
            }
            if req.max_items.is_some_and(|max| size(items.len()) >= max) {
                return Ok(Attempt::Exhausted);
            }
            let index = items.len();
            let child_path = format!("{path}/{index}");
            let mut applicable: Vec<usize> = req
                .prefix_items
                .iter()
                .filter_map(|occurrence| occurrence.get(index).copied())
                .collect();
            applicable.extend(applicable_items(req, index));
            let exclusions: Vec<Node> = if req.unique_items {
                items.clone()
            } else {
                Vec::new()
            };
            let mut selected = None;
            for id in deficient {
                let mut ids = applicable.clone();
                ids.push(id);
                if let Attempt::Witness(node) =
                    self.attempt_many_excluding(&ids, &exclusions, &child_path, depth + 1)?
                {
                    selected = Some(node);
                    break;
                }
            }
            let value = if let Some(node) = selected {
                node
            } else if size(items.len()) < minimum || optional {
                if applicable.is_empty() {
                    applicable.extend_from_slice(&req.unevaluated_items);
                }
                if applicable.is_empty() {
                    if req.require_duplicate && !items.is_empty() {
                        items[0].clone()
                    } else {
                        fresh_scalar(index)
                    }
                } else {
                    match self.attempt_many_excluding(
                        &applicable,
                        &exclusions,
                        &child_path,
                        depth + 1,
                    )? {
                        Attempt::Witness(node) => node,
                        // A failed construction at this chosen position
                        // does not refute other array lengths or placements
                        // of contains matches, nor earlier greedy choices.
                        Attempt::Unsatisfiable(_) | Attempt::Exhausted => {
                            if optional {
                                break;
                            }
                            return Ok(Attempt::Exhausted);
                        }
                    }
                }
            } else {
                return Ok(Attempt::Exhausted);
            };
            items.push(value);
        }
        if req.require_duplicate {
            if items.len() < 2 {
                items.push(
                    items
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Node::new(Value::Null)),
                );
            } else if !has_duplicate(&items) {
                items.push(items[0].clone());
            }
        }
        self.finalize(Node::new(Value::Array(items)), req)
    }
    /// Which contains occurrences still need matches in the construction
    /// so far. These evaluations guide candidate generation; final
    /// whole-schema validation establishes the returned witness.
    fn deficient_contains(
        &mut self,
        req: &Requirements,
        items: &[Node],
    ) -> Result<Vec<usize>, StopReason> {
        let mut deficient = Vec::new();
        for &(id, minimum, _) in &req.contains {
            let mut count = 0_u64;
            for item in items {
                let report = super::super::evaluate::run(self.schema, id, item, self.governor)?;
                count += u64::from(report.violations.is_empty());
            }
            if count < minimum {
                deficient.push(id);
            }
        }
        Ok(deficient)
    }
    fn build_object(
        &mut self,
        req: &Requirements,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        let mut required: Vec<String> = req.required.clone();
        loop {
            let mut added = false;
            for (trigger, deps) in &req.dependent_required {
                if required.contains(trigger) {
                    for dep in deps {
                        if !required.contains(dep) {
                            required.push(dep.clone());
                            added = true;
                        }
                    }
                }
            }
            if !added {
                break;
            }
        }
        let mut req = req.clone();
        for (trigger, id) in &req.dependent_schemas.clone() {
            if required.contains(trigger) {
                req.positive_musts.push(*id);
            }
        }
        let mut members: Vec<Member> = Vec::new();
        for name in &required {
            let child_path = format!("{path}/{}", pointer_segment(name));
            let mut ids: Vec<usize> = req
                .properties
                .iter()
                .filter(|(key, _)| key == name)
                .map(|(_, id)| *id)
                .collect();
            for (pattern, id) in &req.pattern_properties {
                let compiled = self.pattern_of(pattern).clone();
                if compiled.is_match(name, self.governor)? {
                    ids.push(*id);
                }
            }
            let value = match self.attempt_many(&ids, &child_path, depth + 1)? {
                Attempt::Witness(node) => node,
                other => return Ok(other),
            };
            members.push(Member {
                key: name.clone(),
                key_span: None,
                value,
            });
        }
        // Charge before building `min_properties` members, so an
        // unrepresentably large bound (clamped to `u64::MAX` by
        // `canon::as_u64`) fails with an explicit `BudgetExceeded` instead
        // of this loop attempting to build that many members.
        self.governor.charge(
            Resource::ScratchBytes,
            req.min_properties.saturating_mul(32).saturating_add(1),
        )?;
        let mut counter = 0usize;
        while size(members.len()) < req.min_properties {
            let name = loop {
                let candidate = format!("extra{counter}");
                counter += 1;
                if !members.iter().any(|member| member.key == candidate) {
                    break candidate;
                }
            };
            if !req.property_names.is_empty() {
                let key_node = Node::new(Value::String(name.clone()));
                let mut ok = true;
                for &id in &req.property_names {
                    let outcome =
                        super::super::evaluate::run(self.schema, id, &key_node, self.governor)?;
                    if !outcome.violations.is_empty() {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
            }
            let mut ids = req.additional_properties.clone();
            ids.extend(req.unevaluated_properties.iter().copied());
            let value = match self.attempt_many(&ids, &format!("{path}/{name}"), depth + 1)? {
                Attempt::Witness(node) => node,
                other => return Ok(other),
            };
            members.push(Member {
                key: name,
                key_span: None,
                value,
            });
        }
        let minimal = self.finalize(Node::new(Value::Object(members.clone())), &req)?;
        if !matches!(minimal, Attempt::Exhausted) || req.opaque_nots.is_empty() {
            return Ok(minimal);
        }
        self.retry_object_with_extra_member(&req, members, required.len(), path, depth)
    }
    /// The minimal candidate built by [`Gen::build_object`] failed a
    /// `Canon::OpaqueNot` obligation. That can happen even though nothing
    /// else in `req` demands another member: `unevaluatedProperties: false`
    /// (and similarly-shaped structural keywords) has no closed-form
    /// `not`-dual, so its negation falls back to `OpaqueNot` (see
    /// `canon::build_negated`'s module documentation) — and *its* dual is
    /// exactly "some property is not evaluated by whatever
    /// `properties`/`patternProperties`/`additionalProperties` cover it,"
    /// which the empty/required-only candidate can never witness on its
    /// own. Retry once with one extra, synthetic property (bounded —
    /// soundness comes from re-`finalize`-ing, which rejects the retry too
    /// if it still fails).
    fn retry_object_with_extra_member(
        &mut self,
        req: &Requirements,
        mut members: Vec<Member>,
        required_len: usize,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason> {
        let mut counter = required_len;
        let name = loop {
            let candidate = format!("extra{counter}");
            counter += 1;
            if !members.iter().any(|member| member.key == candidate) {
                break candidate;
            }
        };
        if !req.property_names.is_empty() {
            let key_node = Node::new(Value::String(name.clone()));
            for &id in &req.property_names {
                let outcome =
                    super::super::evaluate::run(self.schema, id, &key_node, self.governor)?;
                if !outcome.violations.is_empty() {
                    return Ok(Attempt::Exhausted);
                }
            }
        }
        let mut ids = req.additional_properties.clone();
        ids.extend(req.unevaluated_properties.iter().copied());
        let value = match self.attempt_many(&ids, &format!("{path}/{name}"), depth + 1)? {
            Attempt::Witness(node) => node,
            other => return Ok(other),
        };
        members.push(Member {
            key: name,
            key_span: None,
            value,
        });
        self.finalize(Node::new(Value::Object(members)), req)
    }
    /// Every remaining assertion the merged [`Requirements`] carries against
    /// a fully-constructed candidate: excluded literals, kind-specific
    /// bounds/pattern/format, and every `Canon::OpaqueNot`/`dependentSchemas`
    /// obligation, checked with the real evaluator rather than decided
    /// symbolically. This is also stage 5's whole treatment of a literal
    /// `const`/`enum` candidate.
    fn check(
        &mut self,
        node: &Node,
        req: &Requirements,
    ) -> Result<Option<&'static str>, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        if req
            .excluded
            .iter()
            .any(|excluded| excluded.equivalent(node))
        {
            return Ok(Some("candidate equals an excluded value"));
        }
        if let Some(reason) = match &node.value {
            Value::Null => (!req.types.null).then_some("null is not an admitted type"),
            Value::Bool(_) => (!req.types.boolean).then_some("boolean is not an admitted type"),
            Value::Number(number) => self.check_number(number, req)?,
            Value::String(text) => self.check_string(text, req)?,
            Value::Array(items) => self.check_array(items, req)?,
            Value::Object(members) => check_object(members, req),
        } {
            return Ok(Some(reason));
        }
        for &id in &req.opaque_nots {
            let outcome = super::super::evaluate::run(self.schema, id, node, self.governor)?;
            if outcome.violations.is_empty() {
                return Ok(Some(
                    "candidate must fail a negated sub-schema but satisfies it",
                ));
            }
        }
        for &id in &req.positive_musts {
            let outcome = super::super::evaluate::run(self.schema, id, node, self.governor)?;
            if !outcome.violations.is_empty() {
                return Ok(Some(
                    "candidate does not satisfy a triggered dependentSchemas entry",
                ));
            }
        }
        Ok(None)
    }
    fn check_number(
        &mut self,
        number: &crate::json::Number,
        req: &Requirements,
    ) -> Result<Option<&'static str>, StopReason> {
        let integer = number.is_integer();
        let admitted = match req.types.num {
            NumKind::None => false,
            NumKind::Any => true,
            NumKind::IntOnly => integer,
            NumKind::FracOnly => !integer,
        };
        if !admitted {
            return Ok(Some("number is not an admitted type"));
        }
        if let Some((bound, exclusive)) = &req.minimum {
            let comparison = number.compare(bound);
            if (*exclusive && !comparison.is_gt()) || (!*exclusive && comparison.is_lt()) {
                return Ok(Some("below minimum"));
            }
        }
        if let Some((bound, exclusive)) = &req.maximum {
            let comparison = number.compare(bound);
            if (*exclusive && !comparison.is_lt()) || (!*exclusive && comparison.is_gt()) {
                return Ok(Some("above maximum"));
            }
        }
        for divisor in &req.multiples_of {
            if !number.is_multiple_of(divisor, self.governor)? {
                return Ok(Some("not a required multiple"));
            }
        }
        for divisor in &req.not_multiples_of {
            if number.is_multiple_of(divisor, self.governor)? {
                return Ok(Some("a forbidden multiple"));
            }
        }
        Ok(None)
    }
    fn check_string(
        &mut self,
        text: &str,
        req: &Requirements,
    ) -> Result<Option<&'static str>, StopReason> {
        if !req.types.string {
            return Ok(Some("string is not an admitted type"));
        }
        let length = size(text.chars().count());
        if length < req.min_length {
            return Ok(Some("shorter than minLength"));
        }
        if req.max_length.is_some_and(|bound| length > bound) {
            return Ok(Some("longer than maxLength"));
        }
        for reference in &req.patterns {
            let compiled = self.pattern_of(reference).clone();
            if !compiled.is_match(text, self.governor)? {
                return Ok(Some("does not match pattern"));
            }
        }
        for reference in &req.avoid_patterns {
            let compiled = self.pattern_of(reference).clone();
            if compiled.is_match(text, self.governor)? {
                return Ok(Some("matches an avoided pattern"));
            }
        }
        if let Some(name) = &req.format
            && matches!(
                super::super::format::evaluate(name, text, self.governor)?,
                super::super::format::Outcome::Fail
            )
        {
            return Ok(Some("fails required format"));
        }
        for name in &req.avoid_formats {
            if matches!(
                super::super::format::evaluate(name, text, self.governor)?,
                super::super::format::Outcome::Pass
            ) {
                return Ok(Some("satisfies an avoided format"));
            }
        }
        Ok(None)
    }
    fn check_array(
        &mut self,
        items: &[Node],
        req: &Requirements,
    ) -> Result<Option<&'static str>, StopReason> {
        if !req.types.array {
            return Ok(Some("array is not an admitted type"));
        }
        let length = size(items.len());
        if length < req.min_items {
            return Ok(Some("shorter than minItems"));
        }
        if req.max_items.is_some_and(|bound| length > bound) {
            return Ok(Some("longer than maxItems"));
        }
        if req.unique_items {
            for i in 0..items.len() {
                for j in 0..i {
                    self.governor.charge(Resource::Work, 1)?;
                    if items[i].equivalent(&items[j]) {
                        return Ok(Some("duplicate item violates uniqueItems"));
                    }
                }
            }
        }
        if req.require_duplicate && !has_duplicate(items) {
            return Ok(Some("array must contain a duplicate item"));
        }
        Ok(None)
    }
}
fn check_object(members: &[Member], req: &Requirements) -> Option<&'static str> {
    if !req.types.object {
        return Some("object is not an admitted type");
    }
    let length = size(members.len());
    if length < req.min_properties {
        return Some("fewer than minProperties");
    }
    if req.max_properties.is_some_and(|bound| length > bound) {
        return Some("more than maxProperties");
    }
    for name in &req.required {
        if !members.iter().any(|member| &member.key == name) {
            return Some("missing required member");
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Kind {
    Null,
    Boolean,
    String,
    Number,
    Array,
    Object,
}
fn type_preference(req: &Requirements) -> Vec<Kind> {
    let object_hint = !req.properties.is_empty()
        || !req.pattern_properties.is_empty()
        || !req.additional_properties.is_empty()
        || !req.property_names.is_empty()
        || !req.required.is_empty()
        || !req.dependent_required.is_empty()
        || !req.dependent_schemas.is_empty()
        || !req.unevaluated_properties.is_empty()
        || req.min_properties > 0
        || req.max_properties.is_some();
    let array_hint = !req.items.is_empty()
        || !req.prefix_items.is_empty()
        || !req.contains.is_empty()
        || !req.unevaluated_items.is_empty()
        || req.min_items > 0
        || req.max_items.is_some()
        || req.unique_items
        || req.require_duplicate;
    let string_hint = req.min_length > 0
        || req.max_length.is_some()
        || !req.patterns.is_empty()
        || !req.avoid_patterns.is_empty()
        || req.format.is_some()
        || !req.avoid_formats.is_empty();
    let number_hint = req.minimum.is_some()
        || req.maximum.is_some()
        || !req.multiples_of.is_empty()
        || !req.not_multiples_of.is_empty();
    let mut order: Vec<(Kind, bool)> = Vec::new();
    if req.types.object {
        order.push((Kind::Object, object_hint));
    }
    if req.types.array {
        order.push((Kind::Array, array_hint));
    }
    if req.types.string {
        order.push((Kind::String, string_hint));
    }
    if !matches!(req.types.num, NumKind::None) {
        order.push((Kind::Number, number_hint));
    }
    if req.types.boolean {
        order.push((Kind::Boolean, false));
    }
    if req.types.null {
        order.push((Kind::Null, false));
    }
    order.sort_by_key(|(_, hinted)| !hinted);
    order.into_iter().map(|(kind, _)| kind).collect()
}

fn int_number(value: i64) -> crate::json::Number {
    if value >= 0 {
        crate::json::Number::from_unsigned(u64::try_from(value).unwrap_or(0))
    } else {
        format!("-{}", value.unsigned_abs())
            .parse()
            .expect("valid integer spelling")
    }
}
fn increment_digits(digits: &str) -> String {
    let mut bytes: Vec<u8> = digits.bytes().collect();
    let mut index = bytes.len();
    loop {
        if index == 0 {
            bytes.insert(0, b'1');
            break;
        }
        index -= 1;
        if bytes[index] == b'9' {
            bytes[index] = b'0';
        } else {
            bytes[index] += 1;
            break;
        }
    }
    String::from_utf8(bytes).expect("ascii digits")
}
fn decrement_digits(digits: &str) -> Option<String> {
    if digits == "0" {
        return None;
    }
    let mut bytes: Vec<u8> = digits.bytes().collect();
    let mut index = bytes.len();
    loop {
        index -= 1;
        if bytes[index] == b'0' {
            bytes[index] = b'9';
        } else {
            bytes[index] -= 1;
            break;
        }
    }
    let text = String::from_utf8(bytes).expect("ascii digits");
    let trimmed = text.trim_start_matches('0');
    Some(if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    })
}
/// A best-effort `+1`/`-1` step on an exact decimal integer spelling, used
/// only to seed a numeric search anchor near a bound; `None` for a
/// fractional or exponent spelling this helper does not need to handle
/// exactly (the bounded scan around zero still applies).
fn step_integer(text: &str, delta: i8) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let negative = text.starts_with('-');
    let digits = if negative { &text[1..] } else { text };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude_grows = (!negative && delta > 0) || (negative && delta < 0);
    if magnitude_grows {
        let bumped = increment_digits(digits);
        Some(if negative {
            format!("-{bumped}")
        } else {
            bumped
        })
    } else {
        match decrement_digits(digits) {
            Some(reduced) if reduced != "0" => Some(if negative {
                format!("-{reduced}")
            } else {
                reduced
            }),
            Some(_zero) => Some("0".to_string()),
            None => Some(if delta > 0 {
                "1".to_string()
            } else {
                "-1".to_string()
            }),
        }
    }
}
fn fresh_scalar(index: usize) -> Node {
    Node::new(Value::Number(crate::json::Number::from_count(index)))
}
fn has_duplicate(items: &[Node]) -> bool {
    for i in 0..items.len() {
        for j in 0..i {
            if items[i].equivalent(&items[j]) {
                return true;
            }
        }
    }
    false
}
