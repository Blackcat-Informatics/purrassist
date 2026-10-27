// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Complete context wire values, refusal neighbors and resource boundaries.

use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    json::{self, Member, Node, Value},
};
use purrassist_toolgraph::{
    ContextCodecError, ContextManagement, FieldMask, FieldMaskError, LoadingStrategy,
    MCP_CONTEXT_FORMAT, OutputByteLimit, OutputEnforcement,
};

const SOURCE: &str = r#"{"outputProjectionJq":".orders","maxOutputBytes":100,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":["tag"],"loadingStrategy":"always"}"#;
const FORMAT: Option<&str> = Some(MCP_CONTEXT_FORMAT);
fn source() -> Node {
    json::parse(SOURCE).expect("caller-owned source DOM")
}
fn decode(node: &Node) -> Result<ContextManagement, ContextCodecError> {
    ContextManagement::decode_mcp(FORMAT, node, &mut Governor::new(Budget::default()))
}
fn set(node: &mut Node, field: &str, value: Value) {
    node.pointer_mut(&format!("/{field}"))
        .expect("known fixture field")
        .value = value;
}
fn append(node: &mut Node, key: &str, value: Value) {
    let Value::Object(members) = &mut node.value else {
        panic!("object fixture");
    };
    members.push(Member {
        key: key.into(),
        key_span: None,
        value: Node::new(value),
    });
}

#[test]
fn canonical_context_roundtrips_all_masks_strategies_tags_and_portable_quantities() {
    for (input, canonical) in [
        (".a.b", ".a.b"),
        (".[]", ".[]"),
        ("{a,b}", "{a, b}"),
        (".a[]?", ".a[]?"),
    ] {
        for (loading, wire_loading) in [
            (LoadingStrategy::Always, "always"),
            (LoadingStrategy::OnDemand, "on_demand"),
        ] {
            let context = ContextManagement::new(
                FieldMask::parse(input).expect("supported mask"),
                OutputByteLimit::new(u64::MAX).expect("positive domain"),
                u64::MAX,
                vec!["é\0\n".into(), "é\0\n".into()],
                loading,
            );
            let expected = format!(
                r#"{{"outputProjectionJq":"{canonical}","maxOutputBytes":18446744073709551615,"outputEnforcement":"exact-bytes","historyRetentionTurns":18446744073709551615,"intentTags":["é\u0000\n","é\u0000\n"],"loadingStrategy":"{wire_loading}"}}"#
            );
            let encoded = context
                .encode_json(FORMAT, &mut Governor::new(Budget::default()))
                .expect("canonical governed encoding");
            assert_eq!(encoded, expected);
            let dom = context
                .encode_mcp(FORMAT, &mut Governor::new(Budget::default()))
                .expect("governed DOM encoding");
            let before = dom.to_json();
            let decoded = decode(&dom).expect("complete fragment");
            assert_eq!(decoded, context);
            assert_eq!(decoded.output_enforcement(), OutputEnforcement::ExactBytes);
            assert_eq!(decoded.intent_tags().len(), 2);
            assert_eq!(dom.to_json(), before, "source DOM remains untouched");
        }
    }
    for (spelling, value) in [
        ("1e2", 100),
        ("100.0", 100),
        ("1.5e1", 15),
        ("100e-2", 1),
        ("184467440737095516150e-1", u64::MAX),
    ] {
        let mut node = source();
        set(
            &mut node,
            "maxOutputBytes",
            Value::Number(spelling.parse().expect("mathematical integer")),
        );
        assert_eq!(
            decode(&node)
                .expect("integral quantities are not lexically narrowed")
                .output_limit()
                .max_bytes(),
            value
        );
    }
}

