# Issue #279: Index tool graph parameter hint reifiers during RDF loading

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Part of #33. The RDF loader scans the full reifier table for every FnO input parameter. Build a temporary, admitted lookup over parameter-path reifiers so each lookup searches by native statement identity.

Preserve blank-node scope, duplicate detection, the encounter order used for refusals, and the original source diagnostics for unconsumed reifiers. Admit index storage and work under the existing caller governor. Use native loader fixtures to exercise a matching hint and a competing binding, then run the repository gate.

This delivery covers parameter hint lookup. The remaining loader, codec and SHACL acceptance stays in #33.


## Comments (0)

