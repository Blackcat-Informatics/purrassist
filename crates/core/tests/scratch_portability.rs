// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Regression test: scratch charges for a fixed workload are exact,
//! hard-coded byte counts that do not depend on the compiling target's
//! pointer width.
//!
//! `ScratchVec`/`ScratchMap`/[`purrassist_core::json::Node::estimated_bytes`]
//! charge every stored element by [`purrassist_core::ScratchCharge::CHARGE`]
//! (see `crates/core/src/contract.rs`), a literal fixed at that type's
//! *64-bit* layout size, never by `core::mem::size_of::<T>()`. Every
//! assertion below hard-codes the expected byte count rather than deriving
//! it from `size_of` at test time: if a future change reverted any charge
//! site back to `size_of`, this exact test, compiled for a 32-bit target
//! (wasm32-wasip1/wasm32-unknown-unknown), would start failing even though
//! it kept passing here on 64-bit native, because `size_of::<T>()` shrinks
//! there while these literals cannot. That divergence — not a difference in
//! *this* run's pass/fail status — is the regression this test exists to
//! catch; `scripts/check-wasm.py` (and the direct native/WASI comparison in
//! the acceptance evidence for this change) is what actually runs the other
//! half of that comparison.
use purrassist_core::json::{Member, Node, Value};
use purrassist_core::provenance::{NodeId, ProvenanceNode};
use purrassist_core::{Budget, Governor, ScratchCharge, ScratchMap, ScratchVec};

/// Every element type this crate ever stores in a `ScratchVec`/`ScratchMap`
/// (including as a tuple component) charges exactly its hand-measured
/// 64-bit `size_of::<T>()`, spelled out as a literal, not recomputed.
#[test]
fn scratch_charge_constants_are_fixed_64_bit_literals() {
    assert_eq!(<() as ScratchCharge>::CHARGE, 0);
    assert_eq!(<bool as ScratchCharge>::CHARGE, 1);
    assert_eq!(<u32 as ScratchCharge>::CHARGE, 4);
    assert_eq!(<u64 as ScratchCharge>::CHARGE, 8);
    assert_eq!(<usize as ScratchCharge>::CHARGE, 8);
    assert_eq!(<&'static str as ScratchCharge>::CHARGE, 16);
    assert_eq!(<String as ScratchCharge>::CHARGE, 24);
    assert_eq!(<Node as ScratchCharge>::CHARGE, 56);
    assert_eq!(<Member as ScratchCharge>::CHARGE, 104);
    assert_eq!(<NodeId as ScratchCharge>::CHARGE, 4);
    assert_eq!(<ProvenanceNode as ScratchCharge>::CHARGE, 160);
    // Tuple charges round each component up to an 8-byte word before
    // summing (see `round_up_to_word` in `crates/core/src/contract.rs`),
    // which is itself a fixed, target-independent formula over fixed
    // literals, never `size_of::<(K, V)>()`.
    assert_eq!(<(String, NodeId) as ScratchCharge>::CHARGE, 32);
    assert_eq!(<(u32, u64) as ScratchCharge>::CHARGE, 16);
    assert_eq!(<(String, ()) as ScratchCharge>::CHARGE, 24);
}

/// `ScratchVec<T>` growth charges `capacity_delta * T::CHARGE`, an exact,
/// portable byte count for a fixed sequence of pushes.
#[test]
fn scratch_vec_growth_charges_an_exact_portable_byte_count() {
    let governor = Governor::new(Budget::default());
    let mut values: ScratchVec<Node> = ScratchVec::new(&governor);
    for _ in 0..5 {
        values.push(Node::new(Value::Null)).expect("within budget");
    }
    // `grown_capacity` doubles 0 -> 4 -> 8 for 5 pushes: two charges of
    // `4 * Node::CHARGE` bytes each, computed only from the portable
    // `Node::CHARGE` literal (56), never from `size_of::<Node>()`.
    let expected = 4 * u64::from(u32::try_from(<Node as ScratchCharge>::CHARGE).unwrap())
        + 4 * u64::from(u32::try_from(<Node as ScratchCharge>::CHARGE).unwrap());
    assert_eq!(expected, 448);
    assert_eq!(governor.usage().peak_scratch_bytes, expected);
    assert_eq!(governor.usage().total_scratch_charged, expected);
}

/// `ScratchMap<K, V>` growth charges `capacity_delta * (K, V)::CHARGE`, an
/// exact, portable byte count, for a fixed sequence of inserts.
#[test]
fn scratch_map_growth_charges_an_exact_portable_byte_count() {
    let governor = Governor::new(Budget::default());
    let mut map: ScratchMap<u32, u64> = ScratchMap::new(&governor);
    for key in 0..5u32 {
        map.insert(key, u64::from(key)).expect("within budget");
    }
    // Same 0 -> 4 -> 8 growth as the `ScratchVec` case above, charged at
    // `(u32, u64)::CHARGE` = 16 bytes/entry (4 rounded up to 8, plus 8).
    assert_eq!(<(u32, u64) as ScratchCharge>::CHARGE, 16);
    let expected = 4 * 16 + 4 * 16;
    assert_eq!(governor.usage().peak_scratch_bytes, expected);
    assert_eq!(governor.usage().total_scratch_charged, expected);
}

/// [`Node::estimated_bytes`] is computed entirely from [`ScratchCharge`]
/// literals plus the exact lengths of owned string/number data, so its
/// result is one exact, portable number for a fixed value, hard-coded here.
#[test]
fn node_estimated_bytes_is_an_exact_portable_byte_count() {
    let leaf = Node::new(Value::String("hello".into()));
    assert_eq!(leaf.estimated_bytes(), 56 + 5); // Node::CHARGE + "hello".len()

    let array = Node::new(Value::Array(vec![
        Node::new(Value::Null),
        Node::new(Value::Null),
        Node::new(Value::Null),
    ]));
    // self (56) + 3 element slots (3 * 56) + each element's own estimate (3 * 56).
    assert_eq!(array.estimated_bytes(), 56 + 3 * 56 + 3 * 56);

    let object = Node::new(Value::Object(vec![Member {
        key: "k".into(),
        key_span: None,
        value: Node::new(Value::Null),
    }]));
    // self (56) + 1 member slot (1 * 104) + key len (1) + value estimate (56).
    assert_eq!(object.estimated_bytes(), 56 + 104 + 1 + 56);
}
