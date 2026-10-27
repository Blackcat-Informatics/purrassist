// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Emit standalone rules to exercise the glob component, not policy synthesis.
//! Run `cargo run -p purrassist-toolgraph --example apparmor_path_words`.

use std::io::{self, Write};

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::{AppArmorGlob, LiteralPathBinding, literal_path_word};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut governor = Governor::new(Budget::default());
    let glob = AppArmorGlob::parse("@{ROOT}/{,cache/}**", &["ROOT"], &mut governor)?;
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/work files",
    }];
    let bound = glob.bind_literal_paths(&bindings, &mut governor)?;
    assert_eq!(bound, "\"/srv/work files/{,cache/}**\"");
    let literal = literal_path_word("/srv/work files/report.txt", &mut governor)?;
    let escaped = AppArmorGlob::parse(r"/tmp/literal\*", &[], &mut governor)?
        .to_policy_word(&mut governor)?;
    let class =
        AppArmorGlob::parse("/tmp/[a-z0-9]?", &[], &mut governor)?.to_policy_word(&mut governor)?;
    let mut output = io::stdout().lock();
    writeln!(output, "profile purrassist_path_word_example {{")?;
    for word in [&bound, &literal, &escaped, &class] {
        writeln!(output, "  {word} r,")?;
    }
    writeln!(output, "}}")?;
    Ok(())
}
