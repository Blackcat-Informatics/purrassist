# Validation

## Task 1

### One-time baseline check (not in make check)

Runs the banned-phrase rule (and the .stage rule) of scripts/check-docs.py over the f11b45e versions of README.md, docs/RELEASE.md, docs/QUALIFICATION.md, docs/IMPLEMENTATION.md and docs/FUZZING.md, extracted with git show into a temp directory.

```sh
#!/bin/bash
# One-time baseline: run the banned-phrase rule over the f11b45e versions of five documents.
set -eu
dir=$(mktemp -d)
for path in README.md docs/RELEASE.md docs/QUALIFICATION.md docs/IMPLEMENTATION.md docs/FUZZING.md; do
  mkdir -p "$dir/$(dirname "$path")"
  git show "f11b45e:$path" > "$dir/$path"
done
python3 - "$dir" <<'PY'
import importlib.util, sys
spec = importlib.util.spec_from_file_location('check_docs', 'scripts/check-docs.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
names = ['README.md', 'docs/RELEASE.md', 'docs/QUALIFICATION.md', 'docs/IMPLEMENTATION.md', 'docs/FUZZING.md']
docs = module.Docs(sys.argv[1], names)
report = module.Report()
module.check_banned_phrases(docs, report)
module.check_stage_references(docs, report)
print('\n'.join(report.errors))
print(f'{len(report.errors)} errors')
PY
rm -rf "$dir"
```

Output of bash /tmp/baseline.sh:

```
[banned-phrase] README.md:387: stale or process-bound wording 'nine coverage-guided targets' (/nine (mutation |coverage-guided )?targets/)
[banned-phrase] README.md:17: stale or process-bound wording 'follows review and merge' (/follows? (the )?review and merge/)
[banned-phrase] README.md:14: stale or process-bound wording 'Pre-release' (/Pre-release/)
[banned-phrase] docs/FUZZING.md:967: stale or process-bound wording 'stage report' (/stage report/)
[banned-phrase] docs/IMPLEMENTATION.md:5: stale or process-bound wording 'issue branch' (/issue branch/)
[banned-phrase] docs/IMPLEMENTATION.md:7: stale or process-bound wording 'follows review and merge' (/follows? (the )?review and merge/)
[banned-phrase] docs/QUALIFICATION.md:33: stale or process-bound wording 'Nine targets' (/nine (mutation |coverage-guided )?targets/)
[banned-phrase] docs/QUALIFICATION.md:168: stale or process-bound wording 'nine mutation targets' (/nine (mutation |coverage-guided )?targets/)
[banned-phrase] docs/QUALIFICATION.md:23: stale or process-bound wording 'wire v2' (/wire v2/)
[banned-phrase] docs/QUALIFICATION.md:226: stale or process-bound wording 'reviewed merge commit' (/reviewed merge commit/)
[banned-phrase] docs/QUALIFICATION.md:7: stale or process-bound wording 'follow the review and merge' (/follows? (the )?review and merge/)
[banned-phrase] docs/QUALIFICATION.md:224: stale or process-bound wording 'after review and merge' (/after review and merge/)
[banned-phrase] docs/QUALIFICATION.md:142: stale or process-bound wording 'proved by the `windows-engines` hosted' (/proved by the [^.]*hosted/)
[banned-phrase] docs/RELEASE.md:6: stale or process-bound wording 'reviewed merge commit' (/reviewed merge commit/)
[banned-phrase] docs/RELEASE.md:43: stale or process-bound wording 'no placeholder record' (/no placeholder record/)
[banned-phrase] docs/RELEASE.md:218: stale or process-bound wording 'not yet done' (/not yet done/)
[banned-phrase] docs/RELEASE.md:83: stale or process-bound wording 'five of six' (/five of six/)
17 errors
```

Every inventory site is flagged, including the wrapped sites (the reported line is the line on which the phrase begins):

| Inventory site | Flagged | Phrase |
| --- | --- | --- |
| README 14 | yes (14) | Pre-release |
| README 14-17 | yes (17) | follows review and merge |
| README 387 | yes (387) | nine coverage-guided targets |
| RELEASE 5-7 (wrapped 6-7) | yes (6) | reviewed merge commit |
| RELEASE 43-62 | yes (43) | no placeholder record |
| RELEASE 83 | yes (83) | five of six |
| RELEASE 212-218 | yes (218) | not yet done |
| QUALIFICATION 7-8 (wrapped) | yes (7) | follow the review and merge |
| QUALIFICATION 23 | yes (23) | wire v2 |
| QUALIFICATION 33 | yes (33) | Nine targets |
| QUALIFICATION 142-143 (wrapped) | yes (142) | proved by the `windows-engines` hosted |
| QUALIFICATION 168 | yes (168) | nine mutation targets |
| QUALIFICATION 224 | yes (224) | after review and merge |
| QUALIFICATION 226-227 (wrapped) | yes (226) | reviewed merge commit |
| IMPLEMENTATION 5 | yes (5) | issue branch |
| IMPLEMENTATION 5-7 | yes (7) | follows review and merge |
| FUZZING 967 | yes (967) | stage report |

