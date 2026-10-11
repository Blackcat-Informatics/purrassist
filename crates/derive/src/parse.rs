// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The first-party parser: derive input tokens to a [`RawItem`].
//!
//! It reads attributes, visibility, the item keyword, the name, a rejection of
//! generics, and either named fields (type tokens are kept verbatim up to the
//! next top-level comma, tracking `<`/`>` depth by hand) or unit variants.
//! Attributes other than `#[tool(..)]`, `#[param(..)]` and `#[doc = ".."]`
//! belong to other macros and are ignored.

use crate::model::{
    Body, CratePath, DefaultValue, Delim, Diagnostic, Effects, Ident, Kind, ParamAttrs, Policy,
    RawField, RawItem, RawVariant, Span, Text, ToolAttrs, Tree, Unknown, decode_string,
    doc_description, is_json_number,
};

/// Which derive is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// `ToolDefinition`: structs with named fields.
    Definition,
    /// `ToolEnum`: enums with unit variants.
    Enumeration,
}

/// Where an attribute sits, for the keys it accepts and the messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Site {
    Struct,
    Enum,
    Field,
    Variant,
}

impl Site {
    const fn describe(self) -> &'static str {
        match self {
            Self::Struct => "a struct",
            Self::Enum => "an enum",
            Self::Field => "a field",
            Self::Variant => "an enum variant",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AttrKind {
    Tool,
    Param,
}

impl AttrKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Param => "param",
        }
    }
}

/// A `#[tool(..)]` or `#[param(..)]` attribute: its name token and arguments.
struct RawAttr<'a> {
    kind: AttrKind,
    span: Span,
    args: &'a [Tree],
}

/// The attributes in front of one item, field or variant.
#[derive(Default)]
struct Attrs<'a> {
    helpers: Vec<RawAttr<'a>>,
    docs: Vec<String>,
}

/// Parse the item a derive was applied to.
///
/// # Errors
/// A [`Diagnostic`] for generics, tuple structs, unions, a body that does not
/// match `target`, malformed or misplaced `#[tool]`/`#[param]` keys,
/// non-unit variants, and malformed fields.
pub(crate) fn parse(target: Target, tokens: &[Tree]) -> Result<RawItem, Diagnostic> {
    let mut at = 0;
    let attrs = attributes(tokens, &mut at)?;
    if tokens.get(at).is_some_and(|tree| tree.is_ident("pub")) {
        at += 1;
        if tokens
            .get(at)
            .is_some_and(|tree| tree.group(Delim::Paren).is_some())
        {
            at += 1;
        }
    }
    let Some(keyword) = tokens.get(at) else {
        return Err(Diagnostic::new(
            tokens.first().map_or(0, |tree| tree.span),
            "expected a struct or an enum",
        ));
    };
    at += 1;
    check_keyword(target, keyword)?;
    let name = match tokens.get(at) {
        Some(Tree {
            kind: Kind::Ident(text),
            span,
        }) => Ident {
            text: text.clone(),
            span: *span,
        },
        other => {
            return Err(Diagnostic::new(
                other.map_or(keyword.span, |tree| tree.span),
                "expected the name of the type",
            ));
        }
    };
    at += 1;
    reject_generics(tokens.get(at))?;
    let site = if target == Target::Definition {
        Site::Struct
    } else {
        Site::Enum
    };
    let tool = tool_attrs(&attrs, site)?;
    reject_helpers(&attrs, AttrKind::Param, site)?;
    let body = body(target, tokens.get(at), name.span)?;
    Ok(RawItem {
        ident: name,
        tool,
        doc: doc_description(&attrs.docs),
        body,
    })
}

/// The item keyword must be the one `target` derives for.
fn check_keyword(target: Target, keyword: &Tree) -> Result<(), Diagnostic> {
    let message = match (keyword.ident().unwrap_or_default(), target) {
        ("struct", Target::Definition) | ("enum", Target::Enumeration) => return Ok(()),
        ("struct", Target::Enumeration) => {
            "`#[derive(ToolEnum)]` applies to enums with unit variants; use \
             `#[derive(ToolDefinition)]` for a struct"
        }
        ("enum", Target::Definition) => {
            "`#[derive(ToolDefinition)]` applies to structs with named fields; use \
             `#[derive(ToolEnum)]` for an enum"
        }
        ("union", _) => {
            "unions cannot describe tool arguments; use a struct with named fields \
             (`ToolDefinition`) or an enum with unit variants (`ToolEnum`)"
        }
        _ => "expected a struct (`ToolDefinition`) or an enum (`ToolEnum`)",
    };
    Err(Diagnostic::new(keyword.span, message))
}

/// Generic parameters and `where` clauses follow the type name.
fn reject_generics(next: Option<&Tree>) -> Result<(), Diagnostic> {
    match next {
        Some(token) if token.is_punct('<') => Err(Diagnostic::new(
            token.span,
            "generic parameters are not supported: a generic type has no single tool \
             schema; use a concrete type",
        )),
        Some(token) if token.is_ident("where") => Err(Diagnostic::new(
            token.span,
            "`where` clauses are not supported: generic types cannot describe tool \
             arguments; use a concrete type",
        )),
        _ => Ok(()),
    }
}

