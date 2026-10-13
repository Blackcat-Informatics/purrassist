# Plan enhancement audit: issue #26

Reviewed plan: `/home/paudley/Active/purrassist/.worktrees/26-lint-the-whole-workspace-with-the/.stage/lint-the-whole-workspace-with-the/plan.md`

Inputs read: `.goals`, `AGENTS.md`, `issue-analysis.md`, `prior-art-assessment.md`, `verify.yml`,
`release-qualification.yml`, `publish.yml`, `Makefile`, `rust-toolchain.toml`, `Cargo.toml`,
`scripts/check-release.py`, `scripts/test-check-release.py`, `scripts/check-docs.py` (workflow-step
rules), `scripts/package-workspace.py`, `scripts/test-fuzz-feedback.py`, `scripts/test-fuzz.py`,
`scripts/check-wasm.py`, `docs/RELEASE.md`, `docs/QUALIFICATION.md`, `docs/IMPLEMENTATION.md`. No web
tools were used; the question is answered from the repository and first principles.

## The concept the feature is a shadow of

The issue asks for one more step in one more job. The thing it is a shadow of is **gate identity**:
every `main` commit is a release candidate, so the tag must be able to add nothing but the tag
check. Formally, `Gate_release ⊆ Gate_PR` on the **same toolchain**, with the toolchain a single
registered identity. The plan establishes this for one step (`make check`) and detects drift of one
pin in two files. The release-qualification workflow has five steps that run on the release
toolchain; the plan covers one of them. The other four are the same defect class as #26 (a
command whose first run on the release toolchain is at tag time):

| release-qualification.yml step | PR lane today | On release toolchain in PR CI? |
|---|---|---|
| `make check` | `native` (nightly), `msrv` (tests only) | **no** -> plan Task 1 fixes |
| `make corpus` | `corpus` job (nightly) | **no**: `test = false` suites compile with `-D warnings` on 1.98.0 only at tag time |
| `scripts/test-fuzz-feedback.py` | `fuzz` job via `make fuzz-hosted` | yes: the script self-pins the nightly from `rust-toolchain.toml` (line 27-30), so both lanes are identical. DISCHARGED by inspection |
| `scripts/test-verify-published.py` | `native` (nightly) | **no**: it runs `cargo package --workspace --locked` and builds the CLI on the ambient toolchain (line 271) |
| `make package` | `qualification` job (nightly) | **no**: `package-workspace.py` resolves `rustup which cargo` (line 53-54), so the fresh-`CARGO_HOME` `cargo package` verify build of all six crates on stable happens only at tag time, and again in `publish.yml` as `cargo publish --locked` |

Nothing in `make check` needs the nightly: no `#![feature]` in `crates/`, no `rustfmt.toml`,
`check-wasm.py` and `test-fuzz.py` use the ambient toolchain. Only the coverage probe needs the
nightly (its `llvm-tools` must match the registered `llvm-coverage` 23.1.1) and it already pins
itself. So the whole release gate is runnable on the release toolchain in a PR job today.

## Items

