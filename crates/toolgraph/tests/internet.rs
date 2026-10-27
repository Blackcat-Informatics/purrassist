// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Standalone exact scopes, complete CIDR products and atomic governed refusal.

use purrassist_core::{Budget, CancellationToken, Governor, StopReason, contract::Resource};
use purrassist_toolgraph::{
    InternetAccessSet, InternetEndpoint, InternetError, InternetFamily, InternetLimits,
    InternetPorts, InternetRule, InternetScope, InternetSelector, InternetSide,
};

fn governor() -> Governor {
    Governor::new(Budget::default())
}
fn endpoint(scope: &str, ports: InternetPorts) -> InternetEndpoint<'_> {
    InternetEndpoint {
        scope: InternetScope::parse(scope, &mut governor())
            .expect("numeric or unconstrained fixture"),
        ports,
    }
}
fn rule(peer: &str) -> InternetRule<'_> {
    InternetRule {
        source: "caller:original-rule",
        family: InternetFamily::Inet,
        selector: InternetSelector::Stream,
        accesses: InternetAccessSet::parse("connect", &mut governor())
            .expect("nonempty native access"),
        audit: false,
        deny: false,
        local: endpoint("*", InternetPorts::Any),
        peer: endpoint(
            peer,
            InternetPorts::Range {
                first: 5432,
                last: 5433,
            },
        ),
    }
}
const SMALL: InternetLimits = InternetLimits {
    addresses_per_endpoint: 4,
    rules: 16,
};

#[test]
fn complete_numeric_cidrs_and_local_peer_products_preserve_all_constraints() {
    let mut declaration = rule("192.0.2.1/30");
    declaration.local = endpoint(
        "198.51.100.1/31",
        InternetPorts::Range {
            first: 0,
            last: 65535,
        },
    );
    declaration.audit = true;
    declaration.deny = true;
    declaration.accesses =
        InternetAccessSet::parse("rw,connect,w", &mut governor()).expect("aliases union");
    let output = declaration
        .render(SMALL, &mut governor())
        .expect("bounded complete product");
    assert_eq!(output.source, declaration.source);
    assert_eq!(output.requirement.abi, "4.0-ip");
    assert_eq!(output.requirement.mediation, "network_v8/af_inet");
    assert_eq!(output.rules, 8);
    let expected: Vec<_> = ["198.51.100.0", "198.51.100.1"].into_iter().flat_map(|local| {
        ["192.0.2.0", "192.0.2.1", "192.0.2.2", "192.0.2.3"].into_iter().map(move |peer| format!("audit deny network (connect, send, receive) inet stream ip={local} port=0-65535 peer=(ip={peer} port=5432-5433),"))
    }).collect();
    assert_eq!(output.text.lines().collect::<Vec<_>>(), expected);
    assert_eq!(declaration.peer.scope.source(), "192.0.2.1/30");

    for (scope, family, expected) in [
        (
            "255.255.255.255/32",
            InternetFamily::Inet,
            "255.255.255.255",
        ),
        (
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/128",
            InternetFamily::Inet6,
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        ),
        ("2001:db8::3/128", InternetFamily::Inet6, "2001:db8::3"),
    ] {
        let mut exact = rule(scope);
        exact.family = family;
        let output = exact
            .render(SMALL, &mut governor())
            .expect("maximum exact address");
        assert_eq!(output.rules, 1);
        assert!(
            output
                .text
                .contains(&format!("ip={expected} port=5432-5433"))
        );
    }
    let mut v6 = rule("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/127");
    v6.family = InternetFamily::Inet6;
    let output = v6
        .render(SMALL, &mut governor())
        .expect("inclusive maximum IPv6 neighbor");
    assert_eq!(output.rules, 2);
    assert!(
        output
            .text
            .contains("ip=ffff:ffff:ffff:ffff:ffff:ffff:ffff:fffe ")
    );
    assert!(
        output
            .text
            .contains("ip=ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff ")
    );

    let mut unconstrained = rule("*");
    unconstrained.peer.ports = InternetPorts::Any;
    unconstrained.accesses =
        InternetAccessSet::parse("bind", &mut governor()).expect("local access");
    assert_eq!(
        unconstrained
            .render(SMALL, &mut governor())
            .expect("explicit unconstrained peer")
            .text,
        "network (bind) inet stream,\n"
    );
}

