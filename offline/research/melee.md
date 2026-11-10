# Quick melee (graph 67)

Build 1.74; catalog `data/game_graphs_174.json.gz` (pinned upstream commit, `tools/lobby/vendor/README.md`).

## Where it lives

- Graph 67 is in 30 of the 32 bodies' `graphs` lists (not a weapon graph; `ability_kit.py` skips it as `SHARED`). Its
  per-body overrides are two contact-set config vars (`0x0D800000000002A3/A4`), not read here.
- Its StateAbility (node 14, state 6, owner 5) waits on **logical button 31**, `m_out_CooldownVar` 61, `m_out_StackVar` 33
  (the entity's action-priority stack, which weapon graphs' "reloading" Stacks also write).
- In the command record the key V sets **bit 0x800** (read from the lobby input log, 2026-11-04; Shift 0x08, E 0x10, Q 0x20,
  reload 0x400, jump 0x40, crouch 0x80, left mouse 0x01 with 0x800000 while held, right mouse 0x02).
- Layout: 28 states, 27 owner states (state 1, a server-only Wait, is skipped), 7 instance vars
  `{675: 0, 676: 1, 468: 2, 375: 3, 536: 4, 476: 5, 61: 6}`, 14 remote slots: `soldier.MELEE = Graph(67, 27, 7, 14)`.
- Entry chain (node 21): var476 = 30 (the damage), 215 = 0.5, 1258 = 0, 1257 = 1, 472 = cooldown struct, then switch 18
  (state 7, var1214 entity) false -> the StateAbility. Server-created instances run no Entry, so the kit sends var476 = 30
  and var61 bound to the ability state (full state index 6, 5 bits).
- The swing: ability -> `m_B969AFC9` -> 44 -> Wait 20 (state 8, 0.2 s; vibration 34, stat 54) -> subgraph 8 -> 7 (var788 = 0)
  -> TrackTargets 11 (state 5; radius expression over the entity) -> onBegin 9 (`STU_23A00A53`, state 3) -> subgraph:
  Anim 6 (state 2, alias `0x0C8000000000000D`, category `0x028000000000006A`, 0.76 s) and Anim 27 (state 12, alias
  `0x0C8000000000022A`, priority `0x0C4000000000008D`, 0.76 s), `STU_54FEB21C` 38. TrackTargets' subgraph: ModifyHealth 10
  (state 4), `STU_0A6BE10E` 63 -> `STU_77E13026` 60 -> Wait 61 -> PhysicsExplosion 64 (30), PhysicsExplosion 43.
- Weapon-graph melee overrides (button 31 in the weapon graph): Brigitte 4680 state 14, Torbjörn's hammer 5772 state 30.
  Not served: those heroes swing graph 67's generic melee too.

## Served (2026-11-04, `soldier.melee_instance`, `target.MELEE`)

- Every body gets the graph 67 instance with the ability state on (payload: the cooldown's `Volley.ability`), var476 and var61.
- A V press (`Player.melee`, a `soldier.Volley` on bit 0x800 with a 1 s cooldown, kept in `cooldowns`) pulses switch 6 false,
  Wait 7, Anim 1 and Anim 11 with the cast payload rules (`match.CAST_HOLD`), and the server applies 30 damage to the first
  enemy bot within 2.5 m of the aim (`Match.fire_at_target(player, command, MELEE)`).
- shortcut: TrackTargets 4 and state 3 (undecoded payloads) are left off; the first-person swing plays without them.
- shortcut: one damage for all heroes (Reinhardt 85, Brigitte 35, Torbjörn's hammer 55 and the Dragonblade/Primal Rage
  swings are not modelled); no knockback (PhysicsExplosion), no vibration, no stat.

## In game

- Mei (session `20261104T074455`, `.scratchpad/ws/mei`): the press logs `ability 0x800`, and the first-person swing plays:
  the left arm comes across the screen over four frames at 80 ms and the blaster comes back (`melee-f01..f04`). No assert.
- The swing's hit was not checked against a bot (no bot within 2.5 m in the sweep).
