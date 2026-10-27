// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Execute with `cargo run -p purrassist-toolgraph --example admit_callable`.

#[path = "support/catalog.rs"]
pub mod catalog;

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::CallableBinding;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Preparation uses the actual upstream reader. Its allocations and parsing
    // are caller-owned, rather than governed by CallableBinding::admit.
    let catalog = catalog::read()?;
    let mut governor = Governor::new(Budget::default());
    let binding = CallableBinding::admit(&catalog, 0, &mut governor)
        .map_err(|failure| failure.to_string())?;
    assert_eq!(
        binding
            .parameter(0)
            .map(|value| value.mapping.implementation_property.as_str()),
        Some("z value")
    );
    assert_eq!(binding.output().mapping.implementation_property, "z value");
    assert_eq!(binding.mapping().node.scope.ordinal(), 42);
    Ok(())
}
