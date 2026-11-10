# D.Va's mech (2026-11-07)

## Continuation: Matrix mechanics

Live follow-up: cannon suppression/depletion and two protected Range-fire windows passed. See [verification timings and captures](live-verification-2026-11-07/REPORT.md). Native transform/scale is still an approximation; the earlier initialization blocker below was repaired.

- Cannons stop on the same command that starts Matrix and resume on release or depletion. The rifle still advances with its fire trigger masked.
- Matrix consumes hostile Range shots and eligible hero-bot hitscan/projectile pellets. Friendly shots pass, allies behind Matrix are protected, and beam/melee primaries are excluded. Bot Helix/grenade/Flashbang bursts are checked too; their existing instantaneous timing remains approximate. Soundwave is not blocked.
- Graph `0x05800000000002CC`, DeflectProjectiles instance192 (`0EC57928`), has four inline boxes. Their expressions resolve to corners: (-1.25,-1,.8)..(0,0,10.25), (0,-1,.8)..(1.25,0,10.25), (-1.25,0,.8)..(0,1,10.25), (0,0,.8)..(1.25,1,10.25). Union: [-1.25,-1,.8]..[1.25,1,10.25]. `target.matrix_intersects` uses this union.
- Known limit: the entity-dependent `FDBB0B67` multiplier and native coordinate transform remain unproven. Server geometry uses scale 1 and the current eye/aim frame. See [native trace](soldier-runtime/presentation-next-steps-2026-11-07.md#matrix-native-follow-up-requested-during-research). This establishes simulation behavior, not retail visual alignment.
- Swept segments prevent fast shots tunneling through between updates. Only newly traveled distance is tested, so raising Matrix after a projectile passes the volume cannot erase it retroactively.
- Scope: the existing one-local-player bot combat paths. Player-cast visual-only projectiles are friendly and excluded; there is no new enemy-player projectile simulation. Meter UI, Boosters collision hit, Self-Destruct and pilot/eject/Call Mech remain open.
- Both desktop-control runtimes failed before initialization (`setup refresh had errors` / trusted process exit). No client was launched; the live regression checklist and Range acceptance remain open.

The following sections preserve the fork's earlier research and live checks. Statements that Matrix eats no shots or permits cannon fire describe that earlier implementation and are superseded by this section.

## Finding

- D.Va's hero record `0x02E000000000007A` names `m_gameplayEntity` = `0x040000000000040A`. That entity is the **pilot**:
  - health 150 (graph 51 schema var `0xC7`)
  - `STUWeaponComponent.m_weapons` = 3 slots: two empty (bag -1), then the Light Gun graph `0x02B8`
  - body graphs 51, 67, 75, `0x026A`, `0x0B49`, 2230
- The **mech** is entity `0x0400000000000407`, named only by her graph `0x026A` (which creates it in retail):
  - `STUPetComponent`, model look `0x09800000000014B0`
  - health 300 + armor 300 (schema vars `0xC7`, `0x6C8`), HUD texture `0x0C00000000007D85` (same as the pilot)
  - weapons: slot 0 empty, slot 1 = Fusion Cannons graph `0x050A`
  - body graphs `0x33` (51), `0x4B` (75), `0x276`, `0x2CC`, `0x47E`, `0x43` (67), `0xB2F`, `0x8B6` (2230), `0x1128`
- None of the mech's graphs are in the pinned catalog. Layouts from `tools/assets/graph_layout.py`:
  `0x050A (33, 10, 25)`, `0x0276 (58, 10, 36)`, `0x02CC (37, 19, 24)`, `0x047E (43, 10, 27)`, `0x0B2F (37, 7, 25)`,
  `0x1128 (16, 10, 12)`.
- No separate mech hero exists among the 56 STUHero records.

## Fusion Cannons graph 0x050A (state index -> owner index)

Client-only: Waits 16 and 17. Owner index = state index below 16, state index - 2 above 17.

From `graph_edges.py`:

1. Entry -> switch 0 (true) -> Stack 9 -> DFM 5; UXPresenter 30; LogicalButton 26; DFM chain 4 -> 6 -> 7 -> 15 -> 14 -> 23.
2. Button 26 `m_subgraphPlug` -> switch 1; its false branch -> switch 27 (true) -> Stack 10 -> condition -> Volley 3.
3. Volley 3 `m_onEndPlug` loops back to Stack 10.
4. Button 26 `m_80AF45FB` -> switch 18 -> `STU_BD02E168` 33 (client Waits 16/17), Stack 13 -> Anims 19/20, ChaseVars 22/21,
   Stack 29 -> Stack 31. Not sent (meaning of that plug not known).

Instance vars (presence bit: var): 0: 261, 1: 198, 2: 28, 3: 350, 4: 351, 5: 233, 6: 50, 7: 243, 8: 53, 9: 58.

## What the server does now

- `content.SPAWN_BODY`: D.Va's body is the mech `0x0407`.
- `kits.DVA`: graph `0x050A`, weapon_kit's rules applied by hand (switch 0 on, button 24, switch 1 false, switch 25 true,
  fire Stack 10, Volley 3, DFMs and the presenter on), infinite ammo.
