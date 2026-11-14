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
use purrdf_core::loss::LossProfile;

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
    // A complete statement parses; it is refused later, by ownership rather than by the parser.
    let parsed = load("<urn:a> <urn:b> <urn:c> .\n");
    assert!(
        matches!(parsed, Err(LoadError::UnsupportedConstruct { .. })),
        "{parsed:?}"
    );
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

// Rationale: a missing required property and an out-of-range value are shape
// violations naming the node and predicate.
#[test]
fn a_violated_vocabulary_shape_names_the_node_and_predicate() {
    let missing = load(&edited("pa:name \"edit_file\" ; ", "")).expect_err("name is required");
    let LoadError::InvalidShape {
        subject,
        predicate,
        violation,
    } = missing
    else {
        panic!("{missing:?}");
    };
    assert_eq!(subject, format!("{EX}edit_file"));
    assert_eq!(predicate, format!("{NAMESPACE}name"));
    assert_eq!(violation, Violation::Missing);
    let range = load(&edited("pa:maxOutputBytes 4096", "pa:maxOutputBytes 0"));
    assert!(
        matches!(
            range,
            Err(LoadError::InvalidShape {
                violation: Violation::OutOfRange { .. },
                ..
            })
        ),
        "{range:?}"
    );
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

// Rationale: a predicate outside the caller's vocabulary on an FnO-view subject is refused
// by the strict FnO reader as a statement; the refusal must name that subject and predicate
// rather than the vocabulary namespace.
#[test]
fn an_fno_view_statement_refusal_names_its_subject_and_predicate() {
    let source = edited(
        "pa:title \"Customer orders\"",
        "pa:title \"Customer orders\" ; ex:custom 1",
    );
    let error = load(&source).expect_err("non-vocabulary predicate on an FnO-view subject");
    let LoadError::InvalidFnoBinding {
        subject, predicate, ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}query_customer_orders"));
    assert_eq!(predicate.as_deref(), Some(&*format!("{EX}custom")));
    load(&worked()).expect("neighbouring valid input");
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
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}customerClaim"));
    assert_eq!(predicate, format!("{NAMESPACE}maxEditDistance"));
    assert_eq!(violation, Violation::Missing);
    // A reifier that annotates no parameter path is not silently ignored either.
    let orphan = load(&edited(
        "<< ex:pathShape sh:path ex:path ~ ex:pathClaim >> .",
        "<< ex:pathShape sh:path ex:text ~ ex:pathClaim >> .",
    ));
    assert!(
        matches!(orphan, Err(LoadError::InvalidHint { .. })),
        "{orphan:?}"
    );
    load(&worked()).expect("neighbouring valid input");
}

// Rationale: confinement fields outside the closed vocabulary are refused; the
// nested-profile cycle check keeps a self-referencing child from looping.
#[test]
fn an_invalid_execution_profile_is_refused() {
    let error = load(&edited(
        "pa:confinementMode \"change_hat\"",
        "pa:confinementMode \"unconfined\"",
    ))
    .expect_err("unknown confinement mode");
    let LoadError::InvalidExecutionProfile {
        subject,
        predicate,
        violation,
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}DatabaseProfile"));
    assert_eq!(predicate, format!("{NAMESPACE}confinementMode"));
    assert!(matches!(violation, Violation::OutOfRange { .. }));
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
// refused with the parser's byte offset rather than ignored.
#[test]
fn output_projection_outside_the_field_mask_subset_is_refused() {
    let error = load(&edited(
        "pa:outputProjectionJq \".orders\"",
        "pa:outputProjectionJq \".orders | length\"",
    ))
    .expect_err("pipe is not a field mask");
    let LoadError::UnsupportedProjection {
        subject,
        projection,
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(subject, format!("{EX}QueryContext"));
    assert_eq!(projection, ".orders | length");
    load(&edited(
        "pa:outputProjectionJq \".orders\"",
        "pa:outputProjectionJq \".orders[]?\"",
    ))
    .expect("an optional-elements mask is in the subset");
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
