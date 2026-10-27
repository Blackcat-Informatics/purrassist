// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exact numeric retention, mathematical reading and admitted formatting.

use purrassist_core::{
    Budget, CancellationToken, Governor, StopReason,
    contract::{Resource, format_output_governed},
    json::{self, IntegerReading, Number, NumberParseError},
};
use std::fmt;

#[test]
fn shared_exact_reading_preserves_cancellation_zero_and_full_integer_boundaries() {
    let exact = |negative, magnitude| IntegerReading::Exact {
        negative,
        magnitude,
    };
    for (input, expected) in [
        ("1e2", exact(false, 100)),
        ("100.0", exact(false, 100)),
        ("1.5e1", exact(false, 15)),
        ("100e-2", exact(false, 1)),
        (
            "-0.000e-999999999999999999999999999999999999999999",
            exact(false, 0),
        ),
        ("1e-1", IntegerReading::Fractional),
        ("1.01", IntegerReading::Fractional),
        (
            "340282366920938463463374607431768211455",
            exact(false, u128::MAX),
        ),
        (
            "-340282366920938463463374607431768211455",
            exact(true, u128::MAX),
        ),
        (
            "340282366920938463463374607431768211456",
            IntegerReading::OutOfRange,
        ),
        (
            "1e999999999999999999999999999999999999999999",
            IntegerReading::OutOfRange,
        ),
        (
            "1e-999999999999999999999999999999999999999999",
            IntegerReading::Fractional,
        ),
    ] {
        let number: Number = input.parse().expect("valid numeric source");
        let mut governor = Governor::new(Budget {
            scratch_bytes: 0,
            ..Budget::default()
        });
        assert_eq!(number.integer_magnitude(), expected, "{input}");
        assert_eq!(
            number
                .integer_magnitude_governed(&mut governor)
                .expect("allocation-free exact reading"),
            expected,
            "{input}"
        );
        assert_eq!(governor.usage().peak_scratch_bytes, 0);
    }
    for (input, expected) in [
        (format!("1{}e-4096", "0".repeat(4096)), exact(false, 1)),
        (
            format!("1{}1e-4097", "0".repeat(4096)),
            IntegerReading::Fractional,
        ),
        (format!("0e-{}", "9".repeat(4096)), exact(false, 0)),
    ] {
        let number: Number = input.parse().expect("long valid numeric source");
        assert_eq!(number.integer_magnitude(), expected);
        assert_eq!(
            number
                .integer_magnitude_governed(&mut Governor::new(Budget {
                    scratch_bytes: 0,
                    ..Budget::default()
                }))
                .expect("no exponent-sized allocation"),
            expected
        );
    }
}

#[test]
fn shared_numeric_validation_preserves_parser_spelling_and_distinct_refusals() {
    for source in ["0", "-0.00", "1.2300E+4", "1.5e1", "18446744073709551615"] {
        let ordinary: Number = source.parse().expect("valid ordinary number");
        let mut governor = Governor::new(Budget::default());
        let governed =
            Number::parse_governed(source, &mut governor).expect("valid governed number");
        assert_eq!(governed.as_str(), ordinary.as_str());
        assert_eq!(
            json::parse_with(source, &mut Governor::new(Budget::default()))
                .expect("actual parser shares retention")
                .to_json(),
            source
        );
    }
    for invalid in ["", "01", "1e", "+1", "NaN", "1.", "--1"] {
        assert!(invalid.parse::<Number>().is_err());
        assert!(matches!(
            Number::parse_governed(invalid, &mut Governor::new(Budget::default())),
            Err(NumberParseError::Syntax(_))
        ));
    }
    let source = "1.2300e4";
    let mut governor = Governor::new(Budget::default());
    Number::parse_governed(source, &mut governor).expect("measure retention");
    let usage = governor.usage();
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
        (
            Budget {
                input_bytes: (source.len() - 1) as u64,
                ..Budget::default()
            },
            Resource::InputBytes,
        ),
    ] {
        assert!(
            matches!(Number::parse_governed(source, &mut Governor::new(budget)), Err(NumberParseError::Resource(StopReason::BudgetExceeded(actual))) if actual == resource)
        );
    }
    let number: Number = source.parse().expect("caller-prepared number");
    let mut measured = Governor::new(Budget::default());
    number
        .integer_magnitude_governed(&mut measured)
        .expect("measure reading");
    assert_eq!(
        number.integer_magnitude_governed(&mut Governor::new(Budget {
            work: measured.usage().work - 1,
            ..Budget::default()
        })),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(
        number.integer_magnitude_governed(&mut Governor::with_cancellation(
            Budget::default(),
            token.clone()
        )),
        Err(StopReason::Cancelled)
    );
    assert!(matches!(
        Number::parse_governed(
            source,
            &mut Governor::with_cancellation(Budget::default(), token)
        ),
        Err(NumberParseError::Resource(StopReason::Cancelled))
    ));
}

#[test]
fn standard_numeric_formatting_and_writer_failures_remain_governed() {
    struct Swallows;
    impl fmt::Display for Swallows {
        fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
            let _ = output.write_str("unadmitted");
            Ok(())
        }
    }
    let mut governor = Governor::new(Budget::default());
    let number =
        Number::from_unsigned_governed(u64::MAX, &mut governor).expect("full portable domain");
    assert_eq!(number.as_str(), Number::from_unsigned(u64::MAX).as_str());
    let usage = governor.usage();
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
        assert_eq!(
            Number::from_unsigned_governed(u64::MAX, &mut Governor::new(budget)),
            Err(StopReason::BudgetExceeded(resource))
        );
    }
    assert_eq!(
        format_output_governed(
            format_args!("{Swallows}"),
            &mut Governor::new(Budget {
                work: 0,
                ..Budget::default()
            })
        ),
        Err(StopReason::BudgetExceeded(Resource::Work))
    );
}
