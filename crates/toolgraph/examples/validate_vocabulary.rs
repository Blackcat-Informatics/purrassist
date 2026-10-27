// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Complete caller construction; run with `cargo run -p purrassist-toolgraph --example validate_vocabulary`.

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::{
    FieldMask, OutputByteLimit, PrefixTable, TERM_NAMES, TermBindings, Vocabulary,
    register_loss_profile,
};
use purrdf_core::loss::LossProfile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let namespace = "https://example.org/tool-vocabulary#";
    let vocabulary = Vocabulary::new(
        "tools",
        PrefixTable(vec![("tools".into(), namespace.into())]),
        TermBindings {
            namespace_iri: namespace.into(),
            local_names: TERM_NAMES
                .iter()
                .map(|name| ((*name).into(), format!("{namespace}{name}")))
                .collect(),
        },
        register_loss_profile(LossProfile::builtin())?,
    )?;
    let context = FieldMask::parse(".orders[]?")?;
    // Both values are validated caller inputs, without loading or executing a graph.
    assert_eq!(
        vocabulary.term("Tool"),
        Some("https://example.org/tool-vocabulary#Tool")
    );
    assert_eq!(context.to_string(), ".orders[]?");
    let output_limit = OutputByteLimit::new(4)?;
    let measurement = output_limit.check(b"null", &mut Governor::new(Budget::default()))?;
    assert_eq!(measurement.measured, 4);
    Ok(())
}
