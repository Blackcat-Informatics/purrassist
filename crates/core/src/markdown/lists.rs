// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! List-item splices with exact outside-source preservation.
use super::{Block, BlockKind, Document, line, lines, sections, size};
use crate::{
    Governor, Outcome, StopReason,
    contract::Resource,
    edits::{EditPlan, TextEdit},
};

/// A source-bound operation on a selected root or nested list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListItemEdit {
    /// Insert a sibling before the selected item, with an explicit marker.
    Before {
        /// `-`, `+`, `*`, or one to nine digits followed by `.` or `)`.
        marker: String,
        /// Logical item Markdown, without its list marker or outer prefixes.
        body: String,
    },
    /// Insert a sibling after the selected item; existing numbering stays exact.
    After {
        /// `-`, `+`, `*`, or one to nine digits followed by `.` or `)`.
        marker: String,
        /// Logical item Markdown, without its list marker or outer prefixes.
        body: String,
    },
    /// Replace the selected item's body, retaining its marker spelling.
    Replace {
        /// Logical item Markdown; may include a task checkbox and nested blocks.
        body: String,
    },
    /// Remove the item, retaining enclosing containers and outside bytes.
    Remove,
    /// Relocate the item to a new position among the same siblings,
    /// [`Vec::remove`]-then-[`Vec::insert`] semantics: `to` is the item's
    /// zero-based index in the *final* (still same-length) sibling order.
    Move {
        /// Final zero-based sibling index, `0..siblings.len()`.
        to: usize,
    },
}

impl Document {
    /// Insert, replace, or remove a root or nested list item.
    ///
    /// New items use one space after their marker and continuation indentation
    /// matching that marker's width. Outer prefixes are generated for nested lines.
    /// Existing siblings are never renumbered. The anchor's line ending is used
    /// when an additional separator is needed; unterminated EOF stays unterminated.
    /// Removing a container's only item can leave a blank container line.
    /// Every proposal is reparsed to check the intended item, outside siblings,
    /// and ancestors. Plans retain their source identity and support exact undo.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::{Document, ListItemEdit}};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("> - hot\n", &mut g)?;
    /// let plan = doc.edit_list_item(&[0, 0], &ListItemEdit::After {
    ///     marker: "-".into(), body: "iced".into(),
    /// }, &mut g)?;
    /// assert_eq!(plan.edits[0].replacement, "> - iced\n");
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects invalid/non-item paths, invalid markers, bodies that do not form
    /// exactly one item, and changes to surrounding structure or ancestor identity.
    /// Resource exhaustion and cancellation return no partial plan.
    pub fn edit_list_item(
        &self,
        path: &[usize],
        edit: &ListItemEdit,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        let anchor = self.block_at(path)?;
        if !matches!(anchor.kind, BlockKind::ListItem { .. }) {
            return Err(invalid("selected block is not a list item"));
        }
        let index = path[path.len() - 1];
        let siblings = if path.len() == 1 {
            &self.blocks
        } else {
            &self.block_at(&path[..path.len() - 1])?.children
        };
        if let ListItemEdit::Move { to } = edit {
            return move_item(self, path, siblings, index, *to, governor);
        }
        governor.charge(Resource::Work, size(self.source.len()).saturating_mul(32))?;
        governor.charge(
            Resource::ScratchBytes,
            size(self.source.len())
                .saturating_mul(192)
                .saturating_add(512),
        )?;
        let proposal = prepare(self, path, anchor, siblings, index, edit, governor)?;
        if self.source[proposal.change.range.clone()] == proposal.change.replacement {
            return Ok(EditPlan {
                source: self.digest,
                edits: Vec::new(),
            });
        }
        let mut proposed = self.source.clone();
        proposed.replace_range(proposal.change.range.clone(), &proposal.change.replacement);
        let updated = Self::parse(&proposed, governor)?;
        validate(self, &updated, path, &proposal)?;
        Ok(EditPlan {
            source: self.digest,
            edits: vec![proposal.change],
        })
    }

