// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Stages 3–4 (`and`-merge and preparation): [`Requirements`] accumulates
//! every [`Facet`] a search path has committed to so far into one merged
//! obligation set, detecting the contradictions that make an `allOf`
//! (or a merged `not`-branch) exactly unsatisfiable — an empty [`TypeSet`],
//! a numeric/length/property-count bound crossing itself, or two
//! irreconcilable `const` values — before generation (stage 5, `finish.rs`)
//! ever tries to build a value.
use super::canon::{Facet, PatternRef, TypeSet};
use crate::json::Node;

/// Everything a search path has merged so far. `And`-merging is simply
/// [`Requirements::apply`] folded over every [`Facet`] encountered while
/// walking a `Canon::And`.
#[derive(Clone, Debug)]
pub(super) struct Requirements {
    pub types: TypeSet,
    pub const_value: Option<Node>,
    pub excluded: Vec<Node>,
    pub enum_values: Option<Vec<Node>>,
    pub minimum: Option<(crate::json::Number, bool)>,
    pub maximum: Option<(crate::json::Number, bool)>,
    pub multiples_of: Vec<crate::json::Number>,
    pub not_multiples_of: Vec<crate::json::Number>,
    pub min_length: u64,
    pub max_length: Option<u64>,
    pub patterns: Vec<PatternRef>,
    pub avoid_patterns: Vec<PatternRef>,
    pub format: Option<String>,
    pub avoid_formats: Vec<String>,
    pub min_items: u64,
    pub max_items: Option<u64>,
    pub unique_items: bool,
    pub require_duplicate: bool,
    pub prefix_items: Vec<Vec<usize>>,
    /// Every `items` occurrence merged so far, paired with how many leading
    /// elements *that occurrence's own* `prefixItems` (0 if none) already
    /// covers — see [`Facet::Items`]'s doc comment for why this pairing,
    /// not one shared prefix length, is load-bearing.
    pub items: Vec<(usize, usize)>,
    pub contains: Vec<(usize, u64, Option<u64>)>,
    pub min_contains: Option<u64>,
    pub max_contains: Option<u64>,
    pub min_properties: u64,
    pub max_properties: Option<u64>,
    pub properties: Vec<(String, usize)>,
    pub pattern_properties: Vec<(PatternRef, usize)>,
    pub additional_properties: Vec<usize>,
    pub property_names: Vec<usize>,
    pub required: Vec<String>,
    pub dependent_required: Vec<(String, Vec<String>)>,
    pub dependent_schemas: Vec<(String, usize)>,
    pub unevaluated_properties: Vec<usize>,
    pub unevaluated_items: Vec<usize>,
    pub opaque_nots: Vec<usize>,
    /// Extra whole-value schemas (triggered `dependentSchemas`) the final
    /// candidate must additionally satisfy, checked directly with the real
    /// evaluator rather than folded into member-by-member construction.
    pub positive_musts: Vec<usize>,
    /// Set once a merge step detects a direct, closed-form contradiction
    /// (an empty [`TypeSet`], a crossed bound, or incompatible literals),
    /// independent of whatever stage 5 later fails to construct.
    pub contradiction: Option<&'static str>,
}
impl Default for Requirements {
    fn default() -> Self {
        Self {
            types: TypeSet::universal(),
            const_value: None,
            excluded: Vec::new(),
            enum_values: None,
            minimum: None,
            maximum: None,
            multiples_of: Vec::new(),
            not_multiples_of: Vec::new(),
            min_length: 0,
            max_length: None,
            patterns: Vec::new(),
            avoid_patterns: Vec::new(),
            format: None,
            avoid_formats: Vec::new(),
            min_items: 0,
            max_items: None,
            unique_items: false,
            require_duplicate: false,
            prefix_items: Vec::new(),
            items: Vec::new(),
            contains: Vec::new(),
            min_contains: None,
            max_contains: None,
            min_properties: 0,
            max_properties: None,
            properties: Vec::new(),
            pattern_properties: Vec::new(),
            additional_properties: Vec::new(),
            property_names: Vec::new(),
            required: Vec::new(),
            dependent_required: Vec::new(),
            dependent_schemas: Vec::new(),
            unevaluated_properties: Vec::new(),
            unevaluated_items: Vec::new(),
            opaque_nots: Vec::new(),
            positive_musts: Vec::new(),
            contradiction: None,
        }
    }
}
impl Requirements {
    pub(super) const fn is_contradictory(&self) -> bool {
        self.contradiction.is_some()
    }
    fn fail(&mut self, reason: &'static str) {
        self.contradiction.get_or_insert(reason);
    }
    /// Fold one [`Facet`] into this merged obligation set (the `and`-merge
    /// step, stage 3).
    pub(super) fn apply(&mut self, facet: &Facet) {
        self.merge_one(facet);
        self.check_contradictions();
    }
    fn merge_one(&mut self, facet: &Facet) {
        match facet {
            Facet::Type(set) => {
                self.types = self.types.intersect(*set);
                if self.types.is_empty() {
                    self.fail("type intersection is empty");
                }
            }
            Facet::Const(value) => {
                if let Some(existing) = &self.const_value {
                    if !existing.equivalent(value) {
                        self.fail("two incompatible const values");
                    }
                } else {
                    self.const_value = Some(value.clone());
                }
            }
            Facet::Excluded(values) => self.excluded.extend(values.iter().cloned()),
            Facet::Enum(values) => {
                let merged = self.enum_values.take().map_or_else(
                    || values.clone(),
                    |existing| {
                        existing
                            .into_iter()
                            .filter(|value| values.iter().any(|other| other.equivalent(value)))
                            .collect()
                    },
                );
                if merged.is_empty() {
                    self.fail("enum intersection is empty");
                }
                self.enum_values = Some(merged);
            }
            Facet::Minimum(bound, exclusive) => {
                self.minimum = Some(match &self.minimum {
                    Some((existing, existing_exclusive)) if existing.compare(bound).is_ge() => {
                        (existing.clone(), *existing_exclusive)
                    }
                    _ => (bound.clone(), *exclusive),
                });
            }
            Facet::Maximum(bound, exclusive) => {
                self.maximum = Some(match &self.maximum {
                    Some((existing, existing_exclusive)) if existing.compare(bound).is_le() => {
                        (existing.clone(), *existing_exclusive)
                    }
                    _ => (bound.clone(), *exclusive),
                });
            }
            Facet::MultipleOf(divisor) => self.multiples_of.push(divisor.clone()),
            Facet::NotMultipleOf(divisor) => self.not_multiples_of.push(divisor.clone()),
            Facet::MinLength(bound) => self.min_length = self.min_length.max(*bound),
            Facet::MaxLength(bound) => {
                self.max_length = Some(self.max_length.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::Pattern(reference) => self.patterns.push(reference.clone()),
            Facet::AvoidPattern(reference) => self.avoid_patterns.push(reference.clone()),
            Facet::Format(name) => self.format = Some(name.clone()),
            Facet::AvoidFormat(name) => self.avoid_formats.push(name.clone()),
            Facet::MinItems(bound) => self.min_items = self.min_items.max(*bound),
            Facet::MaxItems(bound) => {
                self.max_items = Some(self.max_items.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::UniqueItems => self.unique_items = true,
            Facet::RequireDuplicate => self.require_duplicate = true,
            Facet::PrefixItems(ids) => self.prefix_items.push(ids.clone()),
            Facet::Items(id, own_prefix_len) => self.items.push((*id, *own_prefix_len)),
            Facet::Contains(id) => self.contains.push((*id, 0, None)),
            Facet::MinContains(bound) => {
                self.min_contains = Some(self.min_contains.map_or(*bound, |m| m.max(*bound)));
            }
            Facet::MaxContains(bound) => {
                self.max_contains = Some(self.max_contains.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::MinProperties(bound) => self.min_properties = self.min_properties.max(*bound),
            Facet::MaxProperties(bound) => {
                self.max_properties = Some(self.max_properties.map_or(*bound, |m| m.min(*bound)));
            }
            Facet::Properties(pairs) => self.properties.extend(pairs.iter().cloned()),
            Facet::PatternProperties(pairs) => {
                self.pattern_properties.extend(pairs.iter().cloned());
            }
            Facet::AdditionalProperties(id) => self.additional_properties.push(*id),
            Facet::PropertyNames(id) => self.property_names.push(*id),
            Facet::Required(names) => self.required.extend(names.iter().cloned()),
            Facet::DependentRequired(pairs) => {
                self.dependent_required.extend(pairs.iter().cloned());
            }
            Facet::DependentSchemas(pairs) => self.dependent_schemas.extend(pairs.iter().cloned()),
            Facet::UnevaluatedProperties(id) => self.unevaluated_properties.push(*id),
            Facet::UnevaluatedItems(id) => self.unevaluated_items.push(*id),
        }
    }
    /// Cross-field contradictions a single [`Facet`] cannot reveal on its
    /// own (a crossed bound, or a literal that conflicts with another
    /// merged obligation), re-checked after every [`Requirements::merge_one`].
    fn check_contradictions(&mut self) {
        if let (Some((min, min_exclusive)), Some((max, max_exclusive))) =
            (&self.minimum, &self.maximum)
        {
            let comparison = min.compare(max);
            if comparison.is_gt() || (comparison.is_eq() && (*min_exclusive || *max_exclusive)) {
                self.fail("minimum exceeds maximum");
            }
        }
        if let Some(max) = self.max_length
            && self.min_length > max
        {
            self.fail("minLength exceeds maxLength");
        }
        if let Some(max) = self.max_items
            && self.min_items > max
        {
            self.fail("minItems exceeds maxItems");
        }
        if let Some(max) = self.max_properties
            && self.min_properties > max
        {
            self.fail("minProperties exceeds maxProperties");
        }
        if let Some(max) = self.max_contains
            && self.min_contains.unwrap_or(0) > max
        {
            self.fail("minContains exceeds maxContains");
        }
        if let Some(const_value) = self.const_value.clone() {
            if self
                .excluded
                .iter()
                .any(|excluded| excluded.equivalent(&const_value))
            {
                self.fail("const value is excluded");
            }
            if let Some(values) = self.enum_values.clone()
                && !values.iter().any(|value| value.equivalent(&const_value))
            {
                self.fail("const value is not in enum");
            }
        }
        if let Some(values) = self.enum_values.clone()
            && values.iter().all(|value| {
                self.excluded
                    .iter()
                    .any(|excluded| excluded.equivalent(value))
            })
        {
            self.fail("every enum value is excluded");
        }
    }
}
