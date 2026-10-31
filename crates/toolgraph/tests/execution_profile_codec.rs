// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Execution wire round trips, source-located refusals and original admission.

use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    json::{self, Node, Value},
};
use purrassist_toolgraph::codec::mcp::{
    EnvelopeProblem, ExecutionMode, ExecutionProfile, ExecutionQuantityProblem, ManifestEnvelope,
};

const PROFILE: &str = r#"{"graphBinding":"https://example.org/profile","mode":"aa-exec","profileName":"tool","hatName":"hat","egressAllowed":false,"addressSpaceBytes":268435456,"fileSizeBytes":1048576,"processCount":16,"cpuSeconds":5,"openFileCount":64}"#;

fn source(text: &str) -> Node {
    json::parse(text).expect("caller-prepared JSON")
}
fn governor() -> Governor {
    Governor::new(Budget::default())
}
fn bundle(profile: &str) -> Node {
    source(&format!(
        r#"{{"tools":[{{"name":"edit","description":"Edit","inputSchema":{{}},"outputSchema":{{}},"annotations":{{}},"_meta":{{"security":{{}},"executionProfile":{profile},"contextManagement":{{}}}}}}],"_meta":{{"security":{{"format":"purrassist-toolgraph-v2","mediaType":"text/turtle","prefixes":{{}},"sourceGraph":"","lossProfile":"purrassist-toolgraph-v1"}}}}"#
    ))
}

