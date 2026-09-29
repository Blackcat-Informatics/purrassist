# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
MAKEFLAGS += --no-print-directory
.PHONY: check fmt lint test doc hygiene wasm-test benchmark package syntax-oracles resources fuzz-json

check: fmt hygiene lint test doc

fmt:
	cargo fmt --all --check

hygiene:
	python3 scripts/check-repository.py
	python3 scripts/check-release.py
	python3 scripts/test-fuzz-json.py
	python3 scripts/test-verify-published.py

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

fuzz-json:
	python3 scripts/test-fuzz-json.py
	python3 scripts/fuzz-json.py $(FUZZ_ARGS)
