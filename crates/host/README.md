<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# purrassist-host

Explicit host boundaries for PurRAssist: root-scoped UTF-8 files, journaled
replacement, conditional undo, shared tools, stdio MCP, and resumable helper
request/response adapters.

File transactions coordinate cooperating writers. Each replacement is atomic
for one file; batches report independent outcomes. The portable path checks are
not a sandbox against malicious concurrent directory replacement. Durable host
writes are implemented on Unix and Windows, with explicit filesystem-dependent
flush evidence. Crash/retry/undo tests exercise the transaction checkpoints;
the Windows CI lane executes the host operations on Windows.

Helper transports, endpoints, authentication, and timeouts belong to the caller.
No model is required. Returned helper proposals undergo local validation and
remain explicit guesses. No live provider qualification is claimed.

Pre-release implementation. See the
[repository](https://github.com/Blackcat-Informatics/purrassist) for the complete
contract. First-party code: MIT OR Apache-2.0 OR MulanPSL-2.0.
