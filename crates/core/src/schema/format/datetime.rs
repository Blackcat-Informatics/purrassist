// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 3339 `date`/`time`/`date-time` and RFC 3339 Appendix A `duration`.
//! Byte-level, allocation-free parsing: every format here rejects any
//! non-ASCII-digit input (including other decimal digit scripts) and any
//! trailing content, matching the official test suite exactly.
use crate::contract::{Governor, Resource, StopReason};

/// Charge one `Work` unit per scanned byte, like every other format
/// validator in this module, before any parsing.
fn charge(text: &str, governor: &mut Governor) -> Result<(), StopReason> {
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )
}

/// One parsed RFC 3339 full-date, kept so callers (recovery's normalization
/// repair) can re-render it with corrected separators/padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// One parsed RFC 3339 partial-time, kept for the same reason as [`Date`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Time {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub fraction_digits: u8,
    pub offset_minutes: Option<i32>,
}

fn digits(bytes: &[u8], start: usize, count: usize) -> Option<u32> {
    let slice = bytes.get(start..start.checked_add(count)?)?;
    if !slice.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value = 0u32;
    for &byte in slice {
        value = value * 10 + u32::from(byte - b'0');
    }
    Some(value)
}

const fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

const fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Parse an exact `full-date` (`YYYY-MM-DD`), requiring the whole slice.
pub(super) fn parse_date(text: &str) -> Option<Date> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = u16::try_from(digits(bytes, 0, 4)?).ok()?;
    let month = u8::try_from(digits(bytes, 5, 2)?).ok()?;
    let day = u8::try_from(digits(bytes, 8, 2)?).ok()?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    Some(Date { year, month, day })
}

/// Parse an exact `partial-time time-offset` (`HH:MM:SS[.f+](Z|+HH:MM)`).
pub(super) fn parse_time(text: &str) -> Option<Time> {
    let bytes = text.as_bytes();
    if bytes.len() < 9 || bytes[2] != b':' || bytes[5] != b':' {
        return None;
    }
    let hour = u8::try_from(digits(bytes, 0, 2)?).ok()?;
    let minute = u8::try_from(digits(bytes, 3, 2)?).ok()?;
    let second = u8::try_from(digits(bytes, 6, 2)?).ok()?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut cursor = 8;
    let mut fraction_digits = 0u8;
    if bytes.get(cursor) == Some(&b'.') {
        let start = cursor + 1;
        let mut end = start;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            return None;
        }
        fraction_digits = u8::try_from(end - start).unwrap_or(u8::MAX);
        cursor = end;
    }
    let offset_minutes = match bytes.get(cursor) {
        Some(b'Z' | b'z') => {
            cursor += 1;
            Some(0)
        }
        Some(sign @ (b'+' | b'-')) => {
            let offset_hour = digits(bytes, cursor + 1, 2)?;
            if bytes.get(cursor + 3) != Some(&b':') {
                return None;
            }
            let offset_minute = digits(bytes, cursor + 4, 2)?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            cursor += 6;
            let magnitude = i32::try_from(offset_hour * 60 + offset_minute).ok()?;
            Some(if *sign == b'-' { -magnitude } else { magnitude })
        }
        _ => return None,
    };
    if cursor != bytes.len() {
        return None;
    }
    if second == 60 {
        // A leap second is only well-formed at the UTC instant 23:59:60;
        // reject the digit sequence otherwise (RFC 3339 section 5.7).
        let local = i32::from(hour) * 60 + i32::from(minute);
        let utc = (local - offset_minutes?).rem_euclid(24 * 60);
        if utc != 23 * 60 + 59 {
            return None;
        }
    }
    Some(Time {
        hour,
        minute,
        second,
        fraction_digits,
        offset_minutes,
    })
}

pub(super) fn is_date(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    charge(text, governor)?;
    Ok(parse_date(text).is_some())
}

pub(super) fn is_time(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    charge(text, governor)?;
    Ok(parse_time(text).is_some())
}

fn date_time_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 11
        && matches!(bytes[10], b'T' | b't')
        && parse_date(&text[..10]).is_some()
        && parse_time(&text[11..]).is_some()
}

pub(super) fn is_date_time(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    charge(text, governor)?;
    Ok(date_time_ok(text))
}

fn ascii_digit_run(bytes: &[u8], start: usize) -> Option<usize> {
    let mut end = start;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    (end > start).then_some(end)
}

