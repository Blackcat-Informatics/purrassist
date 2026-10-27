// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Actual five-field resource-limit consumer and offline qualification fixture.

use purrassist_core::{
    Budget, Governor,
    contract::{Resource, format_output_governed},
};
use purrassist_toolgraph::ResourceLimits;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let limits = match args.as_slice() {
        [] => ResourceLimits { address_space_bytes: 268_435_457, file_size_bytes: 1_048_577,
            processes: 16, cpu_seconds: 5, open_files: 64 },
        [address, file, processes, cpu, open_files] => ResourceLimits {
            address_space_bytes: decimal(address)?, file_size_bytes: decimal(file)?,
            processes: decimal(processes)?, cpu_seconds: decimal(cpu)?, open_files: decimal(open_files)?,
        },
        _ => return Err("use no arguments for the demo, or ADDRESS_BYTES FILE_BYTES PROCESSES CPU_SECONDS OPEN_FILES".into()),
    };
    let mut g = Governor::new(Budget::default());
    let fragment = limits
        .render("caller:resource-limits", &mut g)
        .map_err(|error| error.to_string())?;
    assert_eq!(fragment.limits, limits);
    assert_eq!(fragment.requirement.abi, "3.0");
    assert_eq!(fragment.requirement.mediation, "rlimit");
    g.charge(Resource::Work, 64)?;
    let policy = format_output_governed(
        format_args!(
            "abi <abi/3.0>,\nprofile resource_limits_fixture {{\n{}}}\n",
            fragment.text
        ),
        &mut g,
    )?;
    print!("{policy}");
    Ok(())
}

fn decimal(value: &str) -> Result<u64, Box<dyn std::error::Error>> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("each resource limit must be an unsigned decimal integer".into());
    }
    Ok(value.parse()?)
}
