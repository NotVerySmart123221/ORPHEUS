use super::{AiActorSnapshot, WaitEvent};
use crate::{
    ai_script::{AiCommand, AiEvent, AiScript, AiScriptError, AiSnapshot},
    game_clock::TICK_PERIOD,
};

pub(super) struct LuaPolicy(pub(super) AiScript);

impl LuaPolicy {
    pub(super) fn wait_checked(
        &self,
        actor: AiActorSnapshot,
        event: WaitEvent,
        tick: u32,
    ) -> Result<f64, AiScriptError> {
        let snapshot = AiSnapshot {
            tick,
            dt_secs: TICK_PERIOD.as_secs_f64(),
            actor,
            targets: Vec::new(),
        };
        let ai_event = match event {
            WaitEvent::Spawn => AiEvent::Spawn { tick },
            WaitEvent::Release => AiEvent::Release { tick },
        };
        let commands = self.0.think(&snapshot, &[ai_event])?;
        match commands.as_slice() {
            [AiCommand::Wait { seconds }] => Ok(*seconds),
            [other] => Err(AiScriptError::InvalidCommand(format!(
                "expected wait command from patrol script, got {other:?}"
            ))),
            many => Err(AiScriptError::InvalidCommand(format!(
                "expected 1 wait command from patrol script, got {}",
                many.len()
            ))),
        }
    }

    pub(super) fn think_checked(
        &self,
        actor: AiActorSnapshot,
        tick: u32,
    ) -> Result<usize, AiScriptError> {
        let snapshot = AiSnapshot {
            tick,
            dt_secs: TICK_PERIOD.as_secs_f64(),
            actor,
            targets: Vec::new(),
        };
        let commands = self.0.think(&snapshot, &[AiEvent::Think { tick }])?;
        match commands.as_slice() {
            [AiCommand::MovePatrolSlot { slot }] => Ok(*slot),
            [other] => Err(AiScriptError::InvalidCommand(format!(
                "expected patrol_slot command from patrol script, got {other:?}"
            ))),
            many => Err(AiScriptError::InvalidCommand(format!(
                "expected 1 patrol_slot command from patrol script, got {}",
                many.len()
            ))),
        }
    }
}
