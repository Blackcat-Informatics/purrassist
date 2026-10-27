// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Public capability/mount consumer; prints standalone offline fixtures only.

#[path = "support/privilege_cases.rs"]
pub mod cases;

use std::io::{self, Write};

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::{CapabilityRule, MountRule, dangerous_capability_denials};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut governor = Governor::new(Budget::default());
    let mut output = io::stdout().lock();
    // This executable owns the fixture envelope and diagnostics. Each rule or
    // complete baseline uses the actual public governed library operation.
    output.write_all(b"abi <abi/3.0>,\n")?;
    for &(name, number, dangerous) in cases::CAPABILITIES {
        let mut rule = CapabilityRule {
            source: "caller:capability",
            name,
            audit: false,
            deny: true,
        };
        let denial = rule.render(&mut governor)?;
        assert_eq!(denial.requirement.mediation, "caps");
        writeln!(output, "profile cap_deny_{number} {{\n{}}}", denial.text)?;
        if !dangerous {
            rule.deny = false;
            rule.audit = true;
            let grant = rule.render(&mut governor)?;
            writeln!(output, "profile cap_grant_{number} {{\n{}}}", grant.text)?;
        }
    }
    let baseline = dangerous_capability_denials("caller:baseline", false, &mut governor)?;
    assert_eq!(baseline.text, cases::BASELINE);
    writeln!(output, "profile cap_baseline {{\n{}}}", baseline.text)?;
    for operation in ["mount", "remount", "umount", "pivot_root"] {
        let fragment = MountRule {
            source: "caller:mount",
            operation,
            audit: false,
            deny: true,
        }
        .render(&mut governor)?;
        assert_eq!(fragment.requirement.mediation, "mount");
        writeln!(output, "profile deny_{operation} {{\n{}}}", fragment.text)?;
    }
    Ok(())
}
