// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Independent oracles: a breadth-first edit oracle, exhaustive over every
//! ordered pair of strings of length <= 4 on a 3-symbol alphabet, for
//! Levenshtein and unrestricted Damerau-Levenshtein distance; and a path-
//! normalization oracle, an independent reimplementation of the documented
//! path-matching normalization (`docs/MATCHING-QUALITY.md`'s note that
//! `paths::PathIndex` inherits the matching lattice and protected-class
//! guard) compared against `paths::PathIndex` on generated paths.
use purrassist_core::{
    Budget, Governor, Outcome,
    matching::{self, MatchRequest},
    paths::PathIndex,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const ALPHABET: [u8; 3] = *b"abc";
const MAX_WORD_LENGTH: usize = 4;
// Any optimal edit sequence between two strings of length <= MAX_WORD_LENGTH
// never needs an intermediate string longer than MAX_WORD_LENGTH; two extra
// slots of headroom (matching this file's previous convention) keep the
// bound generous rather than tight.
const MAX_BFS_LENGTH: usize = MAX_WORD_LENGTH + 2;

fn words() -> Vec<Vec<u8>> {
    let mut all = vec![Vec::new()];
    let mut level = vec![Vec::new()];
    for _ in 0..MAX_WORD_LENGTH {
        level = level
            .into_iter()
            .flat_map(|prefix| {
                ALPHABET.map(move |letter| {
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
    if next.len() <= MAX_BFS_LENGTH && seen.insert(next.clone()) {
        queue.push_back((next, distance));
    }
}

/// Shortest sequence of single-character insertions, deletions, and
/// replacements (plus, when `allow_swap`, adjacent transpositions
/// intermixed freely with the other operations, not restricted to one swap
/// per substring) connecting `source` to `target`. A full breadth-first
/// search over the graph of same-alphabet strings up to `MAX_BFS_LENGTH`,
/// so `allow_swap` computes *unrestricted* Damerau-Levenshtein distance,
/// not the more restrictive optimal-string-alignment variant.
fn shortest_edits(source: &[u8], allow_swap: bool) -> BTreeMap<Vec<u8>, usize> {
    let mut seen = BTreeSet::from([source.to_vec()]);
    let mut queue = VecDeque::from([(source.to_vec(), 0)]);
    let mut distances = BTreeMap::new();
    while let Some((current, distance)) = queue.pop_front() {
        if current.len() <= MAX_WORD_LENGTH {
            distances.insert(current.clone(), distance);
            if distances.len() == 121 {
                return distances;
            }
        }
        for position in 0..current.len() {
            let mut deleted = current.clone();
            deleted.remove(position);
            enqueue(deleted, distance + 1, &mut seen, &mut queue);
            for letter in ALPHABET {
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
            for letter in ALPHABET {
                let mut inserted = current.clone();
                inserted.insert(position, letter);
                enqueue(inserted, distance + 1, &mut seen, &mut queue);
            }
        }
    }
    panic!("all short same-alphabet strings are connected by insertions and deletions")
}

#[test]
fn distances_agree_with_independent_shortest_edit_search_exhaustively() {
    let words = words();
    // 1 (empty) + 3 + 9 + 27 + 81 = 121 strings of length <= 4 on {a, b, c}.
    assert_eq!(words.len(), 121);
    for source in &words {
        // One graph traversal answers all 121 destinations for this source;
        // the oracle still visits actual edit edges rather than using the
        // production distance recurrence.
        let ordinary_distances = shortest_edits(source, false);
        let transposing_distances = shortest_edits(source, true);
        for target in &words {
            let left = std::str::from_utf8(source).unwrap();
            let right = std::str::from_utf8(target).unwrap();
            let ordinary = ordinary_distances[target];
            let transposing = transposing_distances[target];
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
    // Every ordered pair above is checked, including (word, word) (distance
    // zero) and both (a, b) and (b, a) directions for every unordered pair.
    assert_eq!(words.len() * words.len(), 14_641);
    // A worked non-ASCII case using a real transposition across a
    // multi-byte character, kept from the original curated regression.
    assert_eq!(
        matching::damerau_levenshtein("é猫", "猫é", &mut Governor::new(Budget::default())),
        Ok(1)
    );
}

// ---------------------------------------------------------------------
// Path-normalization oracle: an independent reimplementation of the two
// documented path-matching invariants -- portability admission (no
// absolute paths, no traversal, no empty components, no backslashes, no
// colons, no NULs) and basename equivalence under case/width/diacritic
// folding -- compared against `paths::PathIndex` on generated inputs. This
// does not call any of `purrassist_core::text`'s normalization functions;
// it is a second, from-scratch implementation limited to the ASCII/Latin-1
// and confusable characters this oracle itself generates.
// ---------------------------------------------------------------------

/// Independent reimplementation of `paths.rs`'s private `portable` check.
fn independently_portable(path: &str) -> bool {
    let mut component = String::new();
    for character in path.chars().chain(std::iter::once('/')) {
        match character {
            '\0' | '\\' | ':' => return false,
            '/' => {
                if component.is_empty() || component == "." || component == ".." {
                    return false;
                }
                component.clear();
            }
            other => component.push(other),
        }
    }
    true
}

const PORTABILITY_CASES: &[&str] = &[
    "src/reader.rs",
    "a/b/c.rs",
    "single.rs",
    "",
    "../secrets",
    "/absolute",
    "a/../b",
    "a//b",
    "C:/file",
    "a\\b",
    "trailing/",
    "a/./b",
    "has\0null",
    "a/b/../c",
    "deep/nested/path/to/file.rs",
];

#[test]
fn path_portability_oracle_matches_index_admission() {
    let components = ["", ".", "..", "src", "café", "file.rs"];
    let mut generated = Vec::new();
    for first in components {
        for second in components {
            for third in components {
                generated.push(format!("{first}/{second}/{third}"));
            }
        }
    }
    generated.extend(PORTABILITY_CASES.iter().map(|case| (*case).to_string()));
    assert_eq!(generated.len(), 231, "documented admission census");
    for case in &generated {
        let admitted =
            PathIndex::new([case.clone()], &mut Governor::new(Budget::default())).is_ok();
        assert_eq!(
            admitted,
            independently_portable(case),
            "portability disagreement for {case:?}"
        );
    }
}

/// Independent fold: lowercase (ASCII, byte-wise, distinct from
/// `text::case_fold`'s full Unicode table), fullwidth Latin letters
/// (U+FF21..=U+FF3A / U+FF41..=U+FF5A) mapped to their ASCII originals, and
/// a small hand-picked Latin-1 diacritic table covering only the letters
/// this oracle's own generator uses -- an intentionally narrower, separately
/// written stand-in for the lattice `matching.rs` documents, sufficient to
/// cross-check that `PathIndex` actually applies *some* case/width/
/// diacritic-insensitive basename comparison, not to reproduce its exact
/// score thresholds.
fn independent_fold(text: &str) -> String {
    text.chars()
        .map(|character| {
            let mapped = match character {
                'A'..='Z' => char::from_u32(u32::from(character) + 32).unwrap_or(character),
                '\u{FF21}'..='\u{FF3A}' => {
                    char::from_u32(u32::from(character) - 0xFEE0 + 32).unwrap_or(character)
                }
                '\u{FF41}'..='\u{FF5A}' => {
                    char::from_u32(u32::from(character) - 0xFEE0).unwrap_or(character)
                }
                'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
                'á' | 'à' | 'â' | 'Á' | 'À' | 'Â' => 'a',
                'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
                'ó' | 'ò' | 'ô' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Ö' => 'o',
                'ý' | 'ÿ' | 'Ý' => 'y',
                'ñ' | 'Ñ' => 'n',
                'ç' | 'Ç' => 'c',
                'ü' | 'ú' | 'ù' | 'Ü' | 'Ú' | 'Ù' => 'u',
                other => other,
            };
            mapped.to_ascii_lowercase()
        })
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

struct BasenameCase {
    indexed: &'static str,
    query: &'static str,
}
const BASENAME_CASES: &[BasenameCase] = &[
    BasenameCase {
        indexed: "src/reader.rs",
        query: "src/reader.rs",
    },
    BasenameCase {
        indexed: "src/reader.rs",
        query: "src/READER.rs",
    },
    BasenameCase {
        indexed: "src/reader.rs",
        query: "src/Reader.rs",
    },
    BasenameCase {
        indexed: "docs/café.md",
        query: "docs/cafe.md",
    },
    BasenameCase {
        indexed: "docs/café.md",
        query: "docs/CAFÉ.md",
    },
    BasenameCase {
        indexed: "lib/widget.py",
        query: "lib/\u{FF37}\u{FF29}\u{FF24}\u{FF27}\u{FF25}\u{FF34}.py",
    },
    BasenameCase {
        indexed: "lib/widget.py",
        query: "lib/gadget.py",
    },
    BasenameCase {
        indexed: "tests/naïve.txt",
        query: "tests/naive.txt",
    },
    BasenameCase {
        indexed: "src/reader.rs",
        query: "src/writer.rs",
    },
    BasenameCase {
        indexed: "src/reader.rs",
        query: "src/unrelated.rs",
    },
];

#[test]
fn path_basename_fold_oracle_matches_index_resolution() {
    for case in BASENAME_CASES {
        let index = PathIndex::new(
            [case.indexed.to_string()],
            &mut Governor::new(Budget::default()),
        )
        .unwrap();
        let context = case
            .indexed
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        let result = index.resolve(case.query, context, &MatchRequest::default());
        let predicted_equivalent =
            independent_fold(basename(case.query)) == independent_fold(basename(case.indexed));
        match result.outcome {
            Outcome::Resolved(_) | Outcome::BestGuess(_) => assert!(
                predicted_equivalent,
                "index resolved {:?} -> {:?} but the independent fold disagrees",
                case.query, case.indexed
            ),
            Outcome::Unresolved | Outcome::Ambiguous(_) => assert!(
                !predicted_equivalent,
                "independent fold predicts {:?} == {:?} but the index did not resolve",
                case.query, case.indexed
            ),
        }
    }
}

#[test]
fn generated_basename_equivalence_and_context_follow_independent_oracle() {
    let names = [
        "reader", "writer", "service", "café", "naïve", "piñata", "résumé",
    ];
    let directories = ["src", "lib", "tests"];
    let mut checked = 0;
    for name in names {
        let variants = [
            name.to_string(),
            name.to_uppercase(),
            independent_fold(name),
            name.chars()
                .map(|letter| {
                    if letter.is_ascii_alphabetic() {
                        char::from_u32(u32::from(letter) + 0xfee0).unwrap()
                    } else {
                        letter
                    }
                })
                .collect(),
        ];
        let indexed: Vec<_> = directories
            .iter()
            .map(|parent| format!("{parent}/{name}.md"))
            .collect();
        let index = PathIndex::new(indexed.clone(), &mut Governor::new(Budget::default())).unwrap();
        for variant in variants {
            let query = format!("{variant}.md");
            assert_eq!(
                independent_fold(&query),
                independent_fold(&format!("{name}.md"))
            );
            for (parent, expected) in directories.iter().zip(&indexed) {
                let result = index.resolve(&query, parent, &MatchRequest::default());
                match result.outcome {
                    Outcome::Resolved(value) | Outcome::BestGuess(value) => {
                        assert_eq!(&value.path, expected);
                    }
                    other => panic!("equivalent basename {query:?} in {parent}: {other:?}"),
                }
                checked += 1;
            }
            let result = index.resolve(&query, "", &MatchRequest::default());
            let Outcome::Ambiguous(matches) = result.outcome else {
                panic!("three equivalent basenames must stay ambiguous: {query:?}");
            };
            assert_eq!(
                matches
                    .iter()
                    .map(|value| &value.path)
                    .collect::<BTreeSet<_>>(),
                indexed.iter().collect()
            );
            checked += 1;
        }
        // Explicit indexed identities override a conflicting context directory.
        for path in &indexed {
            let result = index.resolve(path, "elsewhere", &MatchRequest::default());
            assert!(matches!(result.outcome, Outcome::Resolved(value) if value.path == *path));
        }
    }
    assert_eq!(checked, 112);
}
