# Issue 181 report

Status: committed and pushed (head 71f3718) per the coordinator's holder ruling. No PR opened, no merge.

Head: 71f3718 (pushed to origin paudley/181-main-hygiene-is-red-since-174-four; base 961b824).

## Changes (staged)

- scripts/check-repository.py: `check_markdown_attribution` checks `<name>.license` for paths under `vendor/licenses/`; other Markdown keeps the header rule.
- scripts/test-repository-audit.py: `test_vendored_licence_needs_a_sidecar` (no sidecar fails; sidecar with both tags passes).
- Four sidecars, byte-format copied from b41458b (three lines: FileCopyrightText, License-Identifier, FileComment; jsonld has its FileComment replaced by the policy note):
  - vendor/licenses/purrdf-4.0.2/third-party/csvw/1-LICENSE.md.license: `LicenseRef-W3C-Document`
  - vendor/licenses/purrdf-4.0.2/third-party/rdfc/1-LICENSE.md.license: `LicenseRef-W3C-Software-and-Document`
  - vendor/licenses/purrdf-4.0.2/third-party/shacl/1-LICENSE.md.license: `LicenseRef-W3C-Software-and-Document`
  - vendor/licenses/purrdf-4.0.2/third-party/jsonld-w3c/4-LICENSE.md.license: `LicenseRef-LICENSES-FOR-W3C-TEST-SUITES`
- Upstream .md files are unmodified.

## Copyright holder: open question

Every sidecar currently reads `SPDX-FileCopyrightText: Contributors to the W3C`.

The upstream texts do not name a repository. The only "Repository" wording is "this Repository" with no name. The exact upstream lines that name W3C (or the lack of it) are:

- csvw/1-LICENSE.md, line 1: `All documents in this Repository are licensed by contributors` (no repository named)
- csvw/1-LICENSE.md, lines 2-3: `under the` / `[W3C Document License](http://www.w3.org/Consortium/Legal/copyright-documents).` (names W3C only through the licence link)
- rdfc/1-LICENSE.md, lines 1-3: `All documents in this Repository are licensed by contributors` / `under the` / `[W3C Software and Document License](https://www.w3.org/Consortium/Legal/copyright-software).` (W3C named only in the licence link)
- shacl/1-LICENSE.md, lines 1-3: `All documents in this Repository are licensed by contributors` / `under the` / `[W3C Software and Document License](http://www.w3.org/Consortium/Legal/copyright-software).` (W3C named only in the licence link)
- jsonld-w3c/4-LICENSE.md, lines 1-2: `The JSON-LD Test Suite is covered by the dual-licensing approach described in` / `[LICENSES FOR W3C TEST SUITES](https://www.w3.org/Consortium/Legal/2008/04-testsuite-copyright.html).` (W3C named only in the link; no copyright holder line)

No file contains a "Copyright" line. The precedent holder "Contributors to the W3C CSV on the Web repository" is not named by any of these texts.

Options for the coordinator: (a) keep `Contributors to the W3C` as the holder the licence links name; (b) drop SPDX-FileCopyrightText from the sidecars (not possible under REUSE, which requires it, so this is likely unworkable); (c) name the holder differently as ruled.

## Verification

- check-repository.py: EXIT=0 (.stage/181-check.log)
- test-repository-audit.py: EXIT=0, 53 tests OK (.stage/181-audit.log)
- make hygiene: EXIT=0 (.stage/181-hygiene.log, final line `EXIT=0`; run under `flock /tmp/purrassist-build.lock`).

## Holder ruling applied: record check

Ruling: holder = "Contributors to the <upstream repository as the vendoring record names it>", else the plain holder, reported per file.

Vendoring record (vendor/dependencies.json, `packaged_notices` and `cargo_packages`):
- csvw/1-LICENSE.md, jsonld-w3c/4-LICENSE.md, rdfc/1-LICENSE.md and shacl/1-LICENSE.md are listed under the `purrdf-rdf` (csvw, jsonld-w3c, rdfc) and `purrdf-shapes` (shacl) records.
- Those records name `"repository": "https://github.com/Blackcat-Informatics/purrdf"`. That is the vendoring crate's repository, not the upstream W3C source.
- The record names no upstream source repository for any of the four files.

Applied: the plain holder `Contributors to the W3C` for all four files, because the record names only the vendoring crate's repository. Using that repository would attribute W3C text to Blackcat's own repository, which the texts do not support. Sidecars are unchanged from the staged version.

## Pyright note

The reportOptionalMemberAccess warning at scripts/check-repository.py line 134 (`re.match(r'r(#*)"', ...).group(1)`) is pre-existing. HEAD has the same line at 134, and my change is confined to `check_markdown_attribution` (around line 270). Not fixed here; out of scope for issue 181.
