GBNF compiler for tool argument grammars (issue 223, second part of #36). Partial #36: this PR does not close #36; the prompt projector and routing remain open parts.

The pattern dialect is translated exactly from the core search automaton one scalar at a time (column one of the TOOLS.md table); back-references are ledgered as a defence on another route (the upstream schema compiler refuses a graph back-reference first); look-around is refused by name, filed as #224. The untranslated keywords minLength, maxLength, minItems, maxItems, minProperties, maxProperties and multipleOf are refused by name, each filed (#225 to #231). Integer and decimal bounds are exact. The core gains scalar_transitions, ScalarRun and is_matched for the lowering; DfaState gains Ord for an ordered map.

Main is merged into the branch at 8ea43a2 (the merge brought PR 220 documentation only).

Independent pass PASS at 8ea43a2, .stage/tool-gbnf-gate/reviews/8ea43a2-review.md. make check exit 0 at 8ea43a2 (local, read from its log once it exits).

Refs #36, #223
