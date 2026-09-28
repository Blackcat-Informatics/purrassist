// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Syntax recovery retains every edit and never runs in the strict parser.
use super::SyntaxEdit;
use super::{Member, Node, Number, Value};
use crate::contract::{Governor, Resource, StopReason};
use std::ops::Range;

pub(super) fn recover(
    input: &str,
    governor: &mut Governor,
) -> Result<(Node, usize, Vec<SyntaxEdit>), StopReason> {
    governor.charge(Resource::InputBytes, size(input.len()))?;
    // Every repaired bare/malformed token (`json.syntax.bare-key`,
    // `json.syntax.literal-case`, `json.syntax.bare-string`, ...) re-encodes
    // its replacement through `Node::to_json`, allocating a new String per
    // occurrence on top of the parsed tree itself; a token as short as one
    // byte can still cost tens of bytes once quoted/escaped and boxed into a
    // `SyntaxEdit`, so this bound is per-byte generous rather than a flat
    // per-input-length guess.
    governor.charge(
        Resource::ScratchBytes,
        size(input.len()).saturating_mul(96).saturating_add(4096),
    )?;
    let mut parser = Parser {
        input,
        position: 0,
        edits: Vec::new(),
        governor,
    };
    let node = parser.value(0)?;
    parser.space()?;
    Ok((node, parser.position, parser.edits))
}
fn size(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
struct Parser<'a, 'g> {
    input: &'a str,
    position: usize,
    edits: Vec<SyntaxEdit>,
    governor: &'g mut Governor,
}
impl Parser<'_, '_> {
    fn current(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }
    fn advance(&mut self) -> Result<Option<char>, StopReason> {
        self.governor.charge(Resource::Work, 1)?;
        let character = self.current();
        if let Some(character) = character {
            self.position += character.len_utf8();
        }
        Ok(character)
    }
    fn edit(
        &mut self,
        rule: &'static str,
        span: Range<usize>,
        replacement: String,
        guessed: bool,
    ) -> Result<(), StopReason> {
        self.governor.charge(Resource::EvidenceEntries, 1)?;
        self.edits.push(SyntaxEdit {
            rule,
            span,
            replacement,
            guessed,
        });
        Ok(())
    }
    fn space(&mut self) -> Result<(), StopReason> {
        loop {
            while self.current().is_some_and(char::is_whitespace) {
                self.advance()?;
            }
            let start = self.position;
            if self.input[start..].starts_with("//") {
                while self.current().is_some_and(|c| c != '\n') {
                    self.advance()?;
                }
            } else if self.input[start..].starts_with("/*") {
                self.advance()?;
                self.advance()?;
                while self.current().is_some() && !self.input[self.position..].starts_with("*/") {
                    self.advance()?;
                }
                if self.current().is_some() {
                    self.advance()?;
                    self.advance()?;
                }
            } else {
                return Ok(());
            }
            self.edit(
                "json.syntax.comment",
                start..self.position,
                String::new(),
                false,
            )?;
        }
    }
    fn value(&mut self, depth: u32) -> Result<Node, StopReason> {
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        self.governor.charge(Resource::Work, 1)?;
        self.space()?;
        let start = self.position;
        let value = match self.current() {
            Some('{') => self.object(depth + 1)?,
            Some('[') => self.array(depth + 1)?,
            Some('"' | '\'' | '“') => Value::String(self.string()?),
            None | Some('}' | ']' | ',') => {
                self.edit(
                    "json.syntax.missing-value",
                    start..start,
                    "null".into(),
                    true,
                )?;
                Value::Null
            }
            Some(_) => self.atom()?,
        };
        Ok(Node {
            value,
            span: Some(start..self.position),
        })
    }
    fn atom(&mut self) -> Result<Value, StopReason> {
        let start = self.position;
        while self
            .current()
            .is_some_and(|c| !c.is_whitespace() && !matches!(c, ',' | ']' | '}' | ':'))
        {
            self.advance()?;
        }
        let token = &self.input[start..self.position];
        if token.is_empty() {
            return Err(StopReason::InvalidInput(format!(
                "unrecoverable token at {start}"
            )));
        }
        let (value, spelling) = match token {
            "null" => return Ok(Value::Null),
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            "None" | "NULL" | "Null" => (Value::Null, "null"),
            "True" | "TRUE" => (Value::Bool(true), "true"),
            "False" | "FALSE" => (Value::Bool(false), "false"),
            _ => {
                if let Ok(number) = token.parse::<Number>() {
                    return Ok(Value::Number(number));
                }
                let node = Node::new(Value::String(token.into()));
                self.edit(
                    "json.syntax.bare-string",
                    start..self.position,
                    node.to_json(),
                    true,
                )?;
                return Ok(node.value);
            }
        };
        self.edit(
            "json.syntax.literal-case",
            start..self.position,
            spelling.into(),
            false,
        )?;
        Ok(value)
    }
    fn string(&mut self) -> Result<String, StopReason> {
        let start = self.position;
        let quote = self.advance()?.expect("string opener");
        let closing = if quote == '“' { '”' } else { quote };
        let mut text = String::new();
        let mut closed = false;
        while let Some(character) = self.advance()? {
            if character == closing {
                closed = true;
                break;
            }
            if character == '\\' {
                let escape_start = self.position - 1;
                let Some(escaped) = self.advance()? else {
                    break;
                };
                if escaped == quote && quote != '"' {
                    text.push(escaped);
                    continue;
                }
                let count = if escaped == 'u' { 4 } else { 0 };
                for _ in 0..count {
                    if self.advance()?.is_none() {
                        break;
                    }
                }
                if escaped == 'u' && self.input[self.position..].starts_with("\\u") {
                    // Let the strict parser decide whether these are a surrogate pair.
                    let first = &self.input[escape_start + 2..self.position];
                    if u16::from_str_radix(first, 16)
                        .is_ok_and(|code| (0xd800..=0xdbff).contains(&code))
                    {
                        for _ in 0..6 {
                            if self.advance()?.is_none() {
                                break;
                            }
                        }
                    }
                }
                let encoded = format!("\"{}\"", &self.input[escape_start..self.position]);
                match super::parse_with(&encoded, self.governor) {
                    Ok(node) => text.push_str(node.as_str().expect("string literal")),
                    Err(super::ParseFailure::Resource(reason)) => return Err(reason),
                    Err(super::ParseFailure::Syntax(_)) => {
                        text.push_str(&self.input[escape_start..self.position]);
                        self.edit(
                            "json.syntax.literal-escape",
                            escape_start..self.position,
                            Node::new(Value::String(
                                self.input[escape_start..self.position].into(),
                            ))
                            .to_json(),
                            true,
                        )?;
                    }
                }
            } else {
                text.push(character);
            }
        }
        let replacement = Node::new(Value::String(text.clone())).to_json();
        if !closed || quote != '"' || self.input[start..self.position] != replacement {
            self.edit(
                "json.syntax.string",
                start..self.position,
                replacement,
                !closed,
            )?;
        }
        Ok(text)
    }
    fn separator(&mut self, closing: char) -> Result<bool, StopReason> {
        self.space()?;
        match self.current() {
            Some(c) if c == closing => {
                self.advance()?;
                Ok(false)
            }
            None => {
                self.edit(
                    "json.syntax.close-container",
                    self.position..self.position,
                    closing.to_string(),
                    false,
                )?;
                Ok(false)
            }
            Some(',') => {
                let comma = self.position;
                self.advance()?;
                self.space()?;
                if self.current() == Some(closing) {
                    self.edit(
                        "json.syntax.trailing-comma",
                        comma..comma + 1,
                        String::new(),
                        false,
                    )?;
                    self.advance()?;
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
            Some('}' | ']') => Err(StopReason::InvalidInput(
                "mismatched container closer".into(),
            )),
            _ => {
                self.edit(
                    "json.syntax.missing-comma",
                    self.position..self.position,
                    ",".into(),
                    false,
                )?;
                Ok(true)
            }
        }
    }
    fn array(&mut self, depth: u32) -> Result<Value, StopReason> {
        self.advance()?;
        self.space()?;
        let mut items = Vec::new();
        if self.current() == Some(']') {
            self.advance()?;
            return Ok(Value::Array(items));
        }
        if self.current().is_none() {
            self.separator(']')?;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            if !self.separator(']')? {
                break;
            }
        }
        Ok(Value::Array(items))
    }
    fn object(&mut self, depth: u32) -> Result<Value, StopReason> {
        self.advance()?;
        self.space()?;
        let mut members = Vec::new();
        if self.current() == Some('}') {
            self.advance()?;
            return Ok(Value::Object(members));
        }
        if self.current().is_none() {
            self.separator('}')?;
            return Ok(Value::Object(members));
        }
        loop {
            self.space()?;
            let start = self.position;
            let key = if matches!(self.current(), Some('"' | '\'' | '“')) {
                self.string()?
            } else {
                while self
                    .current()
                    .is_some_and(|c| !c.is_whitespace() && !matches!(c, ':' | ',' | '}' | ']'))
                {
                    self.advance()?;
                }
                if start == self.position {
                    return Err(StopReason::InvalidInput("missing object key".into()));
                }
                let key = self.input[start..self.position].to_string();
                self.edit(
                    "json.syntax.bare-key",
                    start..self.position,
                    Node::new(Value::String(key.clone())).to_json(),
                    false,
                )?;
                key
            };
            let key_span = Some(start..self.position);
            self.space()?;
            if self.current() == Some(':') {
                self.advance()?;
            } else {
                self.edit(
                    "json.syntax.missing-colon",
                    self.position..self.position,
                    ":".into(),
                    false,
                )?;
            }
            let value = self.value(depth)?;
            members.push(Member {
                key,
                key_span,
                value,
            });
            if !self.separator('}')? {
                break;
            }
        }
        Ok(Value::Object(members))
    }
}
