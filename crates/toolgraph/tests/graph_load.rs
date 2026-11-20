// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! `ToolGraph::load` over the worked example in `docs/tool-graph/TOOLS.md`.
//!
//! Each refusal test pairs one invalid input, made by a single textual edit of
//! the worked example, with the unedited example as its neighbouring valid input.

use purrassist_core::{
    Budget, CancellationToken, Effort, Governor, StopReason, contract::Resource,
};
use purrassist_toolgraph::codec::mcp::ExecutionMode;
use purrassist_toolgraph::{
    Coercion, Construct, LoadError, LoadingStrategy, PrefixTable, TERM_NAMES, TermBindings,
    ToolGraph, Violation, Vocabulary, register_loss_profile,
};
use purrdf_core::{
    BlankScope,
    blank_label::{LabelAlphabet, encode_blank_label},
    loss::LossProfile,
};

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../docs/tool-graph/TOOLS.md");

/// The caller-owned vocabulary of the worked example.
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

/// The loader charges one work unit per 64 native steps of the governed parser and
/// `FnO` reader, so the worked example loads under the Standard budget, which
/// `Budget::default()` is.
const WORKED_BUDGET: Budget = Effort::Standard.budget();

fn load(source: &str) -> Result<ToolGraph, LoadError> {
    ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary(),
        &mut Governor::new(WORKED_BUDGET),
    )
}

/// Edit the worked example once; the edit must change it so the test cannot go vacuous.
fn edited(from: &str, to: &str) -> String {
    let source = worked();
    assert!(source.contains(from), "edit target {from:?} is absent");
    source.replacen(from, to, 1)
}

const EX: &str = "https://example.org/tools/";

// Rationale: the acceptance criterion that the worked example loads to the graph the
// document describes: identity, ordered parameters, hints read from reifier rows,
// profile limits and the parsed field mask. Also the shared valid neighbour.
#[test]
fn worked_example_loads_to_the_graph_the_document_describes() {
    let graph = load(&worked()).expect("worked example loads");
    let names: Vec<&str> = graph
        .tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(names, ["edit_file", "query_customer_orders"]);
    assert!(graph.catalog().is_some());

    let query = graph.tool("query_customer_orders").expect("query tool");
    assert_eq!(query.iri, format!("{EX}query_customer_orders"));
    assert_eq!(query.annotations.title.as_deref(), Some("Customer orders"));
    assert_eq!(query.annotations.read_only_hint, Some(true));
    assert_eq!(query.annotations.destructive_hint, Some(false));
    assert_eq!(query.input_shape, format!("{EX}QueryInput"));
    assert_eq!(
        query.implementation.format,
        "application/vnd.purrassist.host-tool"
    );
    let parameters: Vec<(&str, &str, bool)> = query
        .parameters
        .iter()
        .map(|p| {
            (
                p.predicate.as_str(),
                p.implementation_property.as_str(),
                p.hint.is_some(),
            )
        })
        .collect();
    assert_eq!(
        parameters,
        [
            (&*format!("{EX}customer_id"), "customer_id", true),
            (&*format!("{EX}limit"), "limit", true)
        ]
    );
    let customer = query.parameters[0].hint.as_ref().expect("customer hint");
    assert_eq!(customer.reifier, format!("{EX}customerClaim"));
    assert_eq!(customer.aliases, ["customer"]);
    assert_eq!(customer.taints, ["personal-data"]);
    assert_eq!(
        customer.session_binding.as_deref(),
        Some(&*format!("{NAMESPACE}userId"))
    );
    assert_eq!(customer.max_edit_distance, 0);
    assert!(customer.coercions.is_empty() && !customer.case_fold && !customer.unicode_fold);
    let limit = query.parameters[1].hint.as_ref().expect("limit hint");
    assert_eq!(limit.coercions, [Coercion::StringToInteger]);
    assert_eq!(query.output.implementation_property, "orders");

    assert_eq!(query.context.output_projection().to_string(), ".orders");
    assert_eq!(query.context.output_limit().max_bytes(), 4096);
    assert_eq!(query.context.history_retention_turns(), 2);
    assert_eq!(query.context.intent_tags(), ["customer", "orders"]);
    assert_eq!(query.context.loading(), LoadingStrategy::OnDemand);

    let profile = &query.execution_profile;
    assert_eq!(profile.mode, ExecutionMode::ChangeHat);
    assert_eq!(
        profile.profile_name,
        "purrassist.tool.query_customer_orders"
    );
    assert!(profile.egress_allowed);
    assert_eq!(profile.limits.address_space_bytes, 268_435_456);
    assert_eq!(profile.limits.open_files, 64);
    assert_eq!(profile.rules.len(), 11);
    let network = profile
        .rules
        .iter()
        .find(|r| r.class == "NetworkRule")
        .expect("network rule");
    assert!(
        network
            .properties
            .contains(&("peerIp", "10.0.0.7".to_owned()))
    );

    let edit = graph.tool("edit_file").expect("edit tool");
    assert_eq!(edit.execution_profile.mode, ExecutionMode::AaExec);
    assert!(!edit.execution_profile.egress_allowed);
    assert_eq!(
        edit.parameters[0].hint.as_ref().expect("path hint").taints,
        ["untrusted-path"]
    );
    assert!(edit.parameters[1].hint.is_none());
    assert_eq!(edit.context.loading(), LoadingStrategy::Always);
}

// Rationale: `LoadError::Parse` must surface the parser's diagnostic and position.
#[test]
fn malformed_turtle_is_a_parse_error_with_its_position() {
    let broken = load("<urn:a> <urn:b> .\n").expect_err("incomplete statement");
    assert!(
        matches!(broken, LoadError::Parse { line: Some(1), .. }),
        "{broken:?}"
    );
    load(&worked()).expect("neighbouring valid input");
}

// Rationale: an unbound term in the caller's namespace is out of vocabulary, whether
// it is used as a predicate or a class; bound terms load.
#[test]
fn a_term_the_vocabulary_does_not_bind_is_refused_with_subject_and_predicate() {
    let source = edited(
        "pa:title \"Customer orders\"",
        "pa:titel \"Customer orders\"",
    );
    let error = load(&source).expect_err("unbound predicate");
    let LoadError::UnknownTerm {
        subject,
        predicate,
        term,
        at,
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}query_customer_orders"));
    assert_eq!(predicate, format!("{NAMESPACE}titel"));
    assert_eq!(term, predicate);
    // The occurrence is the predicate and object as written in the source, even
    // though the subject was inherited from an earlier `;`-separated clause.
    let at = at.expect("the governed parse retains the statement");
    assert_eq!(&source[at.start..at.end], "pa:titel \"Customer orders\"");
    assert_eq!(source[..at.start].matches('\n').count() as u64 + 1, at.line);
    let class = load(&edited(
        "ex:customerClaim a pa:Hint",
        "ex:customerClaim a pa:Hunt",
    ));
    assert!(
        matches!(class, Err(LoadError::UnknownTerm { .. })),
        "{class:?}"
    );
    load(&worked()).expect("neighbouring valid input");
}

