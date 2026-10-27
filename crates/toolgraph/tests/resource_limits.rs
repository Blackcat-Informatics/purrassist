// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact native representations, complete provenance and atomic governed refusal.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    ResourceLimitError, ResourceLimitField, ResourceLimitProblem, ResourceLimits,
};

fn governor() -> Governor {
    Governor::new(Budget::default())
}
const fn limits(values: [u64; 5]) -> ResourceLimits {
    ResourceLimits {
        address_space_bytes: values[0],
        file_size_bytes: values[1],
        processes: values[2],
        cpu_seconds: values[3],
        open_files: values[4],
    }
}

#[test]
fn complete_exact_declarations_preserve_original_limits_and_source() {
    let source = "caller:limits\nidentity retained as data";
    for (values, text) in [
        (
            [1; 5],
            "set rlimit as <= 1,\nset rlimit fsize <= 1,\nset rlimit nproc <= 1,\nset rlimit cpu <= 1 seconds,\nset rlimit nofile <= 1,\n",
        ),
        (
            [1025, 9_223_372_036_854_775_807, 3, 5, 7],
            "set rlimit as <= 1025,\nset rlimit fsize <= 9223372036854775807,\nset rlimit nproc <= 3,\nset rlimit cpu <= 5 seconds,\nset rlimit nofile <= 7,\n",
        ),
        (
            [
                9_223_372_036_854_775_808,
                18_446_744_073_709_550_592,
                9_223_372_036_854_775_807,
                9_223_372_036_854,
                9_223_372_036_854_775_807,
            ],
            "set rlimit as <= 9007199254740992K,\nset rlimit fsize <= 18014398509481983K,\nset rlimit nproc <= 9223372036854775807,\nset rlimit cpu <= 9223372036854 seconds,\nset rlimit nofile <= 9223372036854775807,\n",
        ),
    ] {
        let original = limits(values);
        let output = original.render(source, &mut governor()).unwrap();
        assert_eq!(output.text, text);
        assert_eq!(output.source, source);
        assert_eq!(output.limits, original);
        assert_eq!(output.requirement.abi, "3.0");
        assert_eq!(output.requirement.mediation, "rlimit");
    }
}

#[test]
fn zero_sentinel_and_unrepresentable_neighbors_refuse_before_output() {
    let fields = [
        ResourceLimitField::AddressSpaceBytes,
        ResourceLimitField::FileSizeBytes,
        ResourceLimitField::Processes,
        ResourceLimitField::CpuSeconds,
        ResourceLimitField::OpenFiles,
    ];
    for (index, field) in fields.into_iter().enumerate() {
        let unsupported: &[u64] = match field {
            ResourceLimitField::AddressSpaceBytes | ResourceLimitField::FileSizeBytes => &[
                9_223_372_036_854_775_809,
                18_446_744_073_709_550_591,
                18_446_744_073_709_550_593,
                u64::MAX,
            ],
            ResourceLimitField::Processes | ResourceLimitField::OpenFiles => {
                &[9_223_372_036_854_775_808, u64::MAX]
            }
            ResourceLimitField::CpuSeconds => &[9_223_372_036_855, 18_446_744_073_711, u64::MAX],
        };
        for value in core::iter::once(0).chain(unsupported.iter().copied()) {
            let mut values = [1; 5];
            values[index] = value;
            let original = limits(values);
            let mut g = governor();
            assert_eq!(
                original.render("caller:invalid", &mut g).unwrap_err(),
                ResourceLimitError::Value {
                    source: "caller:invalid",
                    limits: original,
                    field,
                    problem: if value == 0 {
                        ResourceLimitProblem::Zero
                    } else {
                        ResourceLimitProblem::UnsupportedRepresentation
                    }
                }
            );
            assert_eq!(
                g.usage().peak_scratch_bytes,
                0,
                "all validation precedes formatting"
            );
        }
    }
}

#[test]
fn exact_resource_boundaries_and_cancellation_publish_only_complete_fragments() {
    let source = "caller:current-governor";
    let original = limits([18_446_744_073_709_550_592, 1025, 3, 5, 7]);
    let mut g = governor();
    let expected = original.render(source, &mut g).unwrap();
    let usage = g.usage();
    let exact = Budget {
        input_bytes: u64::try_from(source.len()).unwrap(),
        work: usage.work,
        scratch_bytes: usage.peak_scratch_bytes,
        ..Budget::default()
    };
    assert_eq!(
        original.render(source, &mut Governor::new(exact)).unwrap(),
        expected
    );
    for (budget, reason) in [
        (
            Budget {
                input_bytes: exact.input_bytes - 1,
                ..exact
            },
            StopReason::BudgetExceeded(Resource::InputBytes),
        ),
        (
            Budget {
                work: exact.work - 1,
                ..exact
            },
            StopReason::BudgetExceeded(Resource::Work),
        ),
        (
            Budget {
                scratch_bytes: exact.scratch_bytes - 1,
                ..exact
            },
            StopReason::BudgetExceeded(Resource::ScratchBytes),
        ),
    ] {
        let mut g = Governor::new(budget);
        assert_eq!(
            original.render(source, &mut g).unwrap_err(),
            ResourceLimitError::Resource {
                source,
                limits: original,
                reason: reason.clone()
            }
        );
        assert_eq!(g.check(), Err(reason));
        assert_eq!(
            g.scratch_meter().live(),
            0,
            "partial private output is discarded"
        );
    }
    let token = CancellationToken::new();
    token.cancel();
    let mut g = Governor::with_cancellation(exact, token);
    assert_eq!(
        original.render(source, &mut g).unwrap_err(),
        ResourceLimitError::Resource {
            source,
            limits: original,
            reason: StopReason::Cancelled
        }
    );
    assert_eq!(g.usage().work, 0);
    assert_eq!(g.scratch_meter().live(), 0);
}
