// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Ordered maps share vector storage accounting without changing map semantics.

use purrassist_core::contract::Resource;
use purrassist_core::{Budget, Governor, ScratchMap, ScratchVec, StopReason};

#[test]
fn ordered_map_and_pair_vector_have_identical_growth_charges() {
    let map_governor = Governor::new(Budget::default());
    let vector_governor = Governor::new(Budget::default());
    let mut map = ScratchMap::<u32, u64>::new(&map_governor);
    let mut vector = ScratchVec::<(u32, u64)>::new(&vector_governor);
    for key in [5, 1, 4, 2, 3] {
        assert_eq!(map.insert(key, u64::from(key)), Ok(None));
        vector.push((key, u64::from(key))).unwrap();
        assert_eq!(map_governor.usage(), vector_governor.usage());
        assert_eq!(
            map_governor.scratch_meter().live(),
            vector_governor.scratch_meter().live()
        );
    }
    assert_eq!(map_governor.scratch_meter().live(), 128);
    let charged = map_governor.usage();
    assert_eq!(map.insert(3, 30), Ok(Some(3)));
    assert_eq!(map.get(&3), Some(&30));
    assert_eq!(map.remove(&1), Some(1));
    assert_eq!(map.remove(&9), None);
    assert_eq!(map.insert(0, 10), Ok(None));
    assert_eq!(map_governor.usage(), charged);
    assert_eq!(
        map.iter().copied().collect::<Vec<_>>(),
        vec![(0, 10), (2, 2), (3, 30), (4, 4), (5, 5)]
    );
    assert_eq!(map.len(), 5);
}

#[test]
fn map_drop_releases_only_its_storage_once() {
    let mut governor = Governor::new(Budget::default());
    let meter = governor.scratch_meter();
    let unrelated = governor.lease_scratch(96).unwrap();
    let mut map = ScratchMap::<u32, u64>::new(&governor);
    map.insert(2, 20).unwrap();
    map.insert(1, 10).unwrap();
    assert_eq!(meter.live(), 160);
    drop(map);
    assert_eq!(meter.live(), 96);
    assert_eq!(meter.total_charged(), 160);
    drop(unrelated);
    assert_eq!(meter.live(), 0);
    assert_eq!(governor.check(), Ok(()));
}

#[test]
fn taking_pairs_retains_the_charge_after_plain_vector_drop() {
    let governor = Governor::new(Budget {
        scratch_bytes: 64,
        ..Budget::default()
    });
    let meter = governor.scratch_meter();
    let mut map = ScratchMap::<u32, u64>::new(&governor);
    map.insert(2, 20).unwrap();
    map.insert(1, 10).unwrap();
    let pairs = map.into_pairs();
    assert_eq!(pairs, vec![(1, 10), (2, 20)]);
    assert_eq!(meter.live(), 64);
    drop(pairs);
    assert_eq!(meter.live(), 64);
    let mut next = ScratchMap::<u32, u64>::new(&governor);
    assert_eq!(
        next.insert(0, 0),
        Err(StopReason::BudgetExceeded(Resource::ScratchBytes))
    );
    assert_eq!(next.len(), 0);
    assert_eq!(meter.total_charged(), 64);
}

#[test]
fn exhausted_growth_preserves_entries_and_the_governors_sticky_stop() {
    let governor = Governor::new(Budget {
        scratch_bytes: 64,
        ..Budget::default()
    });
    let meter = governor.scratch_meter();
    let mut map = ScratchMap::<u32, u64>::new(&governor);
    for key in [3, 1, 4, 2] {
        map.insert(key, u64::from(key)).unwrap();
    }
    let expected = vec![(1, 1), (2, 2), (3, 3), (4, 4)];
    let usage = governor.usage();
    let stopped = StopReason::BudgetExceeded(Resource::ScratchBytes);
    assert_eq!(map.reserve(1), Err(stopped.clone()));
    assert_eq!(map.iter().copied().collect::<Vec<_>>(), expected);
    assert_eq!(map.insert(5, 5), Err(stopped.clone()));
    assert_eq!(map.iter().copied().collect::<Vec<_>>(), expected);
    assert_eq!(governor.usage(), usage);
    assert_eq!(governor.check(), Err(stopped.clone()));
    drop(map);
    assert_eq!(meter.live(), 0);
    assert_eq!(governor.check(), Err(stopped.clone()));
    let mut retry = ScratchVec::<(u32, u64)>::new(&governor);
    assert_eq!(retry.push((0, 0)), Err(stopped));
    assert_eq!(meter.live(), 0);
}

#[test]
fn zero_sized_pairs_need_no_storage_charge() {
    let governor = Governor::new(Budget {
        scratch_bytes: 0,
        ..Budget::default()
    });
    let mut map = ScratchMap::<(), ()>::new(&governor);
    map.reserve(usize::MAX).unwrap();
    assert_eq!(map.insert((), ()), Ok(None));
    assert_eq!(map.insert((), ()), Ok(Some(())));
    assert_eq!(map.len(), 1);
    assert_eq!(map.into_pairs(), vec![((), ())]);
    assert_eq!(governor.scratch_meter().live(), 0);
    assert_eq!(governor.usage().total_scratch_charged, 0);
    assert_eq!(governor.check(), Ok(()));
}
