// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Exercise actual semantic lowering and byte-language inclusion, not hat admission.

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::{
    AppArmorGlob, GlobInclusion, GlobLanguage, GlobLanguageInput, LiteralPathBinding,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut governor = Governor::new(Budget::default());
    let child = AppArmorGlob::parse("@{ROOT}/{,cache/}*", &["ROOT"], &mut governor)?;
    let parent = AppArmorGlob::parse("@{ROOT}/**", &["ROOT"], &mut governor)?;
    let bindings = [LiteralPathBinding {
        name: "ROOT",
        path: "/srv/work",
    }];
    let child = GlobLanguage::compile(
        &[GlobLanguageInput {
            glob: &child,
            bindings: &bindings,
        }],
        &mut governor,
    )?;
    let parent = GlobLanguage::compile(
        &[GlobLanguageInput {
            glob: &parent,
            bindings: &bindings,
        }],
        &mut governor,
    )?;
    assert_eq!(
        child.included_in(&parent, &mut governor)?,
        GlobInclusion::Contained
    );
    let witness = match parent.included_in(&child, &mut governor)? {
        GlobInclusion::Counterexample(bytes) => bytes,
        GlobInclusion::Contained => panic!("expected a broader parent language"),
    };
    assert!(parent.accepts(&witness, &mut governor)?);
    assert!(!child.accepts(&witness, &mut governor)?);
    assert!(parent.accepts(b"/srv/work/\xff", &mut governor)?);
    Ok(())
}
