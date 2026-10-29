// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Native Python literal arguments. Source coordinates never pass through a
//! rewritten JSON representation, and no Python expression is executed.
use super::{Codec, CodecContext, CodecEnd, CodecFault, CodecKind, CodecStep, CodecValue};
use crate::contract::{Resource, ScratchLease, ScratchString, ScratchVec, saturating_size as size};
use crate::json::{Member, Node, NumberParseError, OwnedDocument, Value};
use crate::{Governor, StopReason};

/// Keyword arguments whose values are Python literals.
///
/// Strings retain Python quote and escape semantics; lists, dictionaries,
/// numbers, `True`, `False` and `None` become their corresponding JSON values.
/// Positional arguments and executable expressions are refused. The codec ends
/// immediately before the call's closing parenthesis, outside every literal.
///
/// ```
/// use purrassist_core::grammar::codec::{Codec, CodecContext, CodecEnd, PythonicCodec};
/// use purrassist_core::{Budget, Governor};
/// let input = b"name='cat', enabled=True";
/// let mut governor = Governor::new(Budget::default());
/// let value = PythonicCodec::begin(0, &mut governor)?
///     .finish(input, 0..input.len(), CodecEnd::Delimited, CodecContext::default(), &mut governor)?.unwrap();
/// assert_eq!(value.tree.root.get("name").unwrap().as_str(), Some("cat"));
/// # Ok::<(), purrassist_core::StopReason>(())
/// ```
#[derive(Debug)]
pub struct PythonicCodec {
    start: usize,
    cursor: Cursor,
    end: Option<usize>,
}

/// The lexical boundary authority shared by streaming framing and parsing.
#[derive(Debug)]
struct Cursor {
    depth: usize,
    quote: Quote,
}
#[derive(Clone, Copy, Debug)]
enum Quote {
    Outside,
    Opening {
        byte: u8,
        count: u8,
    },
    Inside {
        byte: u8,
        triple: bool,
        escaped: bool,
        run: u8,
    },
}
impl Cursor {
    const fn new() -> Self {
        Self {
            depth: 0,
            quote: Quote::Outside,
        }
    }
    fn feed(&mut self, byte: u8, governor: &mut Governor) -> Result<bool, StopReason> {
        governor.charge(Resource::Work, 1)?;
        match self.quote {
            Quote::Opening { byte: quote, count } if byte == quote => {
                self.quote = if count == 2 {
                    Quote::Inside {
                        byte: quote,
                        triple: true,
                        escaped: false,
                        run: 0,
                    }
                } else {
                    Quote::Opening {
                        byte: quote,
                        count: 2,
                    }
                };
                return Ok(false);
            }
            Quote::Opening {
                byte: quote,
                count: 1,
            } => {
                self.quote = Quote::Inside {
                    byte: quote,
                    triple: false,
                    escaped: false,
                    run: 0,
                };
                self.feed_quoted(byte);
                return Ok(false);
            }
            Quote::Opening { .. } => self.quote = Quote::Outside,
            Quote::Inside { .. } => {
                self.feed_quoted(byte);
                return Ok(false);
            }
            Quote::Outside => {}
        }
        match byte {
            b'\'' | b'"' => self.quote = Quote::Opening { byte, count: 1 },
            b'[' | b'{' => {
                let depth = self
                    .depth
                    .checked_add(1)
                    .ok_or_else(|| governor.stop_resource(Resource::Depth))?;
                governor.charge(Resource::Depth, size(depth))?;
                self.depth = depth;
            }
            b']' | b'}' if self.depth > 0 => self.depth -= 1,
            b')' if self.depth == 0 => return Ok(true),
            _ => {}
        }
        Ok(false)
    }
    fn feed_quoted(&mut self, found: u8) {
        let Quote::Inside {
            byte,
            triple,
            escaped,
            run,
        } = self.quote
        else {
            unreachable!("quote state")
        };
        self.quote = if escaped {
            Quote::Inside {
                byte,
                triple,
                escaped: false,
                run: 0,
            }
        } else if found == b'\\' {
            Quote::Inside {
                byte,
                triple,
                escaped: true,
                run: 0,
            }
        } else if found == byte {
            if !triple || run == 2 {
                Quote::Outside
            } else {
                Quote::Inside {
                    byte,
                    triple,
                    escaped: false,
                    run: run + 1,
                }
            }
        } else {
            Quote::Inside {
                byte,
                triple,
                escaped: false,
                run: 0,
            }
        };
    }
}

