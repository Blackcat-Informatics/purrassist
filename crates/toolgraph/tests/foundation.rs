// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Behavioral boundaries for caller-owned vocabulary, loss profiles and masks.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    BindingProblem, FieldMask, FieldMaskError, FieldMaskKind, ModelFamily, PrefixTable,
    ProjectionTarget, SOURCE_REPRESENTATION, TERM_NAMES, TermBindings, Vocabulary, VocabularyError,
    register_loss_profile,
};
use purrdf_core::loss::{LossDefinition, LossProfile};

fn terms(namespace: &str) -> TermBindings {
    TermBindings {
        namespace_iri: namespace.into(),
        local_names: TERM_NAMES
            .iter()
            .map(|name| ((*name).into(), format!("{namespace}{name}")))
            .collect(),
    }
}

fn vocabulary(
    prefixes: PrefixTable,
    terms: TermBindings,
    profile: LossProfile,
) -> Result<Vocabulary, VocabularyError> {
    Vocabulary::new("tg", prefixes, terms, profile)
}

#[test]
fn caller_namespace_is_complete_distinct_and_shared() {
    let namespace = "https://example.org/owned#";
    let prefixes = PrefixTable(vec![
        ("tg".into(), namespace.into()),
        ("ex".into(), "urn:example:".into()),
    ]);
    let profile = register_loss_profile(LossProfile::builtin()).unwrap();
    let valid = vocabulary(prefixes.clone(), terms(namespace), profile.clone()).unwrap();
    assert_eq!(valid.term("Tool"), Some("https://example.org/owned#Tool"));
    assert_eq!(
        valid.namespaces().expand_iri("tg:Tool").unwrap(),
        valid.term("Tool").unwrap()
    );
    assert_eq!(valid.prefixes().0[0].0, "ex");
    for (case, problem) in [
        (0, BindingProblem::UnknownTerm),
        (1, BindingProblem::Duplicate),
        (2, BindingProblem::AliasedTerm),
        (3, BindingProblem::ForeignTerm),
        (4, BindingProblem::InvalidAbsoluteIri),
    ] {
        let mut altered = terms(namespace);
        match case {
            0 => altered.local_names[0].0 = "unknown".into(),
            1 => altered.local_names[1].0 = altered.local_names[0].0.clone(),
            2 => altered.local_names[1].1 = altered.local_names[0].1.clone(),
            3 => altered.local_names[0].1 = "urn:foreign:term".into(),
            _ => altered.local_names[0].1 = "relative".into(),
        }
        assert!(
            matches!(vocabulary(prefixes.clone(), altered, profile.clone()), Err(VocabularyError::Binding { problem: actual, .. }) if actual == problem)
        );
    }
    let mut incomplete = terms(namespace);
    incomplete.local_names.pop();
    assert!(matches!(
        vocabulary(prefixes.clone(), incomplete, profile.clone()),
        Err(VocabularyError::MissingTerm {
            name: "waiverSource"
        })
    ));
    let mut duplicate = prefixes.clone();
    duplicate.0.push(("tg".into(), "urn:other:".into()));
    assert!(matches!(
        vocabulary(duplicate, terms(namespace), profile.clone()),
        Err(VocabularyError::Binding {
            problem: BindingProblem::Duplicate,
            ..
        })
    ));
    for (prefix, iri) in [
        ("bad prefix", namespace),
        ("tg", "relative"),
        ("other", namespace),
        ("tg", "urn:other:"),
    ] {
        assert!(
            vocabulary(
                PrefixTable(vec![(prefix.into(), iri.into())]),
                terms(namespace),
                profile.clone()
            )
            .is_err()
        );
    }
    assert!(matches!(
        vocabulary(prefixes, terms(namespace), LossProfile::builtin()),
        Err(VocabularyError::LossMeaning { .. })
    ));
    let other = "urn:caller:";
    let alternate = Vocabulary::new(
        "",
        PrefixTable(vec![(String::new(), other.into())]),
        terms(other),
        profile,
    )
    .unwrap();
    assert_eq!(alternate.namespaces().primary_ns(), other);
}

