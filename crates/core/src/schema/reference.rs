// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! One admitted URI, pointer and dynamic-scope resolution path.
use super::index::Map;
use super::{CompilationError, Schema, SchemaError, uri};
use crate::contract::Resource;
use crate::json::{Node, Value};
use crate::{Governor, ScratchLease, ScratchString, ScratchVec, StopReason};

pub(super) fn work(governor: &mut Governor, amount: usize) -> Result<(), StopReason> {
    governor.charge(Resource::Work, u64::try_from(amount).unwrap_or(u64::MAX))
}
pub(super) fn append(
    output: &mut ScratchString,
    mut text: &str,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    while !text.is_empty() {
        let mut end = text.len().min(4096);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        work(governor, end.saturating_add(1))?;
        if end > output.capacity() - output.len() {
            work(governor, output.len())?;
        }
        output.push_str(&text[..end])?;
        text = &text[end..];
    }
    Ok(())
}
pub(super) fn copy(text: &str, governor: &mut Governor) -> Result<ScratchString, StopReason> {
    let mut output = ScratchString::new(governor);
    append(&mut output, text, governor)?;
    Ok(output)
}
/// Decode in bounded blocks so cancellation is observed during UTF-8 validation.
pub(super) fn utf8(
    bytes: &[u8],
    governor: &mut Governor,
) -> Result<Option<ScratchString>, StopReason> {
    let mut output = ScratchString::new(governor);
    let mut offset = 0;
    while offset < bytes.len() {
        let mut end = bytes.len().min(offset.saturating_add(4096));
        if end < bytes.len() {
            while end > offset && bytes[end] & 0xc0 == 0x80 {
                work(governor, 1)?;
                end -= 1;
            }
        }
        if end == offset {
            return Ok(None);
        }
        work(governor, end - offset)?;
        let Ok(text) = core::str::from_utf8(&bytes[offset..end]) else {
            return Ok(None);
        };
        append(&mut output, text, governor)?;
        offset = end;
    }
    Ok(Some(output))
}
/// A search compares no more keys than the map contains; a comparison examines
/// at most the query's length plus its terminator. This intentionally conservative
/// bound does not depend on the standard library's private B-tree node layout.
pub(super) fn lookup_work(
    entries: usize,
    query: usize,
    governor: &mut Governor,
) -> Result<(), StopReason> {
    let entries = u64::try_from(entries).unwrap_or(u64::MAX);
    let query = u64::try_from(query).unwrap_or(u64::MAX);
    governor.charge(
        Resource::Work,
        entries
            .saturating_add(1)
            .saturating_mul(query.saturating_add(1)),
    )
}
pub(super) fn lookup(
    map: &Map<ScratchString, usize>,
    key: &str,
    governor: &mut Governor,
) -> Result<Option<usize>, StopReason> {
    Ok(map.get_governed(key, governor)?.copied())
}

pub(super) enum Error {
    Invalid {
        location: ScratchString,
        message: &'static str,
    },
    Resource(StopReason),
}
impl From<StopReason> for Error {
    fn from(reason: StopReason) -> Self {
        Self::Resource(reason)
    }
}
impl Error {
    pub(super) fn compilation(self, governor: &mut Governor) -> CompilationError {
        match self {
            Self::Resource(reason) => CompilationError::Resource(reason),
            Self::Invalid { location, message } => match copy(message, governor) {
                Ok(message) => CompilationError::InvalidSchema(SchemaError {
                    location: location.into_string(),
                    message: message.into_string(),
                }),
                Err(reason) => CompilationError::Resource(reason),
            },
        }
    }
    pub(super) fn runtime(self, governor: &mut Governor) -> StopReason {
        match self {
            Self::Resource(reason) => reason,
            Self::Invalid { location, message } => {
                let mut text = ScratchString::new(governor);
                for part in [message, " at schema ", location.as_str()] {
                    if let Err(reason) = append(&mut text, part, governor) {
                        return reason;
                    }
                }
                StopReason::InvalidInput(text.into_string())
            }
        }
    }
}
pub(super) fn invalid(location: &str, message: &'static str, governor: &mut Governor) -> Error {
    match copy(location, governor) {
        Ok(location) => Error::Invalid { location, message },
        Err(reason) => Error::Resource(reason),
    }
}

