<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->
<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->
# Retained PurRDF notices

These files retain original bytes from PurRDF revision
`511f8af4efafe7e9e1c4e673c28afb1f5b4f9572`. The dependency registry lists each
local notice path and its SHA-256. Byte-identical first-party, Unicode, lexical,
hashbrown and original Zstandard license texts reuse their existing registered
paths. The common `NOTICE.txt` is shared by the fourteen profiles with no
additional registered components.

The six `upstream-inventory.json` files are archived upstream inventories.
Their `files`, `paths` and provenance references describe the upstream package
layout. The local retained paths are those in `vendor/dependencies.json`.

The core's native table retains hashbrown's original MIT or Apache grant and
its modification attribution. The maintained Zstandard package retains Apache-2.0,
its original source attribution, `PROVENANCE.md` and `UPSTREAM-SHA256.json`.

The shapes inventory labels its SHACL component as source-only, but the library
embeds specification data in `crates/shapes/src/spec/vocab.rs` and
`crates/shapes/src/lint.rs`. The local composite license therefore includes
W3C-20150513 for that embedded data, alongside the first-party grant. The archived
upstream declaration remains unchanged. The SHACL conformance vectors remain
source-only.

The registered RDF CSVW, JSON-LD, Obographs, RDF and canonicalization components
cover conformance data under `crates/rdf/tests/`. Their notices are retained with
that source-only scope. A recipient notice's conditional reference to CC-BY
applies to documentation carrying that header; it does not change every
package's runtime license.

The adjacent `.license` files supply local SPDX attribution for the preserved
upstream Markdown notices. The attribution checker accepts this form only for
registered vendor notice bytes whose SHA-256 matches, with both nonempty SPDX
fields and a single component license identifier. Ordinary Markdown retains its
inline-header requirement.
