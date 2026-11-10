# D.Va

Hero `0x02E000000000007A`. Spawns in her mech (entity `0x0407`, 300 health + 300 armor, Fusion Cannons graph
`0x050A`); the pilot is entity `0x040A` (150 health, Light Gun graph `0x02B8`). Voice set `0x07A0000000000057`
(shared by mech and pilot). Background research: [research/dva-mech.md](../research/dva-mech.md).

## Issues (reported 2026-11-08)

`[x]` fixed and seen live, `[~]` fixed in code and tests, not yet seen live, `[ ]` open.

- [~] 1. No voice line when spawning
- [x] 2. Mech cannons have no shooting animations
- [x] 3. Mech shots originate from the centre of the mech instead of the cannons
- [x] 4. Micro Missiles are not green when shot
- [~] 5. Eject! (passive): the mech's destruction kills the hero instead of ejecting the pilot (the pilot swap is
  seen live through Self-Destruct; ejecting at 1 health is unit-tested only)
- [x] 6. Self-Destruct (ultimate): eject, the left mech glows, beeps down and explodes 3 s later, Call Mech fills
- [x] 7. Call Mech (second ultimate, as pilot)
- [x] 8. Defense Matrix: no bar showing the remaining charge
- [x] 9. Boosters: no flight-time remaining display and no radial countdown in the centre
- [x] 10. Boosters: no knockback on opponents
- [x] 11. Pilot (out of the mech) gameplay: Light Gun fires, her own ult meter fills

Reported 2026-11-08, second round:

- [~] 12. Micro Missiles do not despawn: they stay after hitting their target (root cause fixed and unit-tested;
  the long-hall repro was not hit again live)
- [~] 13. Micro Missiles: no firing sound (launch effects now sent; sound needs a listen)
- [x] 14. Boosters' ring and timer show only on some flights
- [x] 15. Mech cannons: the barrels do not spin when shooting (fixed in round 3, see 15, 17 below)
- [x] 16. No bail-out (eject) or suit-up (Call Mech) animation (round 3 adds the launch arc and the mech drop)
- [x] 17. Pilot: the Light Gun keeps playing its firing animation when not firing (fixed in round 3)
- [x] 18. Pilot: no reload animation

Reported 2026-11-08, third round:

- [x] 19. Boosters' ring timer counts up instead of down
- [x] 20. Eject: no launch arc (the pilot only appeared behind the mech)
- [x] 21. Call Mech: no mech dropping from the sky
- (15 and 17 reported again: the barrels still did not turn and the pilot's gun still moved when idle.)

Found 2026-11-09, checking the mover port:

- [x] 22. Boosters: the server's flight is up to 24 m from the client's without `OW_CLIENT_SYNC`

## Fixes

### 1. Spawn voice line

- Cause: the lobby sent ClientInVoice 27500 only for Soldier (`match.py` `_spawn_steps`).
- Change: `soldier.SPAWN_VOICES` maps hero -> (entry line, respawn line); `spawn_voice(hero, body, respawn)`
  returns None for heroes without one. D.Va: entry `0x076000000000776D` (stimulus `0x2B`, `HrSlct-EntrMtch`),
  respawn `0x0760000000007765` (stimulus `0x2C`, `Rspwn`). Found with the new `tools/assets/voice_lines.py`
  (reads a voice set from the speech manifest; reproduces Soldier's known keys `0x6F9C` / `0x7036`).
- Proof: `test_soldier.py::test_spawn_voice_waits_for_body_ack_and_plays_once_per_spawn` covers both heroes.
- Limit: the 27500 goes out live; whether it is audible is not confirmed.

### 2. Cannon fire animation

- Cause: only the fire Stack and Volley were sent. The fire button's `m_80AF45FB` plug (held fire) runs switch 16
  (var33 priority) true: Stack 13 (var1273, the firing anim category), Anims 17/18, ChaseVars 19/20 (recoil),
  Stacks 27/29 and `BD02E168` 31 (its client Waits flash the muzzle effects).
- Change: `kits.DVA_HELD`, sent while fire is held; ChaseVar 11/12 kick alternates per shot.
- Proof: `test_kits.py::test_dva_cannons_alternate_muzzle_and_kick_while_held`; live recording showed the arms
  recoiling and both muzzles flashing.

### 3. Shot origin

- Cause: Volley 3's muzzle is instance var58, which the graph's server-only SetVars flip between the two cannon
  hardpoints; unsent, shots left the mech's origin.
- Change: `kits.DVA_MUZZLES` (`0x0DC...07`, `0x0DC...05`); shot n uses `DVA_MUZZLES[n % 2]`.
- Proof: same test; live tracers leave each cannon in turn.

### 4. Micro Missiles colour

- Cause: the flown projectile entity had no team filter (component 26), so the client drew the enemy colour.
- Change: `Match._fly` and `launch_orb` give shots `components = {26: [player.team_bit]}`.
- Proof: live, the missiles are green.

### 5, 11. Eject! and the pilot

- Change: the mech stops at 1 health (`Match._damage_player`) and sets `player.form_change = 'eject'`;
  `Match.change_form` switches to `content.pilot(hero)` (same hero GUID on the client, body `0x040A`, name
  `D.Va pilot`) 2 m behind the mech (`target.EJECT_BACK`, stopped short of walls; `switch_hero(at=...)`; round 3:
  1 m behind and thrown back, section 20). The
  pilot is invulnerable for the 1.5 s cast (`player.invulnerable_until`), plays `Ejct-Actvt` (`0x497` / `0x78E6`)
  and uses `kits.DVA_PILOT` (the Light Gun kit from git `0887900`) with `target.PLAYER_KIT['D.Va pilot']` (150
  health). A dead pilot respawns in a mech (`heroes()[guid]`). Ult charge resets on eject (OW1: demeched D.Va
  loses it).
- Proof: `test_roster.py::test_dva_ejects_at_one_health_and_her_pilot_respawns_in_a_mech`. Live (through
  Self-Destruct): the client takes the new body away from the spawn point; pilot view, Light Gun fires, her ult
  meter shows. The first live run put the pilot exactly where the mech stood, so her camera sat inside the kept
  mech: hence `EJECT_BACK`, seen live (the pilot stands behind the mech).
- Limits: on Eject! the mech vanishes at once (no wreck). Ejecting at 1 health is not seen
  live (the Range does little damage to the player).

### 6, 7. Self-Destruct and Call Mech

- Ult meter: `kits.DVA_ULTS` by body. Mech: Self-Destruct graph `0x0B2F` presenter 26 (owner 25). Pilot: graph
  `0x026A` switch 85 (owner 82) false -> presenter 78 (owner 75). Both parent to var10964 and bind var342 against
  var338, sent as Soldier's (`soldier.ult_meter(points, most)`). Points: 1540 (mech), 319.2 (pilot), from damage.
- Q (`0x20`) with a full meter: mech -> `'self_destruct'`: eject (voice `GmOvr-Actvt-Frndly` `0x4D1` / `0x8015`),
  the old mech body is kept (`switch_hero(keep_old=True)`) and `Match._tick_mech_bombs` destroys it 3 s later,
  dealing 1000 -> 100 damage over 20 m to enemy bots not behind cover (mesh ray), then fills Call Mech. Pilot ->
  `'call_mech'`: a new mech where she stands, 250 damage within 2.5 m, voice `CllMch-Actvt` (`0x479` / `0x77C5`).
- Explosion: before leaving the mech, `change_form` sends the mech's last owner frame with
  `kits.dva_self_destruct(fuse)` in place of its `0x0B2F` instance: Ability 0, base state 9 with Stack 1, effects 5
  (`0x0300000000001702`, the glow) and 10, Anim 13, and the fuse (Wait 6 times out on var215, instance bit 2). The
  client keeps that graph state when no more frames come, so it runs the countdown and Wait 6's onFinished
  explosion action (`0x0300000000001703`) itself. The body is destroyed 1.5 s after the blast so the effect plays
  out (`mech_bombs` entry `[player, mech, feet, blow_at, gone_at]`).
- Proof: `test_roster.py::test_self_destruct_blows_the_mech_up_after_its_fuse_and_fills_call_mech`. Live
  (Roadhog dummy): the mech meter filled from cannon damage (96% after 16 s), Q ejected the pilot behind the mech,
  the mech glowed and exploded 3 s later (white blast) killing the dummy, the Call Mech icon lit, Q put her in a
  new mech with the meter at 0%.
- Limits: the called mech appears at once (no drop); no passive ult charge; `dva_self_destruct` leaves out
  `STU_D07EBF01` 2, `STU_F08E1453` 21, `STU_B9898052` 36 (payloads not decoded).

### 8. Defense Matrix meter

- Chain: Matrix graph `0x02CC` Entry -> SubScript 46 (owner 33) starts graph `0x214E` (`Graph(0x214E, 1, 0, 1)`),
  then message state 47 (owner 34) posts message `0x104E` into it; `0x214E`'s client-only GameMessageEntry 1
  starts presenter 0, which parents to var11821 = the mech HUD graph `0x0276`'s client-only presenter 55. Every
  hero resource meter (graphs `0x214D`-`0x2155`) opens on the same message.
- Change: `Ability.always` ((owner state, how) always sent): Matrix sends `(33, subscript(0x214E))` and
  `(34, 'message')`; the body adds `Instance(0x0276, DVA_HUD)` and the child
  `Instance(0x214E, ..., parent=(0x02CC, 46))` after the abilities; var11821 = `ScriptRef(0x0276, 55)` (as
  Widowmaker's `ScriptRef(118, 6)`).
- Crash found on the way: owner indices were first taken as 29/30. Owner 30 is ChaseVar 43, so the 1-bit message
  payload misaligned the body frame and the client asserted `0xF0E7E025(85)` on spawn. Count owner indices from
  `graph_layout.py --states` (skip C and S rows); the tracer's `node=` is the state index.
- Proof: `test_practice.py::test_dva_matrix_meter_host_and_child_follow_the_abilities`; live: the arc right of the
  crosshair appears after a Matrix and refills.

### 9. Boosters ring and seconds

- Widget: presenter 32 (UX, template `0x0A88000000000237` -> base `0x0A880000000000B1`) binds `0x9A` = var215 /
  var567 (ring) and `0x66B1` from var215 (seconds). It is real in 1.74: the Hero Info clip shows the ring and `1s`.
- Cause 1, parent: presenter 32 and the CANCEL prompt (41) parent to var11815. Boosters' graph lists var11815 as an
  entity sync var, so the server owns it; natively it is `0xA00040004` = `ScriptRef(4, 10)`: instance 4 is the
  player's HUD `0x20E4` (`heroselect.controller_frame`), whose presenter 10 publishes var11815 (state 9 publishes
  var10964, the ult meters' parent). Unsent, the presenter's parent slot (state item `+0x218`) stayed null.
  (Graph 463's presenters 35/40 publish the same vars but never run on the client.)
- Cause 2, value: `Ability.clock` built var215 once at the press (0.0) and every resend carried that 0, so the
  ring stayed empty and hidden. Shown on from spawn with var215 = 1.0 it drew (ring + `1s`).
- Change: body var11815 = `ScriptRef(4, 10)` next to 11821; the clock's presence value is a callable read at
  send time (`owner_full_frame` and `Stream.reload_frame` now resolve callable presence values, as they already
  did active payloads). Presenter 32 stays in `running` (begins and ends with the flight).
- Found with: `scan_var`-style search of all graphs for presenters publishing var11815 (`m_45017CEA`), and polling
  state 32's runtime item during a flight (`+0x218` parent, `+0x220` UI id, `+0x238` flags; compare with the
  working ult presenter `0x0B2F` state 26).
- Proof: `test_kits.py::test_boosters_ring_clock_is_read_when_the_frame_is_sent`,
  `test_practice.py::test_dva_matrix_meter_host_and_child_follow_the_abilities` (var11815); live: the ring and
  `2s` draw round the crosshair during the flight and go with it.
- Limit: `ScriptRef(4, 10)` assumes the practice controller's instance numbering (other modes unchecked).

### 10. Boosters knockback

- Change: `Match.boost_collide`: an enemy bot within 1.5 m (2 m vertically) of the flying mech takes 10 damage
  once per flight and is knocked along the heading at 17 m/s for 0.25 s, stopped short of walls
  (`TrainingTarget.knocked`, `patrol`).
- Proof: `test_roster.py::test_boosters_hit_an_enemy_bot_once_and_knock_it_along_the_flight`; live: the Roadhog
  dummy was knocked 4.25 m.

### 12. Micro Missiles left in the air

- Cause: drawn projectiles are re-created a step further every frame (`Match._projectile_updates`, called from
  `remote_movements` while the frame is built), and `shot.shown` forgets the old entity at once. A volley of 18
  plus everything else overflowed the datagram; `server.Client.send_frame` halves the entity list until it fits
  and the cut part was simply dropped. A dropped destroy left that missile frozen in the air for good.
- Change: `send_frame` puts this frame's cut records that must arrive (`op` and `resend`: a step's destroy) at the
  back of the resend queue; a step's create (`resend = False`) and movement records are still dropped.
- Proof: `test_world.py::test_a_destroy_cut_from_a_full_frame_waits_for_the_next`. Live before: a volley down the
  long hall left about ten frozen streaks for 9+ s (E: scratch `miss19.mp4`). After: short and medium volleys clear
  (`miss21.mp4`); the same long-hall shot was not repeated (the spawn faced elsewhere).

### 13. Micro Missiles sound

- Graph `0x1128`: Wait 7 starts effect 8 (`0x0300000000001F99`) and Volley 5's subgraph effect 6
  (`0x030000000000028A`); neither was sent. `Ability.effects` pulses them with the cast (Begin bit, the cast's
  serial). Seen: the launchers flash. Not heard (no audio capture: the only loopback is the user's Voicemeeter bus).
- Lead if still silent: the Volley's per-shot effect list runs from a ch2 shot record keyed to (0x1128, owner 5,
  event = the cast serial & 63), as the bots' shots (research/bot-fire.md).

### 14. Boosters ring on every flight

- Cause: the client does not advance ChaseVar 4 (var215) itself. The ring showed only when some other body frame
  (a cannon shot, a cooldown refresh) happened to go out mid-flight carrying the send-time var215
  (tracer: presenter 32 began on every flight).
- Change: while a `Timed` ability runs, the body is resent every tick (`player.body_dirty`). D.Va's body now goes
  through `Stream.reload_frame` with deltas, as Soldier's (`_body_weapon_frame`), so the resends carry only var215
  and do not replay the flight's StateAnims. Her graphs got delta layouts: `tools/assets/reload_layouts.py` reads
  graphs the catalog lacks from the retail install (rows match the catalog's on 75, 463, 596).
- Proof: live, the ring and seconds show from the start of the flight to its end (`b20.mp4`).

### 15. Cannon barrels

- Graph `0x050A`: held fire's ChaseVar 22 runs var261 up to 1 at 2/s; on release ChaseVar 21 runs it to 0 at
  2/3 per second; DataFlowMapping 21 feeds it to the cannons. var261 (presence bit 0) was never sent.
- Change: `Player.barrels` / `barrel_spin(now)` follow the fire button; the body sends var261 as a weapon value and
  is resent while it ramps. (vars 350/351, ChaseVars 11/12, are each shot's kick; already sent.)
- Seen: the cannons' ring glow comes up while firing and fades after release (`spin23.mp4`). The barrel turn itself
  is not clear at 60 fps.
- Round 3, the real cause (shared with 17): see "15, 17. Time since the shot" below.

### 16. Eject and Call Mech animations

- `Player.ult_instance` holds her ultimate graph (`kits.initialize_practice_kit`); `refresh_abilities` reads it,
  so Match swaps it: `dva_self_destruct`, `dva_call_mech`, `dva_eject`.
- Call Mech: Q now starts the pilot's call (`kits.dva_call_mech`: StateAbility 5, switch 15 true, Wait 6 with Anim
  22, alias `0x0C80000000000433`; the client runs on to Anim 36) and `Match.land_mech` brings the mech
  `target.CALL_MECH_CAST` (1.5 s) later. A pilot killed while calling stays on foot. Seen: she raises her hand for
  1.5 s, then the mech (`call24.mp4`).
- Eject: the pilot body starts with `kits.dva_eject` (Wait 28 + Anim 27, alias `0x0C80000000000439`) until her
  1.5 s cast ends. Seen: after Self-Destruct the first-person view shows an arm pose without the gun for about
  1.5 s, then the gun (`eject25.mp4`); that it is the native bail-out clip is a guess from the graph.
- Limits: no mech drop or eject launch arc; the states before Wait 28 / after Anim 36 are not sent. (Round 3 adds
  both, server-side: sections 20, 21.)

### 17, 18. Pilot Light Gun

- 17, cause: a shot made the client start switch 29 (under Stack 17) and Stacks 8 / 31 (under Volley 5), which
  never ended (tracer), so the firing animation kept playing. Change: `kits.DVA_PILOT`'s fire states add
  `26: 1, 8: 'stack', 28: 'stack'` (owners), so the server turns them off. Seen: they end with each burst; firing
  still works (ammo 9 -> 2 -> 0).
- 18: `kits.DVA_PILOT` gets `reload=({4: 'stack', 1: 1, 0: 1}, 15)` (`weapon_kit.py` on graph 696). Seen: the
  reload animation plays and the magazine refills to 20.
- 17 came back in round 3: the gun still moved before any shot. The fire states were not the cause; see below.

### 15, 17. Time since the shot (round 3)

- Both guns' animations read instance sync vars through DataFlowMappings that the server never sent, so they sat at
  0 = "just fired":
  - Pilot `0x02B8`: var350 (DFM 11), the time since her last burst ended (ChaseVar 10, begun by Volley 5's end);
    var233 (DFM 14), firing.
  - Mech `0x050A`: var350 / var351 (DFMs 15 / 14), each cannon's time since its shot (ChaseVars 12 / 11, begun per
    shot); var233, firing.
- Same route as Soldier's Chase19 (research/soldier-runtime/first-person-fire-assets-2026-11-07.md): 100 means
  long ago, and the client advances a ChaseVar's output only through a per-frame `StateBound`.
- Change (`kits.weapon_instance`):
  - Mech: var350 / var351 = `StateBound(Float(100), 1, 12 / 11, 6, per_frame=True)`; var233 = firing.
  - Pilot: var350 = `Float(100)`; after a burst, ChaseVar 10 with `soldier.chase_since(ms)` (the timing payload
    Soldier used inline, now shared) and var350 bound to it. `Match._body_weapon_frame` keeps `Player.burst_end`
    (the frame that ended her last burst) and passes `fire_elapsed`. var233 = firing.
- Seen:
  - Pilot: frame-to-frame gun motion while idle fell from 10-20 (`pilot22.mp4`) to about 0.5 (`pilot26.mp4`); it
    moves during bursts and settles after. The idle pose matches Hero Info clip 5.
  - Mech: the barrel tips kick and turn with each shot (`mech26.mp4`); they stood still before (`spin23.mp4`).

### 19. Boosters ring counts down

- `Timed.meter` (Boosters' var215, the ring and its digits) now gives the seconds left, not the seconds flown.
- Seen: the ring starts full at "2s", shrinks through "1s" and empties as the flight ends (`boost32.mp4`).

### 20. Eject launch arc

- Hero Info clip 3: after a short black "eject" screen she flies up and back over about 0.5 s, looking down at the
  mech, and lands about 5 m behind it.
- The client's own body takes the velocity in its create's movement state. `Match._eject` now starts the pilot
  `EJECT_BACK` (1 m) behind and `EJECT_RISE` (1 m) above the mech's feet and throws her at `EJECT_LAUNCH` (6 m/s
  back, 5 m/s up): `switch_hero(launch=...)` sets `Mover.launch`, so the create carries it. The server's `Mover`
  keeps the throw through the air (`drift`) and drops it on landing.
- Seen: client position sampled after an eject on open floor: 5.37 m behind the mech, exactly the server's arc
  (0.73 s at 6 m/s, so the client has no air drag). The camera rises above the mech and comes down (`eject28.mp4`).
  Ejecting next to the spawn's crates, she lands on top of them (real collision).
- Not done: the pilot graph's own launch states (`5AFB02C7` 7, `72DDE005` 45) are still not sent.

### 21. Call Mech drop

- Hero Info clip 4: hand up with pink light beams, the mech lands right in front of her with its hatch open, the
  view goes into the cockpit, black, then the mech HUD boots.
- Change: Q makes a display mech body (the player's entity id `DROP_SLOT` 0xFF; `Player.new_body` now cycles
  1..254) `CALL_MECH_DROP` (20 m) above a spot `CALL_MECH_AHEAD` (1.5 m) in front of her (kept off walls). Its
  movement records drop it faster and faster over the 1.5 s cast (`Match.remote_movements`); `land_mech` destroys it
  and puts her mech on that spot. Its create has no movement block, as a bot's: with one, the client keeps no later
  record (below).
- Seen: the mech comes down from above right in front of her and she is in the cockpit a frame later
  (`call32.mp4`). One frame shows the camera inside the landing hull.
- Not done: the beams, the cockpit-entry animation and the HUD boot.

### 22. Boosters flight on the server

- Seen: with resync off, the server's mech caught on edges, once stuck in the spawn table for a whole flight, and
  flew level where the client climbed along the aim (research/lizrollin-runtime.md section 7).
- Cause: `PlayerMover._fly` was a kinematic stand-in on Soldier: 76's 0.5 m capsule, level only. The mech body has
  no entry in `game_movers_174.json`.
- Change (`tools/lobby/player_mover.py`): the mech's own mover values (Soldier: 76's with a 0.7 m capsule), and
  Boosters as the client's walking mover with graph 0x047E's MovementMod: 11.99 m/s, air acceleration 35, forward
  forced, character collision off, and the turn toward the wish in 3D along the aim. Gravity off, no landing and
  the touching acceleration are fitted.
- Proof: ten recorded flights (level, steered, W held, into a wall, aimed up and down) stay within 0.85 m of the
  client, from 21.6 m (research/lizrollin-runtime.md section 8); while a flight runs the feet match the client's.
  Tests: `test_player_mover.py::test_boosters_fly_along_the_aim`,
  `test_player_mover.py::test_recorded_boosters_flights_stay_near_the_client`.
- Not checked: strafing during a flight, other maps, a live run with the new model.

### Known limit: a left Self-Destruct mech shows at its create spot

- The own body's create stamps its movement state at frame 0xFFFFFFFE (ProCore's spawn). The client keeps no
  movement record older than that, so after Self-Destruct the left mech is drawn where it was created (the spawn
  point), not where she ejected. Seen when she walked before ejecting: no mech in front of her (`eject29.mp4`).
- Tried: stamping the create at the newest command frame. The left mech could then be moved, but the Range dummy
  took no cannon hits (two runs, 0 hits against 76-84). Reverted.

## Open

- Left Self-Destruct mech drawn at its create spot (known limit above).
- 21: the beams, cockpit entry and HUD boot of Call Mech; 20: the pilot graph's own launch states.
- 12: repeat the long-hall volley live. 13: listen for the missiles.
- 1: spawn voice audible? 5: ejecting at 1 health live.
- 22: a live Boosters run on the new model (the recordings were replayed offline).
- Owner corrections (2026-11-09, research/lizrollin-runtime.md section 9) are not on for D.Va: Boosters are fitted (up
  to 0.85 m off at a flight's start or end) and her cannons' air deceleration while firing is not modelled. A live
  walk, strafe and jump while firing matched the server within 0.040 m (no speed penalty).
- Other heroes: about 40 graphs' presenters parent to var11815 (and var10964); the server sends neither for them.
  Sending `11815: ScriptRef(4, 10)` for every hero may restore their centre timers too (unchecked).
- Live checking tips: `OW_VIEWER_FILE` (a file holding `Roadhog`) puts a dummy 5 m in front of the spawn to charge
  ultimates without aiming; `OW_CLIENT_SYNC=47124` keeps the server's position on the client's. Picking a hero
  that crashes ends the game by itself; a healthy game needs the user to close it (the Esc menu draws no buttons).