#[test]
fn an_unbound_term_inside_a_reifier_locates_the_rejected_binding() {
    let statement = "<< ex:pathShape sh:path pa:Unbound ~ ex:pathClaim >>";
    let source = format!("{}\n{statement} .\n", worked());
    let error = load(&source).expect_err("unbound reified object");
    let LoadError::UnknownTerm {
        subject,
        predicate,
        term,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}pathClaim"));
    assert_eq!(predicate, "http://www.w3.org/ns/shacl#path");
    assert_eq!(term, format!("{NAMESPACE}Unbound"));
    assert_eq!(&source[at.start..at.end], statement);
}

// A refused value or misplaced predicate identifies a written statement.
// A missing required property has no such statement.
#[test]
fn a_violated_vocabulary_shape_names_the_node_and_predicate() {
    let missing = load(&edited("pa:name \"edit_file\" ; ", "")).expect_err("name is required");
    let LoadError::InvalidShape {
        subject,
        predicate,
        violation,
        at: None,
    } = missing
    else {
        panic!("{missing:?}");
    };
    assert_eq!(subject, format!("{EX}edit_file"));
    assert_eq!(predicate, format!("{NAMESPACE}name"));
    assert_eq!(violation, Violation::Missing);
    let written = "pa:maxOutputBytes 0";
    let source = edited("pa:maxOutputBytes 4096", written);
    let range = load(&source).expect_err("invalid byte limit");
    let LoadError::InvalidShape {
        violation: Violation::OutOfRange { .. },
        at: Some(at),
        ..
    } = range
    else {
        panic!("{range:?}")
    };
    assert_eq!(&source[at.start..at.end], written);

    let written = r#"pa:alias "misplaced""#;
    let source = format!("{}\nex:QueryContext {written} .\n", worked());
    let domain = load(&source).expect_err("hint predicate on query context");
    let LoadError::InvalidShape {
        violation: Violation::Domain { .. },
        at: Some(at),
        ..
    } = domain
    else {
        panic!("{domain:?}")
    };
    assert_eq!(&source[at.start..at.end], written);
    load(&worked()).expect("neighbouring valid input");
}

// Rationale: the strict upstream FnO reader and the callable-binding admission must
// both surface as `InvalidFnoBinding`; a parameter without a mapping is incomplete.
#[test]
fn an_incomplete_fno_binding_is_refused() {
    let error = load(&edited(
        "fno:parameterMapping _:customerMap, _:limitMap",
        "fno:parameterMapping _:customerMap",
    ))
    .expect_err("limit parameter has no mapping");
    assert!(
        matches!(error, LoadError::InvalidFnoBinding { .. }),
        "{error:?}"
    );
    load(&worked()).expect("neighbouring valid input");
}

