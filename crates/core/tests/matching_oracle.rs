// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Independent breadth-first edit oracle for short scalar-distance cases.
use purrassist_core::{Budget, Governor, matching};
use std::collections::{BTreeSet, VecDeque};

fn words() -> Vec<Vec<u8>> {
    let mut all = vec![Vec::new()];
    let mut level = vec![Vec::new()];
    for _ in 0..3 {
        level = level
            .into_iter()
            .flat_map(|prefix| {
                (*b"ab").map(move |letter| {
                    let mut next = prefix.clone();
                    next.push(letter);
                    next
                })
            })
            .collect();
        all.extend(level.iter().cloned());
    }
    all
}

fn enqueue(
    next: Vec<u8>,
    distance: usize,
    seen: &mut BTreeSet<Vec<u8>>,
    queue: &mut VecDeque<(Vec<u8>, usize)>,
) {
    if next.len() <= 5 && seen.insert(next.clone()) {
        queue.push_back((next, distance));
    }
}

fn shortest_edits(source: &[u8], target: &[u8], allow_swap: bool) -> usize {
    let mut seen = BTreeSet::from([source.to_vec()]);
    let mut queue = VecDeque::from([(source.to_vec(), 0)]);
    while let Some((current, distance)) = queue.pop_front() {
        if current == target {
            return distance;
        }
        for position in 0..current.len() {
            let mut deleted = current.clone();
            deleted.remove(position);
            enqueue(deleted, distance + 1, &mut seen, &mut queue);
            for letter in *b"ab" {
                if current[position] != letter {
                    let mut replaced = current.clone();
                    replaced[position] = letter;
                    enqueue(replaced, distance + 1, &mut seen, &mut queue);
                }
            }
            if allow_swap
                && position + 1 < current.len()
                && current[position] != current[position + 1]
            {
                let mut swapped = current.clone();
                swapped.swap(position, position + 1);
                enqueue(swapped, distance + 1, &mut seen, &mut queue);
            }
        }
        for position in 0..=current.len() {
            for letter in *b"ab" {
                let mut inserted = current.clone();
                inserted.insert(position, letter);
                enqueue(inserted, distance + 1, &mut seen, &mut queue);
            }
        }
    }
    panic!("all short binary strings are connected by insertions and deletions")
}

#[test]
fn distances_agree_with_independent_shortest_edit_search() {
    let words = words();
    assert_eq!(words.len(), 15);
    for source in &words {
        for target in &words {
            let left = std::str::from_utf8(source).unwrap();
            let right = std::str::from_utf8(target).unwrap();
            let ordinary = shortest_edits(source, target, false);
            let transposing = shortest_edits(source, target, true);
            assert_eq!(
                matching::levenshtein(left, right, &mut Governor::new(Budget::default())),
                Ok(ordinary),
                "Levenshtein {left:?} -> {right:?}"
            );
            assert_eq!(
                matching::damerau_levenshtein(left, right, &mut Governor::new(Budget::default())),
                Ok(transposing),
                "Damerau {left:?} -> {right:?}"
            );
        }
    }
    assert_eq!(
        matching::damerau_levenshtein("é猫", "猫é", &mut Governor::new(Budget::default())),
        Ok(1)
    );
}
