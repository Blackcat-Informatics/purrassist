// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Combined document behavior over the existing typed rule-family authorities.
use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    AppArmorGlob, CapabilityRule, FileExecution, FilePermissions, FileRule, FileRuleError,
    InternetAccessSet, InternetEndpoint, InternetFamily, InternetLimits, InternetPorts,
    InternetRule, InternetScope, InternetSelector, LiteralPathBinding, MountRule, PolicyBaseline,
    PolicyBody, PolicyCause, PolicyDocument, PolicyDuplicateScope, PolicyInput, PolicyOrigin,
    PolicyProfile, PolicyRule, PolicyTarget, PtraceAccessSet, PtraceRule, ResourceLimits,
    SignalAccessSet, SignalRule, SignalSet, UnixAccessSet, UnixAddress, UnixLabel, UnixPattern,
    UnixPeer, UnixRule, UnixSocketType,
};

#[test]
fn complete_document_is_canonical_and_maps_all_rules_and_defaults() {
    let mut g = Governor::new(Budget::default());
    let path = AppArmorGlob::parse("@{ROOT}/**", &["ROOT"], &mut g).unwrap();
    let root = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/policy workspace",
    }];
    let local = AppArmorGlob::parse("@@{TMP}/ipc", &["TMP"], &mut g).unwrap();
    let peer = AppArmorGlob::parse("@@{TMP}/host", &["TMP"], &mut g).unwrap();
    let tmp = [LiteralPathBinding {
        name: "TMP",
        path: "/run/policy-example",
    }];
    let label = AppArmorGlob::parse("purrassist.tools", &[], &mut g).unwrap();
    let any = InternetEndpoint {
        scope: InternetScope::parse("*", &mut g).unwrap(),
        ports: InternetPorts::Any,
    };
    let rules = [
        PolicyRule::File(FileRule {
            source: "01:file",
            path: &path,
            bindings: &root,
            permissions: FilePermissions::parse("rwkl", &mut g).unwrap(),
            execution: None,
            target: None,
            audit: false,
            deny: false,
            owner: true,
        }),
        PolicyRule::Internet {
            rule: InternetRule {
                source: "02:network",
                family: InternetFamily::Inet,
                selector: InternetSelector::Stream,
                accesses: InternetAccessSet::parse("connect", &mut g).unwrap(),
                audit: false,
                deny: false,
                local: any,
                peer: InternetEndpoint {
                    scope: InternetScope::parse("10.0.0.7", &mut g).unwrap(),
                    ports: InternetPorts::Range {
                        first: 5432,
                        last: 5432,
                    },
                },
            },
            limits: InternetLimits {
                addresses_per_endpoint: 1,
                rules: 1,
            },
        },
        PolicyRule::Unix(UnixRule {
            source: "03:unix",
            accesses: UnixAccessSet::parse("send,receive", &mut g).unwrap(),
            socket_type: UnixSocketType::Stream,
            audit: false,
            deny: false,
            local: UnixAddress::Abstract(UnixPattern {
                glob: &local,
                bindings: &tmp,
            }),
            peer: UnixPeer {
                address: UnixAddress::Abstract(UnixPattern {
                    glob: &peer,
                    bindings: &tmp,
                }),
                label: UnixLabel::Pattern(UnixPattern {
                    glob: &label,
                    bindings: &[],
                }),
            },
        }),
        PolicyRule::Signal(SignalRule {
            source: "04:signal",
            peer: &label,
            bindings: &[],
            audit: false,
            deny: false,
            access: SignalAccessSet::parse("receive", &mut g).unwrap(),
            signals: SignalSet::parse("term,int", &mut g).unwrap(),
        }),
        PolicyRule::Ptrace(PtraceRule {
            source: "05:ptrace",
            peer: &label,
            bindings: &[],
            access: PtraceAccessSet::parse("read", &mut g).unwrap(),
            audit: true,
            deny: true,
        }),
        PolicyRule::Capability(CapabilityRule {
            source: "06:capability",
            name: "net_raw",
            audit: false,
            deny: false,
        }),
        PolicyRule::Mount(MountRule {
            source: "07:mount",
            operation: "mount",
            audit: true,
            deny: true,
        }),
        PolicyRule::Limits {
            source: "08:limits",
            limits: ResourceLimits {
                address_space_bytes: 268_435_456,
                file_size_bytes: 1_048_576,
                processes: 16,
                cpu_seconds: 5,
                open_files: 64,
            },
        },
    ];
    let reversed: Vec<_> = rules.iter().rev().copied().collect();
    for caller_rules in [&rules[..], &reversed[..]] {
        let hats = [
            PolicyBody {
                source: "empty-hat",
                name: "zeta",
                rules: &[],
                imports: &[],
            },
            PolicyBody {
                source: "worker-hat",
                name: "worker",
                rules: caller_rules,
                imports: &[],
            },
        ];
        let reverse_hats = [hats[1], hats[0]];
        for hats in [&hats[..], &reverse_hats[..]] {
            let profiles = [
                PolicyProfile::Standalone(PolicyBody {
                    source: "standalone",
                    name: "purrassist.standalone",
                    rules: caller_rules,
                    imports: &[],
                }),
                PolicyProfile::Parent {
                    body: PolicyBody {
                        source: "parent",
                        name: "purrassist.tools",
                        rules: &[],
                        imports: &[],
                    },
                    hats,
                },
            ];
            let reverse_profiles = [profiles[1], profiles[0]];
            for profiles in [&profiles[..], &reverse_profiles[..]] {
                let input = PolicyInput {
                    source: "caller:golden",
                    target: PolicyTarget::Abi4Ip,
                    home: "/home/example",
                    profiles,
                    dependencies: &[],
                };
                let document = PolicyDocument::prepare(input, &mut g).unwrap();
                let output = document.compile(&mut g).unwrap();
                assert_eq!(
                    output.text.as_str(),
                    include_str!("data/policy_document.apparmor")
                );
                assert_eq!(
                    output
                        .requirements
                        .complete_mediation()
                        .unwrap()
                        .collect::<Vec<_>>(),
                    [
                        "file",
                        "caps",
                        "network_v8/af_inet",
                        "network/af_unix",
                        "signal",
                        "ptrace",
                        "mount",
                        "rlimit"
                    ]
                );
                assert_eq!(output.spans.len(), 48);
                assert_eq!(
                    output
                        .spans
                        .iter()
                        .filter(|span| matches!(span.origin, PolicyOrigin::Caller(_)))
                        .count(),
                    16
                );
                for span in &output.spans {
                    assert!(span.start < span.end && span.end <= output.text.len());
                    let text = &output.text[span.start..span.end];
                    assert!(text.ends_with(",\n"));
                    if span.origin == PolicyOrigin::Baseline(PolicyBaseline::Ssh) {
                        assert!(text.contains("audit deny \"/home/example/.ssh/**\" rwx"));
                    }
                }
            }
        }
    }
}