#[test]
fn closed_context_fields_modes_versions_and_types_fail_with_valid_neighbors() {
    assert!(decode(&source()).is_ok());
    for format in [None, Some("purrassist-toolgraph-v1"), Some("unknown")] {
        let expected = if format.is_none() {
            ContextCodecError::MissingFormat
        } else {
            ContextCodecError::UnsupportedFormat
        };
        assert_eq!(
            ContextManagement::decode_mcp(format, &source(), &mut Governor::new(Budget::default())),
            Err(expected.clone())
        );
        let context = decode(&source()).expect("valid context");
        assert_eq!(
            context.encode_mcp(format, &mut Governor::new(Budget::default())),
            Err(expected)
        );
    }
    let mut node = source();
    append(
        &mut node,
        "maxOutputTokens",
        Value::Number("100".parse().expect("numeric fixture")),
    );
    assert_eq!(decode(&node), Err(ContextCodecError::LegacyTokenLimit));
    let mut node = source();
    append(&mut node, "extra", Value::Null);
    assert!(matches!(
        decode(&node),
        Err(ContextCodecError::UnknownField { index: 6 })
    ));
    let mut node = source();
    append(&mut node, "maxOutputBytes", Value::Null);
    assert_eq!(
        decode(&node),
        Err(ContextCodecError::DuplicateField("maxOutputBytes"))
    );
    let fields = [
        "outputProjectionJq",
        "maxOutputBytes",
        "outputEnforcement",
        "historyRetentionTurns",
        "intentTags",
        "loadingStrategy",
    ];
    for field in fields {
        let mut node = source();
        let Value::Object(members) = &mut node.value else {
            panic!("object fixture");
        };
        members.retain(|member| member.key != field);
        assert_eq!(decode(&node), Err(ContextCodecError::MissingField(field)));
        let mut node = source();
        set(&mut node, field, Value::Bool(false));
        assert_eq!(decode(&node), Err(ContextCodecError::WrongType(field)));
    }
    for (field, text, expected) in [
        (
            "outputEnforcement",
            "tokens",
            ContextCodecError::Enforcement,
        ),
        ("loadingStrategy", "sometimes", ContextCodecError::Loading),
    ] {
        let mut node = source();
        set(&mut node, field, Value::String(text.into()));
        assert_eq!(decode(&node), Err(expected));
    }
    let mut node = source();
    set(
        &mut node,
        "outputProjectionJq",
        Value::String(".a | length".into()),
    );
    assert!(matches!(
        decode(&node),
        Err(ContextCodecError::Projection(
            FieldMaskError::Unsupported { .. }
        ))
    ));
    let mut node = source();
    set(
        &mut node,
        "intentTags",
        Value::Array(vec![Node::new(Value::Null)]),
    );
    assert_eq!(decode(&node), Err(ContextCodecError::TagType { index: 0 }));
    assert_eq!(
        decode(&Node::new(Value::Null)),
        Err(ContextCodecError::WrongType("contextManagement"))
    );
}

#[test]
fn quantity_refusals_preserve_math_semantics_and_unsigned_boundaries() {
    for field in ["maxOutputBytes", "historyRetentionTurns"] {
        for spelling in [
            "-1",
            "1.5",
            "1e-1",
            "18446744073709551616",
            "1e999999999999999999999999999999999999999999999",
        ] {
            let mut node = source();
            set(
                &mut node,
                field,
                Value::Number(
                    spelling
                        .parse()
                        .expect("valid number with invalid quantity"),
                ),
            );
            assert_eq!(
                decode(&node),
                Err(ContextCodecError::Quantity(field)),
                "{field}={spelling}"
            );
        }
    }
    let mut node = source();
    set(
        &mut node,
        "maxOutputBytes",
        Value::Number("-0.0".parse().expect("zero number")),
    );
    assert_eq!(
        decode(&node),
        Err(ContextCodecError::Quantity("maxOutputBytes"))
    );
    let mut node = source();
    set(
        &mut node,
        "historyRetentionTurns",
        Value::Number(
            "-0e-999999999999999999999999999999999999999999999"
                .parse()
                .expect("zero with huge exponent"),
        ),
    );
    assert_eq!(
        decode(&node)
            .expect("mathematical zero history")
            .history_retention_turns(),
        0
    );
}

#[test]
fn codec_dom_operations_and_shared_mask_formatting_obey_all_admitted_boundaries() {
    let mut node = source();
    set(
        &mut node,
        "intentTags",
        Value::Array(vec![Node::new(Value::String("\0".repeat(511)))]),
    );
    let context = decode(&node).expect("source preparation is caller owned");
    for operation in 0..3 {
        let run = |governor: &mut Governor| -> Result<(), ContextCodecError> {
            match operation {
                0 => ContextManagement::decode_mcp(FORMAT, &node, governor).map(drop),
                1 => context.encode_mcp(FORMAT, governor).map(drop),
                _ => context.encode_json(FORMAT, governor).map(drop),
            }
        };
        let mut measured = Governor::new(Budget::default());
        run(&mut measured).expect("complete operation");
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
                    depth: 1,
                    ..Budget::default()
                },
                Resource::Depth,
            ),
        ] {
            assert_eq!(
                run(&mut Governor::new(budget)),
                Err(ContextCodecError::Resource(StopReason::BudgetExceeded(
                    resource
                )))
            );
        }
        run(&mut Governor::new(Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            depth: 2,
            ..Budget::default()
        }))
        .expect("exact valid resource neighbor");
        let token = CancellationToken::new();
        token.cancel();
        assert_eq!(
            run(&mut Governor::with_cancellation(Budget::default(), token)),
            Err(ContextCodecError::Resource(StopReason::Cancelled))
        );
    }
    let mask = FieldMask::parse("{alpha,beta}").expect("caller-owned mask");
    let mut measured = Governor::new(Budget::default());
    let rendered = mask
        .render_governed(&mut measured)
        .expect("canonical formatting");
    assert_eq!(rendered, mask.to_string());
    assert!(rendered.capacity() as u64 <= measured.scratch_meter().live());
    for (budget, resource) in [
        (
            Budget {
                work: measured.usage().work - 1,
                ..Budget::default()
            },
            Resource::Work,
        ),
        (
            Budget {
                scratch_bytes: measured.usage().peak_scratch_bytes - 1,
                ..Budget::default()
            },
            Resource::ScratchBytes,
        ),
    ] {
        assert_eq!(
            mask.render_governed(&mut Governor::new(budget)),
            Err(FieldMaskError::Resource(StopReason::BudgetExceeded(
                resource
            )))
        );
    }
}
