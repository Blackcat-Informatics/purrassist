// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The pure data model of the derives: a small token tree whose spans are
//! indices, the parsed item, and the validated tool model.
//!
//! Nothing here touches `proc_macro`, so parsing, validation, naming and
//! message text are unit-testable (`proc_macro` types panic outside a macro
//! expansion). The boundary in `bridge` converts real tokens to and from this
//! model; a span is the index of the token it was made from.

/// The identity of one source token: the index of that token in the order the
/// boundary visited the input (a group is numbered before its contents).
pub(crate) type Span = usize;

/// An error to be reported as `compile_error!` at the span of the token that
/// caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    /// The token to point at.
    pub(crate) span: Span,
    /// The message, complete and self-contained.
    pub(crate) message: String,
}

impl Diagnostic {
    pub(crate) fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

/// The delimiter of a token group. Invisible (`None`) groups are flattened
/// away by the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delim {
    Paren,
    Brace,
    Bracket,
}

/// What a token is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An identifier or keyword; raw identifiers keep their `r#` prefix.
    Ident(String),
    /// One punctuation character; `joint` when the next token continues the
    /// same operator (`->`, `::`).
    Punct { ch: char, joint: bool },
    /// A literal exactly as written.
    Literal(String),
    /// A delimited group.
    Group { delim: Delim, inner: Vec<Tree> },
}

/// One token tree with the span it carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tree {
    pub(crate) span: Span,
    pub(crate) kind: Kind,
}

impl Tree {
    pub(crate) fn is_ident(&self, text: &str) -> bool {
        matches!(&self.kind, Kind::Ident(ident) if ident == text)
    }

    pub(crate) const fn is_punct(&self, expected: char) -> bool {
        matches!(&self.kind, Kind::Punct { ch, .. } if *ch == expected)
    }

    pub(crate) fn ident(&self) -> Option<&str> {
        match &self.kind {
            Kind::Ident(ident) => Some(ident),
            _ => None,
        }
    }

    pub(crate) fn literal(&self) -> Option<&str> {
        match &self.kind {
            Kind::Literal(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn group(&self, wanted: Delim) -> Option<&[Self]> {
        match &self.kind {
            Kind::Group { delim, inner } if *delim == wanted => Some(inner),
            _ => None,
        }
    }
}

/// An identifier with its span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ident {
    /// As written, including a raw identifier's `r#`.
    pub(crate) text: String,
    pub(crate) span: Span,
}

impl Ident {
    /// The identifier without a raw `r#` prefix.
    pub(crate) fn bare(&self) -> &str {
        self.text.strip_prefix("r#").unwrap_or(&self.text)
    }
}

/// A string known both as the Rust literal that spells it and as its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Text {
    /// A Rust string literal, ready to splice into generated code.
    pub(crate) source: String,
    /// The decoded value.
    pub(crate) value: String,
    pub(crate) span: Span,
}

impl Text {
    /// A text the macro computed itself; its source is an escaped literal.
    pub(crate) fn synthesized(value: String, span: Span) -> Self {
        Self {
            source: format!("{value:?}"),
            value,
            span,
        }
    }
}

/// `effects = ..`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effects {
    Mutating,
    ReadOnly,
}

/// `policy = ..`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Policy {
    Structural,
    Tolerant,
}

/// `unknown = ..`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unknown {
    Error,
    Drop,
    Pass,
}

/// A parsed `default = ..` value, lowered later to JSON value constructors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DefaultValue {
    Null,
    Bool(bool),
    /// A spelling already checked against the JSON number grammar.
    Number(String),
    /// A Rust string literal, spliced as written.
    Str(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

/// The `#[tool(..)]` keys of one item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ToolAttrs {
    pub(crate) name: Option<Text>,
    pub(crate) aliases: Vec<Text>,
    pub(crate) description: Option<Text>,
    pub(crate) effects: Option<Effects>,
    pub(crate) policy: Option<Policy>,
    pub(crate) unknown: Option<(Unknown, Span)>,
}