/// Pointer lookup owns its key through linkage. Capacity is admitted once from
/// the starting route and the exact number of pointer segments, before copying.
pub(super) struct PointerTarget {
    pub(super) key: (usize, Vec<usize>),
    pub(super) pointer: ScratchString,
    pub(super) base_id: usize,
    _route_lease: ScratchLease,
}
pub(super) enum Target {
    Compiled(usize),
    Pointer(PointerTarget),
}
impl PointerTarget {
    pub(super) fn compiled(
        &self,
        schema: &Schema,
        governor: &mut Governor,
    ) -> Result<Option<usize>, StopReason> {
        Ok(schema
            .index
            .by_route
            .get_governed(&(self.key.0, self.key.1.as_slice()), governor)?
            .copied())
    }
}
fn body<'a>(
    schema: &'a Schema,
    id: usize,
    governor: &mut Governor,
) -> Result<&'a Node, StopReason> {
    let location = &schema.index.locations[id];
    let mut node = &schema.documents[location.document];
    for &position in &location.route {
        work(governor, 1)?;
        node = match &node.value {
            Value::Object(members) => &members[position].value,
            Value::Array(items) => &items[position],
            _ => unreachable!("compiled schema route traverses containers"),
        };
    }
    Ok(node)
}
fn fragment(text: &str, uri: &str, governor: &mut Governor) -> Result<ScratchString, Error> {
    let mut bytes = ScratchVec::<u8>::new(governor);
    // Percent decoding cannot produce more bytes than its source.
    bytes.reserve(text.len())?;
    let input = text.as_bytes();
    let mut offset = 0;
    while offset < input.len() {
        work(governor, 1)?;
        let byte = if input[offset] == b'%' {
            let pair = input.get(offset + 1..offset.saturating_add(3));
            let decoded = pair.and_then(|pair| {
                Some(char::from(pair[0]).to_digit(16)? * 16 + char::from(pair[1]).to_digit(16)?)
            });
            let Some(decoded) = decoded else {
                return Err(invalid(uri, "invalid URI fragment", governor));
            };
            offset += 3;
            u8::try_from(decoded).expect("two hexadecimal digits fit a byte")
        } else {
            let byte = input[offset];
            offset += 1;
            byte
        };
        bytes.push(byte)?;
    }
    utf8(&bytes, governor)?.ok_or_else(|| invalid(uri, "invalid URI fragment", governor))
}
fn token(text: &str, uri: &str, governor: &mut Governor) -> Result<ScratchString, Error> {
    let mut output = ScratchString::new(governor);
    // Pointer unescaping only shortens the source; avoid growth relocation.
    output.reserve(text.len())?;
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        work(governor, 4)?;
        output.push(if character == '~' {
            match chars.next() {
                Some('0') => '~',
                Some('1') => '/',
                _ => return Err(invalid(uri, "invalid JSON Pointer escape", governor)),
            }
        } else {
            character
        })?;
    }
    Ok(output)
}

