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

pub(super) fn known(name: &str) -> bool {
    NAMES.contains(&name)
}

/// One evaluated `format` check against a string instance.
pub(super) enum Outcome {
    Pass,
    Fail,
    /// Not one of [`NAMES`]; reported as annotation evidence "not evaluated"
    /// rather than a pass/fail, and never asserted even under
    /// format-assertion (Draft 2020-12 leaves unknown formats as no-ops).
    Unknown,
}

/// This format's stable evidence/diagnostic rule identifier
/// (`schema.format.<name>-v1`).
pub(super) fn rule(name: &str) -> String {
    format!("schema.format.{name}-v1")
}

pub(super) fn evaluate(
    name: &str,
    value: &str,
    governor: &mut Governor,
) -> Result<Outcome, StopReason> {
    let pass = match name {
        "date-time" => datetime::is_date_time(value),
        "date" => datetime::is_date(value),
        "time" => datetime::is_time(value),
        "duration" => datetime::is_duration(value),
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
        _ => return Ok(Outcome::Unknown),
    };
    Ok(if pass { Outcome::Pass } else { Outcome::Fail })
}

/// Whether recovery may attempt a normalization repair for a failing
/// `format`; only `date`, `date-time`, and `time` admit one (requirement 4:
/// every other format violation stays an explicit no-candidate proposal).
pub(super) fn repairable(name: &str) -> bool {
    matches!(name, "date" | "date-time" | "time")
}

/// Normalize a `date`/`date-time`/`time` value that failed its format
/// assertion into a labeled guess, or `None` if no repair applies (the
/// value is not repairable syntax, e.g. not even date-shaped).
pub(super) fn repair(name: &str, value: &str) -> Option<String> {
    datetime::repair(name, value)
}
