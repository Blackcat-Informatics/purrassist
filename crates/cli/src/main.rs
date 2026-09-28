// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! `PurRAssist` command-line entry point.
mod commands;
mod mcp_stdio;
fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match commands::run(&args) {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("purrassist: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
