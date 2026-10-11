# Structure review: PR #19, `purrassist-derive` and core derive support

## Scope

- **Diff:** `fccfbe7..HEAD`, at HEAD `e4850a8`.
- **Derive crate:** `crates/derive/src/{bridge,parse,model,write,lib}.rs`.
- **Core support:** `crates/core/src/tool_call/{derive_support,from_node,mod,toolset}.rs`.
- **Facade:** `crates/purrassist/src/lib.rs`.
- **Tests:** `crates/derive/tests/derive.rs`.
- **Checked against:** plan D-1..D-14 and I8, AGENTS.md contracts, and the `.goals` standing goals.

**Executable probes.** These ran in a throwaway crate (`/tmp/pa-inject`) that uses path dependencies on this worktree. The tests compiled the derives against adversarial inputs:
- string, raw-string, doc-comment and default literals containing `"`, `\`, `*/`, `"); panic!("`, `{ }` and U+202E;
- reserved-prefix field names;
- a shadowed `assert!`;
- shadowed primitive types.

## Summary of the architecture

Module boundaries are sound:
- `proc_macro` appears only in `bridge.rs`, plus the `TokenStream` signature types in `lib.rs`.
- `parse`, `model` and `write` are pure, and are unit-tested through a `cfg(test)` lexer (`model.rs:657`).

The span model works within stable Rust's limits (no `Span::join`). A span is an index into a pre-order table of the original tokens, and invisible groups are flattened. Each diagnostic points at a single token, and the tests assert the exact token for representative cases (`write.rs:694`).

Error reporting is first-error-wins: `Result<_, Diagnostic>`, converted to one spanned `::core::compile_error!`. A generator defect becomes an error, not a panic (`bridge.rs:27`).

Determinism holds:
- there is no hashing;
- the only sort is a stable sort on unique keys (`write.rs:269`);
- output depends only on the input tokens.

String injection through literals is closed, as the probe confirmed:
- every user string spliced into the generated text is the source of exactly one string-literal token that `decode_string` accepted. That rules out byte, C-string, char and suffixed literals.
- synthesized text goes through `{:?}` (`model.rs:125`).
- number spellings are `{:?}`-quoted (`write.rs:90`).
- type tokens are spliced back from the originals rather than re-lexed.

Descriptions, patterns, defaults (including object keys) and enum values containing quotes, backslashes, `"); …` sequences and braces all round-trip byte-exactly. They cannot alter the generated code.

The two remaining surfaces where user text gains meaning in generated code are F-1 and F-3 below.

## Findings

### F-1 REQUIRED: user names are spliced into a `format_args!` string, and the assertion macro path is not absolute

- **Where:** `crates/derive/src/write.rs:356-363`.
- **What it emits:** `const _: () = assert!(__valid_name(<lit>), "<message>");`.
- **The defect:** `<message>` embeds `text.value`, the user's tool, parameter or alias name, via `{message:?}`. The second argument of `assert!` is a **format string**, so braces in a user string are interpreted as format syntax.

**Probe A:** `#[tool(name = "bad {name}")]` fails with `error[E0425]: cannot find value 'name' in this scope`, spanned at the derive. The intended message ("tool name … is not valid …") is never shown.
- The string captures an identifier from the user's scope (inline format-args capture). That is user text being given code meaning.
- `"{}"` instead yields a positional-argument error.
- The input is invalid either way, so there is no false acceptance. But the diagnostic contract (plan D-12, "exact message") is broken for exactly the inputs the check exists for.

**Probe D:** a `macro_rules! assert { ($($t:tt)*) => {}; }` in the deriving module fails expansion with "macro expansion ends with an incomplete expression". A user `assert` that expands to `()` would silently disable the compile-time name check, and only `ToolSet::compile` would then catch the name. This contradicts I8 ("every path is absolute").

**Fix:**
- Emit `::core::assert!(…)`.
- Escape the message as a format string: `message.replace('{', "{{").replace('}', "}}")` before `{:?}`.
- I verified `const _: () = ::core::assert!(f(), "… {{name}} …");` yields `evaluation panicked: … {name} …`.
- Add a unit test in `write.rs` with a name containing `{x}`, plus a hygiene twin that shadows `assert`.

### F-2 ADVISORY: primitive types in generated code are not absolute paths

- **Where:** `write.rs:251,253,318,319,482,484`. The generated code uses bare `bool`, `usize` and `&[&str]`.
- **Probes C/F:** a user `struct usize;`, or `use m::str;`, in the deriving module breaks the expansion:
  - E0326 "implemented const `DEPTH` has an incompatible type";
  - E0308 against `Members::new`.