impl Codec for PythonicCodec {
    const KIND: CodecKind = CodecKind::Pythonic;
    fn begin(at: usize, governor: &mut Governor) -> Result<Self, StopReason> {
        governor.check()?;
        Ok(Self {
            start: at,
            cursor: Cursor::new(),
            end: None,
        })
    }
    fn feed(
        &mut self,
        byte: u8,
        at: usize,
        governor: &mut Governor,
    ) -> Result<CodecStep, StopReason> {
        if self.end.is_some() || self.cursor.feed(byte, governor)? {
            self.end.get_or_insert(at);
            Ok(CodecStep::Complete {
                end: self.end.expect("completed"),
            })
        } else {
            Ok(CodecStep::Consumed)
        }
    }
    fn finish(
        self,
        body: &[u8],
        span: core::ops::Range<usize>,
        _end: CodecEnd,
        context: CodecContext<'_>,
        governor: &mut Governor,
    ) -> Result<Result<CodecValue, CodecFault>, StopReason> {
        debug_assert_eq!(self.start, span.start);
        let mut reader = Reader {
            bytes: body,
            at: 0,
            base: span.start,
            governor,
            edits: Vec::new(),
        };
        let document = match reader.arguments() {
            Ok(document) => document,
            Err(Failure::Resource(reason)) => return Err(reason),
            Err(Failure::Syntax { at, positional }) => {
                return Ok(Err(if positional {
                    CodecFault::PythonicPositionalRejected { at }
                } else {
                    CodecFault::Malformed { at }
                }));
            }
        };
        let (mut tree, _) = super::json::tree(
            document.into_node(),
            Vec::new(),
            crate::tool_call::ArgOrigin::Pythonic,
            body,
            span.start,
            reader.governor,
        )?;
        tree.tool = context.shape.map(|shape| shape.tool.clone());
        super::value(tree, reader.edits, reader.governor)
    }
}

