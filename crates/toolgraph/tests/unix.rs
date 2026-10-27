// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact Unix scopes, shared socket access semantics and atomic governed output.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    AppArmorGlob, GlobBindingProblem, GlobError, LiteralPathBinding, UnixAccessSet, UnixAddress,
    UnixAddressProblem, UnixError, UnixField, UnixLabel, UnixPattern, UnixPeer, UnixRule,
    UnixSocketType,
};

fn governor() -> Governor {
    Governor::new(Budget::default())
}
fn plain<'a>() -> UnixRule<'a> {
    UnixRule {
        source: "caller:original-unix",
        accesses: UnixAccessSet::parse("send,receive", &mut governor())
            .expect("native nonempty accesses"),
        socket_type: UnixSocketType::Any,
        audit: false,
        deny: false,
        local: UnixAddress::Any,
        peer: UnixPeer {
            address: UnixAddress::Any,
            label: UnixLabel::Any,
        },
    }
}
const fn pattern<'a>(
    glob: &'a AppArmorGlob,
    bindings: &'a [LiteralPathBinding<'a>],
) -> UnixPattern<'a> {
    UnixPattern { glob, bindings }
}
fn reason(error: UnixError<'_>) -> StopReason {
    match error {
        UnixError::Resource { reason, .. }
        | UnixError::Pattern {
            reason: GlobError::Resource(reason),
            ..
        } => reason,
        other => panic!("expected exact resource cause, got {other:?}"),
    }
}

#[test]
fn canonical_fields_keep_bound_abstract_anonymous_and_unconstrained_scopes_distinct() {
    let local = AppArmorGlob::parse("@@{ROOT}/{ipc,alt}/[a-z]?*", &["ROOT"], &mut governor())
        .expect("admitted abstract AARE");
    let peer = AppArmorGlob::parse("@@{ROOT}/host", &["ROOT"], &mut governor())
        .expect("admitted abstract peer");
    let label = AppArmorGlob::parse("purrassist.{peer,other}", &[], &mut governor())
        .expect("admitted profile label");
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/run/example",
    }];
    let mut rule = plain();
    rule.local = UnixAddress::Abstract(pattern(&local, &bindings));
    rule.peer = UnixPeer {
        address: UnixAddress::Abstract(pattern(&peer, &bindings)),
        label: UnixLabel::Pattern(pattern(&label, &[])),
    };
    rule.socket_type = UnixSocketType::Datagram;
    rule.audit = true;
    rule.deny = true;
    rule.accesses = UnixAccessSet::parse("rw,connect,w", &mut governor()).expect("alias union");
    let output = rule
        .render(&mut governor())
        .expect("complete canonical rule");
    assert_eq!(
        output.text,
        "audit deny unix (connect, send, receive) type=dgram addr=\"@/run/example/{ipc,alt}/[a-z]?*\" peer=(label=\"purrassist.{peer,other}\" addr=\"@/run/example/host\"),\n"
    );
    assert_eq!(output.source, rule.source);
    assert_eq!(output.requirement.abi, "3.0");
    assert_eq!(output.requirement.mediation, "network/af_unix");

    let named_none = AppArmorGlob::parse("@none", &[], &mut governor())
        .expect("abstract name is not anonymous keyword");
    rule.audit = false;
    rule.deny = false;
    rule.socket_type = UnixSocketType::Stream;
    rule.peer = UnixPeer {
        address: UnixAddress::Any,
        label: UnixLabel::Any,
    };
    rule.local = UnixAddress::Anonymous;
    assert!(
        rule.render(&mut governor())
            .expect("anonymous condition")
            .text
            .contains(" addr=none,")
    );
    rule.local = UnixAddress::Abstract(pattern(&named_none, &[]));
    assert!(
        rule.render(&mut governor())
            .expect("literal abstract none name")
            .text
            .contains(" addr=\"@none\",")
    );
    let marker_only = AppArmorGlob::parse("@", &[], &mut governor())
        .expect("empty abstract name is distinct from anonymous");
    let nullable_label =
        AppArmorGlob::parse("{,}", &[], &mut governor()).expect("admitted empty label language");
    rule.local = UnixAddress::Abstract(pattern(&marker_only, &[]));
    rule.peer.label = UnixLabel::Pattern(pattern(&nullable_label, &[]));
    assert!(
        rule.render(&mut governor())
            .expect("native exact empty scopes are retained")
            .text
            .contains("addr=\"@\" peer=(label=\"{,}\")")
    );
    rule.peer.label = UnixLabel::Any;
    rule.local = UnixAddress::Any;
    assert_eq!(
        rule.render(&mut governor())
            .expect("explicit unconstrained")
            .text,
        "unix (connect, send, receive) type=stream,\n"
    );
}

