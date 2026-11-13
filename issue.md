# Issue #188: Agents: add shell-safety rules (one action per call, deletions alone, no long chains) to AGENTS.md

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Add shell-safety rules to the repository agent instructions (AGENTS.md), per the coordinator standing rule.

Rules to add, in neutral words: one shell call does one thing (related commands may share a call; no long chains joined with && or ;; a script goes in .stage and is run as a file); a deletion is never inside a chain and is its own call on one exact path inside the repository after it has been listed; nothing that could raise an authorization prompt is sent (no glob or variable deletion, no path outside the repository, no long compound command); sub-agent briefs carry these rules.


## Comments (0)

