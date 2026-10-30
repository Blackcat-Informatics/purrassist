// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The Markdown context of call markup: whether a recognized call sits
//! inside fenced code, an inline code span, or a block quote, where it may
//! be an example rather than a call (`tool_call.context.fenced-example`).
//!
//! [`ContextTracker`] is a byte-step state machine the recognizer runs over
//! every output byte. It records only the candidate lines and runs a later
//! decision needs: code-fence lines (a run of at least three backticks or
//! tildes after at most three spaces of indentation), block-quote lines (a
//! `>` after at most three spaces), and backtick runs. Which fences open and
//! close is decided at the end of the turn, over the fence lines outside
//! every recognized call and tool result, so a fence written *inside* an
//! argument value (a file's content, say) never changes how the calls after
//! it are read. A call is in context when its first byte lies inside an
//! open-to-close fenced block (or one still open at the end of output), on a
//! block-quote line, or when backtick runs of equal length touch both of its
//! ends (an inline code span around exactly the call).
//!
//! The tracker's per-byte work is constant and part of the lexical layer's
//! [`K_LEX`](crate::grammar::cost::K_LEX); each recorded line or run charges
//! [`RECORD_BYTES`] scratch bytes. Every decision is a function of the bytes
//! alone, so chunking never changes it.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); `pub(crate)`
// (matching `unreachable_pub`) wins, as in `grammar::engine`.
#![allow(clippy::redundant_pub_crate)]
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};
use crate::grammar::{LineDecision, LineSuffix};
use core::ops::Range;

/// Scratch bytes charged per recorded fence line, quote line or backtick run.
pub(crate) const RECORD_BYTES: u64 = 48;

/// Where the current line stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    /// Inside the leading indentation.
    Start,
    /// Inside a run of fence characters.
    Run,
    /// After a fence run of at least three characters.
    Rest,
    /// A block-quote line.
    Quote,
    /// Any other line.
    Other,
}

/// One candidate code-fence line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FenceLine {
    start: usize,
    end: usize,
    marker: usize,
    indent: u8,
    protocol_closing: bool,
    fence: u8,
    length: usize,
    /// Only whitespace follows the run: the line can close a fence.
    closing: bool,
    /// The info string holds no backtick: the line can open a fence.
    opening: bool,
}

/// One grammar-owned fence region, with a portable charge for its two offsets.
pub(crate) struct ProtocolRegion(Range<usize>);

impl ScratchCharge for ProtocolRegion {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 16);
        16
    };
}

pub(crate) struct StrictFences {
    pub(crate) examples: Vec<Range<usize>>,
    pub(crate) protocol: ScratchVec<ProtocolRegion>,
}

/// The byte-step Markdown context scanner of one turn.
#[derive(Clone, Debug)]
pub(crate) struct ContextTracker {
    line: usize,
    indent: u8,
    state: Line,
    run: (u8, usize),
    run_start: usize,
    protocols: bool,
    suffix: Option<LineSuffix>,
    rest_blank: bool,
    rest_tick: bool,
    tick: Option<usize>,
    fences: Vec<FenceLine>,
    quotes: Vec<Range<usize>>,
    ticks: Vec<Range<usize>>,
}

impl ContextTracker {
    pub(crate) const fn new(protocols: bool) -> Self {
        Self {
            line: 0,
            indent: 0,
            state: Line::Start,
            run: (0, 0),
            run_start: 0,
            protocols,
            suffix: Some(LineSuffix::Horizontal),
            rest_blank: true,
            rest_tick: false,
            tick: None,
            fences: Vec::new(),
            quotes: Vec::new(),
            ticks: Vec::new(),
        }
    }

    /// Consume the byte at offset `at`.
    pub(crate) fn step(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if byte == b'`' {
            self.tick.get_or_insert(at);
        } else if let Some(start) = self.tick.take() {
            governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
            self.ticks.push(start..at);
        }
        if byte == b'\n' {
            return self.end_line(at + 1, false, governor);
        }
        match self.state {
            Line::Start => match byte {
                b' ' if self.indent < 3 => self.indent += 1,
                b' ' | b'\t' if self.protocols => self.indent = 4,
                b'`' | b'~' => {
                    self.state = Line::Run;
                    self.run = (byte, 1);
                    self.run_start = at;
                }
                b'>' if self.indent < 4 => self.state = Line::Quote,
                _ => self.state = Line::Other,
            },
            Line::Run => {
                if byte == self.run.0 {
                    self.run.1 += 1;
                } else if self.run.1 >= 3 {
                    self.state = Line::Rest;
                    self.rest(byte);
                } else {
                    self.state = Line::Other;
                }
            }
            Line::Rest => self.rest(byte),
            Line::Quote | Line::Other => {}
        }
        Ok(())
    }

