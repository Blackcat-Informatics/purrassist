// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The only module that touches `proc_macro`.
//!
//! It converts the derive input to the pure token model (remembering every
//! original token, so a span is an index into that table), runs the pure
//! pipeline, and converts the generated text back: parsed with `FromStr`,
//! generated locals re-spanned to `Span::mixed_site()`, and spliced input
//! tokens put back with their original spans. Errors become a spanned
//! `compile_error!`. Nothing here panics on any input.

use crate::model::{self, Delim, Diagnostic, Kind, Tree};
use crate::parse::{Target, parse};
use crate::write::{Flavor, LOCAL_PREFIX, Output, SPLICE_PREFIX, generate};
use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use std::str::FromStr;

/// Expand one derive.
pub(crate) fn expand(target: Target, flavor: Flavor, input: TokenStream) -> TokenStream {
    let mut originals = Vec::new();
    let trees = import(input, &mut originals);
    let output = parse(target, &trees)
        .and_then(model::resolve)
        .map(|item| generate(&item, flavor));
    match output {
        Ok(output) => export(&output, &originals).unwrap_or_else(|| {
            compile_error(
                "purrassist-derive internal error: the generated code did not tokenize; \
                 please report this input",
                Span::call_site(),
            )
        }),
        Err(Diagnostic { span, message }) => compile_error(
            &message,
            originals
                .get(span)
                .map_or_else(Span::call_site, TokenTree::span),
        ),
    }
}

/// Convert `stream` to the model, recording each token (numbered before its
/// contents) in `originals`. Invisible groups are flattened away.
fn import(stream: TokenStream, originals: &mut Vec<TokenTree>) -> Vec<Tree> {
    let mut out = Vec::new();
    for token in stream {
        if let TokenTree::Group(group) = &token
            && group.delimiter() == Delimiter::None
        {
            out.extend(import(group.stream(), originals));
            continue;
        }
        let span = originals.len();
        originals.push(token.clone());
        let kind = match token {
            TokenTree::Ident(ident) => Kind::Ident(ident.to_string()),
            TokenTree::Punct(punct) => Kind::Punct {
                ch: punct.as_char(),
                joint: punct.spacing() == Spacing::Joint,
            },
            TokenTree::Literal(literal) => Kind::Literal(literal.to_string()),
            TokenTree::Group(group) => Kind::Group {
                delim: match group.delimiter() {
                    Delimiter::Parenthesis => Delim::Paren,
                    Delimiter::Brace => Delim::Brace,
                    Delimiter::Bracket | Delimiter::None => Delim::Bracket,
                },
                inner: import(group.stream(), originals),
            },
        };
        out.push(Tree { span, kind });
    }
    out
}

/// Turn generated text into tokens; `None` when it does not tokenize (a
/// generator defect, reported by the caller as an error rather than a panic).
fn export(output: &Output, originals: &[TokenTree]) -> Option<TokenStream> {
    let stream = TokenStream::from_str(&output.text).ok()?;
    Some(rebuild(stream, output, originals))
}

fn rebuild(stream: TokenStream, output: &Output, originals: &[TokenTree]) -> TokenStream {
    let mut out = TokenStream::new();
    for token in stream {
        match token {
            TokenTree::Ident(ident) => {
                let text = ident.to_string();
                let spliced = text
                    .strip_prefix(SPLICE_PREFIX)
                    .and_then(|number| number.parse::<usize>().ok())
                    .and_then(|number| output.splices.get(number));
                if let Some(spans) = spliced {
                    out.extend(
                        spans
                            .iter()
                            .filter_map(|&span| originals.get(span).cloned()),
                    );
                } else if text.starts_with(LOCAL_PREFIX) {
                    out.extend([TokenTree::Ident(Ident::new(&text, Span::mixed_site()))]);
                } else {
                    out.extend([TokenTree::Ident(ident)]);
                }
            }
            TokenTree::Group(group) => {
                let mut rebuilt = Group::new(
                    group.delimiter(),
                    rebuild(group.stream(), output, originals),
                );
                rebuilt.set_span(group.span());
                out.extend([TokenTree::Group(rebuilt)]);
            }
            other => out.extend([other]),
        }
    }
    out
}

/// `::core::compile_error!("message")` with every token at `span`.
fn compile_error(message: &str, span: Span) -> TokenStream {
    let mut literal = Literal::string(message);
    literal.set_span(span);
    let mut message_group = Group::new(
        Delimiter::Brace,
        TokenStream::from(TokenTree::Literal(literal)),
    );
    message_group.set_span(span);
    let colon = |spacing| {
        let mut punct = Punct::new(':', spacing);
        punct.set_span(span);
        TokenTree::Punct(punct)
    };
    let word = |text: &str| TokenTree::Ident(Ident::new(text, span));
    let mut bang = Punct::new('!', Spacing::Alone);
    bang.set_span(span);
    [
        colon(Spacing::Joint),
        colon(Spacing::Alone),
        word("core"),
        colon(Spacing::Joint),
        colon(Spacing::Alone),
        word("compile_error"),
        TokenTree::Punct(bang),
        TokenTree::Group(message_group),
    ]
    .into_iter()
    .collect()
}
