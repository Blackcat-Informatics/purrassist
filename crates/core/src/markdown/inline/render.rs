// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Nested inline rendering and structural budget checks.
use super::{Dialect, Inline, InlineKind, angle, escape};
use crate::{Governor, StopReason, contract::Resource};
pub(super) fn check(tokens: &[Inline], governor: &mut Governor) -> Result<(), StopReason> {
    let mut stack = vec![(tokens, 0usize)];
    while let Some((tokens, depth)) = stack.pop() {
        governor.charge(Resource::Depth, super::super::size(depth))?;
        for token in tokens {
            governor.charge(Resource::Work, 1)?;
            match &token.kind {
                InlineKind::Strikethrough { children }
                | InlineKind::Emphasis { children, .. }
                | InlineKind::Link { children, .. }
                | InlineKind::Image { children, .. } => stack.push((children, depth + 1)),
                _ => {}
            }
        }
    }
    Ok(())
}
pub(super) fn tokens(
    tokens: &[Inline],
    output: &mut String,
    governor: &mut Governor,
    depth: u32,
    dialect: Dialect,
    in_strong: bool,
) -> Result<(), StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    for token in tokens {
        render(token, output, governor, depth, dialect, in_strong)?;
    }
    Ok(())
}
fn render(
    token: &Inline,
    output: &mut String,
    governor: &mut Governor,
    depth: u32,
    dialect: Dialect,
    in_strong: bool,
) -> Result<(), StopReason> {
    match &token.kind {
        InlineKind::Text(text) => escape(text, output),
        InlineKind::Code(text) => {
            output.push_str("<code>");
            escape(text, output);
            output.push_str("</code>");
        }
        InlineKind::Html(text) => output.push_str(text),
        InlineKind::Autolink { destination, label } => {
            output.push_str("<a href=\"");
            escape(&angle::uri(destination), output);
            output.push_str("\">");
            escape(label, output);
            output.push_str("</a>");
        }
        InlineKind::Strikethrough { children } => {
            output.push_str("<del>");
            tokens(children, output, governor, depth + 1, dialect, false)?;
            output.push_str("</del>");
        }
        InlineKind::Emphasis { strong, children } => {
            let (open, close) = if *strong {
                ("<strong>", "</strong>")
            } else {
                ("<em>", "</em>")
            };
            let redundant = *strong && in_strong && dialect == Dialect::Gfm;
            if !redundant {
                output.push_str(open);
            }
            tokens(children, output, governor, depth + 1, dialect, *strong)?;
            if !redundant {
                output.push_str(close);
            }
        }
        InlineKind::Link {
            destination,
            title,
            children,
            ..
        } => {
            output.push_str("<a href=\"");
            escape(&angle::uri(destination), output);
            output.push('"');
            attribute_title(title.as_deref(), output);
            output.push('>');
            tokens(children, output, governor, depth + 1, dialect, false)?;
            output.push_str("</a>");
        }
        InlineKind::Image {
            destination,
            title,
            children,
            ..
        } => {
            output.push_str("<img src=\"");
            escape(&angle::uri(destination), output);
            output.push_str("\" alt=\"");
            alternative(children, output, governor, depth + 1)?;
            output.push('"');
            attribute_title(title.as_deref(), output);
            output.push_str(" />");
        }
        InlineKind::SoftBreak => output.push('\n'),
        InlineKind::HardBreak => output.push_str("<br />\n"),
        InlineKind::Boundary => {}
    }
    Ok(())
}
fn attribute_title(title: Option<&str>, output: &mut String) {
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        output.push_str(" title=\"");
        escape(title, output);
        output.push('"');
    }
}
fn alternative(
    tokens: &[Inline],
    output: &mut String,
    governor: &mut Governor,
    depth: u32,
) -> Result<(), StopReason> {
    governor.charge(Resource::Depth, u64::from(depth))?;
    for token in tokens {
        match &token.kind {
            InlineKind::Text(text) | InlineKind::Code(text) | InlineKind::Html(text) => {
                escape(text, output);
            }
            InlineKind::Autolink { label, .. } => escape(label, output),
            InlineKind::Strikethrough { children }
            | InlineKind::Emphasis { children, .. }
            | InlineKind::Link { children, .. }
            | InlineKind::Image { children, .. } => {
                alternative(children, output, governor, depth + 1)?;
            }
            InlineKind::SoftBreak | InlineKind::HardBreak => output.push('\n'),
            InlineKind::Boundary => {}
        }
    }
    Ok(())
}