/// The `#[param(..)]` keys of one field or variant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParamAttrs {
    pub(crate) name: Option<Text>,
    pub(crate) aliases: Vec<Text>,
    pub(crate) description: Option<Text>,
    pub(crate) repeat: Option<Span>,
    pub(crate) default: Option<DefaultValue>,
    pub(crate) pattern: Option<Text>,
    pub(crate) extra: Option<Span>,
}

/// A named field as parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawField {
    pub(crate) ident: Ident,
    pub(crate) ty: Vec<Tree>,
    pub(crate) attrs: ParamAttrs,
    pub(crate) doc: Option<String>,
}

/// A unit variant as parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawVariant {
    pub(crate) ident: Ident,
    pub(crate) name: Option<Text>,
    /// An explicit `#[param(description = "..")]`.
    pub(crate) description: Option<Text>,
    /// The doc comment, used when there is no explicit description.
    pub(crate) doc: Option<String>,
}

/// The body of the derived item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Body {
    Struct(Vec<RawField>),
    Enum(Vec<RawVariant>),
}

/// A parsed derive input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawItem {
    pub(crate) ident: Ident,
    pub(crate) tool: ToolAttrs,
    pub(crate) doc: Option<String>,
    pub(crate) body: Body,
}

/// A validated parameter of a tool struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Param {
    pub(crate) ident: Ident,
    /// The canonical parameter name.
    pub(crate) key: Text,
    pub(crate) ty: Vec<Tree>,
    pub(crate) aliases: Vec<Text>,
    pub(crate) description: Option<Text>,
    pub(crate) repeat: bool,
    pub(crate) default: Option<DefaultValue>,
    pub(crate) pattern: Option<Text>,
}

/// The field that collects undeclared members under `unknown = pass`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Extra {
    pub(crate) ident: Ident,
    pub(crate) ty: Vec<Tree>,
}

/// A validated tool struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tool {
    pub(crate) ident: Ident,
    pub(crate) name: Text,
    pub(crate) aliases: Vec<Text>,
    pub(crate) description: Option<Text>,
    pub(crate) effects: Option<Effects>,
    pub(crate) policy: Option<Policy>,
    pub(crate) unknown: Option<Unknown>,
    pub(crate) params: Vec<Param>,
    pub(crate) extra: Option<Extra>,
}

/// A validated enumeration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Enumeration {
    pub(crate) ident: Ident,
    pub(crate) description: Option<Text>,
    /// The ident of each variant and the string value it stands for.
    pub(crate) variants: Vec<(Ident, Text)>,
}

/// A validated derive input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Tool(Tool),
    Enumeration(Enumeration),
}

/// Whether a name made from an identifier is a valid tool name.
///
/// Core accepts non-empty names of ASCII letters, digits and `_ . : -`. An
/// identifier holds only letters, digits and `_`, so for a name derived from
/// one that rule reduces to being ASCII.
pub(crate) const fn is_valid_derived_name(name: &str) -> bool {
    !name.is_empty() && name.is_ascii()
}

/// The `snake_case` spelling of a type identifier: a lower-to-upper boundary,
/// the end of an acronym, and a digit followed by an uppercase letter each
/// start a new word; ASCII is lowercased and an existing `_` is kept.
///
/// `ReadFile` is `read_file`, `HTTPRequest` is `http_request`, `ReadFile2` is
/// `read_file2` and `Read2File` is `read2_file`.
pub(crate) fn snake_case(ident: &str) -> String {
    let chars: Vec<char> = ident.chars().collect();
    let mut out = String::with_capacity(ident.len() + 4);
    for (index, &current) in chars.iter().enumerate() {
        if current.is_ascii_uppercase() && index > 0 {
            let before = chars[index - 1];
            let after = chars.get(index + 1).copied();
            let starts_word = before.is_ascii_lowercase()
                || before.is_ascii_digit()
                || (before.is_ascii_uppercase() && after.is_some_and(|c| c.is_ascii_lowercase()));
            if starts_word {
                out.push('_');
            }
        }
        out.push(current.to_ascii_lowercase());
    }
    out
}

