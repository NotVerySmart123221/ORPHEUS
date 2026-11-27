-- ORPHEUS Workshop Showcase Mod (ABI v2): Dance Party & Sandbox Playground
-- Run with:
--   orpheus_server --features lua-mods --mod-script rust/scripts/mods/dance_party_workshop.lua ...
--
-- In-game chat commands:
--   !dance [0..3]       Make your hero play emote / dance slot 0..3
--   !danceall [0..3]    Make ALL players AND bots dance together!
--   !stopdance          Return all bots to normal AI
--   !voice [0..3]       Play voiceline slot 0..3
--   !spray [0..3]       Place spray slot 0..3
--   !hello              Say Hello!
--   !hero <name>        Swap hero live (e.g. !hero tracer, !hero dva, !hero reinhardt)
--   !ult                Charge ultimate to 100% and reset all cooldowns
--   !tp                 Blink / teleport to where you are aiming (world.raycast)
--   !slowmo <0.1..3.0>  Set slow-motion or fast-forward time scale
--   !grav <0.1..5.0>    Set room gravity scale
--   !speed <0.2..5.0>   Set room movement speed scale
--   !cd <0.1..5.0>      Set ability cooldown multiplier (e.g. !cd 0.25 = 4x faster)
--   !bot <hero>         Spawn a friendly/enemy bot in front of you
--   !fireball           Launch a custom Workshop plasma orb forward
--   !heal               Full heal + clear negative status effects
--   !stun               Stun nearby enemies within 8m for 2s
--   !reset              Reset rules and time scale to defaults
--   !help               Print command list

local party_mode = false

local function split_words(text)
    local words = {}
    for w in string.gmatch(text, "%S+") do
        table.insert(words, w)
    end
    return words
end

local function sender_id(ctx, sender_name)
    if ctx.sender_entity ~= nil then
        return ctx.sender_entity
    end
    for _, p in ipairs(ctx.players or {}) do
        if p.name == sender_name then
            return p.entity
        end
    end
    if ctx.players and ctx.players[1] then
        return ctx.players[1].entity
    end
    return nil
end

