// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Every Draft 2020-12 `format` value: always evaluated as annotation
//! evidence (`schema.format.<name>-v1`); assertion is driven solely by the
//! dialect's `$vocabulary` declaring `format-assertion` (see
//! `super::vocabulary`), never by a caller switch.
mod datetime;
mod email;
mod host;
mod net;
mod pointer;
mod regexp;
mod template;
mod uri;
mod uuid;

use crate::contract::{Governor, StopReason};

/// Admit a parser's documented full-input passes and fixed operations.
fn admit_scans(
    text: &str,
    passes: u64,
    fixed: u64,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(
        crate::contract::Resource::Work,
        crate::contract::saturating_size(text.len())
            .saturating_mul(passes)
            .saturating_add(fixed),
    )
}

/// Consume the two ASCII hex digits after an already consumed percent sign.
/// An invalid first digit leaves the second untouched; a valid first digit
/// always consumes the second, including a non-hex digit.
fn pct_encoded(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    matches!(chars.next(), Some(h) if h.is_ascii_hexdigit())
        && matches!(chars.next(), Some(h) if h.is_ascii_hexdigit())
}

/// The complete Draft 2020-12 format census (every name this module knows
/// how to evaluate).
pub(super) const NAMES: &[&str] = &[
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "idn-email",
    "hostname",
    "idn-hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uri-reference",
    "iri",
    "iri-reference",
    "uuid",
    "uri-template",
    "json-pointer",
    "relative-json-pointer",
    "regex",
];

/// One evaluated `format` check against a string instance.
pub(super) enum Outcome {
    Pass,
    Fail,
    /// Not one of [`NAMES`]; reported as annotation evidence "not evaluated"
    /// rather than a pass/fail, and never asserted even under
    /// format-assertion (Draft 2020-12 leaves unknown formats as no-ops).
    Unknown,
}

pub(super) fn evaluate(
    name: &str,
    value: &str,
    governor: &mut Governor,
) -> Result<Outcome, StopReason> {
    governor.check()?;
    let pass = match name {
        "date-time" => datetime::is_date_time(value, governor)?,
        "date" => datetime::is_date(value, governor)?,
        "time" => datetime::is_time(value, governor)?,
        "duration" => datetime::is_duration(value, governor)?,
        "email" => email::is_email(value, false, governor)?,
        "idn-email" => email::is_email(value, true, governor)?,
        "hostname" => host::is_hostname(value, false, governor)?,
        "idn-hostname" => host::is_hostname(value, true, governor)?,
        "ipv4" => net::is_ipv4(value, governor)?,
        "ipv6" => net::is_ipv6(value, governor)?,
        "uri" => uri::is_uri(value, false, governor)?,
        "uri-reference" => uri::is_uri(value, true, governor)?,
        "iri" => uri::is_iri(value, false, governor)?,
        "iri-reference" => uri::is_iri(value, true, governor)?,
        "uuid" => uuid::is_uuid(value, governor)?,
        "uri-template" => template::is_uri_template(value, governor)?,
        "json-pointer" => pointer::is_json_pointer(value, governor)?,
        "relative-json-pointer" => pointer::is_relative_json_pointer(value, governor)?,
        "regex" => regexp::is_regex(value, governor)?,
        _ => {
            governor.check()?;
            return Ok(Outcome::Unknown);
        }
    };
    governor.check()?;
    Ok(if pass { Outcome::Pass } else { Outcome::Fail })
}

/// Whether recovery may attempt a normalization repair for a failing
/// `format`; only `date`, `date-time`, and `time` admit one (every other
/// format violation stays an explicit no-candidate proposal).
pub(super) fn repairable(name: &str) -> bool {
    matches!(name, "date" | "date-time" | "time")
}

/// Normalize a `date`/`date-time`/`time` value that failed its format
/// assertion into a labeled guess, or `None` if no repair applies (the
/// value is not repairable syntax, e.g. not even date-shaped).
pub(super) fn repair(
    name: &str,
    value: &str,
    governor: &mut Governor,
) -> Result<Option<crate::ScratchString>, StopReason> {
    datetime::repair(name, value, governor)
}

