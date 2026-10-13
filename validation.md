# Validation index — issue #1, PR #23 (0.1.0 release date)

- Change: the CHANGELOG `[0.1.0]` heading date is 2026-10-13. The docs-check fixtures read the checkout heading.
- check-docs.py OK; test-check-docs.py 252 OK; make hygiene exit 0 on 04be6e0.
- The owner authorized the release on 2026-10-12 ("Yes, release 0.1.0 now").
- Release step 2 runs on the merge commit (recorded on issue #1).
- Follow-up 2fb4c3b: RECEIPT_VERSION Rustdoc reworded for the first release, the release-body rule comment, test spacing. make hygiene, core clippy -D warnings and cargo doc -D warnings all pass.
- merge-plan-review.md: VERDICT PASS (its three optional fixes and the Rustdoc note were applied in 2fb4c3b).