- `target.PLAYER_KIT['D.Va']`: 600 health, Fusion Cannons from the wiki (11 pellets, 2 -> 0.6 damage over 10-20 m,
  0.144 s, 4.15 degree spread).
- `target.WEAPON_GRAPHS['D.Va']`: the mannequin's remote Volley (owner 3, remote slot 0, var53 bit 8).

## Verified live (Practice Range)

- Spawns in the mech: cockpit view with both cannons, HUD 600/600 (`research/main-menu/dva-mech-spawn.png`).
- Held fire: 5 shots logged in 0.6 s (0.144 s apart); tracers and impacts through a 1.5 s hold
  (`research/main-menu/dva-mech-fire-sheet.png`). Quick melee swings the cannon arm.
- The viewer mannequin (`OW_VIEWER_FILE` = D.Va) shows the 3P mech with the pilot inside, walks and fires
  (`research/main-menu/dva-mech-mannequin-sheet.png`).

## Mech ability graphs (state index / owner index)

Class and field names from `tools/assets/stu_names.json`; logical buttons from each StateAbility's
`STUConfigVarLogicalButton` (29 right mouse, 35 Shift, 36 E, 37 ultimate); cooldowns from the `STU_7197C080`
(var472) `m_01AFADB6` float. Instance-var bits are the scope-0 sync vars in slot order.

| Graph | Ability | Button | Cooldown | StateAbility | Icon presenters (Entry) | var472 / var61 bits | States |
|---|---|---|---|---|---|---|---|
| `0x047E (43, 10, 27, silent {10})` | Boosters | 35 Shift | 4 s | st0 / o0 | UX 31, 32 (UX 41 is under Wait 8) | 2 / 9 | 43 |
| `0x1128 (16, 10, 12)` | Micro Missiles | 36 E | 8 s | st0 / o0 | UX 12 | 6 / 9 | 16 |
| `0x02CC (37, 19, 24, silent {5, 16})` | Defense Matrix | 29 right mouse | 1 s | st0 / o0 | UX 23 | 8 / 18 | 50 |
| `0x0B2F (37, 7, 25, silent {3, 8, 15})` | Self-Destruct? | 37 | none (ultimate) | st0 / o0 | - | - | 44 |
| `0x0276 (58, 10, 36)` | ? (LogicalButton 25 / o19 on 37 under Wait 7) | 37 | - | - | - | - | 76 |

- Micro Missiles: Ability `m_B969AFC9` -> Wait 7 (the 0.256 s cast) `m_onFinishedPlug` -> WeaponVolley 5; Ability
  `m_6EEEC0AA` -> Anim 11 (`m_onEndPlug` -> Anim 10). The graph's only entity ref is the Volley's projectile
  `0x0400000000001E8A`, whose drawn child (field `436BEFE6`) is `0x0400000000001E99`.
- Defense Matrix: Ability -> Wait 26 -> switch 27 ...; `DeflectProjectiles` 3, Anims 24-26. Its resource meter
  (instance var 10581?) is not sent.
- Boosters: Ability -> ChaseVar 4, Stack 37; ModifyHealth 7, PhysicsExplosion 25 (the collision hit), Anims 1, 13, 14.

## What the server does for the abilities

- `kits.ABILITIES[DVA]`: the three non-ultimate graphs above (HUD icon, var472 cooldown, var61 sweep); Micro Missiles
  also pulses Wait 7 -> Volley 5 and Anim 11 on a press.
- `target.ABILITY_HITS[('D.Va', 0x10)]`: 18 missiles at 11/s (`Match._tick_salvos`), 7 direct + 4 splash in 1.5 m
  each, 40 m/s, from the Volley's two hardpoints in turn toward the crosshair's point. Hardpoints `0x0DC...75` /
  `...77` (graph 1128 inst 183/184, field `2D3CDFA9`) sit on mech model `0x0D000000000011CE` bones `0x13B` / `0x13C`:
  bind pose (+-0.74, 1.83, 0.35) m (right, up, forward). Drawn as `0x0400000000001E99`, now turned along the flight
  (`Projectile.rotation`); the projectile entity `0x1E8A` itself drew nothing when created.
