// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! One normalization walk, with ordinary or caller-owned temporary buffers.
use super::{Normalization, unicode_data};
use crate::StopReason;
use crate::contract::{Buffer, Operation, Text};

pub(super) fn run(
    input: &str,
    form: Normalization,
    operation: &mut Operation<'_>,
) -> Result<Text, StopReason> {
    operation.check()?;
    let compatibility = matches!(form, Normalization::Nfkd | Normalization::Nfkc);
    let mut points = operation.buffer();
    let mut pending = operation.buffer();
    for character in input.chars() {
        operation.work(1)?;
        decompose(
            u32::from(character),
            compatibility,
            &mut points,
            &mut pending,
            operation,
        )?;
    }
    let mut scratch = operation.buffer();
    let mut start = 0;
    for index in 1..points.as_slice().len() {
        if combining(points.as_slice()[index], operation)? == 0 {
            reorder(
                &mut points.as_mut_slice()[start..index],
                &mut scratch,
                operation,
            )?;
            start = index;
        }
    }
    reorder(&mut points.as_mut_slice()[start..], &mut scratch, operation)?;
    drop(scratch);
    if matches!(form, Normalization::Nfc | Normalization::Nfkc) {
        compose(&mut points, operation)?;
    }
    let mut text = operation.text();
    for &point in points.as_slice() {
        operation.work(1)?;
        operation.push(
            &mut text,
            char::from_u32(point).expect("INVARIANT: Unicode tables contain scalar values"),
        )?;
    }
    operation.check()?;
    Ok(text)
}

pub(super) fn combining(code: u32, operation: &mut Operation<'_>) -> Result<u8, StopReason> {
    operation.lookup(unicode_data::COMBINING.len())?;
    Ok(unicode_data::COMBINING
        .binary_search_by_key(&code, |entry| entry.0)
        .map_or(0, |index| unicode_data::COMBINING[index].1))
}

fn decompose(
    code: u32,
    compatibility: bool,
    output: &mut Buffer<u32>,
    pending: &mut Buffer<(u32, u64)>,
    operation: &mut Operation<'_>,
) -> Result<(), StopReason> {
    // Terminal scalars need no decomposition stack allocation.
    let mut current = Some((code, 0));
    while let Some((code, depth)) = current.take().or_else(|| pending.pop()) {
        operation.depth(depth)?;
        operation.work(1)?;
        if (0xac00..0xd7a4).contains(&code) {
            let index = code - 0xac00;
            operation.push_buffer(output, 0x1100 + index / 588)?;
            operation.push_buffer(output, 0x1161 + (index % 588) / 28)?;
            if !index.is_multiple_of(28) {
                operation.push_buffer(output, 0x11a7 + index % 28)?;
            }
            continue;
        }
        operation.lookup(unicode_data::DECOMPOSITION.len())?;
        if let Ok(index) = unicode_data::DECOMPOSITION.binary_search_by_key(&code, |entry| entry.0)
        {
            let (_, compat, mapping) = unicode_data::DECOMPOSITION[index];
            if !compat || compatibility {
                for &point in mapping.iter().rev() {
                    operation.push_buffer(pending, (point, depth.saturating_add(1)))?;
                }
                continue;
            }
        }
        operation.push_buffer(output, code)?;
    }
    Ok(())
}

/// Stable counting preserves the source order of equal combining classes.
fn reorder(
    points: &mut [u32],
    scratch: &mut Buffer<u32>,
    operation: &mut Operation<'_>,
) -> Result<(), StopReason> {
    if points.len() < 2 {
        return Ok(());
    }
    let mut previous = 0;
    let mut ordered = true;
    for &point in points.iter() {
        let class = combining(point, operation)?;
        ordered &= previous <= class;
        previous = class;
    }
    if ordered {
        return Ok(());
    }
    operation.work(256)?;
    let mut counts = [0_usize; 256];
    for &point in points.iter() {
        counts[usize::from(combining(point, operation)?)] += 1;
    }
    operation.work(256)?;
    let mut offset = 0;
    for count in &mut counts {
        let length = *count;
        *count = offset;
        offset += length;
    }
    scratch.clear();
    for _ in points.iter() {
        operation.push_buffer(scratch, 0)?;
    }
    for &point in points.iter() {
        let class = usize::from(combining(point, operation)?);
        operation.work(1)?;
        scratch.as_mut_slice()[counts[class]] = point;
        counts[class] += 1;
    }
    operation.work(points.len())?;
    points.copy_from_slice(scratch.as_slice());
    Ok(())
}

fn pair(first: u32, second: u32, operation: &mut Operation<'_>) -> Result<Option<u32>, StopReason> {
    operation.work(1)?;
    if (0x1100..0x1113).contains(&first) && (0x1161..0x1176).contains(&second) {
        return Ok(Some(
            0xac00 + (first - 0x1100) * 588 + (second - 0x1161) * 28,
        ));
    }
    if (0xac00..0xd7a4).contains(&first)
        && (first - 0xac00).is_multiple_of(28)
        && (0x11a8..0x11c3).contains(&second)
    {
        return Ok(Some(first + second - 0x11a7));
    }
    operation.lookup(unicode_data::COMPOSITION.len())?;
    Ok(unicode_data::COMPOSITION
        .binary_search_by_key(&(first, second), |entry| (entry.0, entry.1))
        .ok()
        .map(|index| unicode_data::COMPOSITION[index].2))
}

/// Composition only removes scalars, so its output fits in the decomposition owner.
fn compose(points: &mut Buffer<u32>, operation: &mut Operation<'_>) -> Result<(), StopReason> {
    let mut written = 0;
    let mut starter = 0;
    let mut has_starter = false;
    let mut last_class = 0;
    for index in 0..points.as_slice().len() {
        let code = points.as_slice()[index];
        let class = combining(code, operation)?;
        if has_starter
            && (last_class == 0 || last_class < class)
            && let Some(composed) = pair(points.as_slice()[starter], code, operation)?
        {
            points.as_mut_slice()[starter] = composed;
            continue;
        }
        if class == 0 {
            starter = written;
            has_starter = true;
        }
        operation.work(1)?;
        points.as_mut_slice()[written] = code;
        written += 1;
        last_class = class;
    }
    operation.work(points.as_slice().len() - written)?;
    while points.as_slice().len() > written {
        points.pop();
    }
    Ok(())
}
