// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Stable semantic output encoding used by receipt digests and protocol adapters.
use super::Interpretation;
use crate::{
    json::Node,
    wire::{object, string},
};
impl Interpretation {
    /// Encode the concept and desired attributes without normalizing their values.
    ///
    /// Alternative receipt digests hash this node's `to_json()` bytes. Map entries
    /// are ordered by their exact keys, consistently across Rust, CLI, and MCP.
    ///
    /// ```
    /// use purrassist_core::intent::{Concept, Interpretation};
    /// use std::collections::BTreeMap;
    /// let value = Interpretation {
    ///     concept: Concept { id: "tea".into(), label: "Tea".into(), attributes: BTreeMap::new() },
    ///     desired: BTreeMap::new(),
    /// };
    /// assert_eq!(value.to_node().pointer("/concept/id").unwrap().as_str(), Some("tea"));
    /// ```
    #[must_use]
    pub fn to_node(&self) -> Node {
        object([
            (
                "concept",
                object([
                    ("id", string(&self.concept.id)),
                    ("label", string(&self.concept.label)),
                    (
                        "attributes",
                        object(
                            self.concept
                                .attributes
                                .iter()
                                .map(|(key, value)| (key, string(value))),
                        ),
                    ),
                ]),
            ),
            (
                "desired",
                object(self.desired.iter().map(|(key, value)| (key, string(value)))),
            ),
        ])
    }
}
