// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Lexer and parser of the grammar DSL (see the [module documentation](super)).
//!
//! Parsing charges `InputBytes` for the source, one `Work` unit per source
//! byte and per syntax node, and `Depth` for every nesting level; it builds
//! a positioned syntax tree that [`super::compile`] resolves and checks.
// `unreachable_pub` and `redundant_pub_crate` disagree on the right
// visibility for items of a crate-private module
// (https://github.com/rust-lang/rust-clippy/issues/6188); these items are
// shared across the grammar and tool-call modules, so `pub(crate)` (matching
// `unreachable_pub`) wins, as in `pattern::derive`.
#![allow(clippy::redundant_pub_crate)]
use super::GrammarError;
use super::cost::K_COMPILE;
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource};

/// One positioned expression of the DSL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PExpr {
    /// `"bytes"`.
    Literal { bytes: Vec<u8>, at: usize },
    /// `/pattern/`.
    Class { source: String, at: usize },
    /// `until("a", "b")`.
    Until { literals: Vec<Vec<u8>>, at: usize },
    /// `slot(codec, "a", "b")`.
    Slot {
        codec: String,
        codec_at: usize,
        literals: Vec<Vec<u8>>,
        at: usize,
    },
    /// A production or closer name.
    Name { name: String, at: usize },
    /// `kind:item`.
    Capture {
        kind: String,
        at: usize,
        inner: Box<Self>,
    },
    /// Juxtaposition.
    Sequence(Vec<Self>, usize),
    /// `a | b`.
    Choice(Vec<Self>, usize),
    /// `item*`, `item+`, `item?`.
    Repeat { inner: Box<Self>, op: u8, at: usize },
    /// `commit`: the attempt is a call (or result) attempt from here on.
    Commit,
    /// `next`: the next call of the same attempt begins here.
    Next { at: usize },
    /// `insert("bytes")`: a token the model omitted, read as inserted.
    Insert { bytes: Vec<u8> },
}

/// One top-level statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Statement {
    Reasoning {
        open: Vec<u8>,
        close: Vec<u8>,
        at: usize,
    },
    Confusable {
        canonical: Vec<u8>,
        variant: Vec<u8>,
        at: usize,
    },
    Closer {
        name: String,
        spellings: Vec<Vec<u8>>,
        at: usize,
    },
    Layout {
        name: String,
        text: Vec<u8>,
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
        name: String,
        expr: PExpr,
        at: usize,
    },
}

/// A parsed grammar file.
#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub(crate) id: String,
    pub(crate) statements: Vec<Statement>,
}

/// Names that cannot name a production or closer.
pub(crate) const KEYWORDS: [&str; 13] = [
    "grammar",
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Str(Vec<u8>),
    Class(String),
    Punct(u8),
    End,
}

struct Lexer<'a> {
    text: &'a [u8],
    pos: usize,
}

fn syntax(offset: usize, message: impl Into<String>) -> GrammarError {
    GrammarError::Syntax {
        offset,
        message: message.into(),
    }
}

const fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

const fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

