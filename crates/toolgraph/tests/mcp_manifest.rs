// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The MCP Extended manifest codec over the worked example in `docs/tool-graph/TOOLS.md`.

use purrassist_core::{Budget, Governor};
use purrassist_toolgraph::codec::mcp::{self, CodecError, EnvelopeProblem, ManifestJson};
use purrassist_toolgraph::{
    ContextCodecError, PrefixTable, TERM_NAMES, TermBindings, ToolGraph, Vocabulary,
    register_loss_profile,
};
use purrdf_core::loss::LossProfile;
use purrdf_rdf::SerializeGraph;

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../docs/tool-graph/TOOLS.md");
const GOLDEN: &str = include_str!("data/worked_manifest.json");

fn vocabulary() -> Vocabulary {
    Vocabulary::new(
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
    .expect("worked vocabulary")
}

/// The Turtle fence of the "End-to-end example" section, read from the document itself.
fn worked() -> String {
    let section = TOOLS
        .split("## End-to-end example")
        .nth(1)
        .expect("worked example section");
    let fence = section.split("```turtle\n").nth(1).expect("turtle fence");
    fence
        .split("\n```")
        .next()
        .expect("closed fence")
        .to_owned()
}

fn load(source: &str) -> ToolGraph {
    ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary(),
        &mut Governor::new(Budget::default()),
    )
    .expect("graph loads")
}

fn governor() -> Governor {
    Governor::new(Budget::default())
}

fn turtle(graph: &ToolGraph) -> Vec<u8> {
    purrdf_rdf::serialize_dataset(
        graph.dataset().as_ref(),
        "text/turtle",
        SerializeGraph::Dataset,
    )
    .expect("dataset serializes")
}

// Rationale: the acceptance criterion that the worked example's manifest bytes are
// fixed: two encodes are identical to each other and to the committed golden, which
// carries the #47 byte contract (`maxOutputBytes`, `exact-bytes`, format v2).
#[test]
fn worked_manifest_is_byte_identical_across_encodes_and_matches_the_golden() {
    let graph = load(&worked());
    let (first, losses) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let (second, _) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode again");
    assert_eq!(first, second);
    assert_eq!(first.as_str(), GOLDEN);
    assert!(losses.is_empty());
    for contract in [
        r#""format":"purrassist-toolgraph-v2""#,
        r#""maxOutputBytes":4096,"outputEnforcement":"exact-bytes""#,
        r#""lossProfile":"purrassist-toolgraph-v1""#,
    ] {
        assert!(first.as_str().contains(contract), "{contract}");
    }
    assert!(!first.as_str().contains("maxOutputTokens"));
}

// Rationale: the round-trip law. Decoding an encoded manifest returns the same typed
// tools and the same dataset (a serialization fixed point), and the same ledger.
#[test]
fn decode_of_encode_returns_the_same_graph_and_ledger() {
    let graph = load(&worked());
    let (manifest, encoded_losses) =
        mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let (decoded, decoded_losses) =
        mcp::decode(&manifest, &vocabulary(), &mut governor()).expect("decode");
    assert_eq!(decoded.tools(), graph.tools());
    assert_eq!(turtle(&decoded), turtle(&graph));
    assert_eq!(decoded_losses, encoded_losses);
    let (again, _) = mcp::encode(&decoded, &vocabulary(), &mut governor()).expect("re-encode");
    assert_eq!(again, manifest);
}

// Rationale: a schema constraint with no plain JSON Schema spelling is a ledgered loss
// that names its source shape, is registered in the caller's profile, and is the only
// difference the round trip may show; the retained source still holds the constraint.
#[test]
fn an_inexpressible_shape_constraint_is_ledgered_and_survives_in_the_source() {
    let from = "ex:pathShape a sh:PropertyShape ; sh:path ex:path ;";
    let source = worked();
    assert!(source.contains(from));
    let source = source.replacen(from, &format!("{from} sh:equals ex:text ;"), 1);
    let graph = load(&source);
    let vocabulary = vocabulary();
    let (manifest, losses) = mcp::encode(&graph, &vocabulary, &mut governor()).expect("encode");
    let [entry] = losses.entries() else {
        panic!("{:?}", losses.entries());
    };
    assert_eq!(entry.code, "sh:equals");
    assert_eq!(
        (entry.from.as_ref(), entry.to.as_ref()),
        ("shacl", "json-schema")
    );
    assert!(
        vocabulary
            .loss_profile()
            .description("shacl", "json-schema", &entry.code)
            .is_some()
    );
    assert!(manifest.as_str().contains("shacl#equals"));
    let (decoded, decoded_losses) =
        mcp::decode(&manifest, &vocabulary, &mut governor()).expect("decode");
    assert_eq!(decoded_losses, losses);
    assert_eq!(turtle(&decoded), turtle(&graph));
}

// Rationale: legacy wire forms are refused, never migrated or read as bytes: a v1
// bundle format, and a `maxOutputTokens` member, even beside the byte field.
#[test]
fn legacy_bundle_format_and_token_limit_are_rejected() {
    let graph = load(&worked());
    let (manifest, _) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let decode = |text: String| {
        mcp::decode(
            &ManifestJson::from_text(text),
            &vocabulary(),
            &mut governor(),
        )
        .expect_err("legacy form")
    };

    let legacy_format = manifest.as_str().replacen(
        r#""format":"purrassist-toolgraph-v2""#,
        r#""format":"purrassist-toolgraph-v1""#,
        1,
    );
    assert!(matches!(
        decode(legacy_format),
        CodecError::Envelope(error) if error.problem == EnvelopeProblem::UnsupportedFormat
    ));

    let tokens = manifest.as_str().replacen(
        r#""maxOutputBytes":4096,"#,
        r#""maxOutputBytes":4096,"maxOutputTokens":1024,"#,
        1,
    );
    assert!(matches!(
        decode(tokens),
        CodecError::Context {
            error: ContextCodecError::LegacyTokenLimit,
            ..
        }
    ));
}

// Rationale: typed metadata and the retained source are one authority; a declaration
// that disagrees with the graph is refused with the first disagreeing member.
#[test]
fn a_declaration_that_disagrees_with_the_source_graph_is_refused() {
    let graph = load(&worked());
    let (manifest, _) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let edited = manifest.as_str().replacen(
        r#""description":"Edit a workspace file.""#,
        r#""description":"Edit any file.""#,
        1,
    );
    assert_ne!(edited, manifest.as_str());
    let refused = mcp::decode(
        &ManifestJson::from_text(edited),
        &vocabulary(),
        &mut governor(),
    )
    .expect_err("disagreement");
    assert!(matches!(
        refused,
        CodecError::Disagreement {
            tool: Some(0),
            member: "description"
        }
    ));
}
