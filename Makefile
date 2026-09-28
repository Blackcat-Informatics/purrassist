# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
MAKEFLAGS += --no-print-directory
.PHONY: check fmt lint test doc hygiene wasm-test benchmark package syntax-oracles

check: fmt hygiene lint test doc

fmt:
	cargo fmt --all --check

hygiene:
	python3 scripts/check-repository.py

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
	cargo package --workspace --locked
	python3 scripts/check-packages.py
