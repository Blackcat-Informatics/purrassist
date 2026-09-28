<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# `PurRAssist`

Assistance for AIs that get the details almost right. This facade exposes the
pure recovery engines and isolated syntax integration.

```rust
use purrassist::{Outcome, RecoveryRequest, Schema};
let schema = Schema::compile(r#"{"type":"object","required":["temperature"],"properties":{"temperature":{"type":"number"}},"additionalProperties":false}"#).unwrap();
let answer = schema.recover("[[{temperatur: '42',}]]", RecoveryRequest::default());
let Outcome::Resolved(value) = answer.outcome else { panic!("unexpected outcome") };
assert_eq!(value.to_json(), r#"{"temperature":42}"#);
```

Inspect every result's status and receipt. Validation does not turn an invented
value into a fact. Ambiguity and resource exhaustion are explicit outcomes.

Pre-release implementation; full qualification remains unfinished. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for capabilities,
limitations, verification, and provenance. First-party code is licensed under
MIT OR Apache-2.0 OR MulanPSL-2.0.
