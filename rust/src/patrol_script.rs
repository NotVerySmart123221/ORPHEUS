//! High-level patrol decisions. Rust owns steering, navigation and movement.

use std::{
    cell::RefCell,
    fmt::Write,
    io,
    sync::{Arc, OnceLock},
};

use orpheus_statescript::{
    script_expr::{Scope, Variable},
    script_graph::GraphStore,
    script_init::{Component, EmptyWorld, MAX_INIT_WORK, Root},
    script_wire::{Value, Variables},
};
use sha2::{Digest, Sha256};

#[cfg(feature = "lua-debug")]
mod lua;
#[cfg(feature = "lua-debug")]
use crate::{
    ai_script::{AiScript, AiScriptError},
    game_clock::FIRST_TICK,
};

const GRAPH_SOURCE: &str = include_str!("../scripts/patrol.statescript.json");
pub(crate) const STATESCRIPT_ABI: u32 = 1;
#[cfg(all(test, feature = "lua-debug"))]
pub(crate) const LUA_SOURCE: &str = include_str!("../scripts/patrol.lua");

const INSTANCE: u16 = 1;
const SPAWN: u64 = 0x0240_0000_0000_f001;
const RELEASE: u64 = 0x0240_0000_0000_f002;
const NEXT_WAYPOINT: u64 = 0x0240_0000_0000_f003;
const ROUTE_INDEX: u32 = 1;
const ROUTE_COUNT: u32 = 2;
const DECISION: u32 = 3;
const MAX_WAIT: f64 = 60.0;

#[derive(Clone, Copy)]
pub(crate) enum WaitEvent {
    Spawn,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(feature = "lua-debug"), allow(dead_code))]
pub(crate) struct AiPatrolSnapshot {
    pub(crate) route_index: usize,
    pub(crate) route_count: usize,
    pub(crate) wait_seconds: f64,
    pub(crate) walking: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(feature = "lua-debug"), allow(dead_code))]
pub(crate) struct AiActorSnapshot {
    pub(crate) entity: u32,
    pub(crate) generation: Option<u64>,
    pub(crate) team: u8,
    pub(crate) position: [f64; 3],
    pub(crate) yaw: f64,
    pub(crate) alive: bool,
    pub(crate) health: f64,
    pub(crate) max_health: f64,
    pub(crate) stunned: bool,
    pub(crate) patrol: Option<AiPatrolSnapshot>,
}

#[cfg(any(test, feature = "lua-debug"))]
impl AiActorSnapshot {
    pub(crate) fn patrol_fixture(route_index: usize, route_count: usize) -> Self {
        Self {
            entity: 0xc000_1000,
            generation: None,
            team: 1,
            position: [0.0; 3],
            yaw: 0.0,
            alive: true,
            health: 200.0,
            max_health: 200.0,
            stunned: false,
            patrol: Some(AiPatrolSnapshot {
                route_index,
                route_count,
                wait_seconds: 0.0,
                walking: false,
            }),
        }
    }
}

