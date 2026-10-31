<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-toolgraph

Pure Rust validation for caller-owned tool-graph vocabulary, loss-profile and
MCP context fragments and [AppArmor](https://apparmor.net/) glob words, with exact output byte bounds. No vocabulary
namespace is supplied implicitly.

This workspace crate is unpublished while its required upstream APIs use an
exact Git revision. Facade/host integration and registry-resolvable packaging
remain part of the complete graph-loader delivery. The existing release set
retains its registry-resolvable dependencies.

Run the executable caller example with
`cargo run -p purrassist-toolgraph --example validate_vocabulary`.

Implemented: validated vocabulary construction, registered projection loss
meanings, the complete caller-owned MCP context fragment codec, the bounded
four-form field-mask parser, and the governed `OutputByteLimit` checker. The checker records exact bytes and refuses oversized
serialized results intact; it makes no token-count claim. Graph loading, the
production schema bridge, manifest codecs, authorization and execution remain specified in
the repository's tool-graph documents; this crate does not yet provide them.

`codec::mcp::ExecutionProfile` reads the complete execution-profile fragment
from a caller-owned JSON DOM; `ManifestEnvelope::read` checks these fields for
every tool and `ToolEnvelope::execution` exposes the borrowed typed view.
Missing, duplicate, unknown, mistyped, unsupported-mode and numeric refusals
retain their original locations and enclosing tool index. The two wire modes
are `aa-exec` and `change_hat`. Profile/hat strings remain verbatim. All five
resource quantities are exact positive `u64` values, including integral decimal
and exponent spellings; the `AppArmor` renderer's narrower representation limits
remain a separate operation. `encode_json` reuses the governed JSON writer with
fixed field order, preserving numeric spellings and decoded string values without copying the source DOM.
This fragment establishes neither RDF agreement nor an executable graph. Full
manifest conversion and host integration remain under #33 and #37.

`FramedPrompt::assemble_bytes` assembles explicit prefix, separator and suffix
with ordered caller-prepared declarations. It preserves original source/part
identities and all supplied bytes, enforces a separate exact prompt byte ceiling,
and publishes complete output only after governed sizing, copying, storage and
cancellation checks. Run
`cargo run -p purrassist-toolgraph --example framed_prompt_bytes` for a consumer
that prepares real wire declarations separately and validates the complete array.
`FramedPrompt::assemble_accounted` extends that same assembler with independent
token admission. A deterministic, pure caller `TokenCounter` supplies an exact
count under recorded tokenizer/configuration identities; it must govern its work
and storage. A separate `ByteLevelTokenContract` supplies a conservative bound
only under explicit caller assertions: at most one token per input byte, no
uncounted implicit special tokens, and no representation-expanding preprocessing.
The caller's encoding identity and admission basis remain visible; these labels
are not library qualification. Accounted output keeps immutable complete bytes
and the separate measurement. Missing/unavailable accounting, empty identities,
independent byte/token excess, cancellation and resource failures refuse without
fallback or truncation, preserving original inputs and any completed measurement.
The same example exercises a documented scalar encoding on the real declarations.
Tokenizer qualification, model-family placement/selection, graph loading, routing
and complete projection ledgers remain under #36 and #47. Tool-result byte limits
remain the separate `OutputByteLimit` contract.

The direct `plain_tool_schemas` example adopts the supported upstream profile of
`purrdf_shapes::json_schema::compile_tool_arguments` at
`742bbbfbe2240e1fc6f178badd049420753ef358` (`PurRDF` #788 / PR #806 and
numeric prerequisite #885 / PR #888).
Run `cargo run -p purrassist-toolgraph --example plain_tool_schemas` to compile
the four worked input/output schemas and validate bare instances and invalid
neighbors with the full first-party core validator. Focused tests cover nested
input/output bindings, exact constraints, source-located losses and native typed
refusals. Source/profile preparation is caller-owned. Native work/bytes/nodes
ceilings are logical bounds; they do not provide complete process heap or
cancellation governance. The numeric consumer completes G7 under #43, extending
the supported-profile adoption from #70. Decimal bounds, enums and constants
preserve arbitrary precision; finite IEEE constants retain their exact numeric
values. Unrestricted float/double domains carry the source-located
`plain:ieee-representability` loss requiring original SHACL validation. Invalid,
nonfinite, negative IEEE zero and identity-losing constants return typed refusals with
valid neighboring cases covered. #33 still owns production loader/codec wiring
and its complete original contract.

`CallableBinding::admit` validates a selected mapping from an already admitted
upstream `FnO` catalog. It requires complete unique parameter/return bindings and
input predicates/properties, retains declaration order and borrows original
native identities and literal facets. Catalog lookup is binary; ambiguity checks
are explicitly quadratic in the selected callable's arity and charged before
each comparison. A fallible retained index supports direct parameter access.
Run `cargo run -p purrassist-toolgraph --example admit_callable` for an executed
consumer using the actual upstream reader. Catalog preparation is caller-owned;
this operation does not govern RDF parsing or upstream catalog construction.

Graph source ownership, precise occurrence spans, SHACL/G7 compilation, manifest
round trips, authorization and execution remain required under issue #33 and
its dependent deliveries. A complete callable binding is no graph-load success
or permission to dispatch.

`ContextManagement` implements the complete six-field MCP context fragment over
a caller-owned JSON DOM. Decoding and encoding both require the explicit
`purrassist-toolgraph-v2` enclosing format. The codec rejects legacy token fields,
unknown/duplicate/missing members, invalid types/quantities/modes and unsupported
masks. It preserves tag values, order and duplicates, accepts mathematical
integer spellings across the full portable `u64` domain, and emits fixed field
order with canonical masks and exact byte enforcement. Run
`cargo run -p purrassist-toolgraph --example roundtrip_context` for an actual
round trip. Full manifest, RDF and host-release integration remain outstanding.

`codec::mcp::ManifestEnvelope` has a source implementation for borrowing a
complete v2 bundle envelope from the existing ordered JSON DOM. It checks fixed
wire fields, duplicates, required types and contract tags, retaining original
declarations and key/value spans. `ToolEnvelope::decode_context` delegates to
the existing context codec. Typed annotation hints use the MCP defaults and
preserve the original title and member presence without copying. Source review
is complete; compilation and execution evidence are recorded in the delivery
PR. Prefix semantics, nested profile metadata,
schema compilation, RDF agreement and bidirectional graph conversion remain
part of the loader/codec delivery.

`AppArmorGlob` parses caller-declared pathname patterns into a flat semantic
arena and emits safe quoted policy words. Literal host bindings use the same
renderer and explicitly refuse pattern metacharacters or policy injection.
The `apparmor_path_words` example exercises actual parsed and bound output;
it emits standalone read rules for parser qualification, not a synthesized
confinement policy. Full permission analysis, graph admission and host enforcement
remain parent requirements. See `AppArmorGlob` Rustdoc for the admitted byte
grammar and contextual class-escape refusals.

`GlobLanguage` compiles a union of these semantic models with complete literal
bindings into the core's existing derivative engine. Matching consumes complete
non-NUL pathname byte strings, including invalid UTF-8. Native [AppArmor](https://apparmor.net/) 4.1.7
file-rule semantics require a non-slash first byte for a whole star component;
brace and escape boundaries affect that restriction. Raw pattern slash runs are
normalized after literal binding, preserving an initial exactly-two-slash
namespace; candidate pathname bytes are unchanged. Unescaped `*`/`?` inside
classes and escaped class comma are explicitly unsupported because native
conversion gives them different semantics from literal class bytes.
Exact inclusion returns
either `Contained` after exhausting the reachable frontier or a shortest byte
counterexample; resource/cancellation refusal remains an error. Run
`cargo run -p purrassist-toolgraph --example glob_containment` for the actual
lowering and inclusion consumer. This is language containment, not complete
permission/owner/deny analysis, hat admission or host confinement.

`InternetRule` implements standalone governed Internet rule fragments from
caller-prepared declarations. It validates numeric family, one documented socket
type or protocol, nonempty access, audit/deny qualifiers and explicit endpoint
scopes/ports. Standard IP parsing and formatting are reused. Numeric CIDRs stay
symbolic until rendering; caller ceilings bound each endpoint and the complete
local-by-peer product before output. Enumeration includes all mathematical
addresses in numeric order, including IPv4 network/broadcast addresses. IPv6
full-space or excessive counts return explicit limit errors, without truncation
or widening. The shared governed formatter returns the complete fragment
atomically and every artifact carries the `4.0-ip`/`network_v8/af_inet`
requirement. This metadata is no running-kernel or loaded-policy attestation.
Run `cargo run -p purrassist-toolgraph --example internet_rules` for the actual
IPv4 CIDR/IPv6 consumer and its offline parser fixture. Graph admission, complete
permission analysis, v3/v4 policy synthesis, Unix rules and host enforcement
remain under the original parent contracts.

`UnixRule` renders standalone Unix socket fragments for caller-prepared
access/type/qualifier/address/peer-label declarations. It reuses the [AppArmor](https://apparmor.net/)
binding and policy-word printer; the Internet and Unix APIs share the same
native access parser without changing Internet errors or governance. Explicit
unconstrained, anonymous (`addr=none`) and abstract addresses remain distinct.
Abstract patterns require a literal `@` before any variable, retaining the
correct `@@{TOOL_TMP}` spelling. Filesystem sockets require file-access rules;
`auto`, unsupported NUL escapes and incompatible peer/local-only accesses are
refused. One private governed output buffer returns the complete rule atomically.
Artifacts require standalone ABI `3.0` and `network/af_unix`; offline compilation
against packaged targets does not attest to the running kernel or loaded policy.
Run `cargo run -p purrassist-toolgraph --example unix_rules` for the actual
abstract/anonymous/unconstrained consumer. File-rule `GlobLanguage` matching is
not Unix-address matching. Admitted-graph synthesis, original v3/v4 integration,
other IPC families, permission/owner/deny analysis and host confinement remain
under the original parent contracts.

`FileRule` renders standalone file permission and execution fragments from an
existing `AppArmorGlob` and its exact literal bindings. It admits unique
`rwaklm` permissions, refuses write with append, and supports `ix`, `px`, `cx`,
`Pix` and `Cix`. Directed targets use the explicit literal ASCII subset
`[A-Za-z_][A-Za-z0-9_.-]*`. Native attachment-based transitions remain available
without a target. Audit, deny and owner qualifiers retain their independent
meanings; denied `ix` emits bare `x` with explicit canonicalization metadata.
Targets on deny rules and other denied transition requests are refused.
Unconfined `ux`/`Ux` always require unavailable authenticated waiver admission;
no caller flag or self-asserted record can authorize them here.

Paths must start with an unescaped literal slash or an explicitly bound absolute
variable. Relative, brace/class-leading and escaped-leading forms are refused
before output. Existing binding validation and policy-word emission preserve
original glob/binding identities and errors. The complete governed artifact
records the selected `3.0`/`file` requirement without attesting host mediation.
Run `cargo run -p purrassist-toolgraph --example file_rules` for the real bound
path, named transition and canonical denial consumer. Graph admission, complete
permission/hat analysis, broader native target names, authenticated waiver
handling, policy synthesis and host enforcement remain parent requirements.

`SignalRule` and `PtraceRule` render governed standalone IPC fragments with an
explicit peer `AppArmorGlob` and its complete literal bindings. Access sets are
nonempty and closed: send/receive for signals, trace/read/readby/tracedby for
ptrace. Native aliases outside these sets are refused. Signals admit the
33 documented names (including `stp` and `exists`) and `rtmin+0` through
`rtmin+32`; bounded leading-zero RT inputs render canonically while the original
field remains in the artifact. Unknown names, OS numbers, `rtmax`, malformed
lists and out-of-range RT offsets are explicit failures.

Both rule types preserve independent audit/deny conditions, always print the
peer and access set, and use the existing bound-word writer without file path
normalization, Unix address markers, reparsing or label matching. An explicit
`**` is broad by request; an omitted peer has no representation. Empty source
remains an existing glob refusal, while nullable admitted expressions retain
their native meaning. The complete artifact carries selected ABI `3.0` and
signal/ptrace mediation metadata, without attesting host configuration or the
other process's permission. Run `cargo run -p purrassist-toolgraph --example
ipc_rules` for the actual consumer. Graph admission, conflicts, complete
synthesis, trust and host confinement remain original #34/#35 requirements.

`CapabilityRule` and `MountRule` render standalone governed privilege fragments
from original borrowed field text. Capability names use the closed ABI `3.0`
forty-name table. Single safe grants and all named denials retain audit/deny
qualifiers; all fourteen normative dangerous grants are refused.
`dangerous_capability_denials` emits that complete explicit deny baseline in
normative order with an empty allow set. `checkpoint_restore` is a typed
unsupported-target refusal: native ABI compatibility lowering to `sys_admin`
must never broaden the requested permission.

Mount rules accept exactly `mount`, `remount`, `umount` and `pivot_root` and
require explicit denial. Requested allows and unknown text are refused; no
path, option or transition semantics are accepted and discarded. Each operation
readmits original rule and field inputs under the current governor, admits
bounded table/formatter computation and fallible writes, and checks final
cancellation before complete publication. Artifacts retain original identities
and explicit `3.0`/`caps` or `3.0`/`mount` target metadata. This is no running-kernel
or loaded-policy attestation.

Run `cargo run -p purrassist-toolgraph --example privilege_rules` for the actual
consumer: all named denials, every safe grant, the complete dangerous-capability
baseline and all four mount denials. The executable owns its offline fixture
envelopes. Graph-wide baseline composition, static admission, full v3/v4 synthesis,
authenticated trust and host confinement remain under #34/#35. Import handling
remains required by the parent contract.

`ResourceLimits::render` emits all five exact positive finite profile limits in
fixed order: `as`, `fsize`, `nproc`, `cpu`, `nofile`. Every field is mandatory.
The complete artifact and typed refusals retain the source identity and all
original numbers. Counts admit through 9223372036854775807; CPU admits through
9223372036854 whole seconds and always prints explicit `seconds`. Byte values
through 9223372036854775807 use bare decimal; larger values require exact
divisibility by 1024 and use the quotient with `K`. The largest supported byte
value is 18446744073709550592. Zero, infinity and unsupported representations
are refused before formatting, without rounding, clamping or wrapping.

Run `cargo run -p purrassist-toolgraph --example resource_limits` for the actual
consumer, or supply five decimal arguments in the documented field order.
The existing governed formatter publishes the complete five-declaration fragment
atomically. Its ABI `3.0`/`rlimit` requirement is an offline target selection;
it establishes neither loaded policy nor actual process enforcement. Full graph
admission, policy composition, trust and host confinement remain under #34/#35.


`ContextManagement::project_result` is the complete caller-prepared result path:
full original output-schema validation, all four mask forms, canonical compact
JSON Lines emission and one exact complete-stream byte check. Every value has
one trailing LF; zero values emit zero bytes, with no outer array. Original node,
mask, validation/stage evidence and explicit transformations survive success and
refusal. Selected duplicate names refuse ambiguity; whole selected values retain
nested duplicates and numeric spelling. Oversize/native resource failures publish
no partial or truncated stream. Source/schema/context preparation, history,
graph admission, authorization and host release remain separate.

Run the bounded stdin consumer with:

```sh
cargo run --locked -p purrassist-toolgraph --example project_result <<'JSON'
{"format":"purrassist-toolgraph-v2","context":{"outputProjectionJq":".orders[]?","maxOutputBytes":64,"outputEnforcement":"exact-bytes","historyRetentionTurns":0,"intentTags":[],"loadingStrategy":"always"},"outputSchema":{"type":"object"},"result":{"orders":[1.00e2,true]}}
JSON
```

The complete stdout artifact is `1.00e2` followed by LF and `true` followed by
LF. Typed refusals and exact measurements appear on stderr, before any stdout.
This envelope demonstrates the callable result pipeline; it is not a complete
MCP/RDF bundle or an execution grant.


## Caller-prepared `AppArmor` policy documents

`PolicyDocument::prepare` accepts an explicit `PolicyInput`: offline ABI 3.0 or
4.0-ip, a literal absolute HOME and caller-owned standalone profiles or parents
with flat hats. Bodies contain the existing typed file, Internet, Unix, signal,
ptrace, capability, mount and resource-limit declarations. Existing per-rule
literal bindings and Internet enumeration ceilings remain authoritative inputs.
Preparation owns only the fixed baseline globs/access sets and borrows the exact
original input. It grants no graph, session or execution authority.

`compile` validates names with `FileTarget`, rejects duplicate root names, local
hat names and caller rule identities within a body, and sorts those scopes by
original byte identities. Identical rules with different identities survive.
Every execution body receives the SSH/environment-file, dangerous-capability,
broad ptrace and four mount-operation denials through the existing rule
renderers. HOME uses existing literal escaping and unsafe-path refusals; root
and trailing slash inputs add no accidental separator. Compiler additions have explicit
baseline origins rather than fabricated caller provenance.

The complete ABI/profile/hat document and source spans publish together after
final cancellation admission. New indices, text and spans use fallible scratch
owners. Existing String-returning renderers retain their documented conservative
reservations; this compiler does not claim prompt release of their detached
intermediate charges. Original input and exact native rule/resource errors
remain available on refusal. ABI 3.0 refuses fine-grained Internet rules instead
of dropping scope. Aggregated ABI/mediation metadata describes an offline target,
not running-kernel, loaded-policy or dispatch evidence.

The same compiler also emits mandatory abstraction imports from ordered
`PolicyImport` occurrences and opaque `PolicyDependency` files. Paths must be
normalized relative `abstractions/...` files; traversal, variables, wildcards,
include delimiters and control bytes are refused. Every direct import must bind
to one declared file. Duplicate dependency paths, including unused transitive
records and identical contents, are a typed conflict preserving both records.
Repeated import occurrences with distinct source identities remain in caller
order, with exact profile/hat spans and manifest indices.

Dependency bytes remain separate external files. Governed SHA-256 computation
records their exact identities; the library never reads the filesystem, parses
imported permissions, or concatenates those bytes into a profile body. All
original declared records remain available, including opaque transitive files.
Their presence does not prove that the caller supplied a complete import closure.
`local_mediation()` describes local typed rules only. `complete_mediation()`
returns a typed refusal pointing to the first import span whenever imports are
opaque. No caller boolean can turn that source artifact into complete feature
requirements or dispatch authority.

The actual `policy_document` consumer retains its full/baseline modes with
unchanged no-import bytes. `--bound-imports NEW_INCLUDE_ROOT` compiles through the
same production path and materializes first-party synthetic dependency files in
a fresh caller-selected directory. Offline qualification uses an explicit isolated
include root, matching ABI/features and fatal warnings; missing required direct
or transitive files fail rather than becoming optional includes. This establishes
no compatibility claim for installed standard abstractions.

Full RDF/SRL/static admission, permission/owner/deny proofs, graph import closure
and imported-permission trust, nested graph profile synthesis, transition-target
existence, graph CLI, loaded mediation and host confinement remain
#33/#34/#35/#39 requirements.


Field masks, context metadata and validated result projection are reexported
from `purrassist_core::output`. The shipped facade exposes these same types
through `purrassist::output`; the read-only host `project_result` operation is
available through CLI `call` and MCP. These are result-processing surfaces,
without graph-backed dispatch or release authority.

## Exact prepared policy trust

`PreparedPolicyContract::prepare` retains a complete original SRL document table
and computes each document's exact-text SHA-256 and finite rooted import-contract
SHA-256. Ordered first-use/back-reference transcripts preserve repeated imports,
sharing and cycles, and bind original locations and optional external bases.
Missing, duplicate or unreachable records fail explicitly.

`TrustedPolicyRegistry::prepare` receives immutable host grant records with
independently supplied SHA-256 approvals and one exact tool, opaque vocabulary,
issuer and session-schema scope. Its canonical complete SHA-256 includes every
grant field and provenance label; ambiguous duplicate authority is refused.
`bind` requires every reachable document's exact grant and publishes only a
complete matched handle after final cancellation admission. New scans and
release-owned storage share the supplied governor; failures preserve originals,
registry context, usage and native causes.

Run `cargo run -p purrassist-toolgraph --example trusted_policy` for independent
synthetic approvals followed by actual native SRL syntax/import checks over the
matched original text and bases. This includes shared/repeated and cyclic imports
and native missing, duplicate and unused table refusals. The consumer refuses
recorded bases that native imported-document checking cannot preserve. Native
parsing/checking allocation is separate from this preparation API.

Matching grants no dispatch authority. Authenticated host configuration, native
closure/protected-head admission, governed rule evaluation, graph admission,
execution and result release remain required under the existing tracking issues.
