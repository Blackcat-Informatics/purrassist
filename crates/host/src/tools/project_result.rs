// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! One host adapter for the core's validated JSON Lines artifact.
use super::validate::{report_node as validation, resource};
use purrassist::{
    CancellationToken, Governor, Receipt, StopReason,
    json::{self, Node, Value},
    output::{
        ContextCodecError, ContextManagement, FieldMaskError, NullSelection, OutputLimitError,
        OutputMeasurement, OutputTransform, ProjectionFailure, ProjectionStage, ProjectionTrace,
    },
    schema::{CompilationError, Schema},
    wire::{array, integer, null, object, string},
};

/// Wire materialization follows the existing host policy, outside the measured
/// artifact. In particular, it may report a native stop after its governor stops.
struct Reply<'a> {
    args: &'a Node,
    raw: &'a str,
    stage: &'static str,
    failure: Option<Node>,
    artifact: Option<Node>,
    validation: Option<Node>,
    transformations: Node,
    receipt: Receipt,
}

pub(super) fn call(args: &Node, cancellation: Option<&CancellationToken>) -> Result<Node, String> {
    let effort = super::effort(args)?;
    let mut governor = super::governor(effort.budget(), cancellation);
    process(args, &mut governor)
}

fn process(args: &Node, governor: &mut Governor) -> Result<Node, String> {
    // These are invocation requirements. Registry schema validation normally
    // establishes them before this handler; the direct Rust entry checks too.
    let raw = super::field(args, "result_json")?;
    let format = super::field(args, "format")?;
    let control = args.get("context").ok_or("missing context")?;
    let schema = args.get("output_schema").ok_or("missing output_schema")?;
    let effort = super::effort(args)?;
    let mut reply = Reply {
        args,
        raw,
        stage: "context",
        failure: None,
        artifact: None,
        validation: None,
        transformations: array([]),
        receipt: Receipt::new(
            raw.as_bytes(),
            "output.project-result.v1",
            effort,
            *governor.limits(),
        ),
    };
    let context = match ContextManagement::decode_mcp(Some(format), control, governor) {
        Ok(context) => context,
        Err(error) => {
            let (failure, stop) = context_error(error);
            return Ok(reply.refused(failure, stop, governor));
        }
    };
    reply.stage = "schema_serialization";
    let schema_source = match schema.to_json_governed(governor) {
        Ok(source) => source,
        Err(reason) => {
            return Ok(reply.refused(resource(&reason, "schema_serialization"), reason, governor));
        }
    };
    reply.stage = "schema_compilation";
    let schema = match Schema::compile_governed(&schema_source, governor) {
        Ok(schema) => schema,
        Err(CompilationError::Resource(reason)) => {
            return Ok(reply.refused(resource(&reason, "schema_compilation"), reason, governor));
        }
        Err(CompilationError::InvalidSchema(error)) => {
            let failure = object([
                ("kind", string("schema_compilation")),
                ("location", string(error.location)),
                ("message", string(error.message)),
            ]);
            return Ok(reply.refused(
                failure,
                StopReason::InvalidInput("invalid output schema".into()),
                governor,
            ));
        }
    };
    reply.stage = "result_parsing";
    let original = match json::parse_with(raw, governor) {
        Ok(original) => original,
        Err(json::ParseFailure::Resource(reason)) => {
            return Ok(reply.refused(resource(&reason, "result_parsing"), reason, governor));
        }
        Err(json::ParseFailure::Syntax(error)) => {
            let failure = object([
                ("kind", string("json_syntax")),
                ("offset", count(error.offset)),
                ("message", string(error.message)),
            ]);
            return Ok(reply.refused(
                failure,
                StopReason::InvalidInput("invalid result JSON".into()),
                governor,
            ));
        }
    };
    match context.project_result(&schema, &original, governor) {
        Ok(artifact) => {
            reply.trace(&artifact.trace);
            reply.artifact = Some(object([
                ("media_type", string("application/x-ndjson")),
                ("text", string(artifact.bytes.as_str())),
                ("measurement", measurement(artifact.measurement)),
            ]));
            Ok(reply.finish(governor))
        }
        Err(error) => {
            reply.trace(&error.trace);
            let (failure, stop) = projection_error(error.reason);
            Ok(reply.refused(failure, stop, governor))
        }
    }
}

