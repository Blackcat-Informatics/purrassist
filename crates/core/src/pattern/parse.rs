// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The parser: full ECMA-262 `u`-mode syntax (this engine has no flags, so
//! every pattern is parsed as if the `u` flag were always set — never the
//! looser Annex B grammar). Named groups, `\k<name>`, `\u{...}`, surrogate
//! pair combination, `\p{...}`/`\P{...}` for `General_Category`, `Script`,
//! `Script_Extensions`, and every ECMA-262 binary Unicode property. Lone
//! braces, class ranges ending in a class escape, and identity escapes of
//! anything but a `SyntaxCharacter` or `/` are all `u`-mode `SyntaxError`s.
use super::ast::{Node, Repetition};
use super::unicode;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason,
    format_scratch_governed, saturating_size as size,
};
use std::rc::Rc;

use super::{PatternCompileError, PatternError};

struct NamedGroup {
    name: ScratchString,
    indices: ScratchVec<usize>,
    paths: ScratchVec<ScratchVec<(u32, u32)>>,
}
impl ScratchCharge for NamedGroup {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 120);
        120
    };
}

struct Parser<'g> {
    chars: ScratchVec<char>,
    position: usize,
    captures: usize,
    governor: &'g mut Governor,
    /// Source-ordered capture indices and mutually exclusive definition paths.
    names: ScratchVec<NamedGroup>,
    path: ScratchVec<(u32, u32)>,
    next_alt_id: u32,
    backrefs: ScratchVec<(usize, usize)>,
}

fn syntax<T>(
    offset: usize,
    message: core::fmt::Arguments<'_>,
    governor: &mut Governor,
) -> Result<T, PatternCompileError> {
    // Fixed formatter dispatch and numeric conversions; variable text scans
    // are admitted at their call sites before constructing these arguments.
    governor.charge(Resource::Work, 128)?;
    let message = format_scratch_governed(message, governor)?;
    governor.check()?;
    Err(PatternCompileError::Syntax(PatternError {
        offset,
        message: message.into_string(),
    }))
}

pub(super) fn parse(
    source: &str,
    governor: &mut Governor,
) -> Result<(Node, usize), PatternCompileError> {
    governor.charge(Resource::Work, size(source.len()))?;
    let mut chars = ScratchVec::new(governor);
    chars.extend(source.chars())?;
    let names = scan_group_names(&chars, governor)?;
    let mut parser = Parser {
        chars,
        position: 0,
        captures: 0,
        names,
        path: ScratchVec::new(governor),
        next_alt_id: 0,
        backrefs: ScratchVec::new(governor),
        governor,
    };
    let node = parser.alternate(0)?;
    if parser.position != parser.chars.len() {
        return parser.fail("unexpected closing group");
    }
    for (offset, index) in &parser.backrefs {
        parser.governor.charge(Resource::Work, 1)?;
        if *index > parser.captures {
            return syntax(
                *offset,
                format_args!("backreference to non-existent group {index}"),
                parser.governor,
            );
        }
    }
    parser.governor.check()?;
    Ok((node, parser.captures))
}

/// One character class atom: either a single scalar (usable as one end of a
/// `-` range) or a whole predicate's interval set (never usable as a range
/// end — a class range ending in a class escape is a `u`-mode `SyntaxError`).
enum ClassAtom {
    Literal(u32),
    Set(Vec<(u32, u32)>),
}

const fn identity_escape_allowed(character: char) -> bool {
    matches!(
        character,
        '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/'
    )
}
fn ascii_digit_ranges() -> Vec<(u32, u32)> {
    vec![(0x30, 0x39)]
}
fn ascii_word_ranges() -> Vec<(u32, u32)> {
    vec![(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)]
}
/// The ECMA-262 `WhiteSpace`/`LineTerminator` set `\s` matches; identical
/// under every mode (unlike `\p{White_Space}`, which is the Unicode
/// property of the same name and can differ).
fn ascii_space_ranges() -> Vec<(u32, u32)> {
    vec![
        (0x9, 0xD),
        (0x20, 0x20),
        (0xA0, 0xA0),
        (0x1680, 0x1680),
        (0x2000, 0x200A),
        (0x2028, 0x2029),
        (0x202F, 0x202F),
        (0x205F, 0x205F),
        (0x3000, 0x3000),
        (0xFEFF, 0xFEFF),
    ]
}
fn dot_ranges() -> Vec<(u32, u32)> {
    unicode::negate(&[(0xA, 0xA), (0xD, 0xD), (0x2028, 0x2028), (0x2029, 0x2029)])
}