1. **TRANSFORMATION: make the release gate one named artifact that both lanes execute.**
   Add `make release-gate` (`check corpus` + `python3 scripts/test-fuzz-feedback.py` +
   `python3 scripts/test-verify-published.py` + `package`, in the order release-qualification.yml
   uses), have `release-qualification.yml`'s `package` job run `check-release.py --tag` then
   `make release-gate`, and have the PR job (the renamed `msrv`, see below) run exactly
   `make release-gate` under the same pin. Drift of the *step list* then becomes impossible by
   construction, and the check-release.py rule (plan R4) shrinks to one fact: both jobs run
   `make release-gate` with the same `RUSTUP_TOOLCHAIN`. Rename `msrv` to `release-gate` since
   after Task 1 it is no longer "MSRV tests" (QUALIFICATION.md line 252 says "stable MSRV workspace
   tests"); key the rule on the new name. Give the job `timeout-minutes: 90` like `qualification`
   and the release job, and install the nightly with `llvm-tools` as release-qualification.yml
   line 32 does (test-fuzz-feedback.py needs it).
   Cost: **M** (Makefile target, two workflow edits, one rename, docs; the PR job grows by corpus
   and package time, which the `.goals` "hard fails, maximal utility" line accepts).
   Payoff: **decisive**. Closes the class, not the instance: "make corpus failed on the tag",
   "cargo package failed on the tag", and "a step added to release-qualification.yml is never run
   on PRs" all become impossible. The next three issues in this area do not arrive.
   Changes the plan at: Governing decisions "The PR lane must use the same toolchain and the same
   command, `make check`" (same command becomes `make release-gate`); Task 1 file list; R4; the
   docs step list at RELEASE.md lines 122-135 (check-docs.py's `release-qualification-steps` rule
   will enforce the rewrite); "Enhancements considered and declined" never considered the other
   release-only steps, which the launching agent explicitly asked about.
   Considered and not recommended: flipping `rust-toolchain.toml` to 1.98.0 so local `make check`
   is the release gate. IMPLEMENTATION.md line 154 states the policy "nightly development
   analysis, stable release builds", and the four recorded breaks show the two lint sets are not
   ordered (stable flags what nightly does not, and vice versa), so running both is the maximal
   answer; item 3 gives developers the local leg.

2. **TRANSFORMATION: single-source the two toolchain identities and check every occurrence.**
   `1.98.0` appears in `verify.yml` (4x), `release-qualification.yml` (2x), `publish.yml` (lines 72
   and 78: the `cargo publish --locked` verify build) and `docs/RELEASE.md`; `nightly-2026-09-14`
   appears 7x in `verify.yml`, 1x in `release-qualification.yml`, in `rust-toolchain.toml` and
   `docs/FUZZING.md`. Plan R4 compares only the `msrv` job with `release-qualification.yml` and
   `rust-version`. It omits `publish.yml`, which is the worst possible drift (the qualified build
   and the published build on different compilers), omits `windows-engines`, and omits the nightly
   pin entirely (a nightly bump that misses one job is the same drift class). Register the release
   pin in one place (`[workspace.metadata.release] toolchain = "1.98.0"` in `Cargo.toml`, readable
   through the `cargo metadata` call check-release.py already makes at line 242; or
   `vendor/tooling.json`, where every other build tool is registered per AGENTS.md), keep
   `rust-toolchain.toml` as the nightly's single source, and make the hygiene rule generic: every
   `RUSTUP_TOOLCHAIN:` and every `rustup toolchain install` line in every `.github/workflows/*.yml`
   names one of the two pins; every stable occurrence equals the release pin; every nightly
   occurrence equals `rust-toolchain.toml`'s channel; the release pin's `major.minor` equals the
   workspace `rust-version`. Fixtures: a pin changed in one file only, the install line differing
   from the job env (the `msrv` job already has both, lines 38 and 44), and `publish.yml` drift.
   Cost: **S-M**. Payoff: **high**: the next toolchain bump is one line, and hygiene proves every
   lane follows; a missed `publish.yml` can never ship a build nobody qualified.
   Changes the plan at: R4 "asserts that the `verify.yml` `msrv` job's `RUSTUP_TOOLCHAIN` equals the
   toolchain `release-qualification.yml` installs and the workspace `rust-version`" (generalise to
   all workflows and both pins); Task 1 "the drift rule (R4), with failing fixtures".

3. **UTILITY: a local, reproducible release-gate command, and remove the phantom tool.**
   The plan's focused checks and Task 2 step 2 cite `rust-validate 1.98.0 make check`; no such
   tool exists anywhere in the repository (grep: zero hits), so the plan's own evidence is not
   reproducible by another contributor. The real command is
   `RUSTUP_TOOLCHAIN=<release pin> make release-gate` (or `make check` before item 1). Add it to
   QUALIFICATION.md "Qualification commands" (lines 239-250, which list no toolchain), and to
   RELEASE.md's pre-tag step 2, so a developer can run the tag-time gate at the desk. Commit
   598d059's message names the real root cause as "local `make check` uses nightly and did not
   catch it"; this is the leg that addresses it.
   Cost: **S**. Payoff: medium-high; also converts the plan's evidence into something anyone can
   rerun. Changes the plan at: Task 1 "Focused checks" line 1 and Task 2 step 2 (replace
   `rust-validate 1.98.0` with the environment-variable form); R5.

