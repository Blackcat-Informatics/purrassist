// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The MCP Extended manifest codec over the worked example in `docs/tool-graph/TOOLS.md`.

use purrassist_core::tool_call::ToolSet;
use purrassist_core::{Budget, Governor, json};
use purrassist_toolgraph::codec::mcp::{self, CodecError, EnvelopeProblem, ManifestJson};
use purrassist_toolgraph::{
    ContextCodecError, PrefixTable, TERM_NAMES, TermBindings, ToolGraph, Vocabulary,
    register_loss_profile,
};
use purrdf_core::loss::LossProfile;
use purrdf_core::{
    BlankScope, CanonicalizationControl, RDFC_V2_CORPUS_POLICY,
    blank_label::{LabelAlphabet, encode_blank_label},
    datasets_isomorphic,
};
use purrdf_rdf::SerializeGraph;
use purrdf_shapes::{engine, shapes};

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../docs/tool-graph/TOOLS.md");
const GOLDEN: &str = include_str!("data/worked_manifest.json");

fn vocabulary() -> Vocabulary {
    vocabulary_with_prefixes(vec![("pa".into(), NAMESPACE.into())])
}

fn vocabulary_with_prefixes(prefixes: Vec<(String, String)>) -> Vocabulary {
    Vocabulary::new(
        "pa",
        PrefixTable(prefixes),
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

#[test]
fn manifest_prefixes_must_match_each_caller_binding_once_in_any_order() {
    let vocabulary = vocabulary_with_prefixes(vec![
        ("pa".into(), NAMESPACE.into()),
        ("ex".into(), "urn:example:".into()),
    ]);
    let graph = ToolGraph::load(b"", "text/turtle", None, &vocabulary, &mut governor())
        .expect("empty graph");
    let (manifest, _) = mcp::encode(&graph, &vocabulary, &mut governor()).expect("manifest");
    let expected = r#""prefixes":{"ex":"urn:example:","pa":"https://example.org/toolgraph#"}"#;
    assert!(manifest.as_str().contains(expected));
    let replace =
        |prefixes| ManifestJson::from_text(manifest.as_str().replacen(expected, prefixes, 1));
    let reordered =
        replace(r#""prefixes":{"pa":"https://example.org/toolgraph#","ex":"urn:example:"}"#);
    mcp::decode(&reordered, &vocabulary, &mut governor())
        .expect("object member order is immaterial");
    for prefixes in [
        r#""prefixes":{"ex":"urn:example:","ex":"urn:example:"}"#,
        r#""prefixes":{"ex":"urn:wrong:","pa":"https://example.org/toolgraph#"}"#,
        r#""prefixes":{"ex":true,"pa":"https://example.org/toolgraph#"}"#,
    ] {
        assert!(
            matches!(
                mcp::decode(&replace(prefixes), &vocabulary, &mut governor()),
                Err(CodecError::Disagreement {
                    tool: None,
                    member: "prefixes"
                })
            ),
            "{prefixes}"
        );
    }
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

#[test]
fn declared_mapping_alias_keeps_the_authored_rdf_and_records_the_fno_projection() {
    let source = worked();
    let canonical = load(&source);
    assert!(canonical.fno_view_losses().is_empty());
    let alias_source = source.replacen(
        "_:queryMapping a fno:Mapping",
        "_:queryMapping a fnom:Mapping",
        1,
    ) + "\nfnom:Mapping a rdfs:Class ; owl:equivalentClass fno:Mapping ;\n\
           rdfs:comment \"Caller alias for the native mapping class.\" .\n";
    let graph = load(&alias_source);
    assert_eq!(graph.tools(), canonical.tools());
    assert_eq!(graph.catalog(), canonical.catalog());

    let ledger = graph.fno_view_losses();
    let [entry] = ledger.entries() else {
        panic!("{ledger:?}");
    };
    assert_eq!(entry.code, "fnom-mapping-type-normalized");
    assert_eq!(
        entry
            .location
            .as_ref()
            .and_then(|site| site.subject.as_deref()),
        Some("_:queryMapping")
    );
    let vocabulary = vocabulary();
    purrdf_core::loss::check_ledger_sound_in(
        vocabulary.loss_profile(),
        ledger,
        "rdf-1.2-toolgraph",
        "fno-admission-view",
    )
    .expect("registered and sound FnO view loss");

    let authored = purrdf_rdf::parse_dataset(alias_source.as_bytes(), "text/turtle", None)
        .expect("parse the authored RDF directly");
    let (manifest, mcp_losses) = mcp::encode(&graph, &vocabulary, &mut governor()).expect("encode");
    assert!(
        mcp_losses
            .entries()
            .iter()
            .all(|loss| loss.code != entry.code)
    );
    let (decoded, decoded_losses) =
        mcp::decode(&manifest, &vocabulary, &mut governor()).expect("decode");
    assert_eq!(decoded_losses, mcp_losses);
    assert_eq!(decoded.fno_view_losses(), ledger);
    assert_eq!(decoded.tools(), graph.tools());
    let mut control = CanonicalizationControl::new(RDFC_V2_CORPUS_POLICY, None);
    assert!(
        datasets_isomorphic(authored.as_ref(), decoded.dataset().as_ref(), &mut control)
            .expect("compare the authored RDF datasets")
    );
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

// Invariant: independent loads of the worked example produce the same manifest
// bytes and ledger. The golden carries the #47 byte contract.
#[test]
fn worked_manifest_is_byte_identical_across_loads_and_matches_the_golden() {
    let source = worked();
    let graph = load(&source);
    let (first, losses) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let second_graph = load(&source);
    let (second, second_losses) =
        mcp::encode(&second_graph, &vocabulary(), &mut governor()).expect("encode second load");
    assert_eq!(first, second);
    assert_eq!(losses, second_losses);
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
// tools, an isomorphic RDF dataset, and the same ledger. Re-encoding also
// reproduces the original manifest bytes.
#[test]
fn decode_of_encode_returns_the_same_graph_and_ledger() {
    let graph = load(&worked());
    let (manifest, encoded_losses) =
        mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let (decoded, decoded_losses) =
        mcp::decode(&manifest, &vocabulary(), &mut governor()).expect("decode");
    assert_eq!(decoded.tools(), graph.tools());
    let mut control = CanonicalizationControl::new(RDFC_V2_CORPUS_POLICY, None);
    assert!(
        datasets_isomorphic(
            graph.dataset().as_ref(),
            decoded.dataset().as_ref(),
            &mut control,
        )
        .expect("compare the worked RDF datasets"),
        "the round trip changed the worked RDF dataset"
    );
    assert_eq!(decoded_losses, encoded_losses);
    let (again, _) = mcp::encode(&decoded, &vocabulary(), &mut governor()).expect("re-encode");
    assert_eq!(again, manifest);
}

#[test]
fn manifest_recovery_hints_distinguish_blank_reifiers_with_the_same_local_label() {
    let scoped = encode_blank_label("claim", BlankScope(9), LabelAlphabet::BlankNodeLabel);
    let scoped = format!("_:{scoped}");
    let source = worked()
        .replace("ex:pathClaim", &scoped)
        .replace("ex:customerClaim", "_:claim");
    let graph = load(&source);
    let (manifest, losses) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let root = json::parse(manifest.as_str()).expect("manifest JSON");
    for (pointer, expected) in [
        ("/tools/0/_meta/security/recoveryHints/0", scoped.as_str()),
        ("/tools/1/_meta/security/recoveryHints/0", "_:claim"),
    ] {
        assert_eq!(
            root.pointer(pointer).and_then(json::Node::as_str),
            Some(expected)
        );
    }
    let (decoded, decoded_losses) =
        mcp::decode(&manifest, &vocabulary(), &mut governor()).expect("decode scoped hints");
    assert_eq!(decoded.tools(), graph.tools());
    assert_eq!(decoded_losses, losses);

    let ambiguous = ManifestJson::from_text(manifest.as_str().replacen(
        &format!(r#""recoveryHints":["{scoped}"]"#),
        r#""recoveryHints":["_:claim"]"#,
        1,
    ));
    assert!(matches!(
        mcp::decode(&ambiguous, &vocabulary(), &mut governor()),
        Err(CodecError::Disagreement {
            tool: Some(0),
            member: "_meta.security",
        })
    ));
}

// The projected schema admits both inputs. The retained SHACL shape must still
// reject the unequal values after decoding, and the ledger must explain the gap.
#[test]
fn an_inexpressible_shape_constraint_is_ledgered_and_survives_in_the_source() {
    let from = "ex:pathShape a sh:PropertyShape ; sh:path ex:path ;";
    let source = worked();
    assert!(source.contains(from));
    let mut source = source.replacen(from, &format!("{from} sh:equals ex:text ;"), 1);
    // Select the focus node without adding rdf:type to its closed argument shape.
    source.push_str("\nex:EditInput sh:targetSubjectsOf ex:path .\n");
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

    let shapes = shapes::from_dataset(decoded.dataset()).expect("decode original SHACL shapes");
    let tools = ToolSet::from_mcp(manifest.as_str(), Budget::default()).expect("adapt manifest");
    let schema = tools.tool("edit_file").expect("edit tool").schema();
    for (text, conforms) in [("notes.txt", true), ("replacement", false)] {
        let arguments =
            json::parse(&format!(r#"{{"path":"notes.txt","text":"{text}"}}"#)).expect("arguments");
        assert!(schema.validate(&arguments).expect("validate JSON").valid);

        let data = format!(
            r#"@prefix ex: <https://example.org/tools/> .
               ex:args ex:path "notes.txt" ; ex:text "{text}" ."#
        );
        let dataset = purrdf_rdf::parse_dataset(data.as_bytes(), "text/turtle", None)
            .expect("parse RDF arguments");
        let report = engine::validate_dataset(&dataset, &shapes).expect("validate RDF arguments");
        assert_eq!(report.conforms, conforms, "{:?}", report.results);
        assert_eq!(report.results.is_empty(), conforms);
        assert!(report.results.iter().all(|result| {
            result.source_constraint_component.as_str()
                == "http://www.w3.org/ns/shacl#EqualsConstraintComponent"
        }));
    }
}

// A manifest declaration must carry the exact byte contract before its
// retained graph can be used.
#[test]
fn invalid_manifest_output_contract_is_refused_before_graph_agreement() {
    let graph = load(&worked());
    let (manifest, _) = mcp::encode(&graph, &vocabulary(), &mut governor()).expect("encode");
    let decode = |text: String| {
        mcp::decode(
            &ManifestJson::from_text(text),
            &vocabulary(),
            &mut governor(),
        )
        .expect_err("invalid output contract")
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

    let query = graph
        .tools()
        .iter()
        .position(|tool| tool.name == "query_customer_orders")
        .expect("worked query declaration");
    let original = r#""maxOutputBytes":4096,"outputEnforcement":"exact-bytes","#;
    assert!(manifest.as_str().contains(original));
    for (replacement, expected) in [
        (
            r#""maxOutputBytes":4096,"maxOutputTokens":1024,"outputEnforcement":"exact-bytes","#,
            ContextCodecError::LegacyTokenLimit,
        ),
        (
            r#""maxOutputBytes":4096,"#,
            ContextCodecError::MissingField("outputEnforcement"),
        ),
        (
            r#""maxOutputBytes":4096,"outputEnforcement":"tokens","#,
            ContextCodecError::Enforcement,
        ),
        (
            r#""maxOutputBytes":4096,"outputEnforcement":"exact-bytes","outputEnforcement":"exact-bytes","#,
            ContextCodecError::DuplicateField("outputEnforcement"),
        ),
        (
            r#""maxOutputBytes":4096,"outputEnforcement":"exact-bytes","extra":true,"#,
            ContextCodecError::UnknownField { index: 3 },
        ),
        (
            r#""maxOutputBytes":0,"outputEnforcement":"exact-bytes","#,
            ContextCodecError::Quantity("maxOutputBytes"),
        ),
    ] {
        let error = decode(manifest.as_str().replacen(original, replacement, 1));
        assert!(
            matches!(&error, CodecError::Context { tool, error } if *tool == query && *error == expected),
            "{replacement}: {error:?}"
        );
    }
    let different_limit = r#""maxOutputBytes":4097,"outputEnforcement":"exact-bytes","#;
    assert!(matches!(
        decode(manifest.as_str().replacen(original, different_limit, 1)),
        CodecError::Disagreement {
            tool: Some(tool),
            member: "_meta.contextManagement",
        } if tool == query
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
