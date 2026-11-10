//! Read-only Lua policy for patrol events on the match worker.

use std::{cell::Cell, io, rc::Rc};

use mlua::{ChunkMode, Function, HookTriggers, Lua, LuaOptions, StdLib, Value};

use crate::game_clock::TICK_PERIOD;

const SOURCE: &str = include_str!("../scripts/patrol.lua");
const ABI_VERSION: u32 = 1;
const MEMORY_LIMIT: usize = 256 * 1024;
const HOOK_INTERVAL: u32 = 64;
const HOOK_BUDGET: u8 = 8;
const MAX_WAIT_SECONDS: f64 = 60.0;

#[derive(Clone, Copy)]
pub(crate) enum WaitEvent {
    Spawn,
    Release,
}

pub(crate) struct PatrolScript {
    handler: Function,
    _lua: Lua,
    remaining: Rc<Cell<u8>>,
}

impl PatrolScript {
    pub(crate) fn load() -> io::Result<Self> {
        Self::from_source(SOURCE)
    }

    pub(crate) fn from_source(source: &str) -> io::Result<Self> {
        let lua = Lua::new_with(StdLib::NONE, LuaOptions::default()).map_err(script_error)?;
        lua.set_memory_limit(MEMORY_LIMIT).map_err(script_error)?;
        let remaining = Rc::new(Cell::new(HOOK_BUDGET));
        let budget = Rc::clone(&remaining);
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
            move |_, _| {
                let left = budget.get().saturating_sub(1);
                budget.set(left);
                if left == 0 {
                    return Err(mlua::Error::runtime("patrol instruction budget exhausted"));
                }
                Ok(mlua::VmState::Continue)
            },
        )
        .map_err(script_error)?;
        let (version, handler): (Value, Function) = lua
            .load(source)
            .set_name("=patrol.lua")
            .set_mode(ChunkMode::Text)
            .set_environment(lua.create_table().map_err(script_error)?)
            .call(())
            .map_err(script_error)?;
        if !matches!(version, Value::Integer(version) if version == i64::from(ABI_VERSION)) {
            return Err(invalid("unsupported patrol script ABI"));
        }
        Ok(Self {
            handler,
            _lua: lua,
            remaining,
        })
    }

    pub(crate) fn wait(&self, event: WaitEvent, frame: u32) -> io::Result<f64> {
        let event = match event {
            WaitEvent::Spawn => "spawn",
            WaitEvent::Release => "release",
        };
        let value = match self.call(event, frame, 0, 0)? {
            Value::Number(value) => value,
            Value::Integer(value) => value as f64,
            _ => return Err(invalid("patrol wait must be a number")),
        };
        if !(TICK_PERIOD.as_secs_f64()..=MAX_WAIT_SECONDS).contains(&value) {
            return Err(invalid("patrol wait is outside the allowed range"));
        }
        Ok(value)
    }

    pub(crate) fn think(&self, frame: u32, current: usize, count: usize) -> io::Result<usize> {
        let Value::Integer(index) = self.call("think", frame, current, count)? else {
            return Err(invalid("patrol route slot must be an integer"));
        };
        usize::try_from(index)
            .ok()
            .filter(|&index| index < count)
            .ok_or_else(|| invalid("patrol route slot is outside the authored route"))
    }

    fn call(&self, event: &str, frame: u32, current: usize, count: usize) -> io::Result<Value> {
        self.remaining.set(HOOK_BUDGET);
        self.handler
            .call((event, frame, current, count))
            .map_err(script_error)
    }
}

fn script_error(error: mlua::Error) -> io::Error {
    invalid(format!("patrol script failed: {error}"))
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_policy_preserves_waits_and_cyclic_route_choices() {
        let script = PatrolScript::load().unwrap();
        for _ in 0..1_000 {
            assert_eq!(script.wait(WaitEvent::Spawn, 1).unwrap(), 3.0);
            assert_eq!(script.think(188, 2, 4).unwrap(), 3);
            assert_eq!(script.wait(WaitEvent::Release, 200).unwrap(), 1.2);
            assert_eq!(script.think(275, 3, 4).unwrap(), 0);
        }
    }

    #[test]
    fn host_validates_command_type_and_range() {
        for source in [
            "return 1, function() return 1.5 end",
            "return 1, function() return -1 end",
            "return 1, function() return 3 end",
            "return 1, function() return '0' end",
        ] {
            let script = PatrolScript::from_source(source).unwrap();
            assert!(script.think(1, 0, 3).is_err());
        }
        for source in [
            "return 1, function() return 0/0 end",
            "return 1, function() return 61 end",
            "return 1, function() return 0 end",
            "return 1, function() return '3' end",
        ] {
            let script = PatrolScript::from_source(source).unwrap();
            assert!(script.wait(WaitEvent::Spawn, 1).is_err());
        }
    }

    #[test]
    fn environment_and_budgets_isolate_failing_handlers() {
        let script = PatrolScript::from_source(
            r#"
            return 1, function(event, frame)
                if event == "spawn" and frame == 1 then
                    while true do end
                elseif event == "spawn" and frame == 2 then
                    local rows = {}
                    for i = 1, 64 do rows[i] = {} end
                    return 3
                elseif io == nil and os == nil and load == nil and loadfile == nil
                    and require == nil and package == nil and coroutine == nil
                    and debug == nil and math == nil and _G == nil then
                    return 3
                end
            end
        "#,
        )
        .unwrap();
        assert!(
            script
                .wait(WaitEvent::Spawn, 1)
                .unwrap_err()
                .to_string()
                .contains("instruction budget exhausted")
        );
        assert_eq!(script.wait(WaitEvent::Spawn, 3).unwrap(), 3.0);
        assert!(PatrolScript::from_source("while true do end").is_err());
        assert!(PatrolScript::from_source("return 1").is_err());
        assert!(PatrolScript::from_source("return 2, function() end").is_err());
        assert!(PatrolScript::from_source("return '1', function() end").is_err());
        script
            ._lua
            .set_memory_limit(script._lua.used_memory() + 512)
            .unwrap();
        assert!(
            script
                .wait(WaitEvent::Spawn, 2)
                .unwrap_err()
                .to_string()
                .contains("memory")
        );
    }
}
