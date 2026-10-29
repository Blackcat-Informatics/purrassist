// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Stages 3–4 (`and`-merge and preparation): [`Requirements`] accumulates
//! every [`Facet`] a search path has committed to so far into one merged
//! obligation set, detecting the contradictions that make an `allOf`
//! (or a merged `not`-branch) exactly unsatisfiable — an empty [`TypeSet`],
//! a numeric/length/property-count bound crossing itself, or two
//! irreconcilable `const` values — before generation (stage 5, `finish.rs`)
//! ever tries to build a value.
use super::super::reference::work;
use super::canon::{Facet, NumKind, PatternRef, TypeSet};
use super::storage::{self, CopyIn, push};
use crate::contract::ScratchCharge;
use crate::json::Node;
use crate::{Governor, ScratchVec, StopReason};

/// Everything a search path has merged so far. `And`-merging is simply
/// [`Requirements::apply`] folded over every [`Facet`] encountered while
/// walking a `Canon::And`.
#[derive(Debug)]
pub(super) struct Requirements<'s> {
    pub types: TypeSet,
    pub const_value: Option<&'s Node>,
    pub excluded: ScratchVec<&'s Node>,
    pub enum_values: Option<ScratchVec<&'s Node>>,
    pub minimum: Option<(&'s crate::json::Number, bool)>,
    pub maximum: Option<(&'s crate::json::Number, bool)>,
    pub multiples_of: ScratchVec<&'s crate::json::Number>,
    pub not_multiples_of: ScratchVec<&'s crate::json::Number>,
    pub min_length: u64,
    pub max_length: Option<u64>,
    pub patterns: ScratchVec<PatternRef<'s>>,
    pub avoid_patterns: ScratchVec<PatternRef<'s>>,
    pub format: Option<&'s str>,
    pub avoid_formats: ScratchVec<&'s str>,
    pub min_items: u64,
    pub max_items: Option<u64>,
    pub unique_items: bool,
    pub require_duplicate: bool,
    pub prefix_items: ScratchVec<ScratchVec<usize>>,
    /// Every `items` occurrence merged so far, paired with how many leading
    /// elements *that occurrence's own* `prefixItems` (0 if none) already
    /// covers — see [`Facet::Items`]'s doc comment for why this pairing,
    /// not one shared prefix length, is load-bearing.
    pub items: ScratchVec<(usize, usize)>,
    pub contains: ScratchVec<(usize, u64, Option<u64>)>,
    pub min_properties: u64,
    pub max_properties: Option<u64>,
    pub properties: ScratchVec<(&'s str, usize)>,
    pub pattern_properties: ScratchVec<(PatternRef<'s>, usize)>,
    pub additional_properties: ScratchVec<usize>,
    pub property_names: ScratchVec<usize>,
    pub required: ScratchVec<&'s str>,
    pub dependent_required: ScratchVec<(&'s str, ScratchVec<&'s str>)>,
    pub dependent_schemas: ScratchVec<(&'s str, usize)>,
    pub unevaluated_properties: ScratchVec<usize>,
    pub unevaluated_items: ScratchVec<usize>,
    pub opaque_nots: ScratchVec<usize>,
    /// Extra whole-value schemas (triggered `dependentSchemas`) the final
    /// candidate must additionally satisfy, checked directly with the real
    /// evaluator rather than folded into member-by-member construction.
    pub positive_musts: ScratchVec<usize>,
    /// Set once a merge step detects a direct, closed-form contradiction
    /// (an empty [`TypeSet`], a crossed bound, or incompatible literals),
    /// independent of whatever stage 5 later fails to construct.
    pub contradiction: Option<&'static str>,
}
impl Requirements<'_> {
    pub(super) fn new(governor: &Governor) -> Self {
        Self {
            types: TypeSet::universal(),
            const_value: None,
            excluded: ScratchVec::new(governor),
            enum_values: None,
            minimum: None,
            maximum: None,
            multiples_of: ScratchVec::new(governor),
            not_multiples_of: ScratchVec::new(governor),
            min_length: 0,
            max_length: None,
            patterns: ScratchVec::new(governor),
            avoid_patterns: ScratchVec::new(governor),
            format: None,
            avoid_formats: ScratchVec::new(governor),
            min_items: 0,
            max_items: None,
            unique_items: false,
            require_duplicate: false,
            prefix_items: ScratchVec::new(governor),
            items: ScratchVec::new(governor),
            contains: ScratchVec::new(governor),
            min_properties: 0,
            max_properties: None,
            properties: ScratchVec::new(governor),
            pattern_properties: ScratchVec::new(governor),
            additional_properties: ScratchVec::new(governor),
            property_names: ScratchVec::new(governor),
            required: ScratchVec::new(governor),
            dependent_required: ScratchVec::new(governor),
            dependent_schemas: ScratchVec::new(governor),
            unevaluated_properties: ScratchVec::new(governor),
            unevaluated_items: ScratchVec::new(governor),
            opaque_nots: ScratchVec::new(governor),
            positive_musts: ScratchVec::new(governor),
            contradiction: None,
        }
    }
}
impl<'s> Requirements<'s> {
    pub(super) const fn is_contradictory(&self) -> bool {
        self.contradiction.is_some()
    }
    fn fail(&mut self, reason: &'static str) {
        self.contradiction.get_or_insert(reason);
    }
    /// Fold one [`Facet`] into this merged obligation set (the `and`-merge
    /// step, stage 3).
    pub(super) fn apply(
        &mut self,
        facet: &Facet<'s>,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        work(governor, 1)?;
        self.merge_one(facet, governor)?;
        self.check_contradictions(governor)
    }
    fn merge_const(&mut self, value: &'s Node, governor: &mut Governor) -> Result<(), StopReason> {
        if let Some(existing) = &self.const_value {
            if !existing.equivalent_governed(value, governor)? {
                self.fail("two incompatible const values");
            }
        } else {
            self.const_value = Some(value);
        }
        Ok(())
    }
    fn merge_enum(
        &mut self,
        values: &'s [Node],
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let merged = if let Some(mut existing) = self.enum_values.take() {
            // Keep the accepted prefix in its original order without
            // allocating another Requirements-owned candidate vector.
            let mut kept = 0;
            for index in 0..existing.len() {
                governor.charge(crate::contract::Resource::Work, 1)?;
                if crate::json::equivalent_any(values, existing[index], governor)? {
                    governor.charge(crate::contract::Resource::Work, 1)?;
                    existing.as_mut_slice().swap(kept, index);
                    kept += 1;
                }
            }
            while existing.len() > kept {
                work(governor, 1)?;
                existing.pop();
            }
            existing
        } else {
            let mut admitted = ScratchVec::new(governor);
            for value in values {
                push(&mut admitted, value, governor)?;
            }
            admitted
        };
        if merged.is_empty() {
            self.fail("enum intersection is empty");
        }
        self.enum_values = Some(merged);
        Ok(())
    }
    fn merge_one(&mut self, facet: &Facet<'s>, governor: &mut Governor) -> Result<(), StopReason> {
        match facet {
            Facet::Type(set) => {
                self.types = self.types.intersect(*set);
                if self.types.is_empty() {
                    self.fail("type intersection is empty");
                }
            }
            Facet::Const(value) => self.merge_const(value, governor)?,
            Facet::Excluded(values) => {
                for value in *values {
                    push(&mut self.excluded, value, governor)?;
                }
            }
            Facet::Enum(values) => self.merge_enum(values, governor)?,
            Facet::Minimum(bound, exclusive) => {
                self.minimum = Some(match &self.minimum {
                    Some((existing, existing_exclusive))
                        if existing.compare_governed(bound, governor)?.is_gt() =>
                    {
                        (*existing, *existing_exclusive)
                    }
                    Some((existing, existing_exclusive))
                        if existing.compare_governed(bound, governor)?.is_eq() =>
                    {
                        (*existing, *existing_exclusive || *exclusive)
                    }
                    _ => (*bound, *exclusive),
                });
            }
            Facet::Maximum(bound, exclusive) => {
                self.maximum = Some(match &self.maximum {
                    Some((existing, existing_exclusive))
                        if existing.compare_governed(bound, governor)?.is_lt() =>
                    {
                        (*existing, *existing_exclusive)
                    }
                    Some((existing, existing_exclusive))
                        if existing.compare_governed(bound, governor)?.is_eq() =>
                    {
                        (*existing, *existing_exclusive || *exclusive)
                    }
                    _ => (*bound, *exclusive),
                });
            }
            Facet::MultipleOf(divisor) => push(&mut self.multiples_of, *divisor, governor)?,
            Facet::NotMultipleOf(divisor) => push(&mut self.not_multiples_of, *divisor, governor)?,
            Facet::MinLength(bound) => self.min_length = self.min_length.max(*bound),
            Facet::MaxLength(bound) => {
                self.max_length = Some(self.max_length.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::Pattern(reference) => push(&mut self.patterns, *reference, governor)?,
            Facet::AvoidPattern(reference) => push(&mut self.avoid_patterns, *reference, governor)?,
            Facet::Format(name) => self.format = Some(*name),
            Facet::AvoidFormat(name) => push(&mut self.avoid_formats, *name, governor)?,
            Facet::MinItems(bound) => self.min_items = self.min_items.max(*bound),
            Facet::MaxItems(bound) => {
                self.max_items = Some(self.max_items.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::UniqueItems => self.unique_items = true,
            Facet::RequireDuplicate => self.require_duplicate = true,
            Facet::PrefixItems(ids) => {
                push(&mut self.prefix_items, ids.copy_in(governor)?, governor)?;
            }
            Facet::Items(id, own_prefix_len) => {
                push(&mut self.items, (*id, *own_prefix_len), governor)?;
            }
            Facet::Contains(id, minimum, maximum) => {
                push(&mut self.contains, (*id, *minimum, *maximum), governor)?;
            }
            Facet::MinProperties(bound) => self.min_properties = self.min_properties.max(*bound),
            Facet::MaxProperties(bound) => {
                self.max_properties = Some(self.max_properties.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::Properties(pairs) => storage::extend(&mut self.properties, pairs, governor)?,
            Facet::PatternProperties(pairs) => {
                storage::extend(&mut self.pattern_properties, pairs, governor)?;
            }
            Facet::AdditionalProperties(id) => {
                push(&mut self.additional_properties, *id, governor)?;
            }
            Facet::PropertyNames(id) => push(&mut self.property_names, *id, governor)?,
            Facet::Required(names) => storage::extend(&mut self.required, names, governor)?,
            Facet::DependentRequired(pairs) => {
                storage::extend(&mut self.dependent_required, pairs, governor)?;
            }
            Facet::DependentSchemas(pairs) => {
                storage::extend(&mut self.dependent_schemas, pairs, governor)?;
            }
            Facet::UnevaluatedProperties(id) => {
                push(&mut self.unevaluated_properties, *id, governor)?;
            }
            Facet::UnevaluatedItems(id) => push(&mut self.unevaluated_items, *id, governor)?,
        }
        Ok(())
    }
    /// Crossed shape-specific bounds exclude only that shape. They prove
    /// a branch contradictory when no admitted shape remains; literals and
    /// enum members are still checked against the resulting type set.
    fn check_contradictions(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if let (Some((min, min_exclusive)), Some((max, max_exclusive))) =
            (&self.minimum, &self.maximum)
        {
            let comparison = min.compare_governed(max, governor)?;
            if comparison.is_gt() || (comparison.is_eq() && (*min_exclusive || *max_exclusive)) {
                self.types.num = NumKind::None;
            }
        }
        if let Some(max) = self.max_length
            && self.min_length > max
        {
            self.types.string = false;
        }
        if let Some(max) = self.max_items
            && self
                .min_items
                .max(if self.require_duplicate { 2 } else { 0 })
                > max
        {
            self.types.array = false;
        }
        if let Some(max) = self.max_properties
            && self.min_properties > max
        {
            self.types.object = false;
        }
        work(governor, self.contains.len())?;
        if self
            .contains
            .iter()
            .any(|(_, minimum, maximum)| maximum.is_some_and(|maximum| *minimum > maximum))
            || (self.unique_items && self.require_duplicate)
        {
            self.types.array = false;
        }
        if self.types.is_empty() {
            self.fail("no admitted type satisfies its shape-specific bounds");
        }
        if let Some(const_value) = self.const_value {
            if storage::equivalent_any(&self.excluded, const_value, governor)? {
                self.fail("const value is excluded");
            }
            if let Some(values) = self.enum_values.as_ref()
                && !storage::equivalent_any(values, const_value, governor)?
            {
                self.fail("const value is not in enum");
            }
        }
        if let Some(values) = self.enum_values.as_ref() {
            let mut all_excluded = true;
            for value in values {
                work(governor, 1)?;
                if !storage::equivalent_any(&self.excluded, value, governor)? {
                    all_excluded = false;
                    break;
                }
            }
            if all_excluded {
                self.fail("every enum value is excluded");
            }
        }
        Ok(())
    }
}

impl ScratchCharge for Requirements<'_> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 1536);
        1536
    };
}
impl CopyIn for Requirements<'_> {
    fn copy_in(&self, governor: &mut Governor) -> Result<Self, StopReason> {
        work(governor, 35)?;
        Ok(Self {
            types: self.types,
            const_value: self.const_value,
            excluded: self.excluded.copy_in(governor)?,
            enum_values: self
                .enum_values
                .as_ref()
                .map(|values| values.copy_in(governor))
                .transpose()?,
            minimum: self.minimum,
            maximum: self.maximum,
            multiples_of: self.multiples_of.copy_in(governor)?,
            not_multiples_of: self.not_multiples_of.copy_in(governor)?,
            min_length: self.min_length,
            max_length: self.max_length,
            patterns: self.patterns.copy_in(governor)?,
            avoid_patterns: self.avoid_patterns.copy_in(governor)?,
            format: self.format,
            avoid_formats: self.avoid_formats.copy_in(governor)?,
            min_items: self.min_items,
            max_items: self.max_items,
            unique_items: self.unique_items,
            require_duplicate: self.require_duplicate,
            prefix_items: self.prefix_items.copy_in(governor)?,
            items: self.items.copy_in(governor)?,
            contains: self.contains.copy_in(governor)?,
            min_properties: self.min_properties,
            max_properties: self.max_properties,
            properties: self.properties.copy_in(governor)?,
            pattern_properties: self.pattern_properties.copy_in(governor)?,
            additional_properties: self.additional_properties.copy_in(governor)?,
            property_names: self.property_names.copy_in(governor)?,
            required: self.required.copy_in(governor)?,
            dependent_required: self.dependent_required.copy_in(governor)?,
            dependent_schemas: self.dependent_schemas.copy_in(governor)?,
            unevaluated_properties: self.unevaluated_properties.copy_in(governor)?,
            unevaluated_items: self.unevaluated_items.copy_in(governor)?,
            opaque_nots: self.opaque_nots.copy_in(governor)?,
            positive_musts: self.positive_musts.copy_in(governor)?,
            contradiction: self.contradiction,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::{Facet, Requirements};
    use crate::{Budget, Governor, StopReason, contract::Resource, json};

    #[test]
    fn comparison_refusal_in_const_and_enum_merge_never_marks_a_contradiction() {
        let first = json::parse(r#"{"x":1}"#).unwrap();
        let equivalent = json::parse(r#"{"x":1.0}"#).unwrap();
        for (initial, next) in [
            (Facet::Const(&first), Facet::Const(&equivalent)),
            (
                Facet::Enum(core::slice::from_ref(&first)),
                Facet::Enum(core::slice::from_ref(&equivalent)),
            ),
        ] {
            let mut governor = Governor::new(Budget::default());
            let mut requirements = Requirements::new(&governor);
            requirements.apply(&initial, &mut governor).unwrap();
            // Preparation and the second merge share one meter. Reserve the
            // remaining allowance so the next comparison cannot admit its stack.
            let remaining = governor.limits().scratch_bytes - governor.scratch_meter().live();
            let occupied = governor.lease_scratch(remaining).unwrap();
            assert_eq!(
                requirements.apply(&next, &mut governor),
                Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
            );
            assert!(!requirements.is_contradictory());
            assert_eq!(
                governor.check(),
                Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
            );
            drop(requirements);
            assert_eq!(governor.scratch_meter().live(), remaining);
            drop(occupied);
            assert_eq!(governor.scratch_meter().live(), 0);
        }
    }
}
