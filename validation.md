# Validation index — issue #1, PR #27 (MCP Registry OIDC workflow)

- Cause: device-flow publish returned 403, granting io.github.paudley/* only. The registry source (internal/api/handlers/v0/auth/github_at.go, v1.8.1) derives org namespaces from GET /user/memberships/orgs for admin roles, with the read:org scope requested. Making membership public was verified (204) and did not change the result. The org's third-party application restrictions hide the org from the registry OAuth app.
- OIDC route: github_oidc.go grants io.github.<repository_owner>/*.
- The install step was simulated locally: checksum OK, binary on PATH, `mcp-publisher validate` passes.
- check-docs, check-repository and check-release pass; make hygiene exit 0 on 798104f.
- Security review (reviews/merge-plan-review.md): PASS on 03be6d5. make hygiene exit 0.
