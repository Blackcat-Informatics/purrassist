// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact authority, finite import identities and atomic governed ownership.
use purrassist_core::{Budget, Governor, StopReason, contract::Resource, digest::Digest};
use purrassist_toolgraph::{
    PolicyContractInput, PolicyGrant, PolicyScope, PolicySource, PolicyTrustCause,
    PreparedPolicyContract, TrustedPolicyRegistry,
};

const SOURCES: [PolicySource<'static>; 3] = [
    PolicySource {
        location: "r",
        external_base: None,
        issuer: "i",
        text: "root",
        imports: &["c", "d", "c"],
    },
    PolicySource {
        location: "c",
        external_base: Some(""),
        issuer: "j",
        text: "child",
        imports: &["r"],
    },
    PolicySource {
        location: "d",
        external_base: Some("https://example.test/d"),
        issuer: "i",
        text: "leaf",
        imports: &[],
    },
];
fn scope() -> PolicyScope<'static> {
    PolicyScope {
        tool: "tool",
        vocabulary: [7; 32],
        session_schema: *Digest::of(b"schema").as_bytes(),
    }
}
const fn input<'a>(documents: &'a [PolicySource<'a>]) -> PolicyContractInput<'a> {
    PolicyContractInput {
        root: "r",
        documents,
    }
}
fn grants<'a>(prepared: &PreparedPolicyContract<'a>) -> Vec<PolicyGrant<'a>> {
    prepared
        .documents()
        .iter()
        .map(|document| PolicyGrant {
            document: *document.document.as_bytes(),
            contract: *document.contract.as_bytes(),
            scope: scope(),
            issuer: document.source.issuer,
            provenance: "host config",
        })
        .collect()
}

#[test]
fn exact_authority_covers_every_grant_field_and_all_transitive_documents() {
    let mut g = Governor::new(Budget::default());
    let prepared = PreparedPolicyContract::prepare(input(&SOURCES), &mut g).unwrap();
    let approved = grants(&prepared);
    let registry = TrustedPolicyRegistry::prepare(&approved, &mut g).unwrap();
    let matched = prepared.bind(&registry, scope(), &mut g).unwrap();
    assert_eq!(matched.input(), input(&SOURCES));
    for (index, document) in matched.documents().iter().enumerate() {
        let grant = matched.grant(index).unwrap();
        assert_eq!(
            (grant.document, grant.contract, grant.issuer),
            (
                *document.document.as_bytes(),
                *document.contract.as_bytes(),
                document.source.issuer
            )
        );
    }
    let original_registry = matched.registry_digest();
    drop(matched);
    for mutation in 0..7 {
        let mut changed = approved.clone();
        match mutation {
            0 => changed[0].document = *Digest::of(b"different document").as_bytes(),
            1 => changed[0].contract = *Digest::of(b"different closure").as_bytes(),
            2 => changed[0].scope.tool = "different tool",
            3 => changed[0].scope.vocabulary = [8; 32],
            4 => changed[0].issuer = "different issuer",
            5 => changed[0].scope.session_schema = *Digest::of(b"different schema").as_bytes(),
            6 => changed[0].provenance = "different audited provenance",
            _ => unreachable!(),
        }
        let changed_registry = TrustedPolicyRegistry::prepare(&changed, &mut g).unwrap();
        assert_ne!(changed_registry.digest(), original_registry);
        let result = prepared.bind(&changed_registry, scope(), &mut g);
        if mutation == 6 {
            assert!(result.is_ok());
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.cause, PolicyTrustCause::UntrustedDocument(0));
            assert_eq!(error.context.request, Some(input(&SOURCES)));
            assert_eq!(error.context.grants, changed);
            assert_eq!(error.context.registry, Some(changed_registry.digest()));
        }
    }
    let mut selected = scope();
    selected.tool = "another";
    assert!(matches!(
        prepared
            .bind(&registry, selected, &mut g)
            .unwrap_err()
            .cause,
        PolicyTrustCause::UntrustedDocument(0)
    ));
    let mut changed_scope = scope();
    changed_scope.vocabulary = [9; 32];
    assert!(prepared.bind(&registry, changed_scope, &mut g).is_err());
    changed_scope = scope();
    changed_scope.session_schema = *Digest::of(b"other session").as_bytes();
    assert!(prepared.bind(&registry, changed_scope, &mut g).is_err());
    let incomplete = TrustedPolicyRegistry::prepare(&approved[..2], &mut g).unwrap();
    assert_eq!(
        prepared
            .bind(&incomplete, scope(), &mut g)
            .unwrap_err()
            .cause,
        PolicyTrustCause::UntrustedDocument(2)
    );
    let mut duplicate = approved.clone();
    duplicate.push(PolicyGrant {
        provenance: "different provenance",
        ..approved[0]
    });
    let error = TrustedPolicyRegistry::prepare(&duplicate, &mut g).unwrap_err();
    assert!(matches!(
        error.cause,
        PolicyTrustCause::DuplicateGrant(0, 3)
    ));
    assert_eq!(error.context.grants, duplicate);
    // Length framing prevents a tool/issuer concatenation collision.
    let mut left = approved[0];
    left.scope.tool = "ab";
    left.issuer = "c";
    let mut right = approved[0];
    right.scope.tool = "a";
    right.issuer = "bc";
    let left_grants = [left];
    let right_grants = [right];
    assert_ne!(
        TrustedPolicyRegistry::prepare(&left_grants, &mut g)
            .unwrap()
            .digest(),
        TrustedPolicyRegistry::prepare(&right_grants, &mut g)
            .unwrap()
            .digest()
    );
}

