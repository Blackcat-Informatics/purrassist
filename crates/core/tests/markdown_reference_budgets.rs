// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Scan-based reference budgets scale with consumed text and stop malformed scans.
use purrassist_core::{
    Budget, Governor, StopReason,
    contract::Resource,
    markdown::{Dialect, Document, InlineFragment, References},
};
use std::fmt::Write;

#[test]
fn many_short_definitions_fit_standard_effort_with_nonquadratic_work() {
    for duplicates in [false, true] {
        let mut previous = 0;
        for count in [256, 512, 1024] {
            let mut source = String::new();
            for index in 0..count {
                writeln!(source, "[r{:04}]: /tea", if duplicates { 0 } else { index }).unwrap();
            }
            let mut g = Governor::new(Budget::default());
            let refs = References::parse_blocks(&[&source], &mut g).unwrap();
            assert_eq!(refs.definitions().len(), count);
            assert_eq!(refs.consumed(), [source.len()]);
            let work = g.usage().work;
            println!(
                "duplicates={duplicates} count={count} bytes={} work={work}",
                source.len()
            );
            if previous != 0 {
                assert!(
                    work < previous * 5 / 2,
                    "doubling definitions must not quadruple reservations"
                );
            }
            previous = work;
            if !duplicates && count == 1024 {
                let mut g = Governor::new(Budget::default());
                let doc = Document::parse(&source, &mut g).unwrap();
                assert_eq!(doc.reference_sites(&mut g).unwrap().len(), count);
                println!(
                    "document plus occurrence inspection work={}",
                    g.usage().work
                );
            }
        }
    }
}

#[test]
fn unterminated_destinations_titles_and_whitespace_stop_inside_their_scan() {
    for source in [
        format!("[a]: <{}", "x".repeat(20_000)),
        format!("[a]: /uri \"{}", "x".repeat(20_000)),
        format!("[a]: {}", " ".repeat(20_000)),
    ] {
        // Admission/hash/source retention reserves four units per byte. The extra
        // 128 units admit the start of parsing but cannot finish these scans.
        let budget = Budget {
            work: u64::try_from(source.len()).unwrap() * 4 + 128,
            ..Budget::default()
        };
        let mut g = Governor::new(budget);
        assert_eq!(
            References::parse_blocks(&[&source], &mut g).unwrap_err(),
            StopReason::BudgetExceeded(Resource::Work)
        );
        assert!(g.usage().work > u64::try_from(source.len()).unwrap() * 4);
        assert!(g.usage().work <= budget.work);
    }
}

#[test]
fn blank_line_detection_preserves_all_line_ending_combinations() {
    for first in ["\n", "\r", "\r\n"] {
        for second in ["\n", "\r", "\r\n"] {
            let valid = format!("[a]: /uri \"one{first}two\"{second}");
            let refs =
                References::parse_blocks(&[&valid], &mut Governor::new(Budget::default())).unwrap();
            assert_eq!(refs.definitions().len(), 1);
            assert_eq!(refs.definitions()[0].title.as_deref(), Some("one\ntwo"));
            let blank = format!("[a]: /uri \"one{first} \t{second}two\"{second}");
            let refs =
                References::parse_blocks(&[&blank], &mut Governor::new(Budget::default())).unwrap();
            assert_eq!(refs.definitions(), []);
        }
    }
}

#[test]
fn repeated_reference_links_do_not_reserve_the_remaining_fragment_each_time() {
    let mut previous = 0;
    for count in [256, 512, 1024] {
        let mut g = Governor::new(Budget::default());
        let refs = References::parse_blocks(&["[tea]: /tea\n"], &mut g).unwrap();
        let source = "[tea] ".repeat(count);
        let fragment =
            InlineFragment::parse_with_references(&source, Dialect::CommonMark, &refs, &mut g)
                .unwrap();
        assert_eq!(
            fragment.render_html(&mut g).unwrap(),
            "<a href=\"/tea\">tea</a> ".repeat(count).trim_end()
        );
        let work = g.usage().work;
        println!("reference links={count} parse/render work={work}");
        if previous != 0 {
            assert!(work < previous * 5 / 2);
        }
        previous = work;
    }
}