#[test]
fn peer_compatibility_and_unknown_fields_are_refused_without_dropping_conditions() {
    let label =
        AppArmorGlob::parse("purrassist.peer", &[], &mut governor()).expect("trusted caller label");
    for access in [
        "create",
        "bind",
        "listen",
        "shutdown",
        "getattr",
        "setattr",
        "getopt",
        "setopt",
        "send,bind",
    ] {
        let mut rule = plain();
        rule.accesses = UnixAccessSet::parse(access, &mut governor()).expect("known access");
        rule.peer.label = UnixLabel::Pattern(pattern(&label, &[]));
        assert!(matches!(
            rule.render(&mut governor()),
            Err(UnixError::PeerAccess {
                source: "caller:original-unix",
                ..
            })
        ));
        rule.peer.label = UnixLabel::Any;
        assert!(
            rule.render(&mut governor()).is_ok(),
            "valid local-only neighbor {access}"
        );
    }
    for access in ["accept", "connect", "send", "receive", "r", "w", "rw"] {
        let mut rule = plain();
        rule.accesses = UnixAccessSet::parse(access, &mut governor()).expect("peer access");
        rule.peer.address = UnixAddress::Anonymous;
        assert!(
            rule.render(&mut governor()).is_ok(),
            "anonymous peer condition {access}"
        );
    }
    for value in ["", " ", "read", "send,", "send,,receive"] {
        assert_eq!(
            UnixAccessSet::parse(value, &mut governor()),
            Err(UnixError::Field {
                field: "access",
                source: value
            })
        );
    }
    for value in ["", "raw", "tcp", "unknown"] {
        assert_eq!(
            UnixSocketType::parse(value, &mut governor()),
            Err(UnixError::Field {
                field: "socket_type",
                source: value
            })
        );
    }
    for value in ["*", "stream", "dgram", "seqpacket"] {
        let mut rule = plain();
        rule.socket_type =
            UnixSocketType::parse(value, &mut governor()).expect("explicit admitted type");
        assert!(rule.render(&mut governor()).is_ok());
    }
}

