// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! First-party RFC 3986 section 5 reference resolution, without network access.
#[derive(Debug)]
struct Uri<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}
impl<'a> Uri<'a> {
    fn parse(input: &'a str) -> Self {
        let (input, fragment) = input
            .split_once('#')
            .map_or((input, None), |(a, b)| (a, Some(b)));
        let (input, query) = input
            .split_once('?')
            .map_or((input, None), |(a, b)| (a, Some(b)));
        let scheme_end = input.find(':').filter(|&end| {
            let mut bytes = input[..end].bytes();
            bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
                && bytes
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        });
        let (scheme, tail) = scheme_end.map_or((None, input), |end| {
            (Some(&input[..end]), &input[end + 1..])
        });
        let (authority, path) = tail.strip_prefix("//").map_or((None, tail), |tail| {
            let end = tail.find('/').unwrap_or(tail.len());
            (Some(&tail[..end]), &tail[end..])
        });
        Self {
            scheme,
            authority,
            path,
            query,
            fragment,
        }
    }
}

pub(super) fn resolve(base: &str, reference: &str) -> String {
    let base = Uri::parse(base);
    let reference = Uri::parse(reference);
    let (scheme, authority, path, query) = if reference.scheme.is_some() {
        (
            reference.scheme,
            reference.authority,
            remove_dots(reference.path),
            reference.query,
        )
    } else if reference.authority.is_some() {
        (
            base.scheme,
            reference.authority,
            remove_dots(reference.path),
            reference.query,
        )
    } else if reference.path.is_empty() {
        (
            base.scheme,
            base.authority,
            base.path.into(),
            reference.query.or(base.query),
        )
    } else {
        let path = if reference.path.starts_with('/') {
            reference.path.into()
        } else if base.authority.is_some() && base.path.is_empty() {
            format!("/{}", reference.path)
        } else {
            let prefix = base
                .path
                .rfind('/')
                .map_or("", |index| &base.path[..=index]);
            format!("{prefix}{}", reference.path)
        };
        (
            base.scheme,
            base.authority,
            remove_dots(&path),
            reference.query,
        )
    };
    let mut output = String::new();
    if let Some(scheme) = scheme {
        output.push_str(scheme);
        output.push(':');
    }
    if let Some(authority) = authority {
        output.push_str("//");
        output.push_str(authority);
    }
    output.push_str(&path);
    if let Some(query) = query {
        output.push('?');
        output.push_str(query);
    }
    if let Some(fragment) = reference.fragment {
        output.push('#');
        output.push_str(fragment);
    }
    output
}

fn remove_dots(mut input: &str) -> String {
    let mut output = String::new();
    while !input.is_empty() {
        if let Some(rest) = input
            .strip_prefix("../")
            .or_else(|| input.strip_prefix("./"))
        {
            input = rest;
        } else if input.starts_with("/./") {
            input = &input[2..];
        } else if input == "/." {
            input = "/";
        } else if input.starts_with("/../") || input == "/.." {
            input = if input == "/.." { "/" } else { &input[3..] };
            output.truncate(output.rfind('/').unwrap_or(0));
        } else if matches!(input, "." | "..") {
            input = "";
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..]
                .find('/')
                .map_or(input.len(), |index| start + index);
            output.push_str(&input[..end]);
            input = &input[end..];
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::resolve;
    #[test]
    fn hierarchy_queries_fragments_and_empty_segments_remain_distinct() {
        let base = "https://example.test/a/b/c?old";
        for (reference, expected) in [
            ("../d", "https://example.test/a/d"),
            ("../../../../d", "https://example.test/d"),
            ("./d/../e", "https://example.test/a/b/e"),
            ("?next=urn:x", "https://example.test/a/b/c?next=urn:x"),
            ("#pointer", "https://example.test/a/b/c?old#pointer"),
            ("d?next=/../", "https://example.test/a/b/d?next=/../"),
            ("//other.test/a/../b", "https://other.test/b"),
            ("/a//b/", "https://example.test/a//b/"),
            ("https://other.test/a//b/../c", "https://other.test/a//c"),
            ("", "https://example.test/a/b/c?old"),
        ] {
            assert_eq!(resolve(base, reference), expected, "{reference}");
        }
        assert_eq!(
            resolve("https://example.test?key=value", "child"),
            "https://example.test/child"
        );
        assert_eq!(resolve("urn:example:source", "other"), "urn:other");
    }
}