#[test]
fn baseline_home_neighbors_are_literal_and_baseline_only_documents_are_complete() {
    for (home, suffix) in [
        ("/", "/.ssh/**"),
        ("/home/example/", "/home/example/.ssh/**"),
        ("/home/work space", "/home/work space/.ssh/**"),
    ] {
        let profiles = [PolicyProfile::Standalone(PolicyBody {
            source: "empty",
            name: "baseline_only",
            rules: &[],
            imports: &[],
        })];
        let mut g = Governor::new(Budget::default());
        let input = PolicyInput {
            source: "baseline",
            target: PolicyTarget::Abi3,
            home,
            profiles: &profiles,
            dependencies: &[],
        };
        let document = PolicyDocument::prepare(input, &mut g).unwrap();
        let output = document.compile(&mut g).unwrap();
        assert!(output.text.contains(suffix));
        assert!(!output.text.contains("@{HOME}"));
        assert_eq!(output.spans.len(), 8);
        assert!(
            output
                .spans
                .iter()
                .all(|span| matches!(span.origin, PolicyOrigin::Baseline(_)))
        );
        assert!(
            output
                .text
                .contains("deny ptrace (trace, read, readby, tracedby) peer=\"**\"")
        );
        if home == "/" {
            assert!(!output.text.contains("//.ssh"));
        }
    }
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        source: "unsafe-home",
        name: "baseline_only",
        rules: &[],
        imports: &[],
    })];
    for home in ["/home/{*}", "/home/@{literal}"] {
        let mut g = Governor::new(Budget::default());
        let input = PolicyInput {
            source: "baseline",
            target: PolicyTarget::Abi3,
            home,
            profiles: &profiles,
            dependencies: &[],
        };
        let refusal = PolicyDocument::prepare(input, &mut g).unwrap_err();
        assert_eq!(refusal.input.home, home);
        assert!(matches!(
            refusal.cause,
            PolicyCause::Glob(purrassist_toolgraph::GlobError::Binding {
                problem: purrassist_toolgraph::GlobBindingProblem::UnsafePath,
                ..
            })
        ));
    }
}