/// `dur-time = "T" (dur-hour / dur-minute / dur-second)`, requiring the
/// whole remaining slice to be consumed by exactly one of the three chained
/// alternatives (RFC 3339 Appendix A).
fn dur_time(bytes: &[u8], start: usize) -> bool {
    let Some(after_first) = ascii_digit_run(bytes, start) else {
        return false;
    };
    match bytes.get(after_first) {
        Some(b'S') => after_first + 1 == bytes.len(),
        Some(b'M') => {
            let end = after_first + 1;
            if end == bytes.len() {
                return true;
            }
            let Some(after_seconds) = ascii_digit_run(bytes, end) else {
                return false;
            };
            bytes.get(after_seconds) == Some(&b'S') && after_seconds + 1 == bytes.len()
        }
        Some(b'H') => {
            let end = after_first + 1;
            if end == bytes.len() {
                return true;
            }
            let Some(after_minutes) = ascii_digit_run(bytes, end) else {
                return false;
            };
            // `dur-hour`'s only continuation is `[dur-minute]`, which
            // requires the literal "M": hours cannot chain directly to
            // seconds without it (`PT1H2S` is invalid).
            if bytes.get(after_minutes) != Some(&b'M') {
                return false;
            }
            let end = after_minutes + 1;
            if end == bytes.len() {
                return true;
            }
            let Some(after_seconds) = ascii_digit_run(bytes, end) else {
                return false;
            };
            bytes.get(after_seconds) == Some(&b'S') && after_seconds + 1 == bytes.len()
        }
        _ => false,
    }
}

/// RFC 3339 Appendix A `duration = "P" (dur-date / dur-time / dur-week)`.
/// Every digit run must be plain ASCII digits; no sign, no fraction, no
/// trailing content survives.
pub(super) fn is_duration(text: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    charge(text, governor)?;
    Ok(duration_ok(text))
}

fn duration_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'P') {
        return false;
    }
    if bytes.get(1) == Some(&b'T') {
        return dur_time(bytes, 2);
    }
    // dur-week = 1*DIGIT "W", standalone (never combined with a dur-time).
    if let Some(after_week_digits) = ascii_digit_run(bytes, 1)
        && bytes.get(after_week_digits) == Some(&b'W')
    {
        return after_week_digits + 1 == bytes.len();
    }
    // dur-date = (dur-day / dur-month / dur-year) [dur-time], where
    // dur-year = 1*DIGIT "Y" [dur-month], dur-month = 1*DIGIT "M" [dur-day],
    // dur-day = 1*DIGIT "D": year may fall through to month, which may fall
    // through to day, but never day directly to year, skipping month.
    let Some(after_digits) = ascii_digit_run(bytes, 1) else {
        return false;
    };
    let mut cursor = match bytes.get(after_digits) {
        Some(b'Y') => {
            let mut cursor = after_digits + 1;
            if let Some(after_month_digits) = ascii_digit_run(bytes, cursor)
                && bytes.get(after_month_digits) == Some(&b'M')
            {
                cursor = after_month_digits + 1;
                if let Some(after_day_digits) = ascii_digit_run(bytes, cursor)
                    && bytes.get(after_day_digits) == Some(&b'D')
                {
                    cursor = after_day_digits + 1;
                }
            }
            cursor
        }
        Some(b'M') => {
            let mut cursor = after_digits + 1;
            if let Some(after_day_digits) = ascii_digit_run(bytes, cursor)
                && bytes.get(after_day_digits) == Some(&b'D')
            {
                cursor = after_day_digits + 1;
            }
            cursor
        }
        Some(b'D') => after_digits + 1,
        _ => return false,
    };
    if cursor == bytes.len() {
        return true;
    }
    if bytes.get(cursor) == Some(&b'T') {
        cursor += 1;
        return dur_time(bytes, cursor);
    }
    false
}

/// Normalize a value that failed its `date`/`date-time`/`time` format
/// assertion (zero-pad numeric fields, and for `date-time`, accept a plain
/// space in place of `T`), or `None` if no such normalization makes it
/// valid. Recovery labels the result an explicit `Guess` inference.
pub(super) fn repair(
    name: &str,
    value: &str,
    governor: &mut Governor,
) -> Result<Option<String>, StopReason> {
    match name {
        "date" => repair_date(value, governor),
        "time" => repair_time(value, governor),
        "date-time" => repair_date_time(value, governor),
        _ => Ok(None),
    }
}

fn zero_padded_digits(field: &str, max_width: usize) -> Option<String> {
    (!field.is_empty()
        && field.len() <= max_width
        && field.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| format!("{field:0>max_width$}"))
}

fn repair_date(value: &str, governor: &mut Governor) -> Result<Option<String>, StopReason> {
    let mut parts = value.split('-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    if parts.next().is_some() {
        return Ok(None);
    }
    let (Some(year), Some(month), Some(day)) = (
        zero_padded_digits(year, 4),
        zero_padded_digits(month, 2),
        zero_padded_digits(day, 2),
    ) else {
        return Ok(None);
    };
    let candidate = format!("{year}-{month}-{day}");
    Ok((candidate != value && is_date(&candidate, governor)?).then_some(candidate))
}

