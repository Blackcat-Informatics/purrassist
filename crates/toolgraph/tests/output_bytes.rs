// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact result byte bounds never inherit prompt token-estimation semantics.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    OutputByteLimit, OutputEnforcement, OutputLimitError, OutputMeasurement, OutputUnit,
};

#[test]
fn result_release_checks_bytes_even_when_token_count_fits() {
    let serialized = "\"é\"";
    // This deterministic scalar-token counter has three tokens. The result
    // contract is still four UTF-8 bytes, independent of that token count.
    let supplied_token_count = serialized.chars().count();
    let bound = OutputByteLimit::new(3).unwrap();
    assert_eq!(supplied_token_count, 3);
    let expected = OutputMeasurement {
        limit: 3,
        measured: 4,
        unit: OutputUnit::Bytes,
        enforcement: OutputEnforcement::ExactBytes,
    };
    assert_eq!(
        bound.check(serialized.as_bytes(), &mut Governor::new(Budget::default())),
        Err(OutputLimitError::Exceeded(expected))
    );
    let exact = OutputByteLimit::new(4).unwrap();
    assert_eq!(
        exact
            .check(serialized.as_bytes(), &mut Governor::new(Budget::default()))
            .unwrap(),
        OutputMeasurement {
            limit: 4,
            ..expected
        }
    );
    assert!(matches!(
        exact.check(b"false", &mut Governor::new(Budget::default())),
        Err(OutputLimitError::Exceeded(OutputMeasurement {
            limit: 4,
            measured: 5,
            ..
        }))
    ));
}

#[test]
fn result_bound_admission_and_cancellation_are_explicit_failures() {
    assert_eq!(OutputByteLimit::new(0), Err(OutputLimitError::ZeroLimit));
    let bound = OutputByteLimit::new(4).unwrap();
    for (budget, resource) in [
        (
            Budget {
                input_bytes: 3,
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
        (
            Budget {
                work: 0,
                ..Budget::default()
            },
            Resource::Work,
        ),
    ] {
        assert_eq!(
            bound.check(b"null", &mut Governor::new(budget)),
            Err(OutputLimitError::Resource(StopReason::BudgetExceeded(
                resource
            )))
        );
    }
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert_eq!(
        bound.check(
            b"null",
            &mut Governor::with_cancellation(Budget::default(), cancellation)
        ),
        Err(OutputLimitError::Resource(StopReason::Cancelled))
    );
}