impl Reply<'_> {
    fn trace(&mut self, trace: &ProjectionTrace<'_>) {
        self.stage = match trace.stage {
            ProjectionStage::Validation => "validation",
            ProjectionStage::Selection => "selection",
            ProjectionStage::Serialization => "serialization",
            ProjectionStage::ByteLimit => "byte_limit",
            ProjectionStage::Complete => "complete",
        };
        self.validation = trace.validation.as_ref().map(validation);
        self.transformations = array(trace.transformations.iter().map(transformation));
    }

    fn refused(mut self, failure: Node, stop: StopReason, governor: &Governor) -> Node {
        self.failure = Some(failure);
        self.receipt.stopped = stop;
        self.finish(governor)
    }

    fn finish(mut self, governor: &Governor) -> Node {
        self.receipt.usage = governor.usage();
        // Preserve every requested mask occurrence, without choosing a duplicate.
        // Context JSON is additionally a string so nested duplicates survive
        // clients which collapse duplicate keys in their ordinary JSON DOM.
        // process checked these required fields before constructing Reply;
        // args remains immutably borrowed for this reply's entire lifetime.
        let context = self
            .args
            .get("context")
            .expect("invocation admitted context");
        let masks = match &context.value {
            Value::Object(members) => array(
                members
                    .iter()
                    .filter(|member| member.key == "outputProjectionJq")
                    .map(|member| member.value.clone()),
            ),
            _ => array([]),
        };
        object([
            (
                "status",
                string(if self.failure.is_none() {
                    "resolved"
                } else {
                    "unresolved"
                }),
            ),
            ("original_json", string(self.raw)),
            (
                "requested",
                object([
                    (
                        "format",
                        self.args
                            .get("format")
                            .cloned()
                            .expect("invocation admitted format"),
                    ),
                    ("context_json", string(context.to_json())),
                    // This is requested-control wire retention, not the governed
                    // schema source used by compilation above.
                    (
                        "output_schema_json",
                        string(
                            self.args
                                .get("output_schema")
                                .expect("invocation admitted schema")
                                .to_json(),
                        ),
                    ),
                    (
                        "effort",
                        self.args.get("effort").cloned().unwrap_or_else(null),
                    ),
                    ("masks", masks),
                ]),
            ),
            ("stage", string(self.stage)),
            ("error", self.failure.unwrap_or_else(null)),
            ("validation", self.validation.unwrap_or_else(null)),
            ("transformations", self.transformations),
            ("artifact", self.artifact.unwrap_or_else(null)),
            ("receipt", self.receipt.to_node()),
        ])
    }
}

fn context_error(error: ContextCodecError) -> (Node, StopReason) {
    let detail = match error {
        ContextCodecError::Resource(reason) => return (resource(&reason, "context"), reason),
        ContextCodecError::Projection(FieldMaskError::Resource(reason)) => {
            return (resource(&reason, "mask"), reason);
        }
        ContextCodecError::Projection(FieldMaskError::Unsupported { offset }) => {
            object([("kind", string("mask_syntax")), ("offset", count(offset))])
        }
        ContextCodecError::Projection(FieldMaskError::DuplicateField { offset }) => object([
            ("kind", string("mask_duplicate_field")),
            ("offset", count(offset)),
        ]),
        ContextCodecError::UnknownField { index } => object([
            ("kind", string("context_unknown_field")),
            ("index", count(index)),
        ]),
        ContextCodecError::TagType { index } => object([
            ("kind", string("context_tag_type")),
            ("index", count(index)),
        ]),
        ContextCodecError::DuplicateField(field) => named_error("context_duplicate_field", field),
        ContextCodecError::MissingField(field) => named_error("context_missing_field", field),
        ContextCodecError::WrongType(field) => named_error("context_wrong_type", field),
        ContextCodecError::Quantity(field) => named_error("context_quantity", field),
        ContextCodecError::MissingFormat => kind("context_missing_format"),
        ContextCodecError::UnsupportedFormat => kind("context_unsupported_format"),
        ContextCodecError::LegacyTokenLimit => kind("context_legacy_token_limit"),
        ContextCodecError::Enforcement => kind("context_enforcement"),
        ContextCodecError::Loading => kind("context_loading"),
    };
    (
        detail,
        StopReason::InvalidInput("invalid result context".into()),
    )
}