#[test]
fn expansion_limits_are_exact_before_output_and_never_widen_or_truncate() {
    for (scope, family, power) in [
        ("10.0.0.7/8", InternetFamily::Inet, 24),
        ("::/0", InternetFamily::Inet6, 128),
        ("2001:db8::/64", InternetFamily::Inet6, 64),
    ] {
        let mut declaration = rule(scope);
        declaration.family = family;
        let mut g = governor();
        assert_eq!(
            declaration.render(SMALL, &mut g).err(),
            Some(InternetError::Addresses {
                source: declaration.source,
                side: InternetSide::Peer,
                scope,
                required_power: power,
                limit: 4
            })
        );
        assert_eq!(
            g.usage().peak_scratch_bytes,
            0,
            "no output allocated before cardinality refusal"
        );
    }
    assert!(rule("10.0.0.7").render(SMALL, &mut governor()).is_ok());
    let declaration = rule("192.0.2.0/30");
    assert!(
        declaration
            .render(
                InternetLimits {
                    addresses_per_endpoint: 4,
                    rules: 4
                },
                &mut governor()
            )
            .is_ok()
    );
    assert!(matches!(
        declaration.render(
            InternetLimits {
                addresses_per_endpoint: 3,
                rules: 4
            },
            &mut governor()
        ),
        Err(InternetError::Addresses {
            required_power: 2,
            ..
        })
    ));

    let mut product = declaration;
    product.local = endpoint("198.51.100.0/31", InternetPorts::Any);
    assert!(matches!(
        product.render(
            InternetLimits {
                addresses_per_endpoint: 4,
                rules: 7
            },
            &mut governor()
        ),
        Err(InternetError::Rules {
            required: 8,
            limit: 7,
            ..
        })
    ));
    assert!(
        product
            .render(
                InternetLimits {
                    addresses_per_endpoint: 4,
                    rules: 8
                },
                &mut governor()
            )
            .is_ok()
    );

    let maximum = InternetLimits {
        addresses_per_endpoint: u64::MAX,
        rules: u64::MAX,
    };
    let mut huge = rule("::/65");
    huge.family = InternetFamily::Inet6;
    huge.local = endpoint("::/65", InternetPorts::Any);
    assert!(
        matches!(huge.render(maximum, &mut governor()), Err(InternetError::Rules { required, .. }) if required == 1_u128 << 126)
    );
    huge.local = endpoint("*", InternetPorts::Any);
    assert!(matches!(
        huge.render(maximum, &mut governor()),
        Err(InternetError::Resource {
            reason: StopReason::BudgetExceeded(Resource::Work),
            ..
        })
    ));
}