Supplementary: the whole checker over a pristine export of f11b45e (git archive), with the new docs/known-limitations.json added, reports violations from every rule family that the inventory implies.

```sh
#!/bin/bash
# Supplementary: the whole checker over a pristine f11b45e export, with the new known-limitations source added.
set -eu
dir=$(mktemp -d)
git archive f11b45e | tar -x -C "$dir"
cp docs/known-limitations.json "$dir/docs/"
python3 - "$dir" <<'PY'
import collections, importlib.util, subprocess, sys
spec = importlib.util.spec_from_file_location('check_docs', 'scripts/check-docs.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
listing = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', '-z', 'f11b45e'], text=True)
names = [n for n in listing.split('\0') if n.endswith('.md') and not n.startswith(module.EXCLUDED_PREFIXES)]
errors = module.check(sys.argv[1], names)
print(dict(collections.Counter(e.split(']')[0] + ']' for e in errors)))
PY
rm -rf "$dir"
```

```
{'[workspace-version]': 5, '[fuzz-targets]': 3, '[receipt-version]': 1, '[banned-phrase]': 22, '[archived-evidence]': 3, '[stage2-heading]': 1, '[windows-run]': 2, '[known-limitations-section]': 2, '[fuzz-row-limits]': 4, '[fuzz-baseline-table]': 1}
```

### Focused checks on the corrected tree

```
$ python3 scripts/check-docs.py
Documentation facts, links, wording and known limitations match their sources.
$ python3 scripts/check-repository.py
Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
$ python3 scripts/test-check-docs.py
----------------------------------------------------------------------
Ran 87 tests in 17.372s

OK
$ cargo test -p purrassist-core --test tool_call_dialects

test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.40s

```

## Task 2

### One-time README migration check (not in make check)

Extracts every backticked token (inline code spans outside fenced blocks) and every link target (inline links, reference definitions, `src`/`href`) from `git show f11b45e:README.md`. A token passes when it occurs as text in the new `README.md` or in the hand-written part of `docs/TOOLS.md` (the generated block is excluded so that it cannot satisfy the check by accident). A relative link passes when its repository-relative, normalized target is linked from either file. Run from the repository root:

```sh
python3 /tmp/migration/check.py
```

```python
import posixpath, re, subprocess, sys
OLD = subprocess.check_output(['git', 'show', 'f11b45e:README.md'], text=True)
README = open('README.md').read()
TOOLS = open('docs/TOOLS.md').read()
begin, end = '<!-- tools-reference:begin -->\n', '<!-- tools-reference:end -->\n'
narrative = TOOLS[:TOOLS.index(begin)] + TOOLS[TOOLS.index(end) + len(end):]

def blank_fences(text):
    return re.sub(r'^(```|~~~).*?^\1[^\n]*$', lambda m: re.sub(r'[^\n]', ' ', m.group(0)), text, flags=re.M | re.S)

def tokens(text):
    return sorted(set(re.findall(r'`([^`\n]+)`', blank_fences(text))))

def targets(text, base):
    t = blank_fences(text)
    found = re.findall(r'\]\(\s*(<[^>]*>|[^)\s]+)', t) + re.findall(r'^\s*\[[^\]]+\]:\s*(\S+)', t, re.M) + re.findall(r'\b(?:src|href)="([^"]+)"', t)
    out = set()
    for f in found:
        f = f.strip('<>')
        if re.match(r'[A-Za-z][A-Za-z0-9+.-]*:', f):
            out.add(f)
        else:
            path, _, frag = f.partition('#')
            norm = posixpath.normpath(posixpath.join(base, path)) if path else base_file[base]
            out.add(norm + ('#' + frag if frag else ''))
    return out
