# T5 repository settings record

## Private vulnerability reporting (Task 3, 2026-10-12T13:15:49-06:00)

- before: `{"enabled":false}`
- action: `gh api -X PUT repos/Blackcat-Informatics/purrassist/private-vulnerability-reporting`
- after: `{"enabled":true}`

## Repository presentation (Task 5, 2026-10-12T13:28:39-06:00)

- action: `gh repo edit Blackcat-Informatics/purrassist --description ... --homepage ... --enable-wiki=false --enable-projects=false --add-topic ...`
- before: `{"description":"","hasDiscussionsEnabled":false,"hasIssuesEnabled":true,"hasProjectsEnabled":true,"hasWikiEnabled":true,"homepageUrl":"","repositoryTopics":null}`
- after: `{"description":"Recovers intent from imperfect AI output — JSON, tool calls, names, paths and Markdown edits — with auditable receipts. Rust crate, MCP server and CLI.","hasDiscussionsEnabled":false,"hasIssuesEnabled":true,"hasProjectsEnabled":false,"hasWikiEnabled":false,"homepageUrl":"https://github.com/Blackcat-Informatics/purrassist#readme","repositoryTopics":[{"name":"ai-agents"},{"name":"cli"},{"name":"fuzzy-matching"},{"name":"json-repair"},{"name":"json-schema"},{"name":"llm"},{"name":"markdown"},{"name":"mcp"},{"name":"mcp-server"},{"name":"model-context-protocol"},{"name":"rust"},{"name":"tool-calling"},{"name":"wasm"}]}`

## Social preview upload (Task 5)

- Attempted via the Playwright browser: https://github.com/Blackcat-Informatics/purrassist/settings returned 404 (the browser session is not signed in to GitHub). No sign-in was attempted.
- Status: **NOT DELIVERED**, an owner step: upload `docs/social-preview.png` in Settings → General → Social preview.
