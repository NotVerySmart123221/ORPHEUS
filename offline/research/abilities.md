# Soldier: 76 abilities (2026-11-02)

Build 104319. Native addresses use base `0x7FF6D1220000`; they come from the `.scratchpad/runtime-weapon` text/rdata snapshot and the IDA dump.

## Which graph is which

Soldier's body (`0x04000000000003CF`) lists body graphs 51, 75, 67, 593, 597, 599, 601, 2331, 2230 and weapon graph 596.

| Ability | Graph | StateAbility node (owner bit) | Logical button | Command bit | UX loadout |
|---|---|---|---|---|---|
| Heavy Pulse Rifle | 596 | - | 20 (fire) | 0x01 | `0x0B9000000000008D` |
| Helix Rockets | 596 | node 65 (bit 31), cooldown var 61 | 29 | 0x02 | `0x0B9000000000008E` |
| Sprint | 597 | node 4 (bit 1) | 35 | 0x08 (Shift) | `0x0B9000000000008F` |
| Biotic Field | 599 | node 2 (bit 0), cooldown var 61 | 36 | 0x10 (E) | `0x0B90000000000090` |
| Tactical Visor | 601 | node 2 (bit 0) | 37 | 0x20 (Q) | `0x0B90000000000091` |

- Command bits were read from the lobby input log while pressing each key. Shift's first frame also carries 0x2000.
- Biotic Field (599): a WeaponVolley throws entity `0x072D` (speed 50, gravity 9.8, lifetime 10), which places `0x03DC`. A TrackTargets state of radius 10 follows. Var472's cooldown struct holds 15.
- Visor (601): uses ultimate vars 342/338. Graph 75 supplies var338 = 1650, the ult cost.

## Owner-frame counts

Owner states are the graph's states that are neither client-only nor server-only. Presence bits are the non-null instance-scope sync vars. This rule reproduces every count already verified (596, 75, 21, 463, 51).

| Graph | Graph(...) | presence order |
|---|---|---|
| 597 | (597, 31, 9) | 7681, 2575, 472, 2333, 9570, 1002, 173, 699, 61 |
| 599 | (599, 21, 6) | 9570, 2575, 5875, 472, 699, 61 |
| 601 | (601, 44, 7) | 3072, 1984, 11271, 215, 502, 567, 699 |

## HUD

Each graph's root UXPresenter (597 bit 28, 599 bit 16, 601 bit 32) carries no payload. Activating the three of them shows the Sprint (SHIFT), Biotic Field (E) and Helix (RMB) icons, plus the ultimate meter at 0% (`abilities/ability-hud.png`).

## State payloads

How the reads were found:
1. Class ID â†’ STU metadata (class ID at `+0xB0`, type key at `+8`).
2. Type key â†’ factory registry (`soldier-runtime/state-constructors.json`).
3. Factory â†’ constructor's final vtable â†’ DSR at `+0x158` and descriptor collector at `+0x168`.
4. The Begin-bit classifier `0x7FF6D217E960` decides whether an extra Begin bit precedes the DSR.

| Class | Begin bit | Bits after the active bit |
|---|---|---|
| UXPresenter `7A7F2732`, Wait `6540C278`, speed modifier `316CFEF2`, `B5C87F3D`, `E3CEF6B9`, `BD02E168` | no | none |
| Effect `691BFA55` | yes | Begin + 2-bit counter (same as the rifle muzzle) |
| StateAbility `C38B92B4` | no | 8-bit mirror flags, linked bit, timing bit; see below |
| `B9898052` (DSR `0x7FF6D1C1D760`) | no | flag bit + 8-bit counter (`end-of-match.md`) |
| ChaseVar `6AE5D1A1` (DSR `0x7FF6D2DB8400`) | no | vector/rate/third flag bits, f32 value(s), optional rate, timing bit (`soldier.chase_payload`; Tactical Visor below) |
| TrackTargets `41E3C7A7` (`0x7FF6D1EB13A0`) | no | has-targets bit, then count and per target entity + tick offset (`kits.sombra_hack_states`) |
| GameMessageEntry `9D6CF8AC` (`0x7FF6D1C20BE0`) | no | one bit (`statescript.message_state`) |
| TrackRay `164EF13B`, `STU_DEBD057B`, `STU_9D7BF987`, `STU_54FEB21C` | no | none (`statescript-payloads.md`, 2026-11-06: tracer-verified on Sombra, Moira and Reinhardt) |

