// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Governed DSL syntax admission; semantic lowering remains a separate phase.
//!
//! Source scans, decoding and copies are pre-admitted. Tokens and the complete
//! positioned tree own fallible scratch storage through lowering. Parser
//! recursion and tree ownership height each have a native ceiling of 128 in
//! addition to caller depth; postfix chains cannot create unbounded drop depth.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::GrammarError;
use super::cost::K_COMPILE;
use crate::contract::{
    Governor, Resource, ScratchCharge, ScratchString, ScratchVec, format_scratch_governed,
    saturating_size as size,
};
use core::fmt;

const NATIVE_DEPTH: u32 = 128;
// Covers trivia/identifier/literal/class scans, UTF-8/escape decoding, copies,
// and the fixed keyword comparisons. Node/list processing is also charged.
const SOURCE_WORK_PER_BYTE: u64 = 64;

/// Exactly one owned child; only the checked constructor can create it.
#[derive(Debug)]
pub(crate) struct Child {
    values: ScratchVec<PExpr>,
    height: u32,
}
impl Child {
    fn new(value: PExpr, governor: &mut Governor) -> Result<Self, GrammarError> {
        let height = value.height() + 1;
        admit_depth(height, governor)?;
        let mut values = ScratchVec::new(governor);
        values.push(value)?;
        Ok(Self { values, height })
    }
    pub(crate) fn get(&self) -> &PExpr {
        &self.values.as_slice()[0]
    }
}

/// One positioned expression; all owning edges retain their exact reservations.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "retain all original syntax offsets; lowering reads only the offsets used by its diagnostics"
)]
pub(crate) enum PExpr {
    Literal {
        bytes: ScratchVec<u8>,
        at: usize,
    },
    Class {
        source: ScratchString,
        at: usize,
    },
    Until {
        literals: ScratchVec<ScratchVec<u8>>,
        at: usize,
    },
    Slot {
        codec: ScratchString,
        codec_at: usize,
        literals: ScratchVec<ScratchVec<u8>>,
        at: usize,
    },
    Name {
        name: ScratchString,
        at: usize,
    },
    Capture {
        kind: ScratchString,
        at: usize,
        inner: Child,
    },
    Sequence(ScratchVec<Self>, usize, u32),
    Choice(ScratchVec<Self>, usize, u32),
    Repeat {
        inner: Child,
        op: u8,
        at: usize,
    },
    Commit,
    Next {
        at: usize,
    },
    Insert {
        bytes: ScratchVec<u8>,
    },
}
impl PExpr {
    const fn height(&self) -> u32 {
        match self {
            Self::Capture { inner, .. } | Self::Repeat { inner, .. } => inner.height,
            Self::Sequence(_, _, height) | Self::Choice(_, _, height) => *height,
            _ => 1,
        }
    }
}
impl ScratchCharge for PExpr {
    // At most two scratch owners (40 each), two positions (8 each), tag/padding.
    const CHARGE: usize = 112;
}
const _: () = assert!(core::mem::size_of::<PExpr>() <= PExpr::CHARGE);

#[derive(Debug)]
#[expect(
    dead_code,
    reason = "retain the original confusable statement offset even though current lowering does not diagnose it"
)]
pub(crate) enum Statement {
    Reasoning {
        open: ScratchVec<u8>,
        close: ScratchVec<u8>,
        explicit: bool,
        at: usize,
    },
    Confusable {
        canonical: ScratchVec<u8>,
        variant: ScratchVec<u8>,
        at: usize,
    },
    Closer {
        name: ScratchString,
        spellings: ScratchVec<ScratchVec<u8>>,
        at: usize,
    },
    Layout {
        name: ScratchString,
        text: ScratchVec<u8>,
        at: usize,
    },
    LineStart {
        opener: ScratchVec<u8>,
        at: usize,
    },
    ExamplesContent {
        at: usize,
    },
    ProtocolFences {
        at: usize,
    },
    OptionalId {
        at: usize,
    },
    DeclarationContent {
        at: usize,
    },
    JsonEnvelope {
        at: usize,
    },
    Production {
        name: ScratchString,
        expr: PExpr,
        at: usize,
    },
}
impl ScratchCharge for Statement {
    const CHARGE: usize = PExpr::CHARGE + 40 + 16;
}
const _: () = assert!(core::mem::size_of::<Statement>() <= Statement::CHARGE);