    const fn rest(&mut self, byte: u8) {
        self.rest_blank &= byte.is_ascii_whitespace();
        self.rest_tick |= byte == b'`';
        if self.protocols
            && let Some(mut suffix) = self.suffix
        {
            self.suffix = match suffix.step(byte) {
                LineDecision::Invalid => None,
                _ => Some(suffix),
            };
        }
    }

    /// Finish the current line at `end` (just past its newline, or the end
    /// of output) and start the next.
    fn end_line(
        &mut self,
        end: usize,
        eof: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let fence = match self.state {
            Line::Run => self.run.1 >= 3,
            Line::Rest => true,
            Line::Start | Line::Quote | Line::Other => false,
        };
        if fence {
            governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
            self.fences.push(FenceLine {
                start: self.line,
                end,
                marker: self.run_start,
                indent: self.indent,
                protocol_closing: self.suffix.is_some_and(|suffix| !eof || suffix.at_end()),
                fence: self.run.0,
                length: self.run.1,
                closing: self.rest_blank,
                opening: !(self.run.0 == b'`' && self.rest_tick),
            });
        } else if self.state == Line::Quote {
            governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
            self.quotes.push(self.line..end);
        }
        self.line = end;
        self.indent = 0;
        self.state = Line::Start;
        self.run = (0, 0);
        self.suffix = Some(LineSuffix::Horizontal);
        self.rest_blank = true;
        self.rest_tick = false;
        Ok(())
    }

    /// End the scan at the end of output, offset `end`.
    pub(crate) fn finish(&mut self, end: usize, governor: &mut Governor) -> Result<(), StopReason> {
        if let Some(start) = self.tick.take() {
            governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
            self.ticks.push(start..end);
        }
        if self.line < end {
            self.end_line(end, true, governor)?;
        }
        Ok(())
    }

    /// The fenced blocks formed by the fence lines outside every range of
    /// `excluded` (sorted by start), a block still open at `end` running to
    /// it.
    fn fenced(&self, excluded: &[Range<usize>], end: usize) -> Vec<Range<usize>> {
        let mut blocks = Vec::new();
        let mut open: Option<&FenceLine> = None;
        for line in &self.fences {
            let position = if self.protocols {
                line.marker
            } else {
                line.start
            };
            if line.indent > 3 || inside(excluded, position) {
                continue;
            }
            match open {
                None if line.opening => open = Some(line),
                Some(opened)
                    if line.fence == opened.fence
                        && line.length >= opened.length
                        && line.closing =>
                {
                    blocks.push(opened.start..line.end);
                    open = None;
                }
                _ => {}
            }
        }
        if let Some(opened) = open {
            blocks.push(opened.start..end);
        }
        blocks
    }

    /// Complete calls protect literal text. Protocol starts identify only a
    /// header, never an arbitrary rejected span's claimed end.
    pub(crate) fn strict_fenced(
        &self,
        calls: &[Range<usize>],
        starts: &[usize],
        end: usize,
        governor: &mut Governor,
    ) -> Result<StrictFences, StopReason> {
        governor.charge(
            Resource::Work,
            crate::contract::saturating_size(calls.len() + starts.len() + self.fences.len()),
        )?;
        let mut regions = StrictFences {
            examples: Vec::new(),
            protocol: ScratchVec::new(governor),
        };
        let mut open: Option<&FenceLine> = None;
        let mut protocol: Option<&FenceLine> = None;
        let (mut next, mut hint, mut protected_end) = (0, 0, 0);
        for line in &self.fences {
            while starts.get(hint).is_some_and(|start| *start < line.marker) {
                hint += 1;
            }
            let header = starts.get(hint).is_some_and(|start| *start == line.marker);
            if header
                && line.marker >= protected_end
                && let Some(prior) = protocol.take()
            {
                regions
                    .protocol
                    .push(ProtocolRegion(prior.start..line.start))?;
            }
            let position = if self.protocols {
                line.marker
            } else {
                line.start
            };
            while let Some(call) = calls.get(next).filter(|call| call.start <= position) {
                if protocol.is_none() && open.is_none_or(|fence| call.start < fence.start) {
                    protected_end = protected_end.max(call.end);
                }
                next += 1;
            }
            if position < protected_end {
                continue;
            }
            if let Some(prior) = protocol {
                if line.fence == prior.fence && line.length == prior.length && line.protocol_closing
                {
                    regions
                        .protocol
                        .push(ProtocolRegion(prior.start..line.end))?;
                    protocol = None;
                }
                continue;
            }
            if open.is_none() && header {
                protocol = Some(line);
                continue;
            }
            if line.indent > 3 {
                continue;
            }
            match open {
                None if line.opening => open = Some(line),
                Some(prior)
                    if line.fence == prior.fence && line.length >= prior.length && line.closing =>
                {
                    governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
                    regions.examples.push(prior.start..line.end);
                    open = None;
                }
                _ => {}
            }
        }
        if let Some(prior) = protocol {
            regions.protocol.push(ProtocolRegion(prior.start..end))?;
        }
        if let Some(prior) = open {
            governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
            regions.examples.push(prior.start..end);
        }
        Ok(regions)
    }

