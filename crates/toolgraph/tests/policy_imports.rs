// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Bound imports preserve source provenance while refusing complete feature claims.
use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason, contract::Resource, digest::Digest,
};
use purrassist_toolgraph::{
    AppArmorGlob, FilePermissions, FileRule, FileRuleError, PolicyBody, PolicyCause,
    PolicyDependency, PolicyDocument, PolicyImport, PolicyInput, PolicyOrigin, PolicyProfile,
    PolicyRule, PolicyTarget,
};

const FILES: [PolicyDependency<'static>; 3] = [
    PolicyDependency {
        source: "file:parent",
        path: "abstractions/purrassist-imports/parent",
        bytes: include_bytes!("../examples/data/policy_import_parent.apparmor"),
    },
    PolicyDependency {
        source: "file:child",
        path: "abstractions/purrassist-imports/child",
        bytes: include_bytes!("../examples/data/policy_import_child.apparmor"),
    },
    PolicyDependency {
        source: "file:opaque-transitive",
        path: "abstractions/opaque",
        bytes: b"not parsed as policy",
    },
];
const fn input<'a>(
    profiles: &'a [PolicyProfile<'a>],
    dependencies: &'a [PolicyDependency<'a>],
) -> PolicyInput<'a> {
    PolicyInput {
        source: "caller:imports",
        target: PolicyTarget::Abi4Ip,
        home: "/home/example",
        profiles,
        dependencies,
    }
}

#[test]
fn import_occurrences_bind_exact_bytes_preserve_order_and_scope_and_refuse_complete_requirements() {
    let imports = [
        PolicyImport {
            source: "z:first",
            path: FILES[1].path,
        },
        PolicyImport {
            source: "a:second",
            path: FILES[0].path,
        },
        PolicyImport {
            source: "repeat",
            path: FILES[1].path,
        },
    ];
    let mut g = Governor::new(Budget::default());
    let path = AppArmorGlob::parse("/srv/purrassist-imports/local", &[], &mut g).unwrap();
    let rules = [PolicyRule::File(FileRule {
        source: "local:rule",
        path: &path,
        bindings: &[],
        permissions: FilePermissions::parse("r", &mut g).unwrap(),
        execution: None,
        target: None,
        audit: false,
        deny: false,
        owner: false,
    })];
    let hats = [PolicyBody {
        source: "hat",
        name: "hat",
        rules: &[],
        imports: &imports[..1],
    }];
    let profiles = [PolicyProfile::Parent {
        body: PolicyBody {
            source: "body",
            name: "parent",
            rules: &rules,
            imports: &imports,
        },
        hats: &hats,
    }];
    let document = PolicyDocument::prepare(input(&profiles, &FILES), &mut g).unwrap();
    let output = document.compile(&mut g).unwrap();
    assert_eq!(output.dependencies.len(), FILES.len());
    for (actual, original) in output.dependencies.iter().zip(FILES) {
        assert_eq!(actual.dependency.source, original.source);
        assert_eq!(actual.digest, Digest::of(original.bytes));
    }
    let occurrences: Vec<_> = output
        .spans
        .iter()
        .filter_map(|span| match span.origin {
            PolicyOrigin::Import { source, dependency } => {
                assert_eq!(
                    output.text[span.start..span.end].trim(),
                    format!(
                        "include <{}>",
                        output.dependencies[dependency].dependency.path
                    )
                );
                Some((source, span.profile, span.hat, dependency))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        occurrences,
        [
            ("z:first", "parent", None, 1),
            ("a:second", "parent", None, 0),
            ("repeat", "parent", None, 1),
            ("z:first", "parent", Some("hat"), 1)
        ]
    );
    assert!(output.text.contains("/srv/purrassist-imports/local"));
    assert!(!output.text.contains("/srv/purrassist-imports/parent r,"));
    assert!(!output.text.contains("not parsed as policy"));
    assert!(
        output
            .requirements
            .local_mediation()
            .any(|class| class == "file")
    );
    let unknown = output.requirements.complete_mediation().err().unwrap();
    assert!(matches!(
        output.spans[unknown.first_import_span].origin,
        PolicyOrigin::Import {
            source: "z:first",
            ..
        }
    ));
}

#[test]
fn unsafe_paths_and_missing_or_ambiguous_direct_bindings_refuse_original_requests() {
    let invalid = [
        "/abstractions/a",
        "abstractions/",
        "abstractions/../x",
        "abstractions/./x",
        "abstractions//x",
        "abstractions/*",
        "abstractions/@{X}",
        "abstractions/x>",
        "abstractions/x\n}",
        "abstractions/a\\b",
        "abstractions/a b",
        "abstractions/a\0",
    ];
    for path in invalid {
        let imports = [PolicyImport {
            source: "original:bad-path",
            path,
        }];
        let profiles = [PolicyProfile::Standalone(PolicyBody {
            source: "body",
            name: "root",
            rules: &[],
            imports: &imports,
        })];
        let mut g = Governor::new(Budget::default());
        let document = PolicyDocument::prepare(input(&profiles, &FILES), &mut g).unwrap();
        assert!(matches!(document.compile(&mut g).unwrap_err().cause,
            PolicyCause::ImportPath { source: "original:bad-path", path: original, .. } if original == path));
    }
    let imports = [PolicyImport {
        source: "original:include",
        path: "abstractions/a-b_1.+/leaf",
    }];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        source: "body",
        name: "root",
        rules: &[],
        imports: &imports,
    })];
    let one = [PolicyDependency {
        source: "file",
        path: imports[0].path,
        bytes: b"opaque",
    }];
    let mut g = Governor::new(Budget::default());
    let missing = PolicyDocument::prepare(input(&profiles, &[]), &mut g).unwrap();
    assert!(matches!(
        missing.compile(&mut g).unwrap_err().cause,
        PolicyCause::MissingDependency {
            import: PolicyImport {
                source: "original:include",
                ..
            }
        }
    ));
    let repeated = [
        one[0],
        PolicyDependency {
            source: "another:file",
            ..one[0]
        },
    ];
    let ambiguous = PolicyDocument::prepare(input(&profiles, &repeated), &mut g).unwrap();
    assert!(matches!(
        ambiguous.compile(&mut g).unwrap_err().cause,
        PolicyCause::DependencyConflict { first, second }
            if first.source == "file" && second.source == "another:file"
    ));
    let no_imports = [PolicyProfile::Standalone(PolicyBody {
        source: "body",
        name: "root",
        rules: &[],
        imports: &[],
    })];
    let unreferenced = PolicyDocument::prepare(input(&no_imports, &repeated), &mut g).unwrap();
    assert!(matches!(unreferenced.compile(&mut g).unwrap_err().cause,
        PolicyCause::DependencyConflict { first, second } if first.bytes == second.bytes));
    let valid = PolicyDocument::prepare(input(&profiles, &one), &mut g).unwrap();
    assert!(valid.compile(&mut g).is_ok());
}