/// Native literal syntax refusal or the unchanged caller resource stop.
#[derive(Debug)]
enum Failure {
    Resource(StopReason),
    Syntax { at: usize, positional: bool },
}
impl From<StopReason> for Failure {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}
type Parsed<T> = Result<T, Failure>;
struct Reader<'a, 'g> {
    bytes: &'a [u8],
    at: usize,
    base: usize,
    governor: &'g mut Governor,
    edits: Vec<crate::grammar::edit::MarkupEdit>,
}
impl Reader<'_, '_> {
    const fn fail<T>(&self) -> Parsed<T> {
        Err(Failure::Syntax {
            at: self.base + self.at,
            positional: false,
        })
    }
    fn advance(&mut self) -> Parsed<u8> {
        let byte = *self.bytes.get(self.at).ok_or(Failure::Syntax {
            at: self.base + self.at,
            positional: false,
        })?;
        self.governor.charge(Resource::Work, 1)?;
        self.at += 1;
        Ok(byte)
    }
    fn space(&mut self) -> Parsed<()> {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.advance()?;
        }
        Ok(())
    }
    fn take(&mut self, byte: u8) -> Parsed<bool> {
        if self.bytes.get(self.at) == Some(&byte) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn identifier(&mut self) -> Parsed<ScratchString> {
        let start = self.at;
        if !self
            .bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            return self.fail();
        }
        self.advance()?;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.advance()?;
        }
        let mut text = ScratchString::new(self.governor);
        text.push_str(
            core::str::from_utf8(&self.bytes[start..self.at]).expect("ASCII identifier"),
        )?;
        Ok(text)
    }
    fn arguments(&mut self) -> Parsed<OwnedDocument> {
        let mut members = ScratchVec::new(self.governor);
        let mut lease = ScratchLease::empty(self.governor);
        self.space()?;
        while self.at < self.bytes.len() {
            let start = self.at;
            let key = match self.identifier() {
                Ok(key) => key,
                Err(Failure::Syntax { .. }) => {
                    return Err(Failure::Syntax {
                        at: self.base + start,
                        positional: true,
                    });
                }
                Err(error) => return Err(error),
            };
            let end = self.at;
            if !identifier(key.as_str()) {
                return self.fail();
            }
            self.space()?;
            if !self.take(b'=')? || self.bytes.get(self.at) == Some(&b'=') {
                return Err(Failure::Syntax {
                    at: self.base + start,
                    positional: true,
                });
            }
            self.space()?;
            let value = self.literal(1)?;
            let (key, key_owner) = key.into_owned_parts();
            let (value, value_owner) = value.into_owned_parts();
            lease.absorb(key_owner);
            lease.absorb(value_owner);
            members.push(Member {
                key,
                key_span: Some(self.base + start..self.base + end),
                value,
            })?;
            self.space()?;
            if self.at == self.bytes.len() {
                break;
            }
            if !self.take(b',')? {
                return self.fail();
            }
            self.space()?;
        }
        let mut document = OwnedDocument::object(members, lease);
        document.set_span(Some(self.base..self.base + self.bytes.len()));
        Ok(document)
    }
    fn literal(&mut self, depth: u32) -> Parsed<OwnedDocument> {
        self.governor.charge(Resource::Depth, u64::from(depth))?;
        if depth > 128 {
            return Err(self.governor.stop_resource(Resource::Depth).into());
        }
        let start = self.at;
        let Some(&byte) = self.bytes.get(self.at) else {
            return self.fail();
        };
        let mut value = match byte {
            b'\'' | b'"' => OwnedDocument::string(self.string(false)?),
            b'r' | b'R' | b'u' | b'U'
                if self
                    .bytes
                    .get(self.at + 1)
                    .is_some_and(|byte| matches!(byte, b'\'' | b'"')) =>
            {
                self.advance()?;
                OwnedDocument::string(self.string(matches!(byte, b'r' | b'R'))?)
            }
            b'[' => self.array(depth)?,
            b'{' => self.object(depth)?,
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number()?,
            b'T' if self.word(b"True")? => OwnedDocument::boolean(true, self.governor),
            b'F' if self.word(b"False")? => OwnedDocument::boolean(false, self.governor),
            b'N' if self.word(b"None")? => OwnedDocument::null(self.governor),
            _ => return self.fail(),
        };
        value.set_span(Some(self.base + start..self.base + self.at));
        Ok(value)
    }
    fn word(&mut self, word: &[u8]) -> Parsed<bool> {
        self.governor.charge(Resource::Work, size(word.len()))?;
        if self.bytes[self.at..].starts_with(word) {
            self.at += word.len();
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn array(&mut self, depth: u32) -> Parsed<OwnedDocument> {
        self.advance()?;
        let mut items = ScratchVec::new(self.governor);
        let mut lease = ScratchLease::empty(self.governor);
        self.space()?;
        while !self.take(b']')? {
            let (item, owner) = self.literal(depth + 1)?.into_owned_parts();
            lease.absorb(owner);
            items.push(item)?;
            self.space()?;
            if self.take(b']')? {
                break;
            }
            if !self.take(b',')? {
                return self.fail();
            }
            self.space()?;
        }
        Ok(OwnedDocument::array(items, lease))
    }
    fn object(&mut self, depth: u32) -> Parsed<OwnedDocument> {
        self.advance()?;
        let mut members = ScratchVec::new(self.governor);
        let mut lease = ScratchLease::empty(self.governor);
        self.space()?;
        while !self.take(b'}')? {
            let start = self.at;
            let key = self.literal(depth + 1)?;
            if !matches!(key.root().value, Value::String(_)) {
                return self.fail();
            }
            let end = self.at;
            self.space()?;
            if !self.take(b':')? {
                return self.fail();
            }
            self.space()?;
            let (value, owner) = self.literal(depth + 1)?.into_owned_parts();
            let (key, key_owner) = key.into_owned_parts();
            let Value::String(key) = key.value else {
                unreachable!("string key")
            };
            lease.absorb(owner);
            lease.absorb(key_owner);
            members.push(Member {
                key,
                key_span: Some(self.base + start..self.base + end),
                value,
            })?;
            self.space()?;
            if self.take(b'}')? {
                break;
            }
            if !self.take(b',')? {
                return self.fail();
            }
            self.space()?;
        }
        Ok(OwnedDocument::object(members, lease))
    }
    fn string(&mut self, raw: bool) -> Parsed<ScratchString> {
        let quote = self.advance()?;
        let triple = self.bytes.get(self.at..self.at + 2) == Some(&[quote, quote]);
        if triple {
            self.advance()?;
            self.advance()?;
        }
        let mut text = ScratchString::new(self.governor);
        loop {
            if self.bytes.get(self.at) == Some(&quote)
                && (!triple || self.bytes.get(self.at..self.at + 3) == Some(&[quote, quote, quote]))
            {
                self.advance()?;
                if triple {
                    self.advance()?;
                    self.advance()?;
                }
                return Ok(text);
            }
            let Some(&byte) = self.bytes.get(self.at) else {
                return self.fail();
            };
            if !triple && matches!(byte, b'\n' | b'\r') {
                return self.fail();
            }
            if byte == b'\\' {
                self.advance()?;
                if raw {
                    text.push('\\')?;
                    let escaped = self.advance()?;
                    if escaped == b'\r' {
                        self.take(b'\n')?;
                        text.push('\n')?;
                    } else if escaped.is_ascii() {
                        text.push(char::from(escaped))?;
                    } else {
                        self.at -= 1;
                        self.utf8_character(&mut text)?;
                    }
                } else {
                    self.escape(&mut text)?;
                }
            } else if byte == b'\r' {
                self.advance()?;
                self.take(b'\n')?;
                text.push('\n')?;
            } else if byte.is_ascii() {
                self.advance()?;
                text.push(char::from(byte))?;
            } else {
                self.utf8_run(&mut text, quote)?;
            }
        }
    }
    fn utf8_run(&mut self, text: &mut ScratchString, quote: u8) -> Parsed<()> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|byte| {
            !byte.is_ascii() || (*byte != quote && *byte != b'\\' && !matches!(byte, b'\r' | b'\n'))
        }) {
            self.advance()?;
        }
        let (run, edits) = super::lossy(
            &self.bytes[start..self.at],
            self.base + start,
            self.governor,
        )?;
        self.edits.extend(edits);
        text.push_str(&run)?;
        Ok(())
    }
    fn utf8_character(&mut self, text: &mut ScratchString) -> Parsed<()> {
        let start = self.at;
        // A UTF-8 scalar occupies at most four bytes. Inspecting the whole
        // suffix here would repeatedly validate it after unknown escapes.
        let end = start.saturating_add(4).min(self.bytes.len());
        self.governor.charge(Resource::Work, size(end - start))?;
        let chunk = self.bytes[start..end]
            .utf8_chunks()
            .next()
            .expect("nonempty character");
        let width = chunk
            .valid()
            .chars()
            .next()
            .map_or_else(|| chunk.invalid().len(), char::len_utf8);
        for _ in 0..width {
            self.advance()?;
        }
        let (decoded, edits) = super::lossy(
            &self.bytes[start..self.at],
            self.base + start,
            self.governor,
        )?;
        self.edits.extend(edits);
        text.push_str(&decoded)?;
        Ok(())
    }

    fn escape(&mut self, text: &mut ScratchString) -> Parsed<()> {
        let byte = self.advance()?;
        let character = match byte {
            b'\\' => '\\',
            b'\'' => '\'',
            b'"' => '"',
            b'a' => '\u{7}',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'v' => '\u{b}',
            b'\n' => return Ok(()),
            b'\r' => {
                self.take(b'\n')?;
                return Ok(());
            }
            b'x' => self.hex_escape(2)?,
            b'u' => self.hex_escape(4)?,
            b'U' => self.hex_escape(8)?,
            b'0'..=b'7' => {
                let mut value = u32::from(byte - b'0');
                for _ in 0..2 {
                    if let Some(digit @ b'0'..=b'7') = self.bytes.get(self.at).copied() {
                        self.advance()?;
                        value = value * 8 + u32::from(digit - b'0');
                    } else {
                        break;
                    }
                }
                char::from_u32(value).expect("three octal digits")
            }
            b'N' => {
                if !self.take(b'{')? {
                    return self.fail();
                }
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|byte| *byte != b'}') {
                    self.advance()?;
                }
                let end = self.at;
                if !self.take(b'}')? {
                    return self.fail();
                }
                let Ok(name) = core::str::from_utf8(&self.bytes[start..end]) else {
                    return self.fail();
                };
                let Some(character) = super::python_names::lookup(name, self.governor)? else {
                    return self.fail();
                };
                character
            }
            _ => {
                // Python preserves unrecognized escapes as two characters.
                text.push('\\')?;
                if byte.is_ascii() {
                    text.push(char::from(byte))?;
                } else {
                    self.at -= 1;
                    self.utf8_character(text)?;
                }
                return Ok(());
            }
        };
        text.push(character)?;
        Ok(())
    }
    fn hex_escape(&mut self, width: usize) -> Parsed<char> {
        let mut value = 0_u32;
        for _ in 0..width {
            let byte = self.advance()?;
            let Some(digit) = char::from(byte).to_digit(16) else {
                return self.fail();
            };
            value = value
                .checked_mul(16)
                .and_then(|value| value.checked_add(digit))
                .ok_or(Failure::Syntax {
                    at: self.base + self.at,
                    positional: false,
                })?;
        }
        char::from_u32(value).ok_or(Failure::Syntax {
            at: self.base + self.at,
            positional: false,
        })
    }
    fn number(&mut self) -> Parsed<OwnedDocument> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
        }) {
            self.advance()?;
        }
        let bytes = self.bytes;
        let source = &bytes[start..self.at];
        let negative = source.first() == Some(&b'-');
        let unsigned = if matches!(source.first(), Some(b'+' | b'-')) {
            &source[1..]
        } else {
            source
        };
        let base = if unsigned.starts_with(b"0x") || unsigned.starts_with(b"0X") {
            16
        } else if unsigned.starts_with(b"0o") || unsigned.starts_with(b"0O") {
            8
        } else if unsigned.starts_with(b"0b") || unsigned.starts_with(b"0B") {
            2
        } else {
            10
        };
        let text = if base == 10 {
            self.decimal_number(unsigned, negative)?
        } else {
            self.radix_number(&unsigned[2..], base, negative)?
        };
        match OwnedDocument::number(text, self.governor) {
            Ok(number) => Ok(number),
            Err(NumberParseError::Resource(reason)) => Err(reason.into()),
            Err(NumberParseError::Syntax(_)) => self.fail(),
        }
    }
    fn radix_number(&mut self, radix: &[u8], base: u32, negative: bool) -> Parsed<ScratchString> {
        let mut text = ScratchString::new(self.governor);
        let mut digits = ScratchVec::new(self.governor);
        digits.push(0_u8)?;
        let mut count = 0;
        for (index, &byte) in radix.iter().enumerate() {
            if byte == b'_' {
                if radix
                    .get(index + 1)
                    .is_none_or(|byte| char::from(*byte).to_digit(base).is_none())
                    || (index > 0 && radix[index - 1] == b'_')
                {
                    return self.fail();
                }
                continue;
            }
            let Some(mut carry) = char::from(byte).to_digit(base) else {
                return self.fail();
            };
            count += 1;
            self.governor.charge(Resource::Work, size(digits.len()))?;
            for digit in digits.as_mut_slice() {
                carry += u32::from(*digit) * base;
                *digit = u8::try_from(carry % 10).expect("decimal digit");
                carry /= 10;
            }
            while carry > 0 {
                digits.push(u8::try_from(carry % 10).expect("decimal digit"))?;
                carry /= 10;
            }
        }
        if count == 0 {
            return self.fail();
        }
        if negative {
            text.push('-')?;
        }
        for &digit in digits.as_slice().iter().rev() {
            text.push(char::from(b'0' + digit))?;
        }
        Ok(text)
    }

    fn decimal_number(&mut self, unsigned: &[u8], negative: bool) -> Parsed<ScratchString> {
        let mut text = ScratchString::new(self.governor);
        // A dot is only punctuation: Python requires a mantissa digit.
        // Check before adding JSON's leading/trailing zero spelling.
        self.governor.charge(Resource::Work, size(unsigned.len()))?;
        if !unsigned
            .iter()
            .take_while(|byte| !matches!(byte, b'e' | b'E'))
            .any(u8::is_ascii_digit)
        {
            return self.fail();
        }
        if negative {
            text.push('-')?;
        }
        for (index, &byte) in unsigned.iter().enumerate() {
            self.governor.charge(Resource::Work, 1)?;
            if byte == b'_' {
                if index == 0
                    || !unsigned[index - 1].is_ascii_digit()
                    || unsigned
                        .get(index + 1)
                        .is_none_or(|byte| !byte.is_ascii_digit())
                {
                    return self.fail();
                }
            } else {
                text.push(char::from(byte))?;
            }
        }
        let offset = usize::from(negative);
        let number = &text.as_str()[offset..];
        if number.starts_with('.') {
            let mut prefixed = ScratchString::new(self.governor);
            if negative {
                prefixed.push('-')?;
            }
            prefixed.push('0')?;
            prefixed.push_str(number)?;
            text = prefixed;
        }
        let dot = text.as_str().find('.');
        if let Some(dot) = dot
            && text
                .as_str()
                .as_bytes()
                .get(dot + 1)
                .is_none_or(|byte| matches!(byte, b'e' | b'E'))
        {
            let mut expanded = ScratchString::new(self.governor);
            expanded.push_str(&text.as_str()[..=dot])?;
            expanded.push('0')?;
            expanded.push_str(&text.as_str()[dot + 1..])?;
            text = expanded;
        }
        // Python allows leading zeros for floats and all-zero integers.
        let number = &text.as_str()[offset..];
        let zeros = number.bytes().take_while(|byte| *byte == b'0').count();
        if zeros > 1 || (zeros == 1 && number.as_bytes().get(1).is_some_and(u8::is_ascii_digit)) {
            let integer_end = number.bytes().take_while(u8::is_ascii_digit).count();
            if integer_end == number.len() && number.bytes().any(|byte| byte != b'0') {
                return self.fail();
            }
            let keep = zeros.min(integer_end.saturating_sub(1));
            let mut trimmed = ScratchString::new(self.governor);
            if negative {
                trimmed.push('-')?;
            }
            trimmed.push_str(&number[keep..])?;
            text = trimmed;
        }
        Ok(text)
    }
}

