// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One structural cursor and escaping authority, with explicit scheduling policies.

use std::convert::Infallible;

use super::{JsonField, JsonLine, JsonOutput as Governed, Node, Value};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchLease, ScratchString, ScratchVec, StopReason,
    saturating_size,
};

#[derive(Clone, Copy)]
enum Phase {
    Start,
    Item,
    Colon,
    Value,
    Separator,
    Done,
}

struct Frame<'a> {
    node: &'a Node,
    index: usize,
    phase: Phase,
}

impl ScratchCharge for Frame<'_> {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 24);
        24
    };
}

#[derive(Clone, Copy)]
enum Token<'a> {
    Raw(&'a str),
    Quoted(&'a str),
}
enum Event<'a> {
    Token(Token<'a>),
    Child(&'a Node),
}

impl<'a> Frame<'a> {
    const fn new(node: &'a Node) -> Self {
        Self {
            node,
            index: 0,
            phase: Phase::Start,
        }
    }

    fn next_event(&mut self, fields: Option<&'a [JsonField<'a>]>) -> Option<Event<'a>> {
        let token = match self.phase {
            Phase::Start => {
                self.phase = Phase::Done;
                match &self.node.value {
                    Value::Null => Token::Raw("null"),
                    Value::Bool(value) => Token::Raw(if *value { "true" } else { "false" }),
                    Value::Number(number) => Token::Raw(number.as_str()),
                    Value::String(text) => Token::Quoted(text),
                    Value::Array(_) => {
                        self.phase = Phase::Item;
                        Token::Raw("[")
                    }
                    Value::Object(_) => {
                        self.phase = Phase::Item;
                        Token::Raw("{")
                    }
                }
            }
            Phase::Item => match &self.node.value {
                Value::Array(items) => {
                    if let Some(child) = items.get(self.index) {
                        self.index += 1;
                        self.phase = Phase::Separator;
                        return Some(Event::Child(child));
                    }
                    self.close()
                }
                Value::Object(_) => {
                    if let Some((key, _)) = self.member(fields) {
                        self.phase = Phase::Colon;
                        Token::Quoted(key)
                    } else {
                        self.close()
                    }
                }
                _ => unreachable!("only containers enter their item phase"),
            },
            Phase::Colon => {
                self.phase = Phase::Value;
                Token::Raw(":")
            }
            Phase::Value => {
                // This immutable member's key was emitted at the same index.
                let (_, child) = self
                    .member(fields)
                    .expect("INVARIANT: member value follows its key");
                self.index += 1;
                self.phase = Phase::Separator;
                return Some(Event::Child(child));
            }
            Phase::Separator => {
                let length = match &self.node.value {
                    Value::Array(items) => items.len(),
                    Value::Object(members) => fields.map_or(members.len(), <[JsonField<'_>]>::len),
                    _ => unreachable!("only containers have separators"),
                };
                if self.index < length {
                    self.phase = Phase::Item;
                    Token::Raw(",")
                } else {
                    self.close()
                }
            }
            Phase::Done => return None,
        };
        Some(Event::Token(token))
    }

    fn member(&self, fields: Option<&'a [JsonField<'a>]>) -> Option<(&'a str, &'a Node)> {
        fields.map_or_else(
            || {
                let Value::Object(members) = &self.node.value else {
                    unreachable!("object member phase");
                };
                members
                    .get(self.index)
                    .map(|member| (member.key.as_str(), &member.value))
            },
            |fields| {
                fields
                    .get(self.index)
                    .map(|field| (field.key, field.value.unwrap_or(&NULL)))
            },
        )
    }

    fn close(&mut self) -> Token<'a> {
        self.phase = Phase::Done;
        Token::Raw(match &self.node.value {
            Value::Array(_) => "]",
            Value::Object(_) => "}",
            _ => unreachable!("only containers have closing delimiters"),
        })
    }
}

