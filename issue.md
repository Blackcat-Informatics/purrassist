# Issue #32: Tool graph phase 2: specification (docs/tool-graph/TOOLS.md and TOOLS_PLAN.md)

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github
Labels: documentation, enhancement

## Body

## Objective

Write the specification for the tool graph as two documents, `docs/tool-graph/TOOLS.md` and `docs/tool-graph/TOOLS_PLAN.md`. Every later phase is built and judged against them.

**This phase is a review stop.** Phase 3 starts only after the maintainer approves both documents. That approval, quoted word for word in a comment on this issue, also serves as the maintainer approval for new dependencies that `docs/DEPENDENCIES.md` requires (see TOOLS_PLAN.md below).

The documents go under `docs/tool-graph/` because `docs/TOOLS.md` is the generated MCP tool reference (`tools_reference_matches_registry`) and must keep that path.

Read the Phase 1 reconnaissance (#31) first. It lists every purrassist and purrdf type the design reuses, gaps G1–G6, and the AppArmor 4.1 grammar facts.

## `docs/tool-graph/TOOLS.md`: what the specification must contain

### 1. Caller-owned vocabulary (complete Turtle 1.2)

purrdf mints no domain IRIs. The PurRAssist Tool & Security Ontology is a caller-owned namespace table: one `json_schema::Namespaces` plus the prefix table handed to every purrdf call. It is never a string literal inside library code. The document defines, with `rdfs:comment`, domain and range for each term:

- **Tool binding:** `fno:Function` / `fno:Parameter` / `fno:Output` / `fnom:Mapping` (W3C Function Ontology, read through `purrdf_core::fno` after Blackcat-Informatics/purrdf#747), linked to SHACL 1.2 `sh:NodeShape` input and output shapes.
- **MCP `ToolAnnotations`:** `title`, `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`, with their exact mapping to and from MCP JSON.
- **Reified parameter hints.** These are RDF 1.2 reifiers on property-shape triples (`<< :customerIdShape sh:path :customer_id ~ :claim1 >>`) carrying:
  - recovery tolerances: allowed edit distance, alias list, permitted type coercions, case and Unicode folding, mapped onto core `ParameterSpec` aliases and `RepairPolicy`;
  - taint labels;
  - session-binding invariants.
  
  The document states which of these become core recovery inputs, which become Datalog facts, and which become AppArmor variables.
- **Authorization rules.** `parameterConstraints` are SPARQL 1.2 RL rule documents (`purrdf_shapes::srl`) that derive `pa:permits` / `pa:denies` / `pa:conflict` facts. They are not prose. The document gives the exact predicate set the gate reads.
- **Execution profile.** Resource ceilings (address space, file size, process count, CPU seconds, open files) with typed units that map one-to-one onto `set rlimit` values, the confinement mode (`aa-exec` standalone profile or in-process `change_hat`), and `egressAllowed`.
- **Context management:** `outputProjectionJq` (a defined field-mask subset; see design decision D6 in the tracking issue), `maxOutputTokens`, `historyRetentionTurns`, `intentTags`, and `loadingStrategy` (`always` / `on_demand`).
- **Granular AppArmor vocabulary:**
  - file rules: the path glob in the AppArmor dialect with `@{VAR}`, the permission set `r w a k l m`, and the exec transitions `ix px cx ux Pix Cix` plus their `->` targets;
  - qualifiers `owner`, `audit`, `deny`;
  - capability allow and deny;
  - network rules over domain, type *or* protocol (they are mutually exclusive in the 4.1 grammar), access set, local and peer `ip=` / `port=`;
  - `unix` rules: access, type, `addr`, `peer=(label= addr=)`;
  - `signal` (access, `set=`, peer), `ptrace` (access, peer), `mount` / `remount` / `umount` / `pivot_root` deny;
  - rlimits;
  - hats and subprofiles;
  - `owl:imports` of abstraction profiles (for example `<abstractions/base>`).
  
  The default is the empty capability set plus an explicit `deny capability` for the named dangerous capabilities.
- **SHACL shapes over the vocabulary itself.** A tool graph is validated against them before anything is compiled.

### 2. End-to-end worked example

Two tools: `query_customer_orders` (database egress, session-bound `customer_id`) and a filesystem tool that edits files under `--root`. Each appears in every representation below, and every representation is derived from the first.

1. RDF 1.2 Turtle plus the SRL rules, including "`customer_id` must equal the authenticated session `user_id` and belong to an active tenant".
2. The compiled AppArmor policy `/etc/apparmor.d/purrassist.tools`:
   - a parent profile with a `^query_customer_orders` hat;
   - a standalone `purrassist.tool.<name>` profile for `aa-exec`;
   - `@{WORKSPACE_ROOT}` and `@{TOOL_TMP}` bound from `--root` and the session;
   - `owner @{WORKSPACE_ROOT}/** rwkl,`;
   - `audit deny @{HOME}/.ssh/** rwx,` and `audit deny /**/.env* rwx,`;
   - the capability denies, network, unix, signal and ptrace rules, the mount denies, and the rlimits.
3. The MCP Extended Tool Manifest JSON, with `annotations`, `_meta.security`, `_meta.executionProfile` and `_meta.contextManagement`.
4. The llama.cpp GBNF grammar, including the model-family call envelope, and the `CompactSignature`.
5. The `purrdf_core::loss::LossLedger` of every projection, entry by entry. Each dropped constraint is mapped to the mandatory post-generation check that enforces it at runtime.

**The network example must be honest about AppArmor 4.1.** "TCP to 10.0.0.0/8 port 5432" cannot be written in AppArmor: there is no CIDR form, and type and protocol are exclusive. The example uses `network (connect) inet stream peer=(ip=<db host> port=5432),` and shows the refusal that a `/8` produces (design decision D4).

### 3. Model-family injection matrix

For Claude (Opus / Sonnet; Anthropic `tools`), Qwen 2.5/3 and Hermes, Llama 3.x/4, Mistral (v3 and v11), DeepSeek (V3, V3.1) and CompactTypeScript, the matrix gives:

- the exact system-prompt placement and the exact bytes, produced by the existing `DeclarationForm` renderers (D8);
- the call envelope that recovery expects (the existing `DialectId`);
- the token budget, as a sound byte upper bound (D5);
- what each family's form drops, as LossLedger codes.

It also covers deterministic `on_demand` retrieval: a `purrdf_text` BM25F profile over `intentTags` (weighted) and descriptions, with a total order, the tie-break, and a worked query.

### 4. Status

Every section is marked as specified, not implemented.

## `docs/tool-graph/TOOLS_PLAN.md`: what the plan must contain

- **Crate layout.** One new pure crate, `purrassist-toolgraph`, that builds for `wasm32-unknown-unknown`. Its modules are `vocab`, `graph`, `codec::mcp`, `policy`, `apparmor`, `lanes::{gbnf, compact, projector}`, `routing`, `recovery` and `manifest`.
  - Host-side confinement goes in `purrassist-host` under `cfg(target_os = "linux")`: launching under `aa-exec` and `change_hat` through `/proc/self/attr`.
  - CLI and MCP wiring goes in `purrassist-cli` and the host registry.
  - `purrassist-core` keeps its empty dependency closure.
- **Trait and type signatures:** `AppArmorSynthesizer`, `GbnfCompiler`, `DatalogPolicyGate`, `ModelFamilyProjector`, `RecoveryBridge`, the tool-graph model types, and every typed error enum and its variants. Each fallible operation states its error semantics. Resource exhaustion is its own outcome.
- **Dependency closure for approval.** It names the exact purrdf crates, which are leaf crates rather than the `purrdf` facade, and lists every third-party package in their transitive closure with its version. The closure includes `rayon`, `sha2`, `hashbrown`, `regex`, `regex-syntax`, and through `purrdf-gts` also `aes-gcm`, `structured-zstd`, `visual-hashing` and `fastrand`. The plan checks each against the network denylist and lists the registration rows for `vendor/dependencies.json` and the baseline.
- **Test gates per Phase 3 issue,** and the conformance suite of Phase 4.

## Acceptance

- Every AppArmor snippet in TOOLS.md is extracted and passes `apparmor_parser -QK` (parse only, no kernel load) on parser 4.1.x. The parser version is recorded.
- Every Turtle block parses through `purrdf_rdf::parse_dataset` as Turtle 1.2. Every SRL block passes `srl::check`. Every JSON block is valid JSON. The GBNF parses with llama.cpp's grammar parser, or a recorded equivalent check.
- Both documents pass `prose-check`.
- The maintainer's approval comment is recorded verbatim on this issue before Phase 3 starts.


## Comments (0)

