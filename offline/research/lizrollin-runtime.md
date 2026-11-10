# LizRollin's 1.74 runtime: what it has that we lack

Evaluated 2026-11-09. This is the game runtime that ORPHEUS integrated and is now porting to Rust (see the ORPHEUS comparison in this session's notes: same upstream as ours, Boi-027's lobby research).

## Source

- `LizRollin/overwatch-1.74-lobby-server`, pinned commit `9f8f3bcb5eccab6fb7738bee3c2371fcf0623f29` (2026-10-03), MIT.
- Local read-only copy: `lizrollin-lobby-server/` (git-ignored; fetched at that one commit, depth 1).
- Lineage: it shares its base with our `tools/lobby/vendor` (Boi-027's `overwatch-1.74-lobby-server`). `bits.py` is identical. `statescript.py`, `world.py` and `server.py` have diverged almost completely.
- Not run: none of its code or tests were executed. Everything below comes from reading the code. The only exception is its JSON data, read with our own scripts.

## Safety scan

- No agent-instruction files (CLAUDE.md, AGENTS.md, .cursorrules and similar).
- No hidden or bidi Unicode in any text file.
- No injection-style text found in code, docs or data.
- Side effects to know before running anything:
  - `ow174/launcher/relay.py` downloads a relay DLL from a GitHub release (squeeeezy).
  - `launcher/inject.py` injects it into the game.
  - `START.bat` opens python.org when Python is missing.
- None of these run unless the launcher is started. We don't need the launcher.

## 1. Character mover: a port of the client's own (`ow174/game/mover.py`, 1042 lines)

- A float32 port of the PC client's mover tick, done from the disassembly. The docstring lists every step's address, and the image base 0x7FF788F10000 matches our build. The tick covers:
  - ground probe and disc sweep
  - pogo spring
  - friction, acceleration and speed caps
  - gravity and terminal velocity
  - jump and crouch with the 0.15 s latch
  - capsule sweep with Gauss-Seidel contacts (`contacts.py`, 2584 lines)
  - the 1/1024 m position grid
- Per-hero values come from the hero's `STUCharacterMoverComponent` (`data/game_movers_174.json`, 38 entries).
  - Soldier: 76 runs 5.5 forward, 4.95 backward and 5.5 strafe. Crouched, 3.0 every way.
  - Accel 35 on the ground and 3.5 in the air. Jump 6.0, gravity 1.75 × 10. Capsule 2.0 m × 0.5 m, crouched 1.25 m.
- It confirms our measurements: backpedal 4.95 matches our `BACKPEDAL = 0.9`, and crouched backward is the full 3.0. Our guessed gravity (17.5) and jump (6.0) are right.
- It shows where our mover's guesses are wrong:
  - radius 0.4 (real 0.5)
  - height 1.8 (real 2.0)
  - instant speed (real: acceleration and deceleration ramps, our open "speed ramps" item)
- Sprint is modelled: the body script's movement mod adds 0.5 to the speed scalar.
- Collision (`collision.py`, `casc.py`, `props.py`, stdlib only):
  - It reads the map's 0x11 physics chunk straight from the install and keeps only the team-proxy-masked shapes.
  - It adds the entity-placeable props the chunk lacks (fences, desks, chairs, clip boxes). Data: `data/collision_props_174.json`.
  - That is our open "props missing from the collision mesh" item (the table by the Range spawn).
- Tests replay a 938-tick live walk (`tests/test_game_mover.py`, 45 tests).

## 2. Owner corrections (`ow174/game/correction.py`)

This explains our dead end in [hitbox-drift.md](hitbox-drift.md), where per-frame own-body ch3 records put the camera under the floor:

- The client compares a ch3 record on its own body field by field (flags, frames, throttles, gravity, spring, angles, position, velocity) with the state it predicted for that command frame (`0x7FF789B73E00`, mode 2).
- Any difference snaps the body to the server's state and replays the later commands.
- So the record must carry the full exact mover state after command frame F, stamped F. Our approximate mover's records were "different" every frame, so the client took our wrong height every frame.
- Retail sent one per frame (OW2 traffic). The +12 discontinuity counter is 0. The spring offsets are not sent.
- Only usable with an exact mover. With our `OW_CLIENT_SYNC` feed (the server follows the client) it isn't needed for single player.

## 3. Statescript VM (`ow174/game/script/`, about 4.8k lines)

- `runtime.py` runs graph instances the way the client does:
  - int64 ms time
  - command frame K is the window [16K, 16K+15]
  - FIFO events
  - plug walks
  - begin, abort and finish order
  - variables resolved as base value plus the last active override
- `nodes.py` implements 45 state, action and condition classes. Among them: BooleanSwitch, Stack, LogicalButton, Wait, ChaseVar, WeaponVolley, Anim, Ability, SubScript (37D754C8), GameMessageEntry, CosmeticEntity and TrackTargets. Any class not listed runs as a plain State.
- `expr.py` evaluates config vars and values like the client's interpreter (`0x7FF78AA91210`).
- `driver.py` sends owner frames, which keeps the client's prediction in step:
  1. full frame (H=0)
  2. C=0 owner delta
  3. C=1 per command frame
- `match.BODY_SCRIPT_HEROES = None` runs every hero's body script.
  - Their WIP verifies only Soldier: 76.
  - Known live defects: fire stuck after kills, Sprint stops, a crash after kills, the Field missing with some skins.
- `decode.py` plus `data/statescript_codecs_174.json` (449 KB, the client's own codec table) read a chunk back the way the client would. They raise `ClientCrash` where the client would crash. That works as an offline test oracle for any frame we build.
- `data/game_graphs_174.json.gz` (1.8 MB, 373 graphs: hero bodies, weapon managers, weapons and their children) replaces the graph catalog we lost from `.scratchpad`. Status graphs such as 0x0079 and 0x006C are not in it.
- Presence rule (`graph.SyncVar.has_presence_bit`): an m_syncVars entry with an id and instance scope. That's the same rule as `graph_layout.py`, so it doesn't explain the 0x0079 var27 mismatch.

## 4. Smaller items

- `commands.py` decodes the sub-frame click (time in 255ths plus the aim at the click) and the view delay. The client's hit test runs at `frame time - (extra + round((1 - sub/255) * frame ms))` (`0x7FF7894BEEC0`). Our vendor skips the sub-frame bits, so fast flicks are judged at the end-of-frame aim.
- `voicelines.py`: spawn voice through the body's networked voice stimuli (18 heroes).
- `notices.py`, `onfire.py`, `stats.py`: kill notices, On Fire, stats.

## 5. Weapon fire, compared 2026-11-09

- **Shot mode rule** (`projectiles._sync`): derives a Volley's shot mode (1 shots and projectiles, 2 shots only, 3 none/automatic) from its authored fields. It matches all 11 modes we read live in the client (research/bot-fire.md); our tables are now tested against it. Their runtime evaluates the fields at shot time, so Sombra's dynamic rate (var 8421) could make it call her automatic (3) where the client says 2.
- **Remote shots** (`_show_others`, marked unverified there): create the projectile entity and name it in the shot record, with no ProjectileVisual snapshot ("the record starts it"). Ours, verified live, also sends the snapshot with position and velocity. Tried live without the snapshot (run 31, research/bot-fire.md): only Cassidy's hitscan tracer draws; Ana's darts, Torbjörn's rivets and Zenyatta's orbs do not. Their recipe would show no flying projectile to other players. Taken for hitscan: Cassidy's bot shots now use it, which dropped our guessed 400 m/s tracer flight.
- **Frame size**: their `server.py` has the loop we fixed (`while len(payload) > MAX_PAYLOAD and len(entities) > 1`), so a big reliable message (a new bot's 20301) can make a datagram the client drops, resent the same forever.

## 6. The open items, tested 2026-11-09

Their mover, collision and VM code was run for this (stdlib only; never the launcher). Their Range and King's Row collision caches were built from our install (`tools/build_collision.py`, 1 s; git-ignored in the copy).

**Mover values** (`data/game_movers_174.json`, 38 bodies):
- Ours is right on run 5.5, the 0.9 backpedal (it holds for the 6.0 heroes too: 5.4 / 6.0), crouch 3.0, jump 6.0 and gravity 17.5 (1.75 × 10).
- Ours is wrong on: Tracer and Genji run 6.0 (back 5.4); the capsule is 2.0 m × 0.5 m (ours 1.8 × 0.4), tanks 2.3-2.5 × 0.7, Torbjörn 1.5 m; the ground acceleration is 35 m/s² with deceleration terms (ours is instant).
- Moot if the port below is taken: it reads these per body.

**Mover port** (`mover.py`, `contacts.py`, `collision.py`, `props.py`):
- Their 45 mover tests pass, the 9 real-map ones on the caches built from our install.
- Against our client: a 57 s Range walk (straights, backpedal, strafes, diagonals, a jump, a crouch walk, turns, a strafe into the spawn table), recorded with `tools/research/mover_record.py` + `mover_walk.py` ([walk](lizrollin-runtime/range-walk-2026-11-09.rec.gz): 7,937 frames at 7 ms, 1,574 with input), replayed by `tools/research/mover_replay.py` from the client's feet:

  | Mover | Horizontal drift (end / most) | Vertical (median / most) | Per-frame step error (p99 / most) |
  |---|---|---|---|
  | Ours (fallback, no `OW_CLIENT_SYNC`) | 2.647 m / 3.923 m | 0.913 m / 1.050 m | 38.3 mm / 841 mm |
  | Theirs, Soldier: 76's data | **0.000 m / 0.038 m** | **0.000 m / 0.024 m** | **0.0 mm / 45.3 mm** |

- Ours climbed the spawn table on the sideways leg (it stays 0.94 m up) and never came back; theirs follows the client to the millimetre on all but a few frames (the most is on landing).
- So their port gives exact positions without the ASI feed, and makes owner corrections (`correction.py`) possible. Not tested: other heroes, other maps, Sprint and movement abilities (their known gaps: mods of scripts the server does not run, knockback, abilities that take over the move).

**Statescript VM** (`script/`):
- Their coverage check (`coverage.missing`) for all 32 heroes: each lacks the encoder for 2-5 networked state classes (4-16 states, none StEC); `STU_861D9015` and `STU_9935FBC8` for every hero.
- Run test (`BodyScript`, 16 ms frames: 3 s primary fire, idle, ability 1, ability 2, 1 s secondary): all 32 heroes run with no exception. Shots in the 3 s match the authored rates where known: Soldier: 76 27 (9/s), Cassidy 6, Mei and Sombra 60, Bastion 24, Orisa 36, Zenyatta 8, Pharah 4. Zero for beams, melee and Hanzo (the bow fires on release, after the window), but also D.Va (0) and Ashe (1), which look wrong. Some instances log "too many events".
- So the VM runs every kit offline; what it sends was verified live only for Soldier: 76.

**Score events (003F.054)**:
- `003F.054` is index 0x3F of type 0x054 (generic settings): GUID `0x0CA000000000003F`, in our install's root manifest (`tools/assets/stu_graph.py 0x0CA000000000003F`): one `STU_A1BCAB6E`.
- `m_3ED74851`: 23 score events, each a weight (1, 25, 10, 5, 2.5, 3.5, 1.5, ...) and an id `0x0D80...` (not stand-alone assets; names not found).
- `m_AFED546B`: 20 entries of (number, kind, value), starting `20 -> 90.0` and `19 -> 0.04`. 19 and 20 are the score and threshold fields of component 64 that `onfire.py` sends, so this may be the On Fire threshold and a rate, which `onfire.py` says the data lacks. Not decoded further.
- The fire meter shows only in the Tutorial mode; any client shows the portrait burn (component 64 field 36).

## 7. The ported mover, checked 2026-11-09

Recorded with `tools/research/mover_record.py --observe`: the client's feet are logged, but `client_sync` does not correct the server's copy, so the live server's own dead reckoning is what's compared (`mover_replay.py --live`).

| Check | Frames | Horizontal drift (end / most) | Vertical (most) |
|---|---|---|---|
| Range, Soldier: 76, the walk course ([rec](lizrollin-runtime/range-observe-dva-2026-11-09.rec.gz), first part) | 12121 | **0.000 m / 0.038 m** | 0.031 m |
| Hanamura vs. AI (Attack, setup), Soldier: 76, the walk course ([rec](lizrollin-runtime/hanamura-observe-2026-11-09.rec.gz)) | 6561 | **0.000 m / 0.039 m** | 0.041 m |
| Range, D.Va's mech, four Boosters flights (same rec, second part) | 12229 | 20.0 m / **24.3 m** | 3.9 m |

- **Walking:** exact on both maps with no correction, and the offline replay of the Range walk gives the same numbers. Hanamura used its own cache (`vendor/cache/collision/0165.col`) with the Assault mode's props.
- **D.Va's Boosters were wrong** (fixed: section 8). An offline replay through `PlayerMover` with the logged flights reproduces the live server exactly, so the cause is our flight model (`PlayerMover._fly`), not timing:
  - The client's boost climbs onto the spawn table as walking would (its feet rose 0.94 m and it stayed on top). Ours caught on the table's edge at y 1.44 / 1.61 and stopped.
  - On the table top, our feet were 1.2 cm below its top. The next flight could not move at all for its 2 s, while the client flew 17 m.
  - Speed is right while nothing is in the way: 0.21 m behind after 1 s of open flight.
  - The mech body (`0x0400000000000407`) has no entry in `game_movers_174.json`, so it runs on Soldier: 76's capsule (2.0 × 0.5 m). It is the only one of our 32 heroes missing; their `tools/extract_game_movers.py` reads it from a DataTool dump.
  - Their port does not model Boosters either: its corrections cover only Soldier: 76 (`correction.HEROES`).
- **`walkplan.py`** runs (about 1 s with the nav cached). Its replay now catches on a wall pillar at Hanamura (29.3, −1.0), where the nav path's corner passes within the 0.5 m capsule. The old 0.4 m mover slid past it and ended 1.9 m from objective A; the new one ends 34 m short. The client has the same capsule, so a live walk of those legs would stop there too: the planner needs clearance from the corners.
- Getting into a `--map` match: the Practice Range card always opens the Range. TRAINING (102, 218) → PRACTICE VS. AI (1499, 360) → EASY (1061, 360) → the hero → CONTINUE (1280, 673), in the 2560 × 720 window.

**Owner corrections (`correction.py`, 114 lines), sized:**
- It needs:
  - their `world.Movement` fields (throttles, input frame, air ticks, gravity, frame, crouch frame, spring, fall) and `write_movement`, replacing our zero-state writer;
  - `command_ack` in `server._payload`;
  - `own_record` in the match's frame.
- The vendored mover already keeps the per-frame `history` it sends.
- The full state is why our own-body ch3 records sank the camera (research/hitbox-drift.md, "Dead end"): every field we left out reached the client's mover as zero.
- Gain: the server could move the player (knockback, pulls, teleports) and the client would follow. Walking needs no correction now.
- Risk: every movement ability the mover doesn't follow gets pulled back by each record. Boosters is one; others: Genji's double jump, Lúcio's wall ride, Mercy's flight. So, like theirs, it would be on for Soldier: 76 only.
- Their `Movement` also carries what another client animates a body from (throttles, input frame, air ticks). That may matter for our bots' walk animations.

**Statescript VM, sized:**
- About 3.1k lines (`driver`, `nodes`, `runtime`, `rulesets`, `coverage`) on top of the decoder we already vendor.
- It would replace the hand mapping in `tools/lobby/kits.py` (875 lines) and parts of `match.py`.
- It is verified live only for Soldier: 76.
- A cheaper first step, like the decoder oracle: run it offline beside our kits and compare the states and variables each sends for the same inputs.

## 8. D.Va's Boosters, fixed 2026-11-09

`tools/lobby/player_mover.py` now runs Boosters through the client's walking mover:

- **Read from the data.**
  - Graph 0x047E's MovementMod (instance 79; `tools/assets/stu_graph.py 0x058000000000047E`):
    - instance 88 swaps in the mover values of instance 89: the mech's, with air acceleration 35 (the mech's is 3.5);
    - SPEED ADD 1.18: 5.5 × 2.18 = 11.99 m/s;
    - DECEL_AIR SET 1, character collision SET 0;
    - throttle mods 17 SET 1 and 18 SET 0 (read at 0x7FF6D1E53960): forward is forced.
  - The mech body's own `STUCharacterMoverComponent` (`0x0400000000000407`, missing from `game_movers_174.json`): Soldier: 76's values with a 0.7 m capsule (`LOCAL_MOVERS`).
- **Fitted.** The state's flag `m_485109CB` (read at 0x7FF6D1F59590, behind obfuscated code) is not decoded.
  - Fitted as `Mover._accelerate`'s turn toward the wish, in 3D, with the wish along the aim, pitch included.
  - Also fitted: gravity off, no landing while the flight runs, and the touching acceleration at 35. The component's own touching value puts the first recording's flights 10–23 m off.
- **Why a 3D turn.**
  - Aimed 39.6° down onto the floor, the client slides at 11.58–11.62 m/s. That is the 3D turn's steady state against the floor (11.60).
  - A level flight would give 11.99 m/s, and a flight along the aim with the floor clipping it 9.24.
  - Aimed 39.6° up, the client climbs at the aim's angle, at 11.99 m/s.

Most off per flight, up to 300 frames after it (m):

| Recording | Level flight (before) | Along the aim |
|---|---|---|
| [range-observe-dva](lizrollin-runtime/range-observe-dva-2026-11-09.rec.gz): four flights at pitch 0 | 0.54 / 0.32 / 1.75 / 1.91 | 0.54 / 0.32 / 0.26 / 0.58 |
| [range-boost-dva](lizrollin-runtime/range-boost-dva-2026-11-09.rec.gz): straight, steered, W held, into a wall, aimed up, aimed down | 0.14 / 7.03 / 7.07 / 0.97 / 21.6 / 0.62 | 0.14 / 0.62 / 0.20 / 0.85 / 0.32 / 0.55 |

The old kinematic `_fly` was 24.3 m off (section 7).

- **While a flight runs, ours is exact but one frame early.** It is 0.000 m from the client's feet one frame later. Walking has no such offset.
- **Flight start and end.** Against match.py's timing (2 s on 7 ms frames), each flight starts and ends up to a frame off. Shifting every flight by a frame helps some and hurts others, so the timing is unchanged. That, and the fall after, is the rest of the error.
- **The steered flight's 3.4 m climb at pitch 0** is now followed: the 3D turn keeps what a slope pushes up.
- **`range-boost-dva`.** Frames 7570–14321 of a live run with resync off (`mover_record.py --observe`, then `mover_walk.py --hero D.Va --course boost`). The server then still ran the level model.
- **Not checked:** strafing during a flight (the wish takes it as walking does), other maps, and a live run with this model.

## 9. Owner corrections, ported 2026-11-09

The server now sends the player's own body its state, as LizRollin's `correction.py` does (section 2):

- **Code.**
  - `tools/lobby/owner_correction.py`: their `correction.py`, with our gate and ack.
  - `vendor/ow174/game/world.py`: their movement-state writer (the full state: input frame and throttles, air ticks, gravity, crouch frame, spring, spring speed, fall; 10-bit frame counts; the record's own frame).
  - `server.Client._frame` computes the packet frame first, so `Match.remote_movements(viewer, frame)` can add the own record. The command ack stays at most 2 past the newest record.
- **Who gets records.** Heroes in `owner_correction.HEROES` (Soldier: 76 only), once the client has the body's create. Since 2026-11-10 every hero but D.Va (section 21).
  - Off with `OW_CORRECTIONS=0`.
  - Off while `OW_CLIENT_SYNC` moves the server's copy to the client. `mover_record.py --observe` keeps them on (`client_sync.OBSERVE`).
- **Live check** (Range, Soldier: 76, the walk course, resync off; [rec](lizrollin-runtime/range-correction-nudge-2026-11-09.rec.gz)):
  - Before the nudge, the client's feet were within 0.039 m of the server's, as without corrections. No errors.
  - `mover_record.py --nudge 1.0` lifted the server's copy 1 m at the first crouch (frame 18012). Ten frames later (one round trip) the client's feet took the server's state. They then matched it to 0.000 m through the fall and the rest of the walk.
  - One 13-frame stretch later, stepping onto the spawn table, the client's feet ran 2 frames behind the server's (0.86 m up, 0.08 m flat), then matched again.
- **Why only Soldier: 76.** The movement mods each body's graphs put on, from the catalog (`STU_316CFEF2`):
  - Soldier: 76, Sprint (graph 0x0255): SPEED ADD var1002 (ours: +0.5, followed) and CROUCH_INTENT SET 0 (not modelled: a crouch during Sprint may be pulled back a frame). Our Sprint also skips the toggle option and the 0.3 s re-sprint delay (`soldier.Sprint`), which a correction would now pull back.
  - D.Va's mech (not in the catalog; `stu_graph.py` on 0x050A and 0x0276):
    - Fusion Cannons (0x050A): DECEL_AIR ×10 while firing. Not modelled; a live walk, strafe and jump while firing matched the server within 0.040 m ([rec](lizrollin-runtime/range-fire-dva-2026-11-09.rec.gz)): no speed penalty.
    - HUD graph (0x0276): no-move states (throttles, jump and crouch SET 0 at priority 99).
    - Boosters: fitted, up to 0.85 m off at a flight's start or end (section 8), which a record would snap. So D.Va stays out.
  - Every other movement ability (section 11) would be pulled back by each record. Measured in section 21: on a kits body the client runs none of them, so nothing is pulled back.

## 10. Their VM beside our kits, 2026-11-09

`tools/research/vm_shadow.py` runs their VM in place (its own process) and our `kits.frame_builder` (read back by the vendored decoder). For each hero's weapon graph it compares the states on and the variables sent: idle, firing, released, reloading. Report: [vm-shadow-2026-11-09.md](lizrollin-runtime/vm-shadow-2026-11-09.md).

- **Found and fixed: our frames overwrote values the graphs set themselves.**
  - Each weapon graph's Entry SetVars var229 / var230: the Volley's spread range, which a Stack lerps into var243. Tracer 0 to 3.5, Cassidy 0 to 6, Roadhog 8, Widowmaker 3, Ashe 2, Wrecking Ball 2, Sombra 0.5 to 2.4, Orisa 0.25 to 1.2, Bastion 1.2.
  - `weapon_instance` sent 229 = 0 and 230 = 0 for every hero, and 476 = 20 (Genji's graph sets 29, Orisa's 11, Roadhog's 6.6).
  - Now `kits.AUTHORED` carries each graph's constant SetVars, and `test_kits` checks the table against the catalog. The run's differing variables went from 1492 to 1436.
- **Leads (closed in section 19):**
  - var581, the reload seconds: ours come from `target.PLAYER_KIT`'s measured timings. The graph's SetVar differs for Ashe (1.5 vs our 3.55, a per-shell reload) and Sigma (1.3 vs 1.5).
  - var476 where the graph computes it (Baptiste 24, Sombra 8, Wrecking Ball 5, Winston 60 in their VM): ours stays 20.
- **Expected differences:**
  - Their VM keeps on many states the client runs itself: BooleanSwitch, Stack, LogicalButton, ChaseVar, DataFlowMapping (1918 states).
  - Ours-only states (278) are mostly the WeaponVolley while firing and our reload branch's Anim, Stack and Wait. The live client takes these.
  - var1648 (beam range): ours is the Entry Stack's constant, since the client computes 0 for our player (as theirs does).
  - D.Va's Fusion Cannons: their catalog lacks the mech, so their VM runs no instance of it.
- **Not checked live:** whether the spread values now show (tracers, crosshair).

## 11. Other heroes' movers, 2026-11-09

Live walks of the same course (`mover_record.py --observe`, `mover_walk.py --hero NAME`), server against client:

| Body | Rec | Horizontal (end / most) | Vertical (most) |
|---|---|---|---|
| Tracer (6.0 m/s; top seen 6.02) | [rec](lizrollin-runtime/range-walk-tracer-2026-11-09.rec.gz) | 0.000 m / 0.042 m | 0.029 m |
| Reinhardt (tank capsule) | [rec](lizrollin-runtime/range-walk-reinhardt-2026-11-09.rec.gz) | 0.000 m / 0.039 m | 0.036 m |
| Torbjörn (short capsule) | [rec](lizrollin-runtime/range-walk-torbjorn-2026-11-09.rec.gz) | 0.000 m / 0.041 m | 0.040 m |

`test_player_mover.py` replays all three with Soldier: 76's walk.

Movement abilities that neither their code nor ours models, from each body's MovementMods (the catalog):
- Genji and Hanzo (0x0157, 0x010C): the same set (SPEED SET 1.3, ground acceleration and deceleration ×0.5, backward throttle ×0.4, no crouch), most likely their wall climb.
- Reinhardt's Charge (0x007D): forward forced, SPEED ADD 2, steering and acceleration cut.
- Mercy's Guardian Angel (0x006E): its own mover values, SPEED ADD 1.44, air acceleration ×0.2, the flag `m_485109CB` as Boosters.
- Winston's Jump Pack (0x010A): gravity ×1.5, air deceleration ×0.5, no landing.
- Pharah (0x0025, 0x002B): SPEED ADD 0.2 and her flight's mods.
- Reaper (0x0020): SPEED ADD 0.5 for Wraith Form.
- Zenyatta (0x017B): Transcendence, SPEED ADD 1.
- Winston, Bastion and others: states that freeze the throttles (SET 0 at priority 99).

Each needs its own model, as Boosters did, before its hero can get owner corrections. Not so on kits bodies, where the client does not run them (section 21).

## 12. Their other modules, and spawn voices, 2026-11-09

- **Modules.** A reading pass over `combat.py`, `observers.py`, `bots.py`, `pools.py`, `movelog.py`, `stats.py`, `notices.py`, `onfire.py`, `heroselect.py`, `link.py` and `messages.py`. Report: [modules-2026-11-09.md](lizrollin-runtime/modules-2026-11-09.md). Nothing in them is addressed to an AI.
  - Their combat needs their VM (hits come from the shooter's Volley), so it is not a drop-in.
  - Their bots do not shoot. Our `messages.py` is ahead of theirs.
  - Worth taking, smallest first:
    - their bots' walk: turn rate, accelerations, waits, measured on OW2 traffic;
    - the bot movement's input-frame rule;
    - death UX: the dead flag 0x2000, the 25004 "killed by" notice, the kill bit in 25001;
    - armour and shield pools in component 51;
    - 25005 +0x88: ours sends a damage percent, which the client may print as a score.
    - Ported 2026-11-09 except +0x88: section 13.
- **Spawn voices.**
  - Theirs begins a voice-stimulus state in the body's statescript (needs their VM; which line plays is unchecked).
  - Ours sends the line itself (ClientInVoice 27500).
  - `soldier.SPAWN_VOICES` now has all 32 heroes: their HrSlct-EntrMtch and first Rspwn lines, from `tools/assets/voice_lines.py` on each spawn body's VoiceSet. Soldier: 76's and D.Va's match the lines heard in game. Genji has no HrSlct-EntrMtch line, so his is his one 0x2B line (FlvrLns).
  - The new lines were not listened to.

## 13. Bot walk, death screen and pools, ported 2026-11-09

- **Bot walk** (`target.TrainingTarget._stride`, from their `bots.py`, which LizRollin measured on OW2 traffic). Only the B8E patrols walk this way.
  - Each uses its body's mover values:
    - Training Bot 1403: 4.5 m/s, up at 4.5 m/s², down at 67.5 m/s².
    - 143E and 146E: 5.5 m/s (5.5, 82.5).
    - Far runners 14ED: 6 m/s (66, 39).
    - `PATROL_SPEED` (2.5) is left to shooters walking their route.
  - The pattern:
    1. Wait 3 s first (FIRST_WAIT).
    2. Walk stop to stop with keys held.
    3. Let go where friction stops it on the stop.
    4. Wait 1.2 s (WALK_GAP), then walk on.
  - It turns 60°/s until 0.5 s after letting go.
  - It waits only at hint points (`stops`) and walks straight through `nav.loop`'s corners. It moves in 16 ms steps.
  - Records (`Match._bot_movement`), for every walking bot (hero bots too):
    - the throttles, measured against its yaw;
    - +36: the frame its keys were last held, with +34/+35 their throttles. Their rule: when that frame is not before the record's own frame, it goes as the record's own frame;
    - gravity 1.75.
    - A test reads them back with their reader.
- **Death screen.**
  - 25004 to a killed player, with one of two values:
    - KILLED_BY (2) and the killer's entity: "YOU WERE KILLED BY <name>";
    - 0 and his own body: "YOU DIED".
  - `_damage_player(..., by)` gets the killer from bots' shots (`Projectile.by`), weapons and abilities.
  - 25001 bit 1 on the killing hit. Their reading, unverified: the kill marker.
  - A dead player's movement has flag 0x2000: the dead pose in other clients' 3P view.
  - Owner corrections pause while the player is dead. His client sets that flag on its own state, and our records do not carry it.
  - A dead player takes no more damage. Before, a hit reset his respawn timer.
  - Not ported:
    - the kill feed line (003E.025 to the body script needs their VM);
    - 25005 +0x88, which stays the damage percent.
- **Pools** (`world.health(now, most, armour, shields)`; `ow174/game/pools.py` vendored byte for byte).
  - Armour and shields get their own parts:
    - armour: type 1, parts from 16;
    - shields: type 2, from 32;
    - both use LizRollin's bytes (0, 1, 0, 1).
    - The health part stays ours (type 3; 1, 1, 0, 0).
  - The total stays one number. Health fills first, then armour, then shields, so damage takes shields, then armour, then health.
  - Armour's damage cut and shield regeneration are not modelled.
  - 0033's values match our kit totals for every hero except two:
    - Orisa: 450 in the data, 400 in our kit. The kit is fixed.
    - D.Va's mech: its pools are in 0109, so it stays one 600 health pool.
- **Checked live 2026-11-09** (a scratch lobby wrapper moved one Training Bot patrol onto a 10 m line in front of the spawn and dealt damage on command):
  - Walk: the bot sets off, stops at each end, waits and walks back; it sets off sideways and turns as it goes, leaning with its thrusters ([frames](lizrollin-runtime/live-bot-turn-2026-11-09.jpg)). No jumps; the client read every record.
  - Reinhardt: 12 white and 8 yellow segments at 500/500; 150 damage left 350/500 with 2 yellow (armour went first). Zarya: 8 white and 8 blue; 100 damage left 300/400 with 4 blue ([bar](lizrollin-runtime/live-shields-2026-11-09.jpg)). The type-3 health part and their type-1/2 parts draw together correctly.
  - Death: "YOU WERE ELIMINATED BY TRAINING BOT", the red death vignette, then a clean respawn at full health and armour 3 s later ([screen](lizrollin-runtime/live-death-screen-2026-11-09.jpg)).
  - Killing hit: a red marker round the crosshair for about 0.45 s, with the elimination burst and "ELIMINATED TRAINING BOT" ([frames](lizrollin-runtime/live-kill-marker-2026-11-09.jpg)). Not separated: which part comes from the 25001 kill bit and which from 25005.
  - A 16th Range bot is too many: the 20300 message that lists the bots overflowed the client's 1230-byte frame, and the load hung on "Entering game".

## 14. Damage data, respawn, elimination score and armour, 2026-11-09

- **Damage data** (`tools/research/hitdef_shadow.py`; report [hitdef-shadow-2026-11-09.md](damage-data/hitdef-shadow-2026-11-09.md), raw [JSON](damage-data/hitdef-theirs-2026-11-09.json)).
  - How it works: their VM runs each hero's body for a second of idle frames, then their `combat.hit_def` evaluates every WeaponVolley's numbers:
    - damage, falloff start, end and floor, the critical multiplier;
    - an explosion's amount and rings;
    - shots per second, pellets, projectile speed.
  - Matched our wiki tables already: most primaries.
    - Ana, Ashe (hip fire), Baptiste, Bastion (Recon), Cassidy, Doomfist, Echo, Genji, Lúcio, Orisa, Reaper, Roadhog, Soldier: 76, Torbjörn, Tracer, Widowmaker, Wrecking Ball, Zenyatta, D.Va's pilot.
    - Pharah, Junkrat and Sigma's projectiles too: impact plus explosion, which our tables count as one hit.
    - Helix, Fire Strike, Flashbang, Concussion Mine, Sticky Bombs, Sleep Dart.
  - Fixed from the data:
    - Mei's spray flies at 40 m/s, not 20.
    - Rates: Sombra 20 shots/s (not 20.8), Bastion's Sentry 30/s (not 31.25), Fan the Hammer 8/s (0.125 s).
    - Ana's Biotic Grenade: 50, a literal in 0B97.0 (the wiki says 60).
    - Sigma's Accretion: 30 on impact plus 40 in the explosion (1BBB.3's two SetVars). Our 70 impact plus 40 splash dealt 110 on a direct hit.
    - Flashbang reaches 6.5 m (was 7).
    - Ability splash follows each Volley's own rings: Flashbang full within 3 m, Biotic Grenade within 4 m, Concussion Mine within 3 m, Sticky Bombs within 2 m, Accretion full to 1.5 m and 30 % at 2.5 m. Before, every ability used Helix's shape.
  - Headshots: `Weapon.crit`.
    - The data's multiplier, where a Volley sets one, matches the wiki's headshot flag: 1 for Bastion's Sentry, Mei's spray, Pharah, Junkrat, Sigma and Fan the Hammer; 2 for Soldier: 76.
    - An unset multiplier is not "can crit": Ana's darts are unset and cannot headshot.
    - So the flag comes from the wiki pages (`research/wiki/roster`).
    - No headshots: Ana, Brigitte, Junkrat, Mei, Moira, Pharah, Reinhardt, Sigma, Symmetra, Winston, Zarya, Bastion's Sentry, Fan the Hammer, Zarya's alt fire, quick melee.
    - We had doubled every hero's head hits.
  - Not comparable from the data: beams and melee (no Volley), D.Va's mech (its graphs are not in the catalog), Hanzo's draw speed, and heal Volleys whose amount is a heal.
  - `tools/research/test_hitdef_shadow.py` pins our tables to the saved snapshot.
- **Respawn time.** The mode graph sends each body script game message 06FD.025 with the time:
  - the objective modes' 0F4A: its var20680, whose one SetVar is 10;
  - the Practice Range's 13C0: 3.
  - `Match.respawn_seconds`: 10 s in Assault, Escort and Hybrid (players and hero bots), else 3.
  - Checked live on Hanamura: 10.0 s from death to respawn.
- **Elimination score (25005 +0x88).** The client prints it next to a flame: "ELIMINATED TRAINING BOT 100", the on-fire score. Their 0 hides it; ours (the share of the damage dealt) shows. Kept. Unverified: what retail sent.
- **Armour** (`target.armour_cut`). It stops 3 of each hit, at most half: OW1's rule since the January 24, 2019 patch, from the patch notes in `research/wiki`. Their code uses the older 5.
  - Each pellet is a hit: bots' pellet hits and players' shotguns pass their count.
  - Checked live: 20 damage left Reinhardt at 483/500.

## 15. The VM, ported 2026-11-09

The statescript VM now runs in our server, opt-in per hero (`OW_BODY_VM`).

- **Vendored** (byte for byte, `tools/lobby/vendor/README.md`):
  - `script/{runtime,nodes,rulesets,coverage,driver}.py`;
  - their `statescript.py` (the VM's frame encoder) as `script/wire.py`, since our `statescript.py` diverged.
  - Local changes: one import line each in `driver.py` and `coverage.py`.
  - Their driver, runtime and ability tests run in our tree (`test_vm_{driver,runtime,abilities}.py`, 46 tests).
- **The adapter** (`tools/lobby/body_vm.py`). The VM runs the body's graphs and sends its frames; our server keeps the gameplay:
  - shots: each Volley's shot goes to our damage code. Soldier: 76's rifle (0x0254 st10) to `fire_at_target`, Helix (0x0254 st16) to `fire_helix`, Biotic Field's throw (0x0257 st1) to `throw_field`. In `OW_BODY_VM=all`, other heroes' primary Volleys (`target.WEAPON_GRAPHS`) go to `fire_at_target`; their other Volleys are logged once and not played;
  - quick melee: 0x0043 st4's activations (their `combat.MELEE`) to `fire_at_target(..., MELEE)`;
  - the ultimate: its Ability state's start zeroes our charge; for Soldier: 76 it starts Tactical Visor's server aim;
  - movement mods: the VM's (`mover.statescript_mods`) replace `PlayerMover`'s own Sprint mod (`script_mods`);
  - into the VM: health (`set_health`, the HUD's #1732/#1737 and the graphs' dead check) and the ultimate charge, scaled to the graph's cost #338 (2310 for Soldier: 76). The VM's own charge over time is off.
- **Lifecycle** (`match.py`). The VM starts with the body's create and sends its full frame once the client acknowledges the create. Then one owner delta per new command frame. A hero switch, respawn or leave retires it. If it raises, the body goes back to `kits.py`'s frames.
- **Offline proof** (`test_body_vm.py`). A VM Soldier in a Practice Range match:
  - the client's reader (their `ClientModel`) reads every frame and ends with the server's states on;
  - 9 rifle shots in 1 s of held fire, one Helix, one Field throw, one melee;
  - Sprint's SPEED ADD 0.5 on the mover while sprinting;
  - health and charge reach the VM; Q starts the visor; a failure falls back to kits.
- **Checked live** (Practice Range, `OW_BODY_VM=1`, Soldier: 76; [HUD](lizrollin-runtime/live-vm-hud-2026-11-09.jpg), [frames](lizrollin-runtime/live-vm-soldier-2026-11-09.jpg)). No VM error in the lobby log; every HUD number below comes from the VM's frames:
  - ammo 30/30 → 22 after 0.9 s of fire (8 shots) → 0 after the rest → 30 after R;
  - tracers while firing; the Helix rocket; Biotic Field's ring on the floor;
  - cooldowns: Helix 6, Field 15, counting down;
  - Sprint: the Shift icon lit, the body carried out of the spawn room, Helix greyed out while sprinting (the graph's own rule);
  - the quick melee swing.
  - Driven by `tools/research/vm_live.py`. The first run's background clicks came before the menu was ready, so it retries until the lobby log shows "loaded the map" and "plays Soldier".
- **Second live check, same day** (`vm_live.py launch` / `drive`: one Training Bot patrol moved to stand still 8 m along the aim, team 1, with `ult` and `kill` commands through a file; [frames](lizrollin-runtime/live-vm-ult-death-2026-11-09.jpg), [HUD after the respawn](lizrollin-runtime/live-vm-respawn-hud-2026-11-09.jpg)):
  - Bot damage: 0.6 s of fire, 6 hits at 7.5 m, the last 3 critical (the recoil climbs to the head), 200 → 20. The ultimate meter rose to 7% from that damage.
  - Ultimate: `ult` filled the charge, and the client played the "ultimate ready" burst. Q started the VM's ultimate (`Soldier: 76 ultimate (VM)`), and the visor overlay and lock-on drew.
  - Tactical Visor: the server aimed its shots. It killed the still bot, then bots at 33 m and 36 m ("ELIMINATED TRAINING BOT 100" twice), and ended after 6 s.
  - Death: "YOU WERE ELIMINATED BY TRAINING BOT", then the respawn 3 s later on a new body, which got a new VM.
  - New body: ammo 30/30. 7 hits killed the revived bot, and the ammo fell to 21. The meter showed 8%. Helix flew and its cooldown showed 6.
  - The lobby log had only the VM's known one-time notes ("config var ... is not computed on the server" and similar), no errors.
  - Found on the way: at the Range spawn the client faces yaw −32768 (180°) in every run, while `spawn_point` gives −170.3°. Shots use the command's yaw, so the hits are unaffected. The server's spawn facing is 9.7° off what the client shows.
- **Not done:**
  - other heroes' abilities and ultimates (server effects);
  - the kill feed: started in section 16, and it doesn't draw yet;
  - their spawn voice through the body script (ours still sends 27500);
  - component 85, which our kit sends and theirs never does.

## 16. Kill feed, fixed 2026-11-09

- **Their path** (`combat._died`; their WIP.md lists it as "written but not checked in the game yet"):
  1. A player's death sends game message 003E.025 to the dead body's script, with parameters killer 0012, crit 0095, loadout 03E6 and assists 03E7.
  2. Its 01CF stores the killer in #392 and pulses st108 (a ClientOnlyPulser).
  3. On the client, st108's client-only chain runs nodes 64 → 65 → 63/62 → 66/67 and sends 0765.025 to the game mode entity (`STU_A3DEF368`):
     - 64 tests `STU_CD7F3617(owner)`, the body's name;
     - 65 tests `#392 && #392 != owner()`; when true, line 66 names the killer, otherwise 67 sends a line without one.
  4. The mode entity's kill feed 0DEA, a client-only child of 0CB6, takes 0765.025 and builds the line.
  - Bots have no body script, so their deaths get no line on either side.
- **Ported:**
  - `body_vm.BodyVM.died`, called from `Match._damage_player`.
  - A fix in the vendored `driver.py` (`_kept`). Their driver merged every graph's `m_9CEC6985` server-only list into one set, so 01CF's synced #392 (presence bit 15) never went out, because 0043, 0B8B and 01CA keep their own #392 server-side. Now each instance uses its own graph's list.
  - Offline: the client model's 01CF holds the killer.
- **Live (`vm_live.py`, killed by the still bot):**
  - `tracer.asi` shows the client's 01CF st108 beginning at the death and reading `1` + count 1 (fresh, one pulse).
  - No line draws in the feed area, before or after the driver fix.
  - The tracer never sees the mode entity's graphs (13C0, 0CB6, 1FC0, 0DEA: 0 lines), even though 1FC0's notices draw. So it can't show what 0DEA does.
- **0DEA's gate** (`expr.source` on the catalogue's 0DEA; corrected the same day):
  - 0DEA's 0765.025 entry (node 4) goes on only when `#9380e` is true (node 3). State 0, a BooleanSwitch on #9380e, also shows the feed's presenter (state 1).
  - Entry → nodes 20, 22, 23 → 21 (#9380e = False) → GameMessageEntry state 11 (07E3.025). Its *idle* subgraph (`m_33DA5163`; vendored `nodes.py` 1030: idle until the message comes, then `m_452862E6`) is Stack state 2: #9380e = `!STU_A77AC2AC() && (STU_0D179CAF() || !STU_33C5B1DE() || STU_CEFF290A())`.
  - So #9380e holds that value from the start, and **07E3.025 turns the feed off**. An earlier reading here had it the other way round.
- **07E3's senders** (`tools/assets/graph_messages.py --find 7E3`, the whole install): 12B8 state 2 and 2983 state 0.
  - `tools/assets/asset_users.py` (all 470,105 assets): 041A → 1147 → 12B8 only. Client code creates 041A (0x7ff6d1d1c29a, a component's vfunc +0x70), so that chain is menu UI.
  - 2982 and 2983 are referenced only by the type-0x0CA assets `0x0CA0000000000155` (lobby-settings schema: `STU_C5BE2B08` entries with identifier, min and max) and `0x0CA000000000019D` (a graph list). Nothing references those two, so the client loads them by type.
  - 2983 (`Graph(0x2983, 3, 1, 2)`): entry on 13B8.025 → state 2 → state 4 (`BD02E168`) → client-only switch 3 (`GetOwner() == STU_8985E00A`, the local player; bytecode `0E 00 0E 01 25`) → client-only state 0 (`38EE1100`), which posts 07E3.025 to the game mode entity (`STU_A3DEF368`). That makes it the "hide the kill feed" side of a setting.
- **Live tries, all reverted:**
  - 12B8 on the mode entity with slot 1 on (runs before this section's correction).
  - The body's component-85 flag at body start: no change.
  - 2983 on the player's controller entity, once the mode script was on the client (owner bits 1 and 2). `tracer.asi` shows the client running its states 2, 4, 3 and 0, so 07E3 went out. Still no line, as expected now that 07E3 hides the feed.
- **The gate's natives** (IDA: the class registration passes a type descriptor; a factory with the same descriptor builds the delegate whose vtable +0x20 evaluates it):
  - `A77AC2AC` (0x7FF6D1F08580): true only when the game mode is `0x0230000000000019`. False in the Range.
  - `33C5B1DE` (0x7FF6D1B5A2E0): the value of UI variable `0x0D800000000004AA` (`sub_7FF6D1E08720`), likely a settings value.
  - `0D179CAF` (0x7FF6D1B4CFC0) and `CEFF290A` (0x7FF6D1B4E0A0) jump into packed code (`loc_7FF6D12D7C60`), so their meaning is unread.
  - So the gate is closed only if `0D179CAF` and `CEFF290A` are false and UI variable `0x4AA` is true.
- **Seen live** (`tracer.asi` runs 13-17; for these runs `vm_live.py` sent a second 0CB6 one second before the kill, so its 0DEA started while the tracer watched, and the screen then drew every notice and line twice; removed since):
  - The tracer never showed the mode entity's graphs because they all start within the first second, before a rescan finds the new system, and then stay idle. A big-region scan found nothing more and was reverted.
  - The new 0DEA begins states 0, 1 (the feed presenter under the #9380e switch), 2, 11, 17 and 21, so **the #9380e gate is open**. The gate's natives all evaluate to 0 (`A77AC2AC`, `0D179CAF`, `33C5B1DE`, `CEFF290A`; run 15, `classeval`).
  - At the death the client runs 01CF's send: `CD7F3617` (the body's name), then the target `A3DEF368` = **`0xD0000010`, our mode entity**.
  - In the same tick 0DEA's line path runs (`F76C486C` at node 50, `CD7F3617` ×2 at nodes 57/58, `AECBBA41` at node 56; run 16). So 0765.025 arrives, passes the gate and builds a line.
- **Why it didn't draw** (run 17): the presenter (state 1) shows only while its `m_3DBF6FD2.m_condition`, #1962, is true. Stack state 12 sets `#1962 = STU_016D65A8() ? (STU_F4C77F4A() || STU_CEFF290A()) : STU_CEFF290A()`.
  - `016D65A8` evaluated to **0**, and `CEFF290A` to 0, so #1962 was false.
  - `F4C77F4A` (evaluator 0x7FF6D2D9AB10) evaluates its child with the context at `+1216` set to its `m_4BA4C058` (5 here). The child is `STU_3832D36C` GetGameRulesetValue on the mode entity, key identifier 9540 = `0x2544`. Schema 0x0006, which the Range's mode 0018 lists, holds the custom-game general settings, so `0x2544` is likely Kill Feed (default True). The evaluator (0x7FF6D1EEFAC0) first reads the expression's value and falls back to the ruleset holder's sparse map only when that has none.
  - 0DEA's other two `F4C77F4A` switches (states 3 and 6) read the local player's #6777 in context 1. They are 0 in our runs and gate only #9514 (state 16's A9F post), not the line.
  - `016D65A8` (0x7FF6D1B55C50) = `!valid || [system 9 + 700]`. Here `valid` (0x7FF6D1993E30) = game state (`+148` of `0x7FF6D17FD6B0`'s result) == 4 && system 9's 16 bytes at `+224` set && its byte `+753` set.
  - System 9 is entry 9 of the client's system table, `(0x4E3494C633194824 - [0x7FF6D4104BD0]) ^ [[0x7FF6D41786E0] + 320]` (`0x7FF6D1846360` returns the 9; research/main-menu-normal/owned-skins.md has system 70). It is the account settings system (vtable `img+0x26113C8`).
- **What fills `+700`:**
  - The Options code names it. `0x7FF6D1CC74C0` copies system 9's bytes to the Options UI: `+700` to `0x0D80000000006426`, `+701` to `0x6047`, `+703` to `0x62BC`, `+704` to `0x5FE9`, and the `+736` map (keys `0xE0xxxx`, 20802's `+0x130`) to more.
  - The save code (`0x7FF6D1CC78E0`) copies the struct at system 9 `+0x2B8` and fills it from the UI: byte `+4` from `0x6426`, `+5` from `0x6047`, `+7` from `0x62BC`, `+8` from `0x5FE9` (an int). That is the layout of 20802's block `+0x120` (u32, then bools and a byte), the block 22203 saves.
  - The Options defaults (`0x7FF6D1CC8650`) set `0x6426` and `0x62BC` to 1 and `0x6047` and `0x5FE9` to 0. So retail's default has the feed on, but system 9 holds only what the server sent, and our lobby never sent 20802.
- **Live checks of the lead** (`tracer.asi` logs system 9's `+0x2B0..+0x2F7` on change):
  - Runs 18-19: the hello 20500 bools don't touch `+700`.
  - Run 20: 20802 with all five `+0x108` bools on: no change.
  - Run 21: 20802 with `+0x120` `+0x4` and `+0x7` on: system 9 `+0x2BC` = 01 and `+0x2BF` = 01. `016D65A8` = 1, and `CEFF290A` is no longer evaluated, so the `F4C77F4A` branch was true. **The line draws.**
  - Run 22, the final code (no second 0CB6): one line, "TRAINING BOT" (red, Training Bot icon) → "SOLDIER: 76" (blue, Soldier icon), next to the "YOU WERE ELIMINATED BY TRAINING BOT" notice. Frame: `lizrollin-runtime/kill-feed-2026-11-09.png`.
- **Ported:** `PracticeRange.settings` (`tools/lobby/practice.py`) sends 20802 at login, with upstream's defaults (`+0x108` `+0x4`, `+0x10D` `+0x0` and `+0x5`) plus the client's Options defaults for `+0x120`. The test is `test_login_settings_turn_on_the_kill_feed_option`.
- **Bot deaths** (added the same day): a dead bot's body gets its kill feed graph in its remote frame, with the killer in #392 and the pulser on.
  - Hero bots use 01CF itself: `Graph(0x01CF, 41, 22, 20)`, state 108 at remote slot 15, #392 at presence bit 15.
  - Range bots have no 01CF. Their four bodies (`0x1403`, `0x143E`, `0x146E`, `0x14ED`) list 0033, 0BA8, 0BE2, 0BA3 and 174F, and 0BE2 references 0C18, one of the 15 graphs that post 0765 (`graph_messages.py --find 765`).
  - 0C18 (`Graph(0x0C18, 9, 7, 3)`) is 01CF's death part alone: entry 003E.025 → `#10155` false → state 8, a ClientOnlyPulser at remote slot 1 → client nodes 75, 79, 83 / 126 → the two 0765 sends. #392 is presence bit 0.
  - Its conditions, read from the install (the opcodes rendered by `expr.source`), are 01CF's nodes 64, 65, 63 and 62:
    - `STU_CD7F3617()`;
    - `#392 && #392 != owner()`;
    - with a killer, `!#7082e || !#392.id(7082)`; without, `!#7082e`.
  - A presence bit counts the graph's instance-scope sync vars in slot order (01CF's #392 is the 16th, bit 15).
  - A hero bot's body dies once (its respawn is a new body), so its pulser is sent fresh at count 1. A Range bot is revived as the same body, so its count is its deaths: the client posts a line per count it has not seen. The fresh bit goes out only while the bot lies dead, so a viewer who first reads a revived bot gets no stale line.
  - Live, run 23 (Range bots): Tactical Visor killed two bots, two lines "SOLDIER: 76 → TRAINING BOT", highlighted as the player's own. A second death of the same body added one line.
  - Live, run 24 (`OW_LIVE_BOT=Reaper`, the still bot as a Reaper hero bot): "SOLDIER: 76 → REAPER" at its death and again for its next body; the player's death read "REAPER → SOLDIER: 76". Frame: `lizrollin-runtime/kill-feed-bots-2026-11-09.png`.
- **Left:** a line has no critical-hit mark, ability icon or assists (003E's other parameters). Saved settings are kept since the same day (research/lizrollin-lobby.md section 2), and bots killing bots were seen drawing lines in the event-map runs (section 5 there).

## 17. Hit detection, compared again 2026-11-09

Their `combat.py` against our `Match.fire_at_target`, `hitbox.py` and the rocket code, after the ports of sections 13 to 16.

| | Theirs | Ours |
| --- | --- | --- |
| A body's shape | A capsule from the feet to the mover's height, the mover's radius ("unverified as a hitbox") | Each hero's own hit volumes, decoded from its model |
| Critical hit | At or above the eye height less a constant ("unverified") | The head volumes; weapons that cannot headshot do not |
| Where the target is | At the shooter's view time, between two kept ticks | The same (`view_time`, `pose_at`) |
| Walls | Ray against the map's collision | The same (`mesh.ray`) |
| Damage | The Volley's hit definition, evaluated by the VM | Tables checked against those definitions (section 14) |
| Projectiles | Fly each tick; the first enemy or wall on the step ends them; direct damage and a blast | Helix and the abilities land where their ray met something at launch; **a player's primary projectiles hit at once** |
| Spread, pellets | One ray | One ray for all pellets |

- So their hit test has nothing ours lacks except the flight of ordinary projectile shots. 15 of our heroes fire projectiles as their primary (25 to 125 m/s), and their shots landed the instant they were fired.
- **Ported:** `Match._tick_shots`. A player's projectile shot now flies at its weapon's speed along the aim it was fired on, to the first wall or its reach. Each tick it hits the first enemy bot on the stretch flown since the last, where the bot is at that moment, so a moving target has to be led.
- **Live** (Pharah in the Range, the still bot 7.5 m ahead): the rocket fired at 35.721 s landed at 35.934 s, 0.213 s later, as 7.48 m at 35 m/s gives. Two rockets (120 each) killed the bot.
- **Left as it was:** a straight line with no arc (Junkrat, Hanzo, Torbjörn and Ana arc in the game), one ray for all pellets, no splash from a primary shot, and no view-time rewind for a projectile (theirs has none either). Whether the client draws the player's own projectile in step with this was not looked at.
- The elimination notice's number ("ELIMINATED TRAINING BOT 100") is our 25005 damage share; theirs sends 0 and shows none.

## 18. Other heroes on the VM, 2026-11-09

Upstream verified only Soldier: 76 on the VM, and so had we. `OW_BODY_VM=all` ran every hero, but only 11 did any damage (those whose primary Volley `target.WEAPON_GRAPHS` listed), and only with their primary.

- **Offline sweep** (`tools/research/vm_heroes.py`, report `lizrollin-runtime/vm-heroes-2026-11-09.md`): each hero's body runs on the VM, idle and then through every button, and the lobby tests' client model reads every frame.
  - All 32 run without an error, and the client reads every frame.
  - For 26 the client's states match the server's at the end. The other 6 (Baptiste, D.Va, Junkrat, Mei, Symmetra, Tracer) each differ by a state the server has on and cannot send: classes `STU_4C96470A`, `STU_43DB06B1`, `STU_B591EBFE` and the Blink effect. That is the known gap of their coverage check (noted with the first look at the VM above), where the client keeps its own prediction.
  - 26 heroes fire 56 Volleys between them. Six fire none: Brigitte, D.Va, Hanzo, Mercy, Reinhardt and Winston (melee weapons, beams, a charged bow; D.Va's mech was not looked into).
  - An ultimate ran for 16 heroes when given a full charge.
  - The VM warns of 72 kinds of things it does not compute (config vars answered as null, node classes it only passes through), many of them for every hero.
- **Ported:** `body_vm.VOLLEYS` lists each Volley with the button that fires it (the sweep's `--volleys`), and `body_vm.effect` turns a shot into what the server already has for that hero and button:
  - the primary button: `fire_at_target` with the hero's weapon, once a command frame;
  - another button: the `target.AbilityHit` the kits cast for it (`fire_ability`), or the alt-fire weapon.
  - 46 of the 56 Volleys now reach the damage code. Not played: Genji's secondary, Orisa's and Sombra's ability 2, Torbjörn's turret, and the ultimates of Pharah, Roadhog, Wrecking Ball (three Volleys) and Zarya.
- **Live** (`OW_BODY_VM=all`, Tracer against the still bot): 17 hits of 12 at 20 a second killed it, with the HUD's health, ammo, ultimate charge and ability icons up, and the kill feed line "TRACER → TRAINING BOT". Before, Tracer did no damage on the VM.
- **What a VM body still lacks:** everything the kits do outside a Volley. That is the beams (Zarya, Symmetra, Winston, Moira, Mercy), melee weapons, Hanzo's bow, healing and shields, the hero-specific code (Bastion's forms, D.Va's mech and matrix, Sombra's hack, Mercy's staff) and most ultimates. `kits.py` stays the default for that reason.
- **Not checked:** any hero but Tracer live, the abilities' hits on the VM (unit test only), and Sigma, whose two spheres are two Volleys and may count double if they fire in different frames.

## 19. The value leads, closed 2026-11-09

Section 10 left two leads: var581 (the reload's seconds) for Ashe and Sigma, and var476 for Baptiste, Sombra, Wrecking Ball and Winston.

- **What the two variables are.** var581 is read by the reload's Wait and Anim nodes. var476 is the Volley's damage amount (`WeaponVolley.m_6396149F` reads it).
- **var581: one real difference, Sigma.**
  - The match sends `target.PLAYER_KIT`'s `reload_seconds`, and that equals the graph's own number for every hero whose graph syncs var581, except Sigma (graph 1.3, ours 1.5). Now 1.3. Sigma has no magazine, so nothing visible changes.
  - Ashe's and Moira's graphs do not sync var581 at all, so nothing can be sent. Ashe's 3.55 with per-shell phases stays.
  - The other six var581 differences of the first report (Orisa, Roadhog, Sombra, Torbjörn, Tracer, Wrecking Ball) were the tool's own: `vm_shadow.py` built our frame with the default 1.5 instead of the hero's `reload_seconds`. Fixed.
- **var476: three fixed, two cannot be sent.**
  - Sombra, Baptiste and Wrecking Ball: the graph's SetVar is an expression (a mode flag picks another number; no offline mode sets it). On our bodies it gives 8, 24 and 5. `kits.VOLLEY_DAMAGE` sends those in place of the default 20.
  - Winston's and Brigitte's graphs do not sync var476 (their VM holds it as a local). Nothing to send.
  - Bastion's expression gives 20, the default.
- **Left as is:** Hanzo, Symmetra and Zarya. Their script sets var476 only when a shot is charged, so an idle body has none; we send 20. Dropping it would change what three working heroes receive for no visible gain.
- **Guard:** `test_kits` runs each kit hero's body on the vendored VM and checks var476 and var581 in our frame against what the script set (23 heroes for var581, 7 for var476).
- **Shadow report rerun** ([vm-shadow-2026-11-09.md](lizrollin-runtime/vm-shadow-2026-11-09.md)): no var476 or var581 is set to different values on the two sides any more. Differing variables 1396 (1436 before), 103 of them set on both sides.
- **Live** (kits path, Sombra against the still bot): the gun fires, each hit takes 8, the HUD and ammo count are right ([frame](lizrollin-runtime/values-sombra-2026-11-09.png)). No visible change was expected from var476 and none was seen.
- **Not checked live:** Baptiste, Wrecking Ball and Sigma.

## 20. How often bot positions go out, compared 2026-11-09

Nothing ported: ours already sends everything theirs sends, more often.

- **Theirs** (`bots.Bot.record_due`, `match.SEND_EVERY`):
  - a datagram every third tick (48 ms), as retail did by their OW2 traffic;
  - in each, a record for every walking bot and every bot that stopped under a second ago (63 ticks);
  - a resting bot: one record a second, staggered by entity id;
  - records start 8 ticks after the creates were queued (`BOT_RECORD_DELAY`), so none comes before or with its create.
- **Ours** (`Match.remote_movements`, `server.Client.send_frame`):
  - a datagram every tick (16 ms);
  - in each, a record for every living bot the client has, walking or not;
  - the first one a frame after the create was queued.
- **Measured** (`tools/research/bot_records.py`: 30 s offline through the real `send_frame`, with their rule counted beside it):

  | | Practice Range (15 bots) | Hanamura skirmish (11 hero bots) |
  |---|---|---|
  | Our datagrams a second | 62.5 | 62.5 |
  | Bytes each (largest; the client takes 1230) | 469 (1123) | 436 (1142) |
  | Our bot records a second | 936 | 686 |
  | Of those, repeating the one before | 72% | 2% |
  | Their rule's bot records a second | 117 | 227 |
  | Bots walking at a time | 3.9 | 9.6 |

  - Building the records and the frame takes about 0.13 ms of a 16 ms tick.
  - The live loop really runs at 57 to 62 ticks a second (Python's monotonic clock steps 15.6 ms on Windows). The packet frame is taken from the time, not the tick count, so the records stay on the client's clock.
- **Why nothing is taken:**
  - The resting saving is their own choice, not retail's: by their notes retail sent a standing player's body a record in every frame.
  - It would not fit as it is. Their bots never shoot, so a standing one changes nothing. Ours turn and aim while standing, so the rule would have to be "differs from the last one sent".
  - It saves bytes on the Range only (about 300 of 469 a datagram). In a Practice vs. AI match almost every record is new.
  - A datagram every third tick would also change owner corrections and statescript frames, with nothing to gain on one machine.
- **Order against the create:**
  - A record never leaves before its bot's create. `send_frame` cuts an oversize frame from the tail, and the creates are queued ahead of the records.
  - It can leave in the create's own frame, after it: once in the Hanamura run. Retail never did that by their notes. No harm has been seen live in Practice vs. AI, so it stays.
- **Guard:** `tools/research/test_bot_records.py` (a record for every bot in every datagram, none before its create, frames within the client's size).

## 21. Movement abilities and owner corrections for every hero, 2026-11-10

Section 9 gave owner corrections to Soldier: 76 only, on the belief that every other hero's movement ability would be pulled back by each record. Measured, that was wrong for the default bodies: **on a kits body the client runs no ability's movement at all.** So every hero but D.Va gets records now.

- **The course** (`mover_walk.py --course ability`, 43 s, Practice Range, `mover_record.py --observe`, ultimate charged by `OW_RECORD_ULT=1`):
  - a walk forward with each of Shift, E, a jump with the jump then held in the air, right mouse held, left mouse held, and Q;
  - a rest and a walk back between them;
  - then a crouch walk, a jump out of a crouch and a strafe each way.
- **Recorded for 31 heroes** (all but D.Va) on kits bodies. Only Soldier: 76 had records then, so the others show what the client does on its own.
- **Result:** replayed in one go by `PlayerMover` with no ability model (Soldier: 76's recorded Sprint aside), 30 heroes end 0.000 m from the client's feet and are never more than 0.042 m off flat (0.038 m for all but Genji and Tracer, who walk 6 m/s) or 0.041 m up. That is the walk course's error (sections 7 and 11): one frame of walking. The live server's copy was within 0.055 m of the client in every frame. Bastion is the exception (below).
- **Why.** A kits body's ability graphs (`kits.ABILITIES`) are sent as HUD cooldowns: the icon, the StateAbility and the cast. Their MovementMod states are never turned on, and the client does not turn them on itself. The passives' graphs are not instanced at all. So on these bodies:
  - Genji's second jump press in the air did nothing; Pharah did not hover and Mercy did not glide on a held jump;
  - Reaper's Wraith Form, Sombra's Stealth, Moira's Fade and Torbjörn's Overload gave no speed; Tracer's Blink, Genji's Swift Strike, Winston's Jump Pack and Reinhardt's Charge did not move the body;
  - a held Barrier Field, a scope and the ultimates changed no speed.
- **The VM's mods are no guide here** (`tools/research/mover_mods.py`, which replays a recording with the mods the vendored VM turns on beside the plain mover). With them the same recordings end metres off:
  - Winston up to 2.4 m in 16 of 19 stretches: Jump Pack's state (0x010A state 2) never goes off, since the VM gets no landing from the mover;
  - Mercy 2.3 m on the held jump (Angelic Descent, 0x0071 state 1); Pharah 1.4 to 1.7 m (hover, 0x0025 state 39).
  - On a VM body (`OW_BODY_VM`) the client does get the graphs' own states, movement included. So there only `body_vm.HEROES` (Soldier: 76) get records.
- **Bastion, found and fixed.**
  - Found: at Reconfigure with W held, the client walked on for the 1.0 s the transform takes (5.48 m) while the server stood still. It was as free for the 0.5 s back to Recon.
  - Cause: the body graph's no-move state (0x012F state 5: the three throttles, jump and crouch SET 0 at priority 99) sits under the switch on var2291 (state 34). `kits.bastion_body` sent var2291 = true from the transform's start but the two states only in Sentry itself, and the client takes the states as sent. `match.py` rooted him for all three phases.
  - Fix: states 5, 34 and 35 go out whenever var2291 is true. Live (`--course sentry`, records on): 0.061 m off for the 2 frames before the client has the state, then 0.000 m through both transforms and Sentry.
  - The same run before the fix shows what a record does to movement only the client makes: it held the client 0.10 to 0.21 m ahead of the server for the whole transform, moving one walking frame (0.038 m) up or down with each record. So a change of speed has to reach both sides, as Sprint's and this state do; a record alone suits a one-off move.
- **What changed** (`tools/lobby/owner_correction.py`): `HEROES = {SOLDIER}` became `OFF = {DVA}`, and a VM body gets records only for `body_vm.HEROES`.
- **Live check with records on** (`mover_record.py --observe --nudge 1.0`, the ability course; one hero for each set of mover values, `player_mover.body_values`):

  | Hero (others with the same mover values) | Off before the nudge, flat / up | Frames until the client took the nudge | Off after, flat / up |
  |---|---|---|---|
  | Reaper (and 14 more, with Soldier: 76 of section 9) | 0.038 / 0.024 m | 6 | 0.038 / 0.033 m |
  | Reinhardt (Orisa, Roadhog) | 0.038 / 0.021 m | 9 | 0.021 / 0.033 m |
  | Bastion, `--course sentry` | 0.061 / 0.003 m | 9 | 0.030 / 0.000 m |
  | Tracer (Genji) | 0.042 / 0.024 m | 11 | 0.042 / 0.026 m |
  | Doomfist | 0.038 / 0.009 m | 8 | 0.038 / 0.027 m |
  | Echo | 0.038 / 0.001 m | 9 | 0.038 / 0.000 m |
  | Lúcio | 0.038 / 0.030 m | 9 | 0.038 / 0.039 m |
  | Mercy | 0.038 / 0.036 m | 9 | 0.038 / 0.019 m |
  | Sigma | 0.038 / 0.017 m | 8 | 0.038 / 0.040 m |
  | Torbjörn | 0.038 / 0.024 m | 10 | 0.038 / 0.016 m |
  | Winston ([rec](lizrollin-runtime/abilities/nudge-winston.rec.gz)) | 0.038 / 0.006 m | 6 | 0.038 / 0.023 m |
  | Wrecking Ball | 0.038 / 0.024 m | 9 | 0.038 / 0.008 m |
  | Zenyatta | 0.038 / 0.027 m | 10 | 0.038 / 0.001 m |

  - Before the nudge the client stayed as near the server as without records. At the first crouch the server's copy was lifted 1 m: the client took that state 6 to 11 frames later, and no later frame was more than 0.05 m off, through the fall and the rest of the course.
- **Guards:**
  - `test_player_mover.py`: every hero not in `OFF` replays its recording under `research/lizrollin-runtime/abilities/` within 0.05 m (Bastion's is the sentry course with records on, within 0.07 m).
  - `test_owner_correction.py`: records for a kits hero, none for D.Va, none for a VM body outside `body_vm.HEROES`.
  - `test_roster.py`: Bastion's no-move state goes out in every phase the server roots him in.
- **What this gives:**
  - The server's copy can no longer drift from the client for these heroes: a difference is corrected within a round trip. `OW_CLIENT_SYNC` is still the only fix for D.Va.
  - The server can now move any of them and the client follows: a teleport or a launch written to `player.mover` (`PlayerMover.launch` exists for D.Va's eject). That is what a knockback, Blink or Jump Pack needs.
- **Still not modelled** (nothing here runs on either side, so nothing is wrong, only missing):
  - Every movement ability and passive listed in section 11, and knockback. None is built (the first five are, in section 23): each needs its distance or velocity from its graph (Jump Pack's and Jump Jet's are expressions on a variable, `STU_FF3471E9`; Swift Strike is a `StateMoveTo`).
  - The speed changes are the cheapest: the graphs' MovementMod states carry constants (Wraith Form SPEED +0.5 at 0x0020 state 4; Fade SPEED +2.5 and jump +6 at 0x0F7C state 19; Brigitte's E SPEED −0.5 at 0x0EDD state 12; Cryo-Freeze's no-move at 0x05FE state 3). Each needs its state sent while the ability runs and the same mod on `PlayerMover`.
  - D.Va: Boosters run on her client and ours are fitted (section 8).
  - Sprint's rules: the server runs Sprint and the client follows it, so the missing 0.3 s re-sprint delay and toggle option are rule gaps, not a mismatch.
- **Not checked:** maps other than the Practice Range; wall climb and wall ride against a wall (their graphs are not instanced, so nothing should run). The 17 heroes missing from the table were recorded without records only; each shares its mover values with a hero in it. No lobby log of these runs has an error.

## 22. Tab board, its stats and the hero switch line, 2026-11-10

The low-value leftovers of section 12: `stats.py`, `onfire.py` and `notices.py`. Their board did not open ("Tab does not open" in their WIP), and neither did ours. It does now, with live numbers, and the chat's hero switch line is ported.

- **Why Tab did nothing** (the client's own graphs, read from the install with `tools/assets/stu_graph.py`, `graph_edges.py` and `graph_messages.py`; then seen live with `tracer.asi`):
  - The HUD's host graph 041C runs on a client-made entity and starts the board's graph 01BE and the graph 0CC9 beside it.
  - In 01BE the Scoreboard key works: while it is held (logical button 77) a Stack sets var 1729, "board open" (state 15). Traced live: state 15 began and ended with the key, and the board's presenter (state 24) was on all along.
  - But 01BE also holds var 1729 off with a second Stack that has a priority (state 17), for as long as var 6699 is off. Traced live: state 17 never ended.
  - Only 0CC9 turns var 6699 on: while game message `06DF.025` is being posted (its state 3) and `13BF.025` is not (state 13).
  - `06DF.025` is posted by `STU_38EE1100` states of the mode graphs (19 of them), each a remote-synced state. A client runs a mode graph's remote states only as the server sends them, and neither server sent one of these.
- **Fix** (`tools/lobby/vendor/ow174/game/match.py`): each mode frame turns one such state on (`statescript.message_state`, one bit of data).

  | Mode | Mode graph | Remote slot | Live |
  |---|---|---|---|
  | Practice Range | 13C0 | 4 | opens ([frame](lizrollin-runtime/tab-board-range-2026-11-10.jpg)) |
  | Skirmish (0x10) | 0D76, now sent for this alone | 2 | opens on Hanamura ([frame](lizrollin-runtime/tab-board-skirmish-2026-11-10.jpg)) |
  | Assault, Escort, Hybrid | 0F4A | each phase's own: assemble 4, setup 7, attack 10, round complete 26 | opens in Assault's setup and attack ([frame](lizrollin-runtime/tab-board-assault-2026-11-10.jpg)); the other two phases and modes not run |
  | Uprising, Retribution, Snowball Offensive | 0D28, 100B, 0CE4 | 0, 0, 5 | not run |

  - The slots come from `tools/assets/board_modes.py`, which lists every mode's graph, its layout and its posting slots. `tools/assets/test_board_modes.py` checks the ones above against the install.
  - Modes with no mode frame of ours (Control, Deathmatch, Capture the Flag and the rest) still have no board. Their lines are in `board_modes.py`'s output; each needs a look in the game, since a mode graph also starts client-only HUD states.
- **More modes, later the same day.** The modes with no mode frame of ours now get their mode graph for the board alone (`match.BOARD_MODES`, from `board_modes.py`), each looked at in the game with Tab held:

  | Mode | Graph | Slot | Seen |
  |---|---|---|---|
  | Skirmish 0x10 | 0D76 | 2 | board (above) |
  | Control 0x17 | 0EC2 | 1 | board, and the mode's own capture header (0%, lock, 0%) ([frame](lizrollin-runtime/tab-board-control-2026-11-10.jpg)) |
  | Deathmatch 0x1D | 0F60 | 0 | board with the mode's own stats (Final Blows, Solo Kills) |
  | Team Deathmatch 0x1E, 0x70 | 0F73 | 0 | board |
  | Elimination 0x07 | 0CE8 | 1 | board |
  | Capture the Flag 0x09 | 0D0B | 6 | board |
  | 0x46 (its stats: Enemies Frozen, Teammates Thawed) | 1A59 | 6 | board |
  | The Range's other mode 0x5A | 13C0 | 4 | board |

  - Escort (Dorado) and Hybrid (Eichenwalde) were run too: the board opens in their attack phase, with the objective header as before.
  - Left out: 0x67, an Assault variant whose graph 29A6 also put an "UNSPECIFIED 0.0" objective header up. Not run: Lucioball 0x20, Yeti Hunt 0x29, Junkenstein 0x03 and 0x2A.
  - Uprising and Snowball Offensive cannot be opened with `run.py --map` (their loading is scripted, so only their Arcade cards host them). Their one-state change is still covered by tests only.
- **Control maps did not load at all.** `content.game_map` sorted a map's points by "objective", and a control map's three arenas name theirs by "stage": the lobby raised KeyError as the match was made, in Skirmish too. Fixed; Ilios loads in Control and the mode graph's header shows.
- **Still 0 on the board, with what was ruled out live:**
  - Each tile's ultimate charge. Not the body's var342 and var338 copied to the player entity; not component 64's field 35; not component 74's fields 1, 4 or 8 (50, 60 and 70 sent, none shown).
  - The board's time. Its text reads var2499 through the game mode entity (01BE's presenter); the Range's mode entity given var2499 = 125.0 still showed 0:00.
- **What the board shows.** The map, the mode and the difficulty; a tile per player entity of each team (bots too: Training Bots, Friendly Bots, the vs-AI heroes), a dead one crossed out; then the local player's own numbers.
- **The numbers: component 64 of the player's own entity** (`tools/lobby/board.py`).
  - LizRollin's layout (`stats.py`, `onfire.py`) is right in all but one thing: the 14 stat GUID fields are the compact id kind, not raw u64. With raw u64 the board showed three "UNKNOWN STAT" entries.
  - Checked by sending a number of its own per stat ([frame](lizrollin-runtime/tab-board-layout-2026-11-10.jpg)): fields 5 to 12 are the mode's player stats with values in 21 to 28 and medals in 37 to 44; fields 13 to 18 are the hero's own with values in 29 to 34. The client names each stat and prints it by its display type (11, 4,444, 00:33, 42%).
  - Which stats: their `data/game_stats_174.json`, vendored. For Soldier: 76 in the Range: Eliminations, Objective Kills, Objective Time, Hero Damage Done, Healing Done, Deaths; then Weapon Accuracy, Kill Streak - Best, Final Blows, Helix Rocket Kills, Tactical Visor Kills.
  - Values: what the match already counts for the Career Profile (`Player.career`), summed over the heroes played for the player stats. New: the primary's shots and hits, for Weapon Accuracy.
  - Sent in the entity's create and again when a number changes, at most twice a second. Live: a kill showed "1 ELIMINATION" with a gold medal and "35% WEAPON ACCURACY", then a death "1 DEATH".
  - Medals: gold for a counted stat above 0 (not Deaths, which the data bars). Their rule ranks teammates; bots are not ranked here.
- **The hero switch line** (their `notices.py`): "Player switched to Reaper (was Soldier: 76)." ([frame](lizrollin-runtime/hero-switch-line-2026-11-10.jpg)).
  - The PvP controller graph 0C90 posts it when its vars 4513, 4514 and 4515 (old hero, new hero, the player's entity) arrive in one controller frame: presence bits 4, 6 and 7. Sent again the client posts it again, so `Match._send_switch_notice` sends them once, after the client has every earlier controller frame, and a plain frame before the next line.
  - Only the modes whose controller is 0C90 (Skirmish, Assault, Escort, Hybrid, Control and two more). Not the first pick, not the Range.
  - The name is the entity's component 29 `m_battleTag`, now in the player's create. It also names the player's tile on the board and the label under the ultimate meter, which both read "SOLDIER: 76" before.
  - Live in Skirmish: one line, once.
- **Not ported:**
  - Joined and left lines: no second player.
  - On Fire: built later, section 24.
- **Open:**
  - Every tile's ultimate charge reads 0%, the player's own included, and the board's "TIME" stays 0:00 (what was tried: above).
  - Stats nothing counts read 0: objective kills and time, healing, and each hero's ability stats.
  - PvE and the two unrun Assault phases are covered by tests only (`test_board.py`: the frames carry the slots, read back by the oracle).
- **Guards:** `tools/lobby/test_board.py`, `test_notices.py`, `tools/assets/test_board_modes.py`. `oracle_graphs_174.json` gains 0D76.

## 23. Abilities that move the hero, first set, 2026-11-10

Section 21 left every movement ability unbuilt and gave the rule: a speed change has to reach the client as a state and the server's mover as the same mod. This builds that, generally, and the first five abilities on it.

- **The mechanism** (kits bodies; nothing changes on a VM body):
  - A kits body gets an ability's graph as an instance whose states the server turns on. Turn a MovementMod state (`STU_316CFEF2`) on and the client puts its mods on its own mover. Checked first with Wraith Form's one state alone: the client ran 8.21 m/s for exactly its 3 s.
  - `player_mover.frame_mods(instances)` reads the mods of every MovementMod state a body frame carries from the graph catalog, and `Match._body_weapon_frame` gives them to the player's mover as it builds the frame. The client takes a frame's states from its command frame + 1; the server's copy takes them from its next step. So the two change speed on the same frame, whatever the frame's delay.
  - A mod's value is its constant, or the instance variable it names: what the frame sends for a synced one, the constant the graph's own SetVar gives an unsynced one.
  - New in the mover: the three throttle mods (17 forward, 18 backward, 19 sideways), which `Mover` did not apply. All three at 0 is "cannot move" (Bastion's Sentry, Cryo-Freeze, Charge's wind-up); Charge then sets forward to 1.
  - Soldier: 76's Sprint keeps its own path (`mover.speed_scale`), which the recordings replay.
- **Reading an ability off its graph** (`tools/research/ability_states.py HERO BUTTON [--kit]`): the hero's body run offline on the vendored VM, one press, and every state of the ability's graph with the seconds it was on, its class, the codec the client reads it by and what it does. `--kit` prints them as `kits.Ability.running` takes them: (owner state, how, on, off).
  - One assumption: `STU_54FEB21C`, the first state of 29 abilities' activation with the ability's main Wait on its end plug, ends at once. The VM has no class for it, so nothing after it ran. With it, Wraith Form's Wait is the 3 s the live client showed.
  - States the kits cannot write (ChaseVar, TrackTargets) and the server's own (Pet, ModifyHealth) are listed, not sent.
- **Timed states** (`kits.Timed`, `Ability.running`): an entry may carry the seconds it is on from and until. The body's instances are built anew only when one of them changes (`Timed.due`); a frame every tick stays for an ability with a clock the client does not run (Boosters).
- **`PHASED`** (`STU_9028E035`): the state Wraith Form, Fade and Cryo-Freeze share. While a body frame has it on, the server deals the player no damage and fires none of his shots (a reload still runs).
- **The five** (tables in `kits.py`, kept equal to the tool's output by `tools/research/test_ability_states.py`):

  | Ability | Graph | Runs | Movement, from its MovementMod | While it runs |
  |---|---|---|---|---|
  | Reaper, Wraith Form | 0020 | 3 s, then 0.4 s of recovery; Shift or primary fire ends it from 0.4 s on | SPEED +0.5, no character collision | phased |
  | Moira, Fade | 0F7C | 0.75 s, then 0.2 s | SPEED +2.5, jump speed +6, no character collision | phased |
  | Mei, Cryo-Freeze | 05FE | 3.99 s, then 0.25 s; Shift or primary fire ends it from 0.2 s on | no moving, jumping or crouching | phased |
  | Reinhardt, Charge | 007D | 0.59 s of wind-up, 3 s, then 0.3 s | rooted in the wind-up; then forward whatever the input, SPEED +2, ground acceleration x0.7 and deceleration x0.4, no jump or crouch, no character collision | |
  | Sombra, Stealth | 0C9B | 0.75 s to fade out, then until Shift or primary fire | SPEED +0.65 (var7779) | |

- **Live** (Practice Range, records on; server against client, `mover_record.py --observe`):

  | Ability | Client | Server | Most apart | Seen |
  |---|---|---|---|---|
  | Wraith Form | 8.21 m/s | 8.21 m/s | 0.058 m | the shadow screen, guns hidden, the icons greyed; over after 3.0 s ([frame](lizrollin-runtime/wraith-form-2026-11-10.jpg)) |
  | Fade | 19.25 m/s at most | 19.25 m/s | 0.193 m (a frame at that speed is 0.135 m) | over after 0.94 s |
  | Cryo-Freeze | 0.00 m/s with W held | 0.00 m/s | 0.076 m, as it lets go | the two CANCEL prompts; a second one ended on Shift after 1.1 s |
  | Charge | 15.80 m/s over 2.7 s with no key held | 15.80 m/s | 0.115 m | 0.59 s rooted first |
  | Stealth | 9.14 m/s | 9.12 m/s | 0.064 m | |

  - One frame of walking apart is the error of every recording (section 21); these are that, at each ability's speed.
  - Stealth's first run showed what a missing value does. var7779 is a synced variable of 0C9B, and the kit did not send it. The client's mod added 0, the server's 0.65, and every record pulled the client along: 0.19 m behind on average, 0.11 to 0.28 m. Sent (`kits.STEALTH_SPEED`), the two agree.
  - What the client does not show: Cryo-Freeze's ice block and its camera, Charge's camera, Stealth's screen and Sombra going unseen. Each hangs on a state that is left out.

- **What the server plays itself** (added the same day; `kits.Ability.heals` and `hides`, `Player.charge`):
  - Charge bumps each bot he runs into once for 50 and knocks it along (Boosters' code with Charge's damage), and ends when a wall stops him: under 3 m/s after 0.3 s of running. Live: a charge at the spawn room's wall ended 0.95 s after the press, one down the open path ran its full 3.89 s.
  - Cryo-Freeze heals 37.5 health a second while the ice holds, in pulses of 0.2 s.
  - No bot sees or shoots Sombra once Stealth has faded her out (0.75 s), until it ends.
- **Shortcuts, each marked in the code:**
  - A timed ability ends on its own button or on `cancel_on` (primary fire for Wraith Form, Cryo-Freeze and Stealth). The client's other cancels are not followed.
  - Charge pins nobody (no 300 at the wall). Its bump's 50 and Cryo-Freeze's 37.5 are the wiki's numbers: the graphs read both from variables no graph sets.
  - Cryo-Freeze has no ice block (the graph's Pet state is the server's to play), and Sombra in Stealth is still drawn.
- **Not built** (what each needs is a state class the kits do not write and the mover does not model yet):
  - Dashes: Blink, Combat Roll and Swift Strike are built since (section 25); Guardian Angel uses `StateMoveTo`, as Swift Strike does. (`STU_75D1C308`, read here at first as the state that drives a dash's velocity, moves nothing: section 25.)
  - Launches: Jump Jet and Jump Pack are built since (section 25); Guardian Angel uses the same action, `STU_FF3471E9`.
  - Flight: Echo's and Mercy's swap the mover's values (`STU_ED2B22BD`), as Boosters do.
  - Passives (wall climb, double jump): their graphs are not instanced on a kits body. Pharah's hover and the two glides are built since (section 25).
  - Knockback: nothing knocks the player back yet. The server can now launch him (`PlayerMover.launch`) and the client follows.
- **Guards:** `tools/lobby/test_ability_moves.py`, `tools/research/test_ability_states.py`.

## 24. On Fire, 2026-11-10

The portrait burns when the player is on fire ([frame](lizrollin-runtime/on-fire-portrait-2026-11-10.jpg)).

- **What the client is sent** is LizRollin's `onfire.py` reading, on the component the Tab board made checkable (section 22): component 64's field 19 is the score, 20 its threshold, and 36 the flag that burns the portrait on every client. `board.component` writes the three.
- **The rule is the server's and not in the client.** `003F.054`'s second table, read again, is 20 keyed settings (key 19 = 90.0, key 7 = 0.04, ...), not a rule; section 6's guess at it was wrong.
- **So the rule is the community's reading of retail** ([PCGamesN, 20 June 2016](https://www.pcgamesn.com/overwatch/overwatch-on-fire-guide), after a player's guide), in `tools/lobby/onfire.py`:
  - on fire from 250 points; 5 points a second drain all the time;
  - an elimination is worth the percent of the target's health the player took. That is the number the client already prints beside "ELIMINATED" (25005 +0x88).
- **Live** (Practice Range, the still bot killed again and again): no flames after two kills, flames from the fourth, still burning after the fifth.
- **shortcut:** only eliminations score. The guide's other sources (1 point per 5 healed, 1 per 25 blocked, objectives, hero-specific ones) are not counted.
- The fire meter itself stays hidden outside the Tutorial, by their reading of the client.
- **Joined and left lines** are not ported: they are for a second player's client.
- **Guards:** `tools/lobby/test_onfire.py`.

## 25. Dashes and launches, 2026-11-10

The abilities section 23 left: the ones that move the hero by an action, not by a MovementMod state. The client plays no action of a kits body's graph, so nothing the server can turn on makes the client's own mover do these. The server moves its copy and the client follows through the owner's records (section 21).

- **Blink** (Tracer, graph 0024):
  - What moves her is the action `STU_A807207A`: `m_6A79BD98` = 7.5 (metres), `m_162A28FA` = 0.1 (seconds), along the input, or straight ahead with none.
  - The two `STU_75D1C308` states and their ChaseVars are not it. Sent to the client alone, with corrections off (`OW_CORRECTIONS=0`), she did not move at all. They are the screen's part of a blink (the field of view, most likely).
  - `PlayerMover.dash(command, metres, seconds)`, called by `Player.take_commands` on the press (`kits.Ability.dash`): each step the capsule is moved a further part of the distance through the map, on top of the step's own movement, and the step's snapshot is put where it ends. Walls stop it.
  - The velocity is left alone. A first version set it to the dash's 75 m/s; the client took that speed from the record and ran 2.4 m past the end before the next record pulled it back.
  - Live (corrections on, four presses: running forward, standing, running backward, in the air): server and client moved the same 7.52 to 9.27 m (the 7.5 plus the run), the client at most 4.2 m behind in the middle of a blink, together again 21 to 22 frames after the press, 0.00 m past the end.
  - Shortcut: the blink's own states (trail, sound, the screen's kick) are not sent, and the client arrives about 0.15 s late, by correction.
- **The client's own answer, as the reference.** On a VM body (`OW_BODY_VM=all`) the client predicts an ability itself, actions included. With owner corrections off (`OW_CORRECTIONS=0`) nothing pulls it back, so an `--observe` recording of its feet is the game's own launch. `tools/research/launch_replay.py REC HERO` replays such a recording's presses through the match code on a kits body and prints how far the server's copy is from the client. Recordings: `lizrollin-runtime/vm-jump-jet-2026-11-10.rec.gz`, `vm-jump-pack-2026-11-10.rec.gz`, `vm-combat-roll-2026-11-10.rec.gz`.
  - One trap: on the VM body the server's copy never leaves the ground, so it ends a leap 0.2 s in, and the client's first leap from the spawn point then loses its rise. Later leaps in a recording, away from where the server stands, fly their whole arc.
- **The launch action** (`STU_FF3471E9`, a list of velocity operations; what each does is read off the client's flights, not off its code):
  - `STU_9AF0FA80` (axis, value): one axis of the velocity is set. Axis 2 is the vertical, 1 the ground plane.
  - `STU_4635C5E2` (vector): added to the velocity.
  - `STU_95A10547` (vector): the velocity becomes it.
  - `STU_748FC65A` (axis, value): the speed on that axis is cut to no more than the value.
  - The client plays the action after the tick's friction and gravity and before its move: the first frame in the air has the launch's speed to the grid. `PlayerMover._throw` does the same from `_move`.
- **Jump Jet** (Pharah, graph 0027): vertical speed 0, then (10 sin yaw, 20, 10 cos yaw) added. She leaves at 20 m/s up and 10 m/s ahead on top of her run, whatever the pitch. `PlayerMover.jet`. Replayed: at most 0.08 m from the client over 1.75 s, standing and at a run.
- **Jump Pack** (Winston, graph 010A): vertical speed 0, then 30 m/s added along a direction the graph takes from `STU_0762B33A`.
  - That direction, from leaps at five pitches: the aim with 0.1 added upward, its elevation kept between 20 and 70 degrees. (Looking level, down, or up to 10 degrees: 20. Up 19.8: 25.0. Up 39.6: 43.9. Up 74.3: 69.9.)
  - While he is in the air a MovementMod state holds gravity x 1.5, half the air's deceleration, no character collision; for the first 0.2 s nothing counts as a landing. `kits.JUMP_PACK` sends those states until the server's copy lands (`Ability.lands`).
  - Winston had no Jump Pack in the kits: its cooldown has two writers (6 s, and 2 s in Primal Rage), so `ability_kit.py` printed none.
  - Replayed: to the millimetre for the 27 frames before the trap above; at most 0.13 m over a whole leap at a run. Level ground, level aim: 17.4 m.
- **Combat Roll** (Cassidy, graph 01F0): the velocity becomes 15 m/s (var1648) along the input, or straight ahead without one, with the vertical speed kept; two MovementMod states take the input and the ground's deceleration away for 0.4 s; then the ground speed is cut to 5.5. `PlayerMover.roll`, `kits.COMBAT_ROLL`. The roll fills the magazine (`Ability.reloads`). Replayed: 0.00 m from the client, standing and forward; 6.21 m in all from a standing start.
- **Swift Strike** (Genji, graph 01DB): a `StateMoveTo` to a point 15 m along the aim (or where a ray hits first) at 50 m/s. `PlayerMover.strike` is a dash along the aim, pitch included, that is the body's whole movement while it lasts; each bot on the way takes 50 once.
  - Not checked against the client: on the VM body the dash curved off to one side, most likely to a target point the server's VM sent. The 15 m and 50 m/s are the graph's numbers.
- **When the movement starts:** with the body frame that carries the ability's states (`Player.moving`, played in `Match._body_weapon_frame`), not on the press. A roll begun one step before its mods lost 1.2 m/s to the ground. A `Timed` ability's end is not a start (`cooldown.active`): the roll played again there at first.
- **Live, kits bodies, corrections on** (`tools/research/follow.py REC`):

  | Ability | The client is behind by at most | Together again | After that |
  |---|---|---|---|
  | Blink | 5.8 m | 0.17 to 0.19 s after the press | 0.00 m |
  | Jump Jet | 1.4 to 1.7 m | 0.08 s | 0.00 m through the flight |
  | Jump Pack | 1.8 m | 0.07 s | 0.00 m to the landing |
  | Combat Roll | 0.6 to 0.95 m | 0.07 s | 0.00 m; 0.15 to 0.2 m for a few frames where it ends |
  | Swift Strike | 3.9 m | 0.37 s | 0.00 m |

  A launch needs one record: the client takes the velocity from it and flies the same arc by itself. A dash is all position, so the client trails it to the end.
  Screens: [Jump Pack](lizrollin-runtime/jump-pack-2026-11-10.jpg), [Swift Strike](lizrollin-runtime/swift-strike-2026-11-10.jpg) (its trail draws: the state that carries it is among those sent).
- **Hover Jets** (Pharah, graph 0025, a passive: section 23 had these down as not instanced on a kits body). It is an ability graph like the others, on the jump button (logical button 11), so the kits instance it.
  - The graph: jump held 0.2 s, then the thrust state `STU_A9C4FF83` (a vector (0, 50, 0) and a number 2.75; no payload), a MovementMod of SPEED +0.2, and a ChaseVar that burns the fuel (var87, 100, at 50 a second). 0.2 s after the release another ChaseVar fills it at 43 a second.
  - **The client runs the thrust state of a kits body itself.** With corrections off and the state on, it rose with the server's copy to within 0.18 m over 2.8 s. So this is a state like a MovementMod, not an action: no following by records is needed.
  - The server's copy needs the thrust's rule, and the client's code for it is not read. Fitted to the client's own hover (VM body, `lizrollin-runtime/vm-hover-jets-2026-11-10.rec.gz`): each step the vertical speed gains 50 m/s2 x (1 - rise / 6.875), all of it while she is not rising and none from 6.875 m/s up (2.5 x the state's 2.75). With gravity that settles at a rise of 4.5 m/s. Other shapes tried (a cap, a quadratic fade, a target speed) were 0.2 to 0.4 m off; this one is within 4 cm over the 2 s of fuel, from the ground and on a fall.
  - `kits.Held` gains a delay before it comes on and tells when its meter starts to fill and when it is full (`Ability.delay`, `refills`); `Ability.lifts` is the push, given to `PlayerMover.lift` with the body frame that has the thrust state on.
  - Live, kits body, corrections on: the client stays within 0.05 m of the server through the hover.
  - Shortcuts: no fuel gauge on the HUD (its presenter is not found yet), and secondary fire does not hover (the graph also listens to it).
- **Angelic Descent and Glide** (Mercy's passives' graph 0071, Echo's 1DFC): each is a MovementMod state under a jump-button state, in a graph with no StateAbility. The kits send them as a `Held` ability with no meter and no ability state (`Ability.state` None).
  - Mercy: terminal velocity 2 m/s while jump is held. Live, corrections off: a held jump lasts 0.88 s against a tap's 0.65 s and falls at 1.95 m/s, on the client and on the server's copy alike (at most 0.04 m apart).
  - Echo: terminal velocity 4 m/s and SPEED +0.5, only while she falls with no ground within 2.5 m under a point 0.25 m above her feet (the graph's RayCast; `Ability.falling`, `Match.falling`). Live only with that rule off, on a plain jump (the Range has no drop that deep near the spawn): the client took the states and fell with the server's copy, at most 0.06 m apart.
- **Shortcuts, each marked in the code:** Jump Jet's own states (jets, animation) are not sent; Jump Pack does no damage where he lands and its cooldown runs from the landing; Swift Strike goes straight to the first wall and does not reset on an elimination; its 50 damage is the wiki's number.
- **What is left of movement, 2026-11-10** (the list is also STATUS.md's item 10):
  - Built since from the graphs' own numbers (section 29): Genji's double jump and Hanzo's lunge. On a VM body the client does not predict them (a second jump press in the air changes nothing), so there is no reference flight yet. Their graphs (0157, 010C) launch with `STU_9AF0FA80` on axis 3 (1.5 for Genji; Lúcio's wall jump 1.15, Sound Barrier 1), whose meaning is not known: a jump of that many jump speeds is the guess.
  - Wall climb (0157's and 010C's `STU_92ED01D0` with a MovementMod) and Lúcio's wall ride (0263) need the server's copy to find the wall.
  - Echo's Flight (1BB6): built since, section 26.
  - Teleports (Recall, Shadow Step, Translocator, Teleporter), the kicks of the hero's own explosions (Coach Gun, Junkrat's mine, Helix, Concussive Blast), Doomfist, Wrecking Ball, Widowmaker's grapple, Shield Bash, Exo Boots, Guardian Angel (needs an ally), Primal Rage's leap: not looked at beyond a survey of their graphs' movement nodes (`tools/research/body_moves.py HERO`, and `body_graphs.py HERO GRAPH` for one graph's nodes).
- **Guards:** `DashTests`, `LaunchTests`, `HoverTests` and `GlideTests` in `tools/lobby/test_ability_moves.py`; `tools/research/test_launch_replay.py` (the four recordings); `test_ability_states.py` (the three new tables).

## 26. Flights along the aim: Echo's Flight, 2026-11-10

Flight (graph 1BB6) is a launch the server plays and a state the client runs itself. Checked against seven of the client's own flights on a VM body (`lizrollin-runtime/vm-flight*.rec.gz`).

- **The launch** (action `STU_FF3471E9`, node 26): the vertical speed is dropped, then 28 m/s is added along var27915.
  - var27915 is the input in the aim's own frame, pitch included: forward along the aim, right along the level right-hand axis. With no input it is the aim.
  - With jump held, (0, 1, 0) is added and the vertical part kept to 1: level aim and jump give 28 m/s ahead and 28 m/s up.
  - The launch adds to the run she has: W held gives 33.3 m/s.
- **The flight** (MovementMod state 6, on for 3.15 s): gravity 0, a mover swap, and the flag `m_485109CB` that D.Va's Boosters have too.
  - The swap (`STU_ED2B22BD`) carries a whole `STUCharacterMoverComponent`: run speeds 8 / 8 / 8, air acceleration 5, air deceleration base 3 (from speed 9 on, 0.35 a metre a second more). `player_mover.SWAP_FIELDS` maps its fields onto `MoverData`.
  - The flag makes the mover fly along the aim. Read off the recordings:
    - The wish is the input along the aim in 3D, plus up for jump and down for crouch, at the run speed (8 m/s).
    - The air's deceleration acts on the whole velocity, not the level part: a launch aimed 30 degrees up slows along its own line.
    - She does not land while it is on (a level launch along the floor slows at the air's rate to the end).
    - Touching a wall does not change the acceleration: she slides along it at full speed. (The walking mover drops to the touching acceleration, 0.5; D.Va's fitted touching acceleration equal to her air acceleration was this.)
  - State 23, the first 0.15 s: the three throttles and the jump and crouch intents at 0.
- **The speed limit** (state 24, 0.15 s to 0.24 s): a MINIMUM on mod 32 with var28952, which a ChaseVar runs from 30 down to 8 in 0.1 s.
  - Mod 32 is the most the speed (3D) may be: the client's flights lose speed along that line (13.95, then 12.42 m/s) in the frames where it is under their speed.
  - Which frames it bites in varies with the client's own statescript ticks: one flight was cut to 14.0, two to 12.42.
  - The kits send state 24 from 0.23 s to 0.35 s with var28952 at 12.42, where the line is when the state ends. The server's copy takes the same mod (`PlayerMover.limit`).
- **Numbers, replayed through the match code on a kits body** (`tools/research/launch_replay.py`, 440 frames each):

  | Flight | Server off the client's own |
  |---|---|
  | No input, level: 7.80 m to a stop | 0.02 m |
  | W held, level: 28.7 m | 0.10 m |
  | W held, aimed 30 degrees up, into a wall and up along it | 1.14 m |
  | Jump held: straight up to the ceiling, then along it | 0.05 m |
  | D held at the press | 0.19 m |
  | S held | 0.29 m |
  | No input, aimed 30 degrees up (the client's limit cut off a frame early) | 0.46 m |
  | Jump at the press, then crouch held (jump bit put back at the press frame) | 0.11 m |

- **Live, kits body, corrections on:** the client is up to 1.5 m behind for 0.06 s (the launch reaches it with the owner's next record), then within 0.05 m to the end, for no input, W held and jump held.
- **Shortcuts:**
  - A press of jump let go in the very frame Shift goes down still launched the client upward; the server reads the press frame's own jump bit.
  - A second press does not end the flight (the graph has a button state for it from 0.2 s; not read).
  - The end's cap of the speed to 8 m/s (`STU_748FC65A`, node 8) is not played: after the limit she is never faster.
  - D.Va's Boosters keep their own fitted path (`PlayerMover.flight`); her graph is not in the catalog, so `frame_flight` reads nothing for her.
- **Guards:** `FlightTests` in `tools/lobby/test_ability_moves.py`; three tests in `tools/research/test_launch_replay.py`; the table in `test_ability_states.py`.

## 27. Launches some time after the press: Coach Gun, Rising Uppercut, 2026-11-10

Both are launch actions behind a Wait, so the server plays them late (`PlayerMover._throw(velocity, after, level, rise)`; a Wait of 0.165 s ends in the client's 24th tick of 7 ms). Checked against the client's own on a VM body (`lizrollin-runtime/vm-coach-gun-` and `vm-rising-uppercut-2026-11-10.rec.gz`).

- **Two more launch ops, read off the flights:**
  - `STU_1308AA6A(axis, value)` multiplies the speed on the axis (1 = ground plane, 2 = vertical).
  - `STU_4635C5E2` with `m_CFEC5F76` 2 (Coach Gun) sets the velocity; with 1 (every other launch) it adds.
- **`STU_0762B33A`, the launch direction of Jump Pack, Coach Gun and the uppercut:** a direction (`m_32B00625`) kept within `m_E26E5E09` degrees of a reference (`m_3661BC45`).
  - Jump Pack: the aim + 0.1 up, within 25 degrees of facing-and-up (45 degrees): 20 to 70 degrees of elevation, as measured in section 25.
  - Rising Uppercut: within 0 degrees of the facing + 0.3 up: always 16.7 degrees up, whatever the aim.
  - Coach Gun: away from the aim + 0.1 up, within 35 degrees of a reference whose function (`STU_B113D362`) is not read. Measured: never flatter than 20 degrees, and 85 degrees when she aims 85 degrees down, so the cone is 20 to 90 degrees.
- **Coach Gun** (graph 1513): 0.165 s after the press the velocity becomes 18 m/s along that direction, taken from the aim at that moment. From then to 0.55 s a MovementMod state (14) cuts the ground's deceleration to 1 % and its acceleration to 10 %.
  - The server against the client's own: 0.02 m (level), 0.00 m (aimed 44 degrees up), 0.09 m and 0.12 m beside a wall (aimed 45 and 85 degrees down).
  - Live, kits body: the client is up to 1.3 m off right after the kick and within 0.05 m 0.4 s after the press.
  - The shot's own hit (`ABILITY_HITS`) now fires at the ability's start only: a `Timed` ability also reports its end.
- **Rising Uppercut** (graph 0DD8), three launches:
  1. At once: the velocity becomes 10 m/s along the facing lifted 0.3.
  2. 0.15 s: the vertical speed is dropped, a fifth of the level speed kept, 1 m/s ahead and 40 m/s up added.
  3. 0.25 s: a fifth of the vertical speed is kept.
  - MovementMod states: no input, jump or character collision to 0.59 s; from 0.24 s to 0.99 s the terminal velocity is 1 m/s (he hangs).
  - The server against the client's own: 0.03 m over 2.3 s standing; the same at a run until both slide down a wall.
  - Live, kits body: each launch reaches the client with the owner's next record (2.8 m off at most, on the rise), together again 0.1 s later.
  - Shortcuts: it hits and lifts nobody; its cooldown starts when the slow fall ends.
- **What the client does not predict on a VM body** (so there is no reference flight for these):
  - Rocket Punch and Seismic Slam: after the first right mouse press the client stood frozen to the end of the recording (not even a jump moved it). The server's VM copy of a state must be stuck with its no-input mod on.
  - Shadow Step and Recall: the client stayed where it was.
  - Exo Boots: a crouch of 1.2 s then jump gave a plain 6 m/s jump (var154, the charge, never rose).
  - The kick of the hero's own explosion (Concussion Mine, Concussive Blast, a rocket at the feet): none.
- **Guards:** `TimedLaunchTests` in `tools/lobby/test_ability_moves.py`; two tests in `tools/research/test_launch_replay.py`; the two tables in `test_ability_states.py`.

## 28. Roll, and two checks that were open, 2026-11-10

- **Wrecking Ball's Roll** (graph 1203, a toggle with no cooldown): its MovementMod state (1) swaps in the ball's mover values (run 10 / 10 / 10, ground acceleration 4, ground deceleration base 1) and takes crouch away. `frame_flight` (section 26) reads the swap, so the kit entry is the state table and nothing else.
  - Live, kits body: he rolls at 10 m/s on both sides, at most 0.04 m apart; a second Shift puts him back on his feet.
  - The tool leaves three of its states out as classes it does not know; all three have no data, and sent as they are (`kits.ROLL`) they give the third-person camera (`lizrollin-runtime/roll-2026-11-10.jpg`).
  - shortcut: the model stays the walker with its guns out. The ball's own pose is not sent (which state carries it is not known).
- **Glide's height rule, live** (section 25 had it only with the rule off): Echo launched upward with Flight (7.8 m up), fell when it ended, and with jump held fell at 4.1 m/s on both sides, 0.00 m apart, until about 2.5 m over the floor, then at the plain fall's speed.
- **Swift Strike against the client's own:** on a VM body the client's dash curves toward a point the server's copy worked out from where *it* stands, so the direction is no reference. The time is: all three dashes took 57 frames (0.4 s) at an even speed, whatever the distance to that point (12.5, 13.6 and 20.2 m). The kits' dash was 15 m at 50 m/s (0.3 s); it is 15 m in 0.4 s now.
  - Live, kits body: the server went 15.15 m, the client was together with it 0.48 s after the press.
- **Looked at and left:**
  - Recall: its rewind is a state class of its own (`STU_41B3FC2B`, with data: codec time16 + s64). The vendored VM has no class for it, so on a VM body it never ends, and its data is not decoded.
  - Knockback from the hero's own explosions: the numbers are in each volley's explosion config (`STU_4AA6CA44`: rings, `m_8A3E3883` the share of damage the owner takes, `m_792F0B4D` 50 for Concussive Blast and 30 for a rocket, `m_2FE97560` 1 and 1.875). Which of them is the push and in what unit is not known, and the client predicts none of it, so there is nothing to fit against.
  - The Hover Jets' fuel gauge: graph 0025's client-only SubScript (state 35, from an Entry) runs the meter graph 214F, whose presenter parents to entity var11821 and reads message 0x104E, which 0025's client-only state 36 posts with var87 / var971. Three live tries, none drew a gauge: var11821 = `ScriptRef(116, 5)` with graph 0074 instanced (its HUD presenter, as Widowmaker's 0076); that plus a 214F child instance under (0025, 35), as D.Va's 214E; that plus both graphs' `STU_9D7BF987` states on. Reverted. Not tried: whether 0074's presenter runs at all (its switch asks whether the owner is the local player).
  - Exo Boots (graph 1983): the charge var154 rises 0.1428 every 0.1 s and the jump speed is multiplied by 1 + 2 x var154 (18 m/s at full charge). What starts the charge (`STU_9D240539`) is not read.

## 29. A press of jump in the air: Genji's second jump, Hanzo's Lunge, 2026-11-10

Section 25 left these as stuck: on a VM body the client predicts neither. Both graphs spell the launch out, so the server plays it from the graph's numbers and the client follows by the owner's records (`kits.AIR_JUMPS`, `Player.air_jump`).

- **Likely why the client does not predict them on a VM body:** both graphs gate the press behind a switch on entity var417 and a 0.1 s Wait. If var417 is "in the air" (not confirmed), the server's VM copy, which never leaves the ground, never opens it.
- **Hanzo's Lunge** (graph 010C, node 82): every axis of the velocity is set to 0, then (x, 0.3, z) x 15 is added, x and z being the input in the facing's frame (straight ahead without one). Cooldown 5 s.
  - That is 15 m/s level and 4.5 m/s up. From the top of a jump the server's copy goes 7.5 m before it lands.
- **Genji's second jump** (graph 0157, node 2): `STU_9AF0FA80` on axis 3 with 1.5, once each time in the air.
  - Axis 3 is taken to be "a jump, times the value": the vertical speed becomes 1.5 x 6 = 9 m/s. Not checked against the client. The other users of axis 3 fit that reading: Sound Barrier 1, Lucio's wall jump 1.15, Translocator's arrival 1.25.
  - Tried for a check: Sound Barrier on a VM body (no hop: the ultimate did not start), and the type's code in IDA (the launch's ops are not dispatched by their type objects or hash constants; not found in the time given).
- **Live, kits bodies:** the client is 0.55 m (second jump) and 0.97 m (Lunge) off for 0.05 s after the press, then with the server's copy.
- **Shortcuts:** no instance is sent for either, so there is no animation or sound and the Lunge's icon does not count down; a wall climb does not give Genji his jump back (there is no wall climb).
- **More reference recordings, for what is left** (`lizrollin-runtime/`):
  - `vm-seismic-slam-2026-11-10.rec.gz`: Seismic Slam alone *is* predicted on a VM body (from the ground: a hop of 4.4 m/s up and 14.6 m/s ahead, 0.59 s). After a Rocket Punch the client stood frozen, which is why section 27 saw nothing. Its graph (0C51) runs on mods 32 and 34 and a mover swap; mod 34 is not known.
  - `vm-wall-ride-2026-11-10.rec.gz`: Lucio, W and jump held at a wall: he rides up it at 2.93 m/s. The client runs the wall ride itself there; on a kits body the server would have to find the wall and turn the state on.
  - Translocator: the client does not move.
- **Guards:** `AirJumpTests` in `tools/lobby/test_ability_moves.py`.

## 30. The mine jump, 2026-11-10

Concussion Mine throws its own Junkrat with a launch in his body graph (096A, node 26), not with the explosion's physics: on the mine's message the velocity gets 15 m/s added straight away from the mine on the level (var2083) and 15 m/s upward.

- The server plays it where its mine goes off (`AbilityHit.kick`, `Match._land_rockets`, `PlayerMover.blast`), for a Junkrat within the mine's 3 m.
- Live, kits body: looking down, Shift: the mine lands at his feet and he goes up 7.4 m; the client is with the server's copy but for the record that carries the launch (1.3 m off at most).
- shortcut, as before this: the mine goes off where it lands, not on the right mouse button.
- The other heroes' own explosions (rockets, Concussive Blast, Helix) have no such launch in their graphs: theirs is the explosion's push (section 28), still not built.
- Guard: `MineJumpTests` in `tools/lobby/test_ability_moves.py`.

## 31. Healing Done, and fire from healing, 2026-11-10

- `Match._heal` takes the player whose healing it is. The health it really gave back counts as his `healing` (the board's Healing Done, stat 0x1D1, a medal stat of every mode) and fills his fire at 1 point per 5 health (`onfire.HEALTH_A_POINT`, the same guide's number as the eliminations').
- Counted: Soldier: 76's Biotic Field (on himself and on allies), Mercy's staff, Moira's grasp and orb, Bastion's repair. A bot's healing is nobody's.
- Live: Soldier hurt himself with a Helix rocket and stood in his field: the board shows 39 Healing Done with its gold medal ([frame](lizrollin-runtime/tab-board-healing-2026-11-10.jpg)).
- Still not counted: objective kills and time, damage blocked, the hero abilities' own stats. The Career Profile does not take healing yet (its per-hero stat id is not known; `career.TOTALS` skips the name).
- Guard: `test_healing_done_is_the_health_a_players_healing_gave_back` in `tools/lobby/test_board.py`.

## 32. Recall, on the server's side, 2026-11-10

Recall's rewind is a state class of its own (`STU_41B3FC2B`, section 28) whose data is not decoded, so the client's own rewind is not started. The server does the part that matters to play:

- `Player.trail` keeps the last 3 s of Tracer's feet and health (`kits.Ability.rewinds`).
- For the ability's 1.25 s (the wiki's length; the graph's own was not read, its VM run sticks on the rewind state) the server's copy goes back along that path, evenly, and nothing can hurt her (the graph's `STU_9028E035` state, sent with the rest of `kits.RECALL`).
- At the end she stands where she stood 3 s before the press, with the most health she had on the path and a full magazine.
- Live, kits body: 14.9 m walked in 2.5 s, E: the server's copy went back to the start in 1.25 s and the client followed 0.8 to 0.9 m behind, together at the end.
- Shortcuts: she is drawn sliding back, in first person, with none of the rewind's look or sound; the three charges of Blink are not given back (retail does not either).
- Guard: `RecallTests` in `tools/lobby/test_ability_moves.py`.

## 33. Exo Boots, 2026-11-10

Baptiste's passive (graph 1983) is all states, so the client runs it itself once the frame carries its variables. Section 28 could not see it on a VM body (the charge never rose there); the graph's tree gives the rule.

- **The graph:** under its enabling switch (owner state 19) a MovementMod state (5) is always on: jump speed x (1 + var154 x (var176 - 1)), with var176 = 3. var154 is the charge:
  - while he is crouched (var6867, written by the watcher state `STU_96F1DBCF`) it rises by 0.1428 at once and again every 0.1 s, to 1: full after 0.6 s;
  - a jump with charge spends it (the watcher's jump plug sets it to 0);
  - 1 s after he stands up without jumping it is set to 0.
- **The kit:** `kits.Crouched` keeps the charge on the server; the ability's instance sends states 19 and 5 always, var176 and the charge (`Ability.crouch`). A frame goes out each time the charge changes.
- **The server's copy** takes the same mod: `player_mover._number` now works a mod's expression out when every variable in it is one the frame sends (`_Sent`). An expression with an unsent variable or a world query still gives no mod.
- **Live, kits body:** a 1.2 s crouch and jump rose 8.13 m (to the spawn room's ceiling), a plain jump 0.97 m, a 0.3 s crouch and jump 4.6 m; client and server 0.00 to 0.13 m apart, with no correction needed: the client predicts the charged jump itself.
- Shortcuts: the crouch button stands for the crouched stance; none of the charge's effects, sounds or its HUD mark are sent.
- Guard: `ExoBootsTests` in `tools/lobby/test_ability_moves.py`.

## 34. Movement still open, with what is known of each, 2026-11-10

Read off the graphs (`tools/research/graph_tree.py`, `body_moves.py`) and the VM-body recordings; none of this is built.

- **Shadow Step** (Reaper, graph 00F2): built since, section 35.
- **Translocator** (Sombra, 0B31 / 0C19): the client does not move on a VM body. Its arrival hop is `STU_9AF0FA80` on axis 3 with 1.25.
- **Guardian Angel** (Mercy, 006E; built since, section 41): a StateMoveTo to a point by the ally (var431: the ally's position, 0.5 m up, 2.5 to 3.7 m short of him) at var1002 = 17 m/s (25 in Valkyrie), targets within 30 m, cooldown 1.5 s. Its end launches use ops not met elsewhere (`STU_95A10547`: the velocity with 5 m/s more upward; `STU_EC4841E7` with 14 and 22). Needs an ally (`ally <hero>` bots exist) and the lock the staff already has.
- **Seismic Slam** (Doomfist, 0C51): predicted by the client on a VM body when nothing has frozen it (`vm-seismic-slam-2026-11-10.rec.gz`). On E the command bit is 0x10, yet the graph waits on logical button 37, which `ability_states.py` reaches with no bit that starts it: his binding is not the table's. Mods 32 (speed limit) and 34 (not known: 40 and -60 here, -98 in Piledriver) and a mover swap.
- **Rocket Punch** (0D06): a charge on the right mouse button, then a MovementMod state with forward forced, SPEED set to 8.18 (45 m/s) and a mod class not met elsewhere (`STU_09F9B1FE`, the punch's direction); it ends with caps to 6 m/s. On a VM body it froze the client.
- **Piledriver and Grappling Claw** (Wrecking Ball, 160A and 14E4): Piledriver is mod 34 at -98 with a landing launch of 5 m/s ahead and 5 up; the claw's mods are expressions of world queries.
- **Widowmaker's Grappling Hook** (005A): a StateMoveTo at 20 m/s to the hook's point or entity.
- **Wall climb** (Genji 0157, Hanzo 010C: state `STU_92ED01D0`, 7.5 in its fields, with a MovementMod) and **wall ride** (Lucio 0263): the client runs the wall ride itself on a VM body (`vm-wall-ride-2026-11-10.rec.gz`: up the wall at 2.93 m/s). On a kits body the server would have to find the wall, turn the state on and move its copy the same way.
- **Shield Bash** (Brigitte, 0EE3, logical button 20 with the shield up): a MovementMod state (forward forced, SPEED +5, no jump or collision) and a cap to 1 m/s at its end. The barrier itself is not built.
- **Knockback** from the hero's own rockets and blasts: section 28.

## 35. Shadow Step, 2026-11-10

Built from the graph's tree (00F2, `tools/research/graph_tree.py`): on the vendored VM a press of E turns none of its states on (its first step is a transition of class `STU_7DEB522D` on logical button 36), so `ability_states.py` gives no table and the two in `kits.py` are written by hand.

- **The flow, as the graph has it:**
  1. E opens the aim: the targeting state (owner state 0, `STU_4C96470A`) under a LogicalButton state on primary fire (2), with its marker (4). The targeting state writes the destination (var985) and has a "good" and a "bad" plug.
  2. A click on a good place starts the cast, a Wait of var1722 + var1723 = 1.5 + 0.5 s: no input (MovementMod 8), the cast's camera (50, the class Roll uses), from 1.0 s nothing can hurt him (33).
  3. At 1.5 s the action `STU_CFCE55DC` moves him, then 0.5 s more of `STU_9028E035` (47).
  4. The cooldown, 10 s, starts when the Wait ends.
- **What the client does by itself once the states are sent** (live, kits body):
  - the aim draws its marker, a shade of Reaper on the floor where the crosshair points ([frame](lizrollin-runtime/shadow-step-aim-2026-11-10.jpg)). The targeting state's one byte of data is sent as 0;
  - the cast goes to third person and he sinks into the floor ([frame](lizrollin-runtime/shadow-step-cast-2026-11-10.jpg)).
- **The server's part** (`kits.Aimed`, `Ability.aiming` and `steps`, `Match.step_point`):
  - the press opens the aim; primary fire starts it if there is a place to go; E again or secondary fire puts it away. No shot leaves the weapon meanwhile (a reload still runs);
  - the destination is the floor the aim meets within 35 m, or the floor under a point 0.6 m short of the wall it meets (under the aim's end when it meets nothing). It goes to the client as var985 (a vector: `statescript.Vec3`, tag 12);
  - 1.5 s after the click the server puts its copy there; the client is with it 0.08 s later (11 frames, then 0.17 m, then 0).
- **Live:** 7.4 m to a spot on the spawn room's floor; 26.3 m through the door.
- **Shortcuts:**
  - The destination was the server's own ray on the line-of-sight mesh, and the far step ended against a wall 2 m higher than he started. Replaced since: section 36.
  - Left out of the cast: the entity the graph creates at the destination (owner state 42), its linked-target state (49) and its messages (62).
  - The mod on index 27 (ADD -0.7, state 46; also in Recall and Translocator) is sent but not known; the server's copy does nothing with it.
- **Guards:** `ShadowStepTests` in `tools/lobby/test_ability_moves.py`.

## 36. Shadow Step lands on the client's marker, 2026-11-10

Section 35's landing spot was the server's own ray, and it could be far off the marker. Two things changed: the client's own marker now reaches the lobby, and the server's rule is the client's targeting query as far as it could be read.

- **Where the client keeps the marker.** The targeting state's class (`STU_4C96470A`) has vtable `img+0x261E920` (LizRollin's codec table gives the address in their image; ours is the same RVA). Its tick (`0x7FF6D1B08B40`, slot 16) calls the placement query `0x7FF6D1EAEB30`, which writes the place to the state item at +0x350 (x, y, z) and returns whether it is good, kept at +0x374.
- **The feed.** `tools/asi/client_feed.c` finds those items by their vtable (on a thread of its own) and, while one is active, sends each frame's place and its good flag beside the feet (`client_sync.MARKER`). `mover_record.py` writes them as `M` rows.
- **With the feed on, the click goes to the marker** (`client_sync.marker`, first thing in `Match.step_point`): the place when it is good, no cast when it is not. Live: two clicks landed 0.00 m and 0.01 m from the marker. The far aim that used to end against a wall 26 m on now lands 17 m on, where the marker was.
- **The query, as far as it was read** (headless IDA dump of the two obfuscated functions; Hex-Rays gives up on both):
  1. From the eye (1.55 m over the feet) along the aim for the node's 35 m.
  2. A sphere of 0.1 + 0.015 m is swept along that line. Where it touches, the contact's normal decides: a floor (normal's height over 0.71934) puts the first candidate 0.5 m above the contact, anything else 0.5 m off the surface along its normal. The second candidate is 0.5 m back along the aim.
  3. Each candidate must be reachable by the sphere (radius less 0.015) from where it touched; the first that is goes on, with the other as its fallback.
  4. A last function (`0x7FF6D1EC0E50`, not read) settles the place: from the recorded markers, the floor under the candidate, if there is room to stand there.
  5. When that fails the query tries again with the aim turned (up to 10 times; section 38: only for a place over a height limit or too near, which Reaper's node does not set).
- **The server's rule without the feed** is steps 1 to 4 on the mover's own collision (`contacts.scene_sweep`, the engine port the mover already uses), with the room-to-stand check as a sweep of the hero's own capsule. `tools/research/marker_compare.py REC` puts it beside the client's markers of a recording:

  | Recording | Aims | Place on both sides | Within 2 cm | Within 0.25 m | Good / no place disagree |
  |---|---|---|---|---|---|
  | The Range's yard (`shadow-step-marker-outside-2026-11-10.rec.gz`) | 57 | 46 | 42 | 43 | 1 |
  | The spawn room, among its desks (`shadow-step-marker-room-2026-11-10.rec.gz`) | 88 | 77 | 62 | 70 | 6 |

  The first rule (a ray on the line-of-sight mesh, section 35) had 53 of 80 within 0.25 m in the room and was up to 12 m off.
- **Where the rule is still off** (as of this section; section 37 reads step 4 and closes the first two):
  - An aim that ends in the air past a ledge: the server takes the ground under its end (18 m off in the one such aim); the client's marker stays up on the ledge. That is step 5 or the inside of step 4.
  - Two aims at a desk in the room: the client's sweep stops at something 3 m on that the mover's collision does not have (the query's filter is not the mover's categories), and the marker sits on the desk.
  - A handful 0.4 to 0.6 m off beside furniture.
- **Guards:** `ShadowStepTests` in `tools/lobby/test_ability_moves.py` (the feed's marker is used; the sphere's place on a flat floor); `tools/research/test_marker_compare.py` (the two recordings); the marker cases in `tools/asi/test_client_feed.c`.

## 37. The targeting query's last function, 2026-11-10

Section 36 left the query's last step (`0x7FF6D1EC0E50`) unread and the server's rule off past ledges and beside furniture. Most of it is read now; the rule lives in `tools/lobby/shadow_step.py`.

- **How it was read.** IDA leaves 1.7 KB out of the function (its obfuscated lookups break the analysis), and that is where the work is done. `ida/Overwatch-runtime.dump` has file offset = RVA, so `tools/research/dump_asm.py START END` disassembles any stretch with capstone, floats and call targets filled in.
- **What the function does:**
  1. From each start (section 36, step 2) it works back toward the eye and calls a probe (`0x7FF6D1EBE800`) for each point. (Read here as every 0.5 m from both starts; section 38 corrects it: two rays under the first start, the line from the second in stretches of 3 m.)
  2. The probe looks for a place straight under the point, no deeper than a small helper says (`0x7FF6D1EBC760`, 30 instructions, read whole):
     - `angle = pi - a - (1 - aim.y) * pi / 2`, with `a` the node's 0.1745 rad (10 degrees);
     - `depth = min(most, along * sin(a) / sin(angle))`, `along` being the point's distance from the eye, and the probe adds 0.5.
     - That is the law of sines: a place has to lie inside a cone of 10 degrees under the aim's line, seen from the eye. For a level aim it is `along * tan(10 degrees) + 0.5`.
  3. Every place a probe accepts goes into a list with its step's number.
  4. The list is projected through a camera at the eye (`0x7FF6D2C93E70` and its neighbours; section 39: it looks at the first start; section 40: from a metre behind the eye), and the place whose projected height is nearest the first start's wins. A strict less-than keeps the first of equals, and the far points come first.
- **What that explains in the recorded markers:**
  - The aim over the ramp (18 m off in section 36): the ground under the aim's end is in the cone, but the last point of the upper floor looks nearer the crosshair. Now 0.00 m off.
  - A level aim at a wall 4 m away shows no place: the floor there is 1.55 m under the line and the cone is 1.1 m deep. The same aim 12 degrees down has one. All three such aims in the room come out as the client has them.
  - The marker on a desk for a level aim at the wall behind it: the desk top is in the cone, the floor is not.
  - Aims above the horizon find a place only on something high (`shadow-step-marker-up-2026-11-10.rec.gz`: 37 aims, 31 with no place on either side).
- **Against the recorded markers** (`tools/research/marker_compare.py`; before / after this section):

  | Recording | Aims | Good or no place disagree | Within 0.25 m | Most |
  |---|---|---|---|---|
  | The yard | 57 | 1 / 1 | 43 / 44 of 47 | 18.19 m / 0.49 m |
  | The spawn room | 88 | 6 / 2 | 70 / 71 of 79 | 3.08 m / 3.03 m |
  | Aims level and upward | 37 | not run / 0 | 3 of 6 | 1.13 m |

- **Still not read, and what it costs:**
  - The probe's own checks of a place (about 700 instructions with two more physics queries). The port takes the first floor under the point with room for a capsule over it. The capsule's radius is fitted (0.4 m: 112 of the 182 aims within 2 cm; the hero's own 0.5 m: 105).
  - The most a place may lie under its point (the probe's argument; the node's 3.0 is used).
  - The query's retries with the aim turned when it finds nothing.
  - Section 38 reads all three.
  - What is left off: 14 of the 132 aims with a place on both sides are more than 0.25 m apart. Most are one step (0.4 to 0.6 m) beside furniture; one is 3 m (the desk again, at 6 degrees down: the client still stands him on it); on the ramp and on high ledges up to 1.1 m.
- **Guards:** `tools/research/test_marker_compare.py` (the three recordings); `ShadowStepTests` in `tools/lobby/test_ability_moves.py` (the cone's depth, the far point of a level aim).

## 38. The probe and its checks, 2026-11-10

Section 37 left the probe (`0x7FF6D1EBE800`) unread. It is read now, with the functions it calls, and reading it showed two things section 37 had wrong. The rule in `tools/lobby/shadow_step.py` is rewritten to it.

- **What section 37 had wrong:**
  - The line is not tried every 0.5 m from both starts. The first start gets two rays, the second a line let down in stretches (below).
  - The node's 3 is the length of a stretch, not the most a place may lie under its point. That is the aim's own length (the node's 35 m).
- **The node's fields by their place in the object.** The class registers each field with its hash and offset (the code around `0x7FF6D272D6E6`: `mov r8d, hash` / `mov [rsp+0x20], offset`, one pair per field; the table the registration leaves in memory holds each field's end, 8 more):

  | Offset | Field | Reaper's value | What the query does with it |
  |---|---|---|---|
  | 0xF0 | `m_B3094B0E` | 0.5 | how far off the touched surface a start is put; the distance between the first start's two rays; added to every depth |
  | 0xF8 | `m_B42B2BEB` | 3 | the length of a stretch of the line from the second start |
  | 0x100 | `m_15F880A4` | 0.75 | the lower height over a place the eye has to see |
  | 0x108 | `m_00A23E4B` | 0 | the nearest a place may be to the eye |
  | 0x110 | `m_6386E7B4` | 35 | the reach |
  | 0x120 | `m_2ADABA5F` | 0.1745 | the cone's angle |
  | 0x128 | `m_3656A811` | 0.1 | the swept sphere's radius (0.015 more) |
  | 0x138 | `m_33C8E790` | 0.8 | the width of the room over a place (a sphere of 0.4) |
  | 0x140 | `m_60B6AF94` | 1.5 | the room's height; half of it and 0.1 is the upper height the eye has to see |

  0x130, 0x148, 0x160, 0x168 and 0x178 are read too (a switch in the floor check, a narrower filter, a height limit and its height, a condition per place); Reaper's node sets none of them.
- **The search** (`0x7FF6D1EC0E50`), as it is:
  1. Under the first start and under the point 0.5 m nearer the eye: a ray straight down (two at most).
  2. The line from the second start to the eye is cut into `round(length / 3)` equal stretches. Each is a shape of two points, its own and the next one up the line, as thick as the sphere (0.1 after the sphere touched something, 0.115 when it touched nothing), cast straight down. The first point of an upward aim has no point above it and gets a ray.
  3. Both go down `depth + 0.5`, the depth being section 37's cone with the aim's length as its most.
  4. The state item keeps a stride, a start and the last winner (+0x36C, +0x368, +0x370; the tick `0x7FF6D1B08B40` passes them in and moves the start on by one): the search can be spread over several ticks. The constructor (the code at `0x7FF6D1B10D0B`) sets the stride to 1 and no write to it was found, so every tick tries every point.
  5. The query turns the aim and tries again only for a place over the height limit or nearer than the least distance. Reaper's node sets neither, so there is no second try.
- **The probe's checks of what a cast touched**, in its order:
  1. The map's bounds (`0x7FF6D1E3F280`: its lowest height and a walk over its volumes). Not ported; the same function cuts the aim short in the query.
  2. A floor (`0x7FF6D1EC5550`): on the map's own geometry the surface's bit 0 clear (the bit that keeps the mover's probe off a triangle) and the contact's normal over 0.71934 upright. A hit on an entity goes through more, which was not read.
  3. Sight (`0x7FF6D1EBDA00`), from the hit raised by 0.015: a ray from 0.85 m over it to the eye and one back, and a ray from the eye to 0.75 m over it. None may hit.
  4. Room (`0x7FF6D1EBE210`): a sphere of 0.4 m from 0.5 m to 1.6 m over it. It may not hit.
  5. The place is the hit itself.
- **Two things the markers settled:**
  - The filter of the sight and room casts has one bit more than the others' (+0x18: 0xE001 against the constructor's 0xE000, `0x7FF6D1EACB30`). In those casts a start inside something counts as a hit: with that, 7 aims whose first place is a few centimetres too near a wall come out as the client has them. In `contacts.scene_sweep` it is the cast's own flag (`touching`, the input's +0xA0).
  - A ray's place is on the surface, a stretch's 1 cm over it (the engine's convex cast gives the contact pushed out by the triangle's skin). The markers have both, to the millimetre.
- **What the casts meet.** Their filter reads as the mover's with one category less: the hero's own category (0x18 without one), include 0x1081 where the mover has 0x1481 (`props.py`). So the rule runs on the mover's collision as it is. (Section 39: less the placed desks, and no place on a flagged polytope.)
- **Against the recorded markers** (`tools/research/marker_compare.py`; section 37's rule / this one):

  | Recording | Aims | Place on both sides | Within 1 cm | Within 0.25 m | Only one side has a place | Most |
  |---|---|---|---|---|---|---|
  | The yard | 57 | 47 / 47 | 42 / 46 | 44 / 47 | 1 / 1 | 0.49 m / 0.02 m |
  | The spawn room | 88 | 79 / 80 | 57 / 75 | 71 / 77 | 2 / 2 | 3.03 m / 3.03 m |
  | Aims level and upward | 37 | 6 / 6 | 1 / 5 | 3 / 6 | 0 / 0 | 1.13 m / 0.09 m |

  Of the 182 aims, 46 have no place on either side and 126 are within a centimetre. A place takes about 10 ms to work out.
- **The 10 aims still off, and what is known of each** (section 39 settles 6 of them):
  - 3.03 m: the room, heading 90, 6 degrees down. The client stands him on a chair 2.4 m on; the rule finds the floor at the wall behind it, which looks nearer the crosshair. The same floor spot is the client's own marker at 12 degrees down. Not explained.
  - 0.50 m: a level aim onto a counter. A monitor's hull 0.25 m from the place stops the rule's room sphere and not the client's.
  - 0.48 m, and four of 0.02 to 0.17 m: a level stretch let down on a flat top touches along its whole length. Which triangle's contact is kept goes by the order of the mesh's tree, which the port does not have.
  - 3 where only the rule has a place: twice a ray or a stretch touches the edge of a steep triangle (the contact's normal is upright, the triangle is not); once on a low box.
- **Guards:** `tools/research/test_marker_compare.py` (the three recordings, to the centimetre); `ShadowStepTests` in `tools/lobby/test_ability_moves.py` (the depth, a table under the line, sight and room).

## 39. The aims still off after section 38, 2026-11-10

Section 38 left 10 of 136 aims off. A fourth recording (`shadow-step-marker-pitch-2026-11-10.rec.gz`: 98 aims in the spawn room, six headings, level to 15 degrees down in steps of one) showed where the marker switches from one place to another, and with it five more things the rule had wrong or lacked. All are in `tools/lobby/shadow_step.py` now.

- **A thin probe is a true ray.** The rule cast it as a line 1 cm thick (the engine's sweep with no radius). A true ray ends on the surface, has the triangle's own normal, and misses an edge it passes by millimetres (`World.ground`, the mover probe's own query). The case that showed it: the sphere touches a counter's edge, the contact comes back pushed 1 cm off the edge by the engine's skin, the start is half a metre over that, and a ray dropped from there passes the edge by 4.5 mm and finds nothing. The client shows no place there.
- **The pick's camera looks at the first start, not along the aim.** The placement function gives its camera the direction from the eye to the first start as its forward and the world's up (the code before `0x7FF6D1EC2164`); the candidates are compared by their height in that picture alone. Two aims 6 degrees apart had flipped between two places 0.17 m apart.
- **The loops stop early.** A switch (`img+0x2E2EBC8`, 1 in the runtime image) ends the first start's rays at their first place and the second start's stretches two after their first. No recorded aim tells the difference; it is ported because it is read.
- **The casts go through the Range's desks.** With the placed desks in the collision the rule stood him on a desk where the client's marker is on the floor under it (7 aims at one heading), and a monitor's hull stopped the room sphere where the client's passes (5 aims). The marker does stand on a railing. Of the prop models with shapes here, the desks (`0x0D00000000001102`, `0x0D00000000001104`) have every flag but one (mask 0x1FDF), the railings (`0x0D00000000001986`) only five (0x111A). The rule takes the flag 0x80 as the difference (`collision.PASSED`): fitted on these two kinds, not read. The node's cast filter (`m_castFilter`, asset `0x069000000000010C`, class `STU_8EFC337B`, with the flags 0x1000 and 0x80) is the likely place in the client.
- **A crate is no place.** Each polytope of the collision chunk has a flag word after its mask and material. Bit 0 is set on 30 of the Range's 36 (crates, coolers, lockers) and clear on 6 (the tables). The client refuses every place on a flagged one (9 aims at a cooler) and takes the ones on a table: it is the polytope's "not ground", as a triangle's surface bit is, and the probe's floor check reads it (section 38, check 2). The cache keeps it as a mask bit (`collision.NO_STAND`, cache version 6). The mover's port does not read it; whether the client's mover does was not looked at.
- **What was ruled out:** the casts' category is the hero's team proxy (`0x7FF6D1DEAA70`: 0x08, 0x10, or 0x18 without a team), as the mover's, and the Range has no shape that stops one team only; the extra half metre of depth is real (13 agreeing markers lie deeper than the cone); a triangle's face normal in place of the contact's normal breaks aims the client accepts.
- **Against the recorded markers** (`tools/research/marker_compare.py`; section 38's rule / this one):

  | Recording | Aims | Place on both sides | Within 1 cm | Only one side has a place | Most |
  |---|---|---|---|---|---|
  | The yard | 57 | 47 / 47 | 46 / 46 | 1 / 1 | 0.02 m / 0.02 m |
  | The spawn room | 88 | 80 / 80 | 75 / 77 | 2 / 0 | 3.03 m / 3.03 m |
  | Aims level and upward | 37 | 6 / 6 | 5 / 5 | 0 / 0 | 0.09 m / 0.09 m |
  | Pitch by pitch in the room | 98 | 74 / 74 | 50 / 66 | 9 / 0 | 3.92 m / 3.08 m |

  Of 280 aims, 72 have no place on either side and 194 of the other 208 are within a centimetre (176 before).
- **The 14 still off:**
  - (Section 40: wrong. No box is in the way; the pick's camera was misplaced.) 7, all 3.0 m: one heading past a table, 2 to 7 degrees down. The client's sphere is stopped at the table by something above 2.30 to 2.35 m that the collision does not have, and passes under it from 8 degrees on. A screenshot shows an open cardboard box on that table: one of the client's own breakables, which the prop data leaves out because only the client runs their physics. Not verified. The server could not follow such a box once it is knocked over anyway; with the feed on the marker is the client's.
  - 3 of 0.48 m (one aim, recorded twice) and 0.09 m: a level stretch let down on a flat top touches along its whole length, and which triangle's contact is kept goes by the mesh tree's order, which the port does not have.
  - 1 of 0.09 m and 1 of 0.17 m: two places almost equally near the middle of the pick's picture.
  - 1 of 0.02 m.
  - 1 where only the rule has a place: a stretch resting on the ridge between two steep faces of a rock. (Section 40: the floor check goes by the faces, and has no place there.)
- **Guards:** `tools/research/test_marker_compare.py` (the four recordings); `ShadowStepTests` in `tools/lobby/test_ability_moves.py` (a placed prop with a railing's and with a desk's flags; a barred floor).

## 40. No box in the way: the pick's camera and the floor check, 2026-11-10

Section 39 put seven aims (one heading in the spawn room, 2 to 7 degrees down, 3 m off) down to a breakable box that stops the client's sphere. That was wrong, and the collision data is left as it is. Two things in the rule were, and both are read and ported now (`tools/lobby/shadow_step.py`).

- **Why it is not a box:**
  - For those aims the client's marker is, to 0.1 mm, where the rule's own second stretch touches the rim of a stool: `(56.9084, 1.7763, 42.1026)` at 1 degree down, `(56.9789, 1.7756, 42.0907)` at 7. That stretch only comes out so when the sphere went on to the far wall. With the aim ended in the air over the table the line is another, it is one stretch and not two, and it is 0.115 m thick and not 0.1: the contact is then 2 cm elsewhere at 6 degrees.
  - The casts could not meet such a box either. The engine's pair filter (`0x7FF6D2017730`) wants the shape's category in the query's second word, which is 0x1081 in every cast of the query: categories 1, 0x80 and 0x1000. A prop with physics of its own has 0x100 (`props.py`).
  - So the rule had both places, the stool's rim and the floor at the far wall, and picked the wrong one.
- **The pick's camera stands a metre behind the eye.**
  - The placement function fills the camera's description with the eye, the direction to the first start, the world's up, and three numbers: 1, pi and twice the cone's angle (the code before `0x7FF6D1EC2164`). The camera's setter (`0x7FF6D2C97380`) takes the last two, halved, as its fields of view (across and up) and puts the camera at `eye - 1 * direction`: the first number is a distance back.
  - From a metre back a near place lies nearer the middle of the picture than from the eye. For the seven aims: from the eye the floor at the wall is 8.8 degrees under the first start and the stool 10.4; from a metre behind, their heights in the picture are 0.1308 and 0.1306 at 6.9 degrees down, and 0.1150 and 0.1151 at 7.9.
  - Live, with the pitch in steps of 0.1 degree: the marker goes from the stool to the floor between 7.53 and 7.62 degrees down, the same going up and going down, and from 0.45 m nearer the wall it is on the floor at 5 degrees already. The rule changes place at the same steps (`shadow-step-marker-fine-2026-11-10.rec.gz`, 143 aims, all to the centimetre).
  - Of the 483 recorded aims where the rule as section 39 had it finds several places and the client's is one of them, the pick from a metre back takes the client's 483 times; from the eye 411 times, from half a metre 420, from two metres 431.
  - Not settled: whether the picture's upward axis is the camera's own or the world's. Both take the client's place all 483 times; the rule uses the camera's own.
- **A floor is judged by the mesh's faces, not by how the cast touched it** (`0x7FF6D1EC5550`, read whole now; `shadow_step._floor`):
  - What was touched is a mesh (shape type 3): the function asks the mesh for its triangles within 5 cm of the hit (`0x7FF6D2003900`, the box query the mover's contacts use) and goes through them in their order. For each it takes the point of the triangle nearest a point 5 cm over the hit (`0x7FF6D1FE8230`) and the triangle's own normal.
    - A triangle facing sideways or down (normal's height under 0.0000119) is not looked at.
    - A triangle farther than the nearest floor so far is not looked at (by 0.0000119 more once the answer is yes, less while it is no).
    - A floor (normal's height over 0.71934) makes the answer yes unless the map bars its surface (bit 0), and becomes the nearest. A steeper one makes it no.
  - Anything else (a polytope of the chunk, or no triangle in the box): the contact's own normal over 0.71934 and the surface word's bit 0 clear, as section 38 had it for everything. A polytope's flag (section 39) is that word.
  - What it changes: a line let down on a table's edge touches it with a contact normal 52 degrees off upright, and the table's top is a floor all the same; the marker stands there (10 aims of the new wall recording). And a line resting on the ridge between two steep faces of a rock is on no floor, though its contact is upright (the one aim where only the rule had a place).
- **Over a floor the first start is the normal's height times 0.5 up**, not 0.5: the client multiplies the offset by the normal after zeroing its level parts (`0x7FF6D1EAFED3`). On a slope of 30 degrees that is 0.43 m.
- **New recordings** (`tools/research/marker_compare.py`; its aims now count the same heading and pitch from another place as another aim, so the upward recording has 46 aims where it had 37):
  - `shadow-step-marker-wall-2026-11-10.rec.gz`: 196 aims at the room's far wall, from the spawn (heading 96 to 81 in steps of 2 degrees) and from 0.7 m beside it (100 to 91), level to 13 degrees down.
  - `shadow-step-marker-fine-2026-11-10.rec.gz`: 143 aims at heading 90, the pitch from 5 to 9 degrees in steps of 0.1, from the spawn, from 0.64 m farther and from 0.46 m nearer, and once going down from 9 to 2 degrees.
- **Against the recorded markers** (section 39's rule / this one):

  | Recording | Aims | Place on both sides | Within 1 cm | Only one side has a place |
  |---|---|---|---|---|
  | The yard | 57 | 47 / 47 | 46 / 46 | 1 / 0 |
  | The spawn room | 88 | 80 / 80 | 77 / 78 | 0 / 0 |
  | Pitch by pitch in the room | 98 | 74 / 74 | 66 / 73 | 0 / 0 |
  | Aims level and upward | 46 | 11 / 11 | 10 / 10 | 0 / 0 |
  | The far wall (new) | 196 | 184 / 186 | 165 / 183 | 2 / 0 |
  | Fine steps of pitch (new) | 143 | 143 / 143 | 101 / 143 | 0 / 0 |

  Of 628 aims, 87 have no place on either side and 533 of the other 541 are within a centimetre (465 of 539 before, 3 one-sided).
- **The 8 still off, to chase later.** Seven are level aims, where a stretch lies flat on a table top or on the floor and touches it along its whole length; the engine keeps one contact of many, and which one was not looked into. The eighth is 2 cm on the yard's floor. Recording is the part of the file name after `shadow-step-marker-`; `python tools/research/marker_compare.py FILE --worst 8` prints them.

  | Recording | Frame | Feet | Heading | Pitch down | The client's marker | The rule's place | Apart |
  |---|---|---|---|---|---|---|---|
  | `wall` | 17478 | (54.57, 1.00, 42.15) | 81.2 | 0.0 | (58.73, 1.01, 42.77) | (57.57, 1.01, 42.59) | 1.17 m |
  | `wall` | 16161 | (54.57, 1.00, 42.15) | 83.2 | 0.0 | (58.70, 1.01, 42.62) | (57.55, 1.01, 42.49) | 1.16 m |
  | `room` | 10008 | (54.57, 1.00, 42.15) | 120.0 | 0.0 | (57.19, 1.96, 40.62) | (57.61, 1.96, 40.38) | 0.48 m |
  | `pitch` | 11350 | (54.57, 1.00, 42.15) | 120.0 | 0.0 | (57.19, 1.96, 40.62) | (57.61, 1.96, 40.38) | 0.48 m |
  | `room` | 15503 | (54.57, 1.00, 42.15) | -30.0 | 0.0 | (53.39, 1.87, 44.17) | (53.43, 1.87, 44.09) | 0.09 m |
  | `up` | 13017 | (54.57, 1.02, 28.48) | 0.0 | 0.0 | (54.59, 2.17, 45.15) | (54.50, 2.16, 45.18) | 0.09 m |
  | `wall` | 9580 | (54.57, 1.00, 42.15) | 93.1 | 0.0 | (57.39, 1.78, 41.99) | (57.44, 1.78, 41.99) | 0.05 m |
  | `outside` | 15967 | (54.67, -2.00, 6.55) | -60.0 | 5.9 | (50.60, -2.00, 8.75) | (50.62, -2.00, 8.75) | 0.02 m |

  - The two of 1.2 m: the floor, a line 5 m long lying on it. The two of 0.48 m are one aim over a table top, recorded twice.
  - Where to start: the stretch's cast (`contacts.scene_sweep` with two points) against the mesh, and which of the touching triangles the client's sweep reports (`mesh_sweep`; the engine's is `0x7FF6D1F817C0` to `0x7FF6D1FAD430`).
- **Still fitted, not read:** that the casts go through the placed props with the flag 0x80 (section 39). Without it 13 aims of the pitch recording are off again, so it stays.
- **Guards:** `tools/research/test_marker_compare.py` (the six recordings); `ShadowStepTests` in `tools/lobby/test_ability_moves.py` (a table's edge, a ramp, no triangle near, a polytope's normal and flag).

## 41. Guardian Angel, 2026-11-10

Mercy's Shift flies her to an ally. Built on the kits body the way Swift Strike is: the server moves its copy and the client follows by the owner's records.

- **The graph** (006E, `tools/research/graph_tree.py 006E`):
  - Shift (logical button 35) with a target in the graph's own tracking state (owner state 5) sets var431, the point to fly to, in three steps: by the ally (var536), 0.5 m up, and short of him by 2.5 m plus something of the two bodies, 3.7 m at most.
  - A StateMoveTo (owner state 0) takes her there at var1002 = 17 m/s. Targets count within var1648 = 30 m.
  - After 0.15 s the button again ends it. So does the jump button, which sets var87 and with it the launches on the way out (nodes 73, 77 and 100: the velocity with 5 m/s more upward, then 14 or 22 m/s).
  - The cooldown (var472 = 1.5 s) starts when the MoveTo ends.
  - The VM does not run any of it without a target (`ability_states.py Mercy shift` stops at the tracking state), so the states sent are read off the tree.
- **What is built:**
  - `kits.Ability.follows` (speed, reach, metres short, metres up): the press only starts the ability with an ally bot under the crosshair (`Match.ally_ahead`: the staff's cone, 30 m, in clear view). Without one nothing starts and no cooldown runs.
  - `Match.guard` aims the mover at the point by the ally every step, wherever he is now; `PlayerMover.fly_to` makes 17 m/s straight at it the body's whole movement (no gravity, no input). Once there, or with the ally dead, it ends and the 1.5 s start.
  - It is a `Timed` ability: the button again after 0.15 s ends it on the way.
  - While it runs the frame carries the StateAbility and the flight's pose (owner state 2).
- **Live** (the viewer's mannequin as the ally: `OW_VIEWER_FILE` holding `ally soldier: 76`): from 4.1 m away from the point she is there 0.25 s after the press, half a metre up, then drops. The client's own body trails the server's by 1.3 m at most on the way and is there 0.14 s later. The arm goes out in the flight's pose. No errors in the lobby log.
- **Shortcuts:**
  - The point is 2.5 m short of the ally; the graph's own distance (2.5 to 3.7 m by the two bodies) is not decoded.
  - A jump on the way does not sling her on.
  - 2 s at most; the MoveTo's own limits (its fields `m_717EAFE7` = 50, `m_7F3716A6` = 0.2 and more) are not read.
  - The target is picked as the staff picks its own, among ally bots only. Valkyrie's 25 m/s is not there (no ultimate).
- **Guards:** `GuardianAngelTests` in `tools/lobby/test_ability_moves.py` (the flight, its end and cooldown; no ally in view, too far, an enemy; the button again).

## What to take, cheapest first

1. **Decoder oracle.** Done 2026-11-09: `tools/lobby/oracle.py` and `conftest.py` (details in `tools/lobby/CHANGELOG.md`).
   - Every `Stream` chunk in the lobby tests is read by the client's rules, and a test fails on a crash, refusal or misread.
   - The 18 graphs the catalog lacks come from `tools/assets/graph_record.py`.
   - First sweep: one bug. `target.WEAPON_GRAPHS` was missing the silent remote slots for 9 heroes, so Lúcio bots' fire never showed and Mei's frames were misread.
   - The sweep now reads 707 of 708 chunks cleanly.
   - Frames built without a `Stream` (most kit tests call `frame_builder` directly) are checked only where a test calls `oracle.read_full`.
2. **Hit registration.** Done 2026-11-09, together with the view-delay rewind and per-hero eye heights (`tools/lobby/CHANGELOG.md`, research/hitbox-drift.md items 3-4).
   - Our IDA confirms the view-time formula (0x7FF6D17CEEC0) and the writer's order (0x7FF6D1E4DD00).
   - The click angles' yaw/pitch order was confirmed live (research/hitbox-drift.md item 4).
   - Eye heights come from each body's `STUFirstPersonComponent` (`tools/assets/eye_heights.py`), not only Soldier's.
3. **Mover data.** Use `game_movers_174.json` values for capsule and speeds, plus `collision_props_174.json` props, in the fallback mover when `OW_CLIENT_SYNC` is off. Superseded by 4, which reads them (section 6).
4. **Full mover port** (`mover.py`, `contacts.py`, `collision.py`, `casc.py`, `props.py`; about 5k lines, stdlib only). Exact dead reckoning by default without the ASI feed, and it makes `correction.py` usable. Tested 2026-11-09 (section 6): exact against our client on a 57 s Range walk, where our mover ended 2.6 m off. **Ported 2026-11-09:** vendored byte for byte, with `tools/lobby/player_mover.py` for Sprint, Boosters and launches (`tools/lobby/CHANGELOG.md`). A second live walk on our server: 0.000 m at the end, 0.039 m at most (`lizrollin-runtime/range-walk-2026-11-09-port.rec.gz`). Exact with no correction on the Range and Hanamura (section 7); D.Va's Boosters within 0.85 m (section 8). Tracer, Reinhardt and Torbjörn exact too (section 11). `correction.py` ported for Soldier: 76 and checked live (section 9).
5. **VM port.** This would replace hand-mapping each hero in `kits.py`. It's a large refactor of our `statescript.py`/`match.py` layer, and upstream has verified only Soldier: 76. ORPHEUS's later fixes to this runtime are in its changelog, not in this commit. Run beside our kits instead (section 10): it found the spread values our frames overwrote. **Ported 2026-11-09** (section 15): opt-in per hero (`OW_BODY_VM`), checked live for Soldier: 76; `kits.py` stays the default and the fallback.
6. **Bot walk, death screen, pools.** Ported and checked live 2026-11-09 (section 13).
7. **Damage data, respawn time, armour.** Compared and fixed 2026-11-09 (section 14).

Keep the MIT notice with anything copied, as `tools/lobby/vendor/README.md` does for Boi-027's code.
