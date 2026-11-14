// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Single-source tool-graph load transcript shared by every portable target.
use purrassist_core::{Effort, Governor};
use purrassist_toolgraph::{
    PrefixTable, TERM_NAMES, TermBindings, ToolGraph, Vocabulary, register_loss_profile,
};
use purrdf_core::loss::LossProfile;

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../docs/tool-graph/TOOLS.md");

/// Load the worked example of `docs/tool-graph/TOOLS.md` and print the tool
/// count and the tool IRIs in graph order, one per line.
///
/// # Panics
/// Panics if the worked example no longer loads; that is a qualification failure.
#[must_use]
pub fn toolgraph_transcript_text() -> String {
    let vocabulary = Vocabulary::new(
        "pa",
        PrefixTable(vec![("pa".into(), NAMESPACE.into())]),
        TermBindings {
            namespace_iri: NAMESPACE.into(),
            local_names: TERM_NAMES
                .iter()
                .map(|name| ((*name).into(), format!("{NAMESPACE}{name}")))
                .collect(),
        },
        register_loss_profile(LossProfile::builtin()).expect("loss profile"),
    )
    .expect("worked vocabulary");
    let section = TOOLS
        .split("## End-to-end example")
        .nth(1)
        .expect("worked example section");
    let fence = section.split("```turtle\n").nth(1).expect("turtle fence");
    let source = fence.split("\n```").next().expect("closed fence");
    let graph = ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut Governor::new(Effort::Standard.budget()),
    )
    .expect("worked example loads");
    let mut text = format!("tools: {}\n", graph.tools().len());
    for tool in graph.tools() {
        text.push_str(&tool.iri);
        text.push('\n');
    }
    text
}