#[test]
fn rooted_transcripts_preserve_finite_cycles_order_bases_and_canonical_tables() {
    let mut g = Governor::new(Budget::default());
    let original = PreparedPolicyContract::prepare(input(&SOURCES), &mut g).unwrap();
    // Independently constructed with Python hashlib/struct from the documented
    // transcript: root/cycle roots encode 293 bytes, standalone leaf 129 bytes.
    let expected = [
        "sha256:5a280420bf79c88df3290e6db28f932a555b03a2a114d183553ee44dfda764b7",
        "sha256:9e7ac29aca9ca4f8f9ea4ced370337dfca915981f8c7909fa1331f236d2909d0",
        "sha256:692bb43b23861de04bcc51d9535d538f000bfdbf79824fbd1599bb338caa3e61",
    ];
    for (document, expected) in original.documents().iter().zip(expected) {
        assert_eq!(document.contract.to_string(), expected);
        assert_eq!(
            document.document,
            Digest::of(document.source.text.as_bytes())
        );
    }
    let approved = grants(&original);
    let registry = TrustedPolicyRegistry::prepare(&approved, &mut g).unwrap();
    let reordered = [SOURCES[2], SOURCES[0], SOURCES[1]];
    let reordered = PreparedPolicyContract::prepare(input(&reordered), &mut g).unwrap();
    assert_eq!(reordered.root().contract, original.root().contract);
    assert!(reordered.bind(&registry, scope(), &mut g).is_ok());
    let reversed: Vec<_> = approved.iter().rev().copied().collect();
    let reversed = TrustedPolicyRegistry::prepare(&reversed, &mut g).unwrap();
    assert_eq!(registry.digest(), reversed.digest());
    assert!(original.bind(&reversed, scope(), &mut g).is_ok());
    for mutation in 0..6 {
        let mut changed = SOURCES;
        match mutation {
            0 => changed[0].external_base = Some(""),
            1 => changed[1].external_base = Some("https://example.test/new"),
            2 => changed[1].text = "child ",
            3 => changed[0].imports = &["d", "c", "c"],
            4 => changed[0].imports = &["c", "d"],
            5 => changed[1].issuer = "another issuer",
            _ => unreachable!(),
        }
        let changed = PreparedPolicyContract::prepare(input(&changed), &mut g).unwrap();
        assert_ne!(changed.root().contract, original.root().contract);
        assert!(changed.bind(&registry, scope(), &mut g).is_err());
    }
    let mut missing = SOURCES;
    missing[0].imports = &["absent"];
    assert_eq!(
        PreparedPolicyContract::prepare(input(&missing), &mut g)
            .unwrap_err()
            .cause,
        PolicyTrustCause::MissingImport(0, 0)
    );
    let duplicate = [SOURCES[0], SOURCES[1], SOURCES[2], SOURCES[1]];
    assert_eq!(
        PreparedPolicyContract::prepare(input(&duplicate), &mut g)
            .unwrap_err()
            .cause,
        PolicyTrustCause::DuplicateDocument(1, 3)
    );
    let mut extra = SOURCES;
    extra[0].imports = &["c"];
    assert_eq!(
        PreparedPolicyContract::prepare(input(&extra), &mut g)
            .unwrap_err()
            .cause,
        PolicyTrustCause::UnreachedDocument(2)
    );
    let absent = PolicyContractInput {
        root: "absent",
        documents: &SOURCES,
    };
    assert_eq!(
        PreparedPolicyContract::prepare(absent, &mut g)
            .unwrap_err()
            .cause,
        PolicyTrustCause::MissingRoot
    );
    let mut renamed = SOURCES;
    renamed[2].location = "renamed";
    renamed[0].imports = &["c", "renamed", "c"];
    let renamed = PreparedPolicyContract::prepare(input(&renamed), &mut g).unwrap();
    assert_ne!(renamed.root().contract, original.root().contract);
}

