// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Complete callable bindings, ordering, provenance and explicit refusal boundaries.

#[path = "../examples/support/catalog.rs"]
pub mod catalog;

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{BindingError, CallableBinding};
use purrdf_core::fno::FnoCatalog;

#[test]
fn upstream_reader_bindings_keep_declaration_order_and_native_source_records() {
    let catalog = catalog::read().expect("real upstream FnO reader accepts the caller fixture");
    let mut governor = Governor::new(Budget::default());
    let binding = CallableBinding::admit(&catalog, 0, &mut governor).expect("complete mapping");
    let parameters: Vec<_> = binding.parameters().collect();
    assert_eq!(
        parameters
            .iter()
            .map(|value| value.parameter.iri.as_str())
            .collect::<Vec<_>>(),
        ["urn:example:z", "urn:example:a"]
    );
    assert_eq!(
        parameters
            .iter()
            .map(|value| value.mapping.implementation_property.as_str())
            .collect::<Vec<_>>(),
        ["z value", "a value"]
    );
    assert_eq!(
        binding
            .parameter(0)
            .expect("first declaration")
            .mapping
            .node
            .label,
        "z"
    );
    assert_eq!(binding.output().mapping.implementation_property, "z value");
    assert_eq!(binding.mapping().node.scope.ordinal(), 42);
    assert_eq!(parameters[0].mapping.node.scope.ordinal(), 42);
    assert_eq!(
        &binding.function().label,
        &catalog.as_input().functions[0].label
    );
    assert!(
        matches!(&binding.function().label, purrdf_core::TermValue::Literal { language: Some(language), .. } if language == "de")
    );
    assert_eq!(binding.implementation().see_also, "urn:example:host:reader");
    assert!(governor.usage().work > 0);
}

#[test]
fn incomplete_and_ambiguous_bindings_preserve_actionable_original_operands() {
    for case in 0..8 {
        let mut input = catalog::input();
        match case {
            0 => input.params[0].predicate = None,
            1 => input.functions[0].output.predicate = None,
            2 => {
                input.mappings[0].parameter_mappings.pop();
            }
            3 => {
                let mut duplicate = input.mappings[0].parameter_mappings[0].clone();
                duplicate.node.label = "other-parameter-binding".into();
                input.mappings[0].parameter_mappings.push(duplicate);
            }
            4 => input.mappings[0].return_mappings.clear(),
            5 => {
                let mut duplicate = input.mappings[0].return_mappings[0].clone();
                duplicate.node.label = "other-return-binding".into();
                input.mappings[0].return_mappings.push(duplicate);
            }
            6 => {
                let predicate = input.params[1].predicate.clone();
                input.params[0].predicate = predicate;
            }
            _ => input.mappings[0].parameter_mappings[0].implementation_property = "z value".into(),
        }
        let invalid = FnoCatalog::try_new(input)
            .expect("boundary must be reachable through actual native admission");
        let error = CallableBinding::admit(&invalid, 0, &mut Governor::new(Budget::default()))
            .expect_err("incomplete or ambiguous callable");
        match (case, error) {
            (0, BindingError::ParameterPredicate(parameter)) => {
                assert_eq!(parameter.iri, "urn:example:a");
            }
            (1, BindingError::OutputPredicate(output)) => {
                assert_eq!(output.iri, "urn:example:result");
            }
            (
                2,
                BindingError::ParameterMappingCount {
                    parameter,
                    count: 0,
                },
            ) => assert_eq!(parameter.iri, "urn:example:z"),
            (
                3,
                BindingError::ParameterMappingCount {
                    parameter,
                    count: 2,
                },
            ) => assert_eq!(parameter.iri, "urn:example:a"),
            (4, BindingError::ReturnMappingCount { count: 0, .. })
            | (5, BindingError::ReturnMappingCount { count: 2, .. }) => {}
            (6, BindingError::PredicateCollision { first, second }) => {
                assert_eq!(first.iri, "urn:example:z");
                assert_eq!(second.iri, "urn:example:a");
            }
            (7, BindingError::PropertyCollision { first, second }) => {
                assert_eq!(first.node.label, "z");
                assert_eq!(second.node.label, "a");
            }
            (_, unexpected) => panic!("case {case}: {unexpected:?}"),
        }
        // The unaltered neighboring catalog remains callable for each refusal.
        let valid = FnoCatalog::try_new(catalog::input()).expect("valid native catalog");
        CallableBinding::admit(&valid, 0, &mut Governor::new(Budget::default()))
            .expect("complete neighbor");
    }
}

#[test]
fn input_and_output_names_are_separate_scopes_without_identifier_restrictions() {
    let mut input = catalog::input();
    input.mappings[0].parameter_mappings[0]
        .implementation_property
        .clear();
    input.mappings[0].return_mappings[0]
        .implementation_property
        .clear();
    input.functions[0]
        .output
        .predicate
        .clone_from(&input.params[0].predicate);
    let catalog =
        FnoCatalog::try_new(input).expect("empty property names are valid native plain strings");
    let binding = CallableBinding::admit(&catalog, 0, &mut Governor::new(Budget::default()))
        .expect("input and output are separate scopes");
    assert_eq!(
        binding
            .parameter(1)
            .expect("second parameter")
            .mapping
            .implementation_property,
        ""
    );
    assert_eq!(binding.output().mapping.implementation_property, "");
}

#[test]
fn selection_work_storage_and_cancellation_fail_without_partial_success() {
    let catalog = FnoCatalog::try_new(catalog::input()).expect("valid catalog");
    assert!(matches!(
        CallableBinding::admit(&catalog, 1, &mut Governor::new(Budget::default())),
        Err(BindingError::MappingIndex {
            index: 1,
            available: 1
        })
    ));
    let mut governor = Governor::new(Budget::default());
    CallableBinding::admit(&catalog, 0, &mut governor).expect("measure complete admission");
    let usage = governor.usage();
    for (budget, resource) in [
        (
            Budget {
                work: usage.work - 1,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
    ] {
        assert!(
            matches!(CallableBinding::admit(&catalog, 0, &mut Governor::new(budget)), Err(BindingError::Resource(StopReason::BudgetExceeded(actual))) if actual == resource)
        );
    }
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        CallableBinding::admit(
            &catalog,
            0,
            &mut Governor::with_cancellation(Budget::default(), token)
        ),
        Err(BindingError::Resource(StopReason::Cancelled))
    ));
}
