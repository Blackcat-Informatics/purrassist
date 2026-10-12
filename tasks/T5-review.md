# T5 review: GitHub repository presentation

Reviewer: independent, read-only. No settings were changed and no browser sign-in was attempted by this review.
Date: 2026-10-12

## Scope

Task 5 and the "GitHub presentation" contract row of plan.md, the record tasks/T5-repo-settings.md, and output-rules.md. Verified against live state only.

## Evidence

Command: `gh repo view Blackcat-Informatics/purrassist --json description,homepageUrl,repositoryTopics,hasWikiEnabled,hasProjectsEnabled,hasIssuesEnabled,hasDiscussionsEnabled`

```
{"description":"Recovers intent from imperfect AI output — JSON, tool calls, names, paths and Markdown edits — with auditable receipts. Rust crate, MCP server and CLI.","hasDiscussionsEnabled":false,"hasIssuesEnabled":true,"hasProjectsEnabled":false,"hasWikiEnabled":false,"homepageUrl":"https://github.com/Blackcat-Informatics/purrassist#readme","repositoryTopics":[{"name":"ai-agents"},{"name":"cli"},{"name":"fuzzy-matching"},{"name":"json-repair"},{"name":"json-schema"},{"name":"llm"},{"name":"markdown"},{"name":"mcp"},{"name":"mcp-server"},{"name":"model-context-protocol"},{"name":"rust"},{"name":"tool-calling"},{"name":"wasm"}]}
```

Command: `gh api repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`

```
{"enabled":true}
```

Command: `curl -s https://github.com/Blackcat-Informatics/purrassist | grep -o 'og:image" content="[^"]*'`

```
og:image" content="https://opengraph.githubassets.com/57c842b0.../Blackcat-Informatics/purrassist
```

## Checks

| Check | Result |
|---|---|
| Description text equals plan (exact string, em dashes included) | PASS |
| Homepage equals `https://github.com/Blackcat-Informatics/purrassist#readme` | PASS |
| 13 topics, set equal to the plan list (ai-agents, cli, fuzzy-matching, json-repair, json-schema, llm, markdown, mcp, mcp-server, model-context-protocol, rust, tool-calling, wasm); none missing or extra | PASS |
| Wiki off, projects off, issues on, discussions off | PASS |
| Private vulnerability reporting enabled | PASS |
| Record "after" JSON equals live output (description, flags, homepage, all 13 topics); PVR after `{"enabled":true}` matches | PASS |
| Record has before values (empty description/homepage, no topics, wiki and projects on) and timestamps | PASS |
| Description presents the three interfaces (Rust crate, MCP server, CLI) | PASS |
| Description and record contain no wording about hosted-CI provider accounts or payments (output-rules.md) | PASS |
| Social-preview row honestly NOT DELIVERED, reason given (settings page returned 404, browser session not signed in), no sign-in attempted, owner step named | PASS |
| Live og:image is still the default `opengraph.githubassets.com` card, consistent with nothing having been uploaded | PASS (consistent) |

## Findings

1. NOTE (prominent): the social preview is NOT DELIVERED. The plan defines this path: it is not counted as done and goes to the owner (upload `docs/social-preview.png` under Settings > General > Social preview, or drop it from this stage). It must stay in the Task 6 PR body's remaining-owner-steps list. The live og:image confirms it is undelivered. This is not a blocker for this review.
2. NOTE: the homepage points at the README by design; the plan lists the switch to docs.rs after publication as an owner handoff item.
3. No discrepancies between the record and live state. No blocking issues.

VERDICT: PASS