#[test]
fn native_stops_and_late_refusals_release_all_new_owners_without_partial_matches() {
    let mut g = Governor::new(Budget::default());
    let prepared = PreparedPolicyContract::prepare(input(&SOURCES), &mut g).unwrap();
    let prepare_work = g.usage().work;
    let approved = grants(&prepared);
    let before_registry = g.usage().work;
    let registry = TrustedPolicyRegistry::prepare(&approved, &mut g).unwrap();
    let registry_work = g.usage().work - before_registry;
    let before_match = g.usage().work;
    let baseline = g.scratch_meter().live();
    let matched = prepared.bind(&registry, scope(), &mut g).unwrap();
    let match_work = g.usage().work - before_match;
    assert!(g.scratch_meter().live() > baseline);
    drop(matched);
    assert_eq!(g.scratch_meter().live(), baseline);
    drop(registry);
    drop(prepared);
    assert_eq!(g.scratch_meter().live(), 0);
    assert_source_admission_stops();
    let mut limited = Governor::new(Budget {
        work: prepare_work - 1,
        ..Budget::default()
    });
    assert_eq!(
        PreparedPolicyContract::prepare(input(&SOURCES), &mut limited)
            .unwrap_err()
            .cause,
        PolicyTrustCause::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(limited.scratch_meter().live(), 0);
    let mut limited = Governor::new(Budget {
        work: registry_work - 1,
        ..Budget::default()
    });
    assert_eq!(
        TrustedPolicyRegistry::prepare(&approved, &mut limited)
            .unwrap_err()
            .cause,
        PolicyTrustCause::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(limited.scratch_meter().live(), 0);
    let mut limited = Governor::new(Budget {
        work: prepare_work + registry_work + match_work - 1,
        ..Budget::default()
    });
    let prepared = PreparedPolicyContract::prepare(input(&SOURCES), &mut limited).unwrap();
    let registry = TrustedPolicyRegistry::prepare(&approved, &mut limited).unwrap();
    let live = limited.scratch_meter().live();
    assert_eq!(
        prepared
            .bind(&registry, scope(), &mut limited)
            .unwrap_err()
            .cause,
        PolicyTrustCause::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(limited.scratch_meter().live(), live);
    assert_registry_and_match_stops(&prepared, &registry, &approved);
    let mut fresh = Governor::new(Budget::default());
    let incomplete = TrustedPolicyRegistry::prepare(&approved[..2], &mut fresh).unwrap();
    let live = fresh.scratch_meter().live();
    assert_eq!(
        prepared
            .bind(&incomplete, scope(), &mut fresh)
            .unwrap_err()
            .cause,
        PolicyTrustCause::UntrustedDocument(2)
    );
    assert_eq!(fresh.scratch_meter().live(), live);
}

fn assert_source_admission_stops() {
    for resource in [Resource::InputBytes, Resource::Work, Resource::ScratchBytes] {
        let mut budget = Budget::default();
        match resource {
            Resource::InputBytes => budget.input_bytes = 0,
            Resource::Work => budget.work = 0,
            Resource::ScratchBytes => budget.scratch_bytes = 0,
            _ => unreachable!(),
        }
        let mut limited = Governor::new(budget);
        let error = PreparedPolicyContract::prepare(input(&SOURCES), &mut limited).unwrap_err();
        assert_eq!(
            error.cause,
            PolicyTrustCause::Resource(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(limited.check(), Err(StopReason::BudgetExceeded(resource)));
        assert_eq!(error.context.request, Some(input(&SOURCES)));
        assert_eq!(error.usage, limited.usage());
        assert_eq!(limited.scratch_meter().live(), 0);
    }
    let empty_references = [""; 1024];
    let empty_sources = [PolicySource {
        location: "r",
        external_base: None,
        issuer: "i",
        text: "",
        imports: &empty_references,
    }];
    let mut limited = Governor::new(Budget {
        work: 100,
        ..Budget::default()
    });
    assert_eq!(
        PreparedPolicyContract::prepare(input(&empty_sources), &mut limited)
            .unwrap_err()
            .cause,
        PolicyTrustCause::Resource(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(limited.scratch_meter().live(), 0);
}

fn assert_registry_and_match_stops(
    prepared: &PreparedPolicyContract<'_>,
    registry: &TrustedPolicyRegistry<'_>,
    approved: &[PolicyGrant<'_>],
) {
    for resource in [Resource::InputBytes, Resource::Work, Resource::ScratchBytes] {
        let mut budget = Budget::default();
        match resource {
            Resource::InputBytes => budget.input_bytes = 0,
            Resource::Work => budget.work = 0,
            Resource::ScratchBytes => budget.scratch_bytes = 0,
            _ => unreachable!(),
        }
        let mut registry_limited = Governor::new(budget);
        assert_eq!(
            TrustedPolicyRegistry::prepare(approved, &mut registry_limited)
                .unwrap_err()
                .cause,
            PolicyTrustCause::Resource(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(registry_limited.scratch_meter().live(), 0);
        let mut bind_limited = Governor::new(budget);
        let error = prepared
            .bind(registry, scope(), &mut bind_limited)
            .unwrap_err();
        assert_eq!(
            error.cause,
            PolicyTrustCause::Resource(StopReason::BudgetExceeded(resource))
        );
        assert_eq!(error.context.registry, Some(registry.digest()));
        assert_eq!(bind_limited.scratch_meter().live(), 0);
    }
    let mut canceled = Governor::new(Budget::default());
    canceled.cancel();
    let error = prepared.bind(registry, scope(), &mut canceled).unwrap_err();
    assert_eq!(
        error.cause,
        PolicyTrustCause::Resource(StopReason::Cancelled)
    );
    assert_eq!(canceled.scratch_meter().live(), 0);
    assert_eq!(
        TrustedPolicyRegistry::prepare(approved, &mut canceled)
            .unwrap_err()
            .cause,
        PolicyTrustCause::Resource(StopReason::Cancelled)
    );
    assert_eq!(
        PreparedPolicyContract::prepare(input(&SOURCES), &mut canceled)
            .unwrap_err()
            .cause,
        PolicyTrustCause::Resource(StopReason::Cancelled)
    );
}
