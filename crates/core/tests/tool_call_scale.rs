// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The tool-call cost checks at the scale `docs/RESOURCES.md` quotes (`make
//! corpus`): 1 MiB arguments satisfy the published work bound and scale
//! linearly from 64 KiB, recover under the published budgets, and adding a
//! dialect adds at most its published per-dialect term per byte of a 64 KiB
//! argument. `tool_call_cost.rs` runs the same checks on small arguments in
//! `make check`.
mod tool_call_cost_support;

#[test]
fn one_mib_argument_work_bound() {
    tool_call_cost_support::argument_work_bound(1 << 16, 1 << 20);
}

#[test]
fn one_mib_argument_fits_published_budget() {
    tool_call_cost_support::argument_fits_published_budget(1 << 20);
}

#[test]
fn work_independent_of_dialect_count_beyond_bound() {
    tool_call_cost_support::work_independent_of_dialect_count(1 << 16);
}