fn repair_time(value: &str, governor: &mut Governor) -> Result<Option<String>, StopReason> {
    let mut parts = value.splitn(3, ':');
    let (Some(hour), Some(minute), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(None);
    };
    let split = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let (second, suffix) = rest.split_at(split);
    let (Some(hour), Some(minute), Some(second)) = (
        zero_padded_digits(hour, 2),
        zero_padded_digits(minute, 2),
        zero_padded_digits(second, 2),
    ) else {
        return Ok(None);
    };
    let candidate = format!("{hour}:{minute}:{second}{suffix}");
    Ok((candidate != value && is_time(&candidate, governor)?).then_some(candidate))
}

fn repair_date_time(value: &str, governor: &mut Governor) -> Result<Option<String>, StopReason> {
    let Some(separator) = value.find(['T', 't', ' ']) else {
        return Ok(None);
    };
    let (date_part, time_part) = (&value[..separator], &value[separator + 1..]);
    let date_fixed = if is_date(date_part, governor)? {
        date_part.to_string()
    } else {
        let Some(fixed) = repair_date(date_part, governor)? else {
            return Ok(None);
        };
        fixed
    };
    let time_fixed = if is_time(time_part, governor)? {
        time_part.to_string()
    } else {
        let Some(fixed) = repair_time(time_part, governor)? else {
            return Ok(None);
        };
        fixed
    };
    let candidate = format!("{date_fixed}T{time_fixed}");
    Ok((candidate != value && is_date_time(&candidate, governor)?).then_some(candidate))
}

#[cfg(test)]
mod tests {
    use super::{is_date, is_date_time, is_duration, is_time, repair};
    use crate::{Budget, Governor};
    fn governor() -> Governor {
        Governor::new(Budget::default())
    }
    #[test]
    fn dates_reject_calendar_and_syntax_violations() {
        assert_eq!(is_date("2020-02-29", &mut governor()), Ok(true));
        assert_eq!(is_date("2021-02-29", &mut governor()), Ok(false));
        assert_eq!(is_date("1998-1-20", &mut governor()), Ok(false));
        assert_eq!(is_date("2020-01-01\u{0}", &mut governor()), Ok(false));
    }
    #[test]
    fn times_apply_the_leap_second_bidi_condition() {
        assert_eq!(is_time("23:59:60Z", &mut governor()), Ok(true));
        assert_eq!(is_time("01:29:60+01:30", &mut governor()), Ok(true));
        assert_eq!(is_time("22:59:60Z", &mut governor()), Ok(false));
        assert_eq!(is_time("23:59:60+01:00", &mut governor()), Ok(false));
        assert_eq!(is_time("23:60:00+00:01", &mut governor()), Ok(false));
    }
    #[test]
    fn date_times_require_a_separator_and_valid_halves() {
        assert_eq!(
            is_date_time("1963-06-19T08:30:06.283185Z", &mut governor()),
            Ok(true)
        );
        assert_eq!(
            is_date_time("1963-06-19t08:30:06.283185z", &mut governor()),
            Ok(true)
        );
        assert_eq!(
            is_date_time("1963-06-19T08:30:06.28123+01:00Z", &mut governor()),
            Ok(false)
        );
        assert_eq!(
            is_date_time("1985-04-12T23:20:50Ztail", &mut governor()),
            Ok(false)
        );
    }
    #[test]
    fn durations_follow_the_appendix_a_grammar_order() {
        assert_eq!(is_duration("P4DT12H30M5S", &mut governor()), Ok(true));
        assert_eq!(is_duration("P1M2D", &mut governor()), Ok(true));
        assert_eq!(is_duration("P2D1Y", &mut governor()), Ok(false));
        assert_eq!(is_duration("PT1D", &mut governor()), Ok(false));
        assert_eq!(is_duration("P1WT1H", &mut governor()), Ok(false));
        assert_eq!(
            is_duration(
                "P999999999999999999999999999999999999999999999999999999999999999999999999999999D",
                &mut governor()
            ),
            Ok(true)
        );
    }
    #[test]
    fn work_is_charged_per_scanned_byte() {
        let mut tight = Governor::new(Budget {
            work: 3,
            ..Budget::default()
        });
        assert_eq!(
            is_date_time("1963-06-19T08:30:06.283185Z", &mut tight),
            Err(crate::StopReason::BudgetExceeded(
                crate::contract::Resource::Work
            ))
        );
    }
    #[test]
    fn repairs_normalize_common_slips() {
        assert_eq!(
            repair("date", "2026-9-28", &mut governor()),
            Ok(Some("2026-09-28".into()))
        );
        assert_eq!(
            repair("date-time", "2026-09-28 10:00:00Z", &mut governor()),
            Ok(Some("2026-09-28T10:00:00Z".into()))
        );
        assert_eq!(repair("date", "2026-09-28", &mut governor()), Ok(None));
        assert_eq!(repair("date", "not-a-date", &mut governor()), Ok(None));
    }
}