#[derive(Debug)]
pub(crate) struct Document {
    pub(crate) id: ScratchString,
    pub(crate) statements: ScratchVec<Statement>,
}

pub(crate) const KEYWORDS: [&str; 16] = [
    "grammar",
    "line-start",
    "examples-content",
    "protocol-fences",
    "optional-id",
    "declaration-content",
    "json-envelope",
    "reasoning",
    "confusable",
    "closer",
    "layout",
    "until",
    "slot",
    "commit",
    "next",
    "insert",
];

#[derive(Debug)]
enum Token {
    Ident(ScratchString),
    Str(ScratchVec<u8>),
    Class(ScratchString),
    Punct(u8),
    End,
}
impl Token {
    fn ident(&self, expected: &str) -> bool {
        matches!(self, Self::Ident(name) if name.as_str() == expected)
    }
    const fn punct(&self, expected: u8) -> bool {
        matches!(self, Self::Punct(byte) if *byte == expected)
    }
}

struct Lexer<'a> {
    text: &'a [u8],
    pos: usize,
}

fn syntax(offset: usize, message: fmt::Arguments<'_>, governor: &mut Governor) -> GrammarError {
    // Diagnostics format constants, one byte/character or borrowed fixed text.
    if let Err(reason) = governor.charge(Resource::Work, 128) {
        return GrammarError::Resource(reason);
    }
    match format_scratch_governed(message, governor) {
        Ok(message) => GrammarError::Syntax {
            offset,
            message: message.into_string(),
        },
        Err(reason) => GrammarError::Resource(reason),
    }
}
fn admit_depth(depth: u32, governor: &mut Governor) -> Result<(), GrammarError> {
    if depth > NATIVE_DEPTH {
        return Err(GrammarError::Resource(
            governor.stop_resource(Resource::Depth),
        ));
    }
    governor.charge(Resource::Depth, u64::from(depth))?;
    Ok(())
}
const fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}
const fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