/// The braced body (or the `;` of a unit struct) after the name.
fn body(target: Target, next: Option<&Tree>, name_span: Span) -> Result<Body, Diagnostic> {
    match (target, next) {
        (Target::Definition, Some(group)) if group.group(Delim::Brace).is_some() => Ok(
            Body::Struct(fields(group.group(Delim::Brace).unwrap_or_default())?),
        ),
        (Target::Definition, Some(group)) if group.group(Delim::Paren).is_some() => {
            Err(Diagnostic::new(
                group.span,
                "tuple structs are not supported: parameters need names; use a struct with \
                 named fields",
            ))
        }
        (Target::Definition, Some(semicolon)) if semicolon.is_punct(';') => {
            Ok(Body::Struct(Vec::new()))
        }
        (Target::Enumeration, Some(group)) if group.group(Delim::Brace).is_some() => Ok(
            Body::Enum(variants(group.group(Delim::Brace).unwrap_or_default())?),
        ),
        (_, other) => Err(Diagnostic::new(
            other.map_or(name_span, |tree| tree.span),
            if target == Target::Definition {
                "expected a struct body with named fields"
            } else {
                "expected an enum body with unit variants"
            },
        )),
    }
}

/// Read the leading outer attributes starting at `at`.
fn attributes<'a>(tokens: &'a [Tree], at: &mut usize) -> Result<Attrs<'a>, Diagnostic> {
    let mut attrs = Attrs::default();
    while let Some(hash) = tokens.get(*at).filter(|tree| tree.is_punct('#')) {
        let Some(inner) = tokens
            .get(*at + 1)
            .and_then(|tree| tree.group(Delim::Bracket))
        else {
            return Err(Diagnostic::new(hash.span, "expected `[` after `#`"));
        };
        *at += 2;
        let Some(head) = inner.first() else { continue };
        match head.ident() {
            Some("doc") => {
                if let [_, equals, literal] = inner
                    && equals.is_punct('=')
                    && let Some(value) = literal.literal().and_then(decode_string)
                {
                    attrs.docs.push(value);
                }
            }
            Some(name @ ("tool" | "param")) => {
                let kind = if name == "tool" {
                    AttrKind::Tool
                } else {
                    AttrKind::Param
                };
                let args = match inner {
                    [_, group] => group.group(Delim::Paren),
                    _ => None,
                };
                let Some(args) = args else {
                    return Err(Diagnostic::new(
                        head.span,
                        format!(
                            "`#[{0}]` needs a parenthesized list of keys: `#[{0}(key = value, ..)]`",
                            kind.name()
                        ),
                    ));
                };
                attrs.helpers.push(RawAttr {
                    kind,
                    span: head.span,
                    args,
                });
            }
            _ => {}
        }
    }
    Ok(attrs)
}

/// Fail on a helper attribute of `kind` where it does not apply.
fn reject_helpers(attrs: &Attrs<'_>, kind: AttrKind, site: Site) -> Result<(), Diagnostic> {
    let hint = match (kind, site) {
        (AttrKind::Tool, Site::Field | Site::Variant) => "; it belongs on the struct or enum",
        (AttrKind::Param, Site::Struct | Site::Enum) => "; it belongs on fields and variants",
        _ => "",
    };
    attrs
        .helpers
        .iter()
        .find(|attr| attr.kind == kind)
        .map_or(Ok(()), |attr| {
            Err(Diagnostic::new(
                attr.span,
                format!(
                    "`#[{}]` does not apply to {}{hint}",
                    kind.name(),
                    site.describe(),
                ),
            ))
        })
}

/// One `key` or `key = value` item of an attribute argument list.
struct KeyItem<'a> {
    key: &'a Tree,
    value: Option<&'a [Tree]>,
}

impl KeyItem<'_> {
    fn name(&self) -> &str {
        self.key.ident().unwrap_or_default()
    }
}

/// Split an argument list at its top-level commas into key items.
fn key_items(args: &[Tree]) -> Result<Vec<KeyItem<'_>>, Diagnostic> {
    let mut items = Vec::new();
    for part in args.split(|tree| tree.is_punct(',')) {
        let Some(key) = part.first() else { continue };
        if key.ident().is_none() {
            return Err(Diagnostic::new(key.span, "expected a key name"));
        }
        let value = match &part[1..] {
            [] => None,
            [equals, value @ ..] if equals.is_punct('=') => {
                if value.is_empty() {
                    return Err(Diagnostic::new(equals.span, "expected a value after `=`"));
                }
                Some(value)
            }
            [other, ..] => {
                return Err(Diagnostic::new(
                    other.span,
                    "expected `=` and a value, or a comma",
                ));
            }
        };
        items.push(KeyItem { key, value });
    }
    Ok(items)
}

fn unknown_key(attr: AttrKind, site: Site, item: &KeyItem<'_>, accepted: &[&str]) -> Diagnostic {
    Diagnostic::new(
        item.key.span,
        format!(
            "unknown key `{}` in `#[{}(..)]` on {}; expected one of: {}",
            item.name(),
            attr.name(),
            site.describe(),
            accepted.join(", ")
        ),
    )
}

fn text_value(item: &KeyItem<'_>) -> Result<Text, Diagnostic> {
    let Some(value) = item.value else {
        return Err(Diagnostic::new(
            item.key.span,
            format!("`{0}` needs a value: `{0} = \"...\"`", item.name()),
        ));
    };
    match value {
        [literal] => match literal
            .literal()
            .and_then(|text| decode_string(text).map(|decoded| (text.to_owned(), decoded)))
        {
            Some((source, decoded)) => Ok(Text {
                source,
                value: decoded,
                span: literal.span,
            }),
            None => Err(Diagnostic::new(
                literal.span,
                format!("`{}` must be a string literal", item.name()),
            )),
        },
        [first, ..] => Err(Diagnostic::new(
            first.span,
            format!("`{}` must be a single string literal", item.name()),
        )),
        [] => Err(Diagnostic::new(
            item.key.span,
            format!("`{}` needs a value after `=`", item.name()),
        )),
    }
}

/// Keywords that cannot be a path segment (a raw identifier can).
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where", "while",
    "Self",
];

