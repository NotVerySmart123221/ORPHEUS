//! Read-only patrol policy adapter over the bounded Lua `AiScript` host.

use std::{fmt::Write, io};

use sha2::{Digest, Sha256};

use crate::{
    ai_script::{AiActorSnapshot, AiCommand, AiEvent, AiScript, AiScriptError, AiSnapshot},
    game_clock::{FIRST_TICK, TICK_PERIOD},
};

pub(crate) const DEFAULT_SOURCE: &str = include_str!("../scripts/patrol.lua");

#[derive(Clone, Copy)]
pub(crate) enum WaitEvent {
    Spawn,
    Release,
}

pub(crate) fn format_digest(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

pub(crate) struct PatrolScript {
    inner: AiScript,
    digest: [u8; 32],
}

impl PatrolScript {
    pub(crate) fn load() -> io::Result<Self> {
        Self::from_source(DEFAULT_SOURCE)
    }

    pub(crate) fn from_source(source: &str) -> io::Result<Self> {
        Self::from_source_checked(source).map_err(io::Error::from)
    }

    pub(crate) fn from_source_checked(source: &str) -> Result<Self, AiScriptError> {
        let inner = AiScript::from_source("=patrol.lua", source)?;
        let digest = Sha256::digest(source.as_bytes()).into();
        Ok(Self { inner, digest })
    }

    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
    }

    #[cfg(test)]
    pub(crate) fn digest_hex(&self) -> String {
        format_digest(&self.digest)
    }

    pub(crate) fn wait(
        &self,
        actor: AiActorSnapshot,
        event: WaitEvent,
        tick: u32,
    ) -> io::Result<f64> {
        self.wait_checked(actor, event, tick)
            .map_err(io::Error::from)
    }

    pub(crate) fn wait_checked(
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
        let commands = self.inner.think(&snapshot, &[ai_event])?;
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

    pub(crate) fn think(&self, actor: AiActorSnapshot, tick: u32) -> io::Result<usize> {
        self.think_checked(actor, tick).map_err(io::Error::from)
    }

    pub(crate) fn think_checked(
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
        let commands = self.inner.think(&snapshot, &[AiEvent::Think { tick }])?;
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

    pub(crate) fn smoke_check(&self) -> Result<(), AiScriptError> {
        let _ = self.wait_checked(
            AiActorSnapshot::patrol_fixture(0, 4),
            WaitEvent::Spawn,
            FIRST_TICK,
        )?;
        let slot = self.think_checked(AiActorSnapshot::patrol_fixture(0, 4), FIRST_TICK + 1)?;
        if slot >= 4 {
            return Err(AiScriptError::InvalidCommand(format!(
                "smoke check patrol_slot {slot} out of bounds for 4 slots"
            )));
        }
        let _ = self.wait_checked(
            AiActorSnapshot::patrol_fixture(0, 4),
            WaitEvent::Release,
            FIRST_TICK + 2,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_policy_preserves_waits_and_cyclic_route_choices() {
        let script = PatrolScript::load().unwrap();
        for _ in 0..1_000 {
            assert_eq!(
                script
                    .wait(AiActorSnapshot::patrol_fixture(0, 4), WaitEvent::Spawn, 1)
                    .unwrap(),
                3.0
            );
            assert_eq!(
                script
                    .think(AiActorSnapshot::patrol_fixture(2, 4), 188)
                    .unwrap(),
                3
            );
            assert_eq!(
                script
                    .wait(
                        AiActorSnapshot::patrol_fixture(3, 4),
                        WaitEvent::Release,
                        200
                    )
                    .unwrap(),
                1.2
            );
            assert_eq!(
                script
                    .think(AiActorSnapshot::patrol_fixture(3, 4), 275)
                    .unwrap(),
                0
            );
        }
    }

    #[test]
    fn host_validates_command_type_and_range() {
        for source in [
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = 1.5 } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = -1 } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = 3 } } end }",
            "return { abi = 2, think = function() return { { kind = 'patrol_slot', slot = '0' } } end }",
            "return { abi = 2, think = function() return { { kind = 'hold' } } end }",
        ] {
            let script = PatrolScript::from_source(source).unwrap();
            assert!(
                script
                    .think(AiActorSnapshot::patrol_fixture(0, 3), 1)
                    .is_err()
            );
        }
        for source in [
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 0/0 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 61 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = 0 } } end }",
            "return { abi = 2, think = function() return { { kind = 'wait', seconds = '3' } } end }",
            "return { abi = 2, think = function() return { { kind = 'hold' } } end }",
        ] {
            let script = PatrolScript::from_source(source).unwrap();
            assert!(
                script
                    .wait(AiActorSnapshot::patrol_fixture(0, 3), WaitEvent::Spawn, 1)
                    .is_err()
            );
        }
    }

    #[test]
    fn environment_and_budgets_isolate_failing_handlers() {
        let script = PatrolScript::from_source(
            r#"
            return {
                abi = 2,
                think = function(snapshot, events)
                    local event = events[1]
                    if event.kind == "spawn" and event.tick == 1 then
                        while true do end
                    elseif event.kind == "spawn" and event.tick == 2 then
                        local rows = {}
                        for i = 1, 64 do rows[i] = {} end
                        return { { kind = "wait", seconds = 3 } }
                    elseif io == nil and os == nil and load == nil and loadfile == nil
                        and require == nil and package == nil and coroutine == nil
                        and debug == nil and math == nil and _G == nil then
                        return { { kind = "wait", seconds = 3 } }
                    end
                    return {}
                end,
            }
        "#,
        )
        .unwrap();
        assert!(matches!(
            script
                .wait(AiActorSnapshot::patrol_fixture(0, 2), WaitEvent::Spawn, 1)
                .unwrap_err()
                .get_ref()
                .and_then(|err| err.downcast_ref::<AiScriptError>()),
            Some(AiScriptError::InstructionBudgetExceeded { .. })
        ));
        assert_eq!(
            script
                .wait(AiActorSnapshot::patrol_fixture(0, 2), WaitEvent::Spawn, 3)
                .unwrap(),
            3.0
        );
        assert!(PatrolScript::from_source("while true do end").is_err());
        assert!(PatrolScript::from_source("return 1").is_err());
        assert!(PatrolScript::from_source("return 1, function() end").is_err());
        assert!(PatrolScript::from_source("return { abi = 1, think = function() end }").is_err());
        assert!(PatrolScript::from_source("return { abi = '2', think = function() end }").is_err());
        script
            .inner
            .set_memory_limit_for_test(script.inner.used_memory() + 512)
            .unwrap();
        assert!(matches!(
            script
                .wait(AiActorSnapshot::patrol_fixture(0, 2), WaitEvent::Spawn, 2)
                .unwrap_err()
                .get_ref()
                .and_then(|err| err.downcast_ref::<AiScriptError>()),
            Some(AiScriptError::MemoryLimitExceeded)
        ));
    }
}
