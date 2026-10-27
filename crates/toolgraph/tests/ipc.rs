// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Closed IPC vocabulary, native label preservation and atomic resource refusal.

use core::fmt::Write;
use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    AppArmorGlob, GlobError, IpcError, LiteralPathBinding, PtraceAccessSet, PtraceRule,
    SignalAccessSet, SignalRule, SignalSet,
};
fn governor() -> Governor {
    Governor::new(Budget::default())
}

#[test]
fn canonical_signal_and_ptrace_sets_preserve_fields_labels_and_qualifiers() {
    let peer = AppArmorGlob::parse("@{LABEL}//{peer,}", &["LABEL"], &mut governor()).unwrap();
    let bindings = [LiteralPathBinding {
        name: "LABEL",
        path: "/purrassist",
    }];
    let source = "rtmin+00032 term,int,exists,rtmin+0000,term";
    let signal = SignalRule {
        source: "caller:signal",
        peer: &peer,
        bindings: &bindings,
        access: SignalAccessSet::parse("receive,send send", &mut governor()).unwrap(),
        signals: SignalSet::parse(source, &mut governor()).unwrap(),
        audit: true,
        deny: true,
    };
    let output = signal.render(&mut governor()).unwrap();
    assert_eq!(
        output.text,
        "audit deny signal (send, receive) set=(int, term, exists, rtmin+0, rtmin+32) peer=\"/purrassist//{peer,}\",\n"
    );
    assert_eq!(output.source, signal.source);
    assert_eq!(output.peer_source, peer.source());
    assert_eq!(output.bindings[0].path, bindings[0].path);
    assert_eq!(output.signal_source, Some(source));
    assert_eq!(output.requirement.abi, "3.0");
    assert_eq!(output.requirement.mediation, "signal");
    let ptrace = PtraceRule {
        source: "caller:ptrace",
        peer: &peer,
        bindings: &bindings,
        access: PtraceAccessSet::parse("tracedby read trace,readby trace", &mut governor())
            .unwrap(),
        audit: false,
        deny: false,
    };
    let output = ptrace.render(&mut governor()).unwrap();
    assert_eq!(
        output.text,
        "ptrace (trace, read, readby, tracedby) peer=\"/purrassist//{peer,}\",\n"
    );
    assert_eq!(output.requirement.mediation, "ptrace");
    assert_eq!(output.signal_source, None);
    let named = "hup int quit ill trap abrt bus fpe kill usr1 segv usr2 pipe alrm term stkflt chld cont stop stp ttin ttou urg xcpu xfsz vtalrm prof winch io pwr sys emt exists";
    let mut complete = named.to_string();
    for offset in 0..=32 {
        write!(complete, " rtmin+{offset}").unwrap();
    }
    let signals = SignalSet::parse(&complete, &mut governor()).unwrap();
    let full = SignalRule {
        signals,
        audit: false,
        deny: false,
        ..signal
    }
    .render(&mut governor())
    .unwrap();
    assert!(
        full.text
            .contains(&format!("set=({})", complete.replace(' ', ", ")))
    );
}

#[test]
fn malformed_sets_aliases_and_out_of_range_rt_words_refuse_with_original_fields() {
    for text in [
        "",
        " ",
        ",send",
        "send,",
        "send,,receive",
        "read",
        "rw",
        "write",
        "Send",
        "trace",
    ] {
        assert!(
            matches!(SignalAccessSet::parse(text, &mut governor()), Err(IpcError::Field { field: "signal.access", source, .. }) if source == text)
        );
    }
    for text in [
        "",
        "read,",
        "read,,trace",
        "r",
        "w",
        "rw",
        "write",
        "send",
        "Trace",
    ] {
        assert!(
            matches!(PtraceAccessSet::parse(text, &mut governor()), Err(IpcError::Field { field: "ptrace.access", source, .. }) if source == text)
        );
    }
    for text in [
        "",
        "term,",
        ",int",
        "term,,int",
        "tstp",
        "lost",
        "15",
        "-1",
        "rtmax",
        "rtmin+-1",
        "rtmin+33",
        "rtmin+",
        "rtmin+00099999999999999999999999",
        "rtmin+1x",
        "term\u{a0}int",
    ] {
        assert!(
            matches!(SignalSet::parse(text, &mut governor()), Err(IpcError::Field { field: "signal.set", source, .. }) if source == text)
        );
    }
    let zeros = format!("rtmin+{}32", "0".repeat(1024));
    assert!(SignalSet::parse(&zeros, &mut governor()).is_ok());
    assert!(SignalSet::parse("exists,stp,rtmin+0,rtmin+32", &mut governor()).is_ok());
    assert!(matches!(
        AppArmorGlob::parse("", &[], &mut governor()),
        Err(GlobError::Syntax { offset: 0 })
    ));
}