impl Lexer<'_> {
    fn skip_trivia(&mut self) {
        while let Some(&byte) = self.text.get(self.pos) {
            if byte.is_ascii_whitespace() {
                self.pos += 1;
            } else if byte == b'#' {
                while self.text.get(self.pos).is_some_and(|&byte| byte != b'\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Result<(Token, usize), GrammarError> {
        self.skip_trivia();
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
                self.pos += 1;
            }
            let ident = String::from_utf8(self.text[start..self.pos].to_vec())
                .map_err(|_| syntax(start, "identifier is not UTF-8"))?;
            return Ok((Token::Ident(ident), start));
        }
        match byte {
            b'"' => self.string(start).map(|bytes| (Token::Str(bytes), start)),
            b'/' => self
                .class(start)
                .map(|source| (Token::Class(source), start)),
            b'=' | b'|' | b';' | b'(' | b')' | b'*' | b'+' | b'?' | b':' | b',' => {
                self.pos += 1;
                Ok((Token::Punct(byte), start))
            }
            _ => Err(syntax(
                start,
                format!("unexpected character {:?}", char::from(byte)),
            )),
        }
    }

    fn string(&mut self, start: usize) -> Result<Vec<u8>, GrammarError> {
        self.pos += 1;
        let mut bytes = Vec::new();
        loop {
            let Some(&byte) = self.text.get(self.pos) else {
                return Err(syntax(start, "unterminated string literal"));
            };
            self.pos += 1;
            match byte {
                b'"' => return Ok(bytes),
                b'\n' => return Err(syntax(start, "unterminated string literal")),
                b'\\' => self.escape(&mut bytes)?,
                _ => bytes.push(byte),
            }
        }
    }

    fn escape(&mut self, bytes: &mut Vec<u8>) -> Result<(), GrammarError> {
        let at = self.pos - 1;
        let Some(&kind) = self.text.get(self.pos) else {
            return Err(syntax(at, "unterminated escape"));
        };
        self.pos += 1;
        match kind {
            b'\\' | b'"' => bytes.push(kind),
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b't' => bytes.push(b'\t'),
            b'x' => {
                let digits = self
                    .text
                    .get(self.pos..self.pos + 2)
                    .ok_or_else(|| syntax(at, "\\x needs two hexadecimal digits"))?;
                let text = core::str::from_utf8(digits)
                    .map_err(|_| syntax(at, "\\x needs two hexadecimal digits"))?;
                let value = u8::from_str_radix(text, 16)
                    .map_err(|_| syntax(at, "\\x needs two hexadecimal digits"))?;
                self.pos += 2;
                bytes.push(value);
            }
            b'u' => {
                if self.text.get(self.pos) != Some(&b'{') {
                    return Err(syntax(at, "\\u needs braces: \\u{HEX}"));
                }
                let close = self.text[self.pos..]
                    .iter()
                    .position(|&byte| byte == b'}')
                    .ok_or_else(|| syntax(at, "unterminated \\u{...} escape"))?;
                let digits = &self.text[self.pos + 1..self.pos + close];
                let scalar = core::str::from_utf8(digits)
                    .ok()
                    .filter(|text| !text.is_empty() && text.len() <= 6)
                    .and_then(|text| u32::from_str_radix(text, 16).ok())
                    .and_then(char::from_u32)
                    .ok_or_else(|| syntax(at, "\\u{...} must name a Unicode scalar value"))?;
                self.pos += close + 1;
                let mut buffer = [0; 4];
                bytes.extend_from_slice(scalar.encode_utf8(&mut buffer).as_bytes());
            }
            _ => {
                return Err(syntax(at, format!("unknown escape \\{}", char::from(kind))));
            }
        }
        Ok(())
    }

    fn class(&mut self, start: usize) -> Result<String, GrammarError> {
        self.pos += 1;
        let mut source = Vec::new();
        loop {
            let Some(&byte) = self.text.get(self.pos) else {
                return Err(syntax(start, "unterminated class /.../"));
            };
            self.pos += 1;
            match byte {
                b'/' => break,
                b'\n' => return Err(syntax(start, "unterminated class /.../")),
                b'\\' if self.text.get(self.pos) == Some(&b'/') => {
                    self.pos += 1;
                    source.push(b'/');
                }
                b'\\' => {
                    source.push(b'\\');
                    if let Some(&next) = self.text.get(self.pos) {
                        self.pos += 1;
                        source.push(next);
                    }
                }
                _ => source.push(byte),
            }
        }
        String::from_utf8(source).map_err(|_| syntax(start, "class is not UTF-8"))
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
        let (token, at) = self.lexer.next()?;
        self.token = token;
        self.at = at;
        Ok(())
    }

    fn node(&mut self) -> Result<(), GrammarError> {
        self.governor
            .charge(Resource::Work, K_COMPILE)
            .map_err(GrammarError::Resource)
    }

    fn enter(&mut self) -> Result<(), GrammarError> {
        self.depth += 1;
        self.governor
            .charge(Resource::Depth, u64::from(self.depth))
            .map_err(GrammarError::Resource)
    }

    fn expect(&mut self, punct: u8) -> Result<(), GrammarError> {
        if self.token == Token::Punct(punct) {
            self.advance()
        } else {
            Err(syntax(self.at, format!("expected `{}`", char::from(punct))))
        }
    }

    fn string(&mut self) -> Result<(Vec<u8>, usize), GrammarError> {
        let at = self.at;
        let Token::Str(bytes) = &self.token else {
            return Err(syntax(at, "expected a string literal"));
        };
        if bytes.is_empty() {
            return Err(GrammarError::EmptyLiteral { offset: at });
        }
        let bytes = bytes.clone();
        self.advance()?;
        Ok((bytes, at))
    }

    fn ident(&mut self) -> Result<(String, usize), GrammarError> {
        let at = self.at;
        let Token::Ident(name) = &self.token else {
            return Err(syntax(at, "expected a name"));
        };
        let name = name.clone();
        self.advance()?;
        Ok((name, at))
    }

    fn document(&mut self) -> Result<Document, GrammarError> {
        if self.token != Token::Ident("grammar".into()) {
            return Err(syntax(self.at, "a grammar starts with `grammar \"id\";`"));
        }
        self.advance()?;
        let (id, id_at) = self.string()?;
        let id = String::from_utf8(id).map_err(|_| syntax(id_at, "grammar id is not UTF-8"))?;
        if !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(syntax(id_at, "grammar id must match [A-Za-z0-9._-]+"));
        }
        self.expect(b';')?;
        let mut statements = Vec::new();
        while self.token != Token::End {
            self.node()?;
            statements.push(self.statement()?);
        }
        Ok(Document { id, statements })
    }

    fn statement(&mut self) -> Result<Statement, GrammarError> {
        let (name, at) = self.ident()?;
        let statement = match name.as_str() {
            "reasoning" => {
                let (open, _) = self.string()?;
                let (close, _) = self.string()?;
                Statement::Reasoning { open, close, at }
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
                reserved(&name, name_at)?;
                self.expect(b'=')?;
                let mut spellings = vec![self.string()?.0];
                while self.token == Token::Punct(b'|') {
                    self.advance()?;
                    spellings.push(self.string()?.0);
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
            "optional-id" => Statement::OptionalId { at },
            "declaration-content" => Statement::DeclarationContent { at },
            "json-envelope" => Statement::JsonEnvelope { at },
            "grammar" => return Err(syntax(at, "duplicate `grammar` header")),
            _ => {
                reserved(&name, at)?;
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
        let mut alternatives = vec![self.sequence()?];
        while self.token == Token::Punct(b'|') {
            self.advance()?;
            alternatives.push(self.sequence()?);
        }
        self.depth -= 1;
        Ok(if alternatives.len() == 1 {
            alternatives.pop().expect("one alternative")
        } else {
            PExpr::Choice(alternatives, at)
        })
    }

    fn sequence(&mut self) -> Result<PExpr, GrammarError> {
        let at = self.at;
        let mut items = Vec::new();
        while !matches!(self.token, Token::Punct(b'|' | b')' | b';') | Token::End) {
            items.push(self.postfixed()?);
        }
        match items.len() {
            0 => Err(syntax(at, "expected an expression")),
            1 => Ok(items.pop().expect("one item")),
            _ => Ok(PExpr::Sequence(items, at)),
        }
    }

    fn postfixed(&mut self) -> Result<PExpr, GrammarError> {
        let mut item = self.atom()?;
        while let Token::Punct(op @ (b'*' | b'+' | b'?')) = self.token {
            self.node()?;
            let at = self.at;
            self.advance()?;
            item = PExpr::Repeat {
                inner: Box::new(item),
                op,
                at,
            };
        }
        Ok(item)
    }

    fn literal_list(&mut self) -> Result<Vec<Vec<u8>>, GrammarError> {
        let mut literals = vec![self.string()?.0];
        while self.token == Token::Punct(b',') {
            self.advance()?;
            literals.push(self.string()?.0);
        }
        Ok(literals)
    }

    fn atom(&mut self) -> Result<PExpr, GrammarError> {
        self.node()?;
        let at = self.at;
        match self.token.clone() {
            Token::Str(_) => {
                let (bytes, at) = self.string()?;
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
                    ("commit", token) if *token != Token::Punct(b':') => Ok(PExpr::Commit),
                    ("next", token) if *token != Token::Punct(b':') => Ok(PExpr::Next { at }),
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
                            inner: Box::new(inner),
                        })
                    }
                    _ => {
                        reserved(&name, at)?;
                        Ok(PExpr::Name { name, at })
                    }
                }
            }
            _ => Err(syntax(at, "expected an expression")),
        }
    }
}

fn reserved(name: &str, offset: usize) -> Result<(), GrammarError> {
    if KEYWORDS.contains(&name) {
        return Err(GrammarError::ReservedName {
            offset,
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Parse `text` into a positioned document.
pub(crate) fn parse(text: &str, governor: &mut Governor) -> Result<Document, GrammarError> {
    governor
        .charge(Resource::InputBytes, size(text.len()))
        .map_err(GrammarError::Resource)?;
    governor
        .charge(Resource::Work, size(text.len()).saturating_mul(K_COMPILE))
        .map_err(GrammarError::Resource)?;
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
        assert_eq!(document.id, "t.v1");
        assert!(
            matches!(&document.statements[0], Statement::Production { name, .. } if name == "close")
        );
        let Statement::Production { expr, .. } = &document.statements[1] else {
            panic!("production expected");
        };
        let PExpr::Sequence(items, _) = expr else {
            panic!("sequence expected");
        };
        assert_eq!(
            items[0],
            PExpr::Literal {
                bytes: "｜A".as_bytes().to_vec(),
                at: 54
            }
        );
        assert!(matches!(&items[1], PExpr::Capture { kind, inner, .. }
            if kind == "name" && matches!(**inner, PExpr::Repeat { op: b'*', .. })));
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
}
