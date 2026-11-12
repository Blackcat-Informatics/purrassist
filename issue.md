# Issue #175: Repair the red verify run on main at ffee73d

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

The push run 41771464402 on ffee73d fails 6 of 8 jobs (native ubuntu and macos, release-gate, windows-engines, corpus, fuzz). Recorded failing assertions: tool_call_cost.rs:326, :230, :626, tool_call_cost_support/mod.rs:612, receipt_wire.rs:255, tool_call_recover.rs:129 and :1050, plus the fuzz oracle. Each assertion is repaired where the written contract for the preset says so: widen it only where the contract allows both outcomes, otherwise fix the code; where no document speaks, the behavior before the regression is the contract. Each repair is its own small reviewed PR.

## Comments (0)

