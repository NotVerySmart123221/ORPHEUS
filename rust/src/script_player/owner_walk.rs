use crate::{
    mover::Mover,
    script_init::{InitFault, MoveToKey},
};

use super::PlayerBodyRuntime;

impl PlayerBodyRuntime {
    pub(super) fn live_move(&self) -> Option<(MoveToKey, [f64; 3], f64)> {
        if self.world.vitals.death().is_some() {
            return None;
        }
        let mut requests =
            self.component
                .snapshot()?
                .instances
                .iter()
                .flat_map(|(&id, instance)| {
                    instance.states.iter().filter_map(move |(&raw, state)| {
                        if !matches!(state.kind, crate::script_init::StateKind::MoveTo { .. }) {
                            return None;
                        }
                        let (destination, speed) = state.move_request()?;
                        Some((
                            MoveToKey {
                                instance: id,
                                raw,
                                activation: state.activation,
                                start_ms: state.start_ms,
                            },
                            destination,
                            speed,
                        ))
                    })
                });
        // Conflicting authored drives are ambiguous; do not pick an ID winner.
        let request = requests.next()?;
        requests.next().is_none().then_some(request)
    }

    pub(super) fn report_move(&mut self, frame: u32, mover: &Mover) -> Result<(), InitFault> {
        if let Some((key, outcome)) = mover.owner_move_result(frame) {
            self.component.queue_move_result(frame, key, outcome)?;
        }
        Ok(())
    }
}
