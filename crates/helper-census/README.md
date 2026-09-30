<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# PurRAssist helper census

Unpublished structural Rust inspection and strict helper-ledger tooling.
See [the detector guide](../../docs/HELPER-CENSUS.md) for commands, traversal,
normalization, acceptance criteria, known review distinctions and provenance.
Original source identity and file digests are in [UPSTREAM.json](UPSTREAM.json).

```sh
cargo run --quiet --locked -p purrassist-helper-census -- --self-test
cargo run --quiet --locked -p purrassist-helper-census -- --baseline
cargo run --quiet --locked -p purrassist-helper-census -- --check
```

The baseline is an inspection artifact. Strict checking rejects existing
unreviewed groups; it does not implicitly sanction the reported duplicates.
