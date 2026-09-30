// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Property tests for the `Patch` algebra over deterministically generated
//! patches (a fixed-seed in-test LCG, not a fuzzing harness): `invert ∘
//! apply` is the identity, `compose` is associative, two same-source
//! disjoint patches commute (`p;q ≡ q';p'`), and `rebase` succeeds exactly
//! when the plan and the intervening change commute.
use purrassist_core::{
    Budget, Governor, Outcome,
    digest::Digest,
    patch::{Patch, TextEdit},
};

/// A small, deterministic linear congruential generator (Knuth's MMIX
/// constants). Not cryptographic; only used to generate reproducible test
/// input, matching this crate's other in-test-only generators.
struct Lcg(u64);
impl Lcg {
    const fn new(seed: u64) -> Self {
        Self(seed)
    }
    const fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            usize::try_from(self.next_u64() % u64::try_from(bound).unwrap_or(u64::MAX)).unwrap_or(0)
        }
    }
}

const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ.\n";

fn random_text(rng: &mut Lcg, len: usize) -> String {
    (0..len)
        .map(|_| ALPHABET[rng.below(ALPHABET.len())] as char)
        .collect()
}

/// Generate up to three non-overlapping, non-touching, sorted edits over a
/// buffer of `len` bytes (all ASCII, so every byte offset is a valid UTF-8
/// boundary).
fn random_edits(rng: &mut Lcg, len: usize) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut cursor = 0_usize;
    let count = rng.below(4);
    for _ in 0..count {
        if cursor > len {
            break;
        }
        let start = cursor + rng.below(len - cursor + 1);
        if start > len {
            break;
        }
        let span = rng.below((len - start).min(6) + 1);
        let end = start + span;
        let replacement_len = rng.below(6);
        let replacement = random_text(rng, replacement_len);
        edits.push(TextEdit {
            range: start..end,
            replacement,
        });
        cursor = end + 1 + rng.below(3);
    }
    edits
}

fn random_patch(rng: &mut Lcg, source: &str) -> Patch {
    Patch {
        source: Digest::of(source.as_bytes()),
        edits: random_edits(rng, source.len()),
    }
}

fn apply_ok(patch: &Patch, source: &str) -> String {
    let Outcome::Resolved(applied) = patch.apply(source, Budget::default()).outcome else {
        panic!("patch {patch:?} must resolve against {source:?}")
    };
    applied.text
}

#[test]
fn invert_then_apply_is_the_identity_over_generated_patches() {
    let mut rng = Lcg::new(0xC0FF_EE00_1234_5678);
    for _ in 0..1200 {
        let mut governor = Governor::new(Budget::default());
        let length = 20 + rng.below(80);
        let source = random_text(&mut rng, length);
        let patch = random_patch(&mut rng, &source);
        let applied_text = apply_ok(&patch, &source);
        let inverse = patch
            .invert(&source, &mut governor)
            .unwrap_or_else(|error| panic!("invert must succeed: {error}"));
        let restored = apply_ok(&inverse, &applied_text);
        assert_eq!(restored, source, "patch={patch:?}");
    }
}

#[test]
fn compose_is_associative_over_generated_three_step_chains() {
    let mut rng = Lcg::new(0x1357_9BDF_2468_ACE0);
    let mut checked = 0_usize;
    for _ in 0..500 {
        let mut governor = Governor::new(Budget::default());
        let length = 20 + rng.below(60);
        let t1 = random_text(&mut rng, length);
        let p = random_patch(&mut rng, &t1);
        let t2 = apply_ok(&p, &t1);
        let q = random_patch(&mut rng, &t2);
        let t3 = apply_ok(&q, &t2);
        let r = random_patch(&mut rng, &t3);
        let t4 = apply_ok(&r, &t3);

        let pq = p.compose(&q, &t1, &mut governor).expect("p;q composes");
        assert_eq!(apply_ok(&pq, &t1), t3);
        let qr = q.compose(&r, &t2, &mut governor).expect("q;r composes");
        assert_eq!(apply_ok(&qr, &t2), t4);

        let left = pq
            .compose(&r, &t1, &mut governor)
            .expect("(p;q);r composes");
        let right = p
            .compose(&qr, &t1, &mut governor)
            .expect("p;(q;r) composes");
        let left_text = apply_ok(&left, &t1);
        let right_text = apply_ok(&right, &t1);
        assert_eq!(left_text, t4);
        assert_eq!(right_text, t4);
        assert_eq!(left_text, right_text, "compose must be associative");
        checked += 3; // p, q, r all exercised as one composable chain
    }
    assert!(checked >= 1000, "generated {checked} patches");
}