### StateAbility

1. **Owner prelude.** Before the DSR, `0x7FF6D2D91930` collects descriptors through vtable `+0x168`. For StateAbility, `0x7FF6D1B0FA90` adds the mirror at `state+0x1E8`, whose type info is `0x7FF6D4387540`. The reflected visitor `0x7FF6D2E00130` reads every field not flagged `0x08`.
   - The mirror has four fields: `+0x78` s64, `+0x80` f32, `+0x84` f32 (all flagged `0x08`), and `+0x88` u8 (flags 0).
   - So the prelude reads 8 bits, bit-exact.
   - Type hashes are CRC32s of the type names (`u8` = `0x3B9327D2`, `s64` = `0x015E6ADB`, `f32` = `0x8FA75A30`).
   - The u8 holds flags: bit 0 in use (`0x7FF6D1EB6BC0`), bit 1 ended (`0x7FF6D1EC7890`), bit 2 one-shot (cleared after use).
2. **DSR** `0x7FF6D1EA4AC0`:
   - First a linked-state reference: one bit when absent.
   - Then one bit for the timing block. When it is 0, the DSR sets `+0x80 = 0`, `+0x84 = 1.0` and `+0x78 = now`, and **reads nothing more**.
   - When it is 1, it reads an f32, then a bit with an optional f32, then a tick offset (bit 0 â†’ 8 bits; bit 1 then 1 â†’ 32 bits; bit 1 then 0 â†’ 16 bits).
3. **Minimal payload:** 10 bits (`ability_payload`).
   - Sending 12 or 20 bits misaligned the frame. The client then built garbage instance events until its record pool asserted: `0xF0E7E025(85)`, with the allocator reached from `0x7FF6D1C2BB98`.

## Sprint (verified)

The client does not start graph597 by itself. With the StateAbility active, Shift alone neither raises the speed nor lights the icon. The server therefore runs it (`soldier.Sprint`).

**Start condition:** Shift (0x08) plus forward input, without fire, secondary fire or crouch.

**On start**, the body frame activates the StateAbility (flags = 1) and the activation children:
- speed modifier `316CFEF2` (bit 0), with presence var1002 = 0.5 (`graph597` node 3)
- anim-category Stack 5
- effect 9
- Stacks 13, 19, 27 and 29
- the second modifier 20 (`A9561CA0 = 21`)

The server mover runs at 1.5x.

**Session 20261102T042050:**
- The server logged sprint start, then 25 m in 3.0 s (8.25 m/s), ending at (54.57, 0.21, 17.26).
- The client lit the Sprint icon and covered the same route down to the lower Caution ramp (`abilities/sprint-run.png`, `abilities/sprint-hud.png`).
- A walk without the server states reached only the stairs.

**Limits:**
- shortcut: graph597's toggle mode (button 31), its 0.3 s re-sprint delay (var636) and its cancel buttons 12/14 are not modelled.
- The sprint start animation (Anim 15) is not sent.

## Helix Rockets (verified)

Helix is part of the weapon graph 596, not an ability graph. The server runs it (`soldier.Helix`), the same way as the rifle volley.

**Button:** logical 29, command bit 0x02 (right mouse button).

**States in the weapon frame:**
- StateAbility node 65 = owner bit 31 (`ability_payload`, idle)
- LogicalButton 14 (Begin bit + 4-bit counter)
- BooleanSwitch 35
- On fire: switch 15, Stack 30 = 1, and Volley 38 = bit 16 (`volley_payload` with the next serial)

**Authored data** (Volley node 38):

| Item | Value |
|---|---|
| Speed | 50 m/s |
| Lifetime | 5 s (range 250 m) |
| Impact damage | 40 |
| Explosion damage | 80; rings â‰¤1 m Ã—1.0, 3 m Ã—0.5 |
| Cooldown | 6 s (var472) |