impl PythonicCodec {
    pub(super) const fn complete_end(&self) -> Option<usize> {
        self.end
    }
}

/// Print only native literal syntax; every keyword must be a Python identifier.
pub(super) fn print(
    arguments: &Node,
    governor: &mut Governor,
) -> Result<String, crate::grammar::print::PrintError> {
    use crate::grammar::print::unsupported;
    let Value::Object(members) = &arguments.value else {
        return Err(unsupported("Pythonic arguments must be an object"));
    };
    let mut output = ScratchString::new(governor);
    for (index, member) in members.iter().enumerate() {
        governor.charge(Resource::Work, size(member.key.len()))?;
        if !identifier(&member.key) {
            return Err(unsupported("Pythonic keyword is not an identifier"));
        }
        if index > 0 {
            output.push_str(", ")?;
        }
        output.push_str(&member.key)?;
        output.push('=')?;
        print_literal(&member.value, &mut output, 0, governor)?;
    }
    Ok(output.into_string())
}
fn identifier(text: &str) -> bool {
    text.as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !matches!(
            text,
            "False"
                | "None"
                | "True"
                | "and"
                | "as"
                | "assert"
                | "async"
                | "await"
                | "break"
                | "class"
                | "continue"
                | "def"
                | "del"
                | "elif"
                | "else"
                | "except"
                | "finally"
                | "for"
                | "from"
                | "global"
                | "if"
                | "import"
                | "in"
                | "is"
                | "lambda"
                | "nonlocal"
                | "not"
                | "or"
                | "pass"
                | "raise"
                | "return"
                | "try"
                | "while"
                | "with"
                | "yield"
        )
}
fn print_literal(
    node: &Node,
    output: &mut ScratchString,
    depth: u32,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    governor.charge(Resource::Work, 1)?;
    if depth > 128 {
        return Err(governor.stop_resource(Resource::Depth));
    }
    match &node.value {
        Value::Null => output.push_str("None")?,
        Value::Bool(value) => output.push_str(if *value { "True" } else { "False" })?,
        Value::Number(number) => {
            governor.charge(Resource::Work, size(number.as_str().len()))?;
            output.push_str(number.as_str())?;
        }
        Value::String(text) => output.push_str(&crate::json::quote_governed(text, governor)?)?,
        Value::Array(items) => {
            output.push('[')?;
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ")?;
                }
                print_literal(item, output, depth + 1, governor)?;
            }
            output.push(']')?;
        }
        Value::Object(members) => {
            output.push('{')?;
            for (index, member) in members.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ")?;
                }
                output.push_str(&crate::json::quote_governed(&member.key, governor)?)?;
                output.push_str(": ")?;
                print_literal(&member.value, output, depth + 1, governor)?;
            }
            output.push('}')?;
        }
    }
    Ok(())
}
