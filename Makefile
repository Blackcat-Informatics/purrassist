# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
MAKEFLAGS += --no-print-directory
.PHONY: check fmt lint test doc hygiene wasm-test benchmark package syntax-oracles resources fuzz helper-test helper-census helper-check

check: fmt hygiene helper-test helper-check lint test doc

fmt:
	cargo fmt --all --check

hygiene:
	python3 scripts/check-repository.py
	python3 scripts/test-repository-audit.py
	python3 scripts/check-release.py
	python3 scripts/test-fuzz.py
	python3 scripts/test-fuzz-feedback.py
	python3 scripts/test-verify-published.py
	python3 scripts/test-resource-report.py
	python3 scripts/check-resource-limits.py --schema-only docs/qualification/resources-linux-x86_64.json

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	RUSTFLAGS="$${RUSTFLAGS:-} -D warnings" cargo test --workspace --profile verification --locked

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

resources:
	python3 scripts/test-resource-report.py
	python3 scripts/measure-resources.py $(RESOURCE_ARGS)

# TARGET selects one of json/markdown/schema/pattern/unified/fuzzy/matching/patch
# (see docs/FUZZING.md); FUZZ_ARGS forwards to scripts/fuzz.py (--iterations,
# --seed, --timeout, --replay). With no TARGET, every target runs once at the
# documented smoke count (scripts/fuzz.py's default of 1,000 mutations),
# ignoring FUZZ_ARGS, so the gate stays deterministic across all eight targets.
fuzz:
	python3 scripts/test-fuzz.py
	python3 scripts/test-fuzz-feedback.py
	@if [ -z "$(TARGET)" ]; then \
		for target in json markdown schema pattern unified fuzzy matching patch; do \
			echo "== fuzz target: $$target =="; \
			python3 scripts/fuzz.py --target $$target || exit 1; \
		done; \
	else \
		python3 scripts/fuzz.py --target $(TARGET) $(FUZZ_ARGS); \
	fi

# Detector self-tests, ledger validity and strict clone enforcement are mandatory.
# Only documented exact typed API distinctions may remain in the ledger.
helper-test:
	cargo run --quiet --locked -p purrassist-helper-census -- --self-test
	python3 scripts/check-shared-helpers.py --self-test
	python3 scripts/check-shared-helpers.py --validate-ledger

helper-census:
	cargo run --quiet --locked -p purrassist-helper-census -- --baseline

helper-check:
	python3 scripts/check-shared-helpers.py