    /// Renumber a contiguous run of ordered-list siblings (every immediately
    /// adjacent sibling, in both directions from `path`, that is also an
    /// ordered item) sequentially from `start`, keeping each item's `.`/`)`
    /// delimiter, checkbox state, and body exact.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, markdown::Document};
    /// let mut g = Governor::new(Budget::default());
    /// let doc = Document::parse("1. a\n5. b\n9. c\n", &mut g)?;
    /// let plan = doc.renumber_list(&[0], 1, &mut g)?;
    /// assert_eq!(plan.edits.len(), 2); // "1." is already correct; "5." and "9." change
    /// # Ok::<(), purrassist_core::StopReason>(())
    /// ```
    ///
    /// # Errors
    /// Rejects a missing/non-ordered-list selection, an item missing its
    /// numeric marker, and numbering overflow. Any proposal that changes an
    /// item's body, checkbox state, or a block outside the run is rejected.
    /// Resource exhaustion and cancellation propagate without a partial plan.
    pub fn renumber_list(
        &self,
        path: &[usize],
        start: u64,
        governor: &mut Governor,
    ) -> Result<EditPlan, StopReason> {
        governor.charge(Resource::Work, size(path.len()).saturating_add(1))?;
        governor.charge(Resource::Depth, size(path.len().saturating_sub(1)))?;
        let anchor = self.block_at(path)?;
        if !matches!(
            anchor.kind,
            BlockKind::ListItem {
                ordinal: Some(_),
                ..
            }
        ) {
            return Err(invalid("selected block is not an ordered list item"));
        }
        let index = path[path.len() - 1];
        let siblings = if path.len() == 1 {
            &self.blocks
        } else {
            &self.block_at(&path[..path.len() - 1])?.children
        };
        governor.charge(Resource::Work, size(siblings.len()).saturating_mul(8))?;
        governor.charge(
            Resource::ScratchBytes,
            size(siblings.len()).saturating_mul(64).saturating_add(256),
        )?;
        let ordered = |block: &Block| {
            matches!(
                block.kind,
                BlockKind::ListItem {
                    ordinal: Some(_),
                    ..
                }
            )
        };
        let mut lo = index;
        while lo > 0 && ordered(&siblings[lo - 1]) {
            lo -= 1;
        }
        let mut hi = index + 1;
        while hi < siblings.len() && ordered(&siblings[hi]) {
            hi += 1;
        }
        let mut edits = Vec::with_capacity(hi - lo);
        for (offset, item) in siblings[lo..hi].iter().enumerate() {
            let number = start
                .checked_add(offset as u64)
                .ok_or_else(|| invalid("renumbering overflows"))?;
            let first_line_range = item.lines[0].clone();
            let first_line = line(&self.source, &first_line_range);
            let leading = first_line.len() - first_line.trim_start_matches([' ', '\t']).len();
            let digits = first_line[leading..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            if digits == 0 {
                return Err(invalid("ordered list item is missing its numeric marker"));
            }
            let digit_start = first_line_range.start + leading;
            let old = &self.source[digit_start..digit_start + digits];
            let new = number.to_string();
            if old != new {
                edits.push(TextEdit {
                    range: digit_start..digit_start + digits,
                    replacement: new,
                });
            }
        }
        let plan = EditPlan {
            source: self.digest,
            edits,
        };
        if plan.edits.is_empty() {
            return Ok(plan);
        }
        let applied = plan.apply_governed(&self.source, governor);
        let Outcome::Resolved(applied) = applied.outcome else {
            return Err(applied.receipt.stopped);
        };
        let updated = Self::parse(&applied.text, governor)?;
        validate_renumbering(path, siblings, &updated, lo, hi, start)?;
        Ok(plan)
    }
}
/// Check that renumbering [`Document::renumber_list`]'s selected run touched
/// nothing else: every preceding/following sibling stays exact (only
/// shifted, for the following ones, by the digit-width change), and every
/// renumbered item keeps its exact body and checkbox state.
fn validate_renumbering(
    path: &[usize],
    siblings: &[Block],
    updated: &Document,
    lo: usize,
    hi: usize,
    start: u64,
) -> Result<(), StopReason> {
    let updated_siblings = if path.len() == 1 {
        &updated.blocks
    } else {
        &updated.block_at(&path[..path.len() - 1])?.children
    };
    if updated_siblings.len() < siblings.len() || updated_siblings[..lo] != siblings[..lo] {
        return Err(invalid("renumbering changed preceding blocks"));
    }
    for (offset, item) in siblings[lo..hi].iter().enumerate() {
        let BlockKind::ListItem { checked, .. } = item.kind else {
            unreachable!("already matched ListItem above")
        };
        let expected = BlockKind::ListItem {
            ordinal: Some(start + offset as u64),
            checked,
        };
        if updated_siblings[lo + offset].kind != expected {
            return Err(invalid(
                "renumbering changed an item's body or checkbox state",
            ));
        }
    }
    let old_end = siblings[hi - 1].span.end;
    let new_end = updated_siblings[hi - 1].span.end;
    if !sections::same_tail(&siblings[hi..], &updated_siblings[hi..], old_end, new_end) {
        return Err(invalid("renumbering changed following blocks"));
    }
    Ok(())
}
struct Proposal {
    change: TextEdit,
    /// Number of old and new items in the affected sibling window.
    old_count: usize,
    new_count: usize,
    index: usize,
    intended: Option<Document>,
    /// Insertion at unterminated EOF adds this separator to the prior item.
    eof_separator: usize,
    placeholder: bool,
}
fn prepare(
    doc: &Document,
    path: &[usize],
    anchor: &Block,
    siblings: &[Block],
    index: usize,
    edit: &ListItemEdit,
    g: &mut Governor,
) -> Result<Proposal, StopReason> {
    let newline = ending(&doc.source[anchor.lines[0].clone()]).unwrap_or("\n");
    if matches!(edit, ListItemEdit::Remove) {
        return removal(doc, path, anchor, siblings, index, newline);
    }
    debug_assert!(
        !matches!(edit, ListItemEdit::Move { .. }),
        "Move is handled before prepare() is called"
    );
    let outer = sections::prefix(doc, path, g)?;
    let inherited = if matches!(edit, ListItemEdit::Replace { .. }) {
        marker(doc, anchor)
    } else {
        String::new()
    };
    let (marker, body) = match edit {
        ListItemEdit::Before { marker, body } | ListItemEdit::After { marker, body } => {
            (marker.as_str(), body.as_str())
        }
        ListItemEdit::Replace { body } => (inherited.as_str(), body.as_str()),
        ListItemEdit::Remove | ListItemEdit::Move { .. } => unreachable!("handled earlier"),
    };
    validate_marker(marker)?;
    g.charge(
        Resource::InputBytes,
        size(body.len()).saturating_add(size(marker.len())),
    )?;
    g.charge(
        Resource::Work,
        size(body.len()).saturating_mul(16).saturating_add(1),
    )?;
    g.charge(
        Resource::ScratchBytes,
        size(body.len()).saturating_mul(128).saturating_add(512),
    )?;
    let first = line(&doc.source, &anchor.lines[0]);
    let leading = first.len() - first.trim_start_matches([' ', '\t']).len();
    let label = format!("{}{marker}", &first[..leading]);
    let at_eof = anchor.span.end == doc.source.len();
    let terminated = doc.source[..anchor.span.end].ends_with(['\r', '\n']);
    let final_newline = matches!(edit, ListItemEdit::Before { .. }) || !at_eof || terminated;
    let local = item(&label, body, "", newline, final_newline, g)?;
    let intended = Document::parse(&local, g)?;
    if intended.blocks.len() != 1 || !matches!(intended.blocks[0].kind, BlockKind::ListItem { .. })
    {
        return Err(invalid("body must form exactly one list item"));
    }
    let mut replacement = item(&label, body, &outer, newline, final_newline, g)?;
    let (range, old_count, new_index, eof_separator) = match edit {
        ListItemEdit::Before { .. } => {
            replacement.push_str(&outer);
            (anchor.span.start..anchor.span.start, 0, index, 0)
        }
        ListItemEdit::After { .. } => {
            replacement.insert_str(0, &outer);
            let extra = if terminated {
                0
            } else {
                replacement.insert_str(0, newline);
                newline.len()
            };
            (anchor.span.end..anchor.span.end, 0, index + 1, extra)
        }
        ListItemEdit::Replace { .. } => (anchor.span.clone(), 1, index, 0),
        ListItemEdit::Remove | ListItemEdit::Move { .. } => unreachable!("handled earlier"),
    };
    Ok(Proposal {
        change: TextEdit { range, replacement },
        old_count,
        new_count: 1,
        index: new_index,
        intended: Some(intended),
        eof_separator,
        placeholder: false,
    })
}
fn removal(
    doc: &Document,
    path: &[usize],
    anchor: &Block,
    siblings: &[Block],
    index: usize,
    newline: &str,
) -> Result<Proposal, StopReason> {
    let parent = if path.len() > 1 {
        Some(doc.block_at(&path[..path.len() - 1])?)
    } else {
        None
    };
    let first_line = parent.is_some_and(|parent| {
        line_start(&doc.source, parent.span.start) == line_start(&doc.source, anchor.span.start)
    });
    let (range, replacement, placeholder) = if first_line {
        siblings.get(index + 1).map_or_else(
            || {
                let replacement = if doc.source[..anchor.span.end].ends_with(['\r', '\n']) {
                    newline.into()
                } else {
                    String::new()
                };
                (anchor.span.clone(), replacement, true)
            },
            |next| (anchor.span.start..next.span.start, String::new(), false),
        )
    } else {
        (
            line_start(&doc.source, anchor.span.start)..anchor.span.end,
            String::new(),
            false,
        )
    };
    Ok(Proposal {
        change: TextEdit { range, replacement },
        old_count: 1,
        new_count: 0,
        index,
        intended: None,
        eof_separator: 0,
        placeholder,
    })
}
fn item(
    marker: &str,
    body: &str,
    outer: &str,
    newline: &str,
    final_newline: bool,
    g: &mut Governor,
) -> Result<String, StopReason> {
    let body_lines = lines(body);
    let prefix_size = size(outer.len())
        .saturating_add(size(marker.len()))
        .saturating_add(1);
    let growth = prefix_size.saturating_mul(size(body_lines.len()).saturating_add(2));
    g.charge(Resource::Work, growth)?;
    g.charge(Resource::ScratchBytes, growth.saturating_mul(4))?;
    let mut text = format!("{marker} ");
    let continuation = format!("{outer}{}", " ".repeat(marker.len() + 1));
    for (index, line) in body_lines.iter().enumerate() {
        if index != 0 {
            text.push_str(&continuation);
        }
        text.push_str(&body[line.clone()]);
    }
    if final_newline && !text.ends_with(['\r', '\n']) {
        text.push_str(newline);
    }
    if !final_newline {
        if text.ends_with("\r\n") {
            text.truncate(text.len() - 2);
        } else if text.ends_with(['\r', '\n']) {
            text.pop();
        }
    }
    Ok(text)
}
/// Relocate the sibling at `index` to final position `to`, [`Vec::remove`]/
/// [`Vec::insert`] semantics, by rewriting the whole sibling window as one
/// edit (each surviving sibling's own raw source bytes, reordered) and
/// reparse-checking the result via the same [`Proposal`]/[`validate`] path
/// every other list-item edit uses.
///
/// A nested sibling's own leading container-prefix bytes (the enclosing
/// quote's `"> "`, a further-nested list's continuation indent, ...) sit
/// *outside* its `span` (see [`Block::span`]'s documentation), in the gap
/// between the previous sibling's end and this one's start; a root-level
/// sibling has no such gap. Reordering must carry that separator along by
/// *position*, not by moved identity: the exact bytes that originally
/// separated position `p - 1` from position `p` still separate them after
/// reordering, regardless of which sibling now occupies each slot. This
/// keeps every sibling that does not change position byte-for-byte
/// unchanged (rather than resynthesizing a canonical separator that could
/// subtly differ, e.g. a blank quote line's bare `">"` versus `"> "`) and
/// keeps the whole window's total length exactly unchanged.
fn move_item(
    doc: &Document,
    path: &[usize],
    siblings: &[Block],
    index: usize,
    to: usize,
    governor: &mut Governor,
) -> Result<EditPlan, StopReason> {
    if to >= siblings.len() {
        return Err(invalid("move target index is out of range"));
    }
    if to == index {
        return Ok(EditPlan {
            source: doc.digest,
            edits: Vec::new(),
        });
    }
    let region = siblings[0].span.start..siblings[siblings.len() - 1].span.end;
    governor.charge(Resource::Work, size(region.len()).saturating_mul(4))?;
    governor.charge(
        Resource::ScratchBytes,
        size(region.len()).saturating_mul(4).saturating_add(512),
    )?;
    let gaps: Vec<&str> = (1..siblings.len())
        .map(|position| &doc.source[siblings[position - 1].span.end..siblings[position].span.start])
        .collect();
    let mut order: Vec<usize> = (0..siblings.len()).collect();
    let moved = order.remove(index);
    order.insert(to, moved);
    let mut replacement = String::with_capacity(region.len());
    for (position, &original) in order.iter().enumerate() {
        if position > 0 {
            replacement.push_str(gaps[position - 1]);
        }
        replacement.push_str(&doc.source[siblings[original].span.clone()]);
    }
    let change = TextEdit {
        range: region,
        replacement,
    };
    let plan = EditPlan {
        source: doc.digest,
        edits: vec![change.clone()],
    };
    let applied = plan.apply_governed(&doc.source, governor);
    let Outcome::Resolved(applied) = applied.outcome else {
        return Err(applied.receipt.stopped);
    };
    let updated = Document::parse(&applied.text, governor)?;
    // A move never changes the sibling count, so `old_count`/`new_count`
    // are always equal here (unlike insert/replace, which legitimately can
    // change how many blocks occupy the window). `intended` is deliberately
    // omitted: parsing `change.replacement` *standalone* is not a reliable
    // oracle for a moved window, because whether a line such as a nested
    // `>` continues an already-open container or starts a new one depends
    // on what precedes the window in the real document, not on the
    // replacement text alone. The reordering itself is instead checked
    // directly below, by kind, position by position.
    let proposal = Proposal {
        new_count: siblings.len(),
        old_count: siblings.len(),
        index: 0,
        intended: None,
        eof_separator: 0,
        placeholder: false,
        change,
    };
    validate(doc, &updated, path, &proposal)?;
    let updated_siblings: &[Block] = if path.len() == 1 {
        updated.blocks()
    } else {
        &updated.block_at(&path[..path.len() - 1])?.children
    };
    if updated_siblings.len() != siblings.len()
        || order
            .iter()
            .enumerate()
            .any(|(position, &original)| updated_siblings[position].kind != siblings[original].kind)
    {
        return Err(invalid(
            "move produced a different sibling arrangement than expected",
        ));
    }
    Ok(plan)
}
fn validate(
    old: &Document,
    new: &Document,
    path: &[usize],
    proposal: &Proposal,
) -> Result<(), StopReason> {
    let mut before = old.blocks.as_slice();
    let mut after = new.blocks.as_slice();
    for &index in &path[..path.len() - 1] {
        let a = before
            .get(index)
            .ok_or_else(|| invalid("missing ancestor"))?;
        let b = after
            .get(index)
            .ok_or_else(|| invalid("edit removes ancestor"))?;
        if before[..index] != after[..index]
            || a.kind != b.kind
            || a.span.start != b.span.start
            || map(a.span.end, proposal) != b.span.end
            || !tail(&before[index + 1..], &after[index + 1..], proposal)
        {
            return Err(invalid("edit changes an ancestor or outside sibling"));
        }
        before = &a.children;
        after = &b.children;
    }
    let index = proposal.index;
    let mut prefix = before[..index].to_vec();
    if proposal.eof_separator != 0 {
        for block in &mut prefix {
            extend_eof(block, old.source.len(), proposal.eof_separator);
        }
    }
    if after.len() < index || prefix != after[..index] {
        return Err(invalid("edit changes preceding list siblings"));
    }
    let count = if proposal.placeholder
        && after
            .get(index)
            .is_some_and(|block| matches!(block.kind, BlockKind::Blank))
    {
        1
    } else {
        proposal.new_count
    };
    if after.len() < index + count
        || !tail(
            &before[index + proposal.old_count..],
            &after[index + count..],
            proposal,
        )
    {
        return Err(invalid("edit changes following list siblings"));
    }
    if let Some(intended) = &proposal.intended
        && !sections::same_shape(
            &intended.source,
            &intended.blocks,
            &new.source,
            &after[index..index + count],
        )
    {
        return Err(invalid(
            "inserted item differs from its intended structure or text",
        ));
    }
    Ok(())
}
fn tail(old: &[Block], new: &[Block], proposal: &Proposal) -> bool {
    let mut expected = old.to_vec();
    for block in &mut expected {
        sections::shift(
            block,
            proposal.change.range.end,
            proposal.change.range.start + proposal.change.replacement.len(),
        );
    }
    expected == new
}
const fn map(offset: usize, proposal: &Proposal) -> usize {
    offset - proposal.change.range.end
        + proposal.change.range.start
        + proposal.change.replacement.len()
}
fn extend_eof(block: &mut Block, eof: usize, extra: usize) {
    if block.span.end == eof {
        block.span.end += extra;
    }
    for line in &mut block.lines {
        if line.end == eof {
            line.end += extra;
        }
    }
    if let BlockKind::CodeFence { content, .. } = &mut block.kind
        && content.end == eof
    {
        content.end += extra;
    }
    for child in &mut block.children {
        extend_eof(child, eof, extra);
    }
}
fn marker(doc: &Document, anchor: &Block) -> String {
    let text = line(&doc.source, &anchor.lines[0]).trim_start_matches([' ', '\t']);
    text.split([' ', '\t']).next().unwrap_or("").into()
}
fn validate_marker(marker: &str) -> Result<(), StopReason> {
    if matches!(marker, "-" | "+" | "*")
        || ((2..=10).contains(&marker.len())
            && marker.ends_with(['.', ')'])
            && marker.as_bytes()[..marker.len() - 1]
                .iter()
                .all(u8::is_ascii_digit))
    {
        Ok(())
    } else {
        Err(invalid("invalid list marker"))
    }
}
fn line_start(source: &str, offset: usize) -> usize {
    source[..offset]
        .rfind(['\r', '\n'])
        .map_or(0, |position| position + 1)
}
fn ending(text: &str) -> Option<&'static str> {
    if text.ends_with("\r\n") {
        Some("\r\n")
    } else if text.ends_with('\r') {
        Some("\r")
    } else if text.ends_with('\n') {
        Some("\n")
    } else {
        None
    }
}
fn invalid(message: &str) -> StopReason {
    StopReason::InvalidInput(message.into())
}