/// Decode a plain or raw string literal (no suffix) to its value, or `None`
/// when `literal` is anything else (a byte string, a character, a number, or
/// a literal with a suffix).
pub(crate) fn decode_string(literal: &str) -> Option<String> {
    if let Some(rest) = literal.strip_prefix('r') {
        let hashes = rest.bytes().take_while(|&byte| byte == b'#').count();
        let body = rest[hashes..].strip_prefix('"')?;
        let closing = format!("\"{}", "#".repeat(hashes));
        let body = body.strip_suffix(closing.as_str())?;
        return Some(body.to_owned());
    }
    let body = literal.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(character) = chars.next() {
        if character == '"' {
            return None;
        }
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next()? {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '\\' => out.push('\\'),
            '0' => out.push('\0'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            'x' => {
                let high = chars.next()?.to_digit(16)?;
                let low = chars.next()?.to_digit(16)?;
                let value = high * 16 + low;
                if value > 0x7f {
                    return None;
                }
                out.push(char::from_u32(value)?);
            }
            'u' => {
                if chars.next()? != '{' {
                    return None;
                }
                let mut value = 0_u32;
                let mut digits = 0;
                loop {
                    match chars.next()? {
                        '}' => break,
                        '_' => {}
                        digit => {
                            value = value.checked_mul(16)?.checked_add(digit.to_digit(16)?)?;
                            digits += 1;
                        }
                    }
                }
                if digits == 0 {
                    return None;
                }
                out.push(char::from_u32(value)?);
            }
            '\n' => {
                // A line continuation skips the whitespace that follows it.
                let rest = chars.as_str().trim_start_matches([' ', '\t', '\n', '\r']);
                chars = rest.chars();
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Whether `text` is spelled as an RFC 8259 number.
pub(crate) fn is_json_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(at) {
        Some(b'0') => at += 1,
        Some(b'1'..=b'9') => at += digits(at),
        _ => return false,
    }
    if bytes.get(at) == Some(&b'.') {
        let fraction = digits(at + 1);
        if fraction == 0 {
            return false;
        }
        at += 1 + fraction;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = digits(at);
        if exponent == 0 {
            return false;
        }
        at += exponent;
    }
    at == bytes.len()
}

/// The doc-comment lines of an item as one description: every line trimmed,
/// joined with `\n`, surrounding blank lines removed; `None` when nothing is
/// left.
pub(crate) fn doc_description(lines: &[String]) -> Option<String> {
    let joined = lines
        .iter()
        .flat_map(|line| line.split('\n'))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = joined.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The span of the first token of `ty` that names `own` (or `Self`) as a path
/// head or argument: a direct self-reference. A segment after `::` is another
/// item's path, so it is left to the recursion check of the generated code.
pub(crate) fn find_self_reference<'a>(own: &str, ty: &'a [Tree]) -> Option<&'a Tree> {
    let mut previous_colon = false;
    for tree in ty {
        match &tree.kind {
            Kind::Ident(ident) if !previous_colon && (ident == own || ident == "Self") => {
                return Some(tree);
            }
            Kind::Group { inner, .. } => {
                if let Some(found) = find_self_reference(own, inner) {
                    return Some(found);
                }
            }
            _ => {}
        }
        previous_colon = tree.is_punct(':');
    }
    None
}

/// Validate a parsed item into the model the writer consumes.
///
/// # Errors
/// The first rule the item breaks, spanned at the offending token.
pub(crate) fn resolve(item: RawItem) -> Result<Item, Diagnostic> {
    match item.body {
        Body::Struct(fields) => resolve_tool(item.ident, item.tool, item.doc, fields),
        Body::Enum(variants) => resolve_enumeration(item.ident, item.tool, item.doc, variants),
    }
}

fn describe(explicit: Option<Text>, doc: Option<String>, span: Span) -> Option<Text> {
    explicit.or_else(|| doc.map(|doc| Text::synthesized(doc, span)))
}

/// The message for a derived name that is not valid.
pub(crate) fn invalid_derived_name_message(
    what: &str,
    derived: &str,
    source: &str,
    attribute: &str,
) -> String {
    format!(
        "the {what} name `{derived}` derived from {source} is not a valid {what} name \
         (ASCII letters, digits and `_ . : -`); add {attribute}"
    )
}

/// The tool name: the explicit one, or the type name in `snake_case`.
fn tool_name(explicit: Option<Text>, ident: &Ident) -> Result<Text, Diagnostic> {
    if let Some(name) = explicit {
        return Ok(name);
    }
    let derived = snake_case(ident.bare());
    if is_valid_derived_name(&derived) {
        Ok(Text::synthesized(derived, ident.span))
    } else {
        Err(Diagnostic::new(
            ident.span,
            invalid_derived_name_message(
                "tool",
                &derived,
                &format!("`{}`", ident.bare()),
                "#[tool(name = \"..\")]",
            ),
        ))
    }
}

/// The checks on a field marked `extra`, which is not a parameter.
fn check_extra(field: &RawField, span: Span, seen: bool) -> Result<(), Diagnostic> {
    let attrs = &field.attrs;
    if attrs.name.is_some()
        || !attrs.aliases.is_empty()
        || attrs.description.is_some()
        || attrs.repeat.is_some()
        || attrs.default.is_some()
        || attrs.pattern.is_some()
    {
        return Err(Diagnostic::new(
            span,
            "`extra` collects undeclared members and cannot be combined with other \
             #[param] keys",
        ));
    }
    if seen {
        return Err(Diagnostic::new(
            span,
            "only one field can be marked #[param(extra)]",
        ));
    }
    Ok(())
}

/// One declared parameter, checked against its siblings.
fn resolve_param(owner: &Ident, field: RawField, siblings: &[Param]) -> Result<Param, Diagnostic> {
    if let Some(found) = find_self_reference(owner.bare(), &field.ty) {
        return Err(Diagnostic::new(
            found.span,
            format!(
                "field `{}` refers to `{}` itself; a tool's arguments cannot contain \
                 themselves, so a recursive schema must be declared by hand",
                field.ident.bare(),
                owner.bare()
            ),
        ));
    }
    let key = match field.attrs.name {
        Some(name) => name,
        None if is_valid_derived_name(field.ident.bare()) => {
            Text::synthesized(field.ident.bare().to_owned(), field.ident.span)
        }
        None => {
            return Err(Diagnostic::new(
                field.ident.span,
                invalid_derived_name_message(
                    "parameter",
                    field.ident.bare(),
                    &format!("field `{}`", field.ident.bare()),
                    "#[param(name = \"..\")]",
                ),
            ));
        }
    };
    if let Some(first) = siblings.iter().find(|param| param.key.value == key.value) {
        return Err(Diagnostic::new(
            key.span,
            format!(
                "parameter name `{}` is declared twice (first by field `{}`)",
                key.value,
                first.ident.bare()
            ),
        ));
    }
    let description = describe(field.attrs.description, field.doc, field.ident.span);
    Ok(Param {
        ident: field.ident,
        key,
        ty: field.ty,
        aliases: field.attrs.aliases,
        description,
        repeat: field.attrs.repeat.is_some(),
        default: field.attrs.default,
        pattern: field.attrs.pattern,
    })
}

/// `unknown = pass` and the `extra` field must come together.
fn check_pass(extra: Option<&Extra>, unknown: Option<(Unknown, Span)>) -> Result<(), Diagnostic> {
    match (extra, unknown) {
        (Some(extra), unknown) if !matches!(unknown, Some((Unknown::Pass, _))) => {
            Err(Diagnostic::new(
                extra.ident.span,
                "a #[param(extra)] field needs #[tool(unknown = pass)] on the struct",
            ))
        }
        (None, Some((Unknown::Pass, span))) => Err(Diagnostic::new(
            span,
            "`unknown = pass` needs exactly one field marked #[param(extra)] of type \
             `json::Node` to receive the undeclared members",
        )),
        _ => Ok(()),
    }
}

fn resolve_tool(
    ident: Ident,
    tool: ToolAttrs,
    doc: Option<String>,
    fields: Vec<RawField>,
) -> Result<Item, Diagnostic> {
    let name = tool_name(tool.name, &ident)?;
    let description = describe(tool.description, doc, ident.span);
    let mut params: Vec<Param> = Vec::new();
    let mut extra: Option<Extra> = None;
    for field in fields {
        if let Some(extra_span) = field.attrs.extra {
            check_extra(&field, extra_span, extra.is_some())?;
            extra = Some(Extra {
                ident: field.ident,
                ty: field.ty,
            });
        } else {
            let param = resolve_param(&ident, field, &params)?;
            params.push(param);
        }
    }
    check_pass(extra.as_ref(), tool.unknown)?;
    Ok(Item::Tool(Tool {
        ident,
        name,
        aliases: tool.aliases,
        description,
        effects: tool.effects,
        policy: tool.policy,
        unknown: tool.unknown.map(|(unknown, _)| unknown),
        params,
        extra,
    }))
}

fn resolve_enumeration(
    ident: Ident,
    tool: ToolAttrs,
    doc: Option<String>,
    variants: Vec<RawVariant>,
) -> Result<Item, Diagnostic> {
    if variants.is_empty() {
        return Err(Diagnostic::new(
            ident.span,
            "a ToolEnum needs at least one variant: an enumeration without values accepts nothing",
        ));
    }
    let mut resolved: Vec<(Ident, Text)> = Vec::with_capacity(variants.len());
    let mut lines: Vec<String> = Vec::new();
    for variant in variants {
        let value = variant.name.unwrap_or_else(|| {
            Text::synthesized(variant.ident.bare().to_owned(), variant.ident.span)
        });
        if resolved.iter().any(|(_, seen)| seen.value == value.value) {
            return Err(Diagnostic::new(
                value.span,
                format!("enumeration value `{}` is declared twice", value.value),
            ));
        }
        let text = variant.description.map(|text| text.value).or(variant.doc);
        if let Some(text) = text {
            lines.push(format!(
                "- {}: {}",
                value.value,
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            ));
        }
        resolved.push((variant.ident, value));
    }
    let own = describe(tool.description, doc, ident.span).map(|text| text.value);
    let composed = match (own, lines.is_empty()) {
        (own, true) => own,
        (None, false) => Some(lines.join("\n")),
        (Some(own), false) => Some(format!("{own}\n{}", lines.join("\n"))),
    };
    Ok(Item::Enumeration(Enumeration {
        description: composed.map(|text| Text::synthesized(text, ident.span)),
        ident,
        variants: resolved,
    }))
}

/// A tiny lexer for tests: builds the same trees the boundary would, with
/// spans numbered in visiting order (a group before its contents).
#[cfg(test)]
pub(crate) mod lex {
    use super::{Delim, Kind, Tree};

    /// Tokens of `source`, which must be well-formed.
    pub(crate) fn trees(source: &str) -> Vec<Tree> {
        let chars: Vec<char> = source.chars().collect();
        let mut at = 0;
        let mut next_span = 0;
        let trees = sequence(&chars, &mut at, &mut next_span, None);
        assert_eq!(at, chars.len(), "unbalanced test source {source:?}");
        trees
    }

    fn closer(delim: Delim) -> char {
        match delim {
            Delim::Paren => ')',
            Delim::Brace => '}',
            Delim::Bracket => ']',
        }
    }

    fn sequence(
        chars: &[char],
        at: &mut usize,
        next_span: &mut usize,
        until: Option<Delim>,
    ) -> Vec<Tree> {
        let mut trees = Vec::new();
        while *at < chars.len() {
            let current = chars[*at];
            if current.is_whitespace() {
                *at += 1;
                continue;
            }
            if Some(current) == until.map(closer) {
                return trees;
            }
            let span = *next_span;
            *next_span += 1;
            let delim = match current {
                '(' => Some(Delim::Paren),
                '{' => Some(Delim::Brace),
                '[' => Some(Delim::Bracket),
                _ => None,
            };
            if let Some(delim) = delim {
                *at += 1;
                let inner = sequence(chars, at, next_span, Some(delim));
                assert_eq!(chars[*at], closer(delim));
                *at += 1;
                trees.push(Tree {
                    span,
                    kind: Kind::Group { delim, inner },
                });
                continue;
            }
            let start = *at;
            let kind = if current == '"' || (current == 'r' && is_raw_string(chars, *at)) {
                skip_string(chars, at);
                Kind::Literal(chars[start..*at].iter().collect())
            } else if current.is_ascii_digit() {
                while *at < chars.len()
                    && (chars[*at].is_ascii_alphanumeric()
                        || chars[*at] == '_'
                        || chars[*at] == '.'
                        || (matches!(chars[*at], '+' | '-') && matches!(chars[*at - 1], 'e' | 'E')))
                {
                    *at += 1;
                }
                Kind::Literal(chars[start..*at].iter().collect())
            } else if current.is_alphabetic() || current == '_' {
                if current == 'r'
                    && chars.get(*at + 1) == Some(&'#')
                    && chars
                        .get(*at + 2)
                        .is_some_and(|c| c.is_alphabetic() || *c == '_')
                {
                    *at += 2;
                }
                while *at < chars.len() && (chars[*at].is_alphanumeric() || chars[*at] == '_') {
                    *at += 1;
                }
                Kind::Ident(chars[start..*at].iter().collect())
            } else {
                *at += 1;
                let joint = chars
                    .get(*at)
                    .is_some_and(|next| "!#$%&*+-./:;<=>?@^|~,'".contains(*next));
                Kind::Punct { ch: current, joint }
            };
            trees.push(Tree { span, kind });
        }
        assert!(until.is_none(), "unclosed group in test source");
        trees
    }

    fn is_raw_string(chars: &[char], at: usize) -> bool {
        let mut cursor = at + 1;
        while chars.get(cursor) == Some(&'#') {
            cursor += 1;
        }
        chars.get(cursor) == Some(&'"')
    }

    fn skip_string(chars: &[char], at: &mut usize) {
        if chars[*at] == 'r' {
            *at += 1;
            let mut hashes = 0;
            while chars[*at] == '#' {
                hashes += 1;
                *at += 1;
            }
            *at += 1;
            loop {
                if chars[*at] == '"' && chars[*at + 1..].iter().take(hashes).all(|c| *c == '#') {
                    *at += 1 + hashes;
                    return;
                }
                *at += 1;
            }
        }
        *at += 1;
        while chars[*at] != '"' {
            *at += if chars[*at] == '\\' { 2 } else { 1 };
        }
        *at += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_follows_the_naming_decision() {
        for (input, expected) in [
            ("ReadFile", "read_file"),
            ("HTTPRequest", "http_request"),
            ("ReadFile2", "read_file2"),
            ("Read2File", "read2_file"),
            ("Search", "search"),
            ("ABC", "abc"),
            ("IOError", "io_error"),
            ("A1B", "a1_b"),
            ("already_snake", "already_snake"),
            ("Mixed_Case", "mixed_case"),
            ("X", "x"),
        ] {
            assert_eq!(snake_case(input), expected, "{input}");
        }
        assert_eq!(snake_case("Größe"), "größe", "non-ASCII is kept as written");
    }

    #[test]
    fn derived_names_must_be_ascii() {
        assert!(is_valid_derived_name("read_file2"));
        assert!(!is_valid_derived_name(""));
        assert!(!is_valid_derived_name("größe"));
        assert!(!is_valid_derived_name("日本"));
    }

    #[test]
    fn derived_names_that_are_not_valid_are_spanned_errors() {
        use crate::parse::{Target, parse};
        let error = |source: &str| {
            let raw = parse(Target::Definition, &lex::trees(source)).unwrap();
            resolve(raw).unwrap_err()
        };
        // `struct A { größe : u8 }`: the field is token 3.
        let found = error("struct A { größe: u8 }");
        assert_eq!(found.span, 3);
        assert_eq!(
            found.message,
            "the parameter name `größe` derived from field `größe` is not a valid parameter \
             name (ASCII letters, digits and `_ . : -`); add #[param(name = \"..\")]"
        );
        // An explicit name is left to the compile-time assertion.
        let raw = parse(
            Target::Definition,
            &lex::trees(r#"struct A { #[param(name = "g")] größe: u8 }"#),
        )
        .unwrap();
        assert!(resolve(raw).is_ok());
    }

    #[test]
    fn string_literals_decode() {
        assert_eq!(
            decode_string(r#""a\nb\t\"\\\u{e9}\x41\0""#).as_deref(),
            Some("a\nb\t\"\\\u{e9}A\0")
        );
        assert_eq!(decode_string("\"a\\\n   b\"").as_deref(), Some("ab"));
        assert_eq!(
            decode_string(r##"r#"raw "quoted" \n"#"##).as_deref(),
            Some(r#"raw "quoted" \n"#)
        );
        assert_eq!(decode_string(r#"r"x""#).as_deref(), Some("x"));
        for rejected in [
            r#"b"bytes""#,
            "'c'",
            "12",
            r#""suffix"x"#,
            r#""\q""#,
            r#""\x80""#,
            r#""\u{}""#,
            r#""\u{110000}""#,
            r#""unterminated"#,
            r#"r#"x""#,
            r#""a"b""#,
        ] {
            assert_eq!(decode_string(rejected), None, "{rejected}");
        }
    }

    #[test]
    fn json_numbers_are_exactly_rfc_8259() {
        for good in [
            "0",
            "-0",
            "7",
            "10",
            "-1.5",
            "1e5",
            "1E+5",
            "2.5e-3",
            "0.0",
            "123456789012345678901234567890",
        ] {
            assert!(is_json_number(good), "{good}");
        }
        for bad in [
            "", "-", "01", "1.", ".5", "1_000", "0x10", "1u8", "1f64", "1e", "1e+", "+1", "1.5.2",
            "--1", "1 ",
        ] {
            assert!(!is_json_number(bad), "{bad}");
        }
    }

    #[test]
    fn doc_comments_are_trimmed_and_joined() {
        let lines = [
            " Read a file.".to_owned(),
            String::new(),
            "  More. ".to_owned(),
            String::new(),
        ];
        assert_eq!(
            doc_description(&lines).as_deref(),
            Some("Read a file.\n\nMore.")
        );
        assert_eq!(
            doc_description(&[" a\n b ".to_owned()]).as_deref(),
            Some("a\nb")
        );
        assert_eq!(doc_description(&["  ".to_owned()]), None);
        assert_eq!(doc_description(&[]), None);
    }

    #[test]
    fn self_references_are_found_in_nested_positions_only() {
        let ty = lex::trees("::std::vec::Vec<Option<Node>>");
        assert!(find_self_reference("Node", &ty).is_some());
        assert!(find_self_reference("Other", &ty).is_none());
        let qualified = lex::trees("other::Node");
        assert!(
            find_self_reference("Node", &qualified).is_none(),
            "a path segment after `::`"
        );
        let own = lex::trees("Self");
        assert_eq!(find_self_reference("Node", &own).map(|t| t.span), Some(0));
    }
}