#[test]
fn loss_registry_keeps_meanings_and_lane_membership() {
    let profile = register_loss_profile(LossProfile::builtin()).unwrap();
    assert_eq!(register_loss_profile(profile.clone()).unwrap(), profile);
    assert_eq!(
        profile
            .codes_for(
                SOURCE_REPRESENTATION,
                ProjectionTarget::McpExtended.representation()
            )
            .count(),
        0
    );
    assert!(
        profile
            .description(
                SOURCE_REPRESENTATION,
                "apparmor-4.1",
                "confinement-not-expressible"
            )
            .is_none()
    );
    assert!(
        profile
            .description(
                SOURCE_REPRESENTATION,
                "llama-gbnf",
                "confinement-not-expressible"
            )
            .is_some()
    );
    assert!(
        profile
            .description(
                SOURCE_REPRESENTATION,
                "compact-signature-v1",
                "schema-keyword-not-expressible"
            )
            .is_some()
    );
    for family in ModelFamily::ALL {
        let target = ProjectionTarget::Prompt(family).representation();
        assert!(
            profile
                .description(
                    SOURCE_REPRESENTATION,
                    target,
                    "authorization-not-expressible"
                )
                .is_some()
        );
        assert_eq!(
            profile
                .description(SOURCE_REPRESENTATION, target, "description-not-expressible")
                .is_some(),
            family == ModelFamily::CompactTypeScript,
        );
    }
    let conflicting = LossProfile::builtin()
        .extend([LossDefinition {
            from: SOURCE_REPRESENTATION.into(),
            to: "llama-gbnf".into(),
            code: "confinement-not-expressible".into(),
            description: "Confinement is already enforced.".into(),
        }])
        .unwrap();
    assert!(register_loss_profile(conflicting).is_err());
}

#[test]
fn field_masks_consume_only_the_closed_language() {
    for (input, canonical) in [
        (" .customer.name ", ".customer.name"),
        (".[]", ".[]"),
        ("{name,total}", "{name, total}"),
        (".orders[]?", ".orders[]?"),
    ] {
        let mask = FieldMask::parse(input).unwrap();
        assert_eq!(mask.to_string(), canonical);
        assert_eq!(FieldMask::parse(canonical).unwrap(), mask);
    }
    let mask = FieldMask::parse(".orders[]?").unwrap();
    assert!(matches!(
        mask.kind(),
        FieldMaskKind::OptionalElements("orders")
    ));
    for input in [
        "",
        ".",
        ".a..b",
        ".a[0]",
        ".a[]",
        ".a[]?.b",
        ".[]?",
        "{}",
        "{a,}",
        ".a | length",
        ".é",
        ".aé",
        "{a: .b}",
    ] {
        let FieldMaskError::Unsupported { offset } = FieldMask::parse(input).unwrap_err() else {
            panic!("{input}");
        };
        assert!(input.is_char_boundary(offset));
    }
    assert_eq!(
        FieldMask::parse("{a, a}"),
        Err(FieldMaskError::DuplicateField { offset: 4 })
    );
}

#[test]
fn field_mask_scans_and_retention_obey_the_governor() {
    let input = "{customer, order, total}";
    let mut governor = Governor::new(Budget::default());
    FieldMask::parse_governed(input, &mut governor).unwrap();
    let usage = governor.usage();
    for (resource, needed) in [
        (Resource::Work, usage.work),
        (Resource::ScratchBytes, usage.peak_scratch_bytes),
    ] {
        let mut budget = Budget::default();
        match resource {
            Resource::Work => budget.work = needed - 1,
            _ => budget.scratch_bytes = needed - 1,
        }
        assert_eq!(
            FieldMask::parse_governed(input, &mut Governor::new(budget)),
            Err(FieldMaskError::Resource(StopReason::BudgetExceeded(
                resource
            )))
        );
    }
    let budget = Budget {
        input_bytes: (input.len() - 1) as u64,
        ..Budget::default()
    };
    assert_eq!(
        FieldMask::parse_governed(input, &mut Governor::new(budget)),
        Err(FieldMaskError::Resource(StopReason::BudgetExceeded(
            Resource::InputBytes
        )))
    );
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        FieldMask::parse_governed(
            input,
            &mut Governor::with_cancellation(Budget::default(), token)
        ),
        Err(FieldMaskError::Resource(StopReason::Cancelled))
    );
}