#[test]
fn mandatory_peer_bindings_preserve_original_glob_failures_and_nullable_semantics() {
    let peer = AppArmorGlob::parse("@{LABEL}//peer", &["LABEL"], &mut governor()).unwrap();
    let mut rule = PtraceRule {
        source: "caller:peer",
        peer: &peer,
        bindings: &[],
        access: PtraceAccessSet::parse("read", &mut governor()).unwrap(),
        audit: true,
        deny: false,
    };
    assert!(matches!(
        rule.render(&mut governor()),
        Err(IpcError::Peer {
            source: "caller:peer",
            pattern: "@{LABEL}//peer",
            reason: GlobError::MissingBinding { offset: 0 }
        })
    ));
    let bad = [LiteralPathBinding {
        name: "LABEL",
        path: "/bad/*",
    }];
    rule.bindings = &bad;
    assert!(matches!(
        rule.render(&mut governor()),
        Err(IpcError::Peer {
            reason: GlobError::Binding {
                index: 0,
                offset: 5,
                ..
            },
            ..
        })
    ));
    let good = [LiteralPathBinding {
        name: "LABEL",
        path: "/scope",
    }];
    rule.bindings = &good;
    assert_eq!(
        rule.render(&mut governor()).unwrap().text,
        "audit ptrace (read) peer=\"/scope//peer\",\n"
    );
    for source in ["**", "{,peer}", "peer//exact"] {
        let peer = AppArmorGlob::parse(source, &[], &mut governor()).unwrap();
        let output = PtraceRule {
            peer: &peer,
            bindings: &[],
            ..rule
        }
        .render(&mut governor())
        .unwrap();
        assert_eq!(
            output.text,
            format!("audit ptrace (read) peer=\"{source}\",\n")
        );
    }
}

#[test]
fn current_governors_bound_original_inputs_output_and_cancellation_atomically() {
    let peer_text = format!("peer{}", "p".repeat(128));
    let field = format!("rtmin+{}32 int", "0".repeat(256));
    let peer = AppArmorGlob::parse(&peer_text, &[], &mut governor()).unwrap();
    let signal = SignalRule {
        source: "caller:budget",
        peer: &peer,
        bindings: &[],
        access: SignalAccessSet::parse("send", &mut governor()).unwrap(),
        signals: SignalSet::parse(&field, &mut governor()).unwrap(),
        audit: false,
        deny: false,
    };
    let ptrace = PtraceRule {
        source: signal.source,
        peer: signal.peer,
        bindings: &[],
        access: PtraceAccessSet::parse("trace", &mut governor()).unwrap(),
        audit: false,
        deny: false,
    };
    for is_signal in [true, false] {
        let render = |g: &mut Governor| {
            if is_signal {
                signal.render(g)
            } else {
                ptrace.render(g)
            }
        };
        let mut g = governor();
        let text = render(&mut g).unwrap().text;
        let usage = g.usage();
        let input = if is_signal {
            field.len()
        } else {
            peer_text.len()
        };
        let exact = Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            input_bytes: u64::try_from(input).unwrap(),
            ..Budget::default()
        };
        assert_eq!(render(&mut Governor::new(exact)).unwrap().text, text);
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
            let reason = match render(&mut g).err().unwrap() {
                IpcError::Resource { reason, .. }
                | IpcError::Peer {
                    reason: GlobError::Resource(reason),
                    ..
                } => reason,
                other => panic!("unexpected semantic refusal {other:?}"),
            };
            assert_eq!(reason, StopReason::BudgetExceeded(resource));
            assert_eq!(g.check(), Err(reason));
            assert_eq!(
                g.scratch_meter().live(),
                0,
                "private partial output is dropped"
            );
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut g = Governor::with_cancellation(exact, token);
        assert!(matches!(
            render(&mut g),
            Err(IpcError::Resource {
                reason: StopReason::Cancelled,
                ..
            })
        ));
        assert_eq!(g.usage().work, 0);
    }
    assert_constructor_input_limits();
}

fn assert_constructor_input_limits() {
    for text in ["send", "trace", "term"] {
        let mut g = Governor::new(Budget {
            input_bytes: 0,
            ..Budget::default()
        });
        let error = match text {
            "send" => SignalAccessSet::parse(text, &mut g).err().unwrap(),
            "trace" => PtraceAccessSet::parse(text, &mut g).err().unwrap(),
            _ => SignalSet::parse(text, &mut g).err().unwrap(),
        };
        assert!(matches!(
            error,
            IpcError::Resource {
                reason: StopReason::BudgetExceeded(Resource::InputBytes),
                ..
            }
        ));
    }
}