base_file = {'': 'README.md', 'docs': 'docs/TOOLS.md'}
new_text = README + '\n' + narrative
old_tokens = tokens(OLD)
old_targets = targets(OLD, '')
new_targets = targets(README, '') | targets(narrative, 'docs')
missing_tokens = [t for t in old_tokens if t not in new_text]
missing_targets = sorted(t for t in old_targets if t not in new_targets)
print(f'old README: {len(old_tokens)} backticked tokens, {len(old_targets)} link targets')
print(f'tokens absent from new README + docs/TOOLS.md narrative: {len(missing_tokens)}')
for t in missing_tokens: print('  TOKEN', repr(t))
print(f'link targets absent: {len(missing_targets)}')
for t in missing_targets: print('  LINK', t)
```

Output:

```text
old README: 248 backticked tokens, 14 link targets
tokens absent from new README + docs/TOOLS.md narrative: 2
  TOKEN 'call TOOL --root DIRECTORY'
  TOKEN 'make fuzz TARGET=json'
link targets absent: 0
```

### Deliberate removals

| Old token | Reason |
|---|---|
| `make fuzz TARGET=json` | The f11b45e sentence ("Coverage-guided JSON campaigns are available with `make fuzz TARGET=json`") was replaced in Task 1 (dbdc64d) by the ten-target statement, which `check-docs.py` reads from `TARGETS`. The new README keeps the form `make fuzz TARGET=<target>` with the example `TARGET=json`, so no capability is dropped. |
| `call TOOL --root DIRECTORY` | Respelled `purrassist call TOOL --root DIR` in the three-interfaces block and in `docs/TOOLS.md` (the placeholder is `DIR` in every generated entry). The claim that it reads a JSON argument object from stdin and invokes the same operation used by MCP is kept. |

No link target was removed. Every other old token appears in the new README or in the `docs/TOOLS.md` narrative.

## Task 3

Read-only publish preflight (reads crates.io only), run 2026-10-12:

```text
$ python3 scripts/check-release.py --preflight-owners --user-agent 'purrassist-release (https://github.com/Blackcat-Informatics/purrassist)'
Publish preflight passed: 6 crates exist and share owners paudley
exit 0
```

Focused checks (all exit 0):

```text
$ python3 scripts/check-docs.py && python3 scripts/test-check-docs.py && python3 scripts/check-repository.py && python3 scripts/check-release.py && python3 scripts/test-check-release.py
Documentation facts, links, wording and known limitations match their sources.
Ran 167 tests ... OK
Repository attribution, frozen schema data, dependency inventory, core and derive boundaries verified.
Release set and dependency order verified for 0.1.0 (six crates in RELEASE_ORDER)
test-check-release.py: OK
```


## Task 4

Social preview reproduction (render to a temporary file with the recorded command; compare with the committed PNG and `docs/assets.json`):

```
$ rsvg-convert --version
rsvg-convert version 2.63.2

