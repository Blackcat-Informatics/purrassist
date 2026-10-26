// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Push-based tolerant JSON syntax recovery.
//!
//! [`IncrementalRecovery`] reads one JSON-like value (plus the whitespace and
//! comments after it) from UTF-8 bytes supplied in any number of chunks, and
//! reports every syntax repair it made as a [`SyntaxEdit`]. Parsing state is
//! an explicit stack of open containers and a small lexical mode, so nesting
//! never consumes the Rust call stack and a chunk may end anywhere: inside a
//! string, an escape, a `\u` surrogate pair, a bare token, a comment, the
//! two-character `//`, `/*` and `*/` delimiters, or a multi-byte UTF-8
//! sequence. A lookahead character that might belong to the next token
//! (a `/` that may open a comment, a `\` that may continue a surrogate pair)
//! is held without being consumed until the next character decides it.
//!
//! End-of-input repairs (closing open containers, a missing value,
//! an unterminated string or escape) are made only by
//! [`IncrementalRecovery::finish`], never at a chunk boundary, so the
//! recovered node, the consumed length, the edits and every governor charge
//! are functions of the byte sequence alone, identical for every chunking.
//!
//! # Recovery rules
//!
//! | Rule | Repair | Guessed |
//! | --- | --- | --- |
//! | `json.syntax.comment` | drop a `//` or `/* */` comment | no |
//! | `json.syntax.missing-value` | insert `null` where a value is missing | yes |
//! | `json.syntax.literal-case` | respell `None`/`NULL`/`True`/`FALSE`... | no |
//! | `json.syntax.bare-string` | quote a bare token that is not a literal or number | yes |
//! | `json.syntax.bare-key` | quote a bare object key | no |
//! | `json.syntax.string` | requote a single-, curly- or non-canonically quoted string; close an unterminated one | when unterminated |
//! | `json.syntax.literal-escape` | keep an invalid escape as literal text | yes |
//! | `json.syntax.missing-colon` | insert `:` after a key | no |
//! | `json.syntax.missing-comma` | insert `,` between elements | no |
//! | `json.syntax.trailing-comma` | drop a comma before a closer | no |
//! | `json.syntax.close-container` | close a container left open at end of input | no |
//!
//! A closer of the wrong kind, a value position holding `:`, and an object
//! member without a key are unrecoverable and return
//! [`StopReason::InvalidInput`]; so does malformed UTF-8.
//!
//! # Resource contract
//!
//! Each consumed byte charges one `Work` unit (and is admitted against
//! `InputBytes` cumulatively); each value charges one more `Work` unit and
//! its nesting `Depth`; each edit charges one `EvidenceEntries` entry. Output
//! storage (strings, arrays, members, edits and their replacement text) is
//! charged to `ScratchBytes` by logical length as it grows, by doubling, and
//! retained with the returned value; the open-container stack is transient
//! scratch released when the parser is dropped. There is no up-front charge
//! proportional to the input.
use super::{Member, Node, Number, SyntaxEdit, Value, push_char, reserve_one};
use crate::contract::saturating_size as size;
use crate::contract::{Governor, Resource, ScratchCharge, ScratchVec, StopReason};
use core::ops::Range;

/// Recover one value from a complete input: a single-chunk
/// [`IncrementalRecovery`] run, returning the value, the bytes consumed
/// (the value plus trailing whitespace and comments) and every edit.
///
/// # Errors
/// As [`IncrementalRecovery::feed`] and [`IncrementalRecovery::finish`]:
/// [`StopReason::InvalidInput`] for unrecoverable syntax, or the governor's
/// exhaustion or cancellation reason.
///
/// ```
/// use purrassist_core::{Budget, Governor, json::recover_syntax};
/// let mut governor = Governor::new(Budget::default());
/// let (node, consumed, edits) = recover_syntax("[1, 2,] and more", &mut governor).unwrap();
/// assert_eq!(node.to_json(), "[1,2]");
/// assert_eq!(consumed, 8);
/// assert_eq!(edits[0].rule, "json.syntax.trailing-comma");
/// ```
pub fn recover_syntax(
    input: &str,
    governor: &mut Governor,
) -> Result<(Node, usize, Vec<SyntaxEdit>), StopReason> {
    let mut parser = IncrementalRecovery::new(governor);
    parser.feed(input.as_bytes(), governor)?;
    parser.finish(governor)
}

/// One bracketed value a tracked parse started, in pre-order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Container {
    /// Absolute offset of the opening bracket.
    pub(crate) start: usize,
    /// Edits recorded before the opening bracket.
    pub(crate) edits_before: usize,
    /// Brackets consumed by this parse before the opening bracket.
    pub(crate) brackets_before: u64,
    /// Absolute end of the value and its trailing whitespace/comments, once
    /// both are complete; `None` while open (or when the parse failed first).
    pub(crate) end: Option<usize>,
    /// Edits recorded up to `end`.
    pub(crate) edits_through: usize,
    /// Brackets consumed by this parse up to `end`.
    pub(crate) brackets_through: u64,
}
impl ScratchCharge for Container {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 64);
        64
    };
}

/// A `{` or `[` a tracked parse consumed as text (inside a string, comment,
/// bare token or key) rather than as the start of a value.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Orphan {
    /// Absolute offset of the bracket.
    pub(crate) at: usize,
    /// Brackets consumed by this parse before it.
    pub(crate) brackets_before: u64,
}
impl ScratchCharge for Orphan {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 16);
        16
    };
}

/// The result of [`recover_region`]: a complete or partial parse whose
/// containers can be read as the standalone recoveries their openers start.
pub(crate) struct Region {
    /// The recovered value; for a failed parse, the partial tree holding
    /// every container started so far (incomplete ones closed as they were).
    pub(crate) root: Option<Node>,
    /// Whether the parse recovered a whole value (no unrecoverable syntax).
    pub(crate) completed: bool,
    /// Absolute offset where this parse stopped consuming.
    pub(crate) stop: usize,
    /// Every edit, with absolute spans.
    pub(crate) edits: Vec<SyntaxEdit>,
    /// Started containers in pre-order.
    pub(crate) containers: Vec<Container>,
    /// Opening brackets consumed as text, in order.
    pub(crate) orphans: Vec<Orphan>,
    /// Bracket characters (`[]{}`, anywhere, including inside strings) consumed.
    pub(crate) brackets: u64,
}

