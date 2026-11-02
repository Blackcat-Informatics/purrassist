// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Emit the current registry's deterministic grammar-bound documentation rows.
//!
//! This reads compiled grammar metadata. It does not recover calls or run
//! behavioral checks, benchmarks or qualification campaigns.

use purrassist_core::grammar::cost;
use purrassist_core::tool_call::DialectId;

fn main() {
    let mut total = 0_u64;
    println!("| Built-in dialect | `B_d` | `K_d` per byte |");
    println!("| --- | ---: | ---: |");
    for dialect in DialectId::BUILT_IN {
        let bound = dialect.grammar().configuration_bound();
        let work = cost::K_THREAD.saturating_mul(bound);
        total = total.saturating_add(work);
        println!("| `{}` | {bound} | {work} |", dialect.id());
    }
    println!("combined K_d: {total}");
    println!("fixed work per byte: {}", 2 + cost::K_LEX + total);
}