#[test]
fn address_and_shared_binding_refusals_keep_original_field_and_offsets() {
    for (source, declarations, problem) in [
        (
            "/run/filesystem.sock",
            &[][..],
            UnixAddressProblem::Filesystem,
        ),
        (
            r"\/run/filesystem.sock",
            &[][..],
            UnixAddressProblem::Filesystem,
        ),
        (
            "@{ROOT}/ipc",
            &["ROOT"][..],
            UnixAddressProblem::MissingLiteralMarker,
        ),
        ("auto", &[][..], UnixAddressProblem::AutoUnsupported),
        (
            r"\@named",
            &[][..],
            UnixAddressProblem::MissingLiteralMarker,
        ),
    ] {
        let glob = AppArmorGlob::parse(source, declarations, &mut governor())
            .expect("existing syntax admission");
        let mut rule = plain();
        rule.local = UnixAddress::Abstract(pattern(&glob, &[]));
        assert_eq!(
            rule.render(&mut governor()).err(),
            Some(UnixError::Address {
                source: "caller:original-unix",
                field: UnixField::LocalAddress,
                pattern: source,
                problem
            })
        );
    }
    for source in [r"@\x00name", r"@\000name"] {
        assert!(matches!(
            AppArmorGlob::parse(source, &[], &mut governor()),
            Err(GlobError::Unsupported { offset: 1 })
        ));
    }
    let glob = AppArmorGlob::parse("@@{ROOT}/ipc", &["ROOT"], &mut governor())
        .expect("literal abstract marker and declared binding");
    let valid = LiteralPathBinding {
        name: "ROOT",
        path: "/run/example",
    };
    for (bindings, expected) in [
        (vec![], GlobError::MissingBinding { offset: 1 }),
        (
            vec![valid, valid],
            GlobError::Binding {
                index: 1,
                offset: 0,
                problem: GlobBindingProblem::Duplicate,
            },
        ),
        (
            vec![LiteralPathBinding {
                name: "ROOT",
                path: "/run/*",
            }],
            GlobError::Binding {
                index: 0,
                offset: 5,
                problem: GlobBindingProblem::UnsafePath,
            },
        ),
    ] {
        let mut rule = plain();
        rule.peer.address = UnixAddress::Abstract(pattern(&glob, &bindings));
        assert_eq!(
            rule.render(&mut governor()).err(),
            Some(UnixError::Pattern {
                source: "caller:original-unix",
                field: UnixField::PeerAddress,
                pattern: glob.source(),
                reason: expected
            })
        );
    }
    let label = AppArmorGlob::parse("purrassist.peer", &["ROOT"], &mut governor())
        .expect("unused declaration is caller-owned");
    let bindings = [valid];
    let mut rule = plain();
    rule.peer.label = UnixLabel::Pattern(pattern(&label, &bindings));
    assert!(matches!(
        rule.render(&mut governor()),
        Err(UnixError::Pattern {
            field: UnixField::PeerLabel,
            reason: GlobError::Binding {
                problem: GlobBindingProblem::Unused,
                index: 0,
                offset: 0
            },
            ..
        })
    ));
    rule.local = UnixAddress::Abstract(pattern(&glob, &bindings));
    rule.peer.label = UnixLabel::Any;
    assert!(
        rule.render(&mut governor()).is_ok(),
        "corrected literal marker and complete safe binding"
    );
}

#[test]
fn governed_rule_has_exact_work_scratch_depth_input_and_cancellation_neighbors() {
    let glob = AppArmorGlob::parse(
        "@@{ROOT}/longer-than-rule-source/{ipc,other}",
        &["ROOT"],
        &mut governor(),
    )
    .expect("caller-owned pattern");
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/run/example",
    }];
    let mut rule = plain();
    rule.source = "id";
    rule.local = UnixAddress::Abstract(pattern(&glob, &bindings));
    let mut measured = governor();
    let expected = rule
        .render(&mut measured)
        .expect("measure actual rule emitter");
    let usage = measured.usage();
    for (budget, resource) in [
        (
            Budget {
                work: usage.work - 1,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
        (
            Budget {
                depth: 0,
                ..Budget::default()
            },
            Resource::Depth,
        ),
        (
            Budget {
                input_bytes: glob.source().len() as u64 - 1,
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
    ] {
        assert_eq!(
            reason(
                rule.render(&mut Governor::new(budget))
                    .expect_err("atomic refusal has no fragment")
            ),
            StopReason::BudgetExceeded(resource)
        );
    }
    let actual = rule
        .render(&mut Governor::new(Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            depth: 1,
            input_bytes: glob.source().len() as u64,
            ..Budget::default()
        }))
        .expect("exact valid neighbor");
    assert_eq!(actual.text, expected.text);
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        reason(
            rule.render(&mut Governor::with_cancellation(Budget::default(), token))
                .expect_err("pre-tripped cancellation")
        ),
        StopReason::Cancelled
    );
    assert!(matches!(
        UnixAccessSet::parse(
            "send",
            &mut Governor::new(Budget {
                work: 0,
                ..Budget::default()
            })
        ),
        Err(UnixError::Resource {
            reason: StopReason::BudgetExceeded(Resource::Work),
            ..
        })
    ));
    assert!(UnixAccessSet::parse("send", &mut governor()).is_ok());
}
