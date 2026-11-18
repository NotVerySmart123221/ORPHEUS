use std::collections::{BTreeMap, BTreeSet};

use crate::bits::{BitReader, BitWriter};
use crate::script_graph::GraphStore;
use crate::script_init::Snapshot;

type Descriptor = (u32, Option<(u16, u16)>);

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum IdentityError {
    OutOfSequence { first: u32, applied: u32 },
    ReintroducedRemoved(u16),
    ConflictingDescriptor(u16),
    UnknownUpdate(u16),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct IdentityReader {
    live: BTreeMap<u16, Descriptor>,
    removed: BTreeSet<u16>,
    last: u32,
}

impl IdentityReader {
    pub(crate) fn was_removed(&self, id: u16) -> bool {
        self.removed.contains(&id)
    }

    fn descriptor(reader: &mut BitReader<'_>) -> Descriptor {
        let graph = reader.bits(16).unwrap() as u32;
        assert_eq!(reader.bits(2).unwrap(), 0);
        let parent = reader.bit().unwrap().then(|| {
            (
                u16::try_from(reader.w_var().unwrap()).unwrap(),
                u16::try_from(reader.w_var().unwrap()).unwrap(),
            )
        });
        assert!(
            !reader.bit().unwrap(),
            "descriptor references not supported by this reader"
        );
        (graph, parent)
    }

    // this models increasing cumulative chunk numbers for one monotonic-ID VM lease.
    pub(crate) fn apply(
        &mut self,
        chunk: &BitWriter,
        baseline: bool,
        owner: bool,
    ) -> Result<(), IdentityError> {
        let mut reader = BitReader::with_range(chunk.as_bytes(), 0, chunk.bit_len()).unwrap();
        let first = reader.w_u32().unwrap();
        let span = reader.w_var().unwrap();
        assert!(!reader.bit().unwrap());
        let width = reader.w_var().unwrap() as usize;
        assert_eq!(width, reader.left());
        assert_eq!(first == 0, baseline);
        let last = first.checked_add(span).unwrap();
        if last <= self.last {
            return Ok(());
        }
        if first > self.last.saturating_add(1) {
            return Err(IdentityError::OutOfSequence {
                first,
                applied: self.last,
            });
        }
        assert!(!reader.bit().unwrap());
        let is_owner = reader.bit().unwrap();
        assert_eq!(is_owner, !baseline && owner);
        let body_end = if is_owner {
            reader.bit().unwrap(); // initial owner or correction
            assert_ne!(reader.w_var().unwrap(), 0);
            let body_width = reader.w_var().unwrap() as usize;
            assert!(body_width <= reader.left());
            Some(reader.position() + body_width)
        } else {
            None
        };
        if baseline {
            assert!(self.live.is_empty());
        }
        let mut id = 0_u16;
        loop {
            let step = reader.w_u16().unwrap();
            if step == 0 {
                break;
            }
            id = id.checked_add(step).unwrap();
            if baseline {
                assert!(
                    self.live
                        .insert(id, Self::descriptor(&mut reader))
                        .is_none()
                );
            } else if reader.bit().unwrap() {
                self.live.remove(&id);
                self.removed.insert(id);
            } else if reader.bit().unwrap() {
                let descriptor = Self::descriptor(&mut reader);
                if self.removed.contains(&id) {
                    return Err(IdentityError::ReintroducedRemoved(id));
                }
                if let Some(previous) = self.live.get(&id) {
                    if *previous != descriptor {
                        return Err(IdentityError::ConflictingDescriptor(id));
                    }
                } else {
                    self.live.insert(id, descriptor);
                }
            } else {
                if !self.live.contains_key(&id) {
                    return Err(IdentityError::UnknownUpdate(id));
                }
            }
        }
        assert!(body_end.is_none_or(|end| reader.position() <= end));
        self.last = last;
        Ok(())
    }

    pub(crate) fn verify(&self, snapshot: &Snapshot, graphs: &GraphStore) {
        let expected: BTreeMap<_, _> = snapshot
            .instances
            .iter()
            .map(|(&id, instance)| (id, (instance.graph, instance.parent)))
            .collect();
        assert_eq!(self.live, expected);
        for (&id, &(_, parent)) in &self.live {
            if let Some((parent, state)) = parent {
                let (graph, _) = self
                    .live
                    .get(&parent)
                    .expect("descriptor points to absent parent");
                assert_ne!(id, parent);
                assert!(
                    graphs
                        .graph(*graph)
                        .unwrap()
                        .state(usize::from(state))
                        .is_some(),
                    "invalid attachment {parent}:{state}"
                );
            }
            let mut visited = BTreeSet::new();
            let mut current = id;
            while let Some((parent, _)) = self.live[&current].1 {
                assert!(visited.insert(current), "cyclic parent chain at {current}");
                current = parent;
            }
        }
    }
}
