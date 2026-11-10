# Assault mode graph (step 5 starting point)

Mode `0x0230000000000014` (Assault) references graphs `0x0580000000000F4A` (m_CF63B633, the mode graph; Escort and Hybrid use it too), `0xC8E` and `0xC90` (team scripts, also in the Practice Range modes). Source: `.scratchpad/pva/c5.txt` (data survey) and `research/practice-vs-ai-data.md`.

## What runs already

- Hanamura in Assault loads with the assault emblem on hero select. In game there is no objective HUD.
- Graph F4A runs client-side on the game-mode entity `0xD0000010`: its ChaseVar state 36 (remote 21) chases var 1303, the countdown the server already sends (`Match._send_countdown`). The assemble countdown works in Assault.

## Graph F4A (pinned catalog `.scratchpad/game_graphs_174.json.gz`; dump `.scratchpad/gn_f4a.txt` via `graph_nodes.py 0xF4A`)

- 534 nodes; 8-bit state index, 7-bit remote-sync node index, 8-bit sync-var index.
- Node classes: 161 SetVar, 76 `STU_3387AB5D`, 48 `STU_38EE1100`, 29 Stack, 27 BooleanSwitch, 24 `STU_57692020`, 22 EntryGameMessage, 9 SendGameMessage, 8 Wait, 8 ChaseVar, 6 SubScript (`STU_37D754C8`).
- At least 63 remote-synced states (rem 0..62+). Most are `STU_38EE1100` with a `params` list keyed by `0x0A4...` identifiers: the server switches them on through the mode entity's remote-state vector (H0 frame, research/remote-bot-statescript.md).
- SubScripts point at graphs `0xC35`, `0xC8F`, `0xCBB` (state 12), `0xD44` (state 38), `0x1B47`, `0x1FEB` (state 11, client-only).

## The mode instance (2026-11-02)