#[test]
fn refused_mapping_alias_types_locate_the_authored_statement() {
    let alias = "a fnom:Mapping";
    let undeclared = edited(
        "_:queryMapping a fno:Mapping",
        "_:queryMapping a fnom:Mapping",
    );
    let invalid_subject = undeclared.replace("_:queryMapping", "ex:queryMapping")
        + "\nfnom:Mapping owl:equivalentClass fno:Mapping .\n";
    let wrong_target = format!("{undeclared}\nfnom:Mapping owl:equivalentClass fno:Function .\n");
    let (_, predicate, construct) =
        unsupported_construct(&wrong_target, "owl:equivalentClass fno:Function");
    assert_eq!(predicate, "http://www.w3.org/2002/07/owl#equivalentClass");
    assert_eq!(construct, Construct::UnownedStatement);
    for (source, expected) in [
        (undeclared, "requires its owl:equivalentClass declaration"),
        (invalid_subject, "requires Blank"),
    ] {
        let error = load(&source)
            .expect_err("the alias node requires a declaration and blank mapping subject");
        let LoadError::InvalidFnoBinding {
            predicate: Some(predicate),
            detail,
            at: Some(at),
            ..
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!(predicate, "http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
        assert_eq!(&source[at.start..at.end], alias);
        assert!(detail.contains(expected), "{detail}");
    }
}

// A refused statement may share its subject and predicate with valid rows.
// The locator follows the exact statement that the FnO reader declined.
#[test]
fn an_fno_view_statement_refusal_locates_the_exact_row() {
    let extra_type = "a fno:UnexpectedRole";
    let literal = r#"ex:custom "01"^^xsd:integer"#;
    for (source, written, predicate_iri) in [
        (
            format!("{}\nex:query_customer_orders {extra_type} .\n", worked()),
            extra_type,
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_owned(),
        ),
        (
            edited(
                r#"pa:title "Customer orders""#,
                &format!(r#"pa:title "Customer orders" ; {literal}"#),
            ),
            literal,
            format!("{EX}custom"),
        ),
    ] {
        let error = load(&source).expect_err("unclaimed FnO statement");
        let LoadError::InvalidFnoBinding {
            subject,
            predicate,
            at: Some(at),
            ..
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!(subject, format!("{EX}query_customer_orders"));
        assert_eq!(predicate.as_deref(), Some(predicate_iri.as_str()));
        assert_eq!(&source[at.start..at.end], written);
    }
    load(&worked()).expect("neighbouring valid input");
}

// The reader identifies the list head even when it cannot admit the count.
#[test]
fn an_empty_returns_list_is_located_at_its_written_clause() {
    let written = "fno:returns ()";
    let source = edited("fno:returns ( ex:ordersOutput )", written);
    let error = load(&source).expect_err("a callable must have one return");
    let LoadError::InvalidFnoBinding {
        subject,
        predicate,
        at: Some(at),
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}query_customer_orders"));
    assert_eq!(
        predicate.as_deref(),
        Some("https://w3id.org/function/ontology#returns")
    );
    assert_eq!(&source[at.start..at.end], written);
    load(&worked()).expect("neighbouring valid input");
}

// An absent parameter predicate has no statement to highlight.
#[test]
fn a_missing_callable_predicate_has_no_statement_extent() {
    let error = load(&edited("fno:predicate ex:limit ; ", ""))
        .expect_err("the mapped parameter requires a predicate");
    let LoadError::InvalidFnoBinding {
        at: None, detail, ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert!(detail.contains("requires an RDF predicate"), "{detail}");
    load(&worked()).expect("neighbouring valid input");
}

#[test]
fn a_conflicting_coercion_hint_points_to_the_written_none() {
    let written = r#"pa:typeCoercion "n\u006fne""#;
    let source = format!("{}\nex:limitClaim {written} .\n", worked());
    let error = load(&source).expect_err("none cannot accompany a conversion");
    let LoadError::InvalidHint {
        subject,
        predicate,
        at: Some(at),
        violation: Violation::OutOfRange { expected },
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}limitClaim"));
    assert_eq!(predicate, format!("{NAMESPACE}typeCoercion"));
    assert_eq!(expected, "none alone, or conversions without none");
    assert_eq!(&source[at.start..at.end], written);
    load(&worked()).expect("a conversion alone is valid");
}

// Rationale: a hint missing a required field is refused at the hint, while the
// complete hint of the worked example loads.
#[test]
fn an_incomplete_parameter_hint_is_refused_at_the_hint() {
    let error = load(&edited(
        "pa:maxEditDistance 0 ; pa:alias \"customer\"",
        "pa:alias \"customer\"",
    ))
    .expect_err("maxEditDistance is required");
    let LoadError::InvalidHint {
        subject,
        predicate,
        violation,
        at: None,
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}customerClaim"));
    assert_eq!(predicate, format!("{NAMESPACE}maxEditDistance"));
    assert_eq!(violation, Violation::Missing);
    let written = r#"pa:alias "customer"@en"#;
    let source = format!("{}\nex:customerClaim {written} .\n", worked());
    let wrong_kind = load(&source).expect_err("a hint alias requires a plain string");
    let LoadError::InvalidHint {
        violation: Violation::WrongKind { .. },
        at: Some(at),
        ..
    } = wrong_kind
    else {
        panic!("{wrong_kind:?}")
    };
    assert_eq!(&source[at.start..at.end], written);
    // A reifier that annotates no parameter path is not silently ignored either.
    let binding = "<< ex:pathShape sh:path ex:text ~ ex:pathClaim >>";
    let source = edited(
        "<< ex:pathShape sh:path ex:path ~ ex:pathClaim >> .",
        &format!("{binding} ."),
    );
    let error = load(&source).expect_err("the hint binds an unused path");
    let LoadError::InvalidHint {
        violation: Violation::Unresolved,
        at: Some(at),
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(&source[at.start..at.end], binding);
    load(&worked()).expect("neighbouring valid input");
}

#[test]
fn competing_reifiers_for_one_parameter_path_name_the_second_binding() {
    let binding = "<< ex:pathShape sh:path ex:path ~ ex:secondPathClaim >>";
    let source = format!(
        "{}\n{binding} .\n\
         ex:secondPathClaim a pa:Hint ; pa:maxEditDistance 0 ;\n\
         pa:typeCoercion \"none\" ; pa:caseFold false ; pa:unicodeFold false .",
        worked()
    );
    let error = load(&source).expect_err("a parameter path admits only one reifier");
    assert!(
        matches!(
            &error,
            LoadError::InvalidHint {
                subject,
                predicate,
                violation: Violation::Duplicate,
                at: Some(at),
            } if subject == &format!("{EX}secondPathClaim")
                && predicate == "http://www.w3.org/ns/shacl#path"
                && &source[at.start..at.end] == binding
        ),
        "{error:?}"
    );
}

// Rationale: confinement fields outside the closed vocabulary are refused; the
// nested-profile cycle check keeps a self-referencing child from looping.
#[test]
fn an_invalid_execution_profile_is_refused() {
    let written = r#"pa:confinementMode "unconfined""#;
    let source = edited(r#"pa:confinementMode "change_hat""#, written);
    let error = load(&source).expect_err("unknown confinement mode");
    let LoadError::InvalidExecutionProfile {
        subject,
        predicate,
        violation,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}DatabaseProfile"));
    assert_eq!(predicate, format!("{NAMESPACE}confinementMode"));
    assert!(matches!(violation, Violation::OutOfRange { .. }));
    assert_eq!(&source[at.start..at.end], written);

    // Multiplicity is reported before checking either value's kind.
    let duplicate = load(&edited(
        r#"pa:confinementMode "change_hat""#,
        r#"pa:confinementMode 42, "change_hat""#,
    ))
    .expect_err("a single-valued field has two values");
    assert!(matches!(
        duplicate,
        LoadError::InvalidExecutionProfile {
            violation: Violation::TooMany { found: 2 },
            at: None,
            ..
        }
    ));

    let written = "pa:deny false";
    let source = edited(
        r#"pa:mountOperation "mount" ; pa:deny true"#,
        &format!(r#"pa:mountOperation "mount" ; {written}"#),
    );
    let allowed_mount = load(&source).expect_err("mount rules require a denial");
    let LoadError::InvalidExecutionProfile {
        violation: Violation::OutOfRange { .. },
        at: Some(at),
        ..
    } = allowed_mount
    else {
        panic!("{allowed_mount:?}")
    };
    assert_eq!(&source[at.start..at.end], written);
    let cycle = load(&edited(
        "pa:egressAllowed true ;",
        "pa:egressAllowed true ; pa:subprofile ex:DatabaseProfile ;",
    ));
    assert!(
        matches!(
            cycle,
            Err(LoadError::InvalidExecutionProfile {
                violation: Violation::Cycle,
                ..
            })
        ),
        "{cycle:?}"
    );
    load(&worked()).expect("neighbouring valid input");
}

#[test]
fn file_permission_refusals_locate_the_written_property() {
    const PERMISSIONS: &str = r#"pa:filePermissions "rwkl""#;
    for (permissions, expected) in [
        (
            r"r\u003F",
            Violation::OutOfRange {
                expected: "file permissions drawn from r, w, a, k, l and m",
            },
        ),
        ("rr", Violation::Duplicate),
        (
            "wa",
            Violation::OutOfRange {
                expected: "file permissions without both write and append",
            },
        ),
    ] {
        let written = format!("pa:filePermissions \"{permissions}\"");
        let source = edited(PERMISSIONS, &written);
        let error = load(&source).expect_err("file permission set is invalid");
        let LoadError::InvalidExecutionProfile {
            subject,
            predicate,
            violation,
            at: Some(at),
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!(subject, format!("{EX}WorkspaceFile"));
        assert_eq!(predicate, format!("{NAMESPACE}filePermissions"));
        assert_eq!(&source[at.start..at.end], written);
        assert_eq!(violation, expected);
    }

    let graph = load(&edited(PERMISSIONS, r#"pa:filePermissions "wklr""#))
        .expect("the same valid permission set may use a different order");
    let edit = graph.tool("edit_file").expect("worked edit tool");
    let iri = format!("{EX}WorkspaceFile");
    let file = edit
        .execution_profile
        .rules
        .iter()
        .find(|rule| rule.iri == iri)
        .expect("worked file rule");
    assert!(
        file.properties
            .iter()
            .any(|(name, value)| *name == "filePermissions" && value == "wklr")
    );
}

#[test]
fn repeatable_intent_tags_have_no_fixed_cardinality_limit() {
    let tags: Vec<_> = (0..256).map(|index| format!("tag-{index:03}")).collect();
    let literals: Vec<_> = tags.iter().map(|tag| format!("\"{tag}\"")).collect();
    let source = edited(
        r#"pa:intentTags "customer", "orders""#,
        &format!("pa:intentTags {}", literals.join(", ")),
    );
    let graph = load(&source).expect("the governor admits all authored tags");
    let query = graph
        .tool("query_customer_orders")
        .expect("worked query tool");
    assert_eq!(query.context.intent_tags(), tags.as_slice());
}

// Rationale: nothing is skipped: a statement no view owns is refused, while a plain
// class declaration is an owned statement.
#[test]
fn a_statement_no_view_owns_is_refused_not_skipped() {
    let stray = format!("{}\n<urn:stray> <urn:stray-p> <urn:stray-o> .\n", worked());
    let error = load(&stray).expect_err("unowned statement");
    let LoadError::UnsupportedConstruct {
        subject,
        predicate,
        construct,
        at,
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        (subject.as_str(), predicate.as_str(), construct),
        ("urn:stray", "urn:stray-p", Construct::UnownedStatement)
    );
    // The occurrence is the exact statement text, on the line the stray was appended.
    let at = at.expect("the governed parse retains the statement");
    assert_eq!(&stray[at.start..at.end], "<urn:stray-p> <urn:stray-o>");
    assert_eq!(
        (at.line, at.column),
        (worked().matches('\n').count() as u64 + 2, 13)
    );
    let declared = format!("{}\n<urn:declared> a rdfs:Class .\n", worked());
    load(&declared).expect("a class declaration is owned");
}

#[test]
fn an_unowned_row_locates_its_scoped_blank_node() {
    let scoped = encode_blank_label("shape", BlankScope(9), LabelAlphabet::BlankNodeLabel);
    let clause = "rdfs:label \"same\"";
    let owned = format!("{}\n_:shape a sh:NodeShape ; {clause} .\n", worked());
    load(&owned).expect("the shape owns the earlier annotation");
    let source = format!("{owned}_:{scoped} {clause} .\n");
    let error = load(&source).expect_err("the other blank node has no owner");
    let LoadError::UnsupportedConstruct {
        subject,
        construct: Construct::UnownedStatement,
        at: Some(at),
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, "_:shape");
    assert_eq!(Some(at.start), source.rfind(clause));
    assert_eq!(&source[at.start..at.end], clause);
}

// Rationale: a vocabulary statement on a node with no concrete role (a bare shape, or
// a node typed only as the abstract Node class) has no property table to validate it,
// so it is refused rather than silently accepted; the unedited example loads.
#[test]
fn a_vocabulary_statement_on_a_node_without_a_concrete_role_is_refused() {
    load(&worked()).expect("the unedited example loads");
    let cases = [
        (
            "ex:QueryInput pa:name \"x\" .",
            "https://example.org/tools/QueryInput",
            NAMESPACE,
            "name",
        ),
        (
            "ex:EditInput pa:egressAllowed true .",
            "https://example.org/tools/EditInput",
            NAMESPACE,
            "egressAllowed",
        ),
        (
            "ex:marker a pa:Node .",
            "https://example.org/tools/marker",
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
            "type",
        ),
    ];
    for (statement, subject, namespace, local) in cases {
        let error = load(&format!("{}\n{statement}\n", worked())).expect_err(statement);
        let LoadError::UnsupportedConstruct {
            subject: found,
            predicate,
            construct,
            ..
        } = error
        else {
            panic!("{statement}: {error:?}");
        };
        assert_eq!(
            (found.as_str(), predicate, construct),
            (
                subject,
                format!("{namespace}{local}"),
                Construct::UnownedStatement
            ),
            "{statement}"
        );
    }
}

// Rationale: only the four-form field-mask subset is accepted; any other jq is
// refused with the decoded byte offset and the written RDF statement extent.
#[test]
fn output_projection_outside_the_field_mask_subset_is_refused() {
    let written = r#"pa:outputProjectionJq ".orders \u007C length""#;
    let source = edited("pa:outputProjectionJq \".orders\"", written);
    let error = load(&source).expect_err("pipe is not a field mask");
    let LoadError::UnsupportedProjection {
        subject,
        blank_scope: None,
        predicate,
        projection,
        error: purrassist_core::output::FieldMaskError::Unsupported { offset: 8 },
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}QueryContext"));
    assert_eq!(predicate, format!("{NAMESPACE}outputProjectionJq"));
    assert_eq!(projection, ".orders | length");
    assert_eq!(&source[at.start..at.end], written);
    load(&edited(
        "pa:outputProjectionJq \".orders\"",
        "pa:outputProjectionJq \".orders[]?\"",
    ))
    .expect("an optional-elements mask is in the subset");
}

// The rendered label can be equal for two blank nodes. The source extent
// still belongs to the native scope of the context with the invalid mask.
#[test]
fn a_projection_refusal_locates_the_correct_scoped_blank_context() {
    let scope = BlankScope(9);
    let scoped = encode_blank_label("context", scope, LabelAlphabet::BlankNodeLabel);
    let written = r#"pa:outputProjectionJq ".version | length""#;
    let source = worked()
        .replace("ex:QueryContext", "_:context")
        .replace("ex:EditContext", &format!("_:{scoped}"))
        .replacen("pa:outputProjectionJq \".version\"", written, 1);
    let error = load(&source).expect_err("invalid mask on the scoped context");
    let LoadError::UnsupportedProjection {
        subject,
        blank_scope: Some(found),
        at: Some(at),
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, "_:context");
    assert_eq!(found, scope);
    assert_eq!(&source[at.start..at.end], written);
    assert!(
        source[..at.start].contains(r#"_:context a pa:Context ; pa:outputProjectionJq ".orders""#)
    );
}

/// Shared subprofiles expand at each edge. Only an edge to an ancestor is a cycle.
#[test]
fn shared_profiles_preserve_structure_depth_and_cycle_precedence() {
    let mut source = edited(
        "pa:egressAllowed true ;",
        "pa:egressAllowed true ; pa:subprofile ex:SharedProfile, ex:FileProfile ;",
    );
    source.push_str(
        r#"
ex:FileProfile pa:subprofile ex:SharedProfile .
ex:SharedProfile a pa:ExecutionProfile ;
    pa:confinementMode "aa-exec" ; pa:profileName "shared" ; pa:hatName "shared" ;
    pa:egressAllowed false ; pa:addressSpaceBytes 1 ; pa:fileSizeBytes 1 ;
    pa:processCount 1 ; pa:cpuSeconds 1 ; pa:openFileCount 1 .
"#,
    );
    let vocabulary = vocabulary();
    let load = |source: &str, depth| {
        ToolGraph::load(
            source.as_bytes(),
            "text/turtle",
            None,
            &vocabulary,
            &mut Governor::new(Budget {
                depth,
                ..WORKED_BUDGET
            }),
        )
    };
    let graph = load(&source, 3).expect("the longest branch has three profiles");
    let query = graph.tool("query_customer_orders").unwrap();
    let [file, shared] = query.execution_profile.subprofiles.as_slice() else {
        panic!("the root keeps both children");
    };
    assert!(file.iri.ends_with("/FileProfile"));
    assert!(shared.iri.ends_with("/SharedProfile"));
    assert_eq!(file.subprofiles.as_slice(), std::slice::from_ref(shared));

    assert_eq!(
        load(&source, 2).unwrap_err(),
        LoadError::Exhausted(StopReason::BudgetExceeded(Resource::Depth))
    );

    source.push_str(
        "ex:SharedProfile pa:subprofile ex:DatabaseProfile .
",
    );
    // The loader builds edit_file first, so FileProfile is its active root.
    let cycle = load(&source, 3).unwrap_err();
    assert!(
        matches!(
            &cycle,
            LoadError::InvalidExecutionProfile {
                subject,
                violation: Violation::Cycle,
                ..
            } if subject == &format!("{EX}FileProfile")
        ),
        "{cycle:?}"
    );
}

/// Typed tools keep their scratch reservation while borrowed from the graph.
#[test]
fn a_graph_releases_typed_storage_after_drop_or_late_refusal() {
    let mut governor = Governor::new(WORKED_BUDGET);
    let meter = governor.scratch_meter();
    let baseline = 64;
    meter.charge(baseline).unwrap();
    let vocabulary = vocabulary();
    let graph = ToolGraph::load(
        worked().as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut governor,
    )
    .expect("worked graph");
    assert_eq!(graph.tools().len(), 2);
    assert!(meter.live() > baseline);
    let completed_work = governor.usage().work;
    drop(graph);
    assert_eq!(meter.live(), baseline);

    let written = r#"pa:name "edit\u005ffile""#;
    let duplicate = edited(r#"pa:name "query_customer_orders""#, written);
    assert!(matches!(
        ToolGraph::load(
            duplicate.as_bytes(),
            "text/turtle",
            None,
            &vocabulary,
            &mut governor,
        ),
        Err(LoadError::InvalidShape {
            subject,
            predicate,
            violation: Violation::Duplicate,
            at: Some(at),
        }) if subject == format!("{EX}query_customer_orders")
            && predicate == format!("{NAMESPACE}name")
            && &duplicate[at.start..at.end] == written
    ));
    assert_eq!(meter.live(), baseline);
    meter.release(baseline).unwrap();

    // The final charged work belongs to typing. A one-unit reduction refuses
    // after typed values have already been constructed.
    let mut limited = Governor::new(Budget {
        work: completed_work - 1,
        ..WORKED_BUDGET
    });
    let meter = limited.scratch_meter();
    meter.charge(baseline).unwrap();
    assert_eq!(
        ToolGraph::load(
            worked().as_bytes(),
            "text/turtle",
            None,
            &vocabulary,
            &mut limited,
        )
        .unwrap_err(),
        LoadError::Exhausted(StopReason::BudgetExceeded(Resource::Work))
    );
    assert_eq!(meter.live(), baseline);
    meter.release(baseline).unwrap();
}

/// A caller-namespace IRI is checked wherever it occurs in the RDF value.
#[test]
fn vocabulary_checks_literal_datatypes_nested_terms_and_reifier_ids() {
    const SH_HAS_VALUE: &str = "http://www.w3.org/ns/shacl#hasValue";
    const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";
    let original = worked();
    for (statement, unbound, predicate, written) in [
        (
            r#"ex:customerIdShape sh:hasValue "x"^^pa:UnboundDatatype ."#,
            "UnboundDatatype",
            SH_HAS_VALUE,
            r#"sh:hasValue "x"^^pa:UnboundDatatype"#,
        ),
        (
            "ex:customerIdShape sh:hasValue <<( ex:s ex:p <<( ex:s pa:UnboundNested ex:o )>> )>> .",
            "UnboundNested",
            SH_HAS_VALUE,
            "sh:hasValue <<( ex:s ex:p <<( ex:s pa:UnboundNested ex:o )>> )>>",
        ),
        (
            "<< ex:pathShape sh:path ex:path ~ pa:UnboundReifier >> .",
            "UnboundReifier",
            SH_PATH,
            "<< ex:pathShape sh:path ex:path ~ pa:UnboundReifier >>",
        ),
    ] {
        let source = format!("{original}\n{statement}\n");
        let error = load(&source).expect_err("unknown vocabulary term");
        let LoadError::UnknownTerm {
            subject,
            term,
            predicate: actual,
            at: Some(at),
            ..
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!(term, format!("{NAMESPACE}{unbound}"));
        assert_eq!(actual, predicate);
        if unbound == "UnboundReifier" {
            assert_eq!(subject, term);
        } else {
            assert_eq!(subject, format!("{EX}customerIdShape"));
        }
        assert_eq!(&source[at.start..at.end], written);
        let external = format!("ex:{unbound}");
        let replacement = if unbound == "UnboundReifier" {
            "ex:pathClaim"
        } else {
            &external
        };
        let valid = source.replace(&format!("pa:{unbound}"), replacement);
        load(&valid).expect("the neighbouring vocabulary is admitted");
    }
}

// Rationale: budget exhaustion and cancellation are explicit errors, never a partial
// graph; the same input loads under the Standard budget.
#[test]
fn exhausted_budget_and_cancellation_are_explicit() {
    let source = worked();
    let tight = Budget {
        work: 3,
        ..WORKED_BUDGET
    };
    let exhausted = ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary(),
        &mut Governor::new(tight),
    );
    assert_eq!(
        exhausted.unwrap_err(),
        LoadError::Exhausted(StopReason::BudgetExceeded(Resource::Work))
    );
    let token = CancellationToken::new();
    token.cancel();
    let mut governor = Governor::with_cancellation(WORKED_BUDGET, token);
    let cancelled = ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary(),
        &mut governor,
    );
    assert_eq!(cancelled.unwrap_err(), LoadError::Cancelled);
    load(&source).expect("neighbouring valid input");
}

// Rationale: the caller's work and scratch-storage limits bound the governed parse and
// the FnO catalog construction, not only this crate's own rows. A limit one below what
// the worked example measured refuses with the typed exhaustion error for that
// resource and publishes no graph; the measured limit itself loads.
#[test]
fn a_work_or_storage_limit_below_the_worked_example_refuses_without_a_graph() {
    let source = worked();
    let mut measured = Governor::new(WORKED_BUDGET);
    ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary(),
        &mut measured,
    )
    .expect("worked example loads under the Standard budget");
    let (work, scratch) = (measured.usage().work, measured.usage().peak_scratch_bytes);
    assert!(work > 0 && scratch > 0, "the load must be metered");
    let limited = |budget: Budget| {
        ToolGraph::load(
            source.as_bytes(),
            "text/turtle",
            None,
            &vocabulary(),
            &mut Governor::new(budget),
        )
    };
    for (resource, tight, loose) in [
        (
            Resource::Work,
            Budget {
                work: work - 1,
                ..WORKED_BUDGET
            },
            Budget {
                work,
                ..WORKED_BUDGET
            },
        ),
        (
            Resource::ScratchBytes,
            Budget {
                scratch_bytes: scratch - 1,
                ..WORKED_BUDGET
            },
            Budget {
                scratch_bytes: scratch,
                ..WORKED_BUDGET
            },
        ),
    ] {
        let refused = limited(tight);
        assert!(
            matches!(&refused, Err(LoadError::Exhausted(StopReason::BudgetExceeded(got))) if *got == resource),
            "{resource:?}: {refused:?}"
        );
        limited(loose)
            .unwrap_or_else(|error| panic!("{resource:?} at the measured limit: {error}"));
    }
}

// Rationale: a document far above what the Standard budget admits (several hundred
// kilobytes of Turtle at 64 native units per work unit) refuses with the typed work
// exhaustion and publishes no graph, rather than loading or being reported as
// invalid syntax. The document is well-formed Turtle, so only the budget refuses it.
#[test]
fn a_document_far_above_the_standard_budget_refuses_with_work_exhaustion() {
    let source = "<urn:s> <urn:p> <urn:o> .\n".repeat(40_000);
    assert!(
        source.len() > 1_000_000,
        "the document must exceed Standard"
    );
    let refused = ToolGraph::load(
        source.as_bytes(),
        "text/turtle",
        None,
        &vocabulary(),
        &mut Governor::new(Budget::default()),
    );
    assert_eq!(
        refused.unwrap_err(),
        LoadError::Exhausted(StopReason::BudgetExceeded(Resource::Work))
    );
}

// Rationale: only Turtle and TriG are parsed with exact source occurrences. TriG is
// accepted and gives the graph Turtle gives for the same default-graph statements;
// N-Triples and an unknown media type are refused with one stable parse code, never a
// partial graph and never a guess at the format.
#[test]
fn only_turtle_and_trig_media_types_are_loaded() {
    let source = worked();
    let load_as = |media_type: &str| {
        ToolGraph::load(
            source.as_bytes(),
            media_type,
            None,
            &vocabulary(),
            &mut Governor::new(WORKED_BUDGET),
        )
    };
    let turtle = load_as("text/turtle").expect("turtle");
    let trig = load_as("application/trig").expect("a default-graph TriG document loads");
    assert_eq!(trig.tools().len(), turtle.tools().len());
    let iris = |graph: &ToolGraph| -> Vec<String> {
        graph.tools().iter().map(|tool| tool.iri.clone()).collect()
    };
    assert_eq!(iris(&trig), iris(&turtle));
    for media_type in ["application/n-triples", "application/x-unknown"] {
        let LoadError::Parse { code, .. } = load_as(media_type).unwrap_err() else {
            panic!("{media_type} must be a parse refusal");
        };
        assert_eq!(code, "unsupported-format", "{media_type}");
    }
}

// Rationale: a TriG `GRAPH <g> {}` declares a named graph with no statement, so the
// statement walk sees nothing to refuse. The loader must still refuse it as the
// named-graph construct, never drop the graph it was given; the unedited worked
// example is the valid neighbour and loads.
#[test]
fn an_empty_named_graph_is_refused_not_dropped() {
    let load = |source: &str| {
        ToolGraph::load(
            source.as_bytes(),
            "application/trig",
            None,
            &vocabulary(),
            &mut Governor::new(WORKED_BUDGET),
        )
    };
    let source = worked();
    load(&source).expect("the unedited worked example loads as TriG");
    let source = format!("{source}\nGRAPH <urn:empty-graph> {{}}\n");
    let error = load(&source).unwrap_err();
    let LoadError::UnsupportedConstruct {
        subject,
        predicate,
        construct,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        (subject.as_str(), predicate.as_str(), construct),
        ("urn:empty-graph", "-", Construct::NamedGraph)
    );
    assert_eq!(&source[at.start..at.end], "<urn:empty-graph>");
}

/// Check the source occurrence and return its typed construct diagnostic.
fn unsupported_construct(source: &str, written: &str) -> (String, String, Construct) {
    let error = load(source).expect_err("unsupported construct");
    let LoadError::UnsupportedConstruct {
        subject,
        predicate,
        construct,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(&source[at.start..at.end], written);
    (subject, predicate, construct)
}

// Rationale: a statement written inside a TriG named graph carries a graph the tool
// graph has no place for, so it is refused as the named-graph construct rather than
// folded into the default graph; the unedited worked example loads as TriG.
#[test]
fn a_statement_in_a_named_graph_is_refused_as_a_named_graph() {
    let load_trig = |source: &str| {
        ToolGraph::load(
            source.as_bytes(),
            "application/trig",
            None,
            &vocabulary(),
            &mut Governor::new(WORKED_BUDGET),
        )
    };
    load_trig(&worked()).expect("the unedited worked example loads as TriG");
    let source = format!(
        "{}\nGRAPH <urn:named> {{ ex:EditResult a rdfs:Class . }}\n",
        worked()
    );
    let error = load_trig(&source).unwrap_err();
    let LoadError::UnsupportedConstruct {
        subject,
        predicate,
        construct,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(
        (subject.as_str(), predicate.as_str(), construct),
        (
            "https://example.org/tools/EditResult",
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
            Construct::NamedGraph
        )
    );
    assert_eq!(&source[at.start..at.end], "a rdfs:Class");
    assert_eq!(at.start, source.rfind("a rdfs:Class").unwrap());
}

#[test]
fn a_named_graph_reifier_refusal_points_to_its_source_occurrence() {
    let statement = "<< ex:EditResult rdfs:label \"x\" ~ ex:scopedReifier >>";
    let source = format!("{}\nGRAPH <urn:named> {{ {statement} . }}\n", worked());
    let error = ToolGraph::load(
        source.as_bytes(),
        "application/trig",
        None,
        &vocabulary(),
        &mut Governor::new(WORKED_BUDGET),
    )
    .unwrap_err();
    let LoadError::UnsupportedConstruct {
        subject,
        predicate,
        construct: Construct::NamedGraph,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, "https://example.org/tools/scopedReifier");
    assert_eq!(predicate, "-");
    assert_eq!(&source[at.start..at.end], statement);
    assert_eq!(at.start, source.rfind(statement).unwrap());
}

#[test]
fn a_named_graph_annotation_refusal_points_to_the_annotation() {
    let vocabulary = vocabulary();
    let annotation = "ex:scopedReifier pa:maxEditDistance 0";
    let source = format!(
        "{}\n{annotation} .\nGRAPH <urn:named> {{\n  \
         << ex:EditResult rdfs:label \"x\" ~ ex:scopedReifier >> .\n  {annotation} .\n}}",
        worked()
    );
    let error = ToolGraph::load(
        source.as_bytes(),
        "application/trig",
        None,
        &vocabulary,
        &mut Governor::new(WORKED_BUDGET),
    )
    .unwrap_err();
    let LoadError::UnsupportedConstruct {
        subject,
        predicate,
        construct: Construct::NamedGraph,
        at: Some(at),
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, "https://example.org/tools/scopedReifier");
    assert_eq!(Some(predicate.as_str()), vocabulary.term("maxEditDistance"));
    let spelling = "pa:maxEditDistance 0";
    assert_eq!(&source[at.start..at.end], spelling);
    assert_eq!(at.start, source.rfind(spelling).unwrap());
}

// Rationale: a reifier no tool parameter consumes and that is not a typed hint is
// refused as a reifier, never ignored; the unedited worked example loads.
#[test]
fn an_unconsumed_reifier_that_is_not_a_hint_is_refused_as_a_reifier() {
    load(&worked()).expect("neighbouring valid input");
    let source = edited(
        "ex:EditResult a rdfs:Class .",
        "ex:EditResult a rdfs:Class .\n<< ex:EditResult rdfs:label \"x\" ~ ex:plainReifier >> .\n\
         ex:plainReifier pa:maxEditDistance 0 ; pa:typeCoercion \"none\" ;\n \
         pa:caseFold false ; pa:unicodeFold false .",
    );
    let (subject, predicate, construct) = unsupported_construct(
        &source,
        r#"<< ex:EditResult rdfs:label "x" ~ ex:plainReifier >>"#,
    );
    assert_eq!(
        (subject.as_str(), predicate.as_str(), construct),
        (
            "https://example.org/tools/plainReifier",
            "http://www.w3.org/2000/01/rdf-schema#label",
            Construct::Reifier
        )
    );
}

// Rationale: a call-time fact (a session or call predicate, or a runtime class) is not
// a tool description, so it is refused as a runtime fact whichever form states it.
#[test]
fn a_call_time_fact_in_a_tool_graph_is_refused_as_a_runtime_fact() {
    load(&worked()).expect("neighbouring valid input");
    let predicate_form = edited(
        "ex:EditResult a rdfs:Class .",
        "ex:EditResult a rdfs:Class ; pa:userId \"customer-17\" .",
    );
    let (subject, predicate, construct) =
        unsupported_construct(&predicate_form, r#"pa:userId "customer-17""#);
    assert_eq!(
        (subject.as_str(), predicate, construct),
        (
            "https://example.org/tools/EditResult",
            format!("{NAMESPACE}userId"),
            Construct::RuntimeFact
        )
    );
    let class_form = edited(
        "ex:EditResult a rdfs:Class .",
        "ex:EditResult a rdfs:Class, pa:Session .",
    );
    let (_, _, construct) = unsupported_construct(&class_form, "a rdfs:Class, pa:Session");
    assert_eq!(construct, Construct::RuntimeFact);
}

// Rationale: an `fno:Function` that is not also a `pa:Tool` has no tool to describe it,
// so it is refused rather than dropped from the graph; the unedited example loads.
#[test]
fn a_function_without_a_tool_is_refused_as_a_function_without_a_tool() {
    load(&worked()).expect("neighbouring valid input");
    let source = edited(
        "ex:EditResult a rdfs:Class .",
        "ex:EditResult a rdfs:Class .\n\
         ex:orphanFunction a rdfs:Class ; a fno:Function ; rdfs:label \"orphan\" ; fno:expects ( ) ; fno:returns ( ex:orphanOutput ) .\n\
         ex:orphanOutput a fno:Output ; fno:predicate ex:orphan ; fno:type xsd:string .\n\
         _:orphanMapping a fno:Mapping ; rdfs:label \"orphan binding\" ; fno:function ex:orphanFunction ;\n \
         fno:implementation ex:editImplementation ; fno:returnMapping _:orphanReturn .\n\
         _:orphanReturn a fnom:DefaultReturnMapping ; rdfs:label \"orphan return\" ; fnom:functionOutput ex:orphanOutput ;\n \
         fnom:implementationProperty \"orphan\" .",
    );
    let (subject, predicate, construct) = unsupported_construct(&source, "a fno:Function");
    assert_eq!(
        (subject.as_str(), predicate, construct),
        (
            "https://example.org/tools/orphanFunction",
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type".to_owned(),
            Construct::FunctionWithoutTool
        )
    );
}

#[test]
fn callable_mapping_selection_requires_exactly_one_mapping_per_tool() {
    let source = worked();
    let mapping = source
        .lines()
        .find(|line| line.starts_with("_:queryMapping "))
        .expect("worked query mapping");
    let duplicate = format!(
        "{source}\n{}",
        mapping
            .replacen("_:queryMapping", "_:alternateQueryMapping", 1)
            .replace(
                "fno:function ex:query_customer_orders",
                "fno:function <https://example.org/tools/query_customer_orders>",
            )
    );
    let missing = source
        .lines()
        .filter(|line| {
            ![
                "_:queryMapping ",
                "_:customerMap ",
                "_:limitMap ",
                "_:ordersMap ",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .collect::<Vec<_>>()
        .join("\n");
    for (turtle, count) in [(missing, 0), (duplicate, 2)] {
        let error = load(&turtle).expect_err("each tool requires one FnO mapping");
        let LoadError::InvalidFnoBinding {
            subject,
            predicate,
            detail,
            at,
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!(subject, format!("{EX}query_customer_orders"));
        assert_eq!(
            detail,
            format!("a tool needs exactly one fno:Mapping, found {count}")
        );
        let written = at.map(|at| &turtle[at.start..at.end]);
        if count == 0 {
            assert_eq!(predicate, None);
            assert_eq!(written, None);
        } else {
            assert_eq!(
                predicate.as_deref(),
                Some("https://w3id.org/function/ontology#function")
            );
            assert_eq!(written, Some("fno:function ex:query_customer_orders"));
        }
    }
    let graph = load(&source).expect("one mapping per tool");
    assert_eq!(
        graph
            .tool("query_customer_orders")
            .unwrap()
            .implementation
            .iri,
        format!("{EX}queryImplementation")
    );
}

/// A required path must bind exactly one property shape, even when both use the same IRI.
#[test]
fn two_property_shapes_for_the_same_callable_path_are_refused() {
    let written = "sh:property ex:duplicateCustomerShape";
    let source = format!(
        "{}\nex:QueryInput {written} .\n\
         ex:duplicateCustomerShape a sh:PropertyShape ; sh:path ex:customer_id .\n",
        worked()
    );
    assert!(matches!(
        load(&source).unwrap_err(),
        LoadError::InvalidShape {
            subject,
            predicate,
            violation: Violation::Duplicate,
            at: Some(at),
        } if subject == format!("{EX}QueryInput")
            && predicate == "http://www.w3.org/ns/shacl#property"
            && &source[at.start..at.end] == written
    ));
}

#[test]
fn unbound_callable_paths_point_to_the_written_property_reference() {
    let written = "sh:property ex:unboundShape";
    for shape in ["QueryInput", "QueryOutput"] {
        let source = format!(
            "{}\nex:{shape} {written} .\n\
             ex:unboundShape a sh:PropertyShape ; sh:path ex:unexpected .\n",
            worked()
        );
        let error = load(&source).expect_err("each property path requires an FnO binding");
        assert!(
            matches!(
                &error,
                LoadError::InvalidShape {
                    subject,
                    predicate,
                    violation: Violation::Unresolved,
                    at: Some(at),
                } if subject == &format!("{EX}{shape}")
                    && predicate == "http://www.w3.org/ns/shacl#property"
                    && &source[at.start..at.end] == written
            ),
            "{error:?}"
        );
    }
    load(&worked()).expect("all property paths are bound");
}

#[test]
fn a_property_shape_requires_exactly_one_path_value() {
    let original = "sh:path ex:customer_id ;";
    for (replacement, expected) in [
        ("", Violation::Missing),
        (
            "sh:path ex:customer_id, ex:limit, ex:path ;",
            Violation::TooMany { found: 3 },
        ),
    ] {
        let written = "sh:property ex:customerIdShape";
        let source = edited(original, replacement).replace(
            "sh:property ex:customerIdShape, ex:limitShape",
            "sh:property ex:customerIdShape ; sh:property ex:limitShape",
        );
        let error = load(&source).expect_err("invalid path count");
        assert!(
            matches!(
                &error,
                LoadError::InvalidShape { subject, predicate, violation, at: Some(at) }
                    if subject == &format!("{EX}customerIdShape")
                        && predicate == "http://www.w3.org/ns/shacl#path"
                        && violation == &expected
                        && &source[at.start..at.end] == written
            ),
            "{error:?}"
        );
    }
    load(&worked()).expect("single path value");
}

#[test]
fn unresolved_shapes_point_to_the_written_tool_reference() {
    for (predicate, shape) in [("inputShape", "QueryInput"), ("outputShape", "QueryOutput")] {
        let written = format!("pa:{predicate} <https://example.org/tools/UndeclaredShape>");
        let source = edited(&format!("pa:{predicate} ex:{shape}"), &written);
        let error = load(&source).expect_err("shape reference must resolve");
        assert!(
            matches!(
                &error,
                LoadError::InvalidShape {
                    subject,
                    predicate: refused,
                    violation: Violation::Unresolved,
                    at: Some(at),
                } if subject == &format!("{EX}query_customer_orders")
                    && refused == &format!("{NAMESPACE}{predicate}")
                    && source[at.start..at.end] == written
            ),
            "{error:?}"
        );
    }
    load(&worked()).expect("declared shapes");
}

#[test]
fn invalid_subtool_targets_point_to_the_written_edge() {
    let input = "pa:inputShape ex:QueryInput";
    for (target, expected) in [
        ("ex:aaa_undefined", Violation::Unresolved),
        ("_:missing", Violation::WrongKind { expected: "IRI" }),
        (
            r#"[ rdfs:label "unnamed" ]"#,
            Violation::WrongKind { expected: "IRI" },
        ),
        (r#""edit_file""#, Violation::WrongKind { expected: "IRI" }),
    ] {
        let written = format!("pa:subtool {target}");
        let source = edited(input, &format!("{written} ; {input}"));
        // The native occurrence for an inline blank node ends at the opening token.
        let expected_extent = if target.starts_with('[') {
            "pa:subtool ["
        } else {
            &written
        };
        let error = load(&source).expect_err("a subtool needs a defined tool IRI");
        assert!(
            matches!(
                &error,
                LoadError::InvalidShape { subject, predicate, violation, at: Some(at) }
                    if subject == &format!("{EX}query_customer_orders")
                        && predicate == &format!("{NAMESPACE}subtool")
                        && violation == &expected
                        && &source[at.start..at.end] == expected_extent
            ),
            "{error:?}"
        );
    }
    let valid = edited(input, &format!("pa:subtool ex:edit_file ; {input}"));
    let graph = load(&valid).expect("defined subtool");
    assert_eq!(
        graph.tool("query_customer_orders").unwrap().subtools,
        [format!("{EX}edit_file")]
    );
}

// Rationale: only a plain predicate IRI is a parameter path; a `sh:path` that is a
// blank-node path expression is refused as a complex path, never read as an IRI.
#[test]
fn a_non_iri_shape_path_is_refused_as_a_complex_path() {
    load(&worked()).expect("neighbouring valid input");
    let source = edited(
        "ex:customerIdShape a sh:PropertyShape ; sh:path ex:customer_id ;",
        "ex:customerIdShape a sh:PropertyShape ; sh:path [] ;",
    );
    let (subject, predicate, construct) = unsupported_construct(&source, "sh:path []");
    assert_eq!(
        (subject.as_str(), predicate.as_str(), construct),
        (
            "https://example.org/tools/customerIdShape",
            "http://www.w3.org/ns/shacl#path",
            Construct::ComplexPath
        )
    );
}