/// Recover one value starting at absolute offset `start` of `input`,
/// tracking every container it starts.
///
/// Because recovery is deterministic in the input after a value's first
/// byte, the container opened at offset `p` within this parse is exactly the
/// value [`recover_syntax`] would recover from `&input[p..]`, its consumed
/// end is [`Container::end`], and its edits are
/// `edits[edits_before..edits_through]`.
///
/// Unrecoverable syntax is not an error here: it yields `completed: false`
/// and the partial tree, so containers completed before the failure remain
/// usable. Resource exhaustion and cancellation are returned as `Err`.
pub(crate) fn recover_region(
    input: &str,
    start: usize,
    governor: &mut Governor,
) -> Result<Region, StopReason> {
    let mut parser = IncrementalRecovery::new(governor);
    parser.base = start;
    parser.position = start;
    parser.containers = Some(ScratchVec::new(governor));
    parser.orphans = Some(ScratchVec::new(governor));
    let outcome = parser
        .feed(&input.as_bytes()[start..], governor)
        .and_then(|()| parser.end(governor));
    let completed = match outcome {
        Ok(()) => true,
        Err(StopReason::InvalidInput(_)) => false,
        Err(reason) => return Err(reason),
    };
    let root = if completed {
        parser.root.take()
    } else {
        parser.unwind(governor)?
    };
    Ok(Region {
        root,
        completed,
        stop: parser.position,
        edits: core::mem::take(&mut parser.edits),
        containers: parser
            .containers
            .take()
            .map_or_else(Vec::new, ScratchVec::into_vec),
        orphans: parser
            .orphans
            .take()
            .map_or_else(Vec::new, ScratchVec::into_vec),
        brackets: parser.brackets,
    })
}

/// A push-based, chunk-invariant tolerant JSON recovery parser.
///
/// Feed UTF-8 bytes in any number of chunks with [`feed`](Self::feed), then
/// call [`finish`](Self::finish). Parsing stops after one value and the
/// whitespace and comments that follow it; later bytes are not consumed and
/// cost nothing. See the [module documentation](self) for the rules and the
/// resource contract.
///
/// Use the same governor for construction, every `feed`, and `finish`:
/// the open-container stack charges the constructing governor's scratch
/// meter.
///
/// ```
/// use purrassist_core::{Budget, Governor, json::incremental::IncrementalRecovery};
/// let mut governor = Governor::new(Budget::default());
/// let mut parser = IncrementalRecovery::new(&governor);
/// // Chunks may split a string, an escape, a multi-byte character or `//`.
/// let chunks: [&[u8]; 5] = [
///     b"{'gr",
///     b"\xc3",
///     b"\xa9et': \"\\u00",
///     b"e9\", list: [1 2,]} /",
///     b"/ done\n tail",
/// ];
/// for chunk in chunks {
///     parser.feed(chunk, &mut governor).unwrap();
/// }
/// let (node, consumed, edits) = parser.finish(&mut governor).unwrap();
/// assert_eq!(node.to_json(), r#"{"gréet":"é","list":[1,2]}"#);
/// let source = "{'gréet': \"\\u00e9\", list: [1 2,]} // done\n tail";
/// assert_eq!(&source[consumed..], "tail");
/// let rules: Vec<_> = edits.iter().map(|edit| edit.rule).collect();
/// assert_eq!(
///     rules,
///     [
///         "json.syntax.string",
///         "json.syntax.string",
///         "json.syntax.bare-key",
///         "json.syntax.missing-comma",
///         "json.syntax.trailing-comma",
///         "json.syntax.comment",
///     ]
/// );
/// ```
///
/// Recovery that cannot continue is an explicit error, never a guess:
///
/// ```
/// use purrassist_core::{Budget, Governor, StopReason, json::incremental::IncrementalRecovery};
/// let mut governor = Governor::new(Budget::default());
/// let mut parser = IncrementalRecovery::new(&governor);
/// let error = parser.feed(b"[1}", &mut governor).unwrap_err();
/// assert!(matches!(error, StopReason::InvalidInput(_)));
/// ```
#[derive(Debug)]
pub struct IncrementalRecovery {
    /// Absolute offset of the first byte fed.
    base: usize,
    /// Absolute offset of the next byte to be fed.
    fed: usize,
    /// Absolute offset just past the last consumed character.
    position: usize,
    utf8: Utf8,
    mode: Mode,
    stack: ScratchVec<Frame>,
    root: Option<Node>,
    edits: Vec<SyntaxEdit>,
    /// A `/` at this offset was found not to open a comment.
    plain_slash: Option<usize>,
    started: bool,
    failed: Option<StopReason>,
    brackets: u64,
    containers: Option<ScratchVec<Container>>,
    orphans: Option<ScratchVec<Orphan>>,
    /// The bracket being consumed opens a value (it is not an orphan).
    opening: bool,
    /// The container whose trailing whitespace/comments are being read.
    settling: Option<usize>,
    /// U+FFFD characters fed by [`Self::feed_replacement`] in place of
    /// malformed source bytes: each one's offset and source width.
    replaced: Vec<(usize, usize)>,
    /// Whether a bare (unquoted) key must be followed by its `:` (see
    /// [`Self::require_key_colons`]).
    colon_keys: bool,
    /// Where the key awaiting its `:` begins, when it was a bare word.
    bare_key: Option<usize>,
    /// Where the input became unrecoverable when that is before the
    /// character that showed it (a bare word that is no key: its start).
    fault_at: Option<usize>,
}

/// A partially decoded UTF-8 scalar.
#[derive(Clone, Copy, Debug, Default)]
struct Utf8 {
    bytes: [u8; 4],
    have: u8,
    need: u8,
    start: usize,
}

/// What follows the current whitespace/comment run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum After {
    /// Dispatch a value at this nesting depth (already charged).
    Value(u32),
    /// The top-level value is complete; the next significant character stops.
    Top,
    /// A closer, a comma, the end of input, or a missing comma.
    Separator,
    /// After the comma at this offset: a closer makes it a trailing comma.
    Comma(usize),
    /// Directly after an opening bracket.
    Opened,
    /// An object key.
    Key,
    /// The `:` after a key.
    Colon,
}