fn class_from_ranges(ranges: Vec<(u32, u32)>, negated: bool) -> Node {
    // `unicode::negate`/`complement_within` walk their `ranges` argument with
    // a single forward-advancing iterator, so it must already be sorted and
    // merged; a character class built from several atoms (e.g. `[^\s\S]`,
    // where `\s`'s ranges and `\S`'s already-negated ranges are simply
    // concatenated by `class()`) is not sorted on its own.
    let merged = unicode::merge(ranges);
    let resolved = if negated {
        unicode::negate(&merged)
    } else {
        merged
    };
    let clipped = unicode::clip_to_scalars(resolved);
    if clipped.is_empty() {
        Node::NoMatch
    } else {
        Node::Class(Rc::new(clipped))
    }
}

/// Scan names before parsing so references can resolve forward capture indices.
fn scan_group_names(
    chars: &[char],
    governor: &mut Governor,
) -> Result<ScratchVec<NamedGroup>, PatternCompileError> {
    let mut names: ScratchVec<NamedGroup> = ScratchVec::new(governor);
    let mut captures = 0_usize;
    let mut in_class = false;
    let mut index = 0;
    while index < chars.len() {
        governor.charge(Resource::Work, 1)?;
        match chars[index] {
            '\\' => index += 2,
            '[' if !in_class => {
                in_class = true;
                index += 1;
            }
            ']' if in_class => {
                in_class = false;
                index += 1;
            }
            '(' if !in_class => {
                if chars.get(index + 1) == Some(&'?') {
                    if chars.get(index + 2) == Some(&'<')
                        && !matches!(chars.get(index + 3), Some('=' | '!'))
                    {
                        let start = index + 3;
                        let mut end = start;
                        while chars.get(end).is_some_and(|c| *c != '>') {
                            governor.charge(Resource::Work, 1)?;
                            end += 1;
                        }
                        if chars.get(end) != Some(&'>') {
                            return syntax(
                                start,
                                format_args!("unterminated group name"),
                                governor,
                            );
                        }
                        let name = copy_chars(&chars[start..end], governor)?;
                        validate_group_name(name.as_str(), start, governor)?;
                        captures = captures
                            .checked_add(1)
                            .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
                        let existing = find_name(&names, name.as_str(), governor)?;
                        if let Some(existing) = existing {
                            names.as_mut_slice()[existing].indices.push(captures)?;
                        } else {
                            let mut indices = ScratchVec::new(governor);
                            indices.push(captures)?;
                            names.push(NamedGroup {
                                name,
                                indices,
                                paths: ScratchVec::new(governor),
                            })?;
                        }
                        index = end + 1;
                    } else {
                        index += 1;
                    }
                } else {
                    captures = captures
                        .checked_add(1)
                        .ok_or_else(|| governor.stop_resource(Resource::ScratchBytes))?;
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }
    Ok(names)
}

fn copy_chars(chars: &[char], governor: &mut Governor) -> Result<ScratchString, StopReason> {
    governor.charge(Resource::Work, size(chars.len()).saturating_add(1))?;
    let mut text = ScratchString::new(governor);
    for &character in chars {
        text.push(character)?;
    }
    Ok(text)
}

fn find_name(
    names: &[NamedGroup],
    name: &str,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    for (index, group) in names.iter().enumerate() {
        governor.charge(
            Resource::Work,
            size(group.name.len().min(name.len())).saturating_add(1),
        )?;
        if group.name.as_str() == name {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

fn validate_group_name(
    name: &str,
    offset: usize,
    governor: &mut Governor,
) -> Result<(), PatternCompileError> {
    governor.charge(Resource::Work, size(name.len()).saturating_add(1))?;
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        governor.charge(
            Resource::Work,
            size(name.len()).saturating_mul(16).saturating_add(64),
        )?;
        return syntax(
            offset,
            format_args!("invalid group name {name:?}"),
            governor,
        );
    };
    // Four binary searches: two fixed aliases and two property lookups.
    // Each generated table has at most 128 entries and fixed names have
    // at most 11 bytes: at most 4 * 9 * 12 comparison/read units, below 512.
    governor.charge(Resource::Work, 512)?;
    let id_start = unicode::identifier_start();
    let id_continue = unicode::identifier_continue();
    // Each membership query is logarithmic in one fixed generated table.
    governor.charge(
        Resource::Work,
        u64::from(id_start.len().checked_ilog2().unwrap_or(0) + 2),
    )?;
    let starts =
        first == '$' || first == '_' || unicode::interval_contains(id_start, u32::from(first));
    if !starts {
        governor.charge(
            Resource::Work,
            size(name.len()).saturating_mul(16).saturating_add(64),
        )?;
        return syntax(
            offset,
            format_args!("invalid group name {name:?}"),
            governor,
        );
    }
    for character in chars {
        governor.charge(
            Resource::Work,
            u64::from(id_continue.len().checked_ilog2().unwrap_or(0) + 2),
        )?;
        if character != '$' && !unicode::interval_contains(id_continue, u32::from(character)) {
            governor.charge(
                Resource::Work,
                size(name.len()).saturating_mul(16).saturating_add(64),
            )?;
            return syntax(
                offset,
                format_args!("invalid group name {name:?}"),
                governor,
            );
        }
    }
    Ok(())
}

fn paths_diverge(
    p: &[(u32, u32)],
    q: &[(u32, u32)],
    governor: &mut Governor,
) -> Result<bool, StopReason> {
    for index in 0..p.len().min(q.len()) {
        governor.charge(Resource::Work, 1)?;
        if p[index] != q[index] {
            return Ok(p[index].0 == q[index].0 && p[index].1 != q[index].1);
        }
    }
    Ok(false)
}

impl Parser<'_> {
    fn current(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }
    fn advance(&mut self, count: usize) -> Result<(), StopReason> {
        self.governor.charge(Resource::Work, size(count))?;
        self.position = self
            .position
            .checked_add(count)
            .ok_or_else(|| self.governor.stop_resource(Resource::Work))?;
        Ok(())
    }
    fn next(&mut self) -> Result<Option<char>, StopReason> {
        let Some(character) = self.current() else {
            return Ok(None);
        };
        self.advance(1)?;
        Ok(Some(character))
    }
    fn fail<T>(&mut self, message: &str) -> Result<T, PatternCompileError> {
        self.fail_at(self.position, message)
    }
    fn fail_at<T>(&mut self, offset: usize, message: &str) -> Result<T, PatternCompileError> {
        self.governor
            .charge(Resource::Work, size(message.len()).saturating_add(1))?;
        syntax(offset, format_args!("{message}"), self.governor)
    }
    fn alternate(&mut self, depth: usize) -> Result<Node, PatternCompileError> {
        self.governor
            .charge(Resource::Depth, u64::try_from(depth).unwrap_or(u64::MAX))?;
        self.governor.charge(Resource::Work, 1)?;
        let alt_id = self.next_alt_id;
        self.next_alt_id = self
            .next_alt_id
            .checked_add(1)
            .ok_or_else(|| self.governor.stop_resource(Resource::Work))?;
        let mut choices = Vec::new();
        let mut branch = 0u32;
        loop {
            self.path.push((alt_id, branch))?;
            let choice = self.sequence(depth);
            self.path.pop();
            choices.push(choice?);
            if self.current() == Some('|') {
                self.advance(1)?;
                branch = branch
                    .checked_add(1)
                    .ok_or_else(|| self.governor.stop_resource(Resource::Work))?;
            } else {
                break;
            }
        }
        if choices.len() == 1 {
            Ok(choices.remove(0))
        } else {
            Ok(Node::Alternate(choices))
        }
    }
    fn sequence(&mut self, depth: usize) -> Result<Node, PatternCompileError> {
        self.governor.charge(Resource::Work, 1)?;
        let mut items = Vec::new();
        while !matches!(self.current(), None | Some('|' | ')')) {
            let atom = self.atom(depth)?;
            let repeat = match self.current() {
                Some('*') => {
                    self.advance(1)?;
                    Some((0, None))
                }
                Some('+') => {
                    self.advance(1)?;
                    Some((1, None))
                }
                Some('?') => {
                    self.advance(1)?;
                    Some((0, Some(1)))
                }
                Some('{') if self.looks_like_quantifier()? => {
                    self.advance(1)?;
                    let min = self.count()?;
                    let max = if self.current() == Some(',') {
                        self.advance(1)?;
                        if self.current() == Some('}') {
                            None
                        } else {
                            Some(self.count()?)
                        }
                    } else {
                        Some(min)
                    };
                    if self.next()? != Some('}') || max.is_some_and(|max| max < min) {
                        return self.fail("invalid repetition range");
                    }
                    Some((min, max))
                }
                _ => None,
            };
            if let Some((min, max)) = repeat {
                items.push(Node::Repeat(Box::new(Repetition {
                    inner: atom,
                    min,
                    max,
                    greedy: self.current() != Some('?'),
                })));
                if self.current() == Some('?') {
                    self.advance(1)?;
                }
            } else {
                items.push(atom);
            }
        }
        match items.len() {
            0 => Ok(Node::Empty),
            1 => Ok(items.remove(0)),
            _ => Ok(Node::Concat(items)),
        }
    }
    /// A lone `{` (not the start of a valid `{n}`/`{n,}`/`{n,m}` quantifier)
    /// is a `u`-mode `SyntaxError`, not a literal — checked by lookahead so
    /// `sequence` never has to backtrack over a partially consumed `{`.
    fn looks_like_quantifier(&mut self) -> Result<bool, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        let mut index = self.position + 1;
        if !self.chars.get(index).is_some_and(char::is_ascii_digit) {
            return Ok(false);
        }
        while self.chars.get(index).is_some_and(char::is_ascii_digit) {
            self.governor.charge(Resource::Work, 1)?;
            index += 1;
        }
        if self.chars.get(index) == Some(&'}') {
            return Ok(true);
        }
        if self.chars.get(index) != Some(&',') {
            return Ok(false);
        }
        index += 1;
        while self.chars.get(index).is_some_and(char::is_ascii_digit) {
            self.governor.charge(Resource::Work, 1)?;
            index += 1;
        }
        Ok(self.chars.get(index) == Some(&'}'))
    }
    fn count(&mut self) -> Result<usize, PatternCompileError> {
        let mut count = 0_usize;
        let start = self.position;
        while let Some(digit) = self.current().and_then(|character| character.to_digit(10)) {
            self.governor.charge(Resource::Work, 1)?;
            let Some(next) = count
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::try_from(digit).ok()?))
            else {
                return self.fail("repetition exceeds address space");
            };
            count = next;
            self.advance(1)?;
        }
        if start == self.position {
            return self.fail("expected a repetition count");
        }
        Ok(count)
    }
    fn atom(&mut self, depth: usize) -> Result<Node, PatternCompileError> {
        self.governor.charge(Resource::Work, 1)?;
        match self.next()? {
            Some('^') => Ok(Node::Start),
            Some('$') => Ok(Node::End),
            Some('.') => Ok(class_from_ranges(dot_ranges(), false)),
            Some('[') => self.class(),
            Some('\\') => self.escape(false),
            Some('(') => self.group(depth),
            Some('*' | '+' | '?' | '{' | '}') => self.fail("quantifier without an atom"),
            Some(character) => Ok(literal(character)),
            None => self.fail("expected an atom"),
        }
    }
    fn group(&mut self, depth: usize) -> Result<Node, PatternCompileError> {
        let mut capture = true;
        let mut look = None;
        let mut name = None;
        if self.current() == Some('?') {
            self.advance(1)?;
            capture = false;
            match self.next()? {
                Some(':') => {}
                Some('=') => look = Some((true, false)),
                Some('!') => look = Some((false, false)),
                Some('<') => match self.current() {
                    Some('=') => {
                        self.advance(1)?;
                        look = Some((true, true));
                    }
                    Some('!') => {
                        self.advance(1)?;
                        look = Some((false, true));
                    }
                    _ => {
                        capture = true;
                        let start = self.position;
                        let mut text = ScratchString::new(self.governor);
                        while self.current().is_some_and(|c| c != '>') {
                            text.push(self.next()?.expect("checked by while"))?;
                        }
                        if self.next()? != Some('>') {
                            return self.fail("unterminated group name");
                        }
                        name = Some((text, start));
                    }
                },
                _ => return self.fail("invalid group prefix"),
            }
        }
        let index = if capture {
            self.captures = self
                .captures
                .checked_add(1)
                .ok_or_else(|| self.governor.stop_resource(Resource::ScratchBytes))?;
            self.captures
        } else {
            0
        };
        if let Some((group_name, start)) = &name {
            let record = find_name(&self.names, group_name.as_str(), self.governor)?
                .expect("preliminary scan records every named capturing group");
            for prior in &self.names[record].paths {
                self.governor.charge(Resource::Work, 1)?;
                if !paths_diverge(prior, &self.path, self.governor)? {
                    return self.fail_at(
                        *start,
                        "duplicate capturing group name outside mutually exclusive alternatives",
                    );
                }
            }
            self.governor
                .charge(Resource::Work, size(self.path.len()))?;
            let mut path = ScratchVec::new(self.governor);
            path.extend(self.path.iter().copied())?;
            self.names.as_mut_slice()[record].paths.push(path)?;
        }
        let depth = depth
            .checked_add(1)
            .ok_or_else(|| self.governor.stop_resource(Resource::Depth))?;
        let inner = self.alternate(depth)?;
        if self.next()? != Some(')') {
            return self.fail("unterminated group");
        }
        if let Some((positive, behind)) = look {
            Ok(Node::Look {
                inner: Box::new(inner),
                positive,
                behind,
            })
        } else if capture {
            Ok(Node::Group(index, Box::new(inner)))
        } else {
            Ok(inner)
        }
    }
    fn escape(&mut self, in_class: bool) -> Result<Node, PatternCompileError> {
        let escape_start = self.position - 1;
        match self.next()? {
            Some('d') => Ok(class_from_ranges(ascii_digit_ranges(), false)),
            Some('D') => Ok(class_from_ranges(ascii_digit_ranges(), true)),
            Some('w') => Ok(class_from_ranges(ascii_word_ranges(), false)),
            Some('W') => Ok(class_from_ranges(ascii_word_ranges(), true)),
            Some('s') => Ok(class_from_ranges(ascii_space_ranges(), false)),
            Some('S') => Ok(class_from_ranges(ascii_space_ranges(), true)),
            Some('b') => {
                if in_class {
                    Ok(literal('\u{8}'))
                } else {
                    Ok(Node::Boundary(true))
                }
            }
            Some('B') if !in_class => Ok(Node::Boundary(false)),
            Some('n') => Ok(literal('\n')),
            Some('r') => Ok(literal('\r')),
            Some('t') => Ok(literal('\t')),
            Some('f') => Ok(literal('\u{c}')),
            Some('v') => Ok(literal('\u{b}')),
            Some('0') if !self.current().is_some_and(|c| c.is_ascii_digit()) => Ok(literal('\0')),
            Some('x') => self.hex_escape(2),
            Some('u') => self.unicode_escape(),
            Some('k') if !in_class => self.named_backref(),
            Some(character @ '1'..='9') if !in_class => {
                let start = self.position - 1;
                let mut index = usize::try_from(u32::from(character) - u32::from('0')).unwrap_or(0);
                while let Some(digit) = self.current().and_then(|character| character.to_digit(10))
                {
                    self.governor.charge(Resource::Work, 1)?;
                    let Some(next) = index
                        .checked_mul(10)
                        .and_then(|index| index.checked_add(usize::try_from(digit).ok()?))
                    else {
                        return self.fail("backreference too large");
                    };
                    index = next;
                    self.advance(1)?;
                }
                self.backrefs.push((start, index))?;
                self.governor.charge(Resource::Work, 1)?;
                Ok(Node::Backref(vec![index]))
            }
            Some(kind @ ('p' | 'P')) => self.property_escape(kind == 'p'),
            Some('c') => {
                let Some(letter) = self.next()?.filter(char::is_ascii_alphabetic) else {
                    return self.fail("control escape requires an ASCII letter");
                };
                Ok(literal(char::from(
                    u8::try_from(u32::from(letter)).expect("ASCII letter") % 32,
                )))
            }
            Some(character) if identity_escape_allowed(character) => Ok(literal(character)),
            _ => self.fail_at(escape_start, "invalid identity escape under Unicode mode"),
        }
    }
    fn named_backref(&mut self) -> Result<Node, PatternCompileError> {
        if self.next()? != Some('<') {
            return self.fail("expected a group name after \\k");
        }
        let start = self.position;
        let mut name = ScratchString::new(self.governor);
        while self.current().is_some_and(|c| c != '>') {
            name.push(self.next()?.expect("checked by while"))?;
        }
        if self.next()? != Some('>') {
            return self.fail("unterminated group name");
        }
        // Existence is checked here directly (by name, not by index), so
        // there is nothing to add to `self.backrefs`'s numeric-index range
        // check. When the name is duplicated (only possible across
        // mutually exclusive alternatives; see `paths_diverge`
        // in `group`), every index sharing it is a candidate: the
        // backtracking evaluator resolves this at match time to whichever
        // one actually participated (ECMA-262; at most one can, since they
        // are mutually exclusive), matching empty if none did.
        let Some(record) = find_name(&self.names, name.as_str(), self.governor)? else {
            self.governor.charge(
                Resource::Work,
                size(name.len()).saturating_mul(16).saturating_add(64),
            )?;
            return syntax(
                start,
                format_args!("no capturing group named {:?}", name.as_str()),
                self.governor,
            );
        };
        let indices = &self.names[record].indices;
        self.governor
            .charge(Resource::Work, size(indices.len()).saturating_add(1))?;
        // The retained AST copy stays under the existing syntax estimate.
        Ok(Node::Backref(indices.as_slice().to_vec()))
    }
    fn hex_escape(&mut self, count: usize) -> Result<Node, PatternCompileError> {
        let start = self.position;
        let mut value = 0u32;
        for _ in 0..count {
            let Some(digit) = self.next()?.and_then(|character| character.to_digit(16)) else {
                return self.fail_at(start, "invalid hexadecimal escape");
            };
            value = (value << 4) | digit;
        }
        self.finish_code_point(value, start)
    }
    fn unicode_escape(&mut self) -> Result<Node, PatternCompileError> {
        if self.current() == Some('{') {
            self.advance(1)?;
            let start = self.position;
            let mut value = 0u32;
            let mut any = false;
            while let Some(digit) = self.current().and_then(|character| character.to_digit(16)) {
                any = true;
                self.governor.charge(Resource::Work, 1)?;
                let Some(next) = value
                    .checked_mul(16)
                    .and_then(|value| value.checked_add(digit))
                else {
                    return self.fail_at(start, "code point escape out of range");
                };
                value = next;
                self.advance(1)?;
            }
            if !any || self.next()? != Some('}') {
                return self.fail_at(start, "invalid Unicode code point escape");
            }
            self.finish_code_point(value, start)
        } else {
            let start = self.position;
            let high = self.read_hex4(start)?;
            self.governor.charge(Resource::Work, 8)?;
            if (0xD800..=0xDBFF).contains(&high)
                && self.chars.get(self.position) == Some(&'\\')
                && self.chars.get(self.position + 1) == Some(&'u')
                && self
                    .chars
                    .get(self.position + 2..self.position + 6)
                    .is_some_and(|digits| digits.iter().all(char::is_ascii_hexdigit))
            {
                let low_start = self.position + 2;
                self.governor.charge(Resource::Work, 4)?;
                let mut low = 0_u32;
                for character in &self.chars[low_start..low_start + 4] {
                    low = (low << 4)
                        | character
                            .to_digit(16)
                            .expect("lookahead verified four hex digits");
                }
                if (0xDC00..=0xDFFF).contains(&low) {
                    self.advance(6)?;
                    let combined = 0x10000 + (high - 0xD800) * 0x400 + (low - 0xDC00);
                    return self.finish_code_point(combined, start);
                }
            }
            self.finish_code_point(high, start)
        }
    }
    fn read_hex4(&mut self, start: usize) -> Result<u32, PatternCompileError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some(digit) = self.next()?.and_then(|character| character.to_digit(16)) else {
                return self.fail_at(start, "invalid hexadecimal escape");
            };
            value = (value << 4) | digit;
        }
        Ok(value)
    }
    fn finish_code_point(&mut self, value: u32, start: usize) -> Result<Node, PatternCompileError> {
        char::from_u32(value).map_or_else(
            || {
                self.fail_at(
                    start,
                    "escape names an unpaired surrogate, not a Unicode scalar value",
                )
            },
            |character| Ok(literal(character)),
        )
    }

    fn property_escape(&mut self, positive: bool) -> Result<Node, PatternCompileError> {
        if self.next()? != Some('{') {
            return self.fail("property escape requires braces");
        }
        let start = self.position;
        while self.current().is_some_and(|c| c != '}') {
            self.advance(1)?;
        }
        let text = copy_chars(&self.chars[start..self.position], self.governor)?;
        self.governor
            .charge(Resource::Work, size(text.len()).saturating_add(1))?;
        if self.next()? != Some('}') {
            return self.fail("unterminated property escape");
        }
        let ranges = if let Some((name, value)) = text.as_str().split_once('=') {
            match name {
                "General_Category" | "gc" => unicode::category_ranges(value),
                "Script" | "sc" => unicode::script_ranges(value),
                "Script_Extensions" | "scx" => unicode::script_extension_ranges(value),
                _ => None,
            }
        } else {
            unicode::lone_property_ranges(text.as_str())
        };
        let Some(ranges) = ranges else {
            self.governor
                .charge(Resource::Work, size(text.len()).saturating_add(1))?;
            return syntax(
                start,
                format_args!("unsupported Unicode property: {}", text.as_str()),
                self.governor,
            );
        };
        Ok(class_from_ranges(ranges, !positive))
    }
    fn class_atom(&mut self) -> Result<ClassAtom, PatternCompileError> {
        if self.current() == Some('\\') {
            self.advance(1)?;
            let node = self.escape(true)?;
            return Ok(match node {
                Node::Class(intervals)
                    if intervals.len() == 1 && intervals[0].0 == intervals[0].1 =>
                {
                    ClassAtom::Literal(intervals[0].0)
                }
                Node::Class(intervals) => ClassAtom::Set(intervals.as_ref().clone()),
                Node::NoMatch => ClassAtom::Set(Vec::new()),
                _ => return self.fail("invalid character class atom"),
            });
        }
        let Some(character) = self.next()? else {
            return self.fail("unterminated character class");
        };
        Ok(ClassAtom::Literal(u32::from(character)))
    }
    fn class(&mut self) -> Result<Node, PatternCompileError> {
        let negative = self.current() == Some('^');
        if negative {
            self.advance(1)?;
        }
        let mut ranges = Vec::new();
        while self.current() != Some(']') {
            if self.current().is_none() {
                return self.fail("unterminated character class");
            }
            let dash_offset = self.position;
            let first = self.class_atom()?;
            if self.current() == Some('-') && self.chars.get(self.position + 1) != Some(&']') {
                self.advance(1)?;
                let second = self.class_atom()?;
                match (first, second) {
                    (ClassAtom::Literal(start), ClassAtom::Literal(end)) => {
                        if start > end {
                            return self.fail_at(dash_offset, "character class range out of order");
                        }
                        ranges.push((start, end));
                    }
                    _ => {
                        return self.fail_at(
                            dash_offset,
                            "a character class range cannot end in a class escape",
                        );
                    }
                }
            } else {
                match first {
                    ClassAtom::Literal(code) => ranges.push((code, code)),
                    ClassAtom::Set(set) => ranges.extend(set),
                }
            }
        }
        self.advance(1)?;
        Ok(class_from_ranges(ranges, negative))
    }
}

