// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed shared JSON bytes, storage growth and traversal boundaries.

use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    json::{self, Node, Value},
};

#[test]
fn emitters_preserve_known_control_unicode_number_and_occurrence_bytes() {
    let controls: String = (0..32)
        .map(|value| char::from_u32(value).expect("ASCII control scalar"))
        .collect();
    let expected = r#""\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000b\f\r\u000e\u000f\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f""#;
    for text in [controls.as_str(), "é😀\"\\/end"] {
        let expected = if text == controls {
            expected
        } else {
            r#""é😀\"\\/end""#
        };
        assert_eq!(json::quote(text), expected);
        assert_eq!(
            json::quote_governed(text, &mut Governor::new(Budget::default()))
                .expect("admitted quoted text"),
            expected
        );
        assert_eq!(
            Node::new(Value::String(text.into()))
                .to_json_governed(&mut Governor::new(Budget::default()))
                .expect("admitted string node"),
            expected
        );
    }
    for expected in [
        "null",
        "true",
        "false",
        "[]",
        "{}",
        r#"{"z":1.2300e+99,"z":-0.00,"a":[{},[],{"é":"😀"}]}"#,
    ] {
        let node = json::parse(expected).expect("known source JSON");
        assert_eq!(node.to_json(), expected);
        assert_eq!(
            node.to_json_governed(&mut Governor::new(Budget::default()))
                .expect("admitted JSON"),
            expected
        );
    }
}

#[test]
fn escaped_output_backing_capacity_is_admitted_before_growth() {
    for length in [31, 511, 8191] {
        let input = "\0".repeat(length);
        let mut governor = Governor::new(Budget::default());
        let encoded =
            json::quote_governed(&input, &mut governor).expect("escaped expansion admitted");
        let retained = governor.scratch_meter().live();
        assert_eq!(encoded.len(), length * 6 + 2);
        assert!(
            u64::try_from(encoded.capacity()).expect("native capacity fits counter") <= retained
        );
        let budget = Budget {
            scratch_bytes: retained - 1,
            ..Budget::default()
        };
        let mut exhausted = Governor::new(budget);
        assert_eq!(
            json::quote_governed(&input, &mut exhausted),
            Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
        );
        assert_eq!(
            exhausted.scratch_meter().live(),
            0,
            "failed output is dropped rather than returned partially"
        );
        let mut exact = Governor::new(Budget {
            scratch_bytes: retained,
            ..Budget::default()
        });
        assert_eq!(
            json::quote_governed(&input, &mut exact).expect("exact storage neighbor"),
            encoded
        );
    }
}

#[test]
fn work_depth_and_cancellation_refuse_before_unadmitted_emission() {
    let node = json::parse(r#"{"a\u0000":{"b":[false]}}"#).expect("caller-owned source tree");
    let mut governor = Governor::new(Budget::default());
    node.to_json_governed(&mut governor)
        .expect("measure complete emission");
    let required = governor.usage().work;
    for (budget, resource) in [
        (
            Budget {
                work: required - 1,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                depth: 2,
                ..Budget::default()
            },
            Resource::Depth,
        ),
        (
            Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
    ] {
        assert_eq!(
            node.to_json_governed(&mut Governor::new(budget)),
            Err(StopReason::BudgetExceeded(resource))
        );
    }
    let exact = Budget {
        work: required,
        depth: 3,
        ..Budget::default()
    };
    assert_eq!(
        node.to_json_governed(&mut Governor::new(exact))
            .expect("exact work/depth neighbor"),
        node.to_json()
    );
    let key = json::parse(r#"{"long key\u0000":0}"#).expect("key has no extra value depth");
    key.to_json_governed(&mut Governor::new(Budget {
        depth: 1,
        ..Budget::default()
    }))
    .expect("keys do not add depth");
    let mut before_scan = Governor::new(Budget {
        work: 1,
        ..Budget::default()
    });
    assert_eq!(
        json::quote_governed("caller text", &mut before_scan),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(
        before_scan.usage().peak_scratch_bytes,
        0,
        "scan admission precedes output allocation"
    );
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        node.to_json_governed(&mut Governor::with_cancellation(
            Budget::default(),
            token.clone()
        )),
        Err(StopReason::Cancelled)
    );
    assert_eq!(
        json::quote_governed(
            "text",
            &mut Governor::with_cancellation(Budget::default(), token)
        ),
        Err(StopReason::Cancelled)
    );
}

#[test]
fn governed_traversal_handles_deep_trees_and_width_independent_cursor_storage() {
    let depth = 4096;
    let mut node = Node::new(Value::Null);
    for _ in 0..depth {
        node = Node::new(Value::Array(vec![node]));
    }
    let mut governor = Governor::new(Budget {
        depth,
        ..Budget::default()
    });
    let encoded = node
        .to_json_governed(&mut governor)
        .expect("explicit iterative deep traversal");
    assert_eq!(
        encoded,
        format!(
            "{}null{}",
            "[".repeat(usize::try_from(depth).expect("test depth fits usize")),
            "]".repeat(usize::try_from(depth).expect("test depth fits usize"))
        )
    );
    // Source ownership/destruction is outside the emitter. Tear down the caller's
    // deep tree iteratively so its recursive Drop cannot confound this check.
    while let Value::Array(mut children) = node.value {
        node = children.pop().expect("one child per caller-built level");
    }
    let mut overhead = Vec::new();
    for width in [8, 4096] {
        let node = Node::new(Value::Array(vec![Node::new(Value::Null); width]));
        let mut governor = Governor::new(Budget::default());
        let encoded = node
            .to_json_governed(&mut governor)
            .expect("wide shallow tree");
        overhead.push(
            governor.usage().total_scratch_charged
                - u64::try_from(encoded.capacity()).expect("capacity fits counter"),
        );
    }
    assert_eq!(
        overhead[0], overhead[1],
        "cursor storage follows depth rather than sibling count"
    );
}
