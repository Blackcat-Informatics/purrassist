// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Compile actual typed profile/hat bodies into a complete offline policy file.
use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::{
    AppArmorGlob, CapabilityRule, FilePermissions, FileRule, InternetAccessSet, InternetEndpoint,
    InternetFamily, InternetLimits, InternetPorts, InternetRule, InternetScope, InternetSelector,
    LiteralPathBinding, MountRule, PolicyBody, PolicyDependency, PolicyDocument, PolicyImport,
    PolicyInput, PolicyProfile, PolicyRule, PolicyTarget, PtraceAccessSet, PtraceRule,
    ResourceLimits, SignalAccessSet, SignalRule, SignalSet, UnixAccessSet, UnixAddress, UnixLabel,
    UnixPattern, UnixPeer, UnixRule, UnixSocketType,
};
use std::fmt::Write as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut g = Governor::new(Budget::default());
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--baseline-only"] {
        return baseline_only(&mut g);
    }
    if let [mode, root] = args.as_slice()
        && mode == "--bound-imports"
    {
        return bound_imports(std::path::Path::new(root), &mut g);
    }
    if !args.is_empty() {
        return Err(
            "usage: policy_document [--baseline-only | --bound-imports INCLUDE_ROOT]".into(),
        );
    }

    let workspace = AppArmorGlob::parse("@{ROOT}/**", &["ROOT"], &mut g)?;
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/policy workspace",
    }];
    let local = AppArmorGlob::parse("@@{TMP}/ipc", &["TMP"], &mut g)?;
    let peer = AppArmorGlob::parse("@@{TMP}/host", &["TMP"], &mut g)?;
    let tmp = [LiteralPathBinding {
        name: "TMP",
        path: "/run/policy-example",
    }];
    let label = AppArmorGlob::parse("purrassist.tools", &[], &mut g)?;
    let any = InternetEndpoint {
        scope: InternetScope::parse("*", &mut g)?,
        ports: InternetPorts::Any,
    };
    let rules = [
        PolicyRule::File(FileRule {
            source: "01:file",
            path: &workspace,
            bindings: &bindings,
            permissions: FilePermissions::parse("rwkl", &mut g)?,
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
                accesses: InternetAccessSet::parse("connect", &mut g)?,
                audit: false,
                deny: false,
                local: any,
                peer: InternetEndpoint {
                    scope: InternetScope::parse("10.0.0.7", &mut g)?,
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
            accesses: UnixAccessSet::parse("send,receive", &mut g)?,
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
            access: SignalAccessSet::parse("receive", &mut g)?,
            signals: SignalSet::parse("term,int", &mut g)?,
        }),
        PolicyRule::Ptrace(PtraceRule {
            source: "05:ptrace",
            peer: &label,
            bindings: &[],
            access: PtraceAccessSet::parse("read", &mut g)?,
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
            rules: &rules,
            imports: &[],
        },
    ];
    let profiles = [
        PolicyProfile::Standalone(PolicyBody {
            source: "standalone",
            name: "purrassist.standalone",
            rules: &rules,
            imports: &[],
        }),
        PolicyProfile::Parent {
            body: PolicyBody {
                source: "parent",
                name: "purrassist.tools",
                rules: &[],
                imports: &[],
            },
            hats: &hats,
        },
    ];
    let input = PolicyInput {
        source: "caller:example",
        target: PolicyTarget::Abi4Ip,
        home: "/home/example",
        profiles: &profiles,
        dependencies: &[],
    };
    // This executable owns diagnostics; the library preserves exact borrowed errors.
    let document = PolicyDocument::prepare(input, &mut g).map_err(|e| e.to_string())?;
    let output = document.compile(&mut g).map_err(|e| e.to_string())?;
    print!("{}", output.text.as_str());
    Ok(())
}

fn baseline_only(g: &mut Governor) -> Result<(), Box<dyn std::error::Error>> {
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        source: "baseline",
        name: "baseline_only",
        rules: &[],
        imports: &[],
    })];
    let input = PolicyInput {
        source: "caller:baseline",
        target: PolicyTarget::Abi3,
        home: "/home/example/",
        profiles: &profiles,
        dependencies: &[],
    };
    let document = PolicyDocument::prepare(input, g).map_err(|e| e.to_string())?;
    let output = document.compile(g).map_err(|e| e.to_string())?;
    print!("{}", output.text.as_str());
    Ok(())
}

fn bound_imports(
    root: &std::path::Path,
    g: &mut Governor,
) -> Result<(), Box<dyn std::error::Error>> {
    let dependencies = [
        PolicyDependency {
            source: "fixture:parent",
            path: "abstractions/purrassist-imports/parent",
            bytes: include_bytes!("data/policy_import_parent.apparmor"),
        },
        PolicyDependency {
            source: "fixture:child",
            path: "abstractions/purrassist-imports/child",
            bytes: include_bytes!("data/policy_import_child.apparmor"),
        },
    ];
    let imports = [PolicyImport {
        source: "caller:include",
        path: dependencies[0].path,
    }];
    let profiles = [PolicyProfile::Standalone(PolicyBody {
        source: "caller:body",
        name: "purrassist.imports",
        rules: &[],
        imports: &imports,
    })];
    let document = PolicyDocument::prepare(
        PolicyInput {
            source: "caller:imports",
            target: PolicyTarget::Abi4Ip,
            home: "/home/example",
            profiles: &profiles,
            dependencies: &dependencies,
        },
        g,
    )
    .map_err(|e| e.to_string())?;
    let output = document.compile(g).map_err(|e| e.to_string())?;
    let unresolved = output
        .requirements
        .complete_mediation()
        .err()
        .ok_or("opaque imports falsely reported complete requirements")?;
    if !matches!(
        output.spans[unresolved.first_import_span].origin,
        purrassist_toolgraph::PolicyOrigin::Import { .. }
    ) {
        return Err("opaque requirements must retain their import source span".into());
    }
    // Require a fresh caller-selected root; never reuse ambient include files.
    std::fs::create_dir(root)?;
    let mut manifest = String::new();
    for bound in &output.dependencies {
        let path = root.join(bound.dependency.path);
        std::fs::create_dir_all(path.parent().ok_or("dependency has no parent")?)?;
        std::fs::write(&path, bound.dependency.bytes)?;
        writeln!(manifest, "{} {}", bound.digest, bound.dependency.path)?;
    }
    std::fs::write(root.join("dependency-identities.txt"), manifest)?;
    print!("{}", output.text.as_str());
    Ok(())
}
