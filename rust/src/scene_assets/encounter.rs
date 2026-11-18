//! Junkenstein castle gate and spawn anchor map-asset definitions.

use super::*;

/// Authored server placement and script for the castle gate.
#[derive(Clone, Debug, PartialEq)]
pub struct JunkensteinGate {
    pub entity: u32,
    pub position: [f64; 3],
    pub graph: u32,
}

impl JunkensteinGate {
    pub const TAG: u64 = 0x0d60_0000_0000_030e;
}

pub(super) struct SpawnAnchor {
    pub(super) position: [f64; 3],
    pub(super) forward: [f64; 3],
}

#[derive(Default)]
pub(crate) struct SpawnAnchors {
    pub(super) transforms: BTreeMap<u32, SpawnAnchor>,
    pub(super) tagged: BTreeMap<u64, Option<Vec<u32>>>,
}

impl SpawnAnchors {
    pub(crate) fn targets(&self, tag: u64) -> Option<&[u32]> {
        match self.tagged.get(&tag) {
            Some(targets) => targets.as_deref(),
            None => Some(&[]),
        }
    }

    pub(crate) fn position(&self, entity: u32) -> Option<[f64; 3]> {
        self.transforms.get(&entity).map(|anchor| anchor.position)
    }

    pub(crate) fn forward(&self, entity: u32) -> Option<[f64; 3]> {
        self.transforms.get(&entity).map(|anchor| anchor.forward)
    }
}
