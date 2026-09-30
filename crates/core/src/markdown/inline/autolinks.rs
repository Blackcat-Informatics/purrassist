// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! GFM text-node autolinks, preserving raw source spans and protecting existing links.
use super::{Inline, InlineKind, entities};
use crate::{Governor, StopReason, contract::Resource};

pub(super) fn apply(
    source: &str,
    tokens: Vec<Inline>,
    governor: &mut Governor,
) -> Result<Vec<Inline>, StopReason> {
    let mut merged: Vec<Inline> = Vec::new();
    for mut token in tokens {
        match &mut token.kind {
            InlineKind::Emphasis { children, .. } | InlineKind::Strikethrough { children } => {
                *children = apply(source, std::mem::take(children), governor)?;
            }
            _ => {}
        }
        if matches!(token.kind, InlineKind::Text(_))
            && let Some(prior) = merged.last_mut()
            && let (InlineKind::Text(previous), InlineKind::Text(next)) =
                (&mut prior.kind, &token.kind)
        {
            previous.push_str(next);
            prior.span.end = token.span.end;
        } else {
            merged.push(token);
        }
    }
    let mut output = Vec::new();
    for token in merged {
        if matches!(token.kind, InlineKind::Text(_)) {
            split(source, token, &mut output, governor)?;
        } else {
            output.push(token);
        }
    }
    Ok(output)
}
fn split(
    source: &str,
    token: Inline,
    output: &mut Vec<Inline>,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let raw = &source[token.span.clone()];
    governor.charge(
        Resource::Work,
        super::super::size(raw.len())
            .saturating_mul(64)
            .saturating_add(1),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        super::super::size(raw.len()).saturating_mul(128),
    )?;
    let mut cursor = 0;
    let mut start = 0;
    let mut retry = false;
    while cursor < raw.len() {
        let previous = source[..token.span.start + cursor].chars().next_back();
        let found = if retry
            || previous
                .is_none_or(|c| !c.is_ascii_alphanumeric() && !matches!(c, '.' | '-' | '_' | '+'))
        {
            email(&raw[cursor..])
        } else {
            Err(0)
        };
        retry = false;
        if let Ok((width, destination)) = found {
            if start < cursor {
                output.push(literal(
                    source,
                    token.span.start + start..token.span.start + cursor,
                ));
            }
            output.push(Inline {
                span: token.span.start + cursor..token.span.start + cursor + width,
                kind: InlineKind::Autolink {
                    destination,
                    label: raw[cursor..cursor + width].into(),
                },
            });
            cursor += width;
            start = cursor;
            retry = true;
        } else if let Err(skipped) = found
            && skipped > 0
        {
            cursor += skipped;
            retry = true;
        } else {
            cursor += raw[cursor..]
                .chars()
                .next()
                .expect("remaining character")
                .len_utf8();
        }
    }
    if start == 0 {
        output.push(token);
    } else if start < raw.len() {
        output.push(literal(source, token.span.start + start..token.span.end));
    }
    Ok(())
}
fn literal(source: &str, span: std::ops::Range<usize>) -> Inline {
    Inline {
        kind: InlineKind::Text(entities::text(&source[span.clone()])),
        span,
    }
}
pub(super) fn url_at(
    source: &str,
    cursor: usize,
    governor: &mut Governor,
) -> Result<Option<(usize, InlineKind)>, StopReason> {
    let rest = &source[cursor..];
    if !["www.", "http://", "https://", "ftp://"]
        .iter()
        .any(|prefix| {
            rest.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        })
    {
        return Ok(None);
    }
    let previous = source[..cursor].chars().next_back();
    let boundary = if rest.starts_with("www.") {
        previous.is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '('))
    } else {
        previous.is_none_or(|c| !c.is_ascii_alphabetic())
    };
    if !boundary {
        return Ok(None);
    }
    governor.charge(
        Resource::Work,
        super::super::size(rest.len()).saturating_mul(64),
    )?;
    governor.charge(
        Resource::ScratchBytes,
        super::super::size(rest.len()).saturating_mul(128),
    )?;
    Ok(url(rest).map(|(width, destination)| {
        (
            cursor + width,
            InlineKind::Autolink {
                destination,
                label: rest[..width].into(),
            },
        )
    }))
}
fn url(source: &str) -> Option<(usize, String)> {
    let prefix = ["https://", "http://", "ftp://", "www."]
        .into_iter()
        .find(|prefix| {
            source.get(..prefix.len()).is_some_and(|head| {
                if *prefix == "www." {
                    head == *prefix
                } else {
                    head.eq_ignore_ascii_case(prefix)
                }
            })
        })?;
    let host_start = if prefix == "www." { 0 } else { prefix.len() };
    let initial = source[host_start..].chars().next()?;
    if initial.is_whitespace() || crate::text::general_category(initial).starts_with('P') {
        return None;
    }
    // Domain admission and the full URL extent are separate: punctuation can
    // end the domain scan while remaining valid in the linked URL suffix.
    // Count periods before trimming URL delimiters, and reject underscores in
    // the final two scanned segments (unless there are more than ten periods).
    let host = source[host_start..].trim_end_matches(char::is_whitespace);
    let mut cursor = initial.len_utf8();
    let mut periods = 0;
    let mut prior_underscores = 0;
    let mut current_underscores = 0;
    while cursor + 1 < host.len() {
        let mut character = host[cursor..].chars().next().expect("domain character");
        cursor += character.len_utf8();
        if character == '\\' && cursor < host.len() - 1 {
            character = host[cursor..]
                .chars()
                .next()
                .expect("escaped domain character");
            cursor += character.len_utf8();
        }
        match character {
            '_' => current_underscores += 1,
            '.' => {
                prior_underscores = current_underscores;
                current_underscores = 0;
                periods += 1;
            }
            '-' => {}
            character
                if character.is_whitespace()
                    || crate::text::general_category(character).starts_with('P') =>
            {
                break;
            }
            _ => {}
        }
    }
    if (prefix == "www." && periods == 0)
        || (periods <= 10 && prior_underscores + current_underscores > 0)
    {
        return None;
    }
    let mut end = source
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(source.len());
    loop {
        let prior = end;
        while end > 0
            && matches!(
                source.as_bytes()[end - 1],
                b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' | b'\'' | b'"'
            )
        {
            end -= 1;
        }
        if source[..end].ends_with(';')
            && let Some(amp) = source[..end].rfind('&')
            && source[amp + 1..end - 1]
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic())
            && amp + 1 < end - 1
        {
            end = amp;
        }
        let open = source[..end].bytes().filter(|byte| *byte == b'(').count();
        let mut close = source[..end].bytes().filter(|byte| *byte == b')').count();
        while close > open && source[..end].ends_with(')') {
            end -= 1;
            close -= 1;
        }
        if prior == end {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    Some((
        end,
        if prefix == "www." {
            format!("http://{}", &source[..end])
        } else {
            source[..end].into()
        },
    ))
}
fn email(source: &str) -> Result<(usize, String), usize> {
    let protocol = ["mailto:", "xmpp:"]
        .into_iter()
        .find(|prefix| source.starts_with(prefix));
    let offset = protocol.map_or(0, str::len);
    let address = &source[offset..];
    let local = address
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        .count();
    if local == 0 || address.as_bytes().get(local) != Some(&b'@') {
        return Err(0);
    }
    let bytes = address.as_bytes();
    let mut end = local + 1;
    let mut dots = 0;
    while let Some(&byte) = bytes.get(end) {
        if byte == b'@' {
            // Retry at the domain start; the preceding at-sign bounds
            // how much of the next address may become its local part.
            return Err(offset + local + 1);
        }
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_')
            || (byte == b'/' && protocol == Some("xmpp:"))
        {
            end += 1;
        } else if byte == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric) {
            dots += 1;
            end += 1;
        } else {
            break;
        }
    }
    if dots == 0
        || !bytes
            .get(end.wrapping_sub(1))
            .is_some_and(u8::is_ascii_alphabetic)
    {
        // Retain the unconsumed delimiter as a possible new local part,
        // without restarting inside the domain already examined.
        return Err(offset + end);
    }
    let width = offset + end;
    Ok((
        width,
        if protocol.is_some() {
            source[..width].into()
        } else {
            format!("mailto:{}", &source[..width])
        },
    ))
}