/// `crate = path`: a plain path of identifiers joined by `::`, with an optional
/// leading `::`. `crate`, `self` and `super` may only start it (`super` may
/// repeat), and then not after a leading `::`.
fn crate_path(item: &KeyItem<'_>) -> Result<CratePath, Diagnostic> {
    const SHAPE: &str = "`crate` must be a path such as `::purrassist`, `my_alias` or \
                         `crate::reexports::pa`";
    let Some(tokens) = item.value else {
        return Err(Diagnostic::new(
            item.key.span,
            format!("`crate` needs a value: `crate = path`; {SHAPE}"),
        ));
    };
    let separator = |at: usize| {
        tokens.get(at).is_some_and(|tree| {
            matches!(
                tree.kind,
                Kind::Punct {
                    ch: ':',
                    joint: true
                }
            )
        }) && tokens.get(at + 1).is_some_and(|tree| tree.is_punct(':'))
    };
    let mut source = String::new();
    let mut at = if separator(0) { 2 } else { 0 };
    if at == 2 {
        source.push_str("::");
    }
    let mut first = at == 0;
    loop {
        let Some(tree) = tokens.get(at) else {
            // Only reachable after a `::`.
            return Err(Diagnostic::new(
                tokens[at - 1].span,
                format!("`crate` path ends with `::`; {SHAPE}"),
            ));
        };
        let Some(segment) = tree.ident() else {
            return Err(Diagnostic::new(
                tree.span,
                format!("expected a path segment in `crate`; {SHAPE}"),
            ));
        };
        let leading = matches!(segment, "crate" | "self" | "super");
        if KEYWORDS.contains(&segment) || (leading && !first) {
            return Err(Diagnostic::new(
                tree.span,
                format!("`{segment}` cannot be a segment of the `crate` path here; {SHAPE}"),
            ));
        }
        source.push_str(segment);
        at += 1;
        if at == tokens.len() {
            break;
        }
        if !separator(at) {
            return Err(Diagnostic::new(
                tokens[at].span,
                format!("expected `::` between the segments of `crate`; {SHAPE}"),
            ));
        }
        source.push_str("::");
        at += 2;
        first = first && segment == "super";
    }
    Ok(CratePath {
        source,
        span: tokens[0].span,
    })
}

