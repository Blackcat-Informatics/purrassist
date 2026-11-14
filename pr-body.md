Apply the newer compiler clippy rewrite in crates/core/src/tool_call/failed_prefix.rs (issue 215).

The framing tail was if dialect-is-HermesStripped { dropped-span test } else { false }. It is now the logical and of the two, with the dropped-span test on the right, so the right side is evaluated only when the left holds, as before. Value and evaluation order are unchanged. No other change.

Refs #215
