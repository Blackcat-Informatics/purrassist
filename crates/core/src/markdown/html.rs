// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! `CommonMark`'s seven HTML block start and termination classes.

#[derive(Clone, Copy)]
pub(super) enum End {
    Blank,
    Literal(&'static str),
    Folded(&'static str),
}
impl End {
    pub(super) fn closes(self, text: &str) -> bool {
        match self {
            Self::Blank => text.trim_matches([' ', '\t']).is_empty(),
            Self::Literal(marker) => text.contains(marker),
            Self::Folded(marker) => text
                .as_bytes()
                .windows(marker.len())
                .any(|part| part.eq_ignore_ascii_case(marker.as_bytes())),
        }
    }
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "source",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

pub(super) fn start(text: &str, interrupts: bool) -> Option<End> {
    let text = super::unindent(text)?;
    for (tag, close) in [
        ("script", "</script>"),
        ("pre", "</pre>"),
        ("style", "</style>"),
        ("textarea", "</textarea>"),
    ] {
        if text
            .get(..tag.len() + 1)
            .is_some_and(|prefix| prefix.starts_with('<') && prefix[1..].eq_ignore_ascii_case(tag))
            && text
                .as_bytes()
                .get(tag.len() + 1)
                .is_some_and(|b| matches!(b, b' ' | b'\t' | b'>'))
        {
            return Some(End::Folded(close));
        }
    }
    for (open, close) in [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>")] {
        if text.starts_with(open) {
            return Some(End::Literal(close));
        }
    }
    if text.starts_with("<!") && text.as_bytes().get(2).is_some_and(u8::is_ascii_uppercase) {
        return Some(End::Literal(">"));
    }
    let tag_text = text.strip_prefix("</").or_else(|| text.strip_prefix('<'))?;
    let count = tag_text
        .bytes()
        .take_while(u8::is_ascii_alphanumeric)
        .count();
    let name = &tag_text[..count];
    let rest = &tag_text[count..];
    if BLOCK_TAGS.iter().any(|tag| name.eq_ignore_ascii_case(tag))
        && (rest.is_empty() || rest.starts_with([' ', '\t', '>']) || rest.starts_with("/>"))
    {
        return Some(End::Blank);
    }
    (!interrupts && complete_tag(text)).then_some(End::Blank)
}

// Type 7 is one complete open/close tag followed only by whitespace. Unlike
// type 6, a quoted '>' does not close the tag and invalid attributes reject it.
fn complete_tag(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut cursor = 1;
    let closing = bytes.get(cursor) == Some(&b'/');
    if closing {
        cursor += 1;
    }
    if !bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
        return false;
    }
    cursor += 1;
    while bytes
        .get(cursor)
        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'-')
    {
        cursor += 1;
    }
    loop {
        let before = cursor;
        spaces(bytes, &mut cursor);
        if bytes.get(cursor) == Some(&b'>') {
            return bytes[cursor + 1..]
                .iter()
                .all(|b| matches!(b, b' ' | b'\t'));
        }
        if !closing && bytes.get(cursor..cursor + 2) == Some(b"/>") {
            return bytes[cursor + 2..]
                .iter()
                .all(|b| matches!(b, b' ' | b'\t'));
        }
        if closing
            || before == cursor
            || !bytes
                .get(cursor)
                .is_some_and(|b| b.is_ascii_alphabetic() || matches!(b, b'_' | b':'))
        {
            return false;
        }
        cursor += 1;
        while bytes
            .get(cursor)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'))
        {
            cursor += 1;
        }
        let after_name = cursor;
        spaces(bytes, &mut cursor);
        if bytes.get(cursor) != Some(&b'=') {
            cursor = after_name;
            continue;
        }
        cursor += 1;
        spaces(bytes, &mut cursor);
        match bytes.get(cursor) {
            Some(quote @ (b'\'' | b'"')) => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(|b| b != quote) {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(quote) {
                    return false;
                }
                cursor += 1;
            }
            Some(_) => {
                let begin = cursor;
                while bytes.get(cursor).is_some_and(|b| {
                    !matches!(b, b' ' | b'\t' | b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
                }) {
                    cursor += 1;
                }
                if cursor == begin {
                    return false;
                }
            }
            None => return false,
        }
    }
}
fn spaces(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|b| matches!(b, b' ' | b'\t'))
    {
        *cursor += 1;
    }
}
