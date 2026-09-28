// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Delimiter matching with per-kind lower bounds and source-addressed tree assembly.
use super::{Inline, InlineKind};
use crate::{Governor, StopReason, contract::Resource};
use std::{collections::BTreeMap, ops::Range};
#[derive(Default)]
struct Delimiter {
    span: Range<usize>,
    original: usize,
    marker: u8,
    open: bool,
    close: bool,
    previous: usize,
    next: usize,
}
#[derive(Clone, Copy)]
enum Event {
    Open {
        end: usize,
        strong: bool,
        strike: bool,
    },
    Close {
        end: usize,
    },
}
struct Frame {
    start: usize,
    strong: bool,
    strike: bool,
    children: Vec<Inline>,
}

pub(super) fn resolve(
    source: &str,
    tokens: Vec<Inline>,
    strike: bool,
    governor: &mut Governor,
) -> Result<Vec<Inline>, StopReason> {
    let mut delimiters = vec![Delimiter::default()];
    for token in &tokens {
        governor.charge(Resource::Work, 1)?;
        if !matches!(token.kind, InlineKind::Text(_)) {
            continue;
        }
        let raw = &source[token.span.clone()];
        let Some(marker @ (b'*' | b'_' | b'~')) = raw.as_bytes().first().copied() else {
            continue;
        };
        if !raw.bytes().all(|byte| byte == marker) {
            continue;
        }
        if marker == b'~' && (!strike || raw.len() != 2) {
            continue;
        }
        let (open, close) = flanking(source, &token.span, marker);
        let id = delimiters.len();
        delimiters[id - 1].next = id;
        delimiters.push(Delimiter {
            span: token.span.clone(),
            original: raw.len(),
            marker,
            open,
            close,
            previous: id - 1,
            next: 0,
        });
    }
    let events = matches(&mut delimiters, governor)?;
    assemble(source, tokens, &events, governor)
}
fn flanking(source: &str, span: &Range<usize>, marker: u8) -> (bool, bool) {
    let before = source[..span.start].chars().next_back();
    let after = source[span.end..].chars().next();
    let (before_space, before_punct) = classify(before);
    let (after_space, after_punct) = classify(after);
    let left = !after_space && (!after_punct || before_space || before_punct);
    let right = !before_space && (!before_punct || after_space || after_punct);
    if marker == b'_' {
        (
            left && (!right || before_punct),
            right && (!left || after_punct),
        )
    } else {
        (left, right)
    }
}
fn classify(character: Option<char>) -> (bool, bool) {
    let Some(character) = character else {
        return (true, false);
    };
    let category = crate::text::general_category(if character == '\0' {
        '\u{fffd}'
    } else {
        character
    });
    (
        category == "Zs" || matches!(character, '\t' | '\n' | '\r' | '\u{c}'),
        category.starts_with(['P', 'S']),
    )
}
const fn remove(delimiters: &mut [Delimiter], id: usize) {
    let previous = delimiters[id].previous;
    let next = delimiters[id].next;
    delimiters[previous].next = next;
    if next != 0 {
        delimiters[next].previous = previous;
    }
}
fn matches(
    delimiters: &mut [Delimiter],
    governor: &mut Governor,
) -> Result<BTreeMap<usize, Event>, StopReason> {
    let mut floors = [[[0usize; 3]; 2]; 3];
    let mut events = BTreeMap::new();
    let mut closer = delimiters[0].next;
    while closer != 0 {
        governor.charge(Resource::Work, 1)?;
        if !delimiters[closer].close {
            closer = delimiters[closer].next;
            continue;
        }
        let close = &delimiters[closer];
        let kind = if close.marker == b'~' {
            2
        } else {
            usize::from(close.marker == b'_')
        };
        let can_open = usize::from(close.open);
        let modulo = close.original % 3;
        let floor = floors[kind][can_open][modulo];
        let mut opener = close.previous;
        while opener > floor {
            governor.charge(Resource::Work, 1)?;
            let open = &delimiters[opener];
            let odd = close.marker != b'~'
                && (close.open || open.close)
                && (open.original + close.original).is_multiple_of(3)
                && (!open.original.is_multiple_of(3) || !close.original.is_multiple_of(3));
            if open.open && open.marker == close.marker && !odd {
                break;
            }
            opener = open.previous;
        }
        if opener > floor {
            governor.charge(Resource::ScratchBytes, 256)?;
            let width = if delimiters[opener].span.len() >= 2 && delimiters[closer].span.len() >= 2
            {
                2
            } else {
                1
            };
            let open_end = delimiters[opener].span.end;
            let close_start = delimiters[closer].span.start;
            events.insert(
                open_end - width,
                Event::Open {
                    end: open_end,
                    strong: width == 2,
                    strike: delimiters[opener].marker == b'~',
                },
            );
            events.insert(
                close_start,
                Event::Close {
                    end: close_start + width,
                },
            );
            delimiters[opener].span.end -= width;
            delimiters[closer].span.start += width;
            delimiters[opener].next = closer;
            delimiters[closer].previous = opener;
            if delimiters[opener].span.is_empty() {
                remove(delimiters, opener);
            }
            if delimiters[closer].span.is_empty() {
                let next = delimiters[closer].next;
                remove(delimiters, closer);
                closer = next;
            }
        } else {
            floors[kind][can_open][modulo] = delimiters[closer].previous;
            let next = delimiters[closer].next;
            if !delimiters[closer].open {
                remove(delimiters, closer);
            }
            closer = next;
        }
    }
    Ok(events)
}
fn append(token: Inline, output: &mut Vec<Inline>, frames: &mut [Frame]) {
    if let Some(frame) = frames.last_mut() {
        frame.children.push(token);
    } else {
        output.push(token);
    }
}
fn assemble(
    source: &str,
    tokens: Vec<Inline>,
    events: &BTreeMap<usize, Event>,
    governor: &mut Governor,
) -> Result<Vec<Inline>, StopReason> {
    let mut output = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    for token in tokens {
        governor.charge(Resource::Work, 1)?;
        if events.range(token.span.clone()).next().is_none() {
            append(token, &mut output, &mut frames);
            continue;
        }
        let mut position = token.span.start;
        while position < token.span.end {
            governor.charge(Resource::Work, 1)?;
            match events.get(&position) {
                Some(Event::Open {
                    end,
                    strong,
                    strike,
                }) => {
                    governor.charge(
                        Resource::Depth,
                        super::super::size(frames.len().saturating_add(1)),
                    )?;
                    frames.push(Frame {
                        start: position,
                        strong: *strong,
                        strike: *strike,
                        children: Vec::new(),
                    });
                    position = *end;
                }
                Some(Event::Close { end }) => {
                    let frame = frames.pop().expect("matched emphasis opener");
                    append(
                        Inline {
                            span: frame.start..*end,
                            kind: if frame.strike {
                                InlineKind::Strikethrough {
                                    children: frame.children,
                                }
                            } else {
                                InlineKind::Emphasis {
                                    strong: frame.strong,
                                    children: frame.children,
                                }
                            },
                        },
                        &mut output,
                        &mut frames,
                    );
                    position = *end;
                }
                None => {
                    let end = events
                        .range(position..token.span.end)
                        .next()
                        .map_or(token.span.end, |(position, _)| *position);
                    append(
                        Inline {
                            span: position..end,
                            kind: InlineKind::Text(source[position..end].into()),
                        },
                        &mut output,
                        &mut frames,
                    );
                    position = end;
                }
            }
        }
    }
    Ok(output)
}