- Defense Matrix (`kits.Held`): while right mouse is held and its meter var87 lasts it sends the active branch
  (switch 8 true -> effect 4, state 5, Anims 25/24, Stack 1 -> effects 29/28, DeflectProjectiles 3 -> BD02E168 36;
  switch 19 -> ChaseVar 32 draining var87 at var12097 = 50/s); released or empty, a 1 s cooldown, then var87 refills
  at var12096 = 16/s. The effects must be sent with their Begin bit cleared to stop: left out they stayed on, the
  matrix frame hanging in view (tracer: every other state ended, 7/41/42 did not;
  `research/asi-tracer/range-dva-matrix-2026-11-07.log`).
- Boosters (`kits.Timed`): a press runs 2 s (ChaseVar 4, var215 -> var567 = 2.0), a second press after 0.4 s ends
  it; it sends the movement modifier 316CFEF2 2, effect 3, Stacks 21/10/40/11/37/30, Anims 1/13, switch 42 false ->
  C1BA7545 36, and the server's `Mover` flies 11.99 m/s along the aim (`Mover.flight`), stopped by walls and floors.
- DSR widths found statically (`config_var_eval.resolve` -> registration record -> state-constructors registry ->
  the factory's vtable +0x158): DeflectProjectiles, PhysicsExplosion, `C1BA7545` read no bits (empty stub);
  `75D1C308` reads the 1-bit prelude only. A factory that sets the shared base vtable `0x7FF6D383BDF0` gives no answer.
- shortcuts: Boosters deal no collision damage or knockback and fly a straight line (no acceleration or steering
  read from the client); Defense Matrix eats no shots, the cannons still fire while it is up, and no meter bar is
  drawn (presenter 23 has no meter binding); the missiles have no 0.256 s cast time or spread and leave the
  bind-pose hardpoints; Self-Destruct is an ultimate (no ult meter yet).
- Live (2026-11-07, session `20261107T082401`): E draws two missile streams from the pods that meet at the crosshair
  (`research/main-menu/dva-mech-missiles-sheet.png`); held right mouse shows the matrix frame for the hold and it
  clears on release (`dva-mech-matrix-sheet.png`); Shift flies the mech out through the doorway for 2 s with the
  boost cockpit view (`dva-mech-boosters-sheet.png`).
- Live check (Practice Range, 2026-11-07, sessions `20261107T072159` and `20261107T073348`):
  - All three icons draw beside the Fusion Cannons. Shift -> Boosters' icon greys and counts 4, 3, ...; E ->
    Micro Missiles counts 8, 7, ...; right mouse -> Defense Matrix counts 1 (`research/main-menu/dva-mech-ability-hud.png`).
  - E fires the missiles: launch flash and a red missile flying off the cannons (`dva-mech-missiles-sheet.png`).
  - Held right mouse (1.5 s) logs two presses: the 1 s cooldown re-arms while held.
- The red "unavailable" mark (no-entry sign) on Micro Missiles and Defense Matrix, fixed 2026-11-07:
  - Each icon presenter binds UI property `0x0D80000000000786` (the mark) to an `STU_919FD47C` expression. Decoded
    (`research/assault-mode-graph.md` opcodes; `08 i` push dynamic var i, `03 n` or-jump, `1E` not):
    1128/12 and 02CC/23: `v2575 || v2575(entity) || !v4993`; Boosters 047E: `v2575 || v2575(entity)`.
  - So var2575 means *unavailable* (not usable) and var4993 *available*. The graph's Entry switch (1128 switch 3,
    02CC switch 21) sets var4993 on its true branch; the client never gets it, so `!v4993` drew the mark.
  - `kits.ABILITIES[DVA]` sends var4993 = True (1128 bit 0, 02CC bit 1). Live: all three icons clean, then
    Boosters counts 4, Micro Missiles 8, Defense Matrix 1 (`research/main-menu/dva-mech-ability-hud.png`, first
    crop at rest); the missiles hit the floor ahead (`dva-mech-missiles-hit.png`).
  - Earlier misses (each a fresh run): the Entry constants, the switch branch that sets var2575, and var2575 = True
    sent directly (that one *is* the mark).
  - Sombra's Stealth (graph 3227) has `0x0786` = `v2575 || v2575(entity)`, and its kit sends var2575 = True
    (research/abilities.md): that was her red overlay; dropped, her icon is clean and still counts down.

## Open

- Abilities: Boosters' collision hit, Defense Matrix eating shots and its meter bar; Self-Destruct (graph `0x0B2F`) and graph `0x0276` (button 37).
- Eject and the pilot (Light Gun kit was `Kit(Graph(0x02B8, 41, 13, 25), ...)`, see git history of `kits.py`); Call Mech.
- Button plug `m_80AF45FB` branch (cannon spin-up anims 19/20?) not sent.
- Hit volumes are Soldier's (`hitbox.HEROES` has no mech entry); no movement penalty while firing.
- The mannequin's shots land at its feet (not checked whether this is D.Va-specific).
