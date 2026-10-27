// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Assemble an offline parser fixture from the actual standalone rule API.

use purrassist_core::{Budget, Governor, contract::format_output_governed};
use purrassist_toolgraph::{
    InternetAccessSet, InternetEndpoint, InternetFamily, InternetLimits, InternetPorts,
    InternetRule, InternetScope, InternetSelector,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut g = Governor::new(Budget::default());
    let any = InternetEndpoint {
        scope: InternetScope::parse("*", &mut g)?,
        ports: InternetPorts::Any,
    };
    let mut rule = InternetRule {
        source: "caller:small-cidr",
        family: InternetFamily::Inet,
        selector: InternetSelector::Stream,
        accesses: InternetAccessSet::parse("connect", &mut g)?,
        audit: false,
        deny: false,
        local: any,
        peer: InternetEndpoint {
            scope: InternetScope::parse("192.0.2.1/30", &mut g)?,
            ports: InternetPorts::Range {
                first: 5432,
                last: 5433,
            },
        },
    };
    let limits = InternetLimits {
        addresses_per_endpoint: 4,
        rules: 4,
    };
    let v4 = rule.render(limits, &mut g)?;
    rule.source = "caller:ipv6";
    rule.family = InternetFamily::Inet6;
    rule.local = InternetEndpoint {
        scope: InternetScope::parse("2001:db8::2", &mut g)?,
        ports: InternetPorts::Range {
            first: 12345,
            last: 12345,
        },
    };
    rule.peer.scope = InternetScope::parse("2001:db8::7", &mut g)?;
    let v6 = rule.render(limits, &mut g)?;
    assert_eq!(v4.requirement, v6.requirement);
    assert_eq!(v4.requirement.mediation, "network_v8/af_inet");
    // The short static frame and ABI name have bounded Display computation.
    g.charge(purrassist_core::contract::Resource::Work, 128)?;
    let fixture = format_output_governed(
        format_args!(
            "abi <abi/{}>,\nprofile internet_fixture {{\n{}{}}}\n",
            v4.requirement.abi, v4.text, v6.text
        ),
        &mut g,
    )?;
    print!("{fixture}");
    Ok(())
}
