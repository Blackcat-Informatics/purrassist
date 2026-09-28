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
use crate::contract::{Governor, Resource};
use std::collections::BTreeMap;
use std::rc::Rc;

use super::{PatternCompileError, PatternError};

struct Parser<'g> {
    chars: Vec<char>,
    position: usize,
    captures: usize,
    governor: &'g mut Governor,
    /// Every capturing group's index, in source order, grouped by name.
    /// More than one entry for a name only when it is legitimately
    /// duplicated across mutually exclusive alternatives (checked as an
    /// early error at each definition; see `name_paths`); `\k<name>`
    /// resolves to this whole set, and the backtracking evaluator picks
    /// whichever one actually participated at match time (ECMA-262).
    name_indices: BTreeMap<String, Vec<usize>>,
    name_paths: BTreeMap<String, Vec<Vec<(u32, u32)>>>,
    path: Vec<(u32, u32)>,
    next_alt_id: u32,
    backrefs: Vec<(usize, usize)>,
}

pub(super) fn parse(
    source: &str,
    governor: &mut Governor,
) -> Result<(Node, usize), PatternCompileError> {
    let chars: Vec<char> = source.chars().collect();
    let names = scan_group_names(&chars)?;
    let mut name_indices: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (position, name) in names.iter().enumerate() {
        if let Some(name) = name {
            name_indices
                .entry(name.clone())
                .or_default()
                .push(position + 1);
        }
    }
    let mut parser = Parser {
        chars,
        position: 0,
        captures: 0,
        governor,
        name_indices,
        name_paths: BTreeMap::new(),
        path: Vec::new(),
        next_alt_id: 0,
        backrefs: Vec::new(),
    };
    let node = parser.alternate(0)?;
    if parser.position != parser.chars.len() {
        return parser.fail("unexpected closing group");
    }
    for (offset, index) in &parser.backrefs {
        if *index > parser.captures {
            return Err(PatternCompileError::Syntax(PatternError {
                offset: *offset,
                message: format!("backreference to non-existent group {index}"),
            }));
        }
    }
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

/// Find every capturing group's name (or `None` if unnamed) in source-text
/// order, by scanning the raw character stream (skipping escapes and
/// character-class contents) rather than a full parse, so `\k<name>` can
/// resolve a forward reference during the one real parse below.
fn scan_group_names(chars: &[char]) -> Result<Vec<Option<String>>, PatternError> {
    let mut names = Vec::new();
    let mut in_class = false;
    let mut index = 0;
    while index < chars.len() {
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
                            end += 1;
                        }
                        if chars.get(end) != Some(&'>') {
                            return Err(PatternError {
                                offset: start,
                                message: "unterminated group name".into(),
                            });
                        }
                        let name: String = chars[start..end].iter().collect();
                        validate_group_name(&name, start)?;
                        names.push(Some(name));
                        index = end + 1;
                    } else {
                        index += 1;
                    }
                } else {
                    names.push(None);
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }
    Ok(names)
}

fn validate_group_name(name: &str, offset: usize) -> Result<(), PatternError> {
    let mut chars = name.chars();
    let fail = || PatternError {
        offset,
        message: format!("invalid group name {name:?}"),
    };
    let first = chars.next().ok_or_else(fail)?;
    let id_start = unicode::binary_property_ranges("ID_Start").unwrap_or_default();
    let id_continue = unicode::binary_property_ranges("ID_Continue").unwrap_or_default();
    let starts =
        first == '$' || first == '_' || unicode::interval_contains(&id_start, u32::from(first));
    if !starts {
        return Err(fail());
    }
    for character in chars {
        let continues =
            character == '$' || unicode::interval_contains(&id_continue, u32::from(character));
        if !continues {
            return Err(fail());
        }
    }
    Ok(())
}

/// `p` and `q` (two definitions' alternation-branch paths) may share a name
/// only if they diverge at a common `Alternate` ancestor into different
/// branches — see `pattern/mod.rs`'s module documentation for why this
/// mirrors ECMA-262's duplicate-named-capturing-group rule.
fn paths_diverge(p: &[(u32, u32)], q: &[(u32, u32)]) -> bool {
    for index in 0..p.len().min(q.len()) {
        if p[index] != q[index] {
            return p[index].0 == q[index].0 && p[index].1 != q[index].1;
        }
    }
    false
}

