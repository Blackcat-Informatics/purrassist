// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! The published facade's complete validated JSON Lines consumer.
use purrassist::{
    Budget, Governor, json,
    output::{ContextManagement, FieldMask, LoadingStrategy, OutputByteLimit},
    schema::Schema,
};
use std::io::Write as _;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut governor = Governor::new(Budget::default());
    let mask = FieldMask::parse_governed(".orders[]?", &mut governor)?;
    let context = ContextManagement::new(
        mask,
        OutputByteLimit::new(64)?,
        0,
        vec![],
        LoadingStrategy::Always,
    );
    let schema = Schema::compile_governed(r#"{"type":"object"}"#, &mut governor)?;
    let original = json::parse_with(r#"{"orders":[1.00e2,true]}"#, &mut governor)?;
    let artifact = context
        .project_result(&schema, &original, &mut governor)
        .map_err(|error| error.to_string())?;
    assert_eq!(artifact.measurement.measured, 12);
    std::io::stdout()
        .lock()
        .write_all(artifact.bytes.as_str().as_bytes())?;
    Ok(())
}