fn literal(character: char) -> Node {
    let code = u32::from(character);
    Node::Class(Rc::new(vec![(code, code)]))
}

#[cfg(test)]
mod tests {
    use crate::contract::{Budget, CancellationToken, Governor, Resource, StopReason};
    use crate::pattern::{Pattern, PatternCompileError};

    /// Temporary names, branch paths and references release on early, partial
    /// and final parser refusal; the existing retained AST estimate is separate.
    #[test]
    fn parser_metadata_refusals_release_partial_owners() {
        let source = r"^(?:(?<α>a)|(?<α>b))(?<tail>c)\k<α>\k<tail>\1$";
        let mut measured = Governor::new(Budget::default());
        Pattern::compile_governed(source, &mut measured).unwrap();
        let usage = measured.usage();
        assert!(usage.peak_scratch_bytes > measured.scratch_meter().live());
        let exact = Budget {
            work: usage.work,
            input_bytes: u64::try_from(source.len()).unwrap(),
            scratch_bytes: usage.peak_scratch_bytes,
            ..Budget::default()
        };
        Pattern::compile_governed(source, &mut Governor::new(exact)).unwrap();
        for (budget, resource) in [
            (Budget { work: 0, ..exact }, Resource::Work),
            (
                Budget {
                    work: usage.work / 2,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    work: usage.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: 0,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
            (
                Budget {
                    input_bytes: u64::try_from(source.len()).unwrap() - 1,
                    ..exact
                },
                Resource::InputBytes,
            ),
        ] {
            let mut governor = Governor::new(budget);
            let reason = StopReason::BudgetExceeded(resource);
            assert_eq!(
                Pattern::compile_governed(source, &mut governor).unwrap_err(),
                PatternCompileError::Resource(reason.clone())
            );
            assert_eq!(governor.check(), Err(reason));
            assert_eq!(governor.scratch_meter().live(), 0);
        }
        let token = CancellationToken::new();
        let mut governor = Governor::with_cancellation(exact, token.clone());
        token.cancel();
        assert_eq!(
            Pattern::compile_governed(source, &mut governor).unwrap_err(),
            PatternCompileError::Resource(StopReason::Cancelled)
        );
        assert_eq!(governor.scratch_meter().live(), 0);
    }

    /// Dynamic diagnostics retain their original text and character offsets;
    /// refusal while constructing them remains typed and cleans temporaries.
    #[test]
    fn metadata_diagnostics_preserve_character_offsets_and_admit_publication() {
        for (source, offset, message) in [
            ("(?<1>a)", 3, "invalid group name \"1\""),
            ("α\\k<missing>", 4, "no capturing group named \"missing\""),
            (
                "(?<x>a)(?<x>b)",
                10,
                "duplicate capturing group name outside mutually exclusive alternatives",
            ),
            ("(a)\\2", 4, "backreference to non-existent group 2"),
            (
                "\\p{Not_A_Property}",
                3,
                "unsupported Unicode property: Not_A_Property",
            ),
        ] {
            let mut measured = Governor::new(Budget::default());
            let error = Pattern::compile_governed(source, &mut measured).unwrap_err();
            let PatternCompileError::Syntax(error) = error else {
                panic!("expected syntax error");
            };
            assert_eq!((error.offset, error.message.as_str()), (offset, message));
            let mut governor = Governor::new(Budget {
                work: measured.usage().work - 1,
                ..Budget::default()
            });
            assert_eq!(
                Pattern::compile_governed(source, &mut governor).unwrap_err(),
                PatternCompileError::Resource(StopReason::BudgetExceeded(Resource::Work))
            );
            assert_eq!(governor.scratch_meter().live(), 0);
        }
    }
}