pub(crate) fn format_digest(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

enum Policy {
    StateScript(Box<RefCell<Component>>),
    #[cfg(feature = "lua-debug")]
    Lua(lua::LuaPolicy),
}

pub(crate) struct PatrolScript {
    policy: Policy,
    digest: [u8; 32],
}

impl PatrolScript {
    pub(crate) fn load() -> io::Result<Self> {
        static GRAPHS: OnceLock<Arc<GraphStore>> = OnceLock::new();
        let graphs = match GRAPHS.get() {
            Some(graphs) => Arc::clone(graphs),
            None => {
                let parsed = Arc::new(GraphStore::from_json(GRAPH_SOURCE.as_bytes())?);
                Arc::clone(GRAPHS.get_or_init(|| parsed))
            }
        };
        let component = Component::start(
            graphs,
            1,
            Variables::new(),
            vec![Root::new(INSTANCE, 1)],
            1,
            &mut EmptyWorld,
            MAX_INIT_WORK,
        )
        .map_err(io::Error::other)?;
        Ok(Self {
            policy: Policy::StateScript(Box::new(RefCell::new(component))),
            digest: Sha256::digest(GRAPH_SOURCE.as_bytes()).into(),
        })
    }

    #[cfg(all(test, feature = "lua-debug"))]
    pub(crate) fn from_source(source: &str) -> io::Result<Self> {
        Self::from_source_checked(source).map_err(io::Error::from)
    }

    #[cfg(feature = "lua-debug")]
    pub(crate) fn from_source_checked(source: &str) -> Result<Self, AiScriptError> {
        let inner = AiScript::from_source("=patrol.lua", source)?;
        Ok(Self {
            policy: Policy::Lua(lua::LuaPolicy(inner)),
            digest: Sha256::digest(source.as_bytes()).into(),
        })
    }

    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
    }

    #[cfg(all(test, feature = "lua-debug"))]
    pub(crate) fn digest_hex(&self) -> String {
        format_digest(&self.digest)
    }

    pub(crate) fn wait(
        &self,
        _actor: AiActorSnapshot,
        event: WaitEvent,
        _tick: u32,
    ) -> io::Result<f64> {
        match &self.policy {
            Policy::StateScript(component) => {
                let message = match event {
                    WaitEvent::Spawn => SPAWN,
                    WaitEvent::Release => RELEASE,
                };
                let value = decide(component, message, None)?;
                let seconds = orpheus_statescript::script_expr::to_float(&value);
                if !matches!(value, Value::Float(_) | Value::Integer(_))
                    || !(0.0 < seconds && seconds <= MAX_WAIT)
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid patrol wait",
                    ));
                }
                Ok(seconds)
            }
            #[cfg(feature = "lua-debug")]
            Policy::Lua(policy) => policy
                .wait_checked(_actor, event, _tick)
                .map_err(io::Error::from),
        }
    }

    pub(crate) fn think(&self, actor: AiActorSnapshot, _tick: u32) -> io::Result<usize> {
        match &self.policy {
            Policy::StateScript(component) => {
                let route = actor.patrol.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "missing patrol route")
                })?;
                if route.route_index >= route.route_count {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid patrol route index",
                    ));
                }
                let value = decide(component, NEXT_WAYPOINT, Some(route))?;
                let slot = orpheus_statescript::script_expr::to_float(&value);
                if !matches!(value, Value::Float(_) | Value::Integer(_))
                    || !slot.is_finite()
                    || slot.fract() != 0.0
                    || slot < 0.0
                    || slot >= route.route_count as f64
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid patrol waypoint",
                    ));
                }
                Ok(slot as usize)
            }
            #[cfg(feature = "lua-debug")]
            Policy::Lua(policy) => policy.think_checked(actor, _tick).map_err(io::Error::from),
        }
    }

    #[cfg(feature = "lua-debug")]
    pub(crate) fn smoke_check(&self) -> Result<(), AiScriptError> {
        let Policy::Lua(lua) = &self.policy else {
            return Ok(());
        };
        let _ = lua.wait_checked(
            AiActorSnapshot::patrol_fixture(0, 4),
            WaitEvent::Spawn,
            FIRST_TICK,
        )?;
        let slot = lua.think_checked(AiActorSnapshot::patrol_fixture(0, 4), FIRST_TICK + 1)?;
        if slot >= 4 {
            return Err(AiScriptError::InvalidCommand(format!(
                "smoke check patrol_slot {slot} out of bounds for 4 slots"
            )));
        }
        let _ = lua.wait_checked(
            AiActorSnapshot::patrol_fixture(0, 4),
            WaitEvent::Release,
            FIRST_TICK + 2,
        )?;
        Ok(())
    }
}

// Patrol events are synchronous. The room's mover owns the time spent on each leg.
fn decide(
    component: &RefCell<Component>,
    message: u64,
    route: Option<AiPatrolSnapshot>,
) -> io::Result<Value> {
    let mut component = component.borrow_mut();
    let mut world = EmptyWorld;
    component
        .set_instance_variable(INSTANCE, DECISION, Value::Null, &mut world)
        .map_err(io::Error::other)?;
    if let Some(route) = route {
        component
            .set_instance_variable(
                INSTANCE,
                ROUTE_INDEX,
                Value::Integer(route.route_index as i64),
                &mut world,
            )
            .map_err(io::Error::other)?;
        component
            .set_instance_variable(
                INSTANCE,
                ROUTE_COUNT,
                Value::Integer(route.route_count as i64),
                &mut world,
            )
            .map_err(io::Error::other)?;
    }
    let recipients = component
        .queue_message(INSTANCE, message, Variables::new())
        .map_err(io::Error::other)?;
    if recipients != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing or ambiguous patrol decision handler",
        ));
    }
    component.run_now(&mut world).map_err(io::Error::other)?;
    component
        .resolved_variable(
            INSTANCE,
            Variable {
                id: DECISION,
                scope: Scope::Instance,
            },
        )
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "patrol decision VM halted"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_policy_preserves_waits_and_cyclic_route_choices() {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/behavior/statescript-patrol-decisions-v1.json"
        ))
        .unwrap();
        let input = &contract["input"];
        let expected = &contract["expected"];
        let count = input["route_count"].as_u64().unwrap() as usize;
        let actor = AiActorSnapshot::patrol_fixture(0, count);
        let script = PatrolScript::load().unwrap();
        assert_eq!(
            script.wait(actor, WaitEvent::Spawn, 1).unwrap(),
            expected["spawn_wait_seconds"].as_f64().unwrap()
        );
        for (index, next) in input["waypoints"]
            .as_array()
            .unwrap()
            .iter()
            .zip(expected["next_waypoints"].as_array().unwrap())
        {
            let actor = AiActorSnapshot::patrol_fixture(index.as_u64().unwrap() as usize, count);
            assert_eq!(
                script.wait(actor, WaitEvent::Release, 2).unwrap(),
                expected["release_wait_seconds"].as_f64().unwrap()
            );
            assert_eq!(
                script.think(actor, 3).unwrap(),
                next.as_u64().unwrap() as usize
            );
        }
    }

    #[cfg(feature = "lua-debug")]
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

    #[cfg(feature = "lua-debug")]
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
        let Policy::Lua(lua) = &script.policy else {
            unreachable!()
        };
        lua.0
            .set_memory_limit_for_test(lua.0.used_memory() + 512)
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
