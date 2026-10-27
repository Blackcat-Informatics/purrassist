// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Standalone native file declarations, source identity and atomic admission.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    AppArmorGlob, FileCanonicalization, FileExecution as E, FilePermissions, FileRule,
    FileRuleError as Error, FileTarget, GlobBindingProblem, GlobError, LiteralPathBinding,
};
fn governor() -> Governor {
    Governor::new(Budget::default())
}
fn rule(path: &AppArmorGlob) -> FileRule<'_> {
    FileRule {
        source: "caller:file",
        path,
        bindings: &[],
        permissions: FilePermissions::parse("r", &mut governor()).unwrap(),
        execution: None,
        target: None,
        audit: false,
        deny: false,
        owner: false,
    }
}

#[test]
fn complete_file_modes_qualifiers_bindings_and_denial_preserve_native_bytes() {
    let path = AppArmorGlob::parse("@{ROOT}/{a,b}/**", &["ROOT"], &mut governor()).unwrap();
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/caller files",
    }];
    let mut rule = rule(&path);
    rule.bindings = &bindings;
    rule.permissions = FilePermissions::parse("mlkrw", &mut governor()).unwrap();
    rule.audit = true;
    rule.owner = true;
    let output = rule.render(&mut governor()).unwrap();
    assert_eq!(
        output.text,
        "audit owner \"/srv/caller files/{a,b}/**\" rwklm,\n"
    );
    assert_eq!(output.source, rule.source);
    assert_eq!(output.path_source, path.source());
    assert_eq!(output.bindings[0].path, bindings[0].path);
    assert_eq!(output.requirement.abi, "3.0");
    assert_eq!(output.requirement.mediation, "file");
    rule.permissions = FilePermissions::parse("", &mut governor()).unwrap();
    rule.audit = false;
    rule.owner = false;
    for (mode, word) in [
        (E::Inherit, "ix"),
        (E::Profile, "px"),
        (E::Child, "cx"),
        (E::ProfileInherit, "Pix"),
        (E::ChildInherit, "Cix"),
    ] {
        assert_eq!(E::parse(word, &mut governor()).unwrap(), mode);
        rule.execution = Some(mode);
        for target in [
            None,
            Some(FileTarget::parse("target.child-1", &mut governor()).unwrap()),
        ] {
            if mode == E::Inherit && target.is_some() {
                continue;
            }
            rule.target = target;
            let output = rule.render(&mut governor()).unwrap();
            let suffix = if target.is_some() {
                " -> target.child-1"
            } else {
                ""
            };
            assert_eq!(
                output.text,
                format!("\"/srv/caller files/{{a,b}}/**\" {word}{suffix},\n")
            );
            assert_eq!(output.target, target);
            assert_eq!(output.canonicalization, FileCanonicalization::Unchanged);
        }
    }
    rule.target = None;
    rule.deny = true;
    rule.audit = true;
    rule.owner = true;
    rule.execution = Some(E::Inherit);
    rule.permissions = FilePermissions::parse("rw", &mut governor()).unwrap();
    let output = rule.render(&mut governor()).unwrap();
    assert_eq!(
        output.text,
        "audit deny owner \"/srv/caller files/{a,b}/**\" rwx,\n"
    );
    assert_eq!(output.requested_execution, Some(E::Inherit));
    assert_eq!(
        output.canonicalization,
        FileCanonicalization::DeniedInheritToExecute
    );
    rule.execution = None;
    rule.permissions = FilePermissions::parse("a", &mut governor()).unwrap();
    assert_eq!(
        rule.render(&mut governor()).unwrap().text,
        "audit deny owner \"/srv/caller files/{a,b}/**\" a,\n"
    );
}

#[test]
fn invalid_permissions_modes_targets_and_waiver_requests_refuse_with_valid_neighbors() {
    assert!(matches!(
        FilePermissions::parse("rr", &mut governor()),
        Err(Error::DuplicatePermission {
            source: "rr",
            offset: 1
        })
    ));
    assert!(matches!(
        FilePermissions::parse("rZ", &mut governor()),
        Err(Error::Permission {
            source: "rZ",
            offset: 1
        })
    ));
    assert!(matches!(
        FilePermissions::parse("wa", &mut governor()),
        Err(Error::WriteAppend { .. })
    ));
    for valid in ["", "raklm", "rwklm"] {
        assert!(FilePermissions::parse(valid, &mut governor()).is_ok());
    }
    for invalid in ["x", "pix", "cix", "Px", "ixpx", "", " ix"] {
        assert!(
            matches!(E::parse(invalid, &mut governor()), Err(Error::Execution { source }) if source == invalid)
        );
    }
    for invalid in ["", "1target", "a/b", "a b", "target,", "a\n", "a@{X}", "é"] {
        assert!(
            matches!(FileTarget::parse(invalid, &mut governor()), Err(Error::Target { source, .. }) if source == invalid)
        );
    }
    let path = AppArmorGlob::parse("/bin/tool", &[], &mut governor()).unwrap();
    let mut rule = rule(&path);
    rule.permissions = FilePermissions::parse("", &mut governor()).unwrap();
    assert!(matches!(
        rule.render(&mut governor()),
        Err(Error::Empty {
            source: "caller:file"
        })
    ));
    rule.target = Some(FileTarget::parse("target", &mut governor()).unwrap());
    assert!(matches!(
        rule.render(&mut governor()),
        Err(Error::TargetWithoutTransition { .. })
    ));
    rule.execution = Some(E::Inherit);
    assert!(matches!(
        rule.render(&mut governor()),
        Err(Error::TargetWithoutTransition { .. })
    ));
    rule.deny = true;
    assert!(matches!(
        rule.render(&mut governor()),
        Err(Error::TargetOnDeny { .. })
    ));
    rule.target = None;
    assert!(rule.render(&mut governor()).is_ok());
    for mode in [E::Profile, E::Child, E::ProfileInherit, E::ChildInherit] {
        rule.execution = Some(mode);
        assert!(
            matches!(rule.render(&mut governor()), Err(Error::DeniedTransition { execution, .. }) if execution == mode)
        );
    }
    for (word, mode) in [("ux", E::Unconfined), ("Ux", E::UnconfinedScrubbed)] {
        assert_eq!(E::parse(word, &mut governor()).unwrap(), mode);
        rule.execution = Some(mode);
        for deny in [false, true] {
            rule.deny = deny;
            assert!(
                matches!(rule.render(&mut governor()), Err(Error::NeedsAuthenticatedWaiver { execution, source: "caller:file" }) if execution == mode)
            );
        }
    }
}

