// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Official Unicode normalization conformance, all input relations in UAX #15.
use purrassist_core::text::{Normalization, normalize};
fn decode(column: &str) -> String {
    column
        .split_whitespace()
        .map(|point| {
            char::from_u32(u32::from_str_radix(point, 16).expect("hex code point"))
                .expect("Unicode scalar")
        })
        .collect()
}
#[test]
fn unicode_17_normalization_relations() {
    let corpus = include_str!("data/unicode/NormalizationTest.txt");
    let mut cases = 0;
    for (line_number, line) in corpus.lines().enumerate() {
        let line = line.split('#').next().expect("first component").trim();
        if line.is_empty() || line.starts_with('@') {
            continue;
        }
        let columns: Vec<_> = line.split(';').take(5).map(decode).collect();
        assert_eq!(columns.len(), 5);
        for (index, input) in columns.iter().enumerate() {
            for (form, expected) in [
                (
                    Normalization::Nfc,
                    if index < 3 { &columns[1] } else { &columns[3] },
                ),
                (
                    Normalization::Nfd,
                    if index < 3 { &columns[2] } else { &columns[4] },
                ),
                (Normalization::Nfkc, &columns[3]),
                (Normalization::Nfkd, &columns[4]),
            ] {
                assert_eq!(
                    &normalize(input, form),
                    expected,
                    "line {} input {} form {form:?}",
                    line_number + 1,
                    index + 1
                );
            }
        }
        cases += 1;
    }
    assert_eq!(cases, 20_034, "frozen Unicode 17 corpus census");
}