trait Sink {
    type Error;
    fn scan(&mut self, bytes: usize) -> Result<(), Self::Error>;
    fn write(&mut self, text: &str) -> Result<(), Self::Error>;
    fn verbatim_quotes(&mut self, _: &str) -> Result<bool, Self::Error> {
        Ok(false)
    }
    fn note_escape(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

trait GovernedSink: Sink<Error = StopReason> {
    fn governor(&mut self) -> &mut Governor;
}

impl GovernedSink for Governed<'_> {
    fn governor(&mut self) -> &mut Governor {
        self.governor
    }
}

/// The same tokens first count their exact framing length, then stream to SHA-256.
struct HashSink<'a> {
    governor: &'a mut Governor,
    builder: Option<&'a mut crate::digest::DigestBuilder>,
    bytes: u64,
    unescaped: bool,
}

impl GovernedSink for HashSink<'_> {
    fn governor(&mut self) -> &mut Governor {
        self.governor
    }
}

impl Sink for HashSink<'_> {
    type Error = StopReason;

    fn verbatim_quotes(&mut self, input: &str) -> Result<bool, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        if self.builder.is_some() {
            return Ok(self.unescaped);
        }
        let unescaped = unescaped_words(input, self.governor)?;
        self.unescaped &= unescaped;
        Ok(unescaped)
    }

    fn note_escape(&mut self) -> Result<(), StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        self.unescaped = false;
        Ok(())
    }

    fn scan(&mut self, bytes: usize) -> Result<(), StopReason> {
        self.governor.charge(Resource::Work, saturating_size(bytes))
    }

    fn write(&mut self, text: &str) -> Result<(), StopReason> {
        let bytes = saturating_size(text.len());
        self.governor.charge(
            Resource::Work,
            if self.builder.is_some() { bytes } else { 1 },
        )?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| self.governor.stop_resource(Resource::Work))?;
        if let Some(builder) = &mut self.builder {
            builder.update(text.as_bytes());
        }
        Ok(())
    }
}

/// Exact exclusion of the shared encoder's three ASCII escape classes.
/// Valid UTF-8 bytes above ASCII never need JSON escaping.
fn unescaped_words(input: &str, governor: &mut Governor) -> Result<bool, StopReason> {
    let (words, tail) = input.as_bytes().as_chunks::<8>();
    for bytes in words {
        // One safe word load and three whole-word classification queries.
        governor.charge(Resource::Work, 4)?;
        let word = u64::from_ne_bytes(*bytes);
        if zero_byte(word ^ 0x2222_2222_2222_2222)
            || zero_byte(word ^ 0x5c5c_5c5c_5c5c_5c5c)
            || zero_byte(word & 0xe0e0_e0e0_e0e0_e0e0)
        {
            return Ok(false);
        }
    }
    for &byte in tail {
        governor.charge(Resource::Work, 1)?;
        if matches!(byte, 0..=31 | b'"' | b'\\') {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether any byte lane is zero; wrapping subtraction is intentional.
const fn zero_byte(word: u64) -> bool {
    word.wrapping_sub(0x0101_0101_0101_0101) & !word & 0x8080_8080_8080_8080 != 0
}

struct Plain<'a>(&'a mut String);
impl Sink for Plain<'_> {
    type Error = Infallible;
    fn scan(&mut self, _: usize) -> Result<(), Infallible> {
        Ok(())
    }
    fn write(&mut self, text: &str) -> Result<(), Infallible> {
        self.0.push_str(text);
        Ok(())
    }
}

impl<'g> Governed<'g> {
    /// One fallible output buffer, shared by structural and declaration writers.
    pub(crate) fn new(governor: &'g mut Governor) -> Result<Self, StopReason> {
        governor.check()?;
        let output = ScratchString::new(governor);
        Ok(Self { governor, output })
    }

    /// Append already validated text without reinterpreting its spelling.
    pub(crate) fn raw(&mut self, text: &str) -> Result<(), StopReason> {
        self.write(text)
    }

    /// Append a string through the shared JSON escaping authority.
    pub(crate) fn quoted(&mut self, text: &str) -> Result<(), StopReason> {
        quoted(text, self)
    }

    /// Publish only the complete output after the final cancellation check.
    pub(crate) fn finish(self) -> Result<String, StopReason> {
        self.finish_scratch().map(ScratchString::into_string)
    }

    pub(crate) fn finish_scratch(self) -> Result<ScratchString, StopReason> {
        self.governor.check()?;
        Ok(self.output)
    }
}

impl Sink for Governed<'_> {
    type Error = StopReason;
    fn scan(&mut self, bytes: usize) -> Result<(), StopReason> {
        self.governor.charge(Resource::Work, saturating_size(bytes))
    }
    fn write(&mut self, text: &str) -> Result<(), StopReason> {
        self.governor
            .charge(Resource::Work, saturating_size(text.len()))?;
        self.output.push_str(text)
    }
}