    /// Paired inline backtick spans. Literal text of calls that began outside
    /// an inline span cannot open or close that span.
    pub(crate) fn strict_inline(
        &self,
        calls: &[Range<usize>],
        fenced: &[Range<usize>],
        protocol: &[ProtocolRegion],
        governor: &mut Governor,
    ) -> Result<Vec<Range<usize>>, StopReason> {
        governor.charge(
            Resource::Work,
            crate::contract::saturating_size(
                calls.len()
                    + self.ticks.len()
                    + fenced.len() * 2
                    + self.quotes.len()
                    + protocol.len(),
            ),
        )?;
        let mut blocks = Vec::new();
        let mut open: Option<&Range<usize>> = None;
        let (mut next, mut fence, mut call_fence, mut quote, mut protected_end) = (0, 0, 0, 0, 0);
        let mut owned = 0;
        for run in &self.ticks {
            while protocol
                .get(owned)
                .is_some_and(|region| region.0.end <= run.start)
            {
                owned += 1;
            }
            if open.is_none()
                && protocol
                    .get(owned)
                    .is_some_and(|region| region.0.contains(&run.start))
            {
                continue;
            }
            advance_regions(fenced, &mut fence, run.start);
            if fenced
                .get(fence)
                .is_some_and(|block| block.contains(&run.start))
            {
                open = None;
                continue;
            }
            while let Some(call) = calls.get(next).filter(|call| call.start <= run.start) {
                advance_regions(fenced, &mut call_fence, call.start);
                advance_regions(&self.quotes, &mut quote, call.start);
                let example = fenced
                    .get(call_fence)
                    .is_some_and(|block| block.contains(&call.start))
                    || self
                        .quotes
                        .get(quote)
                        .is_some_and(|block| block.contains(&call.start));
                if !example && open.is_none_or(|opening| call.start < opening.start) {
                    protected_end = protected_end.max(call.end);
                }
                next += 1;
            }
            if run.start < protected_end {
                continue;
            }
            match open {
                None => open = Some(run),
                Some(opening) if opening.len() == run.len() => {
                    governor.charge(Resource::ScratchBytes, RECORD_BYTES)?;
                    blocks.push(opening.start..run.end);
                    open = None;
                }
                _ => {}
            }
        }
        Ok(blocks)
    }

    /// Context membership against already computed enclosing ranges.
    pub(crate) fn strict_example(
        &self,
        call: &Range<usize>,
        fenced: &[Range<usize>],
        inline: &[Range<usize>],
        governor: &mut Governor,
    ) -> Result<bool, StopReason> {
        let depth = fenced
            .len()
            .max(self.quotes.len())
            .max(inline.len())
            .max(1)
            .ilog2()
            + 1;
        governor.charge(Resource::Work, u64::from(depth).saturating_mul(3))?;
        Ok(inside(fenced, call.start)
            || inside(&self.quotes, call.start)
            || inside(inline, call.start))
    }