fn projection_error(error: ProjectionFailure<'_>) -> (Node, StopReason) {
    let failure = match error {
        ProjectionFailure::Resource(reason) => return (resource(&reason, "projection"), reason),
        ProjectionFailure::OutputLimit(OutputLimitError::Resource(reason)) => {
            return (resource(&reason, "output_limit"), reason);
        }
        ProjectionFailure::OutputLimit(OutputLimitError::Exceeded(value)) => object([
            ("kind", string("output_limit")),
            ("measurement", measurement(value)),
        ]),
        ProjectionFailure::OutputLimit(OutputLimitError::ZeroLimit) => kind("output_zero_limit"),
        ProjectionFailure::Schema => kind("schema_rejected"),
        ProjectionFailure::WrongType { field } => object([
            ("kind", string("projection_wrong_type")),
            ("field", field.map_or_else(null, string)),
        ]),
        ProjectionFailure::AmbiguousMember { field } => {
            named_error("projection_ambiguous_member", field)
        }
    };
    (
        failure,
        StopReason::InvalidInput("result projection refused".into()),
    )
}

fn kind(name: &str) -> Node {
    object([("kind", string(name))])
}
fn named_error(name: &str, field: &str) -> Node {
    object([("kind", string(name)), ("field", string(field))])
}
fn count(value: usize) -> Node {
    integer(u64::try_from(value).unwrap_or(u64::MAX))
}
fn measurement(value: OutputMeasurement) -> Node {
    object([
        ("limit", integer(value.limit)),
        ("measured", integer(value.measured)),
        ("unit", string("bytes")),
        ("enforcement", string("exact-bytes")),
    ])
}
fn transformation(value: &OutputTransform<'_>) -> Node {
    match value {
        OutputTransform::MaskApplied => kind("mask_applied"),
        OutputTransform::NullInserted {
            field,
            index,
            cause,
        } => object([
            ("kind", string("null_inserted")),
            ("field", string(*field)),
            ("index", count(*index)),
            (
                "cause",
                string(match cause {
                    NullSelection::Missing => "missing",
                    NullSelection::Propagated => "propagated",
                }),
            ),
        ]),
        OutputTransform::OptionalOmitted { field } => named_error("optional_omitted", field),
    }
}
// These schemas describe protocol materialization, not the measured artifact.
pub(super) const INPUT_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["result_json","output_schema","format","context"],"properties":{"result_json":{"type":"string"},"output_schema":{"type":["object","boolean"]},"format":{"type":"string"},"context":{"type":"object"},"effort":{"enum":["exact","quick","standard","deep","maximum"]}}}"#;
pub(super) const OUTPUT_SCHEMA: &str = r#"{"type":"object","additionalProperties":false,"required":["status","original_json","requested","stage","error","validation","transformations","artifact","receipt"],"properties":{"status":{"enum":["resolved","unresolved"]},"original_json":{"type":"string"},"requested":{"type":"object","additionalProperties":false,"required":["format","context_json","masks","output_schema_json","effort"],"properties":{"format":{"type":"string"},"context_json":{"type":"string"},"masks":{"type":"array"},"output_schema_json":{"type":"string"},"effort":{"type":["string","null"]}}},"stage":{"enum":["context","schema_serialization","schema_compilation","result_parsing","validation","selection","serialization","byte_limit","complete"]},"error":{"oneOf":[{"type":"null"},{"oneOf":[{"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"enum":["context_missing_format","context_unsupported_format","context_legacy_token_limit","context_enforcement","context_loading","schema_rejected","output_zero_limit"]}}},{"type":"object","additionalProperties":false,"required":["kind","field"],"properties":{"kind":{"enum":["context_duplicate_field","context_missing_field","context_wrong_type","context_quantity","projection_ambiguous_member"]},"field":{"type":"string"}}},{"type":"object","additionalProperties":false,"required":["kind","index"],"properties":{"kind":{"enum":["context_unknown_field","context_tag_type"]},"index":{"type":"integer","minimum":0}}},{"type":"object","additionalProperties":false,"required":["kind","offset"],"properties":{"kind":{"enum":["mask_syntax","mask_duplicate_field"]},"offset":{"type":"integer","minimum":0}}},{"type":"object","additionalProperties":false,"required":["kind","offset","message"],"properties":{"kind":{"enum":["json_syntax"]},"offset":{"type":"integer","minimum":0},"message":{"type":"string"}}},{"type":"object","additionalProperties":false,"required":["kind","location","message"],"properties":{"kind":{"enum":["schema_compilation"]},"location":{"type":"string"},"message":{"type":"string"}}},{"type":"object","additionalProperties":false,"required":["kind","field"],"properties":{"kind":{"enum":["projection_wrong_type"]},"field":{"oneOf":[{"type":"null"},{"type":"string"}]}}},{"type":"object","additionalProperties":false,"required":["kind","measurement"],"properties":{"kind":{"enum":["output_limit"]},"measurement":{"type":"object","additionalProperties":false,"required":["limit","measured","unit","enforcement"],"properties":{"limit":{"type":"integer","minimum":1},"measured":{"type":"integer","minimum":0},"unit":{"const":"bytes"},"enforcement":{"const":"exact-bytes"}}}}},{"type":"object","additionalProperties":false,"required":["kind","stop","detail","origin"],"properties":{"kind":{"enum":["resource"]},"stop":{"enum":["completed","no_candidate","budget_exceeded","cancelled","invalid_input"]},"detail":{"type":"string"},"origin":{"enum":["context","mask","schema_serialization","schema_compilation","result_parsing","projection","output_limit"]}}}]}]},"validation":{"oneOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["schema","dialect","valid","violations","format_evidence","pattern_evidence"],"properties":{"schema":{"type":"string"},"dialect":{"type":"string"},"valid":{"type":"boolean"},"violations":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["instance_path","schema_path","keyword","message"],"properties":{"instance_path":{"type":"string"},"schema_path":{"type":"string"},"keyword":{"type":"string"},"message":{"type":"string"}}}},"format_evidence":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["validator","constraint","passed","diagnostics"],"properties":{"validator":{"type":"string"},"constraint":{"type":"string"},"passed":{"type":"boolean"},"diagnostics":{"type":"array","items":{"type":"string"}}}}},"pattern_evidence":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["rule","explanation","destination","span"],"properties":{"rule":{"type":"string"},"explanation":{"type":"string"},"destination":{"type":"string"},"span":{"oneOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["source","start","end"],"properties":{"source":{"type":"string"},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0}}}]}}}}}}]},"transformations":{"type":"array","items":{"oneOf":[{"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"mask_applied"}}},{"type":"object","additionalProperties":false,"required":["kind","field","index","cause"],"properties":{"kind":{"const":"null_inserted"},"field":{"type":"string"},"index":{"type":"integer","minimum":0},"cause":{"enum":["missing","propagated"]}}},{"type":"object","additionalProperties":false,"required":["kind","field"],"properties":{"kind":{"const":"optional_omitted"},"field":{"type":"string"}}}]}},"artifact":{"oneOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["media_type","text","measurement"],"properties":{"media_type":{"const":"application/x-ndjson"},"text":{"type":"string"},"measurement":{"type":"object","additionalProperties":false,"required":["limit","measured","unit","enforcement"],"properties":{"limit":{"type":"integer","minimum":1},"measured":{"type":"integer","minimum":0},"unit":{"const":"bytes"},"enforcement":{"const":"exact-bytes"}}}}}]},"receipt":{"type":"object","required":["version","source","sources","alternatives","ambiguity","operation","effort","budget","usage","evidence","losses","inferences","validations","termination"],"properties":{"version":{"type":"integer","minimum":0},"source":{"type":"string"},"sources":{"type":"array"},"alternatives":{"type":"array"},"ambiguity":true,"operation":{"const":"output.project-result.v1"},"effort":{"enum":["exact","quick","standard","deep","maximum"]},"budget":{"type":"object"},"usage":{"type":"object"},"evidence":{"type":"array"},"losses":{"type":"array"},"inferences":{"type":"array"},"validations":{"type":"array"},"termination":{"type":"object","additionalProperties":false,"required":["kind","detail"],"properties":{"kind":{"enum":["completed","no_candidate","budget_exceeded","cancelled","invalid_input"]},"detail":{"type":"string"}}}}}}}"#;

