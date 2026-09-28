// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Frozen official meta-schemas; their BSD license is retained beside the data.
pub(super) fn resource(uri: &str) -> Option<&'static str> {
    Some(
        match uri.strip_prefix("https://json-schema.org/draft/2020-12/")? {
            "schema" => include_str!("metaschemas/schema.json"),
            "meta/core" => include_str!("metaschemas/meta-core.json"),
            "meta/applicator" => include_str!("metaschemas/meta-applicator.json"),
            "meta/unevaluated" => include_str!("metaschemas/meta-unevaluated.json"),
            "meta/validation" => include_str!("metaschemas/meta-validation.json"),
            "meta/meta-data" => include_str!("metaschemas/meta-meta-data.json"),
            "meta/format-annotation" => include_str!("metaschemas/meta-format-annotation.json"),
            "meta/content" => include_str!("metaschemas/meta-content.json"),
            _ => return None,
        },
    )
}