/// The lexical mode.
#[derive(Debug)]
enum Mode {
    Space(After),
    /// A `/` at `at`, held until the next character shows whether it opens
    /// a comment.
    Slash {
        at: usize,
        then: After,
    },
    LineComment {
        start: usize,
        then: After,
    },
    BlockComment {
        start: usize,
        star: bool,
        then: After,
    },
    /// A bare value token or bare object key.
    Token {
        start: usize,
        key: bool,
        text: String,
    },
    Text(Text),
    Done,
}

/// A quoted string in progress.
#[derive(Debug)]
struct Text {
    start: usize,
    quote: char,
    key: bool,
    /// Whether the source so far is byte-identical to the canonical
    /// re-encoding of the decoded value.
    canonical: bool,
    value: String,
    escape: Escape,
    /// The current escape's source text (at most twelve scalars).
    slice: Slice,
}

/// Escape progress inside a string; offsets are the backslash's.
#[derive(Clone, Copy, Debug)]
enum Escape {
    None,
    /// After `\`.
    Backslash(usize),
    /// After `\u`, reading four characters.
    Hex(usize),
    /// After a high-surrogate `\uXXXX`: a `\u` continues the pair.
    LookBackslash(usize),
    /// A `\` (held at the second offset) that continues the pair if `u` follows.
    LookU(usize, usize),
    /// Reading this many remaining characters of the pair's second half.
    Tail(usize, u8),
}

/// The source of one escape: `\` plus at most eleven more scalars.
#[derive(Clone, Copy, Debug)]
struct Slice {
    bytes: [u8; 48],
    len: usize,
    chars: usize,
}
impl Slice {
    const fn new() -> Self {
        Self {
            bytes: [0; 48],
            len: 0,
            chars: 0,
        }
    }
    fn push(&mut self, character: char) {
        let width = character.len_utf8();
        character.encode_utf8(&mut self.bytes[self.len..self.len + width]);
        self.len += width;
        self.chars += 1;
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).expect("escape text is encoded scalars")
    }
}

/// An open container.
#[derive(Debug)]
struct Frame {
    start: usize,
    /// Depth charged for each child value.
    depth: u32,
    object: bool,
    items: Vec<Node>,
    members: Vec<Member>,
    /// The parsed key awaiting its value.
    key: Option<(String, Range<usize>)>,
    /// Index into the tracked containers.
    container: usize,
}
impl ScratchCharge for Frame {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 128);
        128
    };
}
impl ScratchCharge for SyntaxEdit {
    const CHARGE: usize = {
        assert!(core::mem::size_of::<Self>() <= 64);
        64
    };
}

/// What the driver does after one character was offered.
enum Flow {
    /// The character was consumed or held; offer the next one.
    Next,
    /// The mode changed without consuming; offer the same character again.
    Again,
    /// First offer the held character at its offset, then this one again.
    Replay(char, usize),
}

const fn is_quote(character: char) -> bool {
    matches!(character, '"' | '\'' | '“')
}

const fn is_delimiter(character: char) -> bool {
    matches!(character, ',' | ']' | '}' | ':')
}

const fn closing_quote(quote: char) -> char {
    if quote == '“' { '”' } else { quote }
}

/// Whether the canonical encoding of `character` inside a JSON string is
/// an escape rather than the character itself.
const fn needs_escape(character: char) -> bool {
    matches!(character, '"' | '\\' | '\0'..='\u{1f}')
}

/// Whether `slice` is exactly the canonical JSON string encoding of
/// `character` (the encoding [`Node::to_json`] would write).
fn is_canonical_escape(slice: &str, character: char) -> bool {
    let short = match character {
        '"' => Some('"'),
        '\\' => Some('\\'),
        '\n' => Some('n'),
        '\r' => Some('r'),
        '\t' => Some('t'),
        '\u{8}' => Some('b'),
        '\u{c}' => Some('f'),
        _ => None,
    };
    let mut expected = Slice::new();
    expected.push('\\');
    if let Some(short) = short {
        expected.push(short);
    } else if needs_escape(character) {
        let code = u32::from(character);
        for digit in *b"u00" {
            expected.push(char::from(digit));
        }
        for nibble in [code >> 4, code & 0xf] {
            expected.push(char::from_digit(nibble, 16).expect("nibble"));
        }
    } else {
        // An escape always starts with `\`; a character needing no escape
        // is canonically written as itself.
        return false;
    }
    slice == expected.as_str()
}

/// Four ASCII hexadecimal digits, as the strict parser reads `\uXXXX`.
pub(crate) fn hex4(digits: &[u8]) -> Option<u32> {
    let digits = digits.get(..4)?;
    digits.iter().try_fold(0, |value, &byte| {
        char::from(byte)
            .to_digit(16)
            .map(|digit| (value << 4) | digit)
    })
}

/// Decode one escape exactly as strict JSON would decode the string literal
/// `"` + `slice` + `"`, or `None` when that literal is not valid JSON.
pub(crate) fn decode_escape(slice: &str) -> Option<char> {
    let bytes = slice.as_bytes();
    let simple = match bytes.get(1)? {
        b'"' => '"',
        b'\\' => '\\',
        b'/' => '/',
        b'b' => '\u{8}',
        b'f' => '\u{c}',
        b'n' => '\n',
        b'r' => '\r',
        b't' => '\t',
        b'u' => {
            let first = hex4(&bytes[2..])?;
            if (0xd800..=0xdbff).contains(&first) {
                if bytes.len() != 12 || &bytes[6..8] != b"\\u" {
                    return None;
                }
                let second = hex4(&bytes[8..])?;
                if !(0xdc00..=0xdfff).contains(&second) {
                    return None;
                }
                return char::from_u32(0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00));
            }
            return if bytes.len() == 6 {
                char::from_u32(first)
            } else {
                None
            };
        }
        _ => return None,
    };
    (bytes.len() == 2).then_some(simple)
}

/// The quoted JSON encoding of `text`, charged (at its worst-case length)
/// before it is allocated.
fn quoted(text: &str, governor: &mut Governor) -> Result<String, StopReason> {
    governor.charge(
        Resource::ScratchBytes,
        size(text.len()).saturating_mul(6).saturating_add(2),
    )?;
    Ok(super::quote(text))
}