#[cfg(test)]
mod tests {
    use super::*;
    use purrassist::{Budget, CancellationToken, Effort};

    fn request(raw: &str, mask: &str, bound: u64) -> Node {
        object([
            ("result_json", string(raw)),
            ("output_schema", Node::new(Value::Bool(true))),
            ("format", string("purrassist-toolgraph-v2")),
            (
                "context",
                object([
                    ("outputProjectionJq", string(mask)),
                    ("maxOutputBytes", integer(bound)),
                    ("outputEnforcement", string("exact-bytes")),
                    ("historyRetentionTurns", integer(2)),
                    ("intentTags", array([string("orders")])),
                    ("loadingStrategy", string("on_demand")),
                ]),
            ),
        ])
    }
    fn text<'a>(reply: &'a Node, path: &str) -> &'a str {
        reply
            .pointer(path)
            .and_then(Node::as_str)
            .expect("string in structured reply")
    }
    fn refusal(reply: &Node, stage: &str, kind: &str, raw: &str) {
        assert_eq!(text(reply, "/status"), "unresolved");
        assert_eq!(text(reply, "/stage"), stage);
        assert_eq!(text(reply, "/error/kind"), kind);
        assert_eq!(text(reply, "/original_json"), raw);
        assert!(matches!(reply.get("artifact").unwrap().value, Value::Null));
    }

    #[test]
    fn host_masks_preserve_raw_source_and_measure_only_the_checked_artifact() {
        for (raw, mask, expected) in [
            (
                r#" {"a":{"b":1.00e2},"untouched":true} "#,
                ".a.b",
                "1.00e2\n",
            ),
            (r#"["é\n",true]"#, ".[]", "\"é\\n\"\ntrue\n"),
            (r#"{"a":1,"b":2}"#, "{b,a}", "{\"b\":2,\"a\":1}\n"),
            (r#"{"a":[1.0,false]}"#, ".a[]?", "1.0\nfalse\n"),
            (r#"{"a":false}"#, ".a[]?", ""),
            (r#"{"a":{"x":1,"x":2}}"#, ".a", "{\"x\":1,\"x\":2}\n"),
        ] {
            let bound = u64::try_from(expected.len()).unwrap().max(1);
            let reply = call(&request(raw, mask, bound), None).unwrap();
            assert_eq!(text(&reply, "/status"), "resolved");
            assert_eq!(text(&reply, "/original_json"), raw);
            assert_eq!(text(&reply, "/artifact/text"), expected);
            assert_eq!(
                reply
                    .pointer("/artifact/measurement/measured")
                    .unwrap()
                    .to_json(),
                expected.len().to_string()
            );
            assert!(reply.to_json().len() > expected.len());
            if bound > 1 {
                let refused = call(&request(raw, mask, bound - 1), None).unwrap();
                refusal(&refused, "byte_limit", "output_limit", raw);
                assert_eq!(
                    refused
                        .pointer("/error/measurement/measured")
                        .unwrap()
                        .to_json(),
                    expected.len().to_string()
                );
            }
        }
    }

    #[test]
    fn preparation_and_pipeline_refusals_keep_typed_fields_and_available_evidence() {
        let raw = r#"{"a":1,"a":2}"#;
        let reply = call(&request(raw, ".a", 64), None).unwrap();
        refusal(&reply, "selection", "projection_ambiguous_member", raw);
        assert_eq!(text(&reply, "/error/field"), "a");
        assert_eq!(
            reply.pointer("/validation/valid").unwrap().to_json(),
            "true"
        );
        let reply = call(&request("[]", ".a", 64), None).unwrap();
        refusal(&reply, "selection", "projection_wrong_type", "[]");
        let reply = call(&request("{bad", ".a", 64), None).unwrap();
        refusal(&reply, "result_parsing", "json_syntax", "{bad");
        assert!(reply.pointer("/error/offset").is_some());
        assert!(matches!(
            reply.get("validation").unwrap().value,
            Value::Null
        ));
        let mut args = request(r#"{"a":1}"#, ".a", 64);
        *args.pointer_mut("/output_schema").unwrap() =
            json::parse(r#"{"required":["missing"]}"#).unwrap();
        let reply = call(&args, None).unwrap();
        refusal(&reply, "validation", "schema_rejected", r#"{"a":1}"#);
        assert_eq!(
            reply.pointer("/validation/valid").unwrap().to_json(),
            "false"
        );
        *args.pointer_mut("/output_schema").unwrap() = json::parse(r#"{"type":7}"#).unwrap();
        let reply = call(&args, None).unwrap();
        refusal(
            &reply,
            "schema_compilation",
            "schema_compilation",
            r#"{"a":1}"#,
        );
        assert!(reply.pointer("/error/location").is_some());
        for (path, value, error) in [
            ("/format", "old", "context_unsupported_format"),
            (
                "/context/outputEnforcement",
                "tokens",
                "context_enforcement",
            ),
            ("/context/outputProjectionJq", ".a | length", "mask_syntax"),
        ] {
            let mut args = request("null", ".a", 64);
            *args.pointer_mut(path).unwrap() = string(value);
            refusal(&call(&args, None).unwrap(), "context", error, "null");
        }
        let mut args = request("null", ".a", 64);
        let Value::Object(members) = &mut args.pointer_mut("/context").unwrap().value else {
            unreachable!()
        };
        members.push(json::Member {
            key: "outputProjectionJq".into(),
            key_span: None,
            value: string(".b"),
        });
        let reply = call(&args, None).unwrap();
        refusal(&reply, "context", "context_duplicate_field", "null");
        assert_eq!(
            reply.pointer("/requested/masks").unwrap().to_json(),
            r#"[".a",".b"]"#
        );
        assert!(text(&reply, "/requested/context_json").contains(r#""outputProjectionJq":".b""#));
    }

    #[test]
    fn accepted_processing_preserves_native_resource_and_cancellation_refusals() {
        let args = request("null", ".a", 64);
        let mut measured = Governor::new(Budget::default());
        let success = process(&args, &mut measured).unwrap();
        assert_eq!(text(&success, "/status"), "resolved");
        let mut bounded = Governor::new(Budget {
            work: measured.usage().work - 1,
            ..Budget::default()
        });
        let late = process(&args, &mut bounded).unwrap();
        refusal(&late, "byte_limit", "resource", "null");
        assert_eq!(text(&late, "/error/stop"), "budget_exceeded");
        assert!(
            late.get("validation")
                .is_some_and(|v| !matches!(v.value, Value::Null))
        );
        let token = CancellationToken::new();
        token.cancel();
        let mut governor = Governor::with_cancellation(Budget::default(), token);
        let reply = process(&args, &mut governor).unwrap();
        refusal(&reply, "context", "resource", "null");
        assert_eq!(text(&reply, "/error/stop"), "cancelled");
        assert_eq!(text(&reply, "/receipt/termination/kind"), "cancelled");
        let mut governor = Governor::new(Budget {
            work: 0,
            ..Effort::Standard.budget()
        });
        let reply = process(&args, &mut governor).unwrap();
        refusal(&reply, "context", "resource", "null");
        assert_eq!(text(&reply, "/error/stop"), "budget_exceeded");
        assert_eq!(text(&reply, "/error/detail"), "work");
        assert_eq!(text(&reply, "/receipt/termination/kind"), "budget_exceeded");
    }
}