impl Lexer<'_> {
    fn skip_trivia(&mut self, governor: &Governor) -> Result<(), GrammarError> {
        while let Some(&byte) = self.text.get(self.pos) {
            governor.check()?;
            if byte.is_ascii_whitespace() {
                self.pos += 1;
            } else if byte == b'#' {
                while self.text.get(self.pos).is_some_and(|&byte| byte != b'\n') {
                    governor.check()?;
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
        Ok(())
    }
    fn next(&mut self, governor: &mut Governor) -> Result<(Token, usize), GrammarError> {
        self.skip_trivia(governor)?;
        let start = self.pos;
        let Some(&byte) = self.text.get(start) else {
            return Ok((Token::End, start));
        };
        if is_ident_start(byte) {
            while self
                .text
                .get(self.pos)
                .is_some_and(|&byte| is_ident_continue(byte))
            {
                governor.check()?;
                self.pos += 1;
            }
            let mut ident = ScratchString::new(governor);
            // The scan admits ASCII identifier bytes only.
            ident.push_str(
                core::str::from_utf8(&self.text[start..self.pos]).expect("ASCII identifier"),
            )?;
            return Ok((Token::Ident(ident), start));
        }
        match byte {
            b'"' => self
                .string(start, governor)
                .map(|bytes| (Token::Str(bytes), start)),
            b'/' => self
                .class(start, governor)
                .map(|source| (Token::Class(source), start)),
            b'=' | b'|' | b';' | b'(' | b')' | b'*' | b'+' | b'?' | b':' | b',' => {
                self.pos += 1;
                Ok((Token::Punct(byte), start))
            }
            _ => Err(syntax(
                start,
                format_args!("unexpected character {:?}", char::from(byte)),
                governor,
            )),
        }
    }
    fn string(
        &mut self,
        start: usize,
        governor: &mut Governor,
    ) -> Result<ScratchVec<u8>, GrammarError> {
        self.pos += 1;
        let mut bytes = ScratchVec::new(governor);
        loop {
            governor.check()?;
            let Some(&byte) = self.text.get(self.pos) else {
                return Err(syntax(
                    start,
                    format_args!("unterminated string literal"),
                    governor,
                ));
            };
            self.pos += 1;
            match byte {
                b'"' => return Ok(bytes),
                b'\n' => {
                    return Err(syntax(
                        start,
                        format_args!("unterminated string literal"),
                        governor,
                    ));
                }
                b'\\' => self.escape(&mut bytes, governor)?,
                _ => bytes.push(byte)?,
            }
        }
    }
    fn escape(
        &mut self,
        bytes: &mut ScratchVec<u8>,
        governor: &mut Governor,
    ) -> Result<(), GrammarError> {
        let at = self.pos - 1;
        let Some(&kind) = self.text.get(self.pos) else {
            return Err(syntax(at, format_args!("unterminated escape"), governor));
        };
        self.pos += 1;
        match kind {
            b'\\' | b'"' => bytes.push(kind)?,
            b'n' => bytes.push(b'\n')?,
            b'r' => bytes.push(b'\r')?,
            b't' => bytes.push(b'\t')?,
            b'x' => {
                let digits = self.text.get(self.pos..self.pos + 2).ok_or_else(|| {
                    syntax(
                        at,
                        format_args!("\\x needs two hexadecimal digits"),
                        governor,
                    )
                })?;
                let text = core::str::from_utf8(digits).map_err(|_| {
                    syntax(
                        at,
                        format_args!("\\x needs two hexadecimal digits"),
                        governor,
                    )
                })?;
                let value = u8::from_str_radix(text, 16).map_err(|_| {
                    syntax(
                        at,
                        format_args!("\\x needs two hexadecimal digits"),
                        governor,
                    )
                })?;
                self.pos += 2;
                bytes.push(value)?;
            }
            b'u' => {
                if self.text.get(self.pos) != Some(&b'{') {
                    return Err(syntax(
                        at,
                        format_args!("\\u needs braces: \\u{{HEX}}"),
                        governor,
                    ));
                }
                let mut close = self.pos;
                while self.text.get(close).is_some_and(|&byte| byte != b'}') {
                    governor.check()?;
                    close += 1;
                }
                if self.text.get(close).is_none() {
                    return Err(syntax(
                        at,
                        format_args!("unterminated \\u{{...}} escape"),
                        governor,
                    ));
                }
                let digits = &self.text[self.pos + 1..close];
                let scalar = core::str::from_utf8(digits)
                    .ok()
                    .filter(|text| !text.is_empty() && text.len() <= 6)
                    .and_then(|text| u32::from_str_radix(text, 16).ok())
                    .and_then(char::from_u32)
                    .ok_or_else(|| {
                        syntax(
                            at,
                            format_args!("\\u{{...}} must name a Unicode scalar value"),
                            governor,
                        )
                    })?;
                self.pos = close + 1;
                let mut buffer = [0; 4];
                bytes.extend(scalar.encode_utf8(&mut buffer).as_bytes().iter().copied())?;
            }
            _ => {
                return Err(syntax(
                    at,
                    format_args!("unknown escape \\{}", char::from(kind)),
                    governor,
                ));
            }
        }
        Ok(())
    }
    fn class(
        &mut self,
        start: usize,
        governor: &mut Governor,
    ) -> Result<ScratchString, GrammarError> {
        self.pos += 1;
        let mut source = ScratchVec::new(governor);
        loop {
            governor.check()?;
            let Some(&byte) = self.text.get(self.pos) else {
                return Err(syntax(
                    start,
                    format_args!("unterminated class /.../"),
                    governor,
                ));
            };
            self.pos += 1;
            match byte {
                b'/' => break,
                b'\n' => {
                    return Err(syntax(
                        start,
                        format_args!("unterminated class /.../"),
                        governor,
                    ));
                }
                b'\\' if self.text.get(self.pos) == Some(&b'/') => {
                    self.pos += 1;
                    source.push(b'/')?;
                }
                b'\\' => {
                    source.push(b'\\')?;
                    if let Some(&next) = self.text.get(self.pos) {
                        self.pos += 1;
                        source.push(next)?;
                    }
                }
                _ => source.push(byte)?,
            }
        }
        let text = core::str::from_utf8(source.as_slice())
            .map_err(|_| syntax(start, format_args!("class is not UTF-8"), governor))?;
        let mut output = ScratchString::new(governor);
        output.push_str(text)?;
        Ok(output)
    }
}

struct Parser<'a, 'g> {
    lexer: Lexer<'a>,
    token: Token,
    at: usize,
    depth: u32,
    governor: &'g mut Governor,
}
impl Parser<'_, '_> {
    fn advance(&mut self) -> Result<(), GrammarError> {
        let (token, at) = self.lexer.next(self.governor)?;
        self.token = token;
        self.at = at;
        Ok(())
    }
    fn node(&mut self) -> Result<(), GrammarError> {
        self.governor.charge(Resource::Work, K_COMPILE)?;
        admit_depth(1, self.governor)
    }
    fn enter(&mut self) -> Result<(), GrammarError> {
        let depth = self.depth + 1;
        admit_depth(depth, self.governor)?;
        self.depth = depth;
        Ok(())
    }
    fn expect(&mut self, punct: u8) -> Result<(), GrammarError> {
        if self.token.punct(punct) {
            self.advance()
        } else {
            Err(syntax(
                self.at,
                format_args!("expected `{}`", char::from(punct)),
                self.governor,
            ))
        }
    }
    fn string(&mut self) -> Result<(ScratchVec<u8>, usize), GrammarError> {
        let at = self.at;
        if !matches!(self.token, Token::Str(_)) {
            return Err(syntax(
                at,
                format_args!("expected a string literal"),
                self.governor,
            ));
        }
        let Token::Str(bytes) = core::mem::replace(&mut self.token, Token::End) else {
            unreachable!("checked token");
        };
        if bytes.is_empty() {
            return Err(GrammarError::EmptyLiteral { offset: at });
        }
        self.advance()?;
        Ok((bytes, at))
    }
    fn ident(&mut self) -> Result<(ScratchString, usize), GrammarError> {
        let at = self.at;
        if !matches!(self.token, Token::Ident(_)) {
            return Err(syntax(at, format_args!("expected a name"), self.governor));
        }
        let Token::Ident(name) = core::mem::replace(&mut self.token, Token::End) else {
            unreachable!("checked token");
        };
        self.advance()?;
        Ok((name, at))
    }
    fn document(&mut self) -> Result<Document, GrammarError> {
        if !self.token.ident("grammar") {
            return Err(syntax(
                self.at,
                format_args!("a grammar starts with `grammar \"id\";`"),
                self.governor,
            ));
        }
        self.advance()?;
        let (id, id_at) = self.string()?;
        let text = core::str::from_utf8(id.as_slice()).map_err(|_| {
            syntax(
                id_at,
                format_args!("grammar id is not UTF-8"),
                self.governor,
            )
        })?;
        if !text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(syntax(
                id_at,
                format_args!("grammar id must match [A-Za-z0-9._-]+"),
                self.governor,
            ));
        }
        let mut owned_id = ScratchString::new(self.governor);
        owned_id.push_str(text)?;
        drop(id);
        self.expect(b';')?;
        let mut statements = ScratchVec::new(self.governor);
        while !matches!(self.token, Token::End) {
            self.node()?;
            statements.push(self.statement()?)?;
        }
        self.governor.check()?;
        Ok(Document {
            id: owned_id,
            statements,
        })
    }
    fn statement(&mut self) -> Result<Statement, GrammarError> {
        let (name, at) = self.ident()?;
        let statement = match name.as_str() {
            "reasoning" => {
                let (open, _) = self.string()?;
                let (close, _) = self.string()?;
                let explicit = self.token.ident("explicit");
                if explicit {
                    self.advance()?;
                }
                Statement::Reasoning {
                    open,
                    close,
                    explicit,
                    at,
                }
            }
            "confusable" => {
                let (canonical, _) = self.string()?;
                let (variant, _) = self.string()?;
                Statement::Confusable {
                    canonical,
                    variant,
                    at,
                }
            }
            "closer" => {
                let (name, name_at) = self.ident()?;
                reserved(name.as_str(), name_at, self.governor)?;
                self.expect(b'=')?;
                let mut spellings = ScratchVec::new(self.governor);
                spellings.push(self.string()?.0)?;
                while self.token.punct(b'|') {
                    self.advance()?;
                    spellings.push(self.string()?.0)?;
                }
                Statement::Closer {
                    name,
                    spellings,
                    at: name_at,
                }
            }
            "layout" => {
                let (name, name_at) = self.ident()?;
                let (text, _) = self.string()?;
                Statement::Layout {
                    name,
                    text,
                    at: name_at,
                }
            }
            "line-start" => Statement::LineStart {
                opener: self.string()?.0,
                at,
            },
            "examples-content" => Statement::ExamplesContent { at },
            "protocol-fences" => Statement::ProtocolFences { at },
            "optional-id" => Statement::OptionalId { at },
            "declaration-content" => Statement::DeclarationContent { at },
            "json-envelope" => Statement::JsonEnvelope { at },
            "grammar" => {
                return Err(syntax(
                    at,
                    format_args!("duplicate `grammar` header"),
                    self.governor,
                ));
            }
            _ => {
                reserved(name.as_str(), at, self.governor)?;
                self.expect(b'=')?;
                let expr = self.expr()?;
                Statement::Production { name, expr, at }
            }
        };
        self.expect(b';')?;
        Ok(statement)
    }
    fn expr(&mut self) -> Result<PExpr, GrammarError> {
        self.enter()?;
        let at = self.at;
        let mut alternatives = ScratchVec::new(self.governor);
        alternatives.push(self.sequence()?)?;
        while self.token.punct(b'|') {
            self.advance()?;
            alternatives.push(self.sequence()?)?;
        }
        self.depth -= 1;
        self.list(alternatives, at, true)
    }
    fn list(
        &mut self,
        mut items: ScratchVec<PExpr>,
        at: usize,
        choice: bool,
    ) -> Result<PExpr, GrammarError> {
        if items.len() == 1 {
            return Ok(items.pop().expect("one expression"));
        }
        self.governor.charge(Resource::Work, size(items.len()))?;
        let height = items
            .as_slice()
            .iter()
            .map(PExpr::height)
            .max()
            .unwrap_or(0)
            + 1;
        admit_depth(height, self.governor)?;
        Ok(if choice {
            PExpr::Choice(items, at, height)
        } else {
            PExpr::Sequence(items, at, height)
        })
    }
    fn sequence(&mut self) -> Result<PExpr, GrammarError> {
        let at = self.at;
        let mut items = ScratchVec::new(self.governor);
        while !matches!(self.token, Token::Punct(b'|' | b')' | b';') | Token::End) {
            items.push(self.postfixed()?)?;
        }
        if items.is_empty() {
            return Err(syntax(
                at,
                format_args!("expected an expression"),
                self.governor,
            ));
        }
        self.list(items, at, false)
    }
    fn postfixed(&mut self) -> Result<PExpr, GrammarError> {
        let mut item = self.atom()?;
        while let Token::Punct(op @ (b'*' | b'+' | b'?')) = self.token {
            self.node()?;
            let at = self.at;
            self.advance()?;
            item = PExpr::Repeat {
                inner: Child::new(item, self.governor)?,
                op,
                at,
            };
        }
        Ok(item)
    }
    fn literal_list(&mut self) -> Result<ScratchVec<ScratchVec<u8>>, GrammarError> {
        let mut literals = ScratchVec::new(self.governor);
        literals.push(self.string()?.0)?;
        while self.token.punct(b',') {
            self.advance()?;
            literals.push(self.string()?.0)?;
        }
        Ok(literals)
    }
    fn atom(&mut self) -> Result<PExpr, GrammarError> {
        self.node()?;
        let at = self.at;
        match core::mem::replace(&mut self.token, Token::End) {
            Token::Str(bytes) => {
                if bytes.is_empty() {
                    return Err(GrammarError::EmptyLiteral { offset: at });
                }
                self.advance()?;
                Ok(PExpr::Literal { bytes, at })
            }
            Token::Class(source) => {
                self.advance()?;
                Ok(PExpr::Class { source, at })
            }
            Token::Punct(b'(') => {
                self.advance()?;
                let inner = self.expr()?;
                self.expect(b')')?;
                Ok(inner)
            }
            Token::Ident(name) => {
                self.advance()?;
                match (name.as_str(), &self.token) {
                    ("until", Token::Punct(b'(')) => {
                        self.advance()?;
                        let literals = self.literal_list()?;
                        self.expect(b')')?;
                        Ok(PExpr::Until { literals, at })
                    }
                    ("slot", Token::Punct(b'(')) => {
                        self.advance()?;
                        let (codec, codec_at) = self.ident()?;
                        self.expect(b',')?;
                        let literals = self.literal_list()?;
                        self.expect(b')')?;
                        Ok(PExpr::Slot {
                            codec,
                            codec_at,
                            literals,
                            at,
                        })
                    }
                    ("commit", token) if !token.punct(b':') => Ok(PExpr::Commit),
                    ("next", token) if !token.punct(b':') => Ok(PExpr::Next { at }),
                    ("insert", Token::Punct(b'(')) => {
                        self.advance()?;
                        let (bytes, _) = self.string()?;
                        self.expect(b')')?;
                        Ok(PExpr::Insert { bytes })
                    }
                    (_, Token::Punct(b':')) => {
                        self.advance()?;
                        self.enter()?;
                        let inner = self.postfixed()?;
                        self.depth -= 1;
                        Ok(PExpr::Capture {
                            kind: name,
                            at,
                            inner: Child::new(inner, self.governor)?,
                        })
                    }
                    _ => {
                        reserved(name.as_str(), at, self.governor)?;
                        Ok(PExpr::Name { name, at })
                    }
                }
            }
            _ => Err(syntax(
                at,
                format_args!("expected an expression"),
                self.governor,
            )),
        }
    }
}

fn reserved(name: &str, offset: usize, governor: &Governor) -> Result<(), GrammarError> {
    if KEYWORDS.contains(&name) {
        let mut owned = ScratchString::new(governor);
        owned.push_str(name)?;
        governor.check()?;
        return Err(GrammarError::ReservedName {
            offset,
            name: owned.into_string(),
        });
    }
    Ok(())
}

/// Admit complete owned syntax; only later lowering publishes a grammar.
pub(crate) fn parse(text: &str, governor: &mut Governor) -> Result<Document, GrammarError> {
    governor.charge(Resource::InputBytes, size(text.len()))?;
    let work = size(text.len())
        .checked_mul(SOURCE_WORK_PER_BYTE)
        .ok_or_else(|| GrammarError::Resource(governor.stop_resource(Resource::Work)))?;
    governor.charge(Resource::Work, work)?;
    let mut parser = Parser {
        lexer: Lexer {
            text: text.as_bytes(),
            pos: 0,
        },
        token: Token::End,
        at: 0,
        depth: 0,
        governor,
    };
    parser.advance()?;
    parser.document()
}

#[cfg(test)]
mod tests {
    use super::{PExpr, Statement, parse};
    use crate::grammar::GrammarError;
    use crate::{Budget, Governor};

    #[test]
    fn parses_statements_and_escapes() {
        let mut governor = Governor::new(Budget::default());
        let document = parse(
            "grammar \"t.v1\"; # header\nclose = \"</a>\" | \"</b>\";\nx = \"\\u{FF5C}\\x41\" name:/[a-z]+/* ;",
            &mut governor,
        );
        let document = match document {
            Ok(document) => document,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(document.id.as_str(), "t.v1");
        assert!(
            matches!(&document.statements.as_slice()[0], Statement::Production { name, .. } if name.as_str() == "close")
        );
        let Statement::Production { expr, .. } = &document.statements.as_slice()[1] else {
            panic!("production expected");
        };
        let PExpr::Sequence(items, _, _) = expr else {
            panic!("sequence expected");
        };
        let PExpr::Literal { bytes, at } = &items.as_slice()[0] else {
            panic!("literal expected");
        };
        assert_eq!(bytes.as_slice(), "｜A".as_bytes());
        assert_eq!(*at, 54);
        assert!(
            matches!(&items.as_slice()[1], PExpr::Capture { kind, inner, .. }
            if kind.as_str() == "name" && matches!(inner.get(), PExpr::Repeat { op: b'*', .. }))
        );
    }

    #[test]
    fn errors_carry_offsets() {
        let mut governor = Governor::new(Budget::default());
        let error = parse("grammar \"t\";\nx = \"a\" | ;", &mut governor).unwrap_err();
        assert_eq!(
            error,
            GrammarError::Syntax {
                offset: 23,
                message: "expected an expression".into()
            }
        );
    }

    #[test]
    fn syntax_resource_limits_release_unpublished_tree_and_diagnostic_storage() {
        use crate::{CancellationToken, StopReason, contract::Resource};
        let source = "grammar \"budget\"; x = name:/[a-z]+/ (\"a\" | \"b\")?;";
        let mut g = Governor::new(Budget::default());
        let document = parse(source, &mut g).unwrap();
        let usage = g.usage();
        drop(document);
        assert_eq!(g.scratch_meter().live(), 0);
        let exact = Budget {
            input_bytes: u64::try_from(source.len()).unwrap(),
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            ..Budget::default()
        };
        let mut g = Governor::new(exact);
        drop(parse(source, &mut g).unwrap());
        assert_eq!(g.scratch_meter().live(), 0);
        for (budget, resource) in [
            (
                Budget {
                    input_bytes: exact.input_bytes - 1,
                    ..exact
                },
                Resource::InputBytes,
            ),
            (
                Budget {
                    work: exact.work - 1,
                    ..exact
                },
                Resource::Work,
            ),
            (
                Budget {
                    scratch_bytes: exact.scratch_bytes - 1,
                    ..exact
                },
                Resource::ScratchBytes,
            ),
        ] {
            let mut g = Governor::new(budget);
            assert_eq!(
                parse(source, &mut g).unwrap_err(),
                GrammarError::Resource(StopReason::BudgetExceeded(resource))
            );
            assert_eq!(g.check(), Err(StopReason::BudgetExceeded(resource)));
            assert_eq!(g.scratch_meter().live(), 0);
        }
        let token = CancellationToken::new();
        token.cancel();
        let mut g = Governor::with_cancellation(exact, token);
        assert_eq!(
            parse(source, &mut g).unwrap_err(),
            GrammarError::Resource(StopReason::Cancelled)
        );
        assert_eq!(g.usage().work, 0);
        assert_eq!(g.scratch_meter().live(), 0);
        assert_error_ownership();
    }

    fn assert_error_ownership() {
        // The same escaping syntax diagnostic owns the same admission even
        // after a larger private tree has been built and then discarded.
        let mut retained = None;
        for prefix in ["", "unused = \"a\" | \"b\"; "] {
            let source = format!("grammar \"g\"; {prefix}broken = ;");
            let mut g = Governor::new(Budget::default());
            assert_eq!(
                parse(&source, &mut g).unwrap_err(),
                GrammarError::Syntax {
                    offset: source.rfind(';').unwrap(),
                    message: "expected an expression".into(),
                }
            );
            match retained {
                None => retained = Some(g.scratch_meter().live()),
                Some(expected) => assert_eq!(g.scratch_meter().live(), expected),
            }
        }
        // Later semantic lowering is outside this phase. Its identical early
        // refusal must not retain either syntax identity or unused syntax data.
        let mut retained = None;
        for (id, unused) in [
            ("g".to_string(), String::new()),
            (
                "g".repeat(4096),
                format!(" unused = \"{}\";", "x".repeat(4096)),
            ),
        ] {
            let source = format!("grammar \"{id}\"; call = unknown;{unused}");
            let mut g = Governor::new(Budget::default());
            assert_eq!(
                crate::grammar::Grammar::compile_governed(&source, &mut g).unwrap_err(),
                GrammarError::UnknownName {
                    offset: source.find("unknown").unwrap(),
                    name: "unknown".into()
                }
            );
            match retained {
                None => retained = Some(g.scratch_meter().live()),
                Some(expected) => assert_eq!(g.scratch_meter().live(), expected),
            }
        }
    }

    #[test]
    fn native_and_caller_depth_bound_postfix_height_and_parser_recursion() {
        use crate::{StopReason, contract::Resource};
        for postfix in [true, false] {
            for count in [127, 128] {
                let expression = if postfix {
                    format!("\"x\"{}", "?".repeat(count))
                } else {
                    format!("{}\"x\"{}", "(".repeat(count), ")".repeat(count))
                };
                let source = format!(
                    "grammar \"depth\"; call = \"c\"; result = \"r\"; declaration = \"d\"; deep = {expression};"
                );
                let mut g = Governor::new(Budget {
                    depth: 1024,
                    ..Budget::default()
                });
                if count == 127 {
                    // Exercises the actual public compiler and downstream borrow consumers.
                    crate::grammar::Grammar::compile_governed(&source, &mut g).unwrap();
                } else {
                    assert_eq!(
                        parse(&source, &mut g).unwrap_err(),
                        GrammarError::Resource(StopReason::BudgetExceeded(Resource::Depth))
                    );
                    assert_eq!(g.check(), Err(StopReason::BudgetExceeded(Resource::Depth)));
                    assert_eq!(g.scratch_meter().live(), 0);
                }
            }
        }
        let mut g = Governor::new(Budget {
            depth: 2,
            ..Budget::default()
        });
        assert_eq!(
            parse("grammar \"caller\"; x = \"a\"??;", &mut g).unwrap_err(),
            GrammarError::Resource(StopReason::BudgetExceeded(Resource::Depth))
        );
        assert_eq!(g.scratch_meter().live(), 0);
    }
}
