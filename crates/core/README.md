<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-core

Pure Rust intent recovery with zero third-party Cargo dependencies. Includes
schema-guided JSON repair, exact decimal arithmetic, Unicode matching, semantic
domain rules, source-bound edits, and evidence receipts.

```rust
use purrassist_core::{Outcome, schema::{Schema, recovery::RecoveryRequest}};
let schema = Schema::compile(r#"{"type":"integer"}"#).unwrap();
let result = schema.recover("[[42]]", RecoveryRequest::default());
assert!(matches!(result.outcome, Outcome::Resolved(_)));
assert_eq!(result.receipt.sources[0].text(), Some("[[42]]"));
```

Recovery may resolve, guess, preserve ambiguity, or exhaust its explicit budget.
Guesses remain labeled after validation. The core never reads files, contacts
models, or consults ambient clocks or randomness.

Pre-release implementation. Markdown parsing and rendering are tested against
all 652 `CommonMark` and 672 GFM specification examples, with source-preserving
structural edits and explicit resource outcomes. See the
[implementation contract](https://github.com/Blackcat-Informatics/purrassist/blob/main/docs/IMPLEMENTATION.md).

First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0. Unicode and HTML entity tables,
schema meta-schemas, and test corpora retain the upstream notices included with them.