fn token<S: Sink>(token: Token<'_>, sink: &mut S) -> Result<(), S::Error> {
    match token {
        Token::Raw(text) => sink.write(text),
        Token::Quoted(text) => quoted(text, sink),
    }
}

fn quoted<S: Sink>(input: &str, sink: &mut S) -> Result<(), S::Error> {
    if sink.verbatim_quotes(input)? {
        sink.write("\"")?;
        sink.write(input)?;
        return sink.write("\"");
    }
    sink.scan(input.len())?;
    sink.write("\"")?;
    let mut start = 0;
    for (index, character) in input.char_indices() {
        if !matches!(character, '\0'..='\u{1f}' | '"' | '\\') {
            continue;
        }
        sink.note_escape()?;
        if start < index {
            sink.write(&input[start..index])?;
        }
        match character {
            '"' => sink.write("\\\"")?,
            '\\' => sink.write("\\\\")?,
            '\n' => sink.write("\\n")?,
            '\r' => sink.write("\\r")?,
            '\t' => sink.write("\\t")?,
            '\u{8}' => sink.write("\\b")?,
            '\u{c}' => sink.write("\\f")?,
            _ => {
                const HEX: [&str; 16] = [
                    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
                ];
                // All other escaped scalars are ASCII controls (0..=31).
                let code = usize::try_from(u32::from(character))
                    .expect("INVARIANT: ASCII controls fit usize");
                sink.write("\\u00")?;
                sink.write(HEX[code >> 4])?;
                sink.write(HEX[code & 15])?;
            }
        }
        start = index + character.len_utf8();
    }
    if start < input.len() {
        sink.write(&input[start..])?;
    }
    sink.write("\"")
}

const fn infallible(result: Result<(), Infallible>) {
    match result {
        Ok(()) => {}
        Err(never) => match never {},
    }
}

/// The legacy scheduling driver keeps cursor frames on the existing native call
/// stack. It introduces no heap cursor collection into pre-admitted append calls.
pub(super) fn append(node: &Node, output: &mut String) {
    fn recurse(node: &Node, sink: &mut Plain<'_>) {
        let mut frame = Frame::new(node);
        while let Some(event) = frame.next_event(None) {
            match event {
                Event::Child(child) => recurse(child, sink),
                Event::Token(value) => infallible(token(value, sink)),
            }
        }
    }
    recurse(node, &mut Plain(output));
}

pub(super) fn quote(input: &str, output: &mut String) {
    infallible(quoted(input, &mut Plain(output)));
}

pub(super) fn to_json_governed(node: &Node, governor: &mut Governor) -> Result<String, StopReason> {
    to_json_scratch(node, governor).map(ScratchString::into_string)
}

pub(super) fn to_json_scratch(
    node: &Node,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    governor.charge(Resource::Depth, 0)?;
    let output = ScratchString::new(governor);
    let mut frames = ScratchVec::new(governor);
    frames.push(Frame::new(node))?;
    let mut sink = Governed { governor, output };
    emit_frames(&mut frames, None, &mut sink)?;
    sink.finish_scratch()
}