#[test]
fn document_structural_and_native_refusals_keep_original_inputs() {
    let empty = PolicyInput {
        source: "none",
        target: PolicyTarget::Abi3,
        home: "/home/example",
        profiles: &[],
        dependencies: &[],
    };
    let mut g = Governor::new(Budget::default());
    let document = PolicyDocument::prepare(empty, &mut g).unwrap();
    let error = document.compile(&mut g).unwrap_err();
    assert!(matches!(error.cause, PolicyCause::Empty));
    assert_eq!(error.document.input().source, "none");
    assert!(matches!(
        PolicyDocument::prepare(
            PolicyInput {
                home: "relative",
                ..empty
            },
            &mut g
        )
        .unwrap_err()
        .cause,
        PolicyCause::Home { home: "relative" }
    ));
    assert!(matches!(
        PolicyDocument::prepare(
            PolicyInput {
                home: "/bad\0path",
                ..empty
            },
            &mut g
        )
        .unwrap_err()
        .cause,
        PolicyCause::Glob(_)
    ));
    let base = PolicyBody {
        source: "body",
        name: "root",
        rules: &[],
        imports: &[],
    };
    let duplicates = [PolicyProfile::Standalone(base); 2];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &duplicates,
            ..empty
        },
        &mut g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(&mut g).unwrap_err().cause,
        PolicyCause::Duplicate {
            name: "root",
            scope: PolicyDuplicateScope::Profiles
        }
    ));
    let hats = [base, base];
    let parents = [PolicyProfile::Parent {
        body: base,
        hats: &hats,
    }];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &parents,
            ..empty
        },
        &mut g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(&mut g).unwrap_err().cause,
        PolicyCause::Duplicate {
            scope: PolicyDuplicateScope::Hats,
            ..
        }
    ));
    let invalid = [PolicyProfile::Standalone(PolicyBody {
        name: "bad}profile",
        ..base
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &invalid,
            ..empty
        },
        &mut g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(&mut g).unwrap_err().cause,
        PolicyCause::File(FileRuleError::Target {
            source: "bad}profile",
            ..
        })
    ));
    native_rule_refusals(empty, base, &mut g);
}

