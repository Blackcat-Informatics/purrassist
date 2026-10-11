# R1 review (F-1, F-2, F-3, F-6)

Verified by execution against uncommitted changes on HEAD e4850a8.

- Name check: standalone crate with name `bad {name} {} {{ }}` and param `p {x}` gives the intended E0080 message verbatim, with a user `macro_rules! assert` in scope (check still fires). Emitted form is `::core::assert!(cond, "{}", "<msg>")`.
- Hygiene: `-Zunpretty=expanded` of a derive using defaults, extra, nested, enum, repeat and pattern, under a module shadowing bool/usize/str/u8/Option/Result/Some/None/Ok/Err/Vec/String/Box/Node/Value/Member/Default/Into/From/Clone, `mod core/std/alloc/purrassist_core {}` and shadowing assert/format/write macros: compiles; no bare primitive or std/core/purrassist_core name in generated output (all `::`-prefixed).
- Placeholder-named fields and variants (`__pa_splice_0`, `__pa_node`, `__pa_v0`, `__pa_f0`, `__pa_extra`, `__pa_value`): spliced from original tokens; in-repo hygiene test compiles and converts.
- Non-ASCII derived parameter name: spanned derive-time error at the field (`größe`), message asks for `#[param(name = "..")]`; explicit non-ASCII name still fails via E0080.
- `cargo test -q -p purrassist-derive -p purrassist --locked`: pass; `--doc`: pass (45 doctests); clippy `--all-targets -D warnings`: clean; `make hygiene`: pass.

Notes (non-blocking): the hygiene test's `assert!(false)` is deliberate under the shadowing `assert!` macro; fine.

VERDICT: PASS
