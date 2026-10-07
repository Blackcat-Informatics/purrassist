// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Dialect-neutral views of the array and dependency applicators.
//!
//! Draft 2020-12 spells positional array schemas `prefixItems` + `items` and
//! conditional object constraints `dependentRequired` + `dependentSchemas`;
//! Draft-07 spells the same semantics tuple-form `items` + `additionalItems`
//! and `dependencies`. The evaluator, witness algebra, and recovery proposals
//! read these views so each dialect has exactly one interpretation.
use super::Schema;
use crate::json::{Member, Node, Value, pointer_segment};

/// Which compiled child schema applies to each array element.
pub(super) struct ItemLayout {
    /// Child-key prefix of the positional schemas (`prefixItems` or `items`).
    pub prefix: &'static str,
    /// Number of positional schemas.
    pub prefix_len: usize,
    /// Child key of the schema for elements past the positional ones.
    pub rest: Option<&'static str>,
}

impl ItemLayout {
    /// Child key of the schema applying to element `position`, if any.
    pub(super) fn keyword(&self, position: usize) -> Option<String> {
        if position < self.prefix_len {
            Some(format!("{}/{position}", self.prefix))
        } else {
            self.rest.map(str::to_owned)
        }
    }
}

/// One conditional object constraint triggered by a present member name.
pub(super) enum Dependency<'a> {
    /// `dependentRequired` or array-form Draft-07 `dependencies`.
    Required {
        /// Keyword reported by a violation.
        keyword: &'static str,
        /// Member whose presence triggers the requirement.
        trigger: &'a str,
        /// Required member names (strings, validated at compilation).
        names: &'a [Node],
    },
    /// `dependentSchemas` or schema-form Draft-07 `dependencies`.
    Schema {
        /// Member whose presence applies the schema to the whole object.
        trigger: &'a str,
        /// Compiled child location.
        child: usize,
    },
}

fn entries(node: Option<&Node>) -> &[Member] {
    match node {
        Some(Node {
            value: Value::Object(members),
            ..
        }) => members,
        _ => &[],
    }
}

impl Schema {
    /// The positional/rest item schemas of location `id` under its dialect.
    /// Draft-07 `additionalItems` applies only beside tuple-form `items`.
    pub(super) fn item_layout(&self, id: usize) -> ItemLayout {
        let body = self.keywords(id);
        match body.get("items").map(|items| &items.value) {
            Some(Value::Array(tuple)) => ItemLayout {
                prefix: "items",
                prefix_len: tuple.len(),
                rest: body.get("additionalItems").map(|_| "additionalItems"),
            },
            items => ItemLayout {
                prefix: "prefixItems",
                prefix_len: match body.get("prefixItems").map(|prefix| &prefix.value) {
                    Some(Value::Array(prefix)) => prefix.len(),
                    _ => 0,
                },
                rest: items.map(|_| "items"),
            },
        }
    }

    /// Every dependency of location `id`: `dependentRequired`, then
    /// `dependentSchemas`, then Draft-07 `dependencies` in member order.
    pub(super) fn dependencies(&self, id: usize) -> impl Iterator<Item = Dependency<'_>> + '_ {
        let body = self.keywords(id);
        let required =
            entries(body.get("dependentRequired"))
                .iter()
                .filter_map(|entry| match &entry.value.value {
                    Value::Array(names) => Some(Dependency::Required {
                        keyword: "dependentRequired",
                        trigger: &entry.key,
                        names,
                    }),
                    _ => None,
                });
        let schemas = entries(body.get("dependentSchemas"))
            .iter()
            .map(move |entry| Dependency::Schema {
                trigger: &entry.key,
                child: self.child(
                    id,
                    &format!("dependentSchemas/{}", pointer_segment(&entry.key)),
                ),
            });
        let legacy =
            entries(body.get("dependencies"))
                .iter()
                .map(move |entry| match &entry.value.value {
                    Value::Array(names) => Dependency::Required {
                        keyword: "dependencies",
                        trigger: &entry.key,
                        names,
                    },
                    _ => Dependency::Schema {
                        trigger: &entry.key,
                        child: self
                            .child(id, &format!("dependencies/{}", pointer_segment(&entry.key))),
                    },
                });
        required.chain(schemas).chain(legacy)
    }
}
