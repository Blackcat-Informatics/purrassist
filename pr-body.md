MCP declarations can now receive explicit caller-owned effects, repair policy and parameter aliases before tool-set compilation. The original declaration bytes and complete schema validator remain intact, including constraints outside the typed recovery shape.

`ToolSet::from_mcp_with_metadata_governed` accepts canonical member paths through nested objects and arrays. Invalid, duplicate and colliding metadata fails explicitly. Traversal, retained aliases and diagnostic allocations use the existing governor; imports with empty metadata keep their conservative behavior.

Validation: CLI consumer compilation, four focused adapter regression cases and the complete mandatory `make check` gate passed. Independent code review completed; its diagnostic allocation finding was corrected and verified. Commit hooks and signing ran normally. Hosted CI is pending.

Closes #44. The complete graph loader and MCP codec remain tracked by #33.
