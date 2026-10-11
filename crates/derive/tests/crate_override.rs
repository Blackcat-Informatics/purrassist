// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! `#[tool(crate = path)]`: the derives reach core through a path other than
//! `::purrassist_core`, for a renamed dependency or a re-exporting module. The
//! generated code uses the given path for every core item, for structs and
//! enums, and the types interoperate with ones derived the default way.
//! (A renamed Cargo dependency cannot be tested in-tree: Cargo rejects one
//! package under two names, so a `use` alias and re-export modules stand in
//! for it. The path is spliced as written, so the mechanism is the same.)
#![allow(dead_code, clippy::redundant_pub_crate)]

use purrassist_core::json;
use purrassist_core::tool_call::{
    Effects, FromNode, Param, ToolArguments, ToolDefinition as Definition, ToolParameter,
};
use purrassist_derive::{ToolDefinition, ToolEnum};

/// The re-export a downstream crate might keep core behind.
mod reexports {
    pub(crate) use purrassist_core as pa;
}

// A `use` alias: a bare path, as a renamed dependency is spelled.
use purrassist_core as aliased_core;

// Reached through a re-export module, from the crate root.
#[derive(ToolEnum, Debug, PartialEq)]
#[tool(crate = crate::reexports::pa)]
enum Mode {
    Fast,
    #[param(name = "careful")]
    Careful,
}

// Reached through a re-export module.
#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(crate = crate::reexports::pa, effects = read_only)]
struct Search {
    query: String,
    mode: Option<Mode>,
    limit: Vec<u8>,
}

#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(crate = aliased_core)]
struct ViaAlias {
    path: String,
}

#[derive(ToolEnum, Debug, PartialEq)]
#[tool(description = "A kind.", crate = aliased_core)]
enum KindViaAlias {
    A,
}

#[derive(ToolDefinition, Debug, PartialEq)]
#[tool(crate = ::purrassist_core)]
struct Absolute {
    path: String,
}

mod nested {
    // Relative to a module, and the same type nested in a default-derived tool.
    #[derive(purrassist_derive::ToolDefinition, Debug, PartialEq)]
    #[tool(crate = super::reexports::pa)]
    pub(crate) struct ViaSuper {
        pub(crate) depth: u8,
    }

    #[derive(purrassist_derive::ToolDefinition, Debug, PartialEq)]
    #[tool(crate = self::local::pa)]
    pub(crate) struct ViaSelf {
        pub(crate) depth: u8,
    }

    mod local {
        pub(crate) use purrassist_core as pa;
    }
}

// A default-prefix tool that nests types derived with an override.
#[derive(ToolDefinition, Debug, PartialEq)]
struct Mixed {
    search: Search,
    kind: KindViaAlias,
    via_super: nested::ViaSuper,
}

fn parse(text: &str) -> json::Node {
    json::parse(text).expect("test JSON")
}

#[test]
fn a_struct_through_a_reexport_equals_the_hand_built_definition() {
    let by_hand = Definition::new("search")
        .effects(Effects::ReadOnly)
        .param("query", Param::string().required())
        .param("mode", Mode::parameter())
        .param("limit", Param::array(Param::integer()).required());
    assert_eq!(Search::tool_definition().name(), "search");
    assert_eq!(Search::tool_definition(), by_hand);
    assert_eq!(
        Search::from_node(&parse(
            r#"{"query": "q", "mode": "careful", "limit": [1, 2]}"#
        )),
        Ok(Search {
            query: "q".into(),
            mode: Some(Mode::Careful),
            limit: vec![1, 2],
        })
    );
    let error =
        Search::from_node(&parse(r#"{"query": "q", "limit": [], "mode": "slow"}"#)).unwrap_err();
    assert_eq!(error.pointer, "/mode");
}

#[test]
fn an_enum_through_a_reexport_matches_its_values() {
    assert_eq!(Mode::parameter(), Param::enumeration(&["Fast", "careful"]));
    assert_eq!(Mode::from_node(&parse(r#""Fast""#)), Ok(Mode::Fast));
    assert!(Mode::from_node(&parse(r#""fast""#)).is_err());
}

#[test]
fn aliases_absolute_and_relative_paths_all_work() {
    assert_eq!(ViaAlias::tool_definition().name(), "via_alias");
    assert_eq!(
        ViaAlias::from_node(&parse(r#"{"path": "a"}"#)),
        Ok(ViaAlias { path: "a".into() })
    );
    assert_eq!(
        KindViaAlias::parameter(),
        Param::enumeration(&["A"]).description("A kind.")
    );
    assert_eq!(Absolute::tool_definition().name(), "absolute");
    assert_eq!(nested::ViaSuper::tool_definition().name(), "via_super");
    assert_eq!(nested::ViaSelf::tool_definition().name(), "via_self");
    assert_eq!(
        nested::ViaSelf::from_node(&parse(r#"{"depth": 3}"#)),
        Ok(nested::ViaSelf { depth: 3 })
    );
}

#[test]
fn overridden_types_nest_in_default_derived_tools() {
    let mixed = Mixed::from_node(&parse(
        r#"{"search": {"query": "q", "limit": []}, "kind": "A", "via_super": {"depth": 1}}"#,
    ))
    .unwrap();
    assert_eq!(mixed.kind, KindViaAlias::A);
    assert_eq!(mixed.via_super, nested::ViaSuper { depth: 1 });
    assert_eq!(Mixed::tool_definition().name(), "mixed");
}
