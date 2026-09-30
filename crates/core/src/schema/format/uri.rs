// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! RFC 3986 `uri`/`uri-reference` and RFC 3987 `iri`/`iri-reference`.
use super::{net::ipv6_literal, pct_encoded};
use crate::contract::{Governor, Resource, StopReason};
use std::iter::Peekable;
use std::str::Chars;

const fn is_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~')
}
const fn is_sub_delim(c: char) -> bool {
    matches!(
        c,
        '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '='
    )
}
/// RFC 3987 `ucschar`.
pub(super) fn is_ucschar(c: char) -> bool {
    matches!(u32::from(c),
        0xA0..=0xD7FF | 0xF900..=0xFDCF | 0xFDF0..=0xFFEF
        | 0x1_0000..=0x1_FFFD | 0x2_0000..=0x2_FFFD | 0x3_0000..=0x3_FFFD
        | 0x4_0000..=0x4_FFFD | 0x5_0000..=0x5_FFFD | 0x6_0000..=0x6_FFFD
        | 0x7_0000..=0x7_FFFD | 0x8_0000..=0x8_FFFD | 0x9_0000..=0x9_FFFD
        | 0xA_0000..=0xA_FFFD | 0xB_0000..=0xB_FFFD | 0xC_0000..=0xC_FFFD
        | 0xD_0000..=0xD_FFFD | 0xE_1000..=0xE_FFFD)
}
/// RFC 3987 `iprivate`; only `iquery` admits it.
pub(super) fn is_iprivate(c: char) -> bool {
    matches!(u32::from(c), 0xE000..=0xF8FF | 0xF_0000..=0xF_FFFD | 0x10_0000..=0x10_FFFD)
}

type Cursor<'a> = Peekable<Chars<'a>>;

/// Consume characters while they are `%XX` or satisfy `extra`; stop (without
/// consuming) at the first character satisfying `stop`. `false` on the first
/// disallowed character.
fn scan(chars: &mut Cursor<'_>, stop: impl Fn(char) -> bool, extra: impl Fn(char) -> bool) -> bool {
    while let Some(&c) = chars.peek() {
        if stop(c) {
            return true;
        }
        chars.next();
        if c == '%' {
            if !pct_encoded(chars) {
                return false;
            }
        } else if !(is_unreserved(c) || is_sub_delim(c) || extra(c)) {
            return false;
        }
    }
    true
}

fn is_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `host = IP-literal / IPv4address / reg-name` (RFC 3986 section 3.2.2),
/// generalized to `ihost` (RFC 3987) when `iri` allows `ucschar`.
fn is_host(text: &str, iri: bool) -> bool {
    if let Some(inner) = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return ipv6_literal(inner);
    }
    let mut chars = text.chars().peekable();
    scan(&mut chars, |_| false, |c| iri && is_ucschar(c))
}

/// `authority = [ userinfo "@" ] host [ ":" port ]` (RFC 3986 section 3.2).
fn is_authority(text: &str, iri: bool) -> bool {
    let (userinfo, hostport) = match text.split_once('@') {
        Some((info, rest)) => (Some(info), rest),
        None => (None, text),
    };
    if let Some(info) = userinfo {
        let mut chars = info.chars().peekable();
        if !scan(
            &mut chars,
            |_| false,
            |c| c == ':' || (iri && is_ucschar(c)),
        ) {
            return false;
        }
    }
    let (host, port) = if hostport.starts_with('[') {
        match hostport.find(']') {
            Some(end) => (&hostport[..=end], &hostport[end + 1..]),
            None => return false,
        }
    } else {
        hostport
            .find(':')
            .map_or((hostport, ""), |at| (&hostport[..at], &hostport[at..]))
    };
    if !is_host(host, iri) {
        return false;
    }
    port.is_empty() || (port.starts_with(':') && port[1..].bytes().all(|b| b.is_ascii_digit()))
}

/// `pchar` (RFC 3986) or `ipchar` (RFC 3987) as a `scan` `extra` predicate.
fn is_pchar_extra(iri: bool) -> impl Fn(char) -> bool {
    move |c| c == ':' || c == '@' || (iri && is_ucschar(c))
}

/// One RFC 3986/3987 URI or IRI reference. `require_scheme` selects the
/// absolute `uri`/`iri` formats over the `-reference` variants.
pub(super) fn is_reference(text: &str, iri: bool, require_scheme: bool) -> bool {
    let mut rest = text;
    let has_scheme = rest.find(':').is_some_and(|colon| {
        let candidate = &rest[..colon];
        // A scheme never contains '/', '?', or '#'; if the text before the
        // first ':' does, that ':' belongs to a later component instead.
        is_scheme(candidate) && !candidate.contains(['/', '?', '#'])
    });
    if has_scheme {
        rest = &rest[rest.find(':').expect("checked above") + 1..];
    } else if require_scheme {
        return false;
    }
    let (before_fragment, fragment) = match rest.split_once('#') {
        Some((before, after)) => (before, Some(after)),
        None => (rest, None),
    };
    let (before_query, query) = match before_fragment.split_once('?') {
        Some((before, after)) => (before, Some(after)),
        None => (before_fragment, None),
    };
    // `hier-part`/`relative-part`'s `"//" authority path-abempty`
    // alternative: since `before_query` already excludes `?query`/
    // `#fragment`, the authority runs from after `//` to the next `/` (or
    // the end of the string) and `path-abempty` is exactly what remains.
    let (has_authority, authority, path) =
        before_query
            .strip_prefix("//")
            .map_or((false, "", before_query), |rest| {
                let end = rest.find('/').unwrap_or(rest.len());
                (true, &rest[..end], &rest[end..])
            });
    if has_authority {
        if !is_authority(authority, iri) {
            return false;
        }
    } else if !has_scheme && !path.starts_with('/') {
        // `path-noscheme`: the first segment must not contain a literal ':'
        // (RFC 3986 section 3.3), which would make it ambiguous with a scheme.
        let first_segment_end = path.find('/').unwrap_or(path.len());
        if path[..first_segment_end].contains(':') {
            return false;
        }
    }
    let mut path_chars = path.chars().peekable();
    if !scan(
        &mut path_chars,
        |c| matches!(c, '?' | '#'),
        |c| c == '/' || is_pchar_extra(iri)(c),
    ) {
        return false;
    }
    if let Some(query) = query {
        let mut chars = query.chars().peekable();
        if !scan(
            &mut chars,
            |c| c == '#',
            |c| c == '/' || c == '?' || is_pchar_extra(iri)(c) || (iri && is_iprivate(c)),
        ) {
            return false;
        }
    }
    if let Some(fragment) = fragment {
        let mut chars = fragment.chars().peekable();
        if !scan(
            &mut chars,
            |_| false,
            |c| c == '/' || c == '?' || is_pchar_extra(iri)(c),
        ) {
            return false;
        }
    }
    true
}

pub(super) fn is_uri(
    text: &str,
    allow_relative: bool,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    Ok(text.is_ascii() && is_reference(text, false, !allow_relative))
}

pub(super) fn is_iri(
    text: &str,
    allow_relative: bool,
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    governor.charge(
        Resource::Work,
        u64::try_from(text.len()).unwrap_or(u64::MAX),
    )?;
    Ok(is_reference(text, true, !allow_relative))
}