impl Parser<'_> {
    fn current(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }
    fn next(&mut self) -> Option<char> {
        let character = self.current()?;
        self.position += 1;
        Some(character)
    }
    fn fail<T>(&self, message: &str) -> Result<T, PatternCompileError> {
        Err(PatternCompileError::Syntax(PatternError {
            offset: self.position,
            message: message.into(),
        }))
    }
    fn fail_at<T>(offset: usize, message: &str) -> Result<T, PatternCompileError> {
        Err(PatternCompileError::Syntax(PatternError {
            offset,
            message: message.into(),
        }))
    }
    fn alternate(&mut self, depth: usize) -> Result<Node, PatternCompileError> {
        self.governor
            .charge(Resource::Depth, u64::try_from(depth).unwrap_or(u64::MAX))?;
        self.governor.charge(Resource::Work, 1)?;
        let alt_id = self.next_alt_id;
        self.next_alt_id += 1;
        let mut choices = Vec::new();
        let mut branch = 0u32;
        loop {
            self.path.push((alt_id, branch));
            let choice = self.sequence(depth);
            self.path.pop();
            choices.push(choice?);
            if self.current() == Some('|') {
                self.position += 1;
                branch += 1;
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
                    self.position += 1;
                    Some((0, None))
                }
                Some('+') => {
                    self.position += 1;
                    Some((1, None))
                }
                Some('?') => {
                    self.position += 1;
                    Some((0, Some(1)))
                }
                Some('{') if self.looks_like_quantifier() => {
                    self.position += 1;
                    let min = self.count()?;
                    let max = if self.current() == Some(',') {
                        self.position += 1;
                        if self.current() == Some('}') {
                            None
                        } else {
                            Some(self.count()?)
                        }
                    } else {
                        Some(min)
                    };
                    if self.next() != Some('}') || max.is_some_and(|max| max < min) {
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
                    self.position += 1;
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
    fn looks_like_quantifier(&self) -> bool {
        let mut index = self.position + 1;
        if !self.chars.get(index).is_some_and(char::is_ascii_digit) {
            return false;
        }
        while self.chars.get(index).is_some_and(char::is_ascii_digit) {
            index += 1;
        }
        if self.chars.get(index) == Some(&'}') {
            return true;
        }
        if self.chars.get(index) != Some(&',') {
            return false;
        }
        index += 1;
        while self.chars.get(index).is_some_and(char::is_ascii_digit) {
            index += 1;
        }
        self.chars.get(index) == Some(&'}')
    }
    fn count(&mut self) -> Result<usize, PatternCompileError> {
        let mut count = 0_usize;
        let start = self.position;
        while let Some(digit) = self.current().and_then(|character| character.to_digit(10)) {
            let Some(next) = count
                .checked_mul(10)
                .and_then(|value| value.checked_add(usize::try_from(digit).ok()?))
            else {
                return self.fail("repetition exceeds address space");
            };
            count = next;
            self.position += 1;
        }
        if start == self.position {
            return self.fail("expected a repetition count");
        }
        Ok(count)
    }
    fn atom(&mut self, depth: usize) -> Result<Node, PatternCompileError> {
        match self.next() {
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
            self.position += 1;
            capture = false;
            match self.next() {
                Some(':') => {}
                Some('=') => look = Some((true, false)),
                Some('!') => look = Some((false, false)),
                Some('<') => match self.current() {
                    Some('=') => {
                        self.position += 1;
                        look = Some((true, true));
                    }
                    Some('!') => {
                        self.position += 1;
                        look = Some((false, true));
                    }
                    _ => {
                        capture = true;
                        let start = self.position;
                        let mut text = String::new();
                        while self.current().is_some_and(|c| c != '>') {
                            text.push(self.next().expect("checked by while"));
                        }
                        if self.next() != Some('>') {
                            return self.fail("unterminated group name");
                        }
                        name = Some((text, start));
                    }
                },
                _ => return self.fail("invalid group prefix"),
            }
        }
        let index = if capture {
            self.captures += 1;
            self.captures
        } else {
            0
        };
        if let Some((group_name, start)) = &name {
            let paths = self.name_paths.entry(group_name.clone()).or_default();
            for prior in paths.iter() {
                if !paths_diverge(prior, &self.path) {
                    return Self::fail_at(
                        *start,
                        "duplicate capturing group name outside mutually exclusive alternatives",
                    );
                }
            }
            paths.push(self.path.clone());
        }
        let inner = self.alternate(depth + 1)?;
        if self.next() != Some(')') {
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
        match self.next() {
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
                    index = index
                        .checked_mul(10)
                        .and_then(|index| index.checked_add(usize::try_from(digit).ok()?))
                        .ok_or_else(|| {
                            PatternCompileError::Syntax(PatternError {
                                offset: self.position,
                                message: "backreference too large".into(),
                            })
                        })?;
                    self.position += 1;
                }
                self.backrefs.push((start, index));
                Ok(Node::Backref(vec![index]))
            }
            Some(kind @ ('p' | 'P')) => self.property_escape(kind == 'p'),
            Some('c') => {
                let letter = self
                    .next()
                    .filter(char::is_ascii_alphabetic)
                    .ok_or_else(|| {
                        PatternCompileError::Syntax(PatternError {
                            offset: self.position,
                            message: "control escape requires an ASCII letter".into(),
                        })
                    })?;
                Ok(literal(char::from(
                    u8::try_from(u32::from(letter)).expect("ASCII letter") % 32,
                )))
            }
            Some(character) if identity_escape_allowed(character) => Ok(literal(character)),
            _ => Self::fail_at(escape_start, "invalid identity escape under Unicode mode"),
        }
    }
    fn named_backref(&mut self) -> Result<Node, PatternCompileError> {
        if self.next() != Some('<') {
            return self.fail("expected a group name after \\k");
        }
        let start = self.position;
        let mut name = String::new();
        while self.current().is_some_and(|c| c != '>') {
            name.push(self.next().expect("checked by while"));
        }
        if self.next() != Some('>') {
            return self.fail("unterminated group name");
        }
        // Existence is checked here directly (by name, not by index), so
        // there is nothing to add to `self.backrefs`'s numeric-index range
        // check. When the name is duplicated (only possible across
        // mutually exclusive alternatives; see `name_paths`/`paths_diverge`
        // in `group`), every index sharing it is a candidate: the
        // backtracking evaluator resolves this at match time to whichever
        // one actually participated (ECMA-262; at most one can, since they
        // are mutually exclusive), matching empty if none did.
        let Some(indices) = self.name_indices.get(&name).cloned() else {
            return Self::fail_at(start, &format!("no capturing group named {name:?}"));
        };
        Ok(Node::Backref(indices))
    }
    fn hex_escape(&mut self, count: usize) -> Result<Node, PatternCompileError> {
        let start = self.position;
        let mut value = 0u32;
        for _ in 0..count {
            let Some(digit) = self.next().and_then(|character| character.to_digit(16)) else {
                return Self::fail_at(start, "invalid hexadecimal escape");
            };
            value = (value << 4) | digit;
        }
        Self::finish_code_point(value, start)
    }
    fn unicode_escape(&mut self) -> Result<Node, PatternCompileError> {
        if self.current() == Some('{') {
            self.position += 1;
            let start = self.position;
            let mut value = 0u32;
            let mut any = false;
            while let Some(digit) = self.current().and_then(|character| character.to_digit(16)) {
                any = true;
                value = value
                    .checked_mul(16)
                    .and_then(|value| value.checked_add(digit))
                    .ok_or_else(|| {
                        PatternCompileError::Syntax(PatternError {
                            offset: start,
                            message: "code point escape out of range".into(),
                        })
                    })?;
                self.position += 1;
            }
            if !any || self.next() != Some('}') {
                return Self::fail_at(start, "invalid Unicode code point escape");
            }
            Self::finish_code_point(value, start)
        } else {
            let start = self.position;
            let high = self.read_hex4(start)?;
            if (0xD800..=0xDBFF).contains(&high)
                && self.chars.get(self.position) == Some(&'\\')
                && self.chars.get(self.position + 1) == Some(&'u')
                && self
                    .chars
                    .get(self.position + 2..self.position + 6)
                    .is_some_and(|digits| digits.iter().all(char::is_ascii_hexdigit))
            {
                let low_start = self.position + 2;
                let low = u32::from_str_radix(
                    &self.chars[low_start..low_start + 4]
                        .iter()
                        .collect::<String>(),
                    16,
                )
                .unwrap_or(0);
                if (0xDC00..=0xDFFF).contains(&low) {
                    self.position += 6;
                    let combined = 0x10000 + (high - 0xD800) * 0x400 + (low - 0xDC00);
                    return Self::finish_code_point(combined, start);
                }
            }
            Self::finish_code_point(high, start)
        }
    }
    fn read_hex4(&mut self, start: usize) -> Result<u32, PatternCompileError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some(digit) = self.next().and_then(|character| character.to_digit(16)) else {
                return Self::fail_at(start, "invalid hexadecimal escape");
            };
            value = (value << 4) | digit;
        }
        Ok(value)
    }
    fn finish_code_point(value: u32, start: usize) -> Result<Node, PatternCompileError> {
        char::from_u32(value).map(literal).ok_or_else(|| {
            PatternCompileError::Syntax(PatternError {
                offset: start,
                message: "escape names an unpaired surrogate, not a Unicode scalar value".into(),
            })
        })
    }
    fn property_escape(&mut self, positive: bool) -> Result<Node, PatternCompileError> {
        if self.next() != Some('{') {
            return self.fail("property escape requires braces");
        }
        let start = self.position;
        while self.current().is_some_and(|c| c != '}') {
            self.position += 1;
        }
        let text: String = self.chars[start..self.position].iter().collect();
        if self.next() != Some('}') {
            return self.fail("unterminated property escape");
        }
        let ranges = if let Some((name, value)) = text.split_once('=') {
            match name {
                "General_Category" | "gc" => unicode::category_ranges(value),
                "Script" | "sc" => unicode::script_ranges(value),
                "Script_Extensions" | "scx" => unicode::script_extension_ranges(value),
                _ => None,
            }
            .ok_or_else(|| {
                PatternCompileError::Syntax(PatternError {
                    offset: start,
                    message: format!("unsupported Unicode property: {text}"),
                })
            })?
        } else {
            unicode::lone_property_ranges(&text).ok_or_else(|| {
                PatternCompileError::Syntax(PatternError {
                    offset: start,
                    message: format!("unsupported Unicode property: {text}"),
                })
            })?
        };
        Ok(class_from_ranges(ranges, !positive))
    }
    fn class_atom(&mut self) -> Result<ClassAtom, PatternCompileError> {
        if self.current() == Some('\\') {
            self.position += 1;
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
        let character = self.next().ok_or_else(|| {
            PatternCompileError::Syntax(PatternError {
                offset: self.position,
                message: "unterminated character class".into(),
            })
        })?;
        Ok(ClassAtom::Literal(u32::from(character)))
    }
    fn class(&mut self) -> Result<Node, PatternCompileError> {
        let negative = self.current() == Some('^');
        if negative {
            self.position += 1;
        }
        let mut ranges = Vec::new();
        while self.current() != Some(']') {
            if self.current().is_none() {
                return self.fail("unterminated character class");
            }
            let dash_offset = self.position;
            let first = self.class_atom()?;
            if self.current() == Some('-') && self.chars.get(self.position + 1) != Some(&']') {
                self.position += 1;
                let second = self.class_atom()?;
                match (first, second) {
                    (ClassAtom::Literal(start), ClassAtom::Literal(end)) => {
                        if start > end {
                            return Self::fail_at(
                                dash_offset,
                                "character class range out of order",
                            );
                        }
                        ranges.push((start, end));
                    }
                    _ => {
                        return Self::fail_at(
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
        self.position += 1;
        Ok(class_from_ranges(ranges, negative))
    }
}

fn literal(character: char) -> Node {
    let code = u32::from(character);
    Node::Class(Rc::new(vec![(code, code)]))
}