#[test]
fn dependency_manifest_and_partial_output_release_on_late_refusal_resource_stop_and_drop() {
    let bytes = [b'x'; 8192];
    let dependencies = [
        FILES[0],
        PolicyDependency {
            source: "opaque:large",
            path: "abstractions/large",
            bytes: &bytes,
        },
    ];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        source: "body",
        name: "bad name",
        rules: &[],
        imports: &[],
    })];
    let mut preparation = Governor::new(Budget::default());
    let document =
        PolicyDocument::prepare(input(&profiles, &dependencies), &mut preparation).unwrap();
    let mut measured = Governor::new(Budget::default());
    assert!(matches!(
        document.compile(&mut measured).unwrap_err().cause,
        PolicyCause::File(FileRuleError::Target {
            source: "bad name",
            offset: 3
        })
    ));
    let usage = measured.usage();
    assert_eq!(measured.scratch_meter().live(), 0);
    for (budget, resource) in [
        (
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
        (
            Budget {
                input_bytes: u64::try_from(bytes.len()).unwrap() - 1,
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
    ] {
        let mut g = Governor::new(budget);
        assert!(matches!(document.compile(&mut g).unwrap_err().cause,
            PolicyCause::Resource(StopReason::BudgetExceeded(actual)) if actual == resource));
        assert_eq!(g.check(), Err(StopReason::BudgetExceeded(resource)));
        assert_eq!(g.scratch_meter().live(), 0);
    }
    let token = CancellationToken::new();
    token.cancel();
    let mut cancelled = Governor::with_cancellation(Budget::default(), token);
    assert!(matches!(
        document.compile(&mut cancelled).unwrap_err().cause,
        PolicyCause::Resource(StopReason::Cancelled)
    ));
    assert_eq!(cancelled.scratch_meter().live(), 0);
    // Separate successful artifact owners from the documented detached renderer charges.
    let valid = [PolicyProfile::Standalone(PolicyBody {
        source: "body",
        name: "valid",
        rules: &[],
        imports: &[],
    })];
    let document = PolicyDocument::prepare(input(&valid, &dependencies), &mut preparation).unwrap();
    let mut g = Governor::new(Budget::default());
    let output = document.compile(&mut g).unwrap();
    let completed_work = g.usage().work;
    drop(output.text);
    drop(output.spans);
    let live = g.scratch_meter().live();
    drop(output.dependencies);
    assert!(g.scratch_meter().live() < live);
    assert_late_compile_refusal(&document, completed_work, g.scratch_meter().live());
}

fn assert_late_compile_refusal(document: &PolicyDocument<'_>, completed_work: u64, legacy: u64) {
    let mut g = Governor::new(Budget {
        work: completed_work - 1,
        ..Budget::default()
    });
    let result = document.compile(&mut g);
    assert!(matches!(
        result.unwrap_err().cause,
        PolicyCause::Resource(StopReason::BudgetExceeded(Resource::Work))
    ));
    assert_eq!(g.check(), Err(StopReason::BudgetExceeded(Resource::Work)));
    assert_eq!(g.scratch_meter().live(), legacy);
}
