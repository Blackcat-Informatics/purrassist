# Issue #170: Destroy scratch buffers before releasing their reservations

State: OPEN   Repo: Blackcat-Informatics/purrassist   Forge: github

## Body

ScratchVec and ScratchString currently release their scratch reservations before Rust destroys their inner buffers. An owned payload destructor can therefore observe released capacity while its enclosing storage still exists.

Destroy each inner buffer before releasing its reservation. Add focused regression coverage for the payload-destructor boundary, preserving public APIs, resource limits and the existing final release behavior.

This is a complete ownership prerequisite for #18, #33 and #36. The parent contracts remain open; Forest interning, publication, resolved folding and layout admission are separate work.


## Comments (0)

