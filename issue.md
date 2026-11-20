# Issue #281: Normalize declared FnOM Mapping aliases in the native loader

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

Part of #33. Accept the documented FnOM Mapping alias only when the graph explicitly declares it equivalent to the native FnO Mapping class. Normalize that type in the checked temporary FnO view and feed the existing native reader. Retain the original declaration and type in the published RDF.

Expose the registered normalization ledger for the FnO projection. Preserve typed refusals and original source locations. Native tests cover the declared alias, an undeclared alias, a neighboring canonical graph, and a manifest round trip that retains the authored RDF.

The remaining loader, codec and SHACL acceptance stays in #33.


## Comments (0)

