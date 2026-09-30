// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Inline tokenization with indexed backtick runs and bounded angle lookahead.
use super::{Inline, InlineKind, angle, autolinks, emphasis, entities, links, normalize_lines};
use crate::{Governor, StopReason, contract::Resource};
use std::collections::BTreeMap;

pub(super) fn run(
    source: &str,
    references: &super::References,
    strike: bool,
    bare_urls: bool,
    governor: &mut Governor,
) -> Result<Vec<Inline>, StopReason> {
    let ticks = backticks(source, governor)?;
    let mut tokens = Vec::new();
    let mut cursor = 0;
    let mut brackets = Vec::new();
    let mut previous_marker = false;
    while cursor < source.len() {
        governor.charge(Resource::Work, 1)?;
        if source.as_bytes()[cursor] == b']'
            && let Some(end) = links::close(
                source,
                cursor,
                &mut tokens,
                &mut brackets,
                references,
                strike,
                governor,
            )?
        {
            cursor = end;
            previous_marker = false;
            continue;
        }
        let start = cursor;
        let rest = &source[cursor..];
        let marker = matches!(
            source.as_bytes()[cursor],
            b'*' | b'_' | b'~' | b'[' | b']' | b'!'
        );
        let bare = if bare_urls && brackets.is_empty() {
            autolinks::url_at(source, cursor, governor)?
        } else {
            None
        };
        let (end, kind) = if let Some(bare) = bare {
            bare
        } else {
            match source.as_bytes()[cursor] {
                b'*' | b'_' => delimiter(source, cursor),
                b'~' if strike => delimiter(source, cursor),
                b'!' if rest.starts_with("![") => (cursor + 2, InlineKind::Text("![".into())),
                b'~' | b'[' | b']' | b'!' => literal(source, cursor),
                b'`' => code(source, cursor, &ticks),
                b'\\' => escaped(source, cursor),
                b'&' => entities::one(rest).map_or_else(
                    || literal(source, cursor),
                    |(width, value)| (cursor + width, InlineKind::Text(value.into_owned())),
                ),
                b' ' | b'\t' | b'\r' | b'\n' => whitespace(source, cursor),
                b'<' => {
                    governor.charge(
                        Resource::Work,
                        super::super::size(rest.len()).saturating_mul(3),
                    )?;
                    angle::parse(rest).map_or_else(
                        || literal(source, cursor),
                        |(width, kind)| (cursor + width, kind),
                    )
                }
                _ => text_run(source, cursor, bare_urls),
            }
        };
        cursor = end;
        if matches!(kind, InlineKind::Autolink { .. }) {
            links::deactivate(&mut brackets);
        }
        if source.as_bytes()[start] == b'[' || source[start..].starts_with("![") {
            governor.charge(
                Resource::Depth,
                super::super::size(brackets.len().saturating_add(1)),
            )?;
            brackets.push(links::Bracket::new(
                tokens.len(),
                source.as_bytes()[start] == b'!',
            ));
        }
        if !marker
            && !previous_marker
            && let InlineKind::Text(text) = &kind
            && let Some(Inline {
                span,
                kind: InlineKind::Text(prior),
            }) = tokens.last_mut()
        {
            prior.push_str(text);
            span.end = end;
        } else {
            tokens.push(Inline {
                span: start..end,
                kind,
            });
        }
        previous_marker = marker;
    }
    let tokens = emphasis::resolve(source, tokens, strike, governor)?;
    super::render::check(&tokens, governor)?;
    Ok(tokens)
}
fn literal(source: &str, start: usize) -> (usize, InlineKind) {
    let character = source[start..].chars().next().expect("cursor before end");
    (
        start + character.len_utf8(),
        InlineKind::Text(if character == '\0' {
            "\u{fffd}".into()
        } else {
            character.to_string()
        }),
    )
}
fn backticks(
    source: &str,
    governor: &mut Governor,
) -> Result<BTreeMap<usize, Vec<usize>>, StopReason> {
    governor.charge(
        Resource::Work,
        super::super::size(source.len()).saturating_mul(u64::from(source.len().max(1).ilog2()) + 3),
    )?;
    let mut result: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut cursor = 0;
    while cursor < source.len() {
        if source.as_bytes()[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let start = cursor;
        while source.as_bytes().get(cursor) == Some(&b'`') {
            cursor += 1;
        }
        result.entry(cursor - start).or_default().push(start);
    }
    Ok(result)
}
fn code(source: &str, start: usize, ticks: &BTreeMap<usize, Vec<usize>>) -> (usize, InlineKind) {
    let width = source[start..].bytes().take_while(|b| *b == b'`').count();
    let end = ticks.get(&width).and_then(|positions| {
        positions.get(positions.partition_point(|position| *position <= start))
    });
    let Some(&end) = end else {
        return (
            start + width,
            InlineKind::Text(source[start..start + width].into()),
        );
    };
    let content = normalize_lines(&source[start + width..end]).replace('\n', " ");
    let content =
        if content.starts_with(' ') && content.ends_with(' ') && content.bytes().any(|b| b != b' ')
        {
            content[1..content.len() - 1].into()
        } else {
            content
        };
    (end + width, InlineKind::Code(content))
}
fn escaped(source: &str, start: usize) -> (usize, InlineKind) {
    if source
        .as_bytes()
        .get(start + 1)
        .is_some_and(|b| matches!(b, b'\r' | b'\n'))
    {
        let end = after_line(source, start + 1);
        if end < source.len() {
            return (end, InlineKind::HardBreak);
        }
    }
    if let Some(byte) = source
        .as_bytes()
        .get(start + 1)
        .filter(|b| b.is_ascii_punctuation())
    {
        return (start + 2, InlineKind::Text(char::from(*byte).to_string()));
    }
    literal(source, start)
}
fn after_line(source: &str, newline: usize) -> usize {
    let width = if source.as_bytes().get(newline..newline + 2) == Some(b"\r\n") {
        2
    } else {
        1
    };
    let mut cursor = newline + width;
    while source
        .as_bytes()
        .get(cursor)
        .is_some_and(|b| matches!(b, b' ' | b'\t'))
    {
        cursor += 1;
    }
    cursor
}
fn whitespace(source: &str, start: usize) -> (usize, InlineKind) {
    let mut cursor = start;
    while source
        .as_bytes()
        .get(cursor)
        .is_some_and(|b| matches!(b, b' ' | b'\t'))
    {
        cursor += 1;
    }
    if cursor == source.len() {
        return (cursor, InlineKind::Boundary);
    }
    if matches!(source.as_bytes()[cursor], b'\r' | b'\n') {
        let end = after_line(source, cursor);
        if end == source.len() {
            return (end, InlineKind::Boundary);
        }
        let spaces = source[start..cursor]
            .bytes()
            .rev()
            .take_while(|b| *b == b' ')
            .count();
        return (
            end,
            if spaces >= 2 {
                InlineKind::HardBreak
            } else {
                InlineKind::SoftBreak
            },
        );
    }
    (
        cursor,
        if start == 0 {
            InlineKind::Boundary
        } else {
            InlineKind::Text(source[start..cursor].into())
        },
    )
}

fn delimiter(source: &str, start: usize) -> (usize, InlineKind) {
    let marker = source.as_bytes()[start];
    let end = start
        + source[start..]
            .bytes()
            .take_while(|byte| *byte == marker)
            .count();
    (end, InlineKind::Text(source[start..end].into()))
}
fn text_run(source: &str, start: usize, bare_urls: bool) -> (usize, InlineKind) {
    let width = source[start..]
        .find([
            '*', '_', '~', '[', ']', '!', '`', '\\', '<', '&', ' ', '\t', '\r', '\n',
        ])
        .unwrap_or(source.len() - start);
    let width = if bare_urls {
        source[start..]
            .char_indices()
            .skip(1)
            .find(|(_, c)| matches!(c, 'w' | 'h' | 'H' | 'f' | 'F'))
            .map_or(width, |(offset, _)| offset.min(width))
    } else {
        width
    };
    (
        start + width,
        InlineKind::Text(source[start..start + width].replace('\0', "\u{fffd}")),
    )
}