fn choice_value<T: Copy>(item: &KeyItem<'_>, choices: &[(&str, T)]) -> Result<T, Diagnostic> {
    let list = || {
        choices
            .iter()
            .map(|(name, _)| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(" or ")
    };
    let Some(value) = item.value else {
        return Err(Diagnostic::new(
            item.key.span,
            format!("`{}` needs a value: {}", item.name(), list()),
        ));
    };
    match value {
        [token] => token
            .ident()
            .and_then(|word| choices.iter().find(|(name, _)| *name == word))
            .map(|(_, choice)| *choice)
            .ok_or_else(|| {
                Diagnostic::new(token.span, format!("`{}` must be {}", item.name(), list()))
            }),
        [first, ..] => Err(Diagnostic::new(
            first.span,
            format!("`{}` must be {}", item.name(), list()),
        )),
        [] => Err(Diagnostic::new(
            item.key.span,
            format!("`{}` needs a value after `=`", item.name()),
        )),
    }
}

fn flag(item: &KeyItem<'_>) -> Result<Span, Diagnostic> {
    item.value.map_or(Ok(item.key.span), |value| {
        Err(Diagnostic::new(
            value[0].span,
            format!("`{}` takes no value", item.name()),
        ))
    })
}

fn once<T>(slot: &mut Option<T>, item: &KeyItem<'_>, value: T) -> Result<(), Diagnostic> {
    if slot.is_some() {
        return Err(Diagnostic::new(
            item.key.span,
            format!("duplicate key `{}`", item.name()),
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn tool_attrs(attrs: &Attrs<'_>, site: Site) -> Result<ToolAttrs, Diagnostic> {
    let accepted: &[&str] = match site {
        Site::Enum => &["description", "crate"],
        _ => &[
            "name",
            "alias",
            "description",
            "effects",
            "policy",
            "unknown",
            "crate",
        ],
    };
    let mut out = ToolAttrs::default();
    for attr in attrs.helpers.iter().filter(|a| a.kind == AttrKind::Tool) {
        for item in key_items(attr.args)? {
            if !accepted.contains(&item.name()) {
                return Err(unknown_key(AttrKind::Tool, site, &item, accepted));
            }
            match item.name() {
                "crate" => once(&mut out.crate_path, &item, crate_path(&item)?)?,
                "name" => once(&mut out.name, &item, text_value(&item)?)?,
                "alias" => out.aliases.push(text_value(&item)?),
                "description" => once(&mut out.description, &item, text_value(&item)?)?,
                "effects" => once(
                    &mut out.effects,
                    &item,
                    choice_value(
                        &item,
                        &[
                            ("mutating", Effects::Mutating),
                            ("read_only", Effects::ReadOnly),
                        ],
                    )?,
                )?,
                "policy" => once(
                    &mut out.policy,
                    &item,
                    choice_value(
                        &item,
                        &[
                            ("structural", Policy::Structural),
                            ("tolerant", Policy::Tolerant),
                        ],
                    )?,
                )?,
                _ => {
                    let unknown = choice_value(
                        &item,
                        &[
                            ("error", Unknown::Error),
                            ("drop", Unknown::Drop),
                            ("pass", Unknown::Pass),
                        ],
                    )?;
                    let span = item.value.map_or(item.key.span, |value| value[0].span);
                    once(&mut out.unknown, &item, (unknown, span))?;
                }
            }
        }
    }
    Ok(out)
}

fn param_attrs(attrs: &Attrs<'_>, site: Site) -> Result<ParamAttrs, Diagnostic> {
    let accepted: &[&str] = match site {
        Site::Variant => &["name", "description"],
        _ => &[
            "name",
            "alias",
            "description",
            "repeat",
            "default",
            "pattern",
            "extra",
        ],
    };
    let mut out = ParamAttrs::default();
    for attr in attrs.helpers.iter().filter(|a| a.kind == AttrKind::Param) {
        for item in key_items(attr.args)? {
            if !accepted.contains(&item.name()) {
                return Err(unknown_key(AttrKind::Param, site, &item, accepted));
            }
            match item.name() {
                "name" => once(&mut out.name, &item, text_value(&item)?)?,
                "alias" => out.aliases.push(text_value(&item)?),
                "description" => once(&mut out.description, &item, text_value(&item)?)?,
                "repeat" => once(&mut out.repeat, &item, flag(&item)?)?,
                "extra" => once(&mut out.extra, &item, flag(&item)?)?,
                "pattern" => once(&mut out.pattern, &item, text_value(&item)?)?,
                _ => {
                    let Some(value) = item.value else {
                        return Err(Diagnostic::new(
                            item.key.span,
                            "`default` needs a value: `default = <JSON value>`",
                        ));
                    };
                    once(
                        &mut out.default,
                        &item,
                        default_value(value, item.key.span)?,
                    )?;
                }
            }
        }
    }
    Ok(out)
}

/// Parse the tokens of a `default = ..` value as Rust-token JSON.
///
/// # Errors
/// A [`Diagnostic`] at the first token that is not part of a JSON value.
pub(crate) fn default_value(tokens: &[Tree], at: Span) -> Result<DefaultValue, Diagnostic> {
    const EXPECTED: &str = "`default` must be a JSON value: null, true, false, a number, a \
                            string, an array `[..]` or an object `{ \"key\": value }`";
    match tokens {
        [] => Err(Diagnostic::new(at, "expected a JSON value")),
        [word] if word.is_ident("null") => Ok(DefaultValue::Null),
        [word] if word.is_ident("true") => Ok(DefaultValue::Bool(true)),
        [word] if word.is_ident("false") => Ok(DefaultValue::Bool(false)),
        [literal] if literal.literal().is_some() => literal_value(literal, false),
        [minus, literal] if minus.is_punct('-') && literal.literal().is_some() => {
            literal_value(literal, true)
        }
        [group] if group.group(Delim::Bracket).is_some() => {
            let mut items = Vec::new();
            for part in comma_parts(group.group(Delim::Bracket).unwrap_or_default())? {
                items.push(default_value(part, group.span)?);
            }
            Ok(DefaultValue::Array(items))
        }
        [group] if group.group(Delim::Brace).is_some() => {
            let mut members = Vec::new();
            for part in comma_parts(group.group(Delim::Brace).unwrap_or_default())? {
                let [key, colon, value @ ..] = part else {
                    return Err(Diagnostic::new(
                        part[0].span,
                        "an object member is `\"key\": value`",
                    ));
                };
                let Some(source) = key.literal().filter(|text| decode_string(text).is_some())
                else {
                    return Err(Diagnostic::new(
                        key.span,
                        "an object key must be a string literal",
                    ));
                };
                if !colon.is_punct(':') || value.is_empty() {
                    return Err(Diagnostic::new(
                        colon.span,
                        "an object member is `\"key\": value`",
                    ));
                }
                members.push((source.to_owned(), default_value(value, colon.span)?));
            }
            Ok(DefaultValue::Object(members))
        }
        [first, ..] => Err(Diagnostic::new(first.span, EXPECTED)),
    }
}

/// The comma-separated parts of an array or object body; one trailing comma
/// is allowed and an empty part elsewhere is an error.
fn comma_parts(tokens: &[Tree]) -> Result<Vec<&[Tree]>, Diagnostic> {
    let mut parts: Vec<&[Tree]> = tokens.split(|tree| tree.is_punct(',')).collect();
    if parts.last().is_some_and(|last| last.is_empty()) {
        parts.pop();
    }
    if let Some(position) = parts.iter().position(|part| part.is_empty()) {
        let span = tokens
            .iter()
            .filter(|tree| tree.is_punct(','))
            .nth(position)
            .map_or(0, |comma| comma.span);
        return Err(Diagnostic::new(span, "expected a value before this comma"));
    }
    Ok(parts)
}

fn literal_value(literal: &Tree, negative: bool) -> Result<DefaultValue, Diagnostic> {
    let text = literal.literal().unwrap_or_default();
    if decode_string(text).is_some() {
        return if negative {
            Err(Diagnostic::new(
                literal.span,
                "a string cannot be negated; remove the `-`",
            ))
        } else {
            Ok(DefaultValue::Str(text.to_owned()))
        };
    }
    if text.starts_with(|c: char| c.is_ascii_digit()) {
        return if is_json_number(text) {
            Ok(DefaultValue::Number(if negative {
                format!("-{text}")
            } else {
                text.to_owned()
            }))
        } else {
            Err(Diagnostic::new(
                literal.span,
                format!(
                    "`{text}` is not a JSON number: no suffix, `_`, radix prefix, leading zero \
                     or trailing `.`"
                ),
            ))
        };
    }
    Err(Diagnostic::new(
        literal.span,
        "`default` must be a JSON value: null, true, false, a number, a string, an array or \
         an object",
    ))
}

fn ident_of(tree: &Tree) -> Option<Ident> {
    tree.ident().map(|text| Ident {
        text: text.to_owned(),
        span: tree.span,
    })
}

/// Parse the named fields inside a struct body.
fn fields(tokens: &[Tree]) -> Result<Vec<RawField>, Diagnostic> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let attrs = attributes(tokens, &mut at)?;
        if tokens.get(at).is_some_and(|tree| tree.is_ident("pub")) {
            at += 1;
            if tokens
                .get(at)
                .is_some_and(|tree| tree.group(Delim::Paren).is_some())
            {
                at += 1;
            }
        }
        let Some(name) = tokens.get(at).and_then(ident_of) else {
            return Err(Diagnostic::new(
                tokens
                    .get(at)
                    .or_else(|| tokens.last())
                    .map_or(0, |tree| tree.span),
                "expected a field name",
            ));
        };
        at += 1;
        let colon = match tokens.get(at) {
            Some(colon) if colon.is_punct(':') => colon,
            other => {
                return Err(Diagnostic::new(
                    other.map_or(name.span, |tree| tree.span),
                    format!("expected `:` and a type after `{}`", name.bare()),
                ));
            }
        };
        at += 1;
        let start = at;
        let mut depth = 0_usize;
        while let Some(tree) = tokens.get(at) {
            if depth == 0 && tree.is_punct(',') {
                break;
            }
            if tree.is_punct('<') {
                depth += 1;
            } else if tree.is_punct('>') {
                let arrow = at > start
                    && matches!(
                        &tokens[at - 1].kind,
                        Kind::Punct {
                            ch: '-',
                            joint: true
                        }
                    );
                if !arrow {
                    depth = depth.saturating_sub(1);
                }
            }
            at += 1;
        }
        let ty = &tokens[start..at];
        if ty.is_empty() {
            return Err(Diagnostic::new(
                colon.span,
                format!("expected a type for field `{}`", name.bare()),
            ));
        }
        at += 1;
        reject_helpers(&attrs, AttrKind::Tool, Site::Field)?;
        out.push(RawField {
            attrs: param_attrs(&attrs, Site::Field)?,
            doc: doc_description(&attrs.docs),
            ident: name,
            ty: ty.to_vec(),
        });
    }
    Ok(out)
}

/// Parse the unit variants inside an enum body.
fn variants(tokens: &[Tree]) -> Result<Vec<RawVariant>, Diagnostic> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let attrs = attributes(tokens, &mut at)?;
        let Some(ident) = tokens.get(at).and_then(ident_of) else {
            return Err(Diagnostic::new(
                tokens
                    .get(at)
                    .or_else(|| tokens.last())
                    .map_or(0, |tree| tree.span),
                "expected a variant name",
            ));
        };
        at += 1;
        if let Some(group) = tokens
            .get(at)
            .filter(|tree| matches!(tree.kind, Kind::Group { .. }))
        {
            return Err(Diagnostic::new(
                group.span,
                format!(
                    "variant `{}` carries data: a ToolEnum is a string enumeration, so every \
                     variant must be a unit variant",
                    ident.bare()
                ),
            ));
        }
        // An explicit discriminant has no effect on the string value.
        while tokens.get(at).is_some_and(|tree| !tree.is_punct(',')) {
            at += 1;
        }
        at += 1;
        reject_helpers(&attrs, AttrKind::Tool, Site::Variant)?;
        let param = param_attrs(&attrs, Site::Variant)?;
        out.push(RawVariant {
            name: param.name,
            description: param.description,
            doc: doc_description(&attrs.docs),
            ident,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::lex::trees;

    fn parse_struct(source: &str) -> Result<RawItem, Diagnostic> {
        parse(Target::Definition, &trees(source))
    }

    fn parse_enum(source: &str) -> Result<RawItem, Diagnostic> {
        parse(Target::Enumeration, &trees(source))
    }

    /// The span index of the first token of `source` equal to `needle` text.
    fn span_of(source: &str, needle: &str) -> Span {
        fn find(trees: &[Tree], needle: &str) -> Option<Span> {
            for tree in trees {
                let hit = match &tree.kind {
                    Kind::Ident(text) | Kind::Literal(text) => text == needle,
                    Kind::Punct { ch, .. } => needle.len() == 1 && needle.starts_with(*ch),
                    Kind::Group { .. } => false,
                };
                if hit {
                    return Some(tree.span);
                }
                if let Kind::Group { inner, .. } = &tree.kind
                    && let Some(found) = find(inner, needle)
                {
                    return Some(found);
                }
            }
            None
        }
        find(&trees(source), needle).expect("token present")
    }

    /// The span index of the last token of `source` whose text is `needle`.
    fn span_of_last(source: &str, needle: &str) -> Span {
        fn walk(trees: &[Tree], needle: &str, last: &mut Option<Span>) {
            for tree in trees {
                match &tree.kind {
                    Kind::Ident(text) | Kind::Literal(text) if text == needle => {
                        *last = Some(tree.span);
                    }
                    Kind::Punct { ch, .. } if needle.len() == 1 && needle.starts_with(*ch) => {
                        *last = Some(tree.span);
                    }
                    Kind::Group { inner, .. } => walk(inner, needle, last),
                    _ => {}
                }
            }
        }
        let mut last = None;
        walk(&trees(source), needle, &mut last);
        last.expect("token present")
    }

    fn error(source: &str) -> Diagnostic {
        parse_struct(source).expect_err(source)
    }

    #[test]
    fn named_fields_attributes_and_visibility_parse() {
        let source = r#"
            #[derive(Debug)]
            #[doc = " Reads a file."]
            #[tool(name = "read", alias = "cat", alias = "type", effects = read_only, policy = tolerant, unknown = drop)]
            pub(crate) struct ReadFile {
                #[doc = " The path."]
                #[param(name = "p", alias = "file", default = "x", pattern = "^a")]
                pub(crate) path: String,
                pub(in crate::a) r#type: ::std::vec::Vec<Option<u8>>,
                #[param(repeat)]
                tags: Vec<String>,
                #[cfg(feature = "x")]
                maybe: Option<Vec<std::collections::BTreeMap<String, u8>>>,
            }
        "#;
        let item = parse_struct(source).unwrap();
        assert_eq!(item.ident.text, "ReadFile");
        assert_eq!(item.doc.as_deref(), Some("Reads a file."));
        assert_eq!(item.tool.name.as_ref().unwrap().value, "read");
        assert_eq!(
            item.tool
                .aliases
                .iter()
                .map(|a| a.value.as_str())
                .collect::<Vec<_>>(),
            ["cat", "type"]
        );
        assert_eq!(item.tool.effects, Some(Effects::ReadOnly));
        assert_eq!(item.tool.policy, Some(Policy::Tolerant));
        assert_eq!(item.tool.unknown.map(|u| u.0), Some(Unknown::Drop));
        let Body::Struct(fields) = item.body else {
            panic!()
        };
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].doc.as_deref(), Some("The path."));
        assert_eq!(fields[0].attrs.name.as_ref().unwrap().value, "p");
        assert_eq!(
            fields[0].attrs.default,
            Some(DefaultValue::Str("\"x\"".into()))
        );
        assert_eq!(fields[0].attrs.pattern.as_ref().unwrap().value, "^a");
        assert_eq!(fields[1].ident.bare(), "type");
        assert_eq!(fields[1].ident.text, "r#type");
        // The comma inside the generic arguments does not end the type.
        assert_eq!(
            fields[3].ty.len(),
            18,
            "commas inside `<..>` stay in the type"
        );
        assert!(fields[2].attrs.repeat.is_some());
    }

    #[test]
    fn unit_and_empty_structs_are_zero_parameter_tools() {
        for source in [
            "struct Ping;",
            "struct Ping {}",
            "pub(crate) struct Ping { }",
        ] {
            let item = parse_struct(source).unwrap();
            assert_eq!(item.body, Body::Struct(Vec::new()), "{source}");
        }
    }

    #[test]
    fn enums_parse_with_names_docs_and_discriminants() {
        let item = parse_enum(
            r#"#[tool(description = "A mode.")]
               enum Mode { #[param(name = "ASC")] Asc = 1, #[doc = " d"]
               Desc, r#type }"#,
        )
        .unwrap();
        assert_eq!(item.tool.description.as_ref().unwrap().value, "A mode.");
        let Body::Enum(variants) = item.body else {
            panic!()
        };
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].name.as_ref().unwrap().value, "ASC");
        assert_eq!(variants[2].ident.bare(), "type");
    }

    #[test]
    fn generics_are_rejected_at_the_first_generic_token() {
        for (source, token) in [
            ("struct A<T> { t: T }", "<"),
            ("struct A<'a> { t: &'a str }", "<"),
            ("struct A<const N: usize> {}", "<"),
            ("struct A where u8: Copy { t: u8 }", "where"),
        ] {
            let diagnostic = error(source);
            assert_eq!(diagnostic.span, span_of(source, token), "{source}");
            assert!(diagnostic.message.contains("generic") || diagnostic.message.contains("where"));
        }
        assert_eq!(
            error("struct A<T> { t: T }").message,
            "generic parameters are not supported: a generic type has no single tool schema; use a concrete type"
        );
        assert_eq!(
            error("struct A where u8: Copy {}").message,
            "`where` clauses are not supported: generic types cannot describe tool arguments; use a concrete type"
        );
    }

    #[test]
    fn shape_errors_carry_exact_messages_and_spans() {
        let tuple = "struct A(u8);";
        let diagnostic = error(tuple);
        assert_eq!(diagnostic.span, 2, "the `(..)` group");
        assert_eq!(
            diagnostic.message,
            "tuple structs are not supported: parameters need names; use a struct with named fields"
        );
        let union = "union U { a: u8 }";
        assert_eq!(error(union).span, 0);
        assert_eq!(
            error(union).message,
            "unions cannot describe tool arguments; use a struct with named fields (`ToolDefinition`) or an enum with unit variants (`ToolEnum`)"
        );
        assert_eq!(
            parse_struct("enum E { A }").unwrap_err(),
            Diagnostic::new(
                0,
                "`#[derive(ToolDefinition)]` applies to structs with named fields; use `#[derive(ToolEnum)]` for an enum"
            )
        );
        assert_eq!(
            parse_enum("struct S { a: u8 }").unwrap_err(),
            Diagnostic::new(
                0,
                "`#[derive(ToolEnum)]` applies to enums with unit variants; use `#[derive(ToolDefinition)]` for a struct"
            )
        );
        assert_eq!(parse_enum("union U { a: u8 }").unwrap_err().span, 0);
        assert_eq!(
            parse_struct("fn f() {}").unwrap_err().message,
            "expected a struct (`ToolDefinition`) or an enum (`ToolEnum`)"
        );
    }

    #[test]
    fn non_unit_variants_are_rejected_at_the_data() {
        let message = |name: &str| {
            format!(
                "variant `{name}` carries data: a ToolEnum is a string enumeration, so every variant must be a unit variant"
            )
        };
        // `enum`=0 `E`=1 `{..}`=2 `A`=3 `,`=4 `B`=5 `(u8)`=6.
        assert_eq!(
            parse_enum("enum E { A, B(u8) }").unwrap_err(),
            Diagnostic::new(6, message("B"))
        );
        assert_eq!(
            parse_enum("enum E { A { x: u8 } }").unwrap_err(),
            Diagnostic::new(4, message("A"))
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn unknown_duplicate_and_malformed_keys() {
        let source = r#"#[tool(colour = "red")] struct A {}"#;
        assert_eq!(
            error(source),
            Diagnostic::new(
                span_of(source, "colour"),
                "unknown key `colour` in `#[tool(..)]` on a struct; expected one of: name, alias, description, effects, policy, unknown, crate"
            )
        );
        let source = "struct A { #[param(colour)] a: u8 }";
        assert_eq!(
            error(source),
            Diagnostic::new(
                span_of(source, "colour"),
                "unknown key `colour` in `#[param(..)]` on a field; expected one of: name, alias, description, repeat, default, pattern, extra"
            )
        );
        let source = "enum E { #[param(repeat)] A }";
        assert_eq!(
            parse_enum(source).unwrap_err().message,
            "unknown key `repeat` in `#[param(..)]` on an enum variant; expected one of: name, description"
        );
        assert_eq!(
            parse_enum(r#"#[tool(name = "x")] enum E { A }"#)
                .unwrap_err()
                .message,
            "unknown key `name` in `#[tool(..)]` on an enum; expected one of: description, crate"
        );
        let source = r#"#[tool(name = "a", name = "b")] struct A {}"#;
        let duplicate = error(source);
        assert_eq!(duplicate.message, "duplicate key `name`");
        assert_eq!(duplicate.span, 8, "second `name` key");
        assert_eq!(
            error("struct A { #[param(repeat, repeat)] a: Vec<u8> }").message,
            "duplicate key `repeat`"
        );
        for (source, message) in [
            (
                "#[tool(effects = loud)] struct A {}",
                "`effects` must be `mutating` or `read_only`",
            ),
            (
                "#[tool(effects)] struct A {}",
                "`effects` needs a value: `mutating` or `read_only`",
            ),
            (
                "#[tool(policy = x)] struct A {}",
                "`policy` must be `structural` or `tolerant`",
            ),
            (
                "#[tool(unknown = x)] struct A {}",
                "`unknown` must be `error` or `drop` or `pass`",
            ),
            (
                "#[tool(name)] struct A {}",
                "`name` needs a value: `name = \"...\"`",
            ),
            (
                "#[tool(name = 4)] struct A {}",
                "`name` must be a string literal",
            ),
            (
                "#[tool(name = \"a\" \"b\")] struct A {}",
                "`name` must be a single string literal",
            ),
            ("#[tool(name =)] struct A {}", "expected a value after `=`"),
            ("#[tool(\"x\")] struct A {}", "expected a key name"),
            (
                "#[tool(name \"x\")] struct A {}",
                "expected `=` and a value, or a comma",
            ),
            (
                "struct A { #[param(repeat = true)] a: Vec<u8> }",
                "`repeat` takes no value",
            ),
            (
                "struct A { #[param(default)] a: u8 }",
                "`default` needs a value: `default = <JSON value>`",
            ),
            (
                "#[tool] struct A {}",
                "`#[tool]` needs a parenthesized list of keys: `#[tool(key = value, ..)]`",
            ),
            (
                "#[param(name = \"a\")] struct A {}",
                "`#[param]` does not apply to a struct; it belongs on fields and variants",
            ),
            (
                "struct A { #[tool(name = \"a\")] a: u8 }",
                "`#[tool]` does not apply to a field; it belongs on the struct or enum",
            ),
            (
                "enum E { #[tool(name = \"a\")] A }",
                "`#[tool]` does not apply to an enum variant; it belongs on the struct or enum",
            ),
        ] {
            let result = if source.contains("enum ") {
                parse_enum(source)
            } else {
                parse_struct(source)
            };
            assert_eq!(result.expect_err(source).message, message, "{source}");
        }
    }

    #[test]
    fn foreign_attributes_and_inner_cfg_attr_pass_through() {
        let item = parse_struct(
            r#"#[derive(Clone)] #[serde(rename_all = "x")] #[cfg_attr(a, tool(name = "ignored"))]
               struct A { #[serde(skip)] #[allow(dead_code)] a: u8 }"#,
        )
        .unwrap();
        assert_eq!(item.tool, ToolAttrs::default());
    }

    #[test]
    fn field_syntax_errors_point_at_the_problem() {
        let source = "struct A { a u8 }";
        assert_eq!(
            error(source),
            Diagnostic::new(span_of(source, "u8"), "expected `:` and a type after `a`")
        );
        let source = "struct A { a: }";
        assert_eq!(
            error(source),
            Diagnostic::new(span_of(source, ":"), "expected a type for field `a`")
        );
        let source = "struct A { 1: u8 }";
        assert_eq!(
            error(source),
            Diagnostic::new(span_of(source, "1"), "expected a field name")
        );
        assert_eq!(error("struct").message, "expected the name of the type");
        assert_eq!(
            error("struct A").message,
            "expected a struct body with named fields"
        );
        assert_eq!(
            parse_enum("enum E").unwrap_err().message,
            "expected an enum body with unit variants"
        );
        assert_eq!(
            parse_enum("enum E { 1 }").unwrap_err().message,
            "expected a variant name"
        );
        assert_eq!(error("#").message, "expected `[` after `#`");
    }

    #[test]
    fn arrow_tokens_do_not_close_a_generic_list() {
        let item = parse_struct("struct A { f: Box<dyn Fn(u8) -> u8>, g: u8 }").unwrap();
        let Body::Struct(fields) = item.body else {
            panic!()
        };
        assert_eq!(fields.len(), 2);
    }

    #[test]
    fn defaults_parse_as_rust_token_json() {
        let parse = |source: &str| default_value(&trees(source), 0);
        assert_eq!(parse("null"), Ok(DefaultValue::Null));
        assert_eq!(parse("true"), Ok(DefaultValue::Bool(true)));
        assert_eq!(parse("-1.5"), Ok(DefaultValue::Number("-1.5".into())));
        assert_eq!(parse("10"), Ok(DefaultValue::Number("10".into())));
        assert_eq!(parse("2.5e-3"), Ok(DefaultValue::Number("2.5e-3".into())));
        assert_eq!(parse(r#""s\n""#), Ok(DefaultValue::Str(r#""s\n""#.into())));
        assert_eq!(
            parse(r#"[1, [], "a", { "k": null, },]"#),
            Ok(DefaultValue::Array(vec![
                DefaultValue::Number("1".into()),
                DefaultValue::Array(Vec::new()),
                DefaultValue::Str("\"a\"".into()),
                DefaultValue::Object(vec![("\"k\"".into(), DefaultValue::Null)]),
            ]))
        );
        assert_eq!(parse("{}"), Ok(DefaultValue::Object(Vec::new())));
    }

    #[test]
    fn an_empty_value_is_an_error_at_the_given_span() {
        assert_eq!(
            default_value(&[], 7),
            Err(Diagnostic::new(7, "expected a JSON value"))
        );
    }

    #[test]
    fn malformed_defaults_are_rejected_at_the_token() {
        let expect = |source: &str, span: Span, message: &str| {
            assert_eq!(
                default_value(&trees(source), 0),
                Err(Diagnostic::new(span, message)),
                "{source}"
            );
        };
        expect(
            "1_000",
            0,
            "`1_000` is not a JSON number: no suffix, `_`, radix prefix, leading zero or trailing `.`",
        );
        expect(
            "1u8",
            0,
            "`1u8` is not a JSON number: no suffix, `_`, radix prefix, leading zero or trailing `.`",
        );
        expect(
            "0x10",
            0,
            "`0x10` is not a JSON number: no suffix, `_`, radix prefix, leading zero or trailing `.`",
        );
        let negated_char = [
            Tree {
                span: 0,
                kind: Kind::Punct {
                    ch: '-',
                    joint: false,
                },
            },
            Tree {
                span: 1,
                kind: Kind::Literal("'c'".into()),
            },
        ];
        assert_eq!(
            default_value(&negated_char, 0),
            Err(Diagnostic::new(
                1,
                "`default` must be a JSON value: null, true, false, a number, a string, an array or an object"
            ))
        );
        expect("-\"s\"", 1, "a string cannot be negated; remove the `-`");
        expect(
            "+1",
            0,
            "`default` must be a JSON value: null, true, false, a number, a string, an array `[..]` or an object `{ \"key\": value }`",
        );
        expect(
            "foo",
            0,
            "`default` must be a JSON value: null, true, false, a number, a string, an array `[..]` or an object `{ \"key\": value }`",
        );
        expect("[1,,2]", 3, "expected a value before this comma");
        expect("{ 1: 2 }", 1, "an object key must be a string literal");
        expect("{ \"a\" 2 }", 2, "an object member is `\"key\": value`");
        expect("{ \"a\" }", 1, "an object member is `\"key\": value`");
        expect("{ \"a\": }", 2, "an object member is `\"key\": value`");
        expect(
            "1 2",
            0,
            "`default` must be a JSON value: null, true, false, a number, a string, an array `[..]` or an object `{ \"key\": value }`",
        );
    }

    #[test]
    fn crate_paths_are_plain_paths() {
        for path in [
            "::purrassist",
            "::purrassist_core",
            "my_alias",
            "crate::reexports::pa",
            "self::pa",
            "super::super::pa",
            "::a::r#fn",
        ] {
            let spelled = path.replace("::", " :: ");
            let item = parse_struct(&format!("#[tool(crate = {spelled})] struct A {{}}")).unwrap();
            assert_eq!(item.tool.crate_path.unwrap().source, path);
            let item = parse_enum(&format!("#[tool(crate = {path})] enum E {{ A }}")).unwrap();
            assert_eq!(item.tool.crate_path.unwrap().source, path);
        }
    }

    #[test]
    fn malformed_crate_paths_are_spanned_errors() {
        const SHAPE: &str = "`crate` must be a path such as `::purrassist`, `my_alias` or \
                             `crate::reexports::pa`";
        for (value, at, detail) in [
            ("", "crate", "`crate` needs a value: `crate = path`"),
            ("a::", ":", "`crate` path ends with `::`"),
            ("::", ":", "`crate` path ends with `::`"),
            (
                "\"purrassist\"",
                "\"purrassist\"",
                "expected a path segment in `crate`",
            ),
            ("a::<T>", "<", "expected a path segment in `crate`"),
            ("a:b", ":", "expected `::` between the segments of `crate`"),
            ("a b", "b", "expected `::` between the segments of `crate`"),
            (
                "a::crate",
                "crate",
                "`crate` cannot be a segment of the `crate` path here",
            ),
            (
                "::crate",
                "crate",
                "`crate` cannot be a segment of the `crate` path here",
            ),
            (
                "fn",
                "fn",
                "`fn` cannot be a segment of the `crate` path here",
            ),
            (
                "a::Self",
                "Self",
                "`Self` cannot be a segment of the `crate` path here",
            ),
        ] {
            let equals = if value.is_empty() { "" } else { " = " };
            let source = format!("#[tool(crate{equals}{value})] struct A {{}}");
            let diagnostic = error(&source);
            assert_eq!(diagnostic.message, format!("{detail}; {SHAPE}"), "{source}");
            // Spans: the last token named by `at` (the value, never the key).
            assert_eq!(diagnostic.span, span_of_last(&source, at), "{source}");
        }
    }

    #[test]
    fn crate_may_be_given_once() {
        let duplicate = error("#[tool(crate = a, crate = b)] struct A {}");
        assert_eq!(duplicate.message, "duplicate key `crate`");
        let split = error("#[tool(crate = a)] #[tool(crate = b)] struct A {}");
        assert_eq!(split.message, "duplicate key `crate`");
        let enumeration = parse_enum("#[tool(crate = a, crate = b)] enum E { A }").unwrap_err();
        assert_eq!(enumeration.message, "duplicate key `crate`");
    }

    #[test]
    fn crate_is_not_a_field_or_variant_key() {
        assert!(
            error("struct A { #[param(crate = a)] x: u8 }")
                .message
                .starts_with("unknown key `crate` in `#[param(..)]` on a field")
        );
        assert!(
            parse_enum("enum E { #[param(crate = a)] A }")
                .unwrap_err()
                .message
                .starts_with("unknown key `crate` in `#[param(..)]` on an enum variant")
        );
    }
}
