# T4 review (independent)

All commands run in the worktree against uncommitted changes over 4085bf3. All exit 0:
check-release, check-repository, test-repository-audit (29 tests), test-package-workspace (8),
test-verify-published (10), make hygiene, make helper-check,
cargo test -q -p purrassist-derive -p purrassist --locked (42 passed), cargo test --doc -p purrassist --locked (4 passed).

## Checklist
- NAMES, package-workspace comment, CRATE_NAMES/LIB_NAMES, dependencies_of multi-edge: done.
- Proc-macro fixture: `purrassist-derive` has `[lib] proc-macro = true`, the facade (`mid`) depends on it and
  expands `purrassist_derive::derived_marker!()`, so the install genuinely builds and uses a proc-macro crate
  from the local registry (test passes in ~16s).
- verify-published docstring "six"; packaged_notices.derive = {}; check-packages: derive held to zero
  normal/build/target-specific deps (also hardens core with build deps).
- check_derive_closure matches D-11 (normal+build empty; dev reaches only purrassist-core). Fixtures cover
  normal dep (with path), normal/build edge to core fails, build dep fails, dev->core passes, dev->third-party fails,
  missing package. Ambient scan covers crates/derive/src with derive label; core label preserved; real tree asserted non-empty.
- check-release RELEASE_ORDER and publish.yml already contained derive (verified by check-release).
- Docs: RELEASE.md order, derive name reservation + trusted publisher explicitly "not done / required before tagging";
  heading corrected to "five of six"; no residual "all names reserved" claim. INTEGRATION.md list matches every
  new core `pub` item (traits, max_depth, Members/Slot, FromNodeError ctors, is_valid_name const, hidden items, facade
  re-exports; 12 integer types verified). QUALIFICATION has evidence row; "Six" archives. No stale five-crate or #4 claims
  (remaining "five" hits are unrelated: markdown extensions, syntax grammars, historical placeholder text).
- README example compiled and ran in a scratch crate (path dep on the facade): OK.

## Judgement on parse.rs HashMap -> BTreeMap
Acceptable. The scanner intentionally keeps string-literal contents because a proc-macro crate emits code as
strings; skipping them would hide ambient access in generated code (the scanner's existing tests assert this).
The test edit is semantically neutral.

## Non-blocking notes
- Root README.md example is not a doctest (crates/purrassist/README.md, which is doctested, is separate and does not
  mention the derives); correctness verified manually only.
- Some doc lines exceed the prevailing wrap width (RELEASE.md, README, IMPLEMENTATION); cosmetic.

VERDICT: PASS

## Orchestrator disposition
- The root README example was not doctested (implementer's claim corrected). The same derive example is now in `crates/purrassist/README.md`, which is the facade's crate docs, so it runs: `cargo test --doc -p purrassist` goes from 4 to 5 doctests, all passing.
- Over-long retargeted "Still tracked" lines are rewrapped to the surrounding width. `make hygiene` exits 0.
