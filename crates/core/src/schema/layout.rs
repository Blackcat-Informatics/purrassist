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
use crate::json::{Member, Node, Value};

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
    /// Resolve the applicable compiled child, borrowing fixed rest keywords.
    pub(super) fn child(
        &self,
        schema: &Schema,
        id: usize,
        position: usize,
        governor: &mut crate::Governor,
    ) -> Result<Option<usize>, crate::StopReason> {
        if position < self.prefix_len {
            let key = crate::contract::format_scratch_governed(
                format_args!("{}/{position}", self.prefix),
                governor,
            )?;
            super::index::child(schema, id, &key, governor).map(Some)
        } else {
            self.rest
                .map(|key| super::index::child(schema, id, key, governor))
                .transpose()
        }
    }
}

/// One conditional object constraint triggered by a present member name.
#[derive(Clone, Copy)]
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
    pub(super) fn item_layout(
        &self,
        id: usize,
        governor: &mut crate::Governor,
    ) -> Result<ItemLayout, crate::StopReason> {
        super::reference::work(governor, self.index.locations[id].route.len())?;
        let body = self.keywords(id);
        Ok(
            match body
                .get_governed("items", governor)?
                .map(|items| &items.value)
            {
                Some(Value::Array(tuple)) => ItemLayout {
                    prefix: "items",
                    prefix_len: tuple.len(),
                    rest: body
                        .get_governed("additionalItems", governor)?
                        .map(|_| "additionalItems"),
                },
                items => ItemLayout {
                    prefix: "prefixItems",
                    prefix_len: match body
                        .get_governed("prefixItems", governor)?
                        .map(|prefix| &prefix.value)
                    {
                        Some(Value::Array(prefix)) => prefix.len(),
                        _ => 0,
                    },
                    rest: items.map(|_| "items"),
                },
            },
        )
    }

    /// Every dependency of location `id`: `dependentRequired`, then
    /// `dependentSchemas`, then Draft-07 `dependencies` in member order.
    pub(super) fn dependencies<'s>(
        &'s self,
        id: usize,
        governor: &mut crate::Governor,
    ) -> Result<crate::ScratchVec<Dependency<'s>>, crate::StopReason> {
        super::reference::work(governor, self.index.locations[id].route.len())?;
        let body = self.keywords(id);
        let mut output = crate::ScratchVec::new(governor);
        for keyword in ["dependentRequired", "dependentSchemas", "dependencies"] {
            for entry in entries(body.get_governed(keyword, governor)?) {
                super::reference::work(governor, 1)?;
                let dependency = match &entry.value.value {
                    Value::Array(names) if keyword != "dependentSchemas" => Dependency::Required {
                        keyword,
                        trigger: &entry.key,
                        names,
                    },
                    _ if keyword == "dependentRequired" => continue,
                    _ => {
                        let key = super::index::pointer(keyword, &entry.key, governor)?;
                        let child = *self.index.locations[id]
                            .children
                            .get_governed(key.as_str(), governor)?
                            .expect("compiled dependency child");
                        Dependency::Schema {
                            trigger: &entry.key,
                            child,
                        }
                    }
                };
                super::reference::work(governor, 1)?;
                if output.len() == output.capacity() {
                    super::reference::work(governor, output.len())?;
                }
                output.push(dependency)?;
            }
        }
        Ok(output)
    }
}
impl crate::contract::ScratchCharge for Dependency<'_> {
    const CHARGE: usize = {
        assert!(size_of::<Self>() <= 56);
        56
    };
}