$ fc-match -f "%{family}|%{style}|%{file}\n" "Noto Sans"
Noto Sans|Regular|/usr/share/fonts/noto/NotoSans-Regular.ttf
$ fc-match -f "%{family}|%{style}|%{file}\n" "Noto Sans:bold"
Noto Sans|Bold|/usr/share/fonts/noto/NotoSans-Bold.ttf
$ sha256sum fonts
478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823  /usr/share/fonts/noto/NotoSans-Regular.ttf
1df075a380fc7cb898acf64c1f7b3b4dd780de3caa860178bf929de35817a913  /usr/share/fonts/noto/NotoSans-Bold.ttf
$ rsvg-convert -w 1280 -h 640 docs/social-preview.svg -o $TMP/render.png
$ sha256sum $TMP/render.png docs/social-preview.png
a1045507f838b2b314b459ac5c1a27d4412526e5457c07dff0996413b85d3338  /tmp/tmp.lHdc703y97/render.png
a1045507f838b2b314b459ac5c1a27d4412526e5457c07dff0996413b85d3338  docs/social-preview.png
byte-identical: PASS
$ stat -c %s docs/social-preview.png
112623
```

Result: the render is byte-reproducible (sha256 `a1045507...3338` equals the committed PNG and the `docs/assets.json` output digest). `fc-match` resolves "Noto Sans" and "Noto Sans:bold" to Noto Sans Regular and Bold (font Version 2.015), so no substitution occurred. The Arch `noto-fonts` package supplied the files; it ships an Apache-2.0 LICENSE file, but the fonts' own name table declares the SIL OFL 1.1, which is what is registered and vendored.

## Task 6: full local gate (single run on final head)

- Head: `f0d978a37f2f8a9388646311ce608e40df077d4c` (clean tree apart from untracked `.stage/`)
- `make check`: exit 0, 2m13s; 110 `test result: ok` groups; no warnings or failures (log: /tmp/purr-make-check.log)
- `make package`: exit 0, 18.6s; "Verified 6 release archives". Archive sha256:
  - purrassist-core-0.1.0: b22111e2aea43655964c43bc33a19a5c286bc85ce318b8e913b007c8fa7e8fce (2898988 bytes)
  - purrassist-derive-0.1.0: 3c41b8646698fc3960ade9d9b088d567fcd812ac6816231d29f39c31818d57ff (63566 bytes)
  - purrassist-syntax-0.1.0: 3c3b01a8179f0f04eed042fb56f18a4ae51a0691a835a502e11f6ac669bf0347 (36315 bytes)
  - purrassist-0.1.0: 277d992c54d6167bd28b048a5a506c3ddbc9419013108a243edf11fe77b4b3fe (15121 bytes)
  - purrassist-host-0.1.0: cff06bfdb0cbadbde6f96844d9eb97062c596cac4ce1830f591d61b7dc36efd1 (110830 bytes)
  - purrassist-cli-0.1.0: 346c780c03e3048d77cf75fd8c78d4df382301bba277f14db6b5ac8ecc0c05f7 (41718 bytes)
- Packaged `purrassist-cli-0.1.0/README.md` line 103: visible `mcp-name: io.github.Blackcat-Informatics/purrassist`; no "Pre-release" text.
- Hosted CI: out of scope for this stage (owner decision).

### Rerun after the completeness-review fix (final head)

The completeness review found a publication overclaim ("Six crates are published") and two wording lines. Commit `a42a817` fixes them, which invalidated the run above, so both gates were rerun on the final head `a42a817af4f19a5dfc95fcd484f63f8f2beb9f26`:
- `make check`: exit 0; 110 `test result: ok` groups (log: /tmp/purr-make-check2.log)
- `make package`: exit 0; "Verified 6 release archives" (log: /tmp/purr-make-package2.log)

---

# Stage 2 (PR #20)

## Review path: lightweight

Reason: the diff is documentation, crate metadata, one binary asset, and stdlib Python doc checkers with tests, plus three new Rust integration tests. No production Rust source changed (`crates/*/src` is untouched), and there is no security, unsafe, concurrency or migration surface. One combined `gap-analyzer` (opus/high) covers gaps, feedback, consumer wiring and the completion rubric, writing `gap-analysis.md`.

Unused conditional specialists:
- quality-engineer: the combined reviewer covers checker edge cases, and every rule already has a failing fixture.
- security-engineer: no untrusted input, unsafe code, network, auth or secret paths changed.
- performance-engineer: no hot path changed. The checker adds about 20 s to `make hygiene`, measured within the 2m13s gate.
- system-architect: no new or reworked modules in shipped crates.

## Review brief

`stagectl brief --mode review` crashed on the binary PNG (UnicodeDecodeError at stagectl/brief.py:76, decoding `git show HEAD:docs/social-preview.png`). `branch-under-review.md` was written by hand with the same git commands; the gate review-brief passed. PR feedback is captured in `raw/pr-20*.json`: 2 owner comments, 0 reviews, 0 inline comments, 0 threads.

## Hosted CI observation (out of scope by owner decision; recorded, not acted on)

Verify run on `a42a817`: native ubuntu/macos, msrv, wasm, corpus and qualification passed. Two jobs failed, both in code this branch does not touch (`git diff origin/main...HEAD -- crates/core/src scripts/fuzz.py vendor/dependencies.json` is empty):
- windows-engines: clippy `too_long_first_doc_paragraph` at crates/core/src/grammar/cost.rs:93, tool_call/emit.rs:1130 and tool_call/ir.rs:600, which came from earlier tool-call merges.
- fuzz: "node version differs from inventory 26.11.1" from the runner toolchain.

## Stage 2 G11

Regeneration through the pinned generator (inputs fetched into `.cache/unicode`, verified against `vendor/unicode/manifest.json`):

```
$ python3 scripts/generate-unicode.py --fetch
generated 5914 decompositions, 961 compositions, 1585 folds, 6565 confusables, 175 scripts, 53 binary properties, 9262 IDNA mapping rows, 1267 Bidi_Class ranges, 932 Joining_Type ranges
```

Diff of generated data touches only the `idna_data.rs` header comment and `reason` line, plus that file's digest in `vendor/unicode/generated.json` (`ae3f0615...` to `dd3001c1...`). Both `unicode_data.rs` files are unchanged.

Regeneration check that can fail: result staged, generator rerun, then require a clean diff.

```
$ git add crates/core/src/text vendor/unicode
$ python3 scripts/generate-unicode.py --fetch
$ git diff --exit-code crates/core/src/text vendor/unicode ; echo $?
0
```

Negative control: with "Task 7" put back in the generator template and the generator rerun against the staged result, `git diff --exit-code --stat` reports `idna_data.rs | 2 +-` and `generated.json | 2 +-` and exits 1. The template was restored and the data regenerated (byte-identical to the staged result).

Focused checks, all passing: `check-docs.py`, `test-check-docs.py` (248 tests), `check-repository.py` with `-X warn_default_encoding -W error::EncodingWarning`; `make hygiene`; `cargo clippy -p purrassist-core -p purrassist-host --all-targets -- -D warnings`; `cargo doc -p purrassist-core -p purrassist-host --no-deps`; `cargo fmt --all --check`; the `Task N` git grep over all six `src/` trees is empty.

Follow-up to G11: the `source-jargon` rule also bans `\brequirements? \d+\b` in published `src/` and collapses each comment run before matching, so a reference wrapped across comment lines is found at its first line. `git grep -niE '\brequirements?\s+[0-9]+'` over `crates/*/src` is empty, and all focused checks pass again (`test-check-docs.py`: 251 tests, OK).

## Stage 2 final gate (single full run after the fixes settled)

Final head: `93c02b3a5836f06b15335e428b8cd1a703cca6db`. The tree was clean apart from untracked `.stage/`.

- `make check`: exit 0, 4m45s (machine under concurrent review load); 110 `test result: ok` groups; no failures (log: /tmp/purr-s2-check.log). This includes `make hygiene` with EncodingWarning as an error on all seven script lines.
- `make package`: exit 0, 31s; "Verified 6 release archives" for source commit `93c02b3` (log: /tmp/purr-s2-package.log).

Final completion verdict: `gap-analysis.md`, "Final completion verdict", which reads **VERDICT: PASS**. It covers M1, M2, L1–L12, N-1–N-7 and G11, all FIXED. Its condition is that the run above exits 0 on `93c02b3`, which is satisfied. The final per-criterion demonstrations are in that report's table.

What it covers and its limits:
- Local validation only.
- Hosted CI is out of scope by owner decision. The two observed hosted failures (windows-engines clippy in untouched core code, and the fuzz runner's Node version) are recorded above and not acted on.
- The social-preview upload remains an owner step.
- The tag, publication and downloaded-package acceptance are not run, and need separate authorization.

Stage 2 commits: `4933cc4`, `d6fb1e0`, `feefaae`, `93c02b3`.

---

# Stage 3 (PR #20)

## Integration assessment

- Remote PR base `origin/main` = `f11b45ece8dbf8b83f9d427fb4949d0ca2f02350`, fetched at Stage 3 start. It is the branch point and an ancestor of head `93c02b3a5836f06b15335e428b8cd1a703cca6db`.
- `git merge-tree --write-tree --no-messages origin/main HEAD` exits 0 with tree `8b2f7182a3ac3675d1d222713bdebfec81d05c28`, identical to `HEAD^{tree}`. The squash candidate is byte-for-byte the tested head tree.
- Semantic assessment: the base-side change set is empty (main has not advanced since the branch point), so there are no shared consumers or build configuration from the other side to combine. No separate semantic agent was dispatched for an empty delta. The branch-side assessment is the Stage 2 combined review (`gap-analysis.md`, final VERDICT PASS).
- Evidence reuse: the Stage 2 final `make check` (exit 0, 110 test groups) and `make package` (exit 0, six archives verified for source commit `93c02b3`) ran on exactly this head and tree. No rerun is needed; nothing changed since.
- Branch policy (`main` ruleset 24943594, read via API): deletion and non-fast-forward are blocked; linear history and signatures are required; pull requests are squash-only with 0 required approvals and no required thread resolution. **No required status checks.**

## Stage 3 binding gates

- Review debt: `review-debt.md`, VERDICT PASS. There are 4 owner status comments, no reviews, inline comments, threads or bot feedback, and no required checks. The non-required `windows-engines` and `fuzz` failures are not caused by this branch. The fuzz job in `verify.yml` has no pinned-Node install step, a pre-existing workflow gap outside this diff.
- Completion audit, reused: the final Stage 2 combined verdict (`gap-analysis.md`, "Final completion verdict", PASS) was judged at `93c02b3`, conditional on the final gate. That gate passed on `93c02b3`. The PR head is still `93c02b3`, the base has not moved, the merge-tree candidate equals the head tree, and no new feedback or criteria arrived. The audit therefore applies unchanged to the merge candidate, and no fresh Stage 3 auditor is required.