- No client instance of F4A exists by itself (live scan, `tools/research/guidhits.py`); the PvP controller C90 does.
- Count rule, checked on C90 (118/29), 13C1 (43/12) and 288B (30/4): owner states = states - client-only - server-only; sync vars = instance-scope entries. F4A: 179 states, 14 client-only, so 165 owner; 102 remote; 59 instance vars, 34 entity vars.
- The countdown frame on the mode entity is now an H0 frame with instance 1 = F4A. Hero select switches to "ATTACK HANAMURA", and in game the top objective widget shows "0.0 UNSPECIFIED".
- Live: F4A active states 11, 79, 144, 145, 167-170 (11 is the client-only SubScript to 1FEB); 1FEB active 0, 2, 3, 6, 10-13, 15-18, 20, 24, 31, 33, 39-41. 1FEB has 7 UXPresenters (widgets 0x0A88...2CC, 083, 22D, 085).
- 1FEB reads, among others, entity vars 23004, 23020, 23021, 1303, 2499, 7901, 10795, 10548 that F4A writes (23004/23020/23021 in client-only Stacks 165/166/168 and node 578). Their meaning is not decoded.
- The player (team 0, spawned at the catalog's defence spawn) is shown as attacking: which team attacks comes from F4A's variables, still unset.

## Phases (2026-11-02)

Remote-state encoding (reader `0x7FF6D1C2BE30`): per remote slot a present bit; then, for a state whose node virtual +0x28 returns false, one bit (0 = end it, 1 = start it) and the state's remote data; for one returning true, a control bit, a desired-active bit and the data. F4A's message states take the plain form with no data (`active={slot: None}`).

Switching one F4A remote state at a time (live, `phase-states` capture):

| Remote | State | Message | Widget |
|---|---|---|---|
| 2 | 5 | 0x700 | WAITING / NOT ENOUGH PLAYERS |
| 5 | 9 | 0x692 | ASSEMBLE YOUR HEROES |
| 8 | 14 | 0x701 | PREPARE TO ATTACK (team 1: PREPARE YOUR DEFENSES) |
| 11 | 18 | 0x694 | ATTACK OBJECTIVE A |
| 13 | 20 | 0x695 | widget hidden |
| 0, 1, 6, 12, 14-18, 31, 33 | | | UNSPECIFIED |

The number left of the text is v1303 (seconds, m:ss). Team 0 attacks: with the player on team 1 the setup text reads "Prepare your defenses" although the catalog's team-1 spawn is the attackers' arcade room.

## Server-started SubScripts and the objective entity (2026-11-02)

- F4A SubScripts: 1FEB (state 11) and 1B47 (144) are client-only and start by themselves; CBB (12), D44 (38), C35 (49) and C8F (163) must come from the server, as instances with `parent=(1, state)` in the mode frame (no remote slot needed).
- CBB alone: the phase banner ("ATTACK OBJECTIVE A" across the screen at round start). C35: the round-complete screen ("ROUND 0 COMPLETE", score 0 | 0, time remaining); it shows at once when created. D44 creates entities (server logic).
- The capture-point icon and progress are not in F4A's family. Hanamura's catalog objectives (placeables 487 and 1034) are entity-chunk records 328 and 875 (catalog index - first_entity 159; record walk of chunk `0x0DD0000B00000165`, each record: 16-byte id, u32 kind, u32 size, u64 entity). Both are entity `0x0400000000001C22`: target tag, `STU_C3ABE193`, statescript graphs `0x0E8E` and `0x0F4C`, schema var `0x2931` = 5. Creating the placeables shows nothing more; the graphs are not in the pinned catalog, so their counts and states need a local graph decoder.

## Objective icons and more mode variables (2026-11-02)

- `tools/assets/graph_layout.py` decodes any graph's layout (arrays in the STU v2 dynamic area; client-only marker E1B0C07F, server-only 9A861B79). Objective graph 0E8E = Graph(0x0E8E, 59, 31, 29), entity vars 9864-9866, 9872, 9876, 9877, 16927, 10400, 10401, 11684, 9900, 10548, 18109, 99, 98, 9836, 9837; client-only SubScripts 70, 73, 74 start 1824 (two UXPresenters, reads 18109 and 16927). 0F4C has no states.
- An E8E instance on each objective entity draws two diamond icons under the widget, both with a padlock, in setup and in the attack phase.
- Not the lock (live probes, `assault-probe-*` captures): E8E's message slots 1, 7, 8, 12, 16, 20 and slots 0, 2, 4, 13, 14, 17, 18, 23 with no data; objective-entity vars 18109 (Int 0-4), 16927, 9866, 10548, 9864, 9865, 9872, 9876, 9877; mode vars 10548, 9866, 10545, 10806-10808, 7712, 7713, 7974, 9518, 6971, 12112, 12376, 19823, 18546.
- Graph 1824's UXPresenters bind UX data `STU_E870E3E8` to vars 11947-11949, 17560-17562 and 25460 (team); each value is an expression over ConfigVarDynamic ids 1030, 18108 and the slot's own id, and the presenters also read 16928, 26613 and 12600. Live probe (2026-11-02): all of these as Float 0.5 / Int 1, on the objective entity and on the mode entity, leave both padlocks as they are. Next lead: decode the `m_expression` bytes to learn which entity each expression reads.
- Mode var 3392 = 1 plays the OVERTIME banner. Mode var 9544 = 1 switches the widget to the score layout (team bars, "0.0%").

## Point B and round end (2026-11-02)

- Mode var 10548 is the point under attack: with remote slot 11, Int 1 turns the widget to "ATTACK OBJECTIVE B". Slot 24 is "ROUND COMPLETE"; slot 25 also reads "ATTACK OBJECTIVE A". 10545 and 7712 do nothing visible.
- The round-complete screen (SubScript 0C35 under state 49) shows "ROUND 0 COMPLETE", 0 | 0 and "TIME REMAINING 0.0" regardless of v1303: its round number, scores and time are its own variables, not found yet.
- `tools/lobby/assault.py` runs the capture rules; see the lobby CHANGELOG. Walk-on check: `research/assault-capture/walk-on.png`.

## Victory/defeat search (2026-11-02)

- Plain remote slots 12, 14-19, 22, 23, 26, 27, 31, 33, 40, 42, 43, 54-57, 59, 61-64, 71 and 78, each set alone in the attack phase, show no banner or screen; at most the widget text changes (`.scratchpad/slot-probe.sh`). Slot to game message (all 0x0240...): 12 and 24 -> 713, 14 -> 6FD, 15 -> 727, 16 and 48 -> 724, 17 and 49 -> 72F, 18, 19 and 50 -> 735, 54 -> 93A, 55 -> 939.
- SubScript C8F (state 163) = Graph(0x0C8F, 10, 4, 3) only relays game message 0x687; D44 (state 38) has no states.
- The match end therefore uses the round-complete screen and then lobby message 20304 (back to the menu).

## Round-complete values (2026-11-03)

- 0C35's presence bits are its 27 instance-scope sync vars (scope 0) in slot order; its scope-1 vars 8078, 7887, 9435, 13049 are entity vars. Node 919 (`86A2C83C`) reads 2198, 4912, 4913, 7899, 7901, 7908, 7911, 7997, 7998, 8017, 8018; ChaseVar states animate 7907->7908, 7910->7911, 8032->8017, 8033->8018 (`.scratchpad/c35_vars.py`).
- Live probes (10 s round, `.scratchpad/rc-run.sh`): **bit 6 (v7899) is the blue (left) score, bit 7 (v7901) the red (right) score**, as Int. Server sends them now (`round-complete-score.png`: "1 | 0" after a round lost on point B).
- No visible change: 7908, 7911, 7997, 8018, 4912, 4913 (Float), 7913, 7914, 10282-10285 (Int), mode entity var 7887.
- 8017 nonzero (Int or Float, with or without 8027) blanks the screen: only the backdrop and two lines draw. It likely selects another flow (time bank?) that needs more data.
- Second pass (same day):
  - **bit 4 (v2198) not 0 adds a "CAPTURE PROGRESS" row; bit 3 (v8018) is its value, Float 0..1** (0.5 shows "50.0%"). The server sends both when the attackers lose (`round-complete-progress.png`, 37.0% with a TEMP progress of 0.37).
  - **bit 5 (v8027) not 0 shows "FINAL ATTEMPT!"** left of the score (`round-complete-final-attempt.png`). Not sent: one round only.
  - 8017 is not the time: with 8032 preset to the same 47.5 the left score animates to "-43" and the screen then blanks. 8017/8032 look like a score-change animation.
  - No visible change: 6828, 7858 (Int), 8032, 8033 (Float).
- Round number ("ROUND 0") and "TIME REMAINING 0.0" are still not found. Not tried: the BooleanSwitch conditions 7967, 8002, 8003, and the scope-1 entity vars 8078, 9435, 13049.

## Capture HUD: graph 1824's expressions (2026-11-03)

- `m_expression` (STU_919FD47C) is a flags word, then one dynamic-area array per set flag: bit0 = ConfigVarDynamic refs (operand list), bit1 = config-var objects (`m_configVars`), bit3 = constants; the last array is the opcode bytes. Opcodes seen: `08 i` push dynamic var #i, `07 i` push var id #i, `0F` read that var on the entity below it (`research/hud-entry-condition.md`), `0E i` call config var #i, `06 i` push constant #i, `11` (17) index: pops an Int and indexes the array below (VM case 17 at `0x7FF6D2DA1D5D`).
- Decoded (`.scratchpad`, ad hoc):
  - Stack 25: local v18108 := `v18109[v16928].v18108`. v18109 and v16927 are objective-entity vars; v16928 is 1824's presence bit 0 (set by E8E's client-only SubScripts 70/73/74).
  - Each padlock/progress value (E870E3E8): `v1030[v18108].v11947` (and 11948, 11949, 17560, 17561), then the same with v18107. v1030 and v18108 are also written by the two UXPresenters (29, 33: field E3B4FA5C).
  - Presenter conditions 36/74: `v16927[v16928]`.