pub(super) fn target(schema: &Schema, uri: &str, governor: &mut Governor) -> Result<Target, Error> {
    if let Some(id) = lookup(&schema.index.anchors, uri, governor)? {
        return Ok(Target::Compiled(id));
    }
    let split = uri::find(uri, b'#', governor)?;
    let (resource, encoded) = split.map_or((uri, ""), |i| (&uri[..i], &uri[i + 1..]));
    let root = lookup(&schema.index.resources, resource, governor)?
        .ok_or_else(|| invalid(uri, "unresolved resource; supply it explicitly", governor))?;
    if encoded.is_empty() {
        return Ok(Target::Compiled(root));
    }
    let pointer = fragment(encoded, uri, governor)?;
    if !pointer.starts_with('/') {
        return Err(invalid(uri, "unresolved anchor", governor));
    }
    let location = &schema.index.locations[root];
    let mut count = location.route.len();
    for byte in pointer.bytes() {
        work(governor, 1)?;
        if byte == b'/' {
            count = count
                .checked_add(1)
                .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes))?;
        }
    }
    let bytes = count
        .checked_mul(8)
        .ok_or(StopReason::BudgetExceeded(Resource::ScratchBytes))?;
    let lease = governor.lease_scratch(u64::try_from(bytes).unwrap_or(u64::MAX))?;
    let mut route = Vec::new();
    route
        .try_reserve_exact(count)
        .map_err(|error| crate::contract::reserve_failure(&error))?;
    for &position in &location.route {
        work(governor, 1)?;
        route.push(position);
    }
    let mut combined = copy(&location.pointer, governor)?;
    append(&mut combined, &pointer, governor)?;
    let mut target = PointerTarget {
        key: (location.document, route),
        pointer: combined,
        base_id: root,
        _route_lease: lease,
    };
    let mut node = body(schema, root, governor)?;
    let mut remaining = &pointer[1..];
    loop {
        let end = uri::find(remaining, b'/', governor)?.unwrap_or(remaining.len());
        let part = &remaining[..end];
        let token = token(part, uri, governor)?;
        let mut found = None;
        match &node.value {
            Value::Object(members) => {
                for (position, member) in members.iter().enumerate() {
                    work(
                        governor,
                        token.len().min(member.key.len()).saturating_add(1),
                    )?;
                    if member.key == token.as_str() {
                        found = Some((position, &member.value));
                        break;
                    }
                }
            }
            Value::Array(items) if token.as_str() == "0" || !token.starts_with('0') => {
                let mut index = Some(0usize);
                for byte in token.bytes() {
                    work(governor, 1)?;
                    index = if byte.is_ascii_digit() {
                        index.and_then(|i| i.checked_mul(10)?.checked_add(usize::from(byte - b'0')))
                    } else {
                        None
                    };
                }
                if !token.is_empty() {
                    found = index.and_then(|i| items.get(i).map(|node| (i, node)));
                }
            }
            _ => {}
        }
        let (position, next) =
            found.ok_or_else(|| invalid(uri, "reference pointer does not exist", governor))?;
        target.key.1.push(position);
        node = next;
        if let Some(id) = target.compiled(schema, governor)? {
            target.base_id = id;
        }
        if end == remaining.len() {
            break;
        }
        remaining = &remaining[end + 1..];
    }
    governor.check()?;
    Ok(Target::Pointer(target))
}

pub(super) fn resolve(
    schema: &Schema,
    base: &str,
    reference: &str,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let uri = uri::resolve(base, reference, governor)?;
    let result = target(schema, &uri, governor).and_then(|target| match target {
        Target::Compiled(id) => Ok(id),
        Target::Pointer(target) => target.compiled(schema, governor)?.ok_or_else(|| {
            invalid(
                &uri,
                "reference does not identify a compiled schema",
                governor,
            )
        }),
    });
    result.map_err(|error| error.runtime(governor))
}

/// Preserve the existing plain-fragment eligibility and outermost scope override.
pub(super) fn dynamic<'a>(
    schema: &Schema,
    mut target: usize,
    reference: &str,
    scope: impl Iterator<Item = &'a str>,
    governor: &mut Governor,
) -> Result<usize, StopReason> {
    let Some(split) = uri::find(reference, b'#', governor)? else {
        return Ok(target);
    };
    let fragment = &reference[split + 1..];
    if fragment.is_empty() || fragment.starts_with('/') {
        return Ok(target);
    }
    if let Some(name) = &schema.index.locations[target].dynamic_anchor {
        for base in scope {
            work(governor, 1)?;
            let mut key = copy(base, governor)?;
            append(&mut key, "#", governor)?;
            append(&mut key, name, governor)?;
            if let Some(candidate) = lookup(&schema.index.anchors, &key, governor)? {
                work(governor, name.len().saturating_add(1))?;
                if schema.index.locations[candidate].dynamic_anchor.as_ref() == Some(name) {
                    target = candidate;
                    break;
                }
            }
        }
    }
    governor.check()?;
    Ok(target)
}