fn invalid_utf8(offset: usize) -> StopReason {
    StopReason::InvalidInput(format!("invalid UTF-8 at byte {offset}"))
}

impl IncrementalRecovery {
    /// A parser for one value. Its open-container stack charges
    /// `governor`'s scratch meter as it grows.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json::incremental::IncrementalRecovery};
    /// let mut governor = Governor::new(Budget::default());
    /// let parser = IncrementalRecovery::new(&governor);
    /// let (node, consumed, edits) = parser.finish(&mut governor).unwrap();
    /// assert_eq!(node.to_json(), "null");
    /// assert_eq!(consumed, 0);
    /// assert_eq!(edits[0].rule, "json.syntax.missing-value");
    /// ```
    #[must_use]
    pub fn new(governor: &Governor) -> Self {
        Self {
            base: 0,
            fed: 0,
            position: 0,
            utf8: Utf8::default(),
            mode: Mode::Space(After::Value(0)),
            stack: ScratchVec::new(governor),
            root: None,
            edits: Vec::new(),
            plain_slash: None,
            started: false,
            failed: None,
            brackets: 0,
            containers: None,
            orphans: None,
            opening: false,
            settling: None,
            replaced: Vec::new(),
            colon_keys: false,
            bare_key: None,
            fault_at: None,
        }
    }

    /// A parser whose first byte sits at absolute offset `base`: node and
    /// edit spans are absolute, and the consumed length is still relative.
    pub(crate) fn starting_at(base: usize, governor: &Governor) -> Self {
        let mut parser = Self::new(governor);
        parser.base = base;
        parser.position = base;
        parser
    }

    /// Read a bare (unquoted) object key only when its `:` follows it: a bare
    /// word with no `:` after it is no key (`{"a": "y" oops}`, prose after an
    /// unclosed object), so no parameter name is invented from positional
    /// text and no free text is read as an argument; the input is then
    /// unrecoverable there. A quoted key may still have its `:` inserted,
    /// and `{path: "a"}` keeps its bare key.
    pub(crate) const fn require_key_colons(mut self) -> Self {
        self.colon_keys = true;
        self
    }

    /// Where the input became unrecoverable, when that lies before the
    /// character that showed it: the start of a bare word that is no key
    /// (see [`Self::require_key_colons`]).
    pub(crate) const fn fault_at(&self) -> Option<usize> {
        self.fault_at
    }

    /// Whether the value and its trailing whitespace/comments are complete:
    /// later bytes would not be consumed.
    pub(crate) const fn is_done(&self) -> bool {
        matches!(self.mode, Mode::Done)
    }

    /// Whether the top-level value is complete: [`Self::is_done`], or only
    /// whitespace has followed it, so the end of input ends it with no
    /// recovery edit.
    pub(crate) const fn value_complete(&self) -> bool {
        matches!(self.mode, Mode::Done | Mode::Space(After::Top))
    }

    /// Whether the parser is inside a double-quoted string: a delimiter
    /// written here is text of the string.
    pub(crate) const fn in_string(&self) -> bool {
        matches!(&self.mode, Mode::Text(text) if text.quote == '"')
    }

    /// Absolute offset just past the last consumed character.
    pub(crate) const fn position(&self) -> usize {
        self.position
    }

    /// The number of edits recorded so far.
    pub(crate) const fn edit_count(&self) -> usize {
        self.edits.len()
    }

    /// Consume the next chunk of input.
    ///
    /// Bytes after the recovered value and its trailing whitespace/comments
    /// are ignored. A chunk may end anywhere, including inside a multi-byte
    /// character; end-of-input repairs wait for [`finish`](Self::finish).
    ///
    /// # Errors
    /// Returns [`StopReason::InvalidInput`] for unrecoverable syntax
    /// (a mismatched closer, a `:` where a value belongs, an object member
    /// without a key) or malformed UTF-8, and the governor's exhaustion or
    /// cancellation reason. Every error is sticky: later calls return it again.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, StopReason, contract::Resource, json::incremental::IncrementalRecovery};
    /// let mut governor = Governor::new(Budget { work: 3, ..Budget::default() });
    /// let mut parser = IncrementalRecovery::new(&governor);
    /// assert_eq!(
    ///     parser.feed(b"[1, 2, 3]", &mut governor),
    ///     Err(StopReason::BudgetExceeded(Resource::Work))
    /// );
    /// ```
    pub fn feed(&mut self, bytes: &[u8], governor: &mut Governor) -> Result<(), StopReason> {
        if let Some(reason) = &self.failed {
            return Err(reason.clone());
        }
        let result = self.consume_chunk(bytes, governor);
        if let Err(reason) = &result {
            self.failed = Some(reason.clone());
        }
        result
    }

    /// Consume one U+FFFD standing for the `width` source bytes at the next
    /// offset, which are not well-formed UTF-8: offsets after it stay
    /// exact. The caller records the replacement. Errors as
    /// [`feed`](Self::feed).
    pub(crate) fn feed_replacement(
        &mut self,
        width: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        if let Some(reason) = &self.failed {
            return Err(reason.clone());
        }
        debug_assert_eq!(self.utf8.need, 0, "a replacement falls between characters");
        let result = (|| {
            self.begin(governor)?;
            let at = self.base + self.fed;
            self.fed += width;
            if matches!(self.mode, Mode::Done) {
                return Ok(());
            }
            governor.charge(Resource::ScratchBytes, 16)?;
            self.replaced.push((at, width));
            self.offer(Some(char::REPLACEMENT_CHARACTER), at, governor)
        })();
        if let Err(reason) = &result {
            self.failed = Some(reason.clone());
        }
        result
    }

    fn consume_chunk(&mut self, bytes: &[u8], governor: &mut Governor) -> Result<(), StopReason> {
        self.begin(governor)?;
        for &byte in bytes {
            if matches!(self.mode, Mode::Done) {
                break;
            }
            let at = self.base + self.fed;
            self.fed += 1;
            if let Some((character, start)) = self.decode(byte, at)? {
                self.offer(Some(character), start, governor)?;
            }
        }
        Ok(())
    }