#[test]
fn invalid_declarations_retain_source_and_peer_access_and_family_are_enforced() {
    for scope in [
        "database.example",
        "10.0.0.256",
        "10.0.0.1/33",
        "::/129",
        "::/",
        "fe80::1%eth0",
        "10.0.0.1/8/9",
    ] {
        assert_eq!(
            InternetScope::parse(scope, &mut governor()),
            Err(InternetError::Scope { source: scope })
        );
    }
    for value in ["", "read", "connect,", "connect,,send"] {
        assert!(
            matches!(InternetAccessSet::parse(value, &mut governor()), Err(InternetError::Field { field: "access", source }) if source == value)
        );
    }
    assert!(matches!(
        InternetFamily::parse("unix", &mut governor()),
        Err(InternetError::Field {
            field: "family",
            source: "unix"
        })
    ));
    assert!(matches!(
        InternetSelector::parse("service-name", &mut governor()),
        Err(InternetError::Field {
            field: "selector",
            ..
        })
    ));
    for port in ["65536", "-1", "1-2-3", "http", "1-", "+2"] {
        assert!(
            matches!(InternetPorts::parse(port, &mut governor()), Err(InternetError::Field { field: "ports", source }) if source == port)
        );
    }
    assert_eq!(
        InternetPorts::parse("9-8", &mut governor()),
        Err(InternetError::ReversedPorts {
            source: "9-8",
            first: 9,
            last: 8
        })
    );
    let mut invalid = rule("192.0.2.1");
    invalid.peer.ports = InternetPorts::Range { first: 9, last: 8 };
    assert!(matches!(
        invalid.render(SMALL, &mut governor()),
        Err(InternetError::ReversedPorts {
            source: "caller:original-rule",
            ..
        })
    ));

    let mut protocol_mismatch = rule("::1");
    protocol_mismatch.family = InternetFamily::Inet6;
    protocol_mismatch.selector = InternetSelector::Icmp;
    assert!(matches!(
        protocol_mismatch.render(SMALL, &mut governor()),
        Err(InternetError::SelectorFamily {
            source: "caller:original-rule",
            ..
        })
    ));

    for (side, local, peer) in [
        (InternetSide::Local, "::1", "192.0.2.1"),
        (InternetSide::Peer, "*", "::1"),
    ] {
        let mut mismatched = rule(peer);
        mismatched.local = endpoint(local, InternetPorts::Any);
        assert!(
            matches!(mismatched.render(SMALL, &mut governor()), Err(InternetError::Family { side: actual, scope: "::1", source: "caller:original-rule" }) if actual == side)
        );
    }
    for access in [
        "create", "bind", "listen", "shutdown", "getattr", "setattr", "getopt", "setopt",
    ] {
        let mut declaration = rule("192.0.2.1");
        declaration.accesses =
            InternetAccessSet::parse(access, &mut governor()).expect("known access");
        assert!(
            matches!(declaration.render(SMALL, &mut governor()), Err(InternetError::PeerAccess { access: actual, .. }) if actual == access)
        );
    }
    for access in ["accept", "connect", "send", "receive", "rw"] {
        let mut declaration = rule("*");
        declaration.accesses =
            InternetAccessSet::parse(access, &mut governor()).expect("peer access");
        assert!(
            declaration.render(SMALL, &mut governor()).is_ok(),
            "port-only peer conditions"
        );
    }
}

#[test]
fn resource_boundaries_refuse_atomically_with_exact_valid_neighbors() {
    let declaration = rule("192.0.2.0/30");
    let mut measured = governor();
    let expected = declaration
        .render(SMALL, &mut measured)
        .expect("measure actual output");
    let usage = measured.usage();
    for budget in [
        Budget {
            work: usage.work - 1,
            ..Budget::default()
        },
        Budget {
            scratch_bytes: usage.peak_scratch_bytes - 1,
            ..Budget::default()
        },
        Budget {
            input_bytes: (declaration.source.len() - 1) as u64,
            ..Budget::default()
        },
    ] {
        assert!(matches!(
            declaration.render(SMALL, &mut Governor::new(budget)),
            Err(InternetError::Resource { .. })
        ));
    }
    let result = declaration
        .render(
            SMALL,
            &mut Governor::new(Budget {
                work: usage.work,
                scratch_bytes: usage.peak_scratch_bytes,
                input_bytes: declaration.source.len() as u64,
                ..Budget::default()
            }),
        )
        .expect("exact boundary neighbor");
    assert_eq!(result.text, expected.text);
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        declaration.render(
            SMALL,
            &mut Governor::with_cancellation(Budget::default(), token)
        ),
        Err(InternetError::Resource {
            reason: StopReason::Cancelled,
            ..
        })
    ));
    assert!(matches!(
        InternetScope::parse(
            "192.0.2.1",
            &mut Governor::new(Budget {
                work: 0,
                ..Budget::default()
            })
        ),
        Err(InternetError::Resource {
            reason: StopReason::BudgetExceeded(Resource::Work),
            ..
        })
    ));
    assert!(InternetScope::parse("192.0.2.1", &mut governor()).is_ok());
}