#[test]
fn commuting_patches_satisfy_the_diamond_law_p_q_equiv_q_prime_p_prime() {
    let mut rng = Lcg::new(0xFEED_FACE_C0DE_BABE);
    let mut commuted = 0_usize;
    for _ in 0..2600 {
        let mut governor = Governor::new(Budget::default());
        let length = 30 + rng.below(90);
        let source = random_text(&mut rng, length);
        // One disjoint sorted edit list, split alternately, guarantees `p`
        // and `q` are each individually valid and mutually disjoint.
        let all = random_edits(&mut rng, source.len());
        if all.len() < 2 {
            continue;
        }
        let p = Patch {
            source: Digest::of(source.as_bytes()),
            edits: all.iter().step_by(2).cloned().collect(),
        };
        let q = Patch {
            source: Digest::of(source.as_bytes()),
            edits: all.iter().skip(1).step_by(2).cloned().collect(),
        };
        let Some((p_onto_q, q_onto_p)) = p
            .commute(&q, &source, &mut governor)
            .unwrap_or_else(|error| panic!("commute must succeed: {error}"))
        else {
            panic!("alternating split of one disjoint edit list must commute");
        };
        // p ; q'  (apply p, then q rebased onto p)
        let via_p = apply_ok(&p, &source);
        let p_then_q = apply_ok(&q_onto_p, &via_p);
        // q ; p'  (apply q, then p rebased onto q)
        let via_q = apply_ok(&q, &source);
        let q_then_p = apply_ok(&p_onto_q, &via_q);
        assert_eq!(p_then_q, q_then_p, "p;q' must equal q;p'");

        let rebased = p
            .rebase(&q, &source, &mut governor)
            .unwrap_or_else(|error| panic!("rebase of commuting patches must succeed: {error}"));
        assert_eq!(rebased.patch, p_onto_q);
        assert_eq!(rebased.transformation.rule, "patch.rebase.v1");
        commuted += 1;
    }
    assert!(commuted >= 1000, "only {commuted} pairs commuted");
}

#[test]
fn rebase_refuses_a_non_commuting_intervening_change_as_stale() {
    let mut governor = Governor::new(Budget::default());
    let base = "hot tea, cold coffee";
    let stale_plan = Patch {
        source: Digest::of(base.as_bytes()),
        edits: vec![TextEdit {
            range: 0..3,
            replacement: "warm".into(),
        }],
    };
    let overlapping = Patch {
        source: Digest::of(base.as_bytes()),
        edits: vec![TextEdit {
            range: 2..5,
            replacement: "T ".into(),
        }],
    };
    let error = stale_plan
        .rebase(&overlapping, base, &mut governor)
        .expect_err("overlapping ranges must not commute");
    assert!(error.to_string().contains("stale"), "{error}");
    assert_eq!(
        stale_plan
            .commute(&overlapping, base, &mut governor)
            .unwrap(),
        None
    );

    let disjoint = Patch {
        source: Digest::of(base.as_bytes()),
        edits: vec![TextEdit {
            range: 9..13,
            replacement: "iced".into(),
        }],
    };
    let rebased = stale_plan
        .rebase(&disjoint, base, &mut governor)
        .expect("disjoint ranges commute");
    let after_intervening = apply_ok(&disjoint, base);
    let final_text = apply_ok(&rebased.patch, &after_intervening);
    assert_eq!(final_text, "warm tea, iced coffee");
}
