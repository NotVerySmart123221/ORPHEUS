# ORPHEUS Lua 5.4 Workshop / Mod API (ABI v2)

ORPHEUS supports sandboxed, hot-reloadable Lua 5.4 Workshop / Mod scripts via `--features lua-mods` and `--mod-script <path>`.

## Architecture & Safety Guarantees

- **Per-Room VM Isolation**: Every match room instantiates its own independent Lua 5.4 VM from the loaded script source (`ModHost::instantiate_for_room`). Concurrent rooms never share mutable Lua globals or contend on a shared mutex during 60 Hz ticks.
- **Strict Sandbox**: `os`, `io`, `package`, `require`, `debug`, `dofile`, `loadfile`, `load`, `collectgarbage`, and `string.dump` are never exposed. `string.rep` is bounded to 4,096 bytes.
- **Hard Resource Limits**:
  - Heap memory: **512 KiB** per room VM
  - Instruction budget: **16,384** VM instructions per init and per hook invocation
  - Source file cap: **128 KiB**
  - Action cap: **64** actions per hook invocation
  - Per-hook fault isolation: a faulty hook is silenced after 5 errors in a generation without crashing the room or silencing unrelated hooks.
- **Backwards Compatibility**: Both `abi = 1` and `abi = 2` scripts are supported seamlessly.
- **Live Hot-Reload**: Type `reload` + Enter in the server console to re-validate and hot-swap the mod across all live rooms (invalid edits roll back cleanly).

## Launching with a Mod Script

```powershell
.\target\release\orpheus_server.exe --runtime "<ORPHEUS_RUNTIME_DIR>" --mod-script rust/scripts/mods/dance_party_workshop.lua
```

Included showcase mods:
- `rust/scripts/mods/dance_party_workshop.lua` — Full ABI v2 showcase (`!dance`, `!danceall`, `!stopdance`, `!voice`, `!spray`, `!hello`, `!hero <name>`, `!ult`, `!tp`, `!slowmo <x>`, `!grav <x>`, `!speed <x>`, `!cd <x>`, `!bot <hero>`, `!fireball`, `!heal`, `!stun`, `!reset`, `!help`)
- `rust/scripts/mods/low_gravity_arena.lua` — ABI v1 Low-Gravity Arena with jump/melee impulses and streaks
- `rust/scripts/mods/headshot_only_practice.lua` — ABI v1 Headshot-Only Practice with crit lifesteal

## Script Structure

```lua
return {
    abi = 2,
    info = {
        name = "My Workshop Mode",
        version = "1.0.0",
        author = "Author",
        description = "Short summary",
    },
    rules = {
        damage_scale = 1.0,      -- [0.0, 10.0]
        healing_scale = 1.0,     -- [0.0, 10.0]
        gravity_scale = 1.0,     -- [0.0, 10.0]
        speed_scale = 1.0,       -- [0.0, 10.0]
        ult_charge_scale = 1.0,  -- [0.0, 10.0]
        cooldown_scale = 1.0,    -- [0.0, 10.0] (0.5 = 2x faster cooldowns)
        time_scale = 1.0,        -- [0.1, 3.0]  (slow-mo / fast-forward)
        friendly_fire = false,
    },
    on_room_start = function(ctx, world) ... end,
    on_player_join = function(ctx, player, world) ... end,
    on_player_leave = function(ctx, player, world) ... end,
    on_player_spawn = function(ctx, player, world) ... end,
    on_hero_switch = function(ctx, player, old_hero_guid, new_hero_guid, world) ... end,
    on_damage = function(ctx, event, world) ... end,
    on_heal = function(ctx, event, world) ... end,
    on_kill = function(ctx, event, world) ... end,
    on_button = function(ctx, player, button, pressed, world) ... end,
    on_emote = function(ctx, player, slot, world) ... end,
    on_chat = function(ctx, sender, text, world) ... end,
    on_tick = function(ctx, dt, world) ... end,
}
```

## Sandbox Libraries & Helpers

- **Base**: `tostring`, `tonumber`, `pairs`, `ipairs`, `next`, `type`, `select`, `pcall`, `xpcall`, `error`, `assert`, `rawget`, `rawset`, `rawequal`, `rawlen`
- **String & UTF-8**: `string.{byte,char,find,format,gmatch,gsub,len,lower,match,rep,reverse,sub,upper}`, `utf8.{char,codepoint,codes,len,offset,charpattern}`
- **Table**: `table.{insert,remove,sort,concat,unpack,move,pack}`
- **Math**: full Lua `math.*`, plus `math.clamp(x, lo, hi)`, `math.lerp(a, b, t)`, `math.round(x)`, and room-isolated deterministic `math.random([m [, n]])`
- **Vectors (`vec3`)**: `vec3.{new,add,sub,scale,dot,cross,len,dist,normalize,lerp}`
- **Constants (`OW`)**: `OW.ABI_VERSION`, `OW.STATUS.*`, `OW.BOT_AI.*`, `OW.COMMS.*`