/// The witness-generation algebra's canonical example value for
/// one known `format` name: always re-checked with [`evaluate`] before use,
/// so an imprecise guess here can only cost a fallback attempt, never an
/// incorrect witness.
pub(super) fn witness(name: &str) -> &'static str {
    match name {
        "date-time" => "1970-01-01T00:00:00Z",
        "date" => "1970-01-01",
        "time" => "00:00:00Z",
        "duration" => "PT0S",
        "email" | "idn-email" => "a@example.com",
        "hostname" | "idn-hostname" => "example.com",
        "ipv4" => "0.0.0.0",
        "ipv6" => "::",
        "uri" | "iri" => "https://example.com/",
        "uuid" => "00000000-0000-0000-0000-000000000000",
        "uri-template" => "https://example.com/{id}",
        "relative-json-pointer" => "0",
        "regex" => "a",
        // "uri-reference"/"iri-reference"/"json-pointer" all accept the
        // empty string, matching every other unknown name's fallback.
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::pct_encoded;

    #[test]
    fn percent_digits_preserve_short_circuit_cursor_consumption() {
        for (source, expected, remaining) in [
            ("aF!", true, "!"),
            ("00終", true, "終"),
            ("Gf!", false, "f!"),
            ("aG!", false, "!"),
            ("é0!", false, "0!"),
            ("０0!", false, "0!"),
            ("a", false, ""),
            ("", false, ""),
        ] {
            let mut chars = source.chars().peekable();
            assert_eq!(pct_encoded(&mut chars), expected, "{source:?}");
            assert_eq!(chars.collect::<String>(), remaining, "{source:?}");
        }
    }

    #[test]
    fn assertion_scans_refuse_work_and_discard_regex_diagnostics() {
        use crate::{Budget, Governor, StopReason, contract::Resource};
        let long = "1".repeat(4096);
        for (name, source) in [
            ("uri", format!("https://example.com/{long}?q=x#y")),
            ("iri-reference", format!("/{long}/é")),
            ("date-time", format!("2026-01-01T00:00:00.{long}Z")),
            ("duration", format!("P{long}Y")),
            ("uuid", "00000000-0000-0000-0000-000000000000".into()),
            ("json-pointer", format!("/{long}/~0~1")),
            ("relative-json-pointer", format!("{long}/x")),
            ("uri-template", format!("/{long}/{{+path}}")),
            ("regex", "[".into()),
        ] {
            let mut governor = Governor::new(Budget::default());
            let outcome = super::evaluate(name, &source, &mut governor).unwrap();
            assert_eq!(
                matches!(outcome, super::Outcome::Pass),
                name != "regex",
                "{name}"
            );
            // In particular, rejected regex diagnostics are temporary, not output.
            assert_eq!(governor.scratch_meter().live(), 0, "{name}");
            let mut refused = Governor::new(Budget {
                work: governor.usage().work - 1,
                ..Budget::default()
            });
            assert!(
                matches!(
                    super::evaluate(name, &source, &mut refused),
                    Err(StopReason::BudgetExceeded(Resource::Work))
                ),
                "{name}"
            );
            assert_eq!(refused.scratch_meter().live(), 0, "{name}");
        }
        for name in ["date", "regex", "unknown-format"] {
            let mut governor = Governor::new(Budget::default());
            governor.cancel();
            assert!(matches!(
                super::evaluate(name, "", &mut governor),
                Err(StopReason::Cancelled)
            ));
        }
    }

    #[test]
    fn hostname_and_email_formats_release_temporary_owners() {
        use crate::{Budget, Governor, StopReason, contract::Resource};
        for (name, source, valid) in [
            ("hostname", "xn--9n2bp8q.xn--9t4b11yi5a", true),
            ("hostname", "xn--X", false),
            ("idn-hostname", "cafe\u{301}.com", true),
            ("idn-hostname", "0a.\u{5d0}", false),
            ("email", "\"quoted@name\"@example.com", true),
            ("idn-email", "δοκιμή@cafe\u{301}.com", true),
            ("idn-email", "δοκιμή@-example.com", false),
        ] {
            let mut governor = Governor::new(Budget::default());
            let outcome = super::evaluate(name, source, &mut governor).unwrap();
            assert_eq!(
                matches!(outcome, super::Outcome::Pass),
                valid,
                "{name}: {source}"
            );
            assert_eq!(governor.scratch_meter().live(), 0);
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
                let mut refused = Governor::new(budget);
                assert!(
                    matches!(super::evaluate(name, source, &mut refused), Err(StopReason::BudgetExceeded(actual)) if actual == resource)
                );
                assert_eq!(refused.scratch_meter().live(), 0);
            }
            let mut cancelled = Governor::new(Budget::default());
            cancelled.cancel();
            assert!(matches!(
                super::evaluate(name, source, &mut cancelled),
                Err(StopReason::Cancelled)
            ));
            assert_eq!(cancelled.scratch_meter().live(), 0);
        }
    }
}
