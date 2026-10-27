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
serialized results intact; it makes no token-count claim. Graph loading, schema
projection, manifest codecs, authorization and execution remain specified in
the repository's tool-graph documents; this crate does not yet provide them.

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
