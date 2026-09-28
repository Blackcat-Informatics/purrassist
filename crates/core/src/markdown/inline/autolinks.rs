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
    while cursor < raw.len() {
        let previous = raw[..cursor].chars().next_back();
        let boundary = previous.is_none_or(|character| {
            character.is_whitespace() || matches!(character, '*' | '_' | '~' | '(')
        });
        let found = if boundary { url(&raw[cursor..]) } else { None };
        let found = found.or_else(|| {
            if previous.is_none_or(|c| {
                !c.is_ascii_alphanumeric() && !matches!(c, '.' | '-' | '_' | '+' | '@')
            }) {
                email(&raw[cursor..])
            } else {
                None
            }
        });
        if let Some((width, destination)) = found {
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
fn url(source: &str) -> Option<(usize, String)> {
    let prefix = ["https://", "http://", "ftp://", "www."]
        .into_iter()
        .find(|prefix| source.starts_with(prefix))?;
    let host_start = if prefix == "www." { 0 } else { prefix.len() };
    let host_end = host_start
        + source[host_start..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .map(char::len_utf8)
            .sum::<usize>();
    let host = source[host_start..host_end].trim_end_matches('.');
    let segments: Vec<_> = host.split('.').collect();
    if segments.len() < 2
        || segments.iter().any(|part| part.is_empty())
        || segments.iter().rev().take(2).any(|part| part.contains('_'))
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
                b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~'
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
    if end < host_start + host.len() {
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
fn email(source: &str) -> Option<(usize, String)> {
    let local = source
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        .count();
    if local == 0 || source.as_bytes().get(local) != Some(&b'@') {
        return None;
    }
    let end = local
        + 1
        + source[local + 1..]
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            .count();
    let domain = source[local + 1..end].trim_end_matches('.');
    if !domain.contains('.') || domain.ends_with(['-', '_']) || domain.split('.').any(str::is_empty)
    {
        return None;
    }
    let end = local + 1 + domain.len();
    Some((end, format!("mailto:{}", &source[..end])))
}
