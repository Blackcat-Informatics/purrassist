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
use super::super::reference;
use super::Attempt;
use super::canon::{self, Canon, NumKind, PatternRef};
use super::merge::Requirements;
use super::storage::{self, CopyIn, push};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, StopReason};
use crate::contract::{ScratchString, ScratchVec, format_scratch_governed};
use crate::json::{Member, Node, Value};
use crate::pattern::{Engine, GenerateOptions, GenerateOutcome, Pattern};

/// Every `items` occurrence in `req` that applies at array index `index` —
/// i.e. every `(id, own_prefix_len)` pair whose own `prefixItems` (if any)
/// does not already cover `index`. See [`super::canon::Facet::Items`]'s doc
/// comment for why this is a per-occurrence test, not a single shared
/// prefix length computed across every `prefixItems` in `req`.
fn applicable_items<'r>(
    req: &'r Requirements<'_>,
    index: usize,
) -> impl Iterator<Item = usize> + 'r {
    req.items
        .iter()
        .filter(move |&&(_, own_len)| index >= own_len)
        .map(|&(id, _)| id)
}

/// Outcome of [`Gen::try_simple_merge`]: either the merge succeeded (with
/// the updated [`Requirements`]), proved a direct contradiction, or the
/// canon contained a choice point too complex for a merge-only pass.
enum SimpleMerge<'a> {
    Merged(ScratchVec<Requirements<'a>>),
    Contradiction,
    TooComplex,
}

