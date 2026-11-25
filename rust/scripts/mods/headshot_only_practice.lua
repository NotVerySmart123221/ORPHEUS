-- Example ORPHEUS Lua 5.4 Workshop Mod: Headshot-Only Practice
-- Non-headshot weapon hits deal 0 damage; headshots grant +25 HP self-heal.
-- Quick melee still works so players can finish off low-HP targets.

local headshot_kills = 0

return {
    abi = 1,
    info = {
        name = "Headshot-Only Practice",
        version = "1.0.0",
        author = "ORPHEUS",
    },
    rules = {
        damage_scale = 1.0,
        healing_scale = 1.0,
        gravity_scale = 1.0,
        speed_scale = 1.0,
        ult_charge_scale = 1.0,
        cooldown_scale = 1.0,
        friendly_fire = false,
    },
    on_room_start = function(ctx)
        headshot_kills = 0
        return {
            { kind = "broadcast_chat", text = "[Mod] Headshot-Only Practice enabled! Type !hs for score." },
        }
    end,
    on_damage = function(ctx, event)
        if not event.headshot and not event.melee then
            return { damage = 0.0, actions = {} }
        end
        if event.headshot then
            return {
                damage = event.damage,
                actions = {
                    { kind = "heal", entity = event.attacker, amount = 25.0 },
                },
            }
        end
        return { damage = event.damage, actions = {} }
    end,
    on_kill = function(ctx, event)
        if event.headshot then
            headshot_kills = headshot_kills + 1
            return {
                {
                    kind = "broadcast_chat",
                    text = "[Mod] Crisp headshot! Total headshot eliminations: " .. tostring(headshot_kills),
                },
            }
        end
        return {}
    end,
    on_chat = function(ctx, sender, text)
        if text == "!hs" then
            return {
                {
                    kind = "broadcast_chat",
                    text = "[Mod] Headshot eliminations this session: " .. tostring(headshot_kills),
                },
            }
        end
        return {}
    end,
}
