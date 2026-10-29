// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Preparation owners release independently of final public output ownership.
use super::{Attempt, generate};
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
        let mut governor = Governor::new(Budget {
            evidence_entries: 0,
            ..Budget::default()
        });
        let result = generate::run(&schema, schema.root, &mut governor).unwrap();
        let actual = match &result {
            Attempt::Witness(_) => "witness",
            Attempt::Unsatisfiable(_) => "unsatisfiable",
            Attempt::Exhausted => "exhausted",
        };
        assert_eq!(actual, expected);
        let usage = governor.usage();
        assert_eq!(
            usage.evidence_entries, 0,
            "internal branches publish no evidence"
        );
        assert!(usage.peak_scratch_bytes > 0);
        if actual == "witness" {
            assert!(governor.scratch_meter().live() > 0);
        }
        drop(result);
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
            drop(result);
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

/// Publication is the only place refutations consume evidence/output storage.
#[test]
fn final_refutation_publication_is_atomic_and_preserves_evidence() {
    use super::{Refutation, WitnessOutcome};
    use crate::contract::Evidence;

    let schema = Schema::compile("false").unwrap();
    let mut internal = Governor::new(Budget {
        evidence_entries: 0,
        ..Budget::default()
    });
    assert!(matches!(
        super::generate(&schema, schema.root, &mut internal),
        Ok(Attempt::Unsatisfiable("boolean false schema"))
    ));
    assert_eq!(internal.scratch_meter().live(), 0);
    assert_eq!(internal.usage().evidence_entries, 0);

    let expected = WitnessOutcome::Unsatisfiable(Refutation {
        reason: "boolean false schema".into(),
        evidence: vec![Evidence {
            rule: "schema.algebra.unsatisfiable-v1".into(),
            explanation: "boolean false schema".into(),
            span: None,
            destination: String::new(),
        }],
    });
    let mut complete = Governor::new(Budget::default());
    let actual = schema.generate_witness_governed(&mut complete).unwrap();
    assert_eq!(actual, expected);
    let usage = complete.usage();
    assert_eq!(usage.evidence_entries, 1);
    assert!(usage.work > internal.usage().work);
    assert!(usage.peak_scratch_bytes > internal.usage().peak_scratch_bytes);
    let retained = complete.scratch_meter().live();
    assert!(retained > 0);
    drop(actual);
    assert_eq!(complete.scratch_meter().live(), retained);

    let exact = Budget {
        work: usage.work,
        scratch_bytes: usage.peak_scratch_bytes,
        evidence_entries: 1,
        ..Budget::default()
    };
    let mut governor = Governor::new(exact);
    assert_eq!(
        schema.generate_witness_governed(&mut governor).unwrap(),
        expected
    );
    for (budget, resource) in [
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
                evidence_entries: 0,
                ..exact
            },
            Resource::EvidenceEntries,
        ),
    ] {
        let mut governor = Governor::new(budget);
        assert_eq!(
            schema.generate_witness_governed(&mut governor),
            Err(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(governor.check(), Err(StopReason::BudgetExceeded(resource)));
        assert_eq!(governor.scratch_meter().live(), 0, "{resource:?}");
        if resource != Resource::EvidenceEntries {
            assert!(governor.usage().work > internal.usage().work);
            assert!(
                governor.usage().total_scratch_charged > internal.usage().total_scratch_charged
            );
        }
    }
}

#[test]
fn candidate_trees_and_pattern_results_remain_owned_until_publication() {
    for source in [
        r#"{"const":{"n":1.00e2,"items":["雪",true]}}"#,
        r#"{"type":"string","pattern":"^a+$","minLength":2}"#,
        r#"{"type":"string","pattern":"^(a)\\1$"}"#,
        r#"{"type":"array","minItems":2,"not":{"uniqueItems":true}}"#,
        r#"{"type":"object","required":["x"],"properties":{"x":{"const":[1,2]}}}"#,
    ] {
        let schema = Schema::compile(source).unwrap();
        let mut governor = Governor::new(Budget::default());
        let attempt = super::generate(&schema, schema.root, &mut governor).unwrap();
        assert!(matches!(attempt, Attempt::Witness(_)), "{source}");
        assert!(governor.scratch_meter().live() > 0, "{source}");
        let usage = governor.usage();
        drop(attempt);
        assert_eq!(governor.scratch_meter().live(), 0, "{source}");
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
        ] {
            let mut governor = Governor::new(budget);
            assert!(
                matches!(super::generate(&schema, schema.root, &mut governor), Err(StopReason::BudgetExceeded(actual)) if actual == resource),
                "{source}"
            );
            assert_eq!(governor.scratch_meter().live(), 0, "{source}");
        }
        let mut governor = Governor::new(Budget::default());
        let published = schema.generate_witness_governed(&mut governor).unwrap();
        assert!(matches!(published, super::WitnessOutcome::Witness(_)));
        let retained = governor.scratch_meter().live();
        assert!(retained > 0);
        drop(published);
        assert_eq!(governor.scratch_meter().live(), retained);
    }
    // Pattern generation succeeds, but the second pattern assertion rejects
    // its candidate. Its string and pattern result are still temporary.
    let schema =
        Schema::compile(r#"{"type":"string","allOf":[{"pattern":"^a$"},{"pattern":"^b$"}]}"#)
            .unwrap();
    let mut governor = Governor::new(Budget::default());
    assert!(matches!(
        super::generate(&schema, schema.root, &mut governor).unwrap(),
        Attempt::Exhausted
    ));
    assert_eq!(governor.scratch_meter().live(), 0);
}
