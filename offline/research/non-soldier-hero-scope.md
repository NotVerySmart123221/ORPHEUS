# Scope: a playable hero other than Soldier: 76 (2026-11-03)

Doc only, no code. Today any pick in hero select plays Soldier (`match.py`, PICK_HERO branch). This lists what is Soldier-specific, what a second hero needs, and a suggested first pick.

## What is Soldier-specific now

| Piece | Where | Per-hero? |
|---|---|---|
| Pick gate: any other hero becomes Soldier | `match.py` PICK_HERO (`hero.guid != SOLDIER`) | remove per playable hero |
| Body script delay and draw | `match.py` `_spawn_steps` caller (`player.hero.guid == SOLDIER`, `SOLDIER_DRAW_DELAY`) | gate on "has a kit" |
| Fire detection and hit test | `Player` command loop (`hero.guid == SOLDIER`, `self.rifle.command`) then `fire_at_target` | needs a generic weapon |
| Weapon graph instance | `soldier.weapon_instance`, `RIFLE = Graph(596, 81, 23, 49)` | layout per hero: `target.WEAPON_GRAPHS` already has all 12 (layout, Volley owner state, ...) |
| Weapon owner states and payloads | `soldier.initialize_practice_soldier` (14 plain states + 10 with payloads, found by live probing) | **unknown per hero**: the main cost |
| Body hosts | same: graphs 21, 75, 4429, 463 | 21/75/4429 look shared; 463's presence bit 0 is an asset `0x0C00000000007DA7` (Soldier's HUD texture?), per hero |
| Abilities | `Sprint`, `Volley` (Helix, Biotic Field), visor: graphs 597, 599, 601 | per hero; can start with none |
| Ult meter | `ULT_COST_VAR`, `ULT_CHARGE` (entity vars 338, 342) | the vars are generic; the cost (2310 points) is per hero |
| Damage | `soldier.rifle_damage` | `target.HERO_KIT` already has every roster weapon (damage, falloff, cadence, magazine, reload) |
| Hit volumes | `hitbox.HEROES` | done for all 12 |

## Suggested first hero: Cassidy

- Hitscan, one bullet per shot, 6-round magazine, no charge: the closest kit to Soldier's rifle, so `fire_at_target` and `HERO_KIT['Cassidy']` cover the damage side.
- Weapon graph `0x01D2` = Graph(0x01D2, 63, 18, 38); Volley owner state 11 (`target.WEAPON_GRAPHS`). Bots already fire it visibly through the ch2 shot record.
- Difference from Soldier: his Volley is not automatic (config mode 3 is Soldier's), so the client does not run the shot itself. A local shot may need the server's shot record (as for bots) on the player's own body, or an owner Volley payload.

## Steps

1. **Done 2026-11-03** (`Rifle(quanta, weapon)`). Generic weapon on `Player`: replace `self.rifle` (Soldier `Rifle`) with a class driven by `HERO_KIT[hero].weapon` (cadence, magazine, reload); Soldier keeps his numbers. Unit-test the cadence and reload.
2. Weapon instance per hero: build it from `WEAPON_GRAPHS[hero]` (layout and Volley state) with ammo presence (var53, bit from `graph_layout.sync_vars`).
3. Probe the owner states: start from Soldier's set mapped by node class (Stack, Wait, Volley), run with Cassidy, screenshot first person. Use the `.scratchpad/zarya_probe.py` style TEMP toggles. Done when the revolver shows in first person and the ammo counter reads 6.
4. Fire: on a fire command, check cadence, send the Volley owner payload, run `fire_at_target` with Cassidy's damage; confirm a bot takes 70 at close range.
5. HUD: find Cassidy's asset for graph 463 presence bit 0 (search 0x0C assets referenced by his hero STU); health shows 225.
6. Abilities: none first (shortcut), then Combat Roll (movement) and Flashbang (bots already have the damage side in `target.ABILITIES`).
7. Lift the pick gate for Cassidy only.

## Results (2026-11-03)

- **Steps 2, 3, 4, 7 done for Cassidy** (`tools/lobby/cassidy.py`, `match.KITS`). Owner states were mapped from Soldier's set by their place in the catalog tree (`.scratchpad/gtree.py`), and the first native run worked:
  - st0 on; st2, st1 off; st34 on: the path to the fire button st12 (Soldier: st1, st2 off, st76 on, st8).
  - st10 on, st44 off: the ammo UXPresenter st50 (Soldier: st67 off, st70).
  - CosmeticEntity st27 on; var49 (weapon entity) is entity-scoped in graph 0x01D2, bound to st27 with 6 state bits.
  - Instance presence: bit 6 var53 ammo, bit 10 var198 capacity (`graph_layout.sync_vars`, scope 0).
- Native run: the revolver shows in first person, the HUD has Cassidy's portrait (texture `0x0C00000000007D2E`, body graph75 var1807) and reads 6/6. Each click fires: recoil pose, ammo 6→5→4→3 (`main-menu/cassidy-revolver.png`, `cassidy-fired.png`).
- Damage uses `target.HERO_KIT` for non-Soldier kits: a hit at 32.6 m took 39 (70→21 over 20–40 m). Headshots ×2 is a shortcut.
- Player health follows `HERO_KIT` (Cassidy 225); the HUD reads 0/225 (the numeric current is the known open issue, `numeric-health.md`).
- Not done: reload animation (Soldier's st39 analog is st9 Anim var581), abilities (step 6), ult meter.
- **More kits** (`tools/lobby/kits.py`, from `tools/assets/weapon_kit.py`): Roadhog, Torbjörn, Sombra, Mei, Bastion, Lúcio, Reaper, Ana, Zenyatta play with their weapon, HUD and fire (Reaper shows both shotguns from one var49 binding).
  - Payloads: Stacks take `bits(1, 2)`; StateAbility, TrackRay and `STU_DEBD057B` states are left out (Sombra crashed with them sent bare: assert `0xF0E7E025(85)`, a misaligned frame).
  - var49's state index is as wide as the graph's total state count. Lúcio (65 states, 61 owner) crashed with 6 bits; found by bisecting with the weapon states and presence emptied, then var49 alone.
  - Bastion's graph has no CosmeticEntity: no var49; the gun still shows.
  - Then ten more from the first WeaponVolley of each body's weapon graph (`weapon_kit.py` on that graph): Tracer, Pharah, Widowmaker, Junkrat, Orisa, Wrecking Ball, Ashe, Echo, Baptiste, Mercy (blaster). Stats: `target.PLAYER_KIT`. Ashe's rifle model does not show.
  - Done 2026-11-03: the last 11 (Zarya, Hanzo, D.Va as pilot, Sigma, Symmetra, Moira, Genji, Doomfist, Reinhardt, Winston, Brigitte). No-Volley kits show the weapon but no shot; no-ammo weapons get `target.INFINITE` rounds; hits stop at the weapon's reach. Open: D.Va's mech graphs (not in the catalog), Genji/Doomfist fire button, shots for the no-Volley kits.

## Risks

- Step 3 took many probes for Soldier (`research/rifle-initialization.md`, `soldier76-weapon-graph.md`). Expect the same here; it is the long pole.
- A wrong owner-state payload can crash the client (two 1 bits did, `bot-fire.md`); probe one state at a time.
- Rough size: 1-2 sessions for steps 1-4 if the owner states map over by node class; longer if not.
