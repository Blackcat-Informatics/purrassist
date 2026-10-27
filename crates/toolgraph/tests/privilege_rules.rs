// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Named privilege policy, explicit denial and atomic governed output boundaries.

#[path = "../examples/support/privilege_cases.rs"]
pub mod cases;

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    CapabilityRule, MountRule, PrivilegeError as Error, dangerous_capability_denials,
};

fn governor() -> Governor {
    Governor::new(Budget::default())
}

#[test]
fn exact_named_capabilities_preserve_qualifiers_and_refuse_every_dangerous_grant() {
    for &(name, _, dangerous) in cases::CAPABILITIES {
        for audit in [false, true] {
            let mut rule = CapabilityRule {
                source: "caller:capability",
                name,
                audit,
                deny: true,
            };
            let denial = rule.render(&mut governor()).unwrap();
            assert_eq!(
                denial.text,
                format!(
                    "{}deny capability {name},\n",
                    if audit { "audit " } else { "" }
                )
            );
            assert_eq!(denial.source, rule.source);
            assert_eq!(denial.name, name);
            assert_eq!(denial.requirement.abi, "3.0");
            assert_eq!(denial.requirement.mediation, "caps");
            rule.deny = false;
            if dangerous {
                assert_eq!(
                    rule.render(&mut governor()).err(),
                    Some(Error::DangerousGrant {
                        source: rule.source,
                        name
                    })
                );
            } else {
                assert_eq!(
                    rule.render(&mut governor()).unwrap().text,
                    format!("{}capability {name},\n", if audit { "audit " } else { "" })
                );
            }
        }
    }
}

#[test]
fn complete_capability_baseline_and_all_mount_denials_keep_normative_bytes() {
    for audit in [false, true] {
        let baseline =
            dangerous_capability_denials("caller:baseline", audit, &mut governor()).unwrap();
        let expected = if audit {
            cases::BASELINE.replace("deny capability ", "audit deny capability ")
        } else {
            cases::BASELINE.into()
        };
        assert_eq!(baseline.text, expected);
        assert_eq!(baseline.source, "caller:baseline");
        assert_eq!(baseline.requirement.abi, "3.0");
        assert_eq!(baseline.requirement.mediation, "caps");
        for operation in ["mount", "remount", "umount", "pivot_root"] {
            let rule = MountRule {
                source: "caller:mount",
                operation,
                audit,
                deny: true,
            };
            let denial = rule.render(&mut governor()).unwrap();
            assert_eq!(
                denial.text,
                format!("{}deny {operation},\n", if audit { "audit " } else { "" })
            );
            assert_eq!(denial.source, rule.source);
            assert_eq!(denial.operation, operation);
            assert_eq!(denial.requirement.abi, "3.0");
            assert_eq!(denial.requirement.mediation, "mount");
            assert_eq!(
                MountRule {
                    deny: false,
                    ..rule
                }
                .render(&mut governor())
                .err(),
                Some(Error::MountAllow {
                    source: rule.source,
                    operation
                })
            );
        }
    }
}

#[test]
fn unsupported_target_and_unknown_fields_keep_original_values_without_native_coercion() {
    for name in [
        "",
        "CAP_NET_RAW",
        "NET_RAW",
        "net_raw ",
        " net_raw",
        "13",
        "net_raw,bpf",
        "net_raw bpf",
        "*",
        "all",
        "net-raw",
        "net_raw,\n}",
    ] {
        let rule = CapabilityRule {
            source: "caller:bad-capability",
            name,
            audit: false,
            deny: false,
        };
        assert_eq!(
            rule.render(&mut governor()).err(),
            Some(Error::UnknownCapability {
                source: rule.source,
                name
            })
        );
    }
    for deny in [false, true] {
        let rule = CapabilityRule {
            source: "caller:new-target",
            name: "checkpoint_restore",
            audit: true,
            deny,
        };
        assert_eq!(
            rule.render(&mut governor()).err(),
            Some(Error::UnsupportedCapability {
                source: rule.source,
                name: rule.name,
                abi: "3.0"
            })
        );
    }
    for operation in [
        "",
        "unmount",
        "pivot-root",
        "Mount",
        "mount ",
        "mount /tmp",
        "mount,umount",
        "mount options=ro",
    ] {
        let rule = MountRule {
            source: "caller:bad-mount",
            operation,
            audit: false,
            deny: true,
        };
        assert_eq!(
            rule.render(&mut governor()).err(),
            Some(Error::UnknownMountOperation {
                source: rule.source,
                operation
            })
        );
    }
    assert!(
        CapabilityRule {
            source: "caller:neighbor",
            name: "bpf",
            audit: false,
            deny: false
        }
        .render(&mut governor())
        .is_ok()
    );
    assert!(
        MountRule {
            source: "caller:neighbor",
            operation: "umount",
            audit: false,
            deny: true
        }
        .render(&mut governor())
        .is_ok()
    );
}

#[test]
fn current_governor_admits_inputs_work_storage_and_cancellation_before_atomic_publication() {
    for operation in 0..3 {
        let source = "caller:resource-boundary";
        let name = "net_raw";
        let mount = "pivot_root";
        let render = |g: &mut Governor| match operation {
            0 => CapabilityRule {
                source,
                name,
                audit: true,
                deny: false,
            }
            .render(g)
            .map(|fragment| fragment.text),
            1 => MountRule {
                source,
                operation: mount,
                audit: true,
                deny: true,
            }
            .render(g)
            .map(|fragment| fragment.text),
            _ => dangerous_capability_denials(source, true, g).map(|fragment| fragment.text),
        };
        let mut measured = governor();
        let expected = render(&mut measured).unwrap();
        let usage = measured.usage();
        let exact = Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            input_bytes: u64::try_from(source.len()).unwrap(),
            ..Budget::default()
        };
        assert_eq!(render(&mut Governor::new(exact)).unwrap(), expected);
        for (limits, dimension) in [
            (
                Budget {
                    work: exact.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: exact.scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    input_bytes: exact.input_bytes - 1,
                    ..exact
                },
                Resource::InputBytes,
            ),
        ] {
            let mut g = Governor::new(limits);
            let error = render(&mut g).err().unwrap();
            assert!(
                matches!(error, Error::Resource { source: original, reason: StopReason::BudgetExceeded(resource), .. }
                if original == source && resource == dimension)
            );
            assert_eq!(g.check(), Err(StopReason::BudgetExceeded(dimension)));
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut cancelled = Governor::with_cancellation(exact, token);
        assert!(
            matches!(render(&mut cancelled), Err(Error::Resource { source: original, reason: StopReason::Cancelled, .. }) if original == source)
        );
        assert_eq!(cancelled.usage().work, 0);
    }
    check_borrowed_field_admission();
}

fn check_borrowed_field_admission() {
    // A short source must not hide a longer borrowed field from this governor.
    for (field, value) in [("capability", "net_raw"), ("mountOperation", "pivot_root")] {
        let mut g = Governor::new(Budget {
            input_bytes: u64::try_from(value.len()).unwrap() - 1,
            ..Budget::default()
        });
        let error = if field == "capability" {
            CapabilityRule {
                source: "id",
                name: value,
                audit: false,
                deny: false,
            }
            .render(&mut g)
            .err()
            .unwrap()
        } else {
            MountRule {
                source: "id",
                operation: value,
                audit: false,
                deny: true,
            }
            .render(&mut g)
            .err()
            .unwrap()
        };
        assert_eq!(
            error,
            Error::Resource {
                source: "id",
                field,
                value,
                reason: StopReason::BudgetExceeded(Resource::InputBytes)
            }
        );
    }
}
