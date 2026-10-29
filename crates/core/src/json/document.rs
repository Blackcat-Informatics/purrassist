// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

use super::Node;
use crate::contract::{ScratchCharge, ScratchLease};

/// Immutable parsed storage; the lease contains transferred buffer reservations,
/// not a size estimate. The root drops before its storage reservation releases.
#[derive(Debug)]
pub(crate) struct OwnedDocument {
    pub(super) root: Node,
    pub(super) lease: ScratchLease,
}

impl OwnedDocument {
    pub(crate) const fn root(&self) -> &Node {
        &self.root
    }

    /// Ordinary public JSON nodes retain their published storage admission.
    pub(super) fn into_node(self) -> Node {
        let Self { root, lease } = self;
        lease.retain();
        root
    }
}

impl ScratchCharge for OwnedDocument {
    const CHARGE: usize = {
        let charge = Node::CHARGE + 16;
        assert!(size_of::<Self>() <= charge);
        charge
    };
}