## Context & Read-Only World Queries

### `ctx`
- `ctx.abi_version`, `ctx.tick`, `ctx.elapsed_secs`, `ctx.map_guid`, `ctx.mode_guid`, `ctx.player_count`, `ctx.time_scale`, `ctx.sender_entity`
- `ctx.rules`: current live `ModRules` table
- `ctx.players` / `ctx.bots`: arrays of actor snapshots (`entity`, `name`, `team`, `hero_guid`, `hero_name`, `health`, `max_health`, `ult_charge`, `position`, `velocity`, `yaw`, `pitch`, `facing`, `alive`, `crouching`, `grounded`, `emoting`, `is_bot`)

### `world`
- `world.raycast(origin, direction, max_distance)` -> `{ hit = true, distance, point, normal }` or `{ hit = false }`
- `world.distance(a, b)` -> Euclidean distance between two `{x, y, z}` vectors
- `world.players_in_radius(center, radius [, team])` -> array of matching players and bots
- `world.get_entity(entity_id)` -> actor snapshot or `nil`
- `world.find_hero(name_or_guid)` -> `{ guid, name, role }` or `nil` (data-driven from `game_heroes_174.json`)
- `world.heroes()` -> list of all heroes in the scene catalog

## Button Names (`on_button`)

Mapped directly to Overwatch 1.74 wire input masks (`pressed` is `true` on press, `false` on release):
`"primary"`, `"secondary"`, `"weapon_swap"`, `"ability1"`, `"ability2"`, `"ultimate"`, `"jump"`, `"crouch"`, `"reload"`, `"melee"`, `"interact"`.

## Supported Actions

| Category | `kind` | Fields |
|---|---|---|
| Emotes / Comms | `play_emote` | `entity`, `slot` (`0..3`, triggers native OW 1.74 emote on players or dance routine on bots) |
| Emotes / Comms | `play_voiceline` | `entity`, `slot` (`0..3`) |
| Emotes / Comms | `place_spray` | `entity`, `slot` (`0..3`) |
| Emotes / Comms | `communication` | `entity`, `message` (`"hello"`, `"need_healing"`, `"thanks"`) |
| Emotes / Comms | `play_voice_cue` | `entity`, `stimulus` (u64 stimulus ID -> emits `CombatNotice::Voice`) |
| Heroes & Ult | `set_hero` | `entity`, `hero` (hero name or GUID -> live native hero switch) |
| Heroes & Ult | `set_ult_charge` | `entity`, `percent` (`0..100`) |
| Heroes & Ult | `add_ult_charge` | `entity`, `amount` (raw ult points) |
| Heroes & Ult | `reset_cooldowns` | `entity` (immediately finishes all active ability cooldowns) |
| Health | `set_health` / `damage` / `heal` | `entity`, `health` or `amount` |
| Health | `kill` / `respawn` | `entity` |
| Health | `add_health_pool` | `entity`, `pool` (`"health"`, `"armor"`, `"shield"`), `amount` |
| Status Effects | `set_status` | `entity`, `status` (`"stun"`, `"sleep"`, `"freeze"`, `"anti_heal"`, `"heal_boost"`, `"hack"`, `"invulnerable"`, `"cc_immune"`), `duration` |
| Status Effects | `clear_status` | `entity`, `status` (status name or `"all"`) |
| Movement | `apply_impulse` / `set_velocity` | `entity`, `velocity` (`{x, y, z}`) |
| Movement | `teleport` | `entity`, `position` (`{x, y, z}`) |
| Movement | `set_facing` | `entity`, `yaw` (deg), `pitch` (deg, optional) |
| Movement | `set_speed_scale` / `set_gravity_scale` | `entity`, `scale` (`0..10`) |
| Combat Scaling | `set_damage_scale` / `set_damage_taken_scale` / `set_healing_scale` | `entity`, `scale` (`0..10`) |
| Bots | `spawn_bot` | `hero` (name or GUID), `team` (`0`/`1`), `position`, `name` (optional) |
| Bots | `despawn_bot` | `entity` |
| Bots | `set_bot_ai` | `entity`, `mode` (`"default"`, `"idle"`, `"dance"`, `"follow"`) |
| Projectiles | `spawn_projectile` | `source`, `origin`, `direction`, `speed`, `damage`, `radius` (opt), `gravity` (opt) |
| Room & Chat | `set_time_scale` | `scale` (`0.1..3.0`) |
| Room & Chat | `set_room_rule` | `key`, `value` |
| Room & Chat | `reset_interactive_prop` / `apply_prop_impulse` | `prop_id` (, `velocity`) |
| Room & Chat | `broadcast_chat` / `send_chat` / `announce` | `text` (, `entity`) |
