// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Render actual standalone file fragments for offline native qualification.

use purrassist_core::{
    Budget, Governor, ScratchString,
    contract::{Resource, format_output_governed},
};
use purrassist_toolgraph::{
    AppArmorGlob, FileCanonicalization, FileExecution, FilePermissions, FileRule, FileTarget,
    LiteralPathBinding,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut g = Governor::new(Budget::default());
    let path = AppArmorGlob::parse("@{ROOT}/**", &["ROOT"], &mut g)?;
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/file fixture",
    }];
    let mut rule = FileRule {
        source: "caller:owner",
        path: &path,
        bindings: &bindings,
        permissions: FilePermissions::parse("mlkrw", &mut g)?,
        execution: None,
        target: None,
        audit: true,
        deny: false,
        owner: true,
    };
    let owner = rule.render(&mut g).map_err(|error| error.to_string())?;
    assert_eq!(owner.text, "audit owner \"/srv/file fixture/**\" rwklm,\n");
    let executable = AppArmorGlob::parse("/bin/file-fixture", &[], &mut g)?;
    rule.path = &executable;
    rule.bindings = &[];
    rule.source = "caller:transition";
    rule.permissions = FilePermissions::parse("r", &mut g)?;
    rule.execution = Some(FileExecution::Profile);
    rule.target = Some(FileTarget::parse("file_target", &mut g)?);
    rule.audit = false;
    rule.owner = false;
    let transition = rule.render(&mut g).map_err(|error| error.to_string())?;
    assert_eq!(
        transition.text,
        "\"/bin/file-fixture\" rpx -> file_target,\n"
    );
    let denied_path = AppArmorGlob::parse("/bin/file-denied", &[], &mut g)?;
    rule.path = &denied_path;
    rule.source = "caller:deny";
    rule.target = None;
    rule.execution = Some(FileExecution::Inherit);
    rule.audit = true;
    rule.deny = true;
    let denied = rule.render(&mut g).map_err(|error| error.to_string())?;
    assert_eq!(
        denied.canonicalization,
        FileCanonicalization::DeniedInheritToExecute
    );
    assert_eq!(denied.requested_execution, Some(FileExecution::Inherit));
    assert_eq!(denied.text, "audit deny \"/bin/file-denied\" rx,\n");
    let modes = additional_rules(&mut g)?;
    // The static frame and requirement names have bounded Display computation.
    g.charge(Resource::Work, 128)?;
    let policy = format_output_governed(
        format_args!(
            "abi <abi/{}>,\nprofile file_fixture {{\n{}{}{}{}profile child_target {{}}\n}}\nprofile file_target {{}}\n",
            owner.requirement.abi,
            owner.text,
            transition.text,
            denied.text,
            modes.as_str(),
        ),
        &mut g,
    )?;
    print!("{policy}");
    Ok(())
}

fn additional_rules(g: &mut Governor) -> Result<String, Box<dyn std::error::Error>> {
    let mut modes = ScratchString::new(g);
    for (execution, spelling) in [
        (FileExecution::Inherit, "ix"),
        (FileExecution::Profile, "px"),
        (FileExecution::Child, "cx"),
        (FileExecution::ProfileInherit, "Pix"),
        (FileExecution::ChildInherit, "Cix"),
    ] {
        for named in [false, true] {
            if execution == FileExecution::Inherit && named {
                continue;
            }
            g.charge(Resource::Work, 32)?;
            let path_text = format_output_governed(
                format_args!(
                    "/bin/mode-{spelling}-{}",
                    if named { "named" } else { "attachment" }
                ),
                g,
            )?;
            let path = AppArmorGlob::parse(&path_text, &[], g)?;
            let target = if named {
                Some(FileTarget::parse(
                    if matches!(
                        execution,
                        FileExecution::Child | FileExecution::ChildInherit
                    ) {
                        "child_target"
                    } else {
                        "file_target"
                    },
                    g,
                )?)
            } else {
                None
            };
            let mode = FileRule {
                source: "caller:mode",
                path: &path,
                bindings: &[],
                permissions: FilePermissions::parse("r", g)?,
                execution: Some(execution),
                target,
                audit: false,
                deny: false,
                owner: false,
            };
            let fragment = mode.render(g).map_err(|error| error.to_string())?;
            g.charge(Resource::Work, u64::try_from(fragment.text.len())?)?;
            modes.push_str(&fragment.text)?;
        }
    }
    for permission in ["r", "w", "a", "k", "l", "m"] {
        g.charge(Resource::Work, 32)?;
        let path_text = format_output_governed(format_args!("/srv/permission-{permission}"), g)?;
        let path = AppArmorGlob::parse(&path_text, &[], g)?;
        let access = FileRule {
            source: "caller:access",
            path: &path,
            bindings: &[],
            permissions: FilePermissions::parse(permission, g)?,
            execution: None,
            target: None,
            audit: false,
            deny: false,
            owner: false,
        };
        let fragment = access.render(g).map_err(|error| error.to_string())?;
        g.charge(Resource::Work, u64::try_from(fragment.text.len())?)?;
        modes.push_str(&fragment.text)?;
    }
    Ok(modes.into_string())
}
