# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
MAKEFLAGS += --no-print-directory
.PHONY: check corpus fmt lint test doc hygiene wasm-test benchmark package syntax-oracles resources fuzz fuzz-hosted helper-test helper-census helper-check release-gate

check: fmt hygiene helper-test helper-check lint test doc

fmt:
	cargo fmt --all --check

hygiene:
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-repository.py
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-repository-audit.py
	python3 scripts/check-release.py
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-release.py
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/check-docs.py
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-check-docs.py
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
	python3 scripts/test-resource-report.py
	python3 scripts/check-resource-limits.py --schema-only docs/qualification/resources-linux-x86_64.json

lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings
	cargo clippy -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite)) -- -D warnings

test:
	RUSTFLAGS="$${RUSTFLAGS:-} -D warnings" cargo test --workspace --locked

# Corpus suites (`test = false` in crates/core/Cargo.toml): vendored
# conformance corpora, generated evaluation corpora, exhaustive oracles and
# 1 MiB scale runs. The list has two users: `make corpus` runs the suites (CI
# runs it in the `corpus` job; `make check` does not), and `make lint` lints
# them with Clippy, so `make check` lints them although it does not run them.
CORPUS_SUITES := correction_evaluation corruption_oracle markdown_corpus matching_oracle pattern_corpus schema_conformance tool_call_scale unicode_conformance
corpus:
	RUSTFLAGS="$${RUSTFLAGS:-} -D warnings" cargo test -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite))

doc:
	RUSTDOCFLAGS="$${RUSTDOCFLAGS:-} -D warnings" cargo doc --workspace --no-deps --locked

wasm-test:
	python3 scripts/check-wasm.py

benchmark:
	cargo bench -p purrassist-core --bench recovery --locked

syntax-oracles:
	python3 scripts/check-syntax-oracles.py

package:
	python3 scripts/check-release.py
	python3 scripts/package-workspace.py

# The one release gate (docs/RELEASE.md): release qualification and the `Verify`
# workflow's `release-gate` job both run exactly this, on the release toolchain.
# No prerequisites, so the recipe lines run in this order even under `make -j`;
# scripts/check-release.py pins the lines and their order.
release-gate:
	$(MAKE) check
	$(MAKE) corpus
	python3 scripts/test-fuzz-feedback.py
	python3 scripts/test-verify-published.py
	$(MAKE) package

resources:
	python3 scripts/test-resource-report.py
	python3 scripts/measure-resources.py $(RESOURCE_ARGS)

# TARGET selects one of json/markdown/schema/pattern/unified/fuzzy/matching/patch/tool-call/tool-call-ref
# (see docs/FUZZING.md); FUZZ_ARGS forwards to scripts/fuzz.py (--iterations,
# --seed, --timeout, --replay). With no TARGET, every target runs once at the
# documented smoke count (scripts/fuzz.py's default of 1,000 mutations),
# ignoring FUZZ_ARGS, so the run stays deterministic across all ten targets.
fuzz:
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
	python3 scripts/test-fuzz-feedback.py
	@if [ -z "$(TARGET)" ]; then \
		for target in json markdown schema pattern unified fuzzy matching patch tool-call tool-call-ref; do \
			echo "== fuzz target: $$target =="; \
			python3 scripts/fuzz.py --target $$target || exit 1; \
		done; \
	else \
		python3 scripts/fuzz.py --target $(TARGET) $(FUZZ_ARGS); \
	fi

# The CI and pre-tag fuzz gate (docs/RELEASE.md): every target at the smoke
# count, stopping at every probe panic. Every target uses the runner default
# probe timeout, including the issue #11 repair reproduction.
fuzz-hosted:
	python3 -X warn_default_encoding -W error::EncodingWarning scripts/test-fuzz.py
	python3 scripts/test-fuzz-feedback.py
	@for target in json markdown schema pattern unified fuzzy matching patch tool-call tool-call-ref; do \
		echo "== fuzz target: $$target =="; \
		python3 scripts/fuzz.py --target $$target || exit 1; \
	done

# Detector self-tests and strict clone enforcement are mandatory; the strict
# helper-check validates the ledger too (--validate-ledger is its report-only
# subset). Only documented exact typed API distinctions may remain in the ledger.
helper-test:
	cargo run --quiet --locked -p purrassist-helper-census -- --self-test
	python3 scripts/check-shared-helpers.py --self-test

helper-census:
	cargo run --quiet --locked -p purrassist-helper-census -- --baseline

helper-check:
	python3 scripts/check-shared-helpers.py
