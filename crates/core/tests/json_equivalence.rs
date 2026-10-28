// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Semantic equality, exact decimals and native refusal through actual consumers.
use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::Resource,
    json::{self, Node, Value},
    schema::Schema,
};

#[test]
fn ordered_duplicates_reordered_groups_and_exact_decimals_share_one_contract() {
    for (left, right, equal) in [
        (
            r#"{"a":1,"b":true,"a":2}"#,
            r#"{"b":true,"a":1.00,"a":2e0}"#,
            true,
        ),
        (
            r#"{"a":1,"b":true,"a":2}"#,
            r#"{"a":2,"b":true,"a":1}"#,
            false,
        ),
        (r#"{"a":1,"a":2}"#, r#"{"a":1,"b":2}"#, false),
        (
            r#"{"é":[1,2],"😀":"x"}"#,
            r#"{"😀":"x","é":[1.0,2e0]}"#,
            true,
        ),
        ("[1,2]", "[2,1]", false),
        (
            "-0e999999999999999999999999999999999999999999",
            "0.000",
            true,
        ),
        (
            "1e999999999999999999999999999999999999999999",
            "10e999999999999999999999999999999999999999998",
            true,
        ),
        (
            "10e-999999999999999999999999999999999999999999",
            "1e-999999999999999999999999999999999999999998",
            true,
        ),
        (
            "1e999999999999999999999999999999999999999999",
            "1e999999999999999999999999999999999999999998",
            false,
        ),
        ("9007199254740993", "9007199254740992", false),
        ("123000e-3", "123.000", true),
        ("-1.2500e2", "-125", true),
    ] {
        let a = json::parse(left).unwrap();
        let b = json::parse(right).unwrap();
        assert_eq!(a.equivalent(&b), equal, "ordinary {left} / {right}");
        let mut governor = Governor::new(Budget::default());
        assert_eq!(
            a.equivalent_governed(&b, &mut governor).unwrap(),
            equal,
            "governed {left} / {right}"
        );
        assert_eq!(governor.scratch_meter().live(), 0);
        assert_eq!(a.to_json(), left);
        assert_eq!(b.to_json(), right);
    }
    let source = format!("1{}e-512", "0".repeat(512));
    let long = json::parse(&source).unwrap();
    let one = json::parse("1").unwrap();
    let mut governor = Governor::new(Budget::default());
    assert!(long.equivalent(&one));
    assert!(long.equivalent_governed(&one, &mut governor).unwrap());
    assert_eq!(governor.scratch_meter().live(), 0);
    assert_eq!(long.to_json(), source);
}

#[test]
fn work_depth_storage_and_cancellation_refuse_without_mutating_or_leaking() {
    let left = json::parse(r#"{"x":[1.250e2,{"a":"é","a":true}]}"#).unwrap();
    let right = json::parse(r#"{"x":[125,{"a":"é","a":true}]}"#).unwrap();
    let mut measured = Governor::new(Budget::default());
    assert!(left.equivalent_governed(&right, &mut measured).unwrap());
    let usage = measured.usage();
    for (resource, budget) in [
        (
            Resource::Work,
            Budget {
                work: 0,
                ..Budget::default()
            },
        ),
        (
            Resource::Work,
            Budget {
                work: usage.work - 1,
                ..Budget::default()
            },
        ),
        (
            Resource::Depth,
            Budget {
                depth: 1,
                ..Budget::default()
            },
        ),
        (
            Resource::ScratchBytes,
            Budget {
                scratch_bytes: 0,
                ..Budget::default()
            },
        ),
        (
            Resource::ScratchBytes,
            Budget {
                scratch_bytes: usage.peak_scratch_bytes - 1,
                ..Budget::default()
            },
        ),
    ] {
        let mut governor = Governor::new(budget);
        let expected = StopReason::BudgetExceeded(resource);
        assert_eq!(
            left.equivalent_governed(&right, &mut governor),
            Err(expected.clone())
        );
        assert_eq!(governor.check(), Err(expected));
        assert_eq!(governor.scratch_meter().live(), 0);
    }
    // Admit the comparison stack, but leave no room for numeric buffers.
    // Calling ordinary Number::eq behind a governed frame would wrongly pass.
    let scalar = Node::new(Value::Null);
    let mut stack = Governor::new(Budget::default());
    assert!(scalar.equivalent_governed(&scalar, &mut stack).unwrap());
    let numeric = json::parse(&format!("1{}e-{}128", "0".repeat(128), "0".repeat(128))).unwrap();
    let one = json::parse("1").unwrap();
    let mut numeric_governor = Governor::new(Budget {
        scratch_bytes: stack.usage().peak_scratch_bytes,
        ..Budget::default()
    });
    assert_eq!(
        numeric.equivalent_governed(&one, &mut numeric_governor),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    assert_eq!(numeric_governor.scratch_meter().live(), 0);

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let mut governor = Governor::with_cancellation(Budget::default(), cancellation);
    assert_eq!(
        left.equivalent_governed(&right, &mut governor),
        Err(StopReason::Cancelled)
    );
    assert_eq!(governor.scratch_meter().live(), 0);
    assert_eq!(left.to_json(), r#"{"x":[1.250e2,{"a":"é","a":true}]}"#);
}

#[test]
fn iterative_comparison_admits_actual_value_depth_including_programmatic_doms() {
    let mut left = Node::new(Value::Null);
    let mut right = Node::new(Value::Null);
    for _ in 0..512 {
        left = Node::new(Value::Array(vec![left]));
        right = Node::new(Value::Array(vec![right]));
    }
    assert!(left.equivalent(&right));
    let mut governor = Governor::new(Budget {
        depth: 512,
        ..Budget::default()
    });
    assert!(left.equivalent_governed(&right, &mut governor).unwrap());
    assert_eq!(governor.scratch_meter().live(), 0);
    let mut governor = Governor::new(Budget {
        depth: 511,
        ..Budget::default()
    });
    assert_eq!(
        left.equivalent_governed(&right, &mut governor),
        Err(StopReason::BudgetExceeded(Resource::Depth))
    );
    assert_eq!(governor.scratch_meter().live(), 0);
}

#[test]
fn schema_const_enum_and_uniqueness_propagate_comparison_refusal() {
    let value = json::parse(r#"{"a":[1.00,2],"b":true}"#).unwrap();
    for source in [
        r#"{"const":{"b":true,"a":[1,2]}}"#,
        r#"{"enum":[{"b":true,"a":[1,2]}]}"#,
    ] {
        let schema = Schema::compile(source).unwrap();
        let mut governor = Governor::new(Budget::default());
        assert!(schema.validate_with(&value, &mut governor).unwrap().valid);
        // Root schema evaluation is at depth zero; only the const/enum
        // comparison traverses this instance's children.
        let mut governor = Governor::new(Budget {
            depth: 0,
            ..Budget::default()
        });
        assert_eq!(
            schema.validate_with(&value, &mut governor).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Depth)
        );
    }
    let schema = Schema::compile(r#"{"uniqueItems":true}"#).unwrap();
    let values = json::parse(r#"[{"a":1,"b":2},{"b":2,"a":1.0}]"#).unwrap();
    assert!(!schema.validate(&values).unwrap().valid);
    let mut governor = Governor::new(Budget {
        depth: 0,
        ..Budget::default()
    });
    assert_eq!(
        schema.validate_with(&values, &mut governor).unwrap_err(),
        StopReason::BudgetExceeded(Resource::Depth)
    );
}
