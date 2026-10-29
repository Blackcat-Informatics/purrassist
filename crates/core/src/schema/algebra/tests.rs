// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Preparation owners release independently of final public output ownership.
use super::generate::{self, Attempt};
use crate::{Budget, Governor, StopReason, contract::Resource, schema::Schema};

#[test]
fn branch_preparation_releases_on_each_outcome_and_refusal() {
    for (source, expected) in [
        (r#"{"anyOf":[false,{"const":7}]}"#, "witness"),
        (
            r#"{"allOf":[{"type":"string"},{"type":"number"}]}"#,
            "unsatisfiable",
        ),
        (r##"{"$ref":"#"}"##, "exhausted"),
    ] {
        let schema = Schema::compile(source).unwrap();
        let mut governor = Governor::new(Budget::default());
        let result = generate::run(&schema, schema.root, &mut governor).unwrap();
        let actual = match result {
            Attempt::Witness(_) => "witness",
            Attempt::Unsatisfiable(_) => "unsatisfiable",
            Attempt::Exhausted => "exhausted",
        };
        assert_eq!(actual, expected);
        let usage = governor.usage();
        assert!(usage.peak_scratch_bytes > 0);
        assert_eq!(governor.scratch_meter().live(), 0);
        for (work, scratch_bytes, reason) in [
            (usage.work, usage.peak_scratch_bytes, None),
            (
                usage.work - 1,
                usage.peak_scratch_bytes,
                Some(Resource::Work),
            ),
            (
                usage.work,
                usage.peak_scratch_bytes - 1,
                Some(Resource::ScratchBytes),
            ),
        ] {
            let mut governor = Governor::new(Budget {
                work,
                scratch_bytes,
                ..Budget::default()
            });
            let result = generate::run(&schema, schema.root, &mut governor);
            if let Some(resource) = reason {
                assert!(
                    matches!(result,Err(StopReason::BudgetExceeded(actual)) if actual==resource)
                );
            } else {
                assert!(result.is_ok());
            }
            assert_eq!(governor.scratch_meter().live(), 0, "{source}");
        }
    }
    // The schema can be compiled under a larger budget than this caller permits
    // for positive or negated canonical preparation.
    let mut nested = r#"{"const":7}"#.to_owned();
    for _ in 0..8 {
        nested = format!(r#"{{"allOf":[{nested}]}}"#);
    }
    for source in [nested.clone(), format!(r#"{{"not":{nested}}}"#)] {
        let schema = Schema::compile(&source).unwrap();
        let mut governor = Governor::new(Budget {
            depth: 4,
            ..Budget::default()
        });
        assert!(matches!(
            generate::run(&schema, schema.root, &mut governor),
            Err(StopReason::BudgetExceeded(Resource::Depth))
        ));
        assert_eq!(governor.scratch_meter().live(), 0);
    }
}