    /// For each call span of `calls`, the context range that makes it a
    /// possible example, if any. `excluded` lists every call and tool-result
    /// span of the turn's selected reading, sorted by start; `end` is the
    /// output length.
    pub(crate) fn examples(
        &self,
        calls: &[Range<usize>],
        excluded: &[Range<usize>],
        end: usize,
        governor: &mut Governor,
    ) -> Result<Vec<Option<Range<usize>>>, StopReason> {
        governor.charge(
            Resource::Work,
            crate::contract::saturating_size(
                self.fences.len() + self.quotes.len() + self.ticks.len() + calls.len(),
            ),
        )?;
        let fenced = self.fenced(excluded, end);
        Ok(calls
            .iter()
            .map(|call| {
                if let Some(block) = fenced.iter().find(|block| block.contains(&call.start)) {
                    return Some(block.clone());
                }
                if let Some(line) = self.quotes.iter().find(|line| line.contains(&call.start)) {
                    return Some(line.clone());
                }
                let before = self.ticks.iter().find(|run| run.end == call.start)?;
                let closing = before.len();
                let after = self
                    .ticks
                    .iter()
                    .find(|run| starts_run(run, call.end, closing))?;
                Some(before.start..after.end)
            })
            .collect())
    }
}

/// Whether `run` starts at `at` and is `length` bytes long.
const fn starts_run(run: &Range<usize>, at: usize, length: usize) -> bool {
    run.start == at && run.end - run.start == length
}

/// Whether `offset` lies inside one of `ranges` (sorted by start).
fn inside(ranges: &[Range<usize>], offset: usize) -> bool {
    let index = ranges.partition_point(|range| range.start <= offset);
    index > 0 && offset < ranges[index - 1].end
}

/// Each cursor advances once per range, covered by the caller's linear scan admission.
fn advance_regions(regions: &[Range<usize>], cursor: &mut usize, at: usize) {
    while regions.get(*cursor).is_some_and(|region| region.end <= at) {
        *cursor += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::ContextTracker;
    use crate::{Budget, Governor};

    fn scan(text: &str) -> ContextTracker {
        let mut governor = Governor::new(Budget::default());
        let mut tracker = ContextTracker::new(false);
        for (at, &byte) in text.as_bytes().iter().enumerate() {
            tracker.step(byte, at, &mut governor).expect("step");
        }
        tracker.finish(text.len(), &mut governor).expect("finish");
        tracker
    }

    fn examples(text: &str, call: &str, excluded: &[&str]) -> Option<core::ops::Range<usize>> {
        let tracker = scan(text);
        let span = |part: &str| {
            let start = text.find(part).expect("part");
            start..start + part.len()
        };
        let excluded: Vec<_> = excluded.iter().map(|part| span(part)).collect();
        tracker
            .examples(
                &[span(call)],
                &excluded,
                text.len(),
                &mut Governor::new(Budget::default()),
            )
            .expect("examples")
            .remove(0)
    }

    #[test]
    fn fenced_block_and_unclosed_fence() {
        let text = "Example:\n```xml\n<c>x</c>\n```\nNow <c>y</c>";
        assert_eq!(
            examples(text, "<c>x</c>", &["<c>x</c>", "<c>y</c>"]),
            Some(9..29)
        );
        assert_eq!(examples(text, "<c>y</c>", &["<c>x</c>", "<c>y</c>"]), None);
        let open = "~~~~\n<c>x</c>\n~~~\n";
        assert_eq!(
            examples(open, "<c>x</c>", &["<c>x</c>"]),
            Some(0..open.len())
        );
    }

    #[test]
    fn fences_inside_calls_are_ignored() {
        let text = "<c>```</c>\n<c>z</c>";
        assert_eq!(
            examples(text, "<c>z</c>", &["<c>```</c>", "<c>z</c>"]),
            None
        );
        let text = "<c>a\n```\nb</c>\n<c>z</c>";
        assert_eq!(
            examples(text, "<c>z</c>", &["<c>a\n```\nb</c>", "<c>z</c>"]),
            None
        );
    }

    #[test]
    fn inline_code_and_block_quote() {
        let text = "Use `<c>x</c>` like this, or ``<c>y</c>`.";
        assert_eq!(examples(text, "<c>x</c>", &[]), Some(4..14));
        assert_eq!(examples(text, "<c>y</c>", &[]), None);
        let text = "> <c>x</c>\n<c>y</c>";
        assert_eq!(examples(text, "<c>x</c>", &[]), Some(0..11));
        assert_eq!(examples(text, "<c>y</c>", &[]), None);
        // Four spaces of indentation are not a fence or a quote.
        let text = "    ```\n<c>x</c>";
        assert_eq!(examples(text, "<c>x</c>", &[]), None);
    }
}