fn native_rule_refusals(empty: PolicyInput<'_>, base: PolicyBody<'_>, g: &mut Governor) {
    let path = AppArmorGlob::parse("/bin/worker", &[], g).unwrap();
    let mut file = FileRule {
        source: "caller:file",
        path: &path,
        bindings: &[],
        permissions: FilePermissions::parse("r", g).unwrap(),
        execution: Some(FileExecution::Unconfined),
        target: None,
        audit: false,
        deny: false,
        owner: false,
    };
    let rules = [PolicyRule::File(file)];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        rules: &rules,
        ..base
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &profiles,
            ..empty
        },
        g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(g).unwrap_err().cause,
        PolicyCause::File(FileRuleError::NeedsAuthenticatedWaiver {
            source: "caller:file",
            ..
        })
    ));
    file.execution = None;
    let rules = [PolicyRule::File(file), PolicyRule::File(file)];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        rules: &rules,
        ..base
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &profiles,
            ..empty
        },
        g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(g).unwrap_err().cause,
        PolicyCause::Duplicate {
            scope: PolicyDuplicateScope::Rules,
            ..
        }
    ));
    let unbound = AppArmorGlob::parse("@{ROOT}/**", &["ROOT"], g).unwrap();
    file.path = &unbound;
    let rules = [PolicyRule::File(file)];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        rules: &rules,
        ..base
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &profiles,
            ..empty
        },
        g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(g).unwrap_err().cause,
        PolicyCause::File(FileRuleError::Glob {
            source: "caller:file",
            reason: purrassist_toolgraph::GlobError::MissingBinding { .. }
        })
    ));
    file.path = &path;
    file.source = "first";
    let first = file;
    file.source = "second";
    let rules = [PolicyRule::File(file), PolicyRule::File(first)];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        rules: &rules,
        ..base
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &profiles,
            ..empty
        },
        g,
    )
    .unwrap();
    let output = document.compile(g).unwrap();
    assert_eq!(
        output
            .spans
            .iter()
            .filter(|span| matches!(span.origin, PolicyOrigin::Caller(_)))
            .count(),
        2
    );
    let any = InternetEndpoint {
        scope: InternetScope::parse("*", g).unwrap(),
        ports: InternetPorts::Any,
    };
    let rules = [PolicyRule::Internet {
        rule: InternetRule {
            source: "caller:inet",
            family: InternetFamily::Inet,
            selector: InternetSelector::Stream,
            accesses: InternetAccessSet::parse("connect", g).unwrap(),
            audit: false,
            deny: false,
            local: any,
            peer: any,
        },
        limits: InternetLimits {
            addresses_per_endpoint: 1,
            rules: 1,
        },
    }];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        rules: &rules,
        ..base
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            profiles: &profiles,
            ..empty
        },
        g,
    )
    .unwrap();
    assert!(matches!(
        document.compile(g).unwrap_err().cause,
        PolicyCause::Target {
            source: "caller:inet",
            target: PolicyTarget::Abi3,
            ..
        }
    ));
}

#[test]
fn document_resource_refusals_and_cancellation_never_publish_partial_text() {
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        source: "caller:body",
        name: "budget",
        rules: &[],
        imports: &[],
    })];
    let mut preparation = Governor::new(Budget::default());
    let document = PolicyDocument::prepare(
        PolicyInput {
            source: "caller:budget",
            target: PolicyTarget::Abi3,
            home: "/home/example",
            profiles: &profiles,
            dependencies: &[],
        },
        &mut preparation,
    )
    .unwrap();
    let mut measured = Governor::new(Budget::default());
    let output = document.compile(&mut measured).unwrap();
    let usage = measured.usage();
    drop(output);
    for resource in [
        Resource::Work,
        Resource::ScratchBytes,
        Resource::InputBytes,
        Resource::Depth,
    ] {
        let budget = match resource {
            Resource::Work => Budget {
                work: 0,
                ..Budget::default()
            },
            Resource::ScratchBytes => Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
            Resource::InputBytes => Budget {
                input_bytes: 0,
                ..Budget::default()
            },
            _ => Budget {
                depth: 0,
                ..Budget::default()
            },
        };
        let mut g = Governor::new(budget);
        assert!(document.compile(&mut g).is_err());
        assert_eq!(g.check(), Err(StopReason::BudgetExceeded(resource)));
    }
    for work in [usage.work / 2, usage.work - 1] {
        let mut g = Governor::new(Budget {
            work,
            ..Budget::default()
        });
        assert!(document.compile(&mut g).is_err());
        assert_eq!(g.check(), Err(StopReason::BudgetExceeded(Resource::Work)));
    }
    let mut g = Governor::new(Budget {
        scratch_bytes: usage.peak_scratch_bytes - 1,
        ..Budget::default()
    });
    assert!(document.compile(&mut g).is_err());
    assert_eq!(
        g.check(),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    let token = CancellationToken::new();
    token.cancel();
    let mut g = Governor::with_cancellation(Budget::default(), token);
    assert!(document.compile(&mut g).is_err());
    assert_eq!(g.check(), Err(StopReason::Cancelled));
}