return {
    abi = 2,
    info = {
        name = "Dance Party & Workshop Sandbox",
        version = "2.0.0",
        author = "ORPHEUS",
        description = "Full ABI v2 showcase: emotes/dance, hero swap, raycast TP, slow-mo, bot spawning, custom projectiles, and status control.",
    },
    rules = {
        damage_scale = 1.0,
        healing_scale = 1.0,
        gravity_scale = 1.0,
        speed_scale = 1.0,
        ult_charge_scale = 1.5,
        cooldown_scale = 1.0,
        time_scale = 1.0,
        friendly_fire = false,
    },

    on_room_start = function(ctx, world)
        party_mode = false
        return {
            {
                kind = "announce",
                text = "[Workshop] Dance Party Sandbox ready! Type !dance, !danceall, !hero <name>, !ult, !tp, !slowmo 0.4, or !help.",
            },
        }
    end,

    on_player_spawn = function(ctx, player, world)
        if party_mode then
            return {
                { kind = "play_emote", entity = player.entity, slot = 0 },
            }
        end
        return {}
    end,

    on_button = function(ctx, player, button, pressed, world)
        -- Hold Interact (F) to trigger a quick dance emote!
        if pressed and button == "interact" then
            return {
                { kind = "play_emote", entity = player.entity, slot = 0 },
            }
        end
        return {}
    end,

    on_chat = function(ctx, sender, text, world)
        local words = split_words(text)
        if #words == 0 then
            return {}
        end
        local cmd = string.lower(words[1])
        local me = sender_id(ctx, sender)

        if cmd == "!help" then
            return {
                {
                    kind = "broadcast_chat",
                    text = "[Workshop] Commands: !dance [0-3], !danceall, !stopdance, !voice [0-3], !spray [0-3], !hero <name>, !ult, !tp, !slowmo <x>, !grav <x>, !speed <x>, !cd <x>, !bot <hero>, !fireball, !heal, !stun, !reset",
                },
            }
        elseif cmd == "!dance" or cmd == "!emote" then
            if not me then return {} end
            local slot = math.clamp(math.floor(tonumber(words[2]) or 0), 0, 3)
            return {
                { kind = "play_emote", entity = me, slot = slot },
                { kind = "broadcast_chat", text = "[Workshop] " .. sender .. " started dancing (slot " .. tostring(slot) .. ")!" },
            }
        elseif cmd == "!danceall" or cmd == "!party" then
            party_mode = true
            local slot = math.clamp(math.floor(tonumber(words[2]) or 0), 0, 3)
            local actions = {}
            for _, p in ipairs(ctx.players or {}) do
                if p.alive then
                    table.insert(actions, { kind = "play_emote", entity = p.entity, slot = slot })
                end
            end
            for _, b in ipairs(ctx.bots or {}) do
                if b.alive then
                    table.insert(actions, { kind = "set_bot_ai", entity = b.entity, mode = OW.BOT_AI.DANCE })
                end
            end
            table.insert(actions, { kind = "announce", text = "[Workshop] DANCE PARTY! Everybody dance!" })
            return actions
        elseif cmd == "!stopdance" then
            party_mode = false
            local actions = {}
            for _, b in ipairs(ctx.bots or {}) do
                table.insert(actions, { kind = "set_bot_ai", entity = b.entity, mode = OW.BOT_AI.DEFAULT })
            end
            table.insert(actions, { kind = "broadcast_chat", text = "[Workshop] Dance party ended; bots resumed normal AI." })
            return actions
        elseif cmd == "!voice" then
            if not me then return {} end
            local slot = math.clamp(math.floor(tonumber(words[2]) or 0), 0, 3)
            return { { kind = "play_voiceline", entity = me, slot = slot } }
        elseif cmd == "!spray" then
            if not me then return {} end
            local slot = math.clamp(math.floor(tonumber(words[2]) or 0), 0, 3)
            return { { kind = "place_spray", entity = me, slot = slot } }
        elseif cmd == "!hello" then
            if not me then return {} end
            return { { kind = "communication", entity = me, message = OW.COMMS.HELLO } }
        elseif cmd == "!hero" and words[2] then
            if not me then return {} end
            local query = table.concat(words, " ", 2)
            local found = world.find_hero(query)
            local hero_name = found and found.name or query
            return {
                { kind = "set_hero", entity = me, hero = hero_name },
                { kind = "broadcast_chat", text = "[Workshop] Switching " .. sender .. " to " .. hero_name .. "..." },
            }
        elseif cmd == "!ult" then
            if not me then return {} end
            return {
                { kind = "set_ult_charge", entity = me, percent = 100 },
                { kind = "reset_cooldowns", entity = me },
                { kind = "broadcast_chat", text = "[Workshop] " .. sender .. " ultimate ready (100%) & cooldowns reset!" },
            }
        elseif cmd == "!tp" then
            if not me then return {} end
            local p = world.get_entity(me)
            if not p then return {} end
            local eye = vec3.add(p.position, { 0.0, 1.5, 0.0 })
            local hit = world.raycast(eye, p.facing, 45.0)
            local dest
            if hit.hit then
                dest = vec3.add(hit.point, vec3.scale(hit.normal, 0.6))
            else
                dest = vec3.add(eye, vec3.scale(p.facing, 15.0))
            end
            return {
                { kind = "teleport", entity = me, position = dest },
            }
        elseif cmd == "!slowmo" then
            local scale = math.clamp(tonumber(words[2]) or 0.4, 0.1, 3.0)
            return {
                { kind = "set_time_scale", scale = scale },
                { kind = "broadcast_chat", text = string.format("[Workshop] Time scale set to %.2fx", scale) },
            }
        elseif cmd == "!grav" then
            local scale = math.clamp(tonumber(words[2]) or 0.45, 0.1, 5.0)
            return {
                { kind = "set_room_rule", key = "gravity_scale", value = scale },
                { kind = "broadcast_chat", text = string.format("[Workshop] Gravity set to %.2fx", scale) },
            }
        elseif cmd == "!speed" then
            local scale = math.clamp(tonumber(words[2]) or 1.5, 0.2, 5.0)
            return {
                { kind = "set_room_rule", key = "speed_scale", value = scale },
                { kind = "broadcast_chat", text = string.format("[Workshop] Move speed set to %.2fx", scale) },
            }
        elseif cmd == "!cd" then
            local scale = math.clamp(tonumber(words[2]) or 0.25, 0.1, 5.0)
            return {
                { kind = "set_room_rule", key = "cooldown_scale", value = scale },
                { kind = "broadcast_chat", text = string.format("[Workshop] Cooldown scale set to %.2fx", scale) },
            }
        elseif cmd == "!bot" then
            if not me then return {} end
            local p = world.get_entity(me)
            if not p then return {} end
            local hero = words[2] or "soldier"
            local team = tonumber(words[3]) or (p.team == 0 and 1 or 0)
            local pos = vec3.add(p.position, vec3.scale(p.facing, 5.0))
            return {
                { kind = "spawn_bot", hero = hero, team = team, position = pos, name = "Workshop Bot" },
                { kind = "broadcast_chat", text = "[Workshop] Spawned bot (" .. hero .. ") ahead!" },
            }
        elseif cmd == "!fireball" then
            if not me then return {} end
            local p = world.get_entity(me)
            if not p then return {} end
            local origin = vec3.add(vec3.add(p.position, { 0.0, 1.4, 0.0 }), vec3.scale(p.facing, 1.0))
            return {
                {
                    kind = "spawn_projectile",
                    source = me,
                    origin = origin,
                    direction = p.facing,
                    speed = 32.0,
                    damage = 85.0,
                    radius = 2.5,
                    gravity = 4.0,
                },
            }
        elseif cmd == "!heal" then
            if not me then return {} end
            return {
                { kind = "clear_status", entity = me, status = "all" },
                { kind = "heal", entity = me, amount = 1000.0 },
                { kind = "broadcast_chat", text = "[Workshop] Full heal & status cleanse applied." },
            }
        elseif cmd == "!stun" then
            if not me then return {} end
            local p = world.get_entity(me)
            if not p then return {} end
            local nearby = world.players_in_radius(p.position, 8.0)
            local actions = {}
            for _, other in ipairs(nearby) do
                if other.entity ~= me and other.alive then
                    table.insert(actions, {
                        kind = "set_status",
                        entity = other.entity,
                        status = OW.STATUS.STUN,
                        duration = 2.0,
                    })
                end
            end
            return actions
        elseif cmd == "!reset" then
            party_mode = false
            return {
                { kind = "set_time_scale", scale = 1.0 },
                { kind = "set_room_rule", key = "gravity_scale", value = 1.0 },
                { kind = "set_room_rule", key = "speed_scale", value = 1.0 },
                { kind = "set_room_rule", key = "cooldown_scale", value = 1.0 },
                { kind = "set_room_rule", key = "damage_scale", value = 1.0 },
                { kind = "set_room_rule", key = "healing_scale", value = 1.0 },
                { kind = "broadcast_chat", text = "[Workshop] All rules reset to 1.0x defaults." },
            }
        end
        return {}
    end,
}
