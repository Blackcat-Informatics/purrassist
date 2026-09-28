// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Inline link grouping precedes delimiter matching; references remain document-owned.
use super::{Inline, InlineKind, emphasis, entities};
use crate::{Governor, StopReason, contract::Resource};

pub(super) struct Bracket {
    index: usize,
    image: bool,
    active: bool,
}
impl Bracket {
    pub(super) const fn new(index: usize, image: bool) -> Self {
        Self {
            index,
            image,
            active: true,
        }
    }
}
pub(super) fn deactivate(brackets: &mut [Bracket]) {
    for bracket in brackets {
        if !bracket.image {
            bracket.active = false;
        }
    }
}
pub(super) fn close(
    source: &str,
    position: usize,
    tokens: &mut Vec<Inline>,
    brackets: &mut Vec<Bracket>,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    let Some(bracket) = brackets.pop() else {
        return Ok(None);
    };
    if !bracket.active || source.as_bytes().get(position + 1) != Some(&b'(') {
        return Ok(None);
    }
    governor.charge(
        Resource::Work,
        super::super::size(source.len() - position).saturating_mul(4),
    )?;
    let Some((end, destination, title)) = target(source, position + 2, governor)? else {
        return Ok(None);
    };
    let children = tokens.split_off(bracket.index + 1);
    let opening = tokens.pop().expect("indexed opening bracket");
    let children = emphasis::resolve(source, children, governor)?;
    let kind = if bracket.image {
        InlineKind::Image {
            destination,
            title,
            children,
        }
    } else {
        deactivate(brackets);
        InlineKind::Link {
            destination,
            title,
            children,
        }
    };
    tokens.push(Inline {
        span: opening.span.start..end,
        kind,
    });
    Ok(Some(end))
}
type Target = (usize, String, Option<String>);
fn target(
    source: &str,
    start: usize,
    governor: &mut Governor,
) -> Result<Option<Target>, StopReason> {
    let mut cursor = spaces(source, start);
    if cursor > start
        && let Some((end, title)) = title(source, cursor)
    {
        let end = spaces(source, end);
        if source.as_bytes().get(end) == Some(&b')') {
            return Ok(Some((end + 1, String::new(), Some(title))));
        }
    }
    let destination_start = cursor;
    let destination = if source.as_bytes().get(cursor) == Some(&b'<') {
        cursor += 1;
        let begin = cursor;
        while cursor < source.len() {
            match source.as_bytes()[cursor] {
                b'>' => break,
                b'<' | b'\r' | b'\n' => return Ok(None),
                b'\\'
                    if source
                        .as_bytes()
                        .get(cursor + 1)
                        .is_some_and(u8::is_ascii_punctuation) =>
                {
                    cursor += 1;
                }
                _ => {}
            }
            cursor += 1;
        }
        if cursor == source.len() {
            return Ok(None);
        }
        let result = entities::text(&source[begin..cursor]);
        cursor += 1;
        result
    } else {
        let mut depth = 0usize;
        while cursor < source.len() {
            let byte = source.as_bytes()[cursor];
            match byte {
                b'\\'
                    if source
                        .as_bytes()
                        .get(cursor + 1)
                        .is_some_and(u8::is_ascii_punctuation) =>
                {
                    cursor += 1;
                }
                b'(' => {
                    depth += 1;
                    governor.charge(Resource::Depth, super::super::size(depth))?;
                }
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                byte if byte <= b' ' || byte == 127 => break,
                _ => {}
            }
            cursor += 1;
        }
        if depth != 0 {
            return Ok(None);
        }
        entities::text(&source[destination_start..cursor])
    };
    let after = spaces(source, cursor);
    let (end, title) = if after > cursor
        && let Some((end, title)) = title(source, after)
    {
        (spaces(source, end), Some(title))
    } else {
        (after, None)
    };
    Ok((source.as_bytes().get(end) == Some(&b')')).then_some((end + 1, destination, title)))
}
fn spaces(source: &str, start: usize) -> usize {
    start
        + source[start..]
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .count()
}
fn title(source: &str, start: usize) -> Option<(usize, String)> {
    let opening = *source.as_bytes().get(start)?;
    let closing = match opening {
        b'\'' | b'"' => opening,
        b'(' => b')',
        _ => return None,
    };
    let mut cursor = start + 1;
    while cursor < source.len() {
        let byte = source.as_bytes()[cursor];
        if byte == closing {
            return Some((cursor + 1, entities::text(&source[start + 1..cursor])));
        }
        if opening == b'(' && byte == b'(' {
            return None;
        }
        if byte == b'\\'
            && source
                .as_bytes()
                .get(cursor + 1)
                .is_some_and(u8::is_ascii_punctuation)
        {
            cursor += 1;
        }
        cursor += 1;
    }
    None
}