    /// Apply end-of-input repairs and return the recovered value, the number
    /// of bytes consumed (the value plus trailing whitespace and comments),
    /// and every edit in source order.
    ///
    /// # Errors
    /// Returns the sticky error of an earlier [`feed`](Self::feed), malformed
    /// UTF-8 at the end of input, unrecoverable syntax found at the end of
    /// input (such as a missing key after a trailing comma), or the
    /// governor's exhaustion or cancellation reason.
    ///
    /// ```
    /// use purrassist_core::{Budget, Governor, json::incremental::IncrementalRecovery};
    /// let mut governor = Governor::new(Budget::default());
    /// let mut parser = IncrementalRecovery::new(&governor);
    /// parser.feed(b"[1, \"tw", &mut governor).unwrap();
    /// let (node, consumed, edits) = parser.finish(&mut governor).unwrap();
    /// assert_eq!(node.to_json(), r#"[1,"tw"]"#);
    /// assert_eq!(consumed, 7);
    /// assert!(edits.iter().any(|edit| edit.rule == "json.syntax.string" && edit.guessed));
    /// ```
    pub fn finish(
        mut self,
        governor: &mut Governor,
    ) -> Result<(Node, usize, Vec<SyntaxEdit>), StopReason> {
        if let Some(reason) = self.failed.take() {
            return Err(reason);
        }
        self.end(governor)?;
        let node = self
            .root
            .take()
            .ok_or_else(|| StopReason::InvalidInput("recovery ended without a value".into()))?;
        Ok((
            node,
            self.position - self.base,
            core::mem::take(&mut self.edits),
        ))
    }

    /// Offer the end of input, completing the value.
    fn end(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        self.begin(governor)?;
        if self.utf8.need != 0 {
            return Err(invalid_utf8(self.utf8.start - self.base));
        }
        if !matches!(self.mode, Mode::Done) {
            self.offer(None, self.base + self.fed, governor)?;
        }
        debug_assert!(matches!(self.mode, Mode::Done));
        Ok(())
    }

    fn begin(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        if !self.started {
            self.started = true;
            self.enter_value(0, governor)?;
        }
        Ok(())
    }

    /// Accumulate one byte; return a completed scalar and its offset.
    fn decode(&mut self, byte: u8, at: usize) -> Result<Option<(char, usize)>, StopReason> {
        if self.utf8.need == 0 {
            if byte.is_ascii() {
                return Ok(Some((char::from(byte), at)));
            }
            let need = match byte {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => return Err(invalid_utf8(at - self.base)),
            };
            self.utf8 = Utf8 {
                bytes: [byte, 0, 0, 0],
                have: 1,
                need,
                start: at,
            };
            return Ok(None);
        }
        if byte & 0xc0 != 0x80 {
            return Err(invalid_utf8(self.utf8.start - self.base));
        }
        self.utf8.bytes[usize::from(self.utf8.have)] = byte;
        self.utf8.have += 1;
        if self.utf8.have < self.utf8.need {
            return Ok(None);
        }
        self.utf8.need = 0;
        let start = self.utf8.start;
        let text = core::str::from_utf8(&self.utf8.bytes[..usize::from(self.utf8.have)])
            .map_err(|_| invalid_utf8(start - self.base))?;
        Ok(text.chars().next().map(|character| (character, start)))
    }

    fn offer(
        &mut self,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        loop {
            match self.handle(character, at, governor)? {
                Flow::Next => return Ok(()),
                Flow::Again => {}
                Flow::Replay(held, held_at) => self.offer(Some(held), held_at, governor)?,
            }
        }
    }

    fn handle(
        &mut self,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        match core::mem::replace(&mut self.mode, Mode::Done) {
            Mode::Space(then) => self.space(then, character, at, governor),
            Mode::Slash { at: slash, then } => {
                if let Some(second @ ('/' | '*')) = character {
                    self.consume('/', slash, governor)?;
                    self.consume(second, at, governor)?;
                    self.mode = if second == '/' {
                        Mode::LineComment { start: slash, then }
                    } else {
                        Mode::BlockComment {
                            start: slash,
                            star: false,
                            then,
                        }
                    };
                    Ok(Flow::Next)
                } else {
                    self.plain_slash = Some(slash);
                    self.mode = Mode::Space(then);
                    Ok(Flow::Replay('/', slash))
                }
            }
            Mode::LineComment { start, then } => match character {
                Some('\n') | None => self.end_comment(start, then, Flow::Again, governor),
                Some(other) => {
                    self.consume(other, at, governor)?;
                    self.mode = Mode::LineComment { start, then };
                    Ok(Flow::Next)
                }
            },
            Mode::BlockComment { start, star, then } => match character {
                None => self.end_comment(start, then, Flow::Again, governor),
                Some('/') if star => {
                    self.consume('/', at, governor)?;
                    self.end_comment(start, then, Flow::Next, governor)
                }
                Some(other) => {
                    self.consume(other, at, governor)?;
                    self.mode = Mode::BlockComment {
                        start,
                        star: other == '*',
                        then,
                    };
                    Ok(Flow::Next)
                }
            },
            Mode::Token {
                start,
                key,
                mut text,
            } => match character {
                Some(other) if !other.is_whitespace() && !is_delimiter(other) => {
                    self.consume(other, at, governor)?;
                    push_char(&mut text, other, governor)?;
                    self.mode = Mode::Token { start, key, text };
                    Ok(Flow::Next)
                }
                _ => {
                    self.end_token(start, key, text, governor)?;
                    Ok(Flow::Again)
                }
            },
            Mode::Text(text) => self.text(text, character, at, governor),
            Mode::Done => Ok(Flow::Next),
        }
    }

    /// Consume one character: admit it, charge its bytes, and advance.
    fn consume(
        &mut self,
        character: char,
        at: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let width = if character == char::REPLACEMENT_CHARACTER {
            self.replaced
                .iter()
                .find(|&&(start, _)| start == at)
                .map_or_else(|| character.len_utf8(), |&(_, width)| width)
        } else {
            character.len_utf8()
        };
        let end = at + width;
        let admitted = size(end - self.base);
        if admitted > governor.limits().input_bytes {
            governor.charge(Resource::InputBytes, admitted)?;
        }
        governor.charge(Resource::Work, size(width))?;
        if matches!(character, '[' | ']' | '{' | '}') {
            if let (false, Some(orphans), '[' | '{') = (self.opening, &mut self.orphans, character)
            {
                orphans.push(Orphan {
                    at,
                    brackets_before: self.brackets,
                })?;
            }
            self.brackets += 1;
        }
        self.position = end;
        Ok(())
    }