4. **UTILITY/ROBUSTNESS: make the gate's sensitivity executable on every PR, not once.**
   Task 2 demonstrates R3 one time with a draft PR, then deletes it. A gate that always passes is
   indistinguishable from one that is disabled (a stray `#![allow(clippy::pedantic)]`, a
   downgraded `[workspace.lints.clippy]`, or `make lint` losing `-D warnings` would all pass
   silently). Add a small sensitivity probe to the release-gate lane: a script that writes a
   throwaway crate using the workspace lints into a temp dir outside the checkout, runs
   `cargo clippy --locked -- -D warnings` on the active toolchain, and **requires** `float_cmp` in
   the output. It runs only in the release-gate job (it needs the release toolchain), hard-fails,
   and is the standing executable evidence AGENTS.md asks for ("Capability claims require
   executable evidence"); the one-time draft-PR run URL remains the issue's literal acceptance.
   Cost: **M**. Payoff: medium-high. Changes the plan at: Task 2 (keep, as acceptance evidence) and
   Task 1 file list (new `scripts/test-release-gate.py` plus a `make` hook in `release-gate`).

5. **ROBUSTNESS: the `rust-version` comparison in R4 is wrong as written.**
   R4 says the toolchain "equals ... the workspace `rust-version`". `Cargo.toml` line 10 is
   `rust-version = "1.98"`; the pin is `1.98.0`. The rule must compare `major.minor` (so a
   `1.98.1` release toolchain still matches), and the fixture "release toolchain bump without the
   PR lane" must include the `rust-version` bump case (`1.99` in Cargo.toml with `1.98.0` pins).
   Cost: **S**. Payoff: the rule works on day one. Changes the plan at: R4 text.

6. **ROBUSTNESS: do not over-claim what the probe shows.**
   R2 is worded "a stable-only lint fails on the PR". The probe `assert_eq!(0.1_f64 + 0.2, 0.3)`
   demonstrates the issue's literal acceptance (R4 in issue-analysis.md: an exact float
   `assert_eq!` fails a hosted PR job with `float_cmp`); whether the pinned nightly also flags that
   exact form is not established in the plan, and `float_cmp` is in `pedantic`, which the
   workspace enables on both toolchains. Either record the `native` job's result on the probe PR
   alongside the `release-gate` result (so the stable-only property is observed, not asserted), or
   reword R2's acceptance to what the probe proves. Also note the probe PR runs the full
   `verify.yml` matrix including two 90-minute jobs; that is a cost, not a defect.
   Cost: **S**. Payoff: the completeness contract claims only what the evidence shows. Changes the
   plan at: Completeness contract R2 and Task 2 step 4.

7. **LEVERAGE: reuse check-docs.py's workflow-step parser instead of a third regex dialect.**
   `scripts/check-docs.py` already has `workflow_steps()` / `workflow_step_names()` (lines 738-791)
   and the `release-qualification-steps` rule that forces RELEASE.md to name every step. The plan
   adds a new workflow rule to `check-release.py`, which has its own text matcher for
   `publish.yml`. Whichever file hosts the drift rule, use one parser for "steps of job X in
   workflow Y" rather than a third ad-hoc regex; and expect `check-docs.py` to fail on the
   RELEASE.md step list the moment item 1 collapses steps, which is the intended enforcement of R5.
   Cost: **S**. Payoff: one less text-parser to keep correct. Changes the plan at: Task 1
   `scripts/check-release.py` bullet.

Items the plan already handles well and that need no change: `--locked` on `make lint`; keeping
`windows-engines`' host/cli Clippy (the only Windows `cfg` lint run); declining the required-status
setting as owner-side; `RUSTFLAGS: -D warnings` making the PR lane a strict superset (the Makefile
composes `$${RUSTFLAGS:-} -D warnings`, so nothing is masked).

Conflicts with `.goals`: none. Items 1 and 2 are the "LOW/NO OPTIONALITY, HARD FAILS" and
"MAXIMAL UTILITY" lines applied to CI.

## Verdict (pass 1)

The plan is sound for the single step the issue names, but the launching agent's question (are
there other release-only gates?) has a concrete "yes" (`make corpus`, `test-verify-published.py`,
`make package` on the release toolchain) that the plan neither covers nor declines, its drift rule
omits `publish.yml` and the nightly pin, its R4 comparison is wrong as written, and its focused
checks cite a tool that does not exist. Items 1, 2, 3 and 5 need adoption or a technical
disposition before the plan is maximal.

VERDICT: BLOCKED

## Pass 2 (plan revision 2)

