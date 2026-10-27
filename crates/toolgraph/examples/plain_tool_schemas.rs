// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Direct upstream plain `FnO` schema consumer; no graph loading or codec is implied.

#[path = "support/plain_schemas.rs"]
pub mod fixtures;

use purrassist_core::{Budget, Governor, json, schema::Schema};
use purrdf_shapes::json_schema::compile_tool_arguments;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shapes = purrdf_shapes::engine::parse_shapes(
        &format!("{}{}", fixtures::PREFIXES, fixtures::SHAPES),
        None,
    )?;
    let namespaces = fixtures::namespaces()?;
    for case in fixtures::WORKED {
        // Native work/bytes/nodes ceilings govern upstream compilation. They are
        // logical ceilings, not core Governor or complete process heap accounting.
        let compiled = compile_tool_arguments(
            &shapes,
            &namespaces,
            &fixtures::profile(case.root, case.class, case.fields),
            &fixtures::LIMITS,
        )?;
        assert!(json::parse(&compiled.schema_json)?.equivalent(&json::parse(case.expected)?));
        assert_eq!(compiled.losses.entries(), []);
        let mut governor = Governor::new(Budget::default());
        let schema = Schema::compile_governed(&compiled.schema_json, &mut governor)?;
        assert!(
            schema
                .validate_with(&json::parse(case.valid)?, &mut governor)?
                .valid
        );
        for invalid in case.invalid {
            assert!(
                !schema
                    .validate_with(&json::parse(invalid)?, &mut governor)?
                    .valid
            );
        }
    }
    Ok(())
}