    fn edit(
        &mut self,
        rule: &'static str,
        span: Range<usize>,
        replacement: String,
        guessed: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::EvidenceEntries, 1)?;
        reserve_one(&mut self.edits, governor)?;
        self.edits.push(SyntaxEdit {
            rule,
            span,
            replacement,
            guessed,
        });
        Ok(())
    }

    /// Insert fixed `replacement` text (at most four bytes) at `at`.
    fn insert(
        &mut self,
        rule: &'static str,
        at: usize,
        replacement: &str,
        guessed: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        governor.charge(Resource::ScratchBytes, size(replacement.len()))?;
        self.edit(rule, at..at, replacement.into(), guessed, governor)
    }

    fn end_comment(
        &mut self,
        start: usize,
        then: After,
        flow: Flow,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        self.edit(
            "json.syntax.comment",
            start..self.position,
            String::new(),
            false,
            governor,
        )?;
        self.mode = Mode::Space(then);
        Ok(flow)
    }

    fn enter_value(&mut self, depth: u32, governor: &mut Governor) -> Result<(), StopReason> {
        governor.charge(Resource::Depth, u64::from(depth))?;
        governor.charge(Resource::Work, 1)?;
        self.mode = Mode::Space(After::Value(depth));
        Ok(())
    }

    fn space(
        &mut self,
        then: After,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        match character {
            Some(white) if white.is_whitespace() => {
                self.consume(white, at, governor)?;
                self.mode = Mode::Space(then);
                Ok(Flow::Next)
            }
            Some('/') if self.plain_slash != Some(at) => {
                self.mode = Mode::Slash { at, then };
                Ok(Flow::Next)
            }
            _ => {
                self.settle();
                self.resolve(then, character, at, governor)
            }
        }
    }

    /// The trailing whitespace/comments of the last delivered container are
    /// complete: record where its standalone recovery would have stopped.
    fn settle(&mut self) {
        let (position, edits, brackets) = (self.position, self.edits.len(), self.brackets);
        if let (Some(index), Some(containers)) = (self.settling.take(), &mut self.containers)
            && let Some(container) = containers.as_mut_slice().get_mut(index)
        {
            container.end = Some(position);
            container.edits_through = edits;
            container.brackets_through = brackets;
        }
    }

    fn resolve(
        &mut self,
        then: After,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        match then {
            After::Value(depth) => self.dispatch_value(depth, character, at, governor),
            After::Top => {
                self.mode = Mode::Done;
                Ok(Flow::Next)
            }
            After::Separator => self.separator(character, at, governor),
            After::Comma(comma) => {
                if character == Some(self.closer()) {
                    self.edit(
                        "json.syntax.trailing-comma",
                        comma..comma + 1,
                        String::new(),
                        false,
                        governor,
                    )?;
                    self.close(character, at, governor)
                } else {
                    self.next_element(governor)?;
                    Ok(Flow::Again)
                }
            }
            After::Opened => match character {
                Some(closer) if closer == self.closer() => self.close(character, at, governor),
                None => self.separator(None, at, governor),
                Some(_) => {
                    self.next_element(governor)?;
                    Ok(Flow::Again)
                }
            },
            After::Key => match character {
                Some(quote) if is_quote(quote) => {
                    self.open_text(quote, at, true, governor)?;
                    Ok(Flow::Next)
                }
                _ => {
                    self.mode = Mode::Token {
                        start: at,
                        key: true,
                        text: String::new(),
                    };
                    Ok(Flow::Again)
                }
            },
            After::Colon => {
                let depth = self.stack.as_slice().last().map_or(0, |frame| frame.depth);
                if character == Some(':') {
                    self.consume(':', at, governor)?;
                    self.enter_value(depth, governor)?;
                    Ok(Flow::Next)
                } else if self.colon_keys
                    && let Some(start) = self.bare_key
                {
                    self.fault_at = Some(start);
                    Err(StopReason::InvalidInput(format!(
                        "a bare word with no `:` after it at {} is no object key",
                        at - self.base
                    )))
                } else {
                    self.insert("json.syntax.missing-colon", at, ":", false, governor)?;
                    self.enter_value(depth, governor)?;
                    Ok(Flow::Again)
                }
            }
        }
    }

    fn closer(&self) -> char {
        match self.stack.as_slice().last() {
            Some(frame) if frame.object => '}',
            _ => ']',
        }
    }

    fn dispatch_value(
        &mut self,
        depth: u32,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        match character {
            Some(open @ ('{' | '[')) => {
                let container = match &mut self.containers {
                    Some(containers) => {
                        containers.push(Container {
                            start: at,
                            edits_before: self.edits.len(),
                            brackets_before: self.brackets,
                            end: None,
                            edits_through: 0,
                            brackets_through: 0,
                        })?;
                        containers.len() - 1
                    }
                    None => 0,
                };
                self.opening = true;
                let consumed = self.consume(open, at, governor);
                self.opening = false;
                consumed?;
                self.stack.push(Frame {
                    start: at,
                    depth: depth + 1,
                    object: open == '{',
                    items: Vec::new(),
                    members: Vec::new(),
                    key: None,
                    container,
                })?;
                self.mode = Mode::Space(After::Opened);
                Ok(Flow::Next)
            }
            Some(quote) if is_quote(quote) => {
                self.open_text(quote, at, false, governor)?;
                Ok(Flow::Next)
            }
            None | Some('}' | ']' | ',') => {
                self.insert("json.syntax.missing-value", at, "null", true, governor)?;
                self.deliver(
                    Node {
                        value: Value::Null,
                        span: Some(at..at),
                    },
                    governor,
                )?;
                Ok(Flow::Again)
            }
            Some(_) => {
                self.mode = Mode::Token {
                    start: at,
                    key: false,
                    text: String::new(),
                };
                Ok(Flow::Again)
            }
        }
    }

    fn separator(
        &mut self,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        let closer = self.closer();
        match character {
            Some(found) if found == closer => self.close(character, at, governor),
            None => {
                let mut text = [0; 4];
                let replacement: &str = closer.encode_utf8(&mut text);
                self.insert(
                    "json.syntax.close-container",
                    at,
                    replacement,
                    false,
                    governor,
                )?;
                self.close(None, at, governor)
            }
            Some(',') => {
                self.consume(',', at, governor)?;
                self.mode = Mode::Space(After::Comma(at));
                Ok(Flow::Next)
            }
            Some('}' | ']') => Err(StopReason::InvalidInput(
                "mismatched container closer".into(),
            )),
            Some(_) => {
                self.insert("json.syntax.missing-comma", at, ",", false, governor)?;
                self.next_element(governor)?;
                Ok(Flow::Again)
            }
        }
    }

    fn next_element(&mut self, governor: &mut Governor) -> Result<(), StopReason> {
        match self.stack.as_slice().last() {
            Some(frame) if frame.object => {
                self.mode = Mode::Space(After::Key);
                Ok(())
            }
            Some(frame) => {
                let depth = frame.depth;
                self.enter_value(depth, governor)
            }
            None => unreachable!("elements belong to an open container"),
        }
    }

    /// Close the innermost container, consuming its closer when present.
    fn close(
        &mut self,
        closer: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        if let Some(closer) = closer {
            self.consume(closer, at, governor)?;
        }
        let frame = self.stack.pop().expect("an open container");
        let value = if frame.object {
            Value::Object(frame.members)
        } else {
            Value::Array(frame.items)
        };
        if self.containers.is_some() {
            self.settling = Some(frame.container);
        }
        self.deliver(
            Node {
                value,
                span: Some(frame.start..self.position),
            },
            governor,
        )?;
        Ok(if closer.is_some() {
            Flow::Next
        } else {
            Flow::Again
        })
    }

    /// Hand a completed value to its container (or make it the result).
    fn deliver(&mut self, node: Node, governor: &mut Governor) -> Result<(), StopReason> {
        let Some(frame) = self.stack.as_mut_slice().last_mut() else {
            self.root = Some(node);
            self.mode = Mode::Space(After::Top);
            return Ok(());
        };
        if frame.object {
            let (key, key_span) = frame.key.take().expect("a member key precedes its value");
            reserve_one(&mut frame.members, governor)?;
            frame.members.push(Member {
                key,
                key_span: Some(key_span),
                value: node,
            });
        } else {
            reserve_one(&mut frame.items, governor)?;
            frame.items.push(node);
        }
        self.mode = Mode::Space(After::Separator);
        Ok(())
    }

    fn end_token(
        &mut self,
        start: usize,
        key: bool,
        text: String,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let span = start..self.position;
        if key {
            if text.is_empty() {
                return Err(StopReason::InvalidInput("missing object key".into()));
            }
            let replacement = quoted(&text, governor)?;
            self.edit(
                "json.syntax.bare-key",
                span.clone(),
                replacement,
                false,
                governor,
            )?;
            self.accept_key(text, span);
            self.bare_key = Some(start);
            return Ok(());
        }
        if text.is_empty() {
            return Err(StopReason::InvalidInput(format!(
                "unrecoverable token at {}",
                start - self.base
            )));
        }
        let (value, spelling) = match text.as_str() {
            "null" => (Value::Null, None),
            "true" => (Value::Bool(true), None),
            "false" => (Value::Bool(false), None),
            "None" | "NULL" | "Null" => (Value::Null, Some("null")),
            "True" | "TRUE" => (Value::Bool(true), Some("true")),
            "False" | "FALSE" => (Value::Bool(false), Some("false")),
            _ => {
                if let Ok(number) = text.parse::<Number>() {
                    governor.charge(Resource::ScratchBytes, size(text.len()))?;
                    (Value::Number(number), None)
                } else {
                    let replacement = quoted(&text, governor)?;
                    self.edit(
                        "json.syntax.bare-string",
                        span.clone(),
                        replacement,
                        true,
                        governor,
                    )?;
                    (Value::String(text), None)
                }
            }
        };
        if let Some(spelling) = spelling {
            governor.charge(Resource::ScratchBytes, size(spelling.len()))?;
            self.edit(
                "json.syntax.literal-case",
                span.clone(),
                spelling.into(),
                false,
                governor,
            )?;
        }
        self.deliver(
            Node {
                value,
                span: Some(span),
            },
            governor,
        )
    }

    /// Store a completed key on the innermost object and expect its colon.
    fn accept_key(&mut self, key: String, span: Range<usize>) {
        if let Some(frame) = self.stack.as_mut_slice().last_mut() {
            frame.key = Some((key, span));
        }
        self.bare_key = None;
        self.mode = Mode::Space(After::Colon);
    }

    fn open_text(
        &mut self,
        quote: char,
        at: usize,
        key: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        self.consume(quote, at, governor)?;
        self.mode = Mode::Text(Text {
            start: at,
            quote,
            key,
            canonical: true,
            value: String::new(),
            escape: Escape::None,
            slice: Slice::new(),
        });
        Ok(())
    }

    fn text(
        &mut self,
        mut text: Text,
        character: Option<char>,
        at: usize,
        governor: &mut Governor,
    ) -> Result<Flow, StopReason> {
        let flow = match (text.escape, character) {
            (Escape::None | Escape::Backslash(_), None) => {
                return self.end_text(text, false, governor).map(|()| Flow::Again);
            }
            (Escape::None, Some(found)) if found == closing_quote(text.quote) => {
                self.consume(found, at, governor)?;
                return self.end_text(text, true, governor).map(|()| Flow::Next);
            }
            (Escape::None, Some('\\')) => {
                self.consume('\\', at, governor)?;
                text.escape = Escape::Backslash(at);
                text.slice = Slice::new();
                text.slice.push('\\');
                Flow::Next
            }
            (Escape::None, Some(plain)) => {
                self.consume(plain, at, governor)?;
                text.canonical &= !needs_escape(plain);
                push_char(&mut text.value, plain, governor)?;
                Flow::Next
            }
            (Escape::Backslash(start), Some(escaped)) => {
                self.consume(escaped, at, governor)?;
                if escaped == text.quote && text.quote != '"' {
                    push_char(&mut text.value, escaped, governor)?;
                    text.escape = Escape::None;
                } else {
                    text.slice.push(escaped);
                    if escaped == 'u' {
                        text.escape = Escape::Hex(start);
                    } else {
                        self.end_escape(&mut text, start, governor)?;
                    }
                }
                Flow::Next
            }
            (Escape::LookBackslash(start), Some('\\')) => {
                // Held unconsumed until the next character shows whether a
                // `\u` continues the surrogate pair.
                text.escape = Escape::LookU(start, at);
                Flow::Next
            }
            (Escape::LookBackslash(start), _)
            | (Escape::Hex(start) | Escape::Tail(start, _), None) => {
                self.end_escape(&mut text, start, governor)?;
                Flow::Again
            }
            (Escape::Hex(start), Some(digit)) => {
                self.consume(digit, at, governor)?;
                text.slice.push(digit);
                if text.slice.chars == 6 {
                    let high = hex4(&text.slice.as_str().as_bytes()[2..])
                        .is_some_and(|code| (0xd800..=0xdbff).contains(&code));
                    if high {
                        text.escape = Escape::LookBackslash(start);
                    } else {
                        self.end_escape(&mut text, start, governor)?;
                    }
                }
                Flow::Next
            }
            (Escape::LookU(start, held), Some('u')) => {
                self.consume('\\', held, governor)?;
                self.consume('u', at, governor)?;
                text.slice.push('\\');
                text.slice.push('u');
                text.escape = Escape::Tail(start, 4);
                Flow::Next
            }
            (Escape::LookU(start, held), _) => {
                self.end_escape(&mut text, start, governor)?;
                self.mode = Mode::Text(text);
                return Ok(Flow::Replay('\\', held));
            }
            (Escape::Tail(start, remaining), Some(next)) => {
                self.consume(next, at, governor)?;
                text.slice.push(next);
                if remaining == 1 {
                    self.end_escape(&mut text, start, governor)?;
                } else {
                    text.escape = Escape::Tail(start, remaining - 1);
                }
                Flow::Next
            }
        };
        self.mode = Mode::Text(text);
        Ok(flow)
    }

    /// Decode the completed escape as strict JSON would, or keep it as
    /// literal text with a guessed `json.syntax.literal-escape` edit.
    fn end_escape(
        &mut self,
        text: &mut Text,
        start: usize,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        text.escape = Escape::None;
        let slice = text.slice;
        let source = slice.as_str();
        if let Some(decoded) = decode_escape(source) {
            text.canonical &= is_canonical_escape(source, decoded);
            return push_char(&mut text.value, decoded, governor);
        }
        text.canonical = false;
        for character in source.chars() {
            push_char(&mut text.value, character, governor)?;
        }
        let replacement = quoted(source, governor)?;
        self.edit(
            "json.syntax.literal-escape",
            start..self.position,
            replacement,
            true,
            governor,
        )
    }

    fn end_text(
        &mut self,
        text: Text,
        closed: bool,
        governor: &mut Governor,
    ) -> Result<(), StopReason> {
        let span = text.start..self.position;
        if !closed || text.quote != '"' || !text.canonical {
            let replacement = quoted(&text.value, governor)?;
            self.edit(
                "json.syntax.string",
                span.clone(),
                replacement,
                !closed,
                governor,
            )?;
        }
        if text.key {
            self.accept_key(text.value, span);
            return Ok(());
        }
        self.deliver(
            Node {
                value: Value::String(text.value),
                span: Some(span),
            },
            governor,
        )
    }

    /// Close every open container as it stands, for a failed tracked parse.
    fn unwind(&mut self, governor: &mut Governor) -> Result<Option<Node>, StopReason> {
        let mut child: Option<Node> = None;
        while let Some(mut frame) = self.stack.pop() {
            if let Some(node) = child.take() {
                if frame.object {
                    let (key, key_span) = frame.key.take().unwrap_or_default();
                    reserve_one(&mut frame.members, governor)?;
                    frame.members.push(Member {
                        key,
                        key_span: Some(key_span),
                        value: node,
                    });
                } else {
                    reserve_one(&mut frame.items, governor)?;
                    frame.items.push(node);
                }
            }
            let value = if frame.object {
                Value::Object(frame.members)
            } else {
                Value::Array(frame.items)
            };
            child = Some(Node {
                value,
                span: Some(frame.start..self.position),
            });
        }
        Ok(child.or_else(|| self.root.take()))
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_escape, is_canonical_escape, recover_syntax};
    use crate::contract::{Budget, Governor};

    #[test]
    fn escapes_decode_like_strict_json() {
        for (slice, expected) in [
            ("\\n", Some('\n')),
            ("\\/", Some('/')),
            ("\\x", None),
            ("\\u0041", Some('A')),
            ("\\u004", None),
            ("\\uD83D", None),
            ("\\uDE00", None),
            ("\\uD83D\\uDE00", Some('😀')),
            ("\\uD83D\\u0041", None),
        ] {
            assert_eq!(decode_escape(slice), expected, "{slice}");
        }
        assert!(is_canonical_escape("\\n", '\n'));
        assert!(is_canonical_escape("\\u001f", '\u{1f}'));
        assert!(!is_canonical_escape("\\u001F", '\u{1f}'));
        assert!(!is_canonical_escape("\\u0041", 'A'));
        assert!(!is_canonical_escape("\\/", '/'));
    }

    #[test]
    fn single_chunk_wrapper_matches_documented_rules() {
        let mut governor = Governor::new(Budget::default());
        let (node, consumed, edits) =
            recover_syntax("{a: 'b', c: [1 2,], d: True} trailing", &mut governor).unwrap();
        assert_eq!(node.to_json(), r#"{"a":"b","c":[1,2],"d":true}"#);
        assert_eq!(consumed, "{a: 'b', c: [1 2,], d: True} ".len());
        let rules: Vec<_> = edits.iter().map(|edit| edit.rule).collect();
        assert_eq!(
            rules,
            [
                "json.syntax.bare-key",
                "json.syntax.string",
                "json.syntax.bare-key",
                "json.syntax.missing-comma",
                "json.syntax.trailing-comma",
                "json.syntax.bare-key",
                "json.syntax.literal-case",
            ]
        );
    }
}