**Server side** (`Match.fire_helix`, `_land_rockets`):
- A ray from the eye along the aim decides the impact: the map mesh or the first bot the client can see.
- The rocket lands after distance / 50 s.
- Each hostile bot takes the explosion damage at its distance from the impact (`helix_splash`), plus 40 if it was the direct hit.
- Friendly bots (the player's team) take no damage from rockets or the rifle.

**Sessions:**
- 20261102T042637: the launch effect is visible.
- 20261102T044552: the second rocket killed bot `B0000001` (120 damage). The client body vanished at 00:47:34.3 and came back at the respawn at 00:47:37.4 (`abilities/helix-kill.png`).
- The friendly-fire filter came after that session; `B0000001` was a team-0 bot. It is covered by `test_target.py`, but not re-run in game.

**Limits:**
- shortcut: linear falloff between the authored rings; upgrade if the curve shows up in the explosion STU.
- shortcut: friendly bots still stop rockets.
- No self-damage, and no cooldown sweep on the HUD (var61 not sent).

## Biotic Field (partly verified)

Run by the server (`soldier.Volley` with button 0x10 = E, logical 36). Cooldown 15 s (graph599 var472).

**Graph599 route:** StateAbility 2 â†’ Wait 6 â†’ Wait 8 â†’ SetVar 33 â†’ Volley 4 (throws `0x072D`, speed 50, gravity 9.8). On finishing, Volley 4 starts the cooldown (SetVar 3: var61 = var472) and the TrackTargets wait.
- The body frame activates the ability (bit 0), Waits 6/8 (bits 3/4) and Volley 4 (bit 1, `volley_payload`) for one pulse (`field_instance`).

**The field:**
- Volley 4's impact entity `0x03DC` has no model. Its components are TargetTag, Pet, an empty Model, and Statescript graph 600 (`0x0580000000000258`).
- Graph 600 spawns the canister `0x068A` (CosmeticEntity; model with look `0x206E`, the same look as the thrown `0x072D`) and plays two effects (ActionEffect `691BFA55`).
- Authored values in graph 600: Ints 5, 5 and 20, a 0.5 s Wait (and 0.25 s). Read as radius 5 m, 5 s, 20 health every 0.5 s (40/s).

**Server side** (`Match.throw_field`, `_tick_fields`):
- Creates `0x068A` at the player's feet for every client, with the team filter bit.
- Every 0.2 s for 5 s it heals teammates within 4.5 m by 7 (35/s, the wiki's 2022-09-30 values; up to 200), then destroys the canister. Graph 600 has no heal action, so its Ints are not used.

**Session 20261102T050120:** the canister stands on the floor where E was pressed and vanishes 5 s later (`abilities/field-canister.png`, `abilities/field-sheet.png`). The game kept running.
- Creating `0x03DC` instead (session 20261102T045521) showed nothing: it has no model.
- Healing is covered by `test_target.py` only. In game the player took no damage, and the health HUD still shows 0 (numeric health is parked).

**Limits:**
- shortcut: no ring or heal effects; they need graph 600 running on the client (a remote statescript for the field entity).
- shortcut: the canister's throw is not flown; it appears at the feet at once.
- shortcut: bots are not healed. Which of graph 600's two Int 5 values is the radius and which the duration is not traced.

## Ultimate charge (verified)

Entity Int var342 is the charge, and var338 (1650, copied by graph75) is the cost. Graph601's HUD presenter and its ready switch (node 79) compare the two, and graph601 node 78 sets var342 back to 0.

**Server side** (`Match._charge_ult`):
- Damage dealt (rifle, Helix) and healing done (Biotic Field) add 1:1 as points, capped at 2310 (wiki). var342 carries `points Ã— 1650 // 2310`, so the HUD meter fills exactly at the cost.
- Each change puts var342 in the body frame's variables and resends the frame.
- The charge survives deaths and is lost on a hero switch.

**Session 20261102T050558:** ten rifle hits killed `B0000501` (200 damage). The meter went 0% â†’ 10% â†’ 12% (`abilities/ult-charge.png`).

**Limits:**
- shortcut: no passive charge over time.

## Tactical Visor (verified)

**Payloads decoded for it** (no Begin bit for either; neither has a mirror prelude):
- **ChaseVar** `6AE5D1A1` (DSR `0x7FF6D2DB8400`): bits vector, rate and third flag (mirror `+0x9C/+0x9D/+0x9E`); f32 value (three when vector: `+0x80..+0x88`); when rate is set, a width bit and a 32- or 16-bit value (`+0x98`); then a timing bit, followed by a tick offset when set (`+0x90`). Minimum: 36 bits.
  - Its mirror (state `+0x1C0`, field table `0x7FF6D43D0FF0`) has six fields, all flagged `0x08` (`.scratchpad/mirror_fields.py`).
- **TrackTargets** `41E3C7A7` (DSR `0x7FF6D1EB13A0`): a has-targets bit. When set: a compact count, then per target a 32-bit entity and a tick offset. Its collector `0x7FF6D2F05640` is empty.

**Graph601 route:**
- Ability 2 (Q = command bit 0x20, logical 37) is allowed while var342 â‰¥ var338.
- Its children: ChaseVar 8 (the timer: var215 chases to 0 at rate var567), Stacks 14/80, Wait 91 (1.2 s), switch 79 (var699 and var11271), and SetVar 78 (var342 = 0).
- `visor_instance` activates bits 0, 6 (`chase_payload(6.0)`), 2, 9, 34, 14, 33 (true), 3 (effect 5) and 42 (BD02E168 92, parent of the client-only presenter 75).

**Server side** (`Match.start_visor`, `_visor_target`):
- Q with a full meter starts 6 s of visor, empties var342, and stops ult charge while it runs.
- Rifle shots hit the living, clear-line enemy closest to the crosshair within a 45Â° cone, wherever the aim is.
- It refills the rifle and ends any reload (`Rifle.refill`). Authored: graph601 node 15 sends game message `0x7D` to Soldier, and graph596 entries 30/34 set var53 (ammo) = var198 (capacity).
- Session `20261102T061254` (meter started full for the test only): 20/30 ammo before Q, and 30/30 with the visor tint and the meter at 0% after it (`abilities/visor-refill.png`).
- Not done: the faster Visor reload (wiki 0.65 s). Graphs 596/601 only ever set the reload length var581 to 1.5, so the speed-up comes from somewhere not yet found.

**Session 20261102T051242** (meter started full for the test only):
- Q showed the yellow visor tint, the lock circle and an orange lock marker on the bot. The meter dropped to 0%, and the overlay ended about 6 s later (`abilities/visor.png`).
- With the aim straight ahead, the rifle killed `B0000501` (14Â° off) and moved on to `B0000201`.

**Limits:**
- shortcut: 6 s is OW1's published duration; graph601's start value for var215 is not traced.
- shortcut: the 45Â° cone is an estimate.
- TrackTargets, DataFlowMappings, the plain State 83 and the anims are not sent. The visor's 1.2 s Wait 91 is sent but not modelled.

## Helix self-damage and field healing (verified)

Session `20261102T062738`: a rocket at Soldier's own feet logged `player damage amount=38 health=162` (half the splash), and the health bar dropped. Biotic Field then logged heals of 7 every ~0.2 s, 162 â†’ 200, and the bar refilled (`abilities/self-damage-heal.png`). Numeric health still reads 0/200.

## Cooldown sweeps (verified)

How the HUD gets the seconds left (statically mapped in `statescript-value-tags.md`, then checked in game):
- var61 (`out_CooldownVar`) is **bound to the StateAbility's output 0**: a `StateBound` value with state index width `m_statesBitCount`. Graph 599 uses presence bit 5 and state 0 (5 bits); graph 596 uses presence bit 10 and state 31 (7 bits). var472 (presence bit 3 / 14) carries the duration.
- Output 0 (`0x7FF6D1EA4D00`) is `max(0, duration âˆ’ (now âˆ’ start) Ã— 0.001 Ã— rate)`. It comes from the StateAbility DSR's timing block: an f32 duration, a rate bit (absent means 1.0), then a `w_var` offset in ms back from now.
- The server builds the payload at send time (`Volley.ability`; `owner_full_frame` accepts a callable payload), so every resend carries the elapsed time instead of restarting the cooldown.
- The HUD reads var61 only when a body frame arrives. The server resends one every 0.5 s while a cooldown runs, plus one after it ends. shortcut: what refreshes it natively is not traced.

Session `20261102T062039`: E counted 15 â†’ 13 â†’ 11 â†’ 8 â†’ 5 â†’ 2 â†’ ready, and RMB 6 â†’ 4 â†’ ready with the native ready flash (`abilities/cooldown-sweeps.png`).

## Other heroes' ability HUD (2026-11-03)

`tools/assets/ability_kit.py` derives, per hero body graph (not the weapon graphs, not the shared graph 67), the
first StateAbility waiting on Shift (logical 35), E (36) or right mouse (29), the UX presenters not under it (the
icon), and the cooldown: the Stack writing var472 holds `STU_7197C080` with the seconds in `m_01AFADB6`; var472 and
var61 are instance sync vars like in graph 599. For Soldier it gives graph 599 exactly as hand-mapped.

`kits.ABILITIES` holds the result for 26 of the 30 kit heroes (`ability_instance`, a `soldier.Volley` per ability as
the cooldown clock). A press runs only the cooldown sweep. The StateAbility payload must be the callable
`Volley.ability`, not its value at press time: sent once, every 0.5 s resend restarted the cooldown and the icon
stayed at duration + 1 (Cassidy: "7" for 6 s). Checked in game: Cassidy's Combat Roll counts 6 -> 1
(`main-menu/ability-sweep-cassidy.png`).

Checked in game, all 26 (`main-menu/ability-hud-shift.png`, `ability-hud-e.png`: the HUD 1 s after Shift, then E):
no crash, every icon shows, and the press is logged. The countdown shows for Roadhog, Ana, Widowmaker, Echo,
Baptiste, Sigma, Moira, Genji and Reinhardt (both), Mei, Orisa, Doomfist, Brigitte and Zarya (Shift), Sombra, Lucio,
Tracer, Pharah, Junkrat, Wrecking Ball, Ashe, Symmetra and Winston (E). No countdown: Reaper (both), Sombra and
Pharah (Shift), Mei, Brigitte and Zarya (E); charge abilities (Tracer's Blink, Junkrat's mine, Symmetra's turrets)
show a red crossed icon. Unclear: Ashe's Shift (icon off the crop) and Mercy's (1.5 s). Zarya's E stays blank for 6 s too: its var61 setter runs only after its TrackTargets state
finds a target (graph 604: Ability -> TrackTargets -> Wait -> ChaseVar -> Pet -> SetVar 61), so the client never
reaches it; the other misses have no shared shape (scratch trace: var61 setter ancestors per graph). Not pressed:
right-mouse abilities (Bastion, Orisa, Wrecking Ball, Doomfist).

Fixed the same day (`main-menu/ability-hud-fixed.png`, `ability-hud-charges.png`):
- var61's `StateBound` takes the StateAbility's **full** state index, not its owner index. They differ when
  client-only states come first (Zarya's E: owner 6, state 9). All 8 misses with no countdown were such abilities,
  and every working one had equal indexes. `Ability.bound` holds the full index; `ability_kit.py` prints it last.
  Now counting: Reaper (both), Pharah, Mercy (Shift), Mei, Zarya (E).
- Charge abilities read var11 (charges left) and var181 (most charges), instance vars set from an Entry the client
  does not run: Tracer's Blink (3, Float), Junkrat's mine (2), Symmetra's turrets (3), Brigitte's Repair Pack (3,
  Int). Sent full in `Ability.extra`, the icons draw normally with their charge counter. shortcut: they never
  count down.
- Sombra's Stealth: var2575 (usable, instance bit 1) stays off on the client; sent `True` it counts down, but the
  icon keeps a red overlay (not traced).
- 2026-11-07, traced on D.Va's mech (research/dva-mech.md): the red overlay is UI property `0x0D80000000000786`,
  an expression per presenter: `v2575 || v2575(entity)` (Boosters, Stealth graph 3227) or with `|| !v4993` (Micro
  Missiles, Defense Matrix). var2575 is *unavailable*, var4993 *available*: Stealth's var2575 = True is what draws
  its overlay. Stealth now sends nothing extra: live, its icon is clean and counts 6, 5, 4 after Shift
  (`research/main-menu/sombra-stealth-hud.png`).

Left out (shortcut): the abilities' effects (movement, projectiles, status), ultimates (logical 37), and abilities
with no var472 cooldown (Torbjorn, Zenyatta, Hanzo, D.Va, Bastion's configuration).

## Ability casts (2026-11-03)

Projectile abilities, run by the server like Biotic Field's throw:
- **Cast pulse:** `ability_kit.cast` gives the owner states from the StateAbility down to its first WeaponVolley,
  when all are payload-free (Wait, plain State): 14 abilities (`kits.Ability.cast`). A press sends them with the
  Volley (`volley_payload`, start then stop); the frame after the stop drops them again. Accepted, no crash, but
  nothing shows in first person: the cast animations are StateAnim states, payload not decoded.
- **Hit:** `target.ABILITY_HITS` (wiki values) flies a Helix-style rocket (`Match.fire_ability`): it lands at the
  first wall or bot, else at its range, and hurts enemy bots (direct + splash). Damage only (shortcuts listed
  there). Authored Volley data (`m_6396149F` direct, `m_D2D11CE9` explosion rings and amount, speed, lifetime)
  is often an expression, so the table uses the wiki.
- **Visual:** the Volley's `m_projectileEntity` does not render (like bot shots' `0x1476`); its child (field
  `436BEFE6`, `tools/assets/stu_refs.py`) does. Flown with the bot-shot method (re-created each frame).
  Seen: Venom Mine (red glow flying off), Symmetra's turret orb, Ana's grenade (faint). Steel Trap has no child
  and stays invisible; Flashbang is too fast for the screenshots (`main-menu/ability-cast-sheet.png`, `-sheet2.png`).
- Symmetra: with full charges the client enters its own turret placement (DEPLOY / CANCEL) on Shift and stays
  there; not served.
- 2026-11-06: the DEPLOY / CANCEL prompt was up from spawn, Shift or not. It is the Teleporter's UX presenter 51
  (UI `0x0D800000000060F7`), a targeting prompt the server sent always-on; dropped from `kits.ABILITIES`, the prompt
  is gone. Doomfist (Rocket Punch, CANCEL), Mei (Ice Wall, BUILD / CANCEL) and Reaper (Shadow Step, TELEPORT /
  CANCEL) had the same stuck prompt from spawn; their `60F7` presenters are dropped too and the prompts are gone
  (`main-menu/targeting-prompt-before-after.png`).

## Next

- Biotic Field's ring effect (graph 600 on the client). Graph 600 (`0x0580000000000258`) layout, read from the raw asset (arrays are (u64 count, u64 offset) from the dynamic-data base, header word 7):
  - 18 nodes; 11 owner states, in order: TrackTargets, Wait, effect `691BFA55` (inst 24), CosmeticEntity (canister), Wait, DataFlowMapping, Wait, `431F3FCC`, `38EE1100` (sends a game message), effect `691BFA55` (inst 92, gated by sync var 2712), BooleanSwitch.
  - Remote states (5): owner states 2, 3, 5, 8, 9.
  - The open questions are its sync-var count (`m_syncVars` reads as count 2, bit count 1) and which frame type a non-owned entity takes. `remote_full_frame` (H0) has never run in game.
  - Tried in session `20261102T062427`: entity `0x03DC` created at the field, plus a remote (H0) frame for instance 1 of `Graph(600, 11, 1, 5)`. Remote 0 and 4 (effects) carried desired-active + a two-bit counter; remote 1 (canister) carried desired-active. The client accepted it (no assert), but neither the canister nor any ring appeared, so it was reverted to creating the canister `0x068A` directly. Next step: read the field entity's graph-600 state flags from client memory to see which part was rejected.
  - Memory read, session of 2026-11-02 12:43 (same frame, field lengthened to 60 s for the probe): the client's graph-600 instance (found by scanning for its graph GUID at instance `+0x1E8`; same instance vtable as graph 596) has 11 states. Effects st2 and st9 and the canister st3 are **active** (flags `0x4061`) with a correct begin time (219723 ms = throw frame 31389 × 7 ms). DataFlowMapping st5 and `38EE1100` st8 are not instantiated (client-only/server-only). So the frame is accepted and applied. Still nothing renders: no ring and no canister, even looking straight at the spot.
  - Lead: the CosmeticEntity node's field `m_1DEF59FA` is dynamic var 2712 (`0x0A98`), the same sync var that gates effect 92. The server never sends it. Its type is not decoded; it may be the entity the canister and ring attach to.
