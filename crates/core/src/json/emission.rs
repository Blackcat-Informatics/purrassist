// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One structural cursor and escaping authority, with explicit scheduling policies.

use std::convert::Infallible;

use super::{JsonOutput as Governed, Node, Value};
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, StopReason, saturating_size,
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

    fn next_event(&mut self) -> Option<Event<'a>> {
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
                Value::Object(members) => {
                    if let Some(member) = members.get(self.index) {
                        self.phase = Phase::Colon;
                        Token::Quoted(&member.key)
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
                // Only object members enter this phase; the key was emitted at
                // this same index, and the immutable source cannot change length.
                let Value::Object(members) = &self.node.value else {
                    unreachable!("object member value phase");
                };
                let child = &members[self.index].value;
                self.index += 1;
                self.phase = Phase::Separator;
                return Some(Event::Child(child));
            }
            Phase::Separator => {
                let length = match &self.node.value {
                    Value::Array(items) => items.len(),
                    Value::Object(members) => members.len(),
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
    sink.scan(input.len())?;
    sink.write("\"")?;
    for character in input.chars() {
        match character {
            '"' => sink.write("\\\"")?,
            '\\' => sink.write("\\\\")?,
            '\n' => sink.write("\\n")?,
            '\r' => sink.write("\\r")?,
            '\t' => sink.write("\\t")?,
            '\u{8}' => sink.write("\\b")?,
            '\u{c}' => sink.write("\\f")?,
            '\0'..='\u{1f}' => {
                const HEX: [&str; 16] = [
                    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f",
                ];
                // This branch contains only ASCII control scalars (0..=31).
                let code = usize::try_from(u32::from(character))
                    .expect("INVARIANT: ASCII controls fit usize");
                sink.write("\\u00")?;
                sink.write(HEX[code >> 4])?;
                sink.write(HEX[code & 15])?;
            }
            _ => {
                let mut bytes = [0; 4];
                sink.write(character.encode_utf8(&mut bytes))?;
            }
        }
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
        while let Some(event) = frame.next_event() {
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
    while !frames.is_empty() {
        sink.governor.charge(Resource::Work, 1)?;
        // The loop condition proves a frame exists, and next_event borrows only
        // immutable source nodes; growing the cursor stack cannot invalidate it.
        let event = frames
            .as_mut_slice()
            .last_mut()
            .expect("INVARIANT: nonempty frame stack")
            .next_event();
        match event {
            Some(Event::Child(child)) => {
                sink.governor
                    .charge(Resource::Depth, saturating_size(frames.len()))?;
                frames.push(Frame::new(child))?;
            }
            Some(Event::Token(value)) => token(value, &mut sink)?,
            None => {
                frames.pop();
            }
        }
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