- So v18109 and v16927 on the objective entity are **arrays** (of entities), not Ints: the 2026-11-02 probes sent Int/Float and could not work. The values 11947.. live on the entity the array names.
- Array wire format (reader `0x7FF6D1F77430`, from tag 15): 3-bit subtype s; s=6 list / s=7 pairs: `w_u16` count, 1 bit keyed (0 = plain array), then if count >= 1 one bit same-type; then each element as a tagged value (4-bit tag, +3-bit subtype for 15), or for s=7 two tagged values. s=4/5 are deltas: three `w_u16` (count, first, last-kept). With same-type = 1 the later elements skip their tag (reader `0x7FF6D1F77A20`, mode 2); not needed.
- Live probe (2026-11-03, `hudA`): objective-entity v18109 and v16927 = Array of three mode-entity references; mode vars v1030 = the same array, v18107 = v18108 = 0, v11947-11949 and v17560-17561 = Float 0.5. The client took the arrays (no error, no disconnect); both padlocks stayed. `statescript.Array` (s=6) is kept.
- Next: find which entity the arrays should name (team entities? the objective entity itself?) and v1030's real writer (presenters 29/33).
- Was: add an `Array` value (s=6, same-type 0) to `statescript.py`, send objective-entity v18109 = [team entity or mode entity] and v16927 likewise, and set v18108 / 11947.. on that entity. Which entity the client expects there is still a guess (the mode entity first).