pub(super) fn hash_part(
    node: &Node,
    builder: &mut crate::digest::DigestBuilder,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    const CONTROLS: u64 = 384;
    const _: () = assert!(
        core::mem::size_of::<(
            ScratchVec<Frame<'static>>,
            HashSink<'static>,
            Frame<'static>,
            Event<'static>,
            Token<'static>,
            ScratchLease,
            Result<(), StopReason>,
            (u64, bool),
            core::slice::Iter<'static, [u8; 8]>,
            core::slice::Iter<'static, u8>,
            u64,
        )>() as u64
            <= CONTROLS
    );
    governor.charge(Resource::Work, 4)?;
    let _controls = governor.lease_scratch(CONTROLS)?;
    governor.charge(Resource::Depth, 0)?;
    let mut frames = ScratchVec::new(governor);
    frames.push(Frame::new(node))?;
    let (bytes, unescaped) = {
        let mut count = HashSink {
            governor,
            builder: None,
            bytes: 0,
            unescaped: true,
        };
        emit_frames(&mut frames, None, &mut count)?;
        (count.bytes, count.unescaped)
    };
    governor.charge(Resource::Work, 8)?;
    builder.update(&bytes.to_be_bytes());
    frames.push(Frame::new(node))?;
    let mut sink = HashSink {
        governor,
        builder: Some(builder),
        bytes: 0,
        unescaped,
    };
    emit_frames(&mut frames, None, &mut sink)?;
    sink.governor.check()
}

static NULL: Node = Node::new(Value::Null);
static OBJECT: Node = Node::new(Value::Object(Vec::new()));

fn emit_frames<'a, S: GovernedSink>(
    frames: &mut ScratchVec<Frame<'a>>,
    fields: Option<&'a [JsonField<'a>]>,
    sink: &mut S,
) -> Result<(), StopReason> {
    while !frames.is_empty() {
        sink.governor().charge(Resource::Work, 1)?;
        // The loop condition proves a frame exists, and next_event borrows only
        // immutable source nodes; growing the cursor stack cannot invalidate it.
        let root_fields = if frames.len() == 1 { fields } else { None };
        let event = frames
            .as_mut_slice()
            .last_mut()
            .expect("INVARIANT: nonempty frame stack")
            .next_event(root_fields);
        match event {
            Some(Event::Child(child)) => {
                sink.governor()
                    .charge(Resource::Depth, saturating_size(frames.len()))?;
                frames.push(Frame::new(child))?;
            }
            Some(Event::Token(value)) => token(value, sink)?,
            None => {
                frames.pop();
            }
        }
    }
    Ok(())
}

pub(super) fn lines_to_json_scratch(
    lines: &[JsonLine<'_>],
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    governor.check()?;
    let mut frames = ScratchVec::new(governor);
    let mut sink = Governed::new(governor)?;
    for line in lines {
        sink.governor.charge(Resource::Work, 1)?;
        sink.governor.charge(Resource::Depth, 0)?;
        let (node, fields) = match line {
            JsonLine::Value(value) => (value.unwrap_or(&NULL), None),
            JsonLine::Object(fields) => (&OBJECT, Some(*fields)),
        };
        frames.push(Frame::new(node))?;
        emit_frames(&mut frames, fields, &mut sink)?;
        sink.raw("\n")?;
    }
    sink.finish_scratch()
}

pub(super) fn quote_governed(input: &str, governor: &mut Governor) -> Result<String, StopReason> {
    quote_scratch(input, governor).map(ScratchString::into_string)
}

pub(super) fn quote_scratch(
    input: &str,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    governor.charge(Resource::Depth, 0)?;
    governor.charge(Resource::Work, 1)?;
    let mut sink = Governed::new(governor)?;
    sink.quoted(input)?;
    sink.finish_scratch()
}

#[cfg(test)]
mod tests {
    use super::super::parse;

    #[test]
    fn existing_append_callers_keep_their_prefix_and_canonical_bytes() {
        let source = r#"{"a":1.00e2,"a":["\u0000",false]}"#;
        let node = parse(source).expect("caller-prepared tree");
        let mut output = String::from("prefix:");
        node.write_json(&mut output);
        assert_eq!(output, format!("prefix:{source}"));
    }
}
