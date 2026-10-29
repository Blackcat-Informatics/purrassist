// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! First-party governed RFC 3986 section 5 resolution, without network access.
use super::reference::{copy, work};
use crate::{Governor, ScratchString, ScratchVec, StopReason};

#[derive(Debug)]
struct Uri<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

pub(super) fn find(
    input: &str,
    needle: u8,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    for (offset, byte) in input.bytes().enumerate() {
        work(governor, 1)?;
        if byte == needle {
            return Ok(Some(offset));
        }
    }
    Ok(None)
}
fn last_slash(input: &str, governor: &mut Governor) -> Result<Option<usize>, StopReason> {
    for (offset, byte) in input.bytes().enumerate().rev() {
        work(governor, 1)?;
        if byte == b'/' {
            return Ok(Some(offset));
        }
    }
    Ok(None)
}
impl<'a> Uri<'a> {
    fn parse(input: &'a str, governor: &mut Governor) -> Result<Self, StopReason> {
        let (input, fragment) = find(input, b'#', governor)?
            .map_or((input, None), |i| (&input[..i], Some(&input[i + 1..])));
        let (input, query) = find(input, b'?', governor)?
            .map_or((input, None), |i| (&input[..i], Some(&input[i + 1..])));
        let mut scheme_end = find(input, b':', governor)?;
        if let Some(end) = scheme_end {
            for (position, byte) in input[..end].bytes().enumerate() {
                work(governor, 1)?;
                if !(byte.is_ascii_alphabetic()
                    || (position > 0
                        && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.'))))
                {
                    scheme_end = None;
                    break;
                }
            }
            if end == 0 {
                scheme_end = None;
            }
        }
        let (scheme, tail) = scheme_end.map_or((None, input), |end| {
            (Some(&input[..end]), &input[end + 1..])
        });
        let (authority, path) = if let Some(tail) = tail.strip_prefix("//") {
            let end = find(tail, b'/', governor)?.unwrap_or(tail.len());
            (Some(&tail[..end]), &tail[end..])
        } else {
            (None, tail)
        };
        Ok(Self {
            scheme,
            authority,
            path,
            query,
            fragment,
        })
    }
}

pub(super) fn resolve(
    base: &str,
    reference: &str,
    governor: &mut Governor,
) -> Result<ScratchString, StopReason> {
    governor.check()?;
    let base = Uri::parse(base, governor)?;
    let reference = Uri::parse(reference, governor)?;
    let (scheme, authority, path, query) = if reference.scheme.is_some() {
        (
            reference.scheme,
            reference.authority,
            remove_dots(reference.path, governor)?,
            reference.query,
        )
    } else if reference.authority.is_some() {
        (
            base.scheme,
            reference.authority,
            remove_dots(reference.path, governor)?,
            reference.query,
        )
    } else if reference.path.is_empty() {
        (
            base.scheme,
            base.authority,
            copy(base.path, governor)?,
            reference.query.or(base.query),
        )
    } else {
        let mut merged = ScratchString::new(governor);
        if !reference.path.starts_with('/') {
            if base.authority.is_some() && base.path.is_empty() {
                super::reference::append(&mut merged, "/", governor)?;
            } else if let Some(end) = last_slash(base.path, governor)? {
                super::reference::append(&mut merged, &base.path[..=end], governor)?;
            }
        }
        super::reference::append(&mut merged, reference.path, governor)?;
        (
            base.scheme,
            base.authority,
            remove_dots(&merged, governor)?,
            reference.query,
        )
    };
    let mut output = ScratchString::new(governor);
    for part in [
        scheme,
        scheme.map(|_| ":"),
        authority.map(|_| "//"),
        authority,
        Some(path.as_str()),
        query.map(|_| "?"),
        query,
        reference.fragment.map(|_| "#"),
        reference.fragment,
    ]
    .into_iter()
    .flatten()
    {
        super::reference::append(&mut output, part, governor)?;
    }
    governor.check()?;
    Ok(output)
}

fn remove_dots(mut input: &str, governor: &mut Governor) -> Result<ScratchString, StopReason> {
    let mut output = ScratchVec::<u8>::new(governor);
    // Dot removal cannot grow the path, so no later push relocates bytes.
    output.reserve(input.len())?;
    while !input.is_empty() {
        work(governor, 16)?;
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
            while !output.is_empty() {
                work(governor, 1)?;
                if output.pop() == Some(b'/') {
                    break;
                }
            }
        } else if matches!(input, "." | "..") {
            input = "";
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = find(&input[start..], b'/', governor)?.map_or(input.len(), |i| start + i);
            for byte in input[..end].bytes() {
                work(governor, 1)?;
                output.push(byte)?;
            }
            input = &input[end..];
        }
    }
    // Only complete path segments and ASCII separators were removed.
    Ok(super::reference::utf8(&output, governor)?
        .expect("removing ASCII path segments preserves UTF-8"))
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
            assert_eq!(
                resolve(
                    base,
                    reference,
                    &mut crate::Governor::new(crate::Budget::default())
                )
                .unwrap()
                .as_str(),
                expected,
                "{reference}"
            );
        }
        assert_eq!(
            resolve(
                "https://example.test?key=value",
                "child",
                &mut crate::Governor::new(crate::Budget::default())
            )
            .unwrap()
            .as_str(),
            "https://example.test/child"
        );
        assert_eq!(
            resolve(
                "urn:example:source",
                "other",
                &mut crate::Governor::new(crate::Budget::default())
            )
            .unwrap()
            .as_str(),
            "urn:other"
        );

        let long = format!("{}leaf", "part/../".repeat(1024));
        let mut measured = crate::Governor::new(crate::Budget::default());
        assert_eq!(
            resolve(base, &long, &mut measured).unwrap().as_str(),
            "https://example.test/a/b/leaf"
        );
        assert_eq!(measured.scratch_meter().live(), 0);
        let usage = measured.usage();
        for (resource, budget) in [
            (
                crate::contract::Resource::Work,
                crate::Budget {
                    work: usage.work - 1,
                    ..crate::Budget::default()
                },
            ),
            (
                crate::contract::Resource::ScratchBytes,
                crate::Budget {
                    scratch_bytes: usage.peak_scratch_bytes - 1,
                    ..crate::Budget::default()
                },
            ),
        ] {
            let mut limited = crate::Governor::new(budget);
            assert_eq!(
                resolve(base, &long, &mut limited).unwrap_err(),
                crate::StopReason::BudgetExceeded(resource)
            );
            assert_eq!(limited.scratch_meter().live(), 0);
        }
        let mut exact = crate::Governor::new(crate::Budget {
            work: usage.work,
            scratch_bytes: usage.peak_scratch_bytes,
            ..crate::Budget::default()
        });
        assert_eq!(
            resolve(base, &long, &mut exact).unwrap().as_str(),
            "https://example.test/a/b/leaf"
        );
    }
}