## Next

1. Point A capture: the capture-progress HUD (which F4A vars/remote states), server capture rules (attackers on the point, contested by defenders), then "Attack objective B".
2. Round end: time out (defenders win) or point B taken (attackers win); victory/defeat screen and return to menu.
3. Setup and round times from an Assault wiki revision.

## Round-complete booleans (2026-11-03)

- Probes on 0C35's presence bits 8 (v7967), 0 (v8002), 1 (v8003), via a TEMP hook (reverted): all three as Bool true, then as Int 2 / 3 / 4, then bit 8 alone as Bool true.
- **Bit 8 (v7967) not 0 shows "SWITCHING SIDES"** with a spinner under the score (`assault-capture/round-complete-switching-sides.png`). It belongs after round 1 of a two-round Assault; the server plays one round, so it stays unsent.
- Bits 0 and 1 change nothing visible. The round number ("ROUND 0") and "TIME REMAINING 0.0" are not among bits 0, 1, 8; the rest of the 27 scope-0 vars are the next candidates (`.scratchpad/c35_vars.py` lists which nodes read each).

## Round number and time remaining: parent forwarding (2026-11-03)

- The round-complete SubScript (0F4A state 49) forwards parent values into 0C35 vars (`STU_6364CD4F` list): 7525, 7858 as is; 7967 from v10551/v10545; 10283/10285 = v10807e[0]/[1]; 10282/10284 from v10795e, v7901, v2499, v10551, v10545; 7919 = v10795e; 8002/8003 from v10795e, v7901. A forwarded child var is overwritten by the parent's value, so setting it as 0C35 presence has no effect.
- 0F4A side: v10551 = v12376e[v10548e] (state 52), v10545 is a Stack over v10551 (state 171), v10807 = v9866e or v9900 (SetVars 155, 345).
- Expression VM (from simple cases): op8 push a dynamic var, op6 push a constant, op17 index an array, op0 return, op2 short-circuit and, op4 jump if false, op24 multiply, op26 fmod.
- The title text is UX var 7906, set client-side to one of four UI strings (`0x5FF4`, `0x6002`, `0x088E`, `0x2FDB`, SetVars 20/23/24/25); the "0" in it is a format argument not yet traced.
- Tried, no visible change (`.scratchpad/rcp1..4`): 0C35 bits 8/16/17/19/20 (only bit 8 shows SWITCHING SIDES, as before); 0F4A presence bits 13 (10551) = 2 and 11 (10545) = 3; mode entity vars 10807 = [75.5, 61.5], 12376 = [2, 3, 4], 10548 = 1, 10298 = 0.
- Next: trace the 0x0A88000000000199 UI's text bindings for the round title and the time cell in IDA (the presenter's format path), instead of more blind probes.


## Round-complete bits probed (2026-11-03)

- The title presenter (0C35 node 18) binds UI data ids to v7906 (title text), v7901 (bit 7, red score), v7899 (bit 6, blue score), v6828 (bit 18), v2198 (bit 4, progress row), v7904/v8013/v17858 (team-side expressions) and two child UIs `0x0A8800000000019A`.
- Single-bit probes (`.scratchpad/banner_patch.py` OW_RC, reverted):
  - bit 2 (v8017) any nonzero value, int or float: the whole screen empties (background only).
  - bit 5 (v8027) = 1: "FINAL ATTEMPT!" shows left of the score (`main-menu/round-complete-final-attempt.png`).
  - bits 9-15, 18, 21, 25, 26 and entity vars 8078, 7887, 9435, 13049: no visible change.