#[test]
fn complete_profiles_roundtrip_both_modes_and_preserve_exact_wire_quantities() {
    for (mode, expected_mode) in [
        ("aa-exec", ExecutionMode::AaExec),
        ("change_hat", ExecutionMode::ChangeHat),
    ] {
        for (number, expected) in [
            ("16", 16),
            ("1.6e1", 16),
            ("160e-1", 16),
            ("18446744073709551615", u64::MAX),
        ] {
            let input = PROFILE
                .replace("aa-exec", mode)
                .replace("\"processCount\":16", &format!("\"processCount\":{number}"));
            let node = source(&input);
            let decoded =
                ExecutionProfile::read(&node, &mut governor()).expect("positive wire domain");
            assert_eq!(decoded.mode(), expected_mode);
            assert_eq!(decoded.limits().processes, expected);
            assert!(core::ptr::eq(
                decoded.original(),
                core::ptr::from_ref(&node)
            ));
            let encoded = decoded
                .encode_json(&mut governor())
                .expect("governed encoder");
            assert_eq!(encoded, input);
            let mut reordered = node.clone();
            let Value::Object(members) = &mut reordered.value else {
                panic!("object fixture")
            };
            members.reverse();
            assert_eq!(
                ExecutionProfile::read(&reordered, &mut governor())
                    .expect("order independent decoding")
                    .encode_json(&mut governor())
                    .expect("fixed output order"),
                encoded
            );
            let roundtrip = source(&encoded);
            assert_eq!(
                ExecutionProfile::read(&roundtrip, &mut governor())
                    .expect("roundtrip")
                    .limits(),
                decoded.limits()
            );
            if expected == u64::MAX {
                assert!(
                    decoded.limits().render("fixture", &mut governor()).is_err(),
                    "renderer limits must not narrow wire decoding"
                );
            }
        }
    }
    // Strings belong to the declared wire domain; policy-word restrictions are
    // separate. Preserve empty names, quotes, Unicode and decoded control bytes.
    let input = PROFILE
        .replace("\"profileName\":\"tool\"", r#""profileName":"""#)
        .replace("\"hatName\":\"hat\"", r#""hatName":"é\"\u0000\n""#);
    let node = source(&input);
    let decoded = ExecutionProfile::read(&node, &mut governor()).expect("verbatim wire strings");
    assert_eq!(decoded.profile_name(), "");
    assert_eq!(decoded.hat_name(), "é\"\0\n");
    assert_eq!(
        decoded
            .encode_json(&mut governor())
            .expect("shared escaping"),
        input
    );
}

#[test]
fn envelope_checks_execution_fields_and_reports_original_occurrences() {
    let valid = bundle(PROFILE);
    let envelope = ManifestEnvelope::read(&valid, &mut governor()).expect("complete envelope");
    let tool = envelope
        .tool(0, &mut governor())
        .expect("tool read")
        .expect("one tool");
    assert!(core::ptr::eq(
        tool.execution().original(),
        tool.execution_profile()
    ));
    assert_eq!(
        tool.execution()
            .encode_json(&mut governor())
            .expect("profile output"),
        PROFILE
    );
    for (input, expected, field) in [
        (
            PROFILE.replace(",\"cpuSeconds\":5", ""),
            EnvelopeProblem::MissingField {
                object: "executionProfile",
                field: "cpuSeconds",
            },
            None,
        ),
        (
            PROFILE.replace("\"cpuSeconds\":5", "\"cpuSeconds\":true"),
            EnvelopeProblem::WrongType("cpuSeconds"),
            Some("cpuSeconds"),
        ),
        (
            PROFILE.replace("\"mode\":\"aa-exec\"", "\"mode\":\"automatic\""),
            EnvelopeProblem::UnsupportedExecutionMode,
            Some("mode"),
        ),
        (
            PROFILE.replace("\"egressAllowed\":false", "\"egressAllowed\":null"),
            EnvelopeProblem::WrongType("egressAllowed"),
            Some("egressAllowed"),
        ),
    ] {
        let node = bundle(&input);
        let original = node
            .pointer("/tools/0/_meta/executionProfile")
            .expect("original profile");
        let failure = ManifestEnvelope::read(&node, &mut governor()).expect_err("invalid profile");
        assert_eq!(failure.problem, expected);
        assert_eq!(failure.tool, Some(0));
        assert_eq!(
            failure.span,
            field
                .map_or(original, |name| original.get(name).expect("field"))
                .span
        );
        assert_eq!(
            node.to_json(),
            bundle(&input).to_json(),
            "input is unchanged"
        );
    }
    for (name, expected) in [
        (
            "mode",
            EnvelopeProblem::DuplicateField {
                object: "executionProfile",
                field: "mode",
            },
        ),
        (
            "unexpected",
            EnvelopeProblem::UnknownField {
                object: "executionProfile",
                index: 10,
            },
        ),
    ] {
        let input = format!(
            "{},\"{name}\":\"aa-exec\"}}",
            PROFILE.strip_suffix('}').expect("object")
        );
        let node = source(&input);
        let Value::Object(members) = &node.value else {
            panic!("object fixture")
        };
        let failure =
            ExecutionProfile::read(&node, &mut governor()).expect_err("duplicate or unknown");
        assert_eq!(failure.problem, expected);
        assert_eq!(
            failure.span,
            members.last().expect("offending member").key_span
        );
    }
    assert_quantity_refusals_are_located();
}

fn assert_quantity_refusals_are_located() {
    for (spelling, problem) in [
        ("0", ExecutionQuantityProblem::NonPositive),
        ("-0.0", ExecutionQuantityProblem::NonPositive),
        ("-1", ExecutionQuantityProblem::NonPositive),
        ("1.5", ExecutionQuantityProblem::Fractional),
        ("18446744073709551616", ExecutionQuantityProblem::OutOfRange),
        (
            "1e999999999999999999999999",
            ExecutionQuantityProblem::OutOfRange,
        ),
    ] {
        let node = source(&PROFILE.replace(
            "\"processCount\":16",
            &format!("\"processCount\":{spelling}"),
        ));
        let failure =
            ExecutionProfile::read(&node, &mut governor()).expect_err("invalid exact quantity");
        assert_eq!(
            failure.problem,
            EnvelopeProblem::ExecutionQuantity {
                field: "processCount",
                problem
            }
        );
        assert_eq!(
            failure.span,
            node.get("processCount").expect("quantity").span
        );
    }
}

#[test]
fn profile_decoding_and_encoding_preserve_refusals_and_release_temporary_output() {
    let node = source(PROFILE);
    let decoded = ExecutionProfile::read(&node, &mut governor()).expect("fixture");
    let mut measured = governor();
    decoded
        .encode_json(&mut measured)
        .expect("complete encoding");
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
            depth: 0,
            ..Budget::default()
        },
    ] {
        let mut limited = Governor::new(budget);
        let failure = decoded
            .encode_json(&mut limited)
            .expect_err("atomic refusal");
        assert!(matches!(
            failure.problem,
            EnvelopeProblem::Resource(StopReason::BudgetExceeded(_))
        ));
        assert_eq!(
            limited.scratch_meter().live(),
            0,
            "temporary output released"
        );
        assert_eq!(failure.span, node.span);
    }
    decoded
        .encode_json(&mut Governor::new(Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            depth: 1,
            ..Budget::default()
        }))
        .expect("exact admitted resource neighbor");
    let mut limited = Governor::new(Budget {
        work: 0,
        ..Budget::default()
    });
    assert_eq!(
        ExecutionProfile::read(&node, &mut limited)
            .expect_err("no work")
            .problem,
        EnvelopeProblem::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    let token = CancellationToken::new();
    token.cancel();
    let mut cancelled = Governor::with_cancellation(Budget::default(), token);
    assert_eq!(
        ExecutionProfile::read(&node, &mut cancelled)
            .expect_err("cancelled decode")
            .problem,
        EnvelopeProblem::Resource(StopReason::Cancelled)
    );
    assert_eq!(
        decoded
            .encode_json(&mut cancelled)
            .expect_err("cancelled encode")
            .problem,
        EnvelopeProblem::Resource(StopReason::Cancelled)
    );
    assert_eq!(node.to_json(), PROFILE);
}
