// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Public standalone Unix consumer; prints an offline parser fixture only.

use purrassist_core::{Budget, Governor, contract::format_output_governed};
use purrassist_toolgraph::{
    AppArmorGlob, LiteralPathBinding, UnixAccessSet, UnixAddress, UnixLabel, UnixPattern, UnixPeer,
    UnixRule, UnixSocketType,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut governor = Governor::new(Budget::default());
    let local = AppArmorGlob::parse("@@{TOOL_TMP}/ipc", &["TOOL_TMP"], &mut governor)?;
    let peer = AppArmorGlob::parse("@@{TOOL_TMP}/host", &["TOOL_TMP"], &mut governor)?;
    let label = AppArmorGlob::parse("purrassist.peer", &[], &mut governor)?;
    let bindings = [LiteralPathBinding {
        name: "TOOL_TMP",
        path: "/run/purrassist/session-17",
    }];
    let mut rule = UnixRule {
        source: "caller:abstract-ipc",
        accesses: UnixAccessSet::parse("send,receive", &mut governor)?,
        socket_type: UnixSocketType::Stream,
        audit: false,
        deny: false,
        local: UnixAddress::Abstract(UnixPattern {
            glob: &local,
            bindings: &bindings,
        }),
        peer: UnixPeer {
            address: UnixAddress::Abstract(UnixPattern {
                glob: &peer,
                bindings: &bindings,
            }),
            label: UnixLabel::Pattern(UnixPattern {
                glob: &label,
                bindings: &[],
            }),
        },
    };
    // This executable owns diagnostics; the library retains borrowed source errors.
    let abstract_rule = rule
        .render(&mut governor)
        .map_err(|error| error.to_string())?;
    rule.source = "caller:anonymous";
    rule.accesses = UnixAccessSet::parse("shutdown,getattr", &mut governor)?;
    rule.local = UnixAddress::Anonymous;
    rule.peer = UnixPeer {
        address: UnixAddress::Any,
        label: UnixLabel::Any,
    };
    let anonymous = rule
        .render(&mut governor)
        .map_err(|error| error.to_string())?;
    rule.source = "caller:unconstrained";
    rule.accesses = UnixAccessSet::parse("create,bind,listen", &mut governor)?;
    rule.local = UnixAddress::Any;
    rule.socket_type = UnixSocketType::Any;
    let unconstrained = rule
        .render(&mut governor)
        .map_err(|error| error.to_string())?;
    assert_eq!(abstract_rule.requirement, anonymous.requirement);
    assert_eq!(abstract_rule.requirement, unconstrained.requirement);
    assert_eq!(abstract_rule.requirement.mediation, "network/af_unix");
    governor.charge(purrassist_core::contract::Resource::Work, 128)?;
    let fixture = format_output_governed(
        format_args!(
            "abi <abi/{}>,\nprofile unix_fixture {{\n{}{}{}}}\n",
            abstract_rule.requirement.abi, abstract_rule.text, anonymous.text, unconstrained.text
        ),
        &mut governor,
    )?;
    print!("{fixture}");
    Ok(())
}