pub(super) fn run(
    schema: &Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<Attempt, StopReason> {
    let scope = crate::ScratchVec::new(governor);
    let active = ScratchVec::new(governor);
    let mut driver = Gen {
        schema,
        governor,
        scope,
        active,
        preparation_depth: 0,
    };
    driver.attempt(id, "", 0)
}

struct Gen<'s, 'g> {
    schema: &'s Schema,
    governor: &'g mut Governor,
    scope: crate::ScratchVec<usize>,
    active: ScratchVec<(usize, ScratchString, bool)>,
    preparation_depth: u32,
}

impl<'s> Gen<'s, '_> {
    /// Enter one schema location under the non-progressing-cycle guard and
    /// dynamic-scope stack, build its [`Canon`], and search it.
    fn attempt(&mut self, id: usize, path: &str, depth: u32) -> Result<Attempt, StopReason> {
        self.enter(id, path, depth, false, |driver, canon, depth| {
            driver.attempt_and(&[canon], Requirements::new(driver.governor), path, depth)
        })
    }
    /// Shared guard/scope/build wrapper for entering schema location `id`
    /// (positively or, when `negated`, via `build_negated`) at `path`.
    fn enter<'a>(
        &mut self,
        id: usize,
        path: &str,
        depth: u32,
        negated: bool,
        body: impl FnOnce(&mut Self, Canon<'a>, u32) -> Result<Attempt, StopReason>,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        self.governor.charge(Resource::Work, 1)?;
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        for (active_id, active_path, active_negated) in &self.active {
            reference::work(
                self.governor,
                path.len().min(active_path.len()).saturating_add(1),
            )?;
            if *active_id == id && active_path.as_str() == path && *active_negated == negated {
                return Ok(Attempt::Exhausted);
            }
        }
        let path_key = reference::copy(path, self.governor)?;
        push(&mut self.active, (id, path_key, negated), self.governor)?;
        let result = (|| {
            let entered =
                super::super::index::enter_scope(self.schema, id, &mut self.scope, self.governor)?;
            let canon = if negated {
                canon::build_negated(self.schema, id, self.governor)
            } else {
                canon::build(self.schema, id, self.governor)
            };
            let result = canon.and_then(|canon| body(self, canon, depth + 1));
            if entered {
                self.scope.pop();
            }
            result
        })();
        self.active.pop();
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
        let mut items = ScratchVec::new(self.governor);
        for &id in ids {
            push(&mut items, Canon::Ref(id), self.governor)?;
        }
        let mut req = Requirements::new(self.governor);
        for value in excluded {
            push(&mut req.excluded, value, self.governor)?;
        }
        self.attempt_and(&items, req, path, depth)
    }
    /// Continuations can nest across sibling scoped branches even when the
    /// canonical tree itself is shallow. Admit their live native frames too.
    fn attempt_and<'a>(
        &mut self,
        items: &[Canon<'a>],
        req: Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        let next = self.preparation_depth.saturating_add(1);
        storage::depth(self.governor, next)?;
        self.preparation_depth = next;
        let result = self.attempt_and_inner(items, req, path, depth);
        self.preparation_depth -= 1;
        result
    }
    fn attempt_and_inner<'a>(
        &mut self,
        mut items: &[Canon<'a>],
        mut req: Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        loop {
            self.governor.check()?;
            self.governor.charge(Resource::Work, 1)?;
            let Some((first, rest)) = items.split_first() else {
                return self.finish(&req, path, depth);
            };
            match first {
                Canon::True => {
                    items = rest;
                    continue;
                }
                Canon::Local(facet) => {
                    req.apply(facet, self.governor)?;
                    if req.is_contradictory() {
                        return Ok(Attempt::Unsatisfiable(
                            req.contradiction.unwrap_or("contradiction"),
                        ));
                    }
                    items = rest;
                    continue;
                }
                Canon::OpaqueNot(id) => {
                    push(&mut req.opaque_nots, *id, self.governor)?;
                    items = rest;
                    continue;
                }
                _ => {}
            }
            return match first {
                Canon::True | Canon::Local(_) | Canon::OpaqueNot(_) => {
                    unreachable!("linear facets advanced above")
                }
                Canon::False => Ok(Attempt::Unsatisfiable("boolean false schema")),
                Canon::And(inner) => {
                    let mut combined = storage::copy(inner, self.governor)?;
                    storage::extend(&mut combined, rest, self.governor)?;
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
                    let entered = super::super::index::enter_scope(
                        self.schema,
                        *id,
                        &mut self.scope,
                        self.governor,
                    )?;
                    let result = (|| {
                        let mut combined =
                            storage::singleton(inner[0].copy_in(self.governor)?, self.governor)?;
                        storage::extend(&mut combined, rest, self.governor)?;
                        self.attempt_and(&combined, req, path, depth)
                    })();
                    if entered {
                        self.scope.pop();
                    }
                    result
                }
                Canon::Ref(target) => self.attempt_ref(*target, false, rest, req, path, depth),
                Canon::NegatedRef(target) => {
                    self.attempt_ref(*target, true, rest, req, path, depth)
                }
                Canon::DynamicRef(id) => {
                    let target = self.resolve_dynamic(*id)?;
                    self.attempt_ref(target, false, rest, req, path, depth)
                }
                Canon::NegatedDynamicRef(id) => {
                    let target = self.resolve_dynamic(*id)?;
                    self.attempt_ref(target, true, rest, req, path, depth)
                }
            };
        }
    }
    fn attempt_ref<'a>(
        &mut self,
        target: usize,
        negated: bool,
        rest: &[Canon<'a>],
        req: Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        self.enter(target, path, depth, negated, |driver, canon, depth| {
            let mut combined = storage::singleton(canon, driver.governor)?;
            storage::extend(&mut combined, rest, driver.governor)?;
            driver.attempt_and(&combined, req, path, depth)
        })
    }
    fn resolve_dynamic(&mut self, id: usize) -> Result<usize, StopReason> {
        let schema = self.schema;
        reference::work(self.governor, schema.index.locations[id].route.len())?;
        let reference = schema
            .keywords(id)
            .get_governed("$dynamicRef", self.governor)?
            .and_then(Node::as_str)
            .expect("dynamic canonical edge retains its originating schema location");
        let target = super::super::reference::resolve(
            schema,
            &schema.index.locations[id].base,
            reference,
            self.governor,
        )?;
        super::super::reference::dynamic(
            schema,
            target,
            reference,
            self.scope
                .iter()
                .map(|&id| schema.index.locations[id].base.as_str()),
            self.governor,
        )
    }

    fn attempt_or<'a>(
        &mut self,
        branches: &[Canon<'a>],
        rest: &[Canon<'a>],
        req: &Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        if branches.is_empty() {
            return Ok(Attempt::Unsatisfiable("empty disjunction"));
        }
        let mut any_exhausted = false;
        for branch in branches {
            self.governor.check()?;
            let mut combined = storage::singleton(branch.copy_in(self.governor)?, self.governor)?;
            storage::extend(&mut combined, rest, self.governor)?;
            let branch_req = req.copy_in(self.governor)?;
            match self.attempt_and(&combined, branch_req, path, depth)? {
                Attempt::Witness(node) => return Ok(Attempt::Witness(node)),
                Attempt::Unsatisfiable(_) => {}
                Attempt::Exhausted => any_exhausted = true,
            }
        }
        if any_exhausted {
            Ok(Attempt::Exhausted)
        } else {
            Ok(Attempt::Unsatisfiable("every disjunct is unsatisfiable"))
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn attempt_if<'a>(
        &mut self,
        cond: usize,
        then: Option<usize>,
        otherwise: Option<usize>,
        rest: &[Canon<'a>],
        req: Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        let branch_req = req.copy_in(self.governor)?;
        let branch_a = self.if_branch(cond, false, then, rest, branch_req, path, depth)?;
        if let Attempt::Witness(node) = branch_a {
            return Ok(Attempt::Witness(node));
        }
        let branch_b = self.if_branch(cond, true, otherwise, rest, req, path, depth)?;
        Ok(match (branch_a, branch_b) {
            (Attempt::Witness(node), _) | (_, Attempt::Witness(node)) => Attempt::Witness(node),
            (Attempt::Unsatisfiable(_), Attempt::Unsatisfiable(_)) => {
                Attempt::Unsatisfiable("if/then and its else-complement are both unsatisfiable")
            }
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
    fn if_branch<'a>(
        &mut self,
        cond: usize,
        negate_cond: bool,
        then_or_else: Option<usize>,
        rest: &[Canon<'a>],
        req: Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        let cond_canon = self.scoped_build(cond, negate_cond)?;
        let condition_req = req.copy_in(self.governor)?;
        match self.try_simple_merge(&cond_canon, condition_req)? {
            SimpleMerge::Contradiction => Ok(Attempt::Unsatisfiable(
                "if-condition branch is unsatisfiable",
            )),
            SimpleMerge::Merged(mut merged_req) => match then_or_else {
                Some(id) => {
                    let branch_canon = self.scoped_build(id, false)?;
                    let branch_req = merged_req[0].copy_in(self.governor)?;
                    match self.try_simple_merge(&branch_canon, branch_req)? {
                        SimpleMerge::Contradiction => {
                            Ok(Attempt::Unsatisfiable("if-branch is unsatisfiable"))
                        }
                        SimpleMerge::Merged(mut final_req) => self.attempt_and(
                            rest,
                            final_req.pop().expect("one merged requirement"),
                            path,
                            depth,
                        ),
                        SimpleMerge::TooComplex => {
                            let mut combined = storage::singleton(branch_canon, self.governor)?;
                            storage::extend(&mut combined, rest, self.governor)?;
                            self.attempt_and(
                                &combined,
                                merged_req.pop().expect("one merged requirement"),
                                path,
                                depth,
                            )
                        }
                    }
                }
                None => self.attempt_and(
                    rest,
                    merged_req.pop().expect("one merged requirement"),
                    path,
                    depth,
                ),
            },
            SimpleMerge::TooComplex => {
                let mut combined = storage::singleton(cond_canon, self.governor)?;
                if let Some(id) = then_or_else {
                    push(&mut combined, self.scoped_build(id, false)?, self.governor)?;
                }
                storage::extend(&mut combined, rest, self.governor)?;
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
    fn try_simple_merge<'a>(
        &mut self,
        canon: &Canon<'a>,
        mut req: Requirements<'a>,
    ) -> Result<SimpleMerge<'a>, StopReason>
    where
        's: 'a,
    {
        Ok(match canon {
            Canon::True => SimpleMerge::Merged(storage::singleton(req, self.governor)?),
            Canon::False => SimpleMerge::Contradiction,
            Canon::Local(facet) => {
                req.apply(facet, self.governor)?;
                if req.is_contradictory() {
                    SimpleMerge::Contradiction
                } else {
                    SimpleMerge::Merged(storage::singleton(req, self.governor)?)
                }
            }
            Canon::OpaqueNot(id) => {
                push(&mut req.opaque_nots, *id, self.governor)?;
                SimpleMerge::Merged(storage::singleton(req, self.governor)?)
            }
            Canon::And(items) => {
                for item in items {
                    match self.try_simple_merge(item, req)? {
                        SimpleMerge::Merged(mut updated) => {
                            req = updated.pop().expect("one merged requirement");
                        }
                        other => return Ok(other),
                    }
                }
                SimpleMerge::Merged(storage::singleton(req, self.governor)?)
            }
            Canon::Scoped(id, inner) => {
                let entered = super::super::index::enter_scope(
                    self.schema,
                    *id,
                    &mut self.scope,
                    self.governor,
                )?;
                let result = self.try_simple_merge(&inner[0], req);
                if entered {
                    self.scope.pop();
                }
                result?
            }
            Canon::Or(_)
            | Canon::OneOf(_)
            | Canon::IfThenElse { .. }
            | Canon::Ref(_)
            | Canon::NegatedRef(_)
            | Canon::DynamicRef(_)
            | Canon::NegatedDynamicRef(_) => SimpleMerge::TooComplex,
        })
    }
    /// Build `id`'s (positive, or negated) canon and wrap it in
    /// `Canon::Scoped(id, ..)`, for a branch (`if`/`then`/`else`) canon
    /// itself builds eagerly rather than deferring to [`Canon::Ref`].
    fn scoped_build(&mut self, id: usize, negated: bool) -> Result<Canon<'s>, StopReason> {
        canon::build_scoped(self.schema, id, negated, self.governor)
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
    fn attempt_one_of<'a>(
        &mut self,
        ids: &[usize],
        rest: &[Canon<'a>],
        req: &Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        if ids.is_empty() {
            return Ok(Attempt::Unsatisfiable("empty disjunction"));
        }
        let mut any_exhausted = false;
        for (position, &id) in ids.iter().enumerate() {
            self.governor.check()?;
            let branch_canon = self.scoped_build(id, false)?;
            let mut combined = storage::singleton(branch_canon, self.governor)?;
            for (other_position, &other_id) in ids.iter().enumerate() {
                if other_position != position {
                    push(&mut combined, Canon::OpaqueNot(other_id), self.governor)?;
                }
            }
            storage::extend(&mut combined, rest, self.governor)?;
            let branch_req = req.copy_in(self.governor)?;
            match self.attempt_and(&combined, branch_req, path, depth)? {
                Attempt::Witness(node) => return Ok(Attempt::Witness(node)),
                Attempt::Unsatisfiable(_) => {}
                Attempt::Exhausted => any_exhausted = true,
            }
        }
        if any_exhausted {
            Ok(Attempt::Exhausted)
        } else {
            Ok(Attempt::Unsatisfiable(
                "every oneOf branch, tried exclusively, is unsatisfiable",
            ))
        }
    }

    // ---- stage 5: generation from a merged `Requirements` ----

    fn finish<'a>(
        &mut self,
        req: &Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        self.governor.charge(Resource::Work, 1)?;
        if let Some(value) = req.const_value {
            let outcome = self.check(value, req)?;
            return Ok(
                outcome.map_or_else(|| Attempt::Witness(value.clone()), Attempt::Unsatisfiable)
            );
        }
        if let Some(values) = &req.enum_values {
            let mut any_checked = false;
            for &value in values {
                if storage::equivalent_any(&req.excluded, value, self.governor)? {
                    continue;
                }
                any_checked = true;
                if self.check(value, req)?.is_none() {
                    return Ok(Attempt::Witness(value.clone()));
                }
            }
            return Ok(if any_checked {
                Attempt::Unsatisfiable("no enum member satisfies the merged constraints")
            } else {
                Attempt::Unsatisfiable("every enum member is excluded")
            });
        }
        let mut any_exhausted = false;
        reference::work(self.governor, 12)?;
        for kind in type_preference(req) {
            match self.build_kind(kind, req, path, depth)? {
                Attempt::Witness(node) => return Ok(Attempt::Witness(node)),
                Attempt::Unsatisfiable(_) => {}
                Attempt::Exhausted => any_exhausted = true,
            }
        }
        if req.types.is_empty() {
            return Ok(Attempt::Unsatisfiable("no admitted type"));
        }
        Ok(if any_exhausted {
            Attempt::Exhausted
        } else {
            Attempt::Unsatisfiable("no admitted type can satisfy the merged constraints")
        })
    }
    fn build_kind<'a>(
        &mut self,
        kind: Kind,
        req: &Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
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
    fn finalize<'a>(&mut self, node: Node, req: &Requirements<'a>) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        Ok(match self.check(&node, req)? {
            None => Attempt::Witness(node),
            Some(_) => Attempt::Exhausted,
        })
    }
    fn build_string<'a>(&mut self, req: &Requirements<'a>) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
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
            let primary = req.patterns[0];
            let pattern = self.pattern_of(&primary)?;
            let mut avoid = crate::contract::ScratchVec::new(self.governor);
            for reference in &req.avoid_patterns {
                let pattern = self.pattern_of(reference)?;
                push(&mut avoid, pattern, self.governor)?;
            }
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
                            return Ok(Attempt::Unsatisfiable(
                                "pattern intersected with its avoided patterns is empty",
                            ));
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
    fn pattern_of(&mut self, reference: &PatternRef<'_>) -> Result<&'s Pattern, StopReason> {
        Ok(self.schema.index.locations[reference.owner]
            .patterns
            .get_governed(reference.keyword, self.governor)?
            .expect("prepared pattern exists"))
    }
    fn build_number<'a>(&mut self, req: &Requirements<'a>) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
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
            candidates.push((*bound).clone());
            if let Some(text) = step_integer(bound.as_str(), 1)
                && let Ok(value) = text.parse()
            {
                candidates.push(value);
            }
        }
        if let Some((bound, _)) = &req.maximum {
            candidates.push((*bound).clone());
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
    fn build_array<'a>(
        &mut self,
        req: &Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        let mut items: Vec<Node> = Vec::new();
        let minimum = req.min_items.max(if req.require_duplicate { 2 } else { 0 });
        // Preserve the established preference for materializing a usable
        // prefix, while retaining a shorter valid array when an optional
        // position cannot be constructed. Contains construction already
        // selects its needed positions and must not add gratuitous matches.
        let preferred = if req.contains.is_empty() {
            reference::work(self.governor, req.prefix_items.len())?;
            size(
                req.prefix_items
                    .iter()
                    .map(ScratchVec::len)
                    .max()
                    .unwrap_or(0),
            )
            .min(req.max_items.unwrap_or(u64::MAX))
        } else {
            0
        };
        // Each contains occurrence has its own minimum; one value can
        // satisfy several occurrences. This sum is a construction bound,
        // not a claim that an array needs disjoint matching elements.
        reference::work(self.governor, req.contains.len())?;
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
            let child_path =
                format_scratch_governed(format_args!("{path}/{index}"), self.governor)?;
            let mut applicable = self.array_obligations(req, index)?;
            let exclusions = if req.unique_items {
                items.as_slice()
            } else {
                &[]
            };
            let mut selected = None;
            for &id in &deficient {
                let mut ids = storage::copy(&applicable, self.governor)?;
                push(&mut ids, id, self.governor)?;
                if let Attempt::Witness(node) =
                    self.attempt_many_excluding(&ids, exclusions, &child_path, depth + 1)?
                {
                    selected = Some(node);
                    break;
                }
            }
            let value = if let Some(node) = selected {
                node
            } else if size(items.len()) < minimum || optional {
                if applicable.is_empty() {
                    storage::extend(&mut applicable, &req.unevaluated_items, self.governor)?;
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
                        exclusions,
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
        self.append_required_duplicate(&mut items, req.require_duplicate)?;
        self.finalize(Node::new(Value::Array(items)), req)
    }
    /// Preserve the ordered prefix/items obligations for this position.
    fn array_obligations(
        &mut self,
        req: &Requirements<'_>,
        index: usize,
    ) -> Result<ScratchVec<usize>, StopReason> {
        let mut applicable = ScratchVec::new(self.governor);
        for occurrence in &req.prefix_items {
            reference::work(self.governor, 1)?;
            if let Some(&id) = occurrence.get(index) {
                push(&mut applicable, id, self.governor)?;
            }
        }
        reference::work(self.governor, req.items.len())?;
        for id in applicable_items(req, index) {
            push(&mut applicable, id, self.governor)?;
        }
        Ok(applicable)
    }
    fn append_required_duplicate(
        &mut self,
        items: &mut Vec<Node>,
        required: bool,
    ) -> Result<(), StopReason> {
        if required {
            if items.len() < 2 {
                items.push(
                    items
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Node::new(Value::Null)),
                );
            } else if !has_duplicate(items, self.governor)? {
                items.push(items[0].clone());
            }
        }
        Ok(())
    }
    /// Which contains occurrences still need matches in the construction
    /// so far. These evaluations guide candidate generation; final
    /// whole-schema validation establishes the returned witness.
    fn deficient_contains<'a>(
        &mut self,
        req: &Requirements<'a>,
        items: &[Node],
    ) -> Result<ScratchVec<usize>, StopReason>
    where
        's: 'a,
    {
        let mut deficient = ScratchVec::new(self.governor);
        for &(id, minimum, _) in &req.contains {
            reference::work(self.governor, 1)?;
            let mut count = 0_u64;
            for item in items {
                let report = super::super::evaluate::run(self.schema, id, item, self.governor)?;
                count += u64::from(report.violations.is_empty());
            }
            if count < minimum {
                push(&mut deficient, id, self.governor)?;
            }
        }
        Ok(deficient)
    }
    fn build_object<'a>(
        &mut self,
        req: &Requirements<'a>,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
        let mut required: ScratchVec<&str> = req.required.copy_in(self.governor)?;
        loop {
            let mut added = false;
            for (trigger, deps) in &req.dependent_required {
                reference::work(self.governor, 1)?;
                if contains_name(&required, trigger, self.governor)? {
                    for dep in deps {
                        if !contains_name(&required, dep, self.governor)? {
                            push(&mut required, *dep, self.governor)?;
                            added = true;
                        }
                    }
                }
            }
            if !added {
                break;
            }
        }
        let mut req = req.copy_in(self.governor)?;
        for (trigger, id) in &req.dependent_schemas {
            reference::work(self.governor, 1)?;
            if contains_name(&required, trigger, self.governor)? {
                push(&mut req.positive_musts, *id, self.governor)?;
            }
        }
        let mut members: Vec<Member> = Vec::new();
        for name in &required {
            let child_path = super::super::index::pointer(path, name, self.governor)?;
            let mut ids = ScratchVec::new(self.governor);
            for (key, id) in &req.properties {
                reference::work(self.governor, key.len().min(name.len()).saturating_add(1))?;
                if key == name {
                    push(&mut ids, *id, self.governor)?;
                }
            }
            for (pattern, id) in &req.pattern_properties {
                let compiled = self.pattern_of(pattern)?;
                if compiled.is_match(name, self.governor)? {
                    push(&mut ids, *id, self.governor)?;
                }
            }
            let value = match self.attempt_many(&ids, &child_path, depth + 1)? {
                Attempt::Witness(node) => node,
                other => return Ok(other),
            };
            members.push(Member {
                key: (*name).to_string(),
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
            let mut ids = req.additional_properties.copy_in(self.governor)?;
            storage::extend(&mut ids, &req.unevaluated_properties, self.governor)?;
            let child_path = super::super::index::pointer(path, &name, self.governor)?;
            let value = match self.attempt_many(&ids, &child_path, depth + 1)? {
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
    fn retry_object_with_extra_member<'a>(
        &mut self,
        req: &Requirements<'a>,
        mut members: Vec<Member>,
        required_len: usize,
        path: &str,
        depth: u32,
    ) -> Result<Attempt, StopReason>
    where
        's: 'a,
    {
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
        let mut ids = req.additional_properties.copy_in(self.governor)?;
        storage::extend(&mut ids, &req.unevaluated_properties, self.governor)?;
        let child_path = super::super::index::pointer(path, &name, self.governor)?;
        let value = match self.attempt_many(&ids, &child_path, depth + 1)? {
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
    fn check<'a>(
        &mut self,
        node: &Node,
        req: &Requirements<'a>,
    ) -> Result<Option<&'static str>, StopReason>
    where
        's: 'a,
    {
        self.governor.charge(Resource::Work, 1)?;
        if storage::equivalent_any(&req.excluded, node, self.governor)? {
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
    fn check_number<'a>(
        &mut self,
        number: &crate::json::Number,
        req: &Requirements<'a>,
    ) -> Result<Option<&'static str>, StopReason>
    where
        's: 'a,
    {
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
    fn check_string<'a>(
        &mut self,
        text: &str,
        req: &Requirements<'a>,
    ) -> Result<Option<&'static str>, StopReason>
    where
        's: 'a,
    {
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
            let compiled = self.pattern_of(reference)?;
            if !compiled.is_match(text, self.governor)? {
                return Ok(Some("does not match pattern"));
            }
        }
        for reference in &req.avoid_patterns {
            let compiled = self.pattern_of(reference)?;
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
    fn check_array<'a>(
        &mut self,
        items: &[Node],
        req: &Requirements<'a>,
    ) -> Result<Option<&'static str>, StopReason>
    where
        's: 'a,
    {
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
                    if items[i].equivalent_governed(&items[j], self.governor)? {
                        return Ok(Some("duplicate item violates uniqueItems"));
                    }
                }
            }
        }
        if req.require_duplicate && !has_duplicate(items, self.governor)? {
            return Ok(Some("array must contain a duplicate item"));
        }
        Ok(None)
    }
}
fn check_object(members: &[Member], req: &Requirements<'_>) -> Option<&'static str> {
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
        if !members.iter().any(|member| member.key.as_str() == *name) {
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
fn type_preference(req: &Requirements<'_>) -> impl Iterator<Item = Kind> {
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
    let choices = [
        (Kind::Object, req.types.object, object_hint),
        (Kind::Array, req.types.array, array_hint),
        (Kind::String, req.types.string, string_hint),
        (
            Kind::Number,
            !matches!(req.types.num, NumKind::None),
            number_hint,
        ),
        (Kind::Boolean, req.types.boolean, false),
        (Kind::Null, req.types.null, false),
    ];
    [true, false].into_iter().flat_map(move |hint| {
        choices
            .into_iter()
            .filter_map(move |(kind, enabled, hinted)| (enabled && hinted == hint).then_some(kind))
    })
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
fn has_duplicate(items: &[Node], governor: &mut Governor) -> Result<bool, StopReason> {
    for i in 0..items.len() {
        for j in 0..i {
            if items[i].equivalent_governed(&items[j], governor)? {
                return Ok(true);
            }
        }
    }
    governor.check()?;
    Ok(false)
}

fn contains_name(
    names: &[&str],
    wanted: &str,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for name in names {
        reference::work(governor, name.len().min(wanted.len()).saturating_add(1))?;
        if *name == wanted {
            return Ok(true);
        }
    }
    Ok(false)
}