- The hygiene test (`tests/derive.rs:1299`) shadows `core`, `std` and several names, but not primitives. The I8 claim is therefore stronger than the evidence.
- **Fix:**
  - Emit `::core::primitive::{bool, usize, str}`.
  - Add `struct usize; struct bool; struct str;` to the hygiene module.

### F-3 ADVISORY: field and variant idents are written as text, so they collide with the writer's reserved prefixes

**Where:**
- `write.rs:304` (`{field}: __pa_vN`);
- `write.rs:310` (extra field);
- `write.rs:453` (`Self::{variant}`).

`bridge::rebuild` then treats any `__pa_splice_N` ident as a placeholder (`bridge.rs:89-97`). It also re-spans any `__pa_*` ident to `mixed_site`.

**Probe B:** `struct B { __pa_splice_0: u8 }` is valid Rust, but fails with `E0560 struct 'B' has no field named 'B'`, because the field name was replaced by splice 0 (the type name). A variant named `__pa_splice_N` fails the same way.

`__pa_node`/`__pa_v0` field names happen to work (probe E).

**Fix (preferred):** splice field and variant idents from their original tokens, as the type name already is (`self.splice([param.ident.span])`). This removes the collision class. It also gives rustc errors in the struct literal the user's spans and edition, instead of `call_site`.

**Alternative:** reject `__pa_` idents with a spanned `compile_error!`.

### F-4 ADVISORY: the sorted-table invariant of `Members` is checked only in debug builds, while the table is computed from a second decoder

**The two sides of the invariant:**
- The derive sorts the `__PA_NAMES` table by its own decoded value, `key.value` from `decode_string` (`write.rs:269`), but emits each name as `key.source` (`write.rs:272`). Rustc re-decodes that source.
- `Members::new` checks strict ordering only with `debug_assert!` (`derive_support.rs:532`). It documents that release builds misclassify members on an unsorted table (`derive_support.rs:465-469`).

**Current risk:** I found no divergence between `decode_string` and rustc for valid names, which are ASCII `[A-Za-z0-9_.:-]` after `__valid_name`.

**The consequence if they ever diverge:** a release-only silent failure. A declared member would come back `Slot::Undeclared`, and so be rejected as `unknown`, or routed into `extra` under `pass`.

**Fix:** move the check to compile time, at zero runtime cost. Add a `#[doc(hidden)] pub const fn __sorted_names(&[&str]) -> bool` (a byte loop) in core. The derive then emits `const _: () = ::core::assert!(…::__sorted_names(__PA_NAMES));`, or `Members` takes a const-constructed table type. Release builds then cannot misclassify.

Separately, the `Members` Rustdoc says generated conversions "ask `is_present` for each required field" (`derive_support.rs:457-460`). They do not: they use per-field `Option` slots and `ToolParameter::absent()`. Correct the doc; AGENTS.md requires documentation to describe the implemented behavior.

### F-5 ADVISORY: core trait surface, extensibility and semver

**`ToolExtra` (`derive_support.rs:242`) cannot be implemented usefully.** The generated code always assigns `Node::new(Value::Object(..))` to the extra field (`write.rs:309`). A user who implements `ToolExtra` for, say, `BTreeMap<String, Node>` passes the bound check and then gets an unrelated type mismatch inside the expansion. Two options:
- (a) give it a constructor, `fn from_undeclared(members: Vec<Member>) -> Self`, and have the generated code call it. This makes `extra` extensible (`Vec<Member>`, maps), which fits "maximal utility";
- (b) seal it.

Either way, decide before the first publish, because sealing later is a breaking change.

**`ToolParameter` (`:137`) is intentionally implementable**, and newtypes such as `struct FilePath(String)` are a real use. But:
- it has three required items, `REQUIRED`, `absent()` and `DEPTH`, with no defaults;
- their coherence (`REQUIRED == absent().is_none()`) is unstated. `REQUIRED = false` with `absent() = None` produces an optional schema member whose absence fails `from_node` with `missing`.

Fix:
- give defaults (`REQUIRED = true`, `absent() = None`, `DEPTH = 0`), so hand impls write only `parameter()`;
- state the invariant;
- note that future items must be defaulted to stay semver-compatible.

