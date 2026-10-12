# T4 review: social preview

## Scope
Uncommitted diff against f11912e: docs/social-preview.{svg,png,png.license}, docs/assets.json, vendor/tooling.json (rsvg-convert, noto-sans), vendor/licenses/{librsvg-2.63.2,noto-sans-2.015}, docs/DEPENDENCIES.md, scripts/check-docs.py, scripts/check-repository.py and their tests. Inputs: plan.md Task 4 and the contract row, output-rules.md, validation.md "Task 4".

## Evidence
- Gate commands: check-docs.py OK; test-check-docs.py 178 tests OK; check-repository.py OK; test-repository-audit.py 48 tests OK.
- Reproduction: `rsvg-convert -w 1280 -h 640` then `cmp` against docs/social-preview.png: identical. sha256 of the SVG (c7240946...cac26), the PNG (a1045507...3338), both Noto Sans files (478c558e..., 1df075a3...) and both notices (OFL 1e6244d9..., LGPL 5749785c...) all equal the recorded values. The LGPL notice is byte-identical to vendor/licenses/profiling-glibc/LGPL-2.1-or-later.txt, as stated.
- Deliverables present: SVG is 1280x640 with SPDX comments, an inlined logo group and one font-family ("Noto Sans", docs/social-preview.svg:115), and the strip text at :126; the PNG, the sidecar, and assets.json (digests, command, rsvg 2.63.2 plus library versions, font upstream/version/digests, license) are all there. tooling.json has both entries with notices, sha256 and manual-only scope. DEPENDENCIES.md has the justification (appended at the end of the file). check-docs.py `check_social_preview` covers the IHDR, size, digests and font family. check-repository.py `check_binary_attribution` is the sidecar rule. Tests cover each failure: wrong dimensions, non-PNG, oversize, SVG/PNG digest drift, fallback list, other family, CSS fallback, missing family, unregistered asset, and sidecar missing or lacking a tag.
- Mutation probes on a temp copy (reverted; the worktree is untouched): one SVG byte changed gives a sha256 failure; adding a ", sans-serif" fallback gives a font-family failure; deleting the sidecar gives "missing attribution sidecar".
- Visual check of the PNG: dark gradient, readable wordmark, tagline and strip, logo not clipped, text well inside the 40 px margin, legible at thumbnail size. Professional.
- Output rules: no attribution and no hosted-CI payment or account wording in the diff.

## License provenance
- librsvg is `LGPL-2.1-or-later` (pacman -Qi librsvg reports it; upstream agrees). Correct. Process-separate use is described accurately. The notice_normalization states honestly that the text was copied from the glibc notice.
- Noto Sans is registered as `OFL-1.1`. The font name table declares OFL 1.1 (verified with strings), and pacman lists noto-fonts as OFL-1.1-no-RFN. Correct. The Apache-2.0 LICENSE file shipped in the distro package is documented only in validation.md:239 and not in the registry.
- vendor/licenses/noto-sans-2.015/OFL.txt is the font copyright line plus the OFL 1.1 text. The text is not byte-identical to the other OFL copies installed on this machine, and I could not locate the Rust-toolchain doc copy to compare against.

## Findings
1. (Blocking, honesty) vendor/tooling.json:186 notice_normalization for noto-sans says only "the installed package ships no OFL file for this font". It does not disclose that the OFL body was sourced from a Rust toolchain documentation copy, nor that the distro package's own LICENSE is Apache-2.0 while the font declares OFL. The disclosure must name the text's source and the Apache-2.0 discrepancy, because AGENTS.md requires license and source provenance to be preserved. Fix: extend the field (the sha256 will not change) and, ideally, record the Apache-2.0 note in assets.json font.distribution.
2. (Minor) Also record that the text's origin is not upstream's OFL.txt, and, if feasible, replace it with the upstream notofonts OFL.txt (the font's own name table points at the OFL, and upstream publishes one).
3. (Non-blocking) The assets.json "license" key at docs/assets.json is the project license for the image. Fine, since the font is not embedded.

## Consumers
check-docs.py (make check docs rules), check-repository.py (SPDX scan, and the sidecar rule applies to every tracked binary outside crates/core/tests/data/), the manual upload in Task 5, and the T6 inventory.

(Superseded by the recheck below.)

## Recheck
- vendor/licenses/noto-sans-2.015/OFL.txt now has sha256 cee9892f...c061a. I fetched raw OFL.txt from notofonts/latin-greek-cyrillic at commit 71cf6f3ca9c185d809dd800ce9a9e58e1c16f1b1 and its sha256 is identical, so the notice is the unmodified upstream file.
- vendor/tooling.json:184 records the new digest. The notice_normalization at :186 names the upstream source and commit, and discloses the Arch package's Apache-2.0 LICENSE file against the font's own OFL 1.1 declaration. Findings 1 and 2 are resolved.
- check-docs.py, check-repository.py and test-repository-audit.py pass. No billing, payment, or attribution wording was found in the registry files.

VERDICT: PASS