- 0F4A's v7901 is the round counter (SetVars 110 and 343 add to it); in 0C35 the same id is the red score. "ROUND 0" and "TIME REMAINING 0.0" are still not found; they are not 0C35 presence.

## Capture HUD arrays, two more targets (2026-11-03)

- TEMP probe (reverted): objective-entity v18109 = v16927 = Array of three references to (a) the objective entity itself, with v18107 = v18108 = 0, v1030 = the same array and v11947-11949, v17560-17561 = Float 0.5 on it; (b) the player's body, with those values in the body frame's variables.
- Both accepted (no error, no disconnect); the padlocks stay as before in the setup and attack phases (`assault-capture/hud-array-probe-obj-player.png`).
- Tried so far as the array's entity: mode entity, objective entity, player. Left: a team entity, or the client fills v16928 (1824's presence bit 0) only when its own SubScripts 70/73/74 run, which may need E8E states on.

## Round number and time: the SubScript init data (2026-11-03, later)

- 0F4A's round-complete SubScript (node 111, state 49) carries an init list (`m_89402D74`, child var := parent expression) and a UI data-id list (`m_17BB822A`). Init: child v7901 := parent v7901 + 1 (round, 1-based); v4912/v4913 := v2499[0]/[1] (team points; v2499 is 0F4A presence bit 45, an array); v7907/v7910 := v12135[0]/[1] and v7908/v7911 := v10806e[0]/[1] (time banked: ChaseVar from 7907 to 7908); v8017/v8018 := v10808e[0]/[1]; v10283/v10285 := v10807e[0]/[1].
- 0C35 state 65 (`STU_9D7BF987`, remote slot 9) lists 30 node-level remote-sync vars (`m_6F0C0343`, data ids in `m_BF5B22B7`): the same ids as 0C35's instance sync vars. The class has no factory of its own; it is built as the base state `7C37840C` (constructor `0x7FF6D2D8F760`, vtable `0x7FF6D39CC9B8`).
- Probes, all with no visible change to "ROUND 0" or "TIME REMAINING 0.0" (TEMP, reverted): 0F4A presence bit 51 (v7901) = 3, bit 45 (v2499) = [4, 5]; mode entity vars 7901 = 3, 2499 = [6, 7], 10806 = [41.5, 42.5], 10808 = [51.5, 52.5], 10807 = [0.25, 0.75], 1303 = 33.5; 0C35 bits 11-14 as floats, 22/23, 24; 0C35 remote slots 1, 5, 6, 7, 8, 9 bare, and slot 9 with the time and score bits set. Without the server-sent 0C35 instance the screen does not open at all (the client does not start the SubScript itself).
- Bit 7 (v7901) stays the red score digit (`assault-capture/round-complete-slot9-probe.png`: 0 | 3).
- The title's format argument is distinct from the statescript title string. The bindings are traced below.

## Retail round result bindings (2026-11-04)

- The CKey-verified STU v2 schema `0x0A88000000000199` has 27 instances. Instance 5 declares data ID `0x0D80000000001EE2` as a localized handle (`BCD1C634`). Graph 0C35 presenter node 18 supplies var 7906 to that slot. Its four authored values are handles in string table `0x0C8800000000007D` (IDs `5FF4`, `6002`, `088E`, `2FDB`).
- The visual asset is CKey-verified STU v1 `0x09A000000000050D` (`RankedRoundTransition`, 190,600 bytes). The `7D211E22` bindings resolve through `B1A08D7B` paths and `80C108B9` segments. `roundCompleteTxt` at `0x20D90` reads `Round` (binding `0x20E00`, segment `0x20EC0`). The time cells read `Team0.Time` (binding `0x23520`, segments `0x235E0` and `0x23630`) and `Team1.Time` (binding `0x27AC0`, segments `0x27B80` and `0x27BD0`).
- These nested properties differ from the localized title handle, the red score var 7901 on 0C35 and the live countdown var 1303. Graph 0C35's server-side action `STU_86A2C83C` (node 88, reached from entry 12) consumes the round, points and time variables and is a lead for the native data producer.
- In the serialized graph the action is instance 919. It reads `v7901` (field `1AF7DD3E`), `v7908`/`v7911` (`362DC33E`/`AC77CFFA`) and `v4912`/`v4913` (`5A0ACD5C`/`8FB185B1`). Its class key RVA `0x2DE3698` is absent from the captured retail state constructor registry, consistent with the action's server-only path. `0199` declares two `019A` child schemas at `081B`/`081C`, with timer-shaped slots `0517`/`01DF` on each child. The mapping from native `TeamN.Time` to these slots or a result message remains unverified.
- The server freezes authoritative combat and movement on either result while still sending the result screen and the timed leave message. The objective padlock data source remains open.
- Retail smoke test `20261104T174849.671774Z` used shortened setup and round timers on Hanamura with the normal result frame and eight-second exit delay. A timeout showed `ROUND 0 COMPLETE` and returned to Play on schedule. The regression test separately verifies that late input and queued combat effects cannot change the result.