#[test]
fn lexical_absolute_admission_and_binding_failures_preserve_original_source_offsets() {
    for source in ["relative", "{/a,/b}", "[/]a", r"\/a", "**/a"] {
        let path = AppArmorGlob::parse(source, &[], &mut governor()).unwrap();
        assert!(
            matches!(rule(&path).render(&mut governor()), Err(Error::AbsolutePrefix { path, source: "caller:file" }) if path == source)
        );
    }
    for source in ["/", "//namespace/**", "/{,a}", "/[a-z]/**"] {
        let path = AppArmorGlob::parse(source, &[], &mut governor()).unwrap();
        assert!(rule(&path).render(&mut governor()).is_ok());
    }
    let path = AppArmorGlob::parse("@{ROOT}/a", &["ROOT"], &mut governor()).unwrap();
    let mut rule = rule(&path);
    assert_eq!(
        rule.render(&mut governor()).err(),
        Some(Error::Glob {
            source: rule.source,
            reason: GlobError::MissingBinding { offset: 0 }
        })
    );
    let unsafe_bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/*",
    }];
    rule.bindings = &unsafe_bindings;
    assert_eq!(
        rule.render(&mut governor()).err(),
        Some(Error::Glob {
            source: rule.source,
            reason: GlobError::Binding {
                index: 0,
                offset: 5,
                problem: GlobBindingProblem::UnsafePath
            }
        })
    );
    let valid = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/root",
    }];
    rule.bindings = &valid;
    assert_eq!(
        rule.render(&mut governor()).unwrap().text,
        "\"/srv/root/a\" r,\n"
    );
    assert_eq!(path.source(), "@{ROOT}/a");
}

#[test]
fn rule_and_field_governors_refuse_atomically_at_exact_resource_boundaries() {
    let path_source = format!("@{{ROOT}}/{}/**", "p".repeat(128));
    let target_source = format!("target_{}", "t".repeat(256));
    let path = AppArmorGlob::parse(&path_source, &["ROOT"], &mut governor()).unwrap();
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/caller files",
    }];
    let mut rule = rule(&path);
    rule.bindings = &bindings;
    rule.execution = Some(E::Profile);
    rule.target = Some(FileTarget::parse(&target_source, &mut governor()).unwrap());
    let mut g = governor();
    let expected = rule.render(&mut g).unwrap().text;
    let usage = g.usage();
    let input = u64::try_from(
        target_source
            .len()
            .max(path_source.len())
            .max(bindings[0].path.len())
            .max(rule.source.len()),
    )
    .unwrap();
    let exact = Budget {
        work: usage.work,
        scratch_bytes: usage.peak_scratch_bytes,
        input_bytes: input,
        ..Budget::default()
    };
    assert_eq!(
        rule.render(&mut Governor::new(exact)).unwrap().text,
        expected
    );
    for (limits, resource) in [
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
        let reason = match rule.render(&mut g).err().unwrap() {
            Error::Resource { reason, .. }
            | Error::Glob {
                reason: GlobError::Resource(reason),
                ..
            } => reason,
            other => panic!("unexpected semantic refusal: {other:?}"),
        };
        assert_eq!(reason, StopReason::BudgetExceeded(resource));
        assert_eq!(g.check(), Err(reason));
    }
    let mut path_only = rule;
    path_only.target = None;
    let mut path_starved = Governor::new(Budget {
        input_bytes: u64::try_from(path_source.len()).unwrap() - 1,
        ..Budget::default()
    });
    assert!(matches!(
        path_only.render(&mut path_starved),
        Err(Error::Resource {
            reason: StopReason::BudgetExceeded(Resource::InputBytes),
            ..
        })
    ));
    let token = CancellationToken::new();
    token.cancel();
    let mut cancelled = Governor::with_cancellation(exact, token);
    assert!(matches!(
        rule.render(&mut cancelled),
        Err(Error::Resource {
            reason: StopReason::Cancelled,
            ..
        })
    ));
    assert_eq!(cancelled.usage().work, 0);
    for source in ["r", "px", "target"] {
        let mut g = Governor::new(Budget {
            input_bytes: 0,
            ..Budget::default()
        });
        let error = match source {
            "r" => FilePermissions::parse(source, &mut g).err().unwrap(),
            "px" => E::parse(source, &mut g).err().unwrap(),
            _ => FileTarget::parse(source, &mut g).err().unwrap(),
        };
        assert!(matches!(
            error,
            Error::Resource {
                reason: StopReason::BudgetExceeded(Resource::InputBytes),
                ..
            }
        ));
    }
}