**`Slot` (`:427`) is a public exhaustive enum.** Generated code matches it without a wildcard (`write.rs:370-409`), so adding a variant breaks both every hand-written matcher and every expansion produced by an older derive.
- Decide now: `#[non_exhaustive]` plus a generated wildcard, or a documented freeze.
- Consider `#[doc(hidden)]` for `Members`/`Slot` if they are not meant as general API. Their `&'static` tables and length-mismatch panic are codegen-shaped.

**Version skew between derive and core.** Generated code calls `#[doc(hidden)]` core items (`__derive_number`, `__valid_name`, `Members`, `Slot`). Derive has no dependency on core, and the facade takes both with caret requirements (`Cargo.toml:19-20`).
- Within the 0.1.x line, `cargo update -p purrassist-derive` can pair a newer derive with an older core. The result is unresolved-path errors inside the expansion.
- **Fix:**
  - pin `purrassist-derive = "=…"` from the facade, as serde does with serde_derive;
  - have every expansion reference a versioned marker, e.g. `…::tool_call::__derive_abi_v1`, so a mismatch is one clear error;
  - document the lockstep for direct-core users.

**The type coverage is closed for foreign types.** Because of orphan rules, users cannot add `ToolParameter` for `f32`, `Box<T>`, `PathBuf`, `NonZero*`, `BTreeSet<T>` and similar; they need newtypes. `f32` is absent from both `FromNode` and `ToolParameter`. Adding `f32` and `Box<T>` (forwarding) is cheap and in the spirit of "subsume, extend".

**No crate-path override.** Generated paths are hard-wired to `::purrassist_core` or `::purrassist::__core` (`write.rs:27-32`). A renamed dependency (`pa = { package = "purrassist" }`) or a downstream framework that re-exports purrassist cannot use the derives. An optional `#[tool(crate = path)]` (serde's `#[serde(crate)]` pattern) would close this. The facade's "no `crate =` needed" property would be kept.

### F-6 ADVISORY: duplicated generation and duplicated grammars

**`tool_definition()` and `object_spec()` each emit the full parameter chain** (`write.rs:215-220`).
- Two independently generated copies must stay identical.
- Each field type is spliced roughly 8 to 12 times per expansion, which multiplies type-check work on large tool sets.
- **Fix:** add `ToolDefinition::arguments(self, ObjectSpec) -> Self` in core (or `From<ObjectSpec>` plus a name), and emit `ToolDefinition::new(name)…​.arguments(<Self as ToolArguments>::object_spec())`.

**`is_json_number` (`model.rs:383`) duplicates core's `Number::from_str` grammar.** A drift would become a runtime panic in `tool_definition()` (`__derive_number`, `derive_support.rs:396`). The input would have passed the derive, so this is an un-reported failure mode from user input.
- **Fix:** add a derive test that uses the existing dev-dependency on core to cross-check `is_json_number` against `Number::from_str` over a shared corpus.

**Inconsistent name validation timing.** Derived *tool* names are validated at expansion and spanned at the ident (`model.rs:470-486`). Derived *parameter* names, such as a non-ASCII field ident, are known exactly at expansion too, but they fall through to the const assertion, which is spanned at the derive. Applying `is_valid_derived_name` to synthesized parameter keys in `resolve_param` would give a spanned error.

## Not findings (checked)

- **No core dependency on derive, and no ambient access** in `derive_support.rs`. `is_valid_name` became a `const fn`, which is semver-compatible.
- **`FromNodeError::within`, `missing`, `duplicate` and `unknown`** escape per RFC 6901 through `pointer_segment`, so they are safe for arbitrary hand-supplied keys.
- **No `FromStr` failure path from user input:**
  - spliced literals are single string-literal tokens;
  - idents are valid idents;
  - all synthesized text is `{:?}`-quoted;
  - the `LexError` branch is correctly an internal error.
- **Indexing in `parse.rs` is guarded:**
  - `value[0]` at `:400` and `:470` is safe because `key_items` guarantees a non-empty value;
  - `part[0]` at `:552` is safe because `comma_parts` drops empty parts.
- **`Ident::new(.., mixed_site)` in `bridge.rs:96`** only receives text from an existing ident with a `__pa_` prefix, so it cannot panic.
- **File sizes are acceptable.** `lib.rs` is mostly the paired `compile_fail` doctests; `parse.rs` and `model.rs` are about 40% unit tests.
- **The `cfg(test)` lexer is a second tokenizer.** Integration tests through real `proc_macro` (`tests/derive.rs`) cover the divergence risk.

VERDICT: BLOCKED