## Retail capture and timeout (2026-11-04)

- Hanamura capture fixture placed Soldier's authoritative body on A and then B, excluded bots from capture accounting and shortened the round and capture timers. It used the normal capture rules, HUD frames and eight-second exit delay. Match `048E` recorded A captured at 15:09:14, B won at 15:09:18, reliable `20304` at 15:09:26, then client `FIN` and `Player left`. See [`lobby.log`](main-menu/20261104T190812.021906Z/lobby.log) and the [Play menu capture](assault-capture/20261104-assault-verified-capture-return.png).
- Retail changed from [`ATTACK OBJECTIVE A`, 0:13](assault-capture/20261104-assault-verified-attack-a.png) to [`ATTACK OBJECTIVE B`, 4:09](assault-capture/20261104-assault-verified-attack-b.png). Both padlocks stayed visible. The win displayed [`ROUND 0 COMPLETE`](assault-capture/20261104-assault-verified-capture-result.png). Match `DEF0` timed out at 15:07:07 and returned via reliable `20304`, client `FIN` and `Player left` at 15:07:15 in the [separate timeout log](main-menu/20261104T190454.238770Z/lobby.log). The tournament card was locked after capture return, so the two runs used separate client launches.
- Capture graph 1824 stacks 17 and 21 populate `v1030[0/1]` from `v17858[0/1]` via `ECEE6694`. Its native evaluator `0x7FF6D1B43170` writes `2EAB`, `2EAC`, `2EAD`, `4498` and `4499` as team colors, matching the `ConfigVarColor` types in UI schema `0x0A88000000000252`. The separate boolean `6374` uses `STU_47703870` on each authored team and a native viewer/team comparison (`0x7FF6D1E3CB00`). The HUD padlock's source remains unresolved.
- The verified world-marker template `0x0BA000000000038E` reads `IsLocked` (path `0x8E50`). Its parent `HUD_In_World_Objectives` (`0x09A0000000000629`) references that template at `0x868` and binds `Items`, `ViewMatrix` and `ProjectionMatrix`. `HUD_GameMode_Duel` (`0x09A0000000000530`) separately reads `IsCapturePointActive` and `CaptureProgress`. These are separate from the Assault top HUD lock.

## Team-time presenter retest (2026-11-04)

- In graph `0C35`, child presenter slots `0517` and `01DF` read the two teams' v7854/v7855 and v7909/v7912 respectively. A temporary remote-instance probe populated all four inputs. The settled win still showed `ROUND 0 COMPLETE`, `2–0` and `TIME REMAINING 0.0`, with over four minutes banked for the attackers and distinct defender test values of 37.5 and 42.5 s. This leaves the `Round` and `TeamN.Time` data producer open. [Settled win](assault-capture/20261104-four-items-assault-capture-result.png) · [Win log](main-menu/20261105T002256.014788Z/lobby.log)
- The capture retest advanced to B with 4:08 on the live timer and both top padlocks still visible. The separate timeout showed `0–0` and `0.0%` progress. Both runs sent reliable `20304` eight seconds after the result and returned to Play. The Tournament Match card was locked on return. [Objective B](assault-capture/20261104-four-items-assault-attack-b.png) · [Timeout result](assault-capture/20261104-four-items-assault-timeout-result.png) · [Timeout return](assault-capture/20261104-four-items-assault-timeout-return.png) · [Timeout log](main-menu/20261105T001846.242271Z/lobby.log)
