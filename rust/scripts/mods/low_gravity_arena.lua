-- Example ORPHEUS Lua 5.4 Workshop Mod: Low-Gravity Arena
-- Run with:
--   orpheus_server --features lua-mods --mod-script rust/scripts/mods/low_gravity_arena.lua ...
-- Supports live hot-reload by typing `reload` + Enter in the server console.

local streaks = {}

return {
    abi = 1,
    info = {
        name = "Low-Gravity Arena",
        version = "1.0.0",
        author = "ORPHEUS",
    },
    rules = {
        damage_scale = 1.0,
        healing_scale = 1.0,
        gravity_scale = 0.45,
        speed_scale = 1.2,
        ult_charge_scale = 1.5,
        cooldown_scale = 1.0,
        friendly_fire = false,
    },
    on_room_start = function(ctx)
        return {
            { kind = "broadcast_chat", text = "[Mod] Low-Gravity Arena active! Type !grav or !help in chat." },
        }
    end,
    on_button = function(ctx, player, button)
        if button == "jump" then
            return {
                { kind = "apply_impulse", entity = player.entity, velocity = { 0.0, 4.5, 0.0 } },
            }
        elseif button == "melee" then
            return {
                { kind = "apply_impulse", entity = player.entity, velocity = { 0.0, 3.0, 0.0 } },
            }
        end
        return {}
    end,
    on_kill = function(ctx, event)
        local count = (streaks[event.killer] or 0) + 1
        streaks[event.killer] = count
        streaks[event.victim] = 0
        if count == 3 or count == 5 or count >= 10 then
            return {
                {
                    kind = "broadcast_chat",
                    text = "[Mod] Elimination streak: " .. tostring(count) .. "!",
                },
            }
        end
        return {}
    end,
    on_chat = function(ctx, sender, text)
        if text == "!help" then
            return {
                {
                    kind = "broadcast_chat",
                    text = "[Mod] Low-Gravity Arena: !grav <0.2..2.0> adjusts gravity.",
                },
            }
        elseif text == "!grav" then
            return {
                { kind = "set_room_rule", key = "gravity_scale", value = 0.45 },
                { kind = "broadcast_chat", text = "[Mod] Gravity reset to 0.45x." },
            }
        end
        return {}
    end,
}
