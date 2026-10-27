// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Actual signal/ptrace API consumer and isolated native qualification fixtures.

use purrassist_core::{
    Budget, Governor,
    contract::{Resource, format_output_governed},
};
use purrassist_toolgraph::{
    AppArmorGlob, LiteralPathBinding, PtraceAccessSet, PtraceRule, SignalAccessSet, SignalRule,
    SignalSet,
};

fn render(
    kind: &str,
    access: &str,
    signals: &str,
    audit: bool,
    deny: bool,
) -> Result<String, String> {
    let mut g = Governor::new(Budget::default());
    let peer = AppArmorGlob::parse("@{LABEL}//peer", &["LABEL"], &mut g)
        .map_err(|error| error.to_string())?;
    let bindings = [LiteralPathBinding {
        name: "LABEL",
        path: "/purrassist",
    }];
    let fragment = match kind {
        "signal" => SignalRule {
            source: "caller:signal",
            peer: &peer,
            bindings: &bindings,
            audit,
            deny,
            access: SignalAccessSet::parse(access, &mut g).map_err(|error| error.to_string())?,
            signals: SignalSet::parse(signals, &mut g).map_err(|error| error.to_string())?,
        }
        .render(&mut g)
        .map_err(|error| error.to_string())?,
        "ptrace" => PtraceRule {
            source: "caller:ptrace",
            peer: &peer,
            bindings: &bindings,
            audit,
            deny,
            access: PtraceAccessSet::parse(access, &mut g).map_err(|error| error.to_string())?,
        }
        .render(&mut g)
        .map_err(|error| error.to_string())?,
        _ => return Err("fixture kind must be signal or ptrace".into()),
    };
    assert_eq!(fragment.requirement.abi, "3.0");
    assert_eq!(fragment.requirement.mediation, kind);
    assert_eq!(fragment.peer_source, "@{LABEL}//peer");
    if kind == "signal" {
        assert_eq!(fragment.signal_source, Some(signals));
    }
    Ok(fragment.text)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let body = match args.as_slice() {
        [] => {
            let signal = render("signal", "receive, send, send", "rtmin+00032 term,int,exists,rtmin+0", true, false)?;
            let ptrace = render("ptrace", "tracedby read trace readby", "", false, false)?;
            assert_eq!(signal, "audit signal (send, receive) set=(int, term, exists, rtmin+0, rtmin+32) peer=\"/purrassist//peer\",\n");
            assert_eq!(ptrace, "ptrace (trace, read, readby, tracedby) peer=\"/purrassist//peer\",\n");
            signal + &ptrace
        }
        [kind, access, signals, qualifiers] => {
            let (audit, deny) = match qualifiers.as_str() {
                "allow" => (false, false), "audit" => (true, false),
                "deny" => (false, true), "audit-deny" => (true, true),
                _ => return Err("fixture qualifier must be allow, audit, deny or audit-deny".into()),
            };
            render(kind, access, signals, audit, deny)?
        }
        _ => return Err("use no arguments for the demo, or KIND ACCESS SIGNALS QUALIFIER for an isolated native fixture".into()),
    };
    let mut g = Governor::new(Budget::default());
    g.charge(Resource::Work, 64)?;
    let policy = format_output_governed(
        format_args!("abi <abi/3.0>,\nprofile ipc_fixture {{\n{body}}}\n"),
        &mut g,
    )?;
    print!("{policy}");
    Ok(())
}
