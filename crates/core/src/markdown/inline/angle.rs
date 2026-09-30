// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Angle-delimited forms share precedence with code spans.
use super::{InlineKind, normalize_lines};
use std::borrow::Cow;
pub(super) fn parse(source: &str) -> Option<(usize, InlineKind)> {
    if let Some(end) = source.find('>') {
        let original = &source[1..end];
        let label = if original.contains('\0') {
            Cow::Owned(original.replace('\0', "\u{fffd}"))
        } else {
            Cow::Borrowed(original)
        };
        if absolute_uri(&label) || email(&label) {
            let destination = if absolute_uri(&label) {
                label.to_string()
            } else {
                format!("mailto:{label}")
            };
            return Some((
                end + 1,
                InlineKind::Autolink {
                    destination,
                    label: label.into_owned(),
                },
            ));
        }
    }
    let end = if source.starts_with("<!-->") {
        Some(5)
    } else if source.starts_with("<!--->") {
        Some(6)
    } else if let Some(rest) = source.strip_prefix("<!--") {
        rest.find("-->").map(|end| end + 7)
    } else if let Some(rest) = source.strip_prefix("<?") {
        rest.find("?>").map(|end| end + 4)
    } else if let Some(rest) = source.strip_prefix("<![CDATA[") {
        rest.find("]]>").map(|end| end + 12)
    } else if source.strip_prefix("<!").is_some_and(|rest| {
        let name = rest.bytes().take_while(u8::is_ascii_uppercase).count();
        name > 0
            && rest.as_bytes().get(name).is_some_and(|byte| {
                matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
            })
    }) {
        source.find('>').map(|end| end + 1)
    } else {
        super::super::html::tag_end(source)
    }?;
    Some((end, InlineKind::Html(normalize_lines(&source[..end]))))
}
fn absolute_uri(text: &str) -> bool {
    let Some((scheme, _)) = text.split_once(':') else {
        return false;
    };
    (2..=32).contains(&scheme.len())
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
        && !text
            .bytes()
            .any(|b| b <= b' ' || matches!(b, b'<' | b'>' | 127))
}
fn email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&b))
        && domain.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && part.as_bytes()[0].is_ascii_alphanumeric()
                && part.as_bytes()[part.len() - 1].is_ascii_alphanumeric()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}
pub(super) fn uri(text: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b";/?:@&=+$,-_.!~*'()#%".contains(&byte) {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    output
}