Reviewed: the same plan path, revision 2 (header "Revision 2 ... The enhancement audit's
TRANSFORMATION, *gate identity*, restructures the plan").

### Disposition of pass-1 items

1. **DISCHARGED.** Governing decisions now name the defect class as gate divergence and list all
   five release-toolchain gates; Task 1 adds `make release-gate` (`check`, `corpus`,
   `test-fuzz-feedback.py`, `test-verify-published.py`, `package`, in the release workflow's
   order), collapses release-qualification.yml steps 4-8 into one `Verify the release gate` step,
   renames `msrv` to `release-gate`, installs both toolchains (`clippy,rustfmt` and `llvm-tools`)
   and sets `timeout-minutes: 90`. P1 asserts the shape on both sides with a fixture per
   assertion. This is the construction I asked for.

2. **DISCHARGED.** P2 checks every stable pin occurrence (`env.RUSTUP_TOOLCHAIN` and every
   install line in `verify.yml`, `release-qualification.yml`, `publish.yml`), `major.minor`
   against `rust-version`, and every nightly occurrence against `rust-toolchain.toml`, with five
   named fixtures including `publish.yml`-only and install-line-only drift. The plan chose
   N-way equality plus the `rust-version` anchor instead of one registered literal for the stable
   pin; that is drift-equivalent (any bump must be made everywhere and hygiene proves it), and
   `rust-toolchain.toml` remains the nightly's single source. Acceptable.

3. **DISCHARGED.** `RUSTUP_TOOLCHAIN=1.98.0 make release-gate` is the documented local command
   (P3 adds it to QUALIFICATION.md's command list); `rust-validate` is explicitly demoted to a
   session convenience, not repository evidence. Verified that this command is coherent end to
   end: `test-fuzz-feedback.py` overrides `RUSTUP_TOOLCHAIN` to the nightly itself, and
   `package-workspace.py` resolves `rustup which cargo`, which honours the variable.

4. **DISCHARGED, with an advisory.** The disposition ("a deliberately failing lint can't live in
   the tree") answers an in-tree design I did not propose; the proposed probe compiles a
   throwaway crate outside the checkout. But the plan's substitute is technically sound: the
   hosted probe is executable sensitivity evidence at the Task 1 head, and Task 3 step 3 reruns
   it whenever `verify.yml` or `Makefile` change before merge. The residual (a later PR
   downgrading `[workspace.lints.clippy]` or dropping `-D warnings` from `make lint` would not
   fail anything) is real but is a different change's review. Advisory, S cost, no plan change
   required: P1 could additionally assert that the Makefile `lint` recipe is exactly
   `cargo clippy --workspace --all-targets --locked -- -D warnings`, which pins R1's command
   structurally rather than only by the one-time log.

5. **DISCHARGED.** P2 compares `major.minor` to `"1.98"` and includes a `rust-version` mismatch
   fixture.

6. **DISCHARGED, better than asked.** The probe is now the pre-#24 runtime comparison in
   `crates/core/src/tool_call/from_node.rs`, which the pinned nightly provably accepted at
   `464306c` and which is true at runtime, so only the 1.98.0 Clippy gate can fail it; R3's
   acceptance requires `native` to pass on the same probe PR, so the stable-only property is
   observed rather than asserted. It is still an exact float `assert_eq!` in a core test, so the
   issue's literal acceptance (ledger R4) holds.

7. **DISCHARGED.** The disposition is terse ("check-docs.py's existing workflow-step parser is
   reused for the new single step") and it is not clear whether `check-release.py`'s P1 rule will
   load that parser or `check-docs.py` simply continues to enforce the RELEASE.md step list. Either
   satisfies the intent (no third parser for step lists, and RELEASE.md forced to follow the
   collapsed step). Not blocking.

### New design introduced in revision 2

8. **ROBUSTNESS (advisory, S): the one remaining lane divergence is the `RUSTFLAGS` env.**
   Task 1 keeps `RUSTFLAGS: -D warnings` on the PR `release-gate` job "as before", while the
   release job sets none. With `make release-gate` this now also reaches `package-workspace.py`
   (which copies `os.environ` into its fresh `CARGO_HOME` builds), `test-verify-published.py`'s
   `cargo package --workspace`, and `fuzz.py`'s coverage build, none of which has run with that
   flag in any lane before. It cannot mask a release failure (strict superset), but it is exactly
   the "PR lane is not literally the release lane" shape item 1 was adopted to eliminate, and it
   prevents P1 from asserting the two jobs' `env:` blocks are equal. Either drop the job-level
   `RUSTFLAGS` (the Makefile already composes `-D warnings` into `test`, `corpus`, `doc`, and
   `lint` passes `-- -D warnings`) or add the same line to the release job, then let P1 assert
   env equality. Changes the plan at: Task 1 "it keeps `RUSTFLAGS: -D warnings`, a strict
   superset, as before". Not blocking; identity is simply stronger than superset.

No other new design concerns: the probe PR's `native` pass is deterministic because the nightly
is a dated pin; the collapsed release step is caught by `check-docs.py`'s
`release-qualification-steps` rule as P3 expects; the 90-minute allowance matches the release job
that already ran the same five gates in run 38356797286.

### Verdict (pass 2)

Every pass-1 item is adopted or dispositioned with a technical reason. Items 4 and 8 remain as
S-cost advisories that tighten P1 and do not change the delivered capability.

VERDICT: PASS
