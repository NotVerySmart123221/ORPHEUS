# Uprising: the mission's scripts (2026-11-05, later)

Where the mission's rules would have to come from, after Uprising boots and loads (`play-menu.md`, fourth round).
Live probes were run with a temporary hook in the vendor `Match._gate_frame` that read extra remote slots, child
instances and entity variables from a scratch file every second (removed again); the client's state tables were
read with `tools/research/stateitems.py` and ad hoc scans. Nothing of the mission is served yet.

## The remote frame was mis-encoded (fixed)

- The client's remote-state reader (`0x7FF6D1C2BE30`) walks the graph's remote table and **skips an entry without
  reading any bit** when the node is not a state (actions, effects: classes `C43F9944`, `0FF09DD0`, `1470EF32`,
  `100EF8C2`, `B49F8723`) or when the state is client- or server-only. For a state it reads the presence bit, then
  one bit (plain classes: start/end; "custom" classes from the hash switch `sub_7FF6D217E960`: a control bit, then
  a desired-active read), then the state's data (`sub_7FF6D2D92060`).
- `statescript.Graph` now carries `silent`, the remote slots that take no bit, and `remote_full_frame` skips them;
  `tools/assets/graph_layout.py` prints them (0D28: 10, 21-24, 31, 38; 0F4A: 37, 38, 45, 73-77, 80, 89-94; 0CBB:
  45, 46; 0C35: 0; 0E8E: 19, 21, 22, 24-26; 100B: 19, 53-57; 0CE4: 37, 47, 48, 50-54). The Assault probes used
  slots below 37 and so never met the problem; 0C35's slot 0 is silent, so its earlier slot probes were one off.
- The sync-variable count of 0D28 is 7 after all (`graph_layout.py` was right): the "6 that worked" was the
  missing slot-10 bit cancelling the extra variable bit. With both fixed, slot 14 lands on state 29 as it should.
- State data, calibrated by writing frames and reading the client's active states: a message state (`38EE1100`)
  and a message entry (`9D6CF8AC`) take **one more bit after the start bit** (0: no optional integer); the loading
  marker `9FC136D7` takes none. `statescript.message_state()` is that bit; the Assault phase states now send it
  (they were off by one bit for everything after them; unverified in game since).

## The mode's three scripts

`STUGameMode 0x023000000000000F` names `0D28` (m_CF63B633, the mode script), `0D29` (field 76E8C82A) and `0D2A`
(field A2781AA4). Assault's names `0F4A`, `0C8E`, `0C90` in the same fields, so 0D2A is Uprising's **player
controller** (the server creates 0C90 for it today: hero select works through that) and 0D29 its team/body
script. 0D2A is 14 states (SubScript to 1194, no hero select of its own); 0D29 is 55 states, entity vars 9956,
8202, 8203, 7730, 6035, 6036, 478, 7423, UX 0x0A88...252/19C/17B/07E, and posts 8C2.025 (a message the widget
graph waits for).

`graph_layout.py 0D28`: Graph(0x0D28, 78, 7, 42), entity vars 1921, 10249, 1303, 9757, 10207, 10082, 10019,
10083, 8619, 10041. Its SubScript states and child graphs (resolved through the 6DF07EE5 node that follows each):

| State | Child | Layout | What it looks like |
|---|---|---|---|
| 0 | 0CBB | (69, 13, 50, silent 45 46) | the phase banner (Assault's announcer) |
| 8 | 0C8F | (10, 4, 3) | relays 687.025 |
| 10 | 0D47 | (13, 0, 2) | sends 7B6/7BE.025 |
| 12 | 0CB6 | (1, 0, 1) | client-only states, one UXPresenter, waits for 6AF/717/692/701.025 |
| 23 | 0DC8 | (9, 1, 1) | |
| 31 | 0E95 | (1, 0, 1) | **the objective widget**: 9 UXPresenters (0x0A88...2B2, 2CE, 2CC, 07E, 252) |
| 33 | 0EB7 | (20, 14, 14) | waits for fourteen messages (6A4, 6A5, 6C7, 709, 71B, 7B6-7BE, 7CF) |
| 43 | 0EC5 | (3, 5, 4) | |
| 64 | 1195 | (5, 1, 2) | |
| 65 | 13A6 | (4, 7, 2) | one UXPresenter (0x0A88...2BD) |
| 77 (client-only) | 0E6B | (22, 3, 10) | the client starts it itself (instance 2); waits for 267/22B/A98/A7C.025 |

The client numbers instances itself: the 0D28 instance object keeps the local index at +0x20 and the server's at
+0x24 (the client-only E6B child is local 2, server -1; server children 2 and 3 became local 6 and 7).

Message states of 0D28 (`tools/assets/graph_messages.py 0D28`): slot 0 st 2 → 6DF, 1/3 → 694, 2/5 → 6C4, 3/6 →
695, 8/19 → 692 (ASSEMBLE), 9/20 → 6A7, 11/21 → 6E4, 12/22 → 6A3, 13/25 → 69C, 15/32 → 6F8, 28/55 → 7ED, 34/67 →
874, 35/70 → A90, 37/74 → 717; entries waiting: slot 4 st 14 ← 74E, 7/17 ← 68E, 26/51 ← 694, 30/59 ← 7F0.

## The objective widget (0E95) and why it still draws nothing

- 0E95's flow (`graph_edges.py`): entry → state 3 (waits for **74E.025**) → state 0 → presenters 1 (0x2B2) and
  2 (0x2CE). State 9 (← 694.025) → presenter 10 (0x2CC, the objective widget) → switch 11 (`DEBD057B`) →
  presenters 13-19 (0x07E variants). Presenter 10 is conditioned on dynamic var 27183 (instance 377).
- Who posts 74E: not 0D28. The install's graphs that do (`graph_messages.py --find 74E`): `0D4A` (state 15, its
  remote slot 0), `0D4B`, `0FEF`. **0D4A is the mission director**: 139 states, 136 owner, 64 Waits, 49
  SubScripts (45 to `0D1C`), entity vars 9879 and 1303, remote slots 0-3 and 9 (4-8 silent). No entity definition
  under 6 KB references 0D4A/0D4B/0FEF/0E95, so the server attaches it at run time (where is unknown; the mode
  entity accepted it as a root instance).
- Live, with a 0D4A root instance on the mode entity and its slot 0 on: 0E95 state 0 and presenters 1, 2 go
  active; with 0D28 slot 1 (694) also on: 10, 11, 12, 16, 20, 24, 25. **Nothing is drawn** for any of it, nor
  with entity vars 9881/10121/12138/2190 or 27183 set on the mode entity, nor with 0CBB created, nor with any
  single plain message slot of 0D28. So the presenters run but their widgets are not visible: the HUD scene or
  container they present into is missing, most likely because the player's controller is 0C90, not 0D2A (whose
  SubScript 1194 may hold the PvE HUD), or because 0D29 never runs.

## Tried: the mode's own controller and team script (2026-11-05, late)

- 0D2A (with 1194 under its SubScript state 2) as a second root instance in the player's controller frame beside
  0C90, then 0D29 as a third root (a `mode_controller` argument to `heroselect.controller_frame`, reverted):
  hero select unchanged; the client creates all three (live: 0D2A state 2 on, 1194 nothing, 0D29 its client-only
  states 26, 46-48); the HUD is unchanged, also with 0D4A, 0E95 and 694.025 on. Neither 0D2A nor 1194 presents a
  widget of its own; 0D29's presenters (0x0A88...252/19C/17B/07E) did not show either. So the controller is not
  what hides the objective widget. Left: where the UXPresenter puts the widget (IDA, the presenter's begin path and
  its scene/layer), and whether 1FEB's 2CC in Assault differs in that respect.

## The map's mission placeables (`0x0DD0000B000006FE`, 1145 records; mode 0x0F uses 89)

Record: 16-byte id, u32 kind, u32 size, u64 entity, u64 identifier at +0x20, position floats at +0x30. The
89 are: 68 × entity `0x16DC` (target tag + empty statescript component, identifier `0x0D80000000002283`:
spawn markers, spread along the whole route), 19 × `0x0176` (tag only), one `0x07A4` at (-62.9, -0.3, -27.2)
and one `0x1B3E`. None names a graph, so the omnics, the terminal and the payload are entities the server
spawns (`0x02E` hero assets 0x165-0x17C, 0x1AC, 0x1B8, 0x1BA, 0x1BB, 0x1C7, 0x1CE, 0x1F9, 0x21D are the 24
non-playable heroes, 312-536 bytes each; names not decoded).

## The omnics spawn (2026-11-05, late)

- `target.omnic_bots()`: four Nulltroopers (hero `0x02E...179`, body `0x040...184A`) and two Slicers (`178`/`1842`) as
  TrainingTargets on team 1 at the first six of the 68 `0x16DC` markers nearest the players' point (-42.2, 0.2, 2.3);
  the vs-AI hero bots are not created in the PvE modes. The markers come from the placeable chunk through
  `tools/assets/map_markers.py` into `tools/lobby/data/pve_markers.json` (nearest: (-64.0, 0.0, 5.2) and
  (-46.3, -0.1, -19.6), 22 m away).
- The client needs the omnic heroes in login 20505's hero list, as for the vs-AI bots: without them its log says
  `Missing entity definition: 0x040000000000184A` the moment the first body arrives and it leaves the match 3 s
  later (back at the difficulty menu, no "FIN" reason beyond that). With them the units render: a purple Null
  Sector trooper with a red eye stands in the corridor west of the spawn room and fires (`main-menu/arcade-
  uprising-omnic.png`, `-zoom.png`).
- Without the map's collision mesh the units shot through the walls (the match sees everything when
  `mesh is None`): `run.py` now extracts `06FE.collision` (714k triangles, 1 s) for the handoff maps too. With it
  only the corridor unit reaches the player at spawn; Mercy still dies in about 30 s standing there (six Soldier
  rifles at Normal scale, real unit stats unread).
- The units walk once the map has an objective: `pve_map` sets the players' spawn point as the one objective, and
  the vs-AI steering (ring round the objective, chase a fresh sighting within LEASH, strafe in a fight) brings the
  two corridor-side Nulltroopers into the spawn room within seconds, where they strafe and fire
  (`main-menu/arcade-uprising-omnics-walk.png`). The nav grid over the 68 markers' box (190 x 95 m plus margin)
  builds in about 30 s and is cached as `data/06FE.nav`; the first omnic's region is connected to the spawn room
  (a 23-waypoint path). The other four stay out of sight behind the route's walls until they see someone.
- Stats (guessed, `target.OMNIC_KIT`): Nulltrooper 200 health / 6 damage per 0.4 s, Slicer 150 / 3 per 0.1 s
  within 10 m, Eradicator 500 / 15 per 0.25 s, Detonator 150 / the Nulltrooper gun. Medium difficulty halves the
  damage: idle Reinhardt (500) at spawn lived about 21 s under the whole wave; a 200-health hero about 8 s.

## The objective HUD draws (2026-11-05, late): the HUD bootstrap graph 0CB6 was missing

- Why nothing drew (IDA, UXPresenter class `7A7F2732`, vtable `0x7FF6D3856898` in the dump): a presenter state does
  not present into a scene by itself. `Begin` (`0x7FF6D1F797D0`) writes a handle (4, instance, state) into the
  dynamic variable its node names (field `45017CEA`), and `Present` (`0x7FF6D1C10F30`) registers the state
  (`0x7FF6D1C0EFD0`) with a per-owner registry: `+0x48/+0x50` registered, `+0x18/+0x20` orphans, `+0x28/+0x30`
  roots. A child presenter (state `+0x238` child-mode, `+0x220` key) names a parent through its target node
  (`+0x130` = `45216F79` -> `99CEEC04`, whose `11B47C68` ref is the parent-handle variable; runtime `+0x18`) and
  stays an orphan, never drawn, until a presenter publishing that variable exists (parent lookup `0x7FF6D1C0F6D0`).
  Live (unicorn over copied memory, `parent_emu.py` in the scratchpad) every 0E95 presenter sat in the orphan list.
- 0E95's presenters name var 19450 (`0x0D80000000004BFA`). The install's publisher of 19450 is the objective frame
  graph **106B** (`findvar.py`), and 106B is created client-side by the HUD bootstrap graph **0CB6** (all states
  client-only; it also creates 2056, 0EC8, 1FC0 and 0DEA on the mode entity; entity vars 12992, 11428, 11429,
  10121, 11404-11408, 1844, 11445, 9879-9881). In Assault the chain is F4A -> 1FEB (client-only SubScript) -> 0CB6,
  so the client builds it alone. In Uprising 0D28 reaches 0CB6 from **state 12, a server-owned SubScript**, so the
  server has to send 0CB6 as a child instance in the frame; it never did, so 106B never existed and the widgets had
  no parent. (Earlier probes had tried 0CBB, the announcer, not 0CB6.)
- Fix (`match.pve_instances`, in the gate frame): instance 2 = 0CB6 under (1, 12), instance 3 = 0E95 under (1, 31),
  instance 4 = the director 0D4A with remote slot 0 on (74E.025 opens the widget). Live: **"DEFEND THE PAYLOAD /
  0.0 REMAINING"** top centre and **"PAYLOAD HEALTH: 0%"** top right, from spawn on
  (`main-menu/arcade-uprising-objective-hud.png`). 0D28 slot 1 (694) is not needed for it.
- Open: the label and variant (the 0x07E switch, var 27183) and the "0.0 REMAINING" / payload-health values, which
  106B binds from the mode entity's vars (9879-9881, 10121, 12138, 2190); the real phases from 0D4A's slots;
  Retribution's 100B and Snowball's 0CE4 reach 0CB6 at states not yet looked up (`scan_subscript.py` finds them).
- The downloaded wiki page (`research/wiki/Overwatch+Wiki-20261106023659.xml`, "Overwatch Uprising") is the event
  announcement: dates, story, loot boxes. No unit stats, wave counts or payload timings in it.

## The HUD's values (2026-11-06): the widget reads one entity, var 2190

- A UXPresenter's widget node (`B317E7B5`, the 0x0A88 widget asset) is followed by `E870E3E8` bindings: field
  `07DD813E` = the value node (always the next node), `11B47C68` = the widget's data key (a dynamic-variable id the
  0x0A88 schema declares, as `research/numeric-health.md` found for the player card). Widget 0x2B2 (the top text)
  binds key 388 to a formatted display text (`E7579408` -> `ConfigVarUXDisplayText` 0x0DE...5BF0, outside the root
  CMF) whose one argument is the expression `08 00 08 01 1a 00` = var 10371 / var 10372 (VM case 26 at
  `0x7FF6D2DA2997`: a divide that yields 0 when the divisor is 0; case 24 multiplies), key 22751 to the string
  0x0C88...1F ("DEFEND THE PAYLOAD"). Widget 0x2CE (PAYLOAD HEALTH) binds 13684 = 1, 24878 = var 10371,
  17772 = var 10372, 25269 = 250.0. The 0x07E variants (presenters 13-19, strings 0x0C88...1E) hang off the
  694 path and its switch, so the label is fixed by which message opens the widget.
- 10371 and 10372 are written by 0E95 itself: states 6 and 26 (`StateStack` 594C2D80: `592D15F7` = target var,
  `07DD813E` = value) set them from `9B7648C3` and `60146072` over **var 2190** (`0x88E`, the graph's one synced
  entity var; 0x2699/0x2789/0x2F6A are its presenters' handle vars, and writing them from the server removes the
  widget). Live: with 2190 = an omnic's entity the panel read "PAYLOAD HEALTH: 100%" with a full bar, so the two
  ops are the entity's health and maximum, read natively on the client. Entity vars sent in the frame only land
  on variables some graph on the entity declares (1303 by 0D28, 10548/3392 by 0F4A, 1732/1737 by 463); 10371 and
  10372 are declared nowhere (`scan_decl.py`, `scan_cvd.py` over all 1586 graphs), so they cannot be sent.
- Served now: a payload stand-in, `target.payload_target` (a Range training bot, team 0, 1500 health, at the
  players' point, behaviour `payload`: never shoots, the omnics target it like a hero bot), and the gate frame
  carries `DEFENDED` = its entity once the client has it, keeps going after assembly, and only then turns the
  director's slot 0 on. Live: "PAYLOAD HEALTH: 95%" with the bar after half a minute under fire
  (`main-menu/arcade-uprising-payload-health.png`).
- **The "REMAINING" timer is the mode entity's v1303**, the same countdown the hero select shows (0D4A declares it
  too): with v1303 run from 240 after assembly the widget reads "DEFEND THE PAYLOAD / 3:58 REMAINING"
  (`main-menu/arcade-uprising-defend-timer.png`), so `DEFEND_SECONDS` = 240 and the PvE countdown frame keeps v1303
  going. The expression behind key 388 (10371 / 10372) is not it: those two ops read one obfuscated-key
  component of entity 2190 (delegates `off_7FF6D3883B28` / `off_7FF6D3884298`, evaluators `0x7FF6D1F0B6C0` /
  `0x7FF6D1F112A0` -> `0x7FF6D1F377C0` / `0x7FF6D1F37AE0`: entity lookup through context+0x10 vtable+0x168, the
  component by the key pair 0x9B5175927A195769 / 0x190F02EA706F9322, then its vtable +0x40 and +0x50 as a float;
  0 when the entity has no such component). What that component is stays open; it is not needed for the timer.
  What the real director feeds 2190 (the real payload entity, which is not in the map's placeables; 0x07A4 beside
  the spawn is 288 bytes with no model) is still open, as is the 694-path label switch (var 12140 and 1030).

## What the real match looks like (2026-11-06, gameplay video)

Source: a 15-minute "Overwatch Uprising 2018 Gameplay (Full Match)" video (Hard, four players; `/watch`, HUD strip
every 5 s in `.scratchpad\watch\sheets`). The HUD, top centre unless said, by video time:

| Time | Objective widget | Other HUD |
|---|---|---|
| 0:00-4:15 | **HACK THE TERMINALS**: three hexagon slots; the active one shows the hack percentage (0 -> 100 in about 90 s), done ones a yellow tick, later ones a padlock | top left TEAM SCORE with four player cards (ult %, health bars), star rating; top right PAYLOAD HEALTH not yet shown |
| 4:20-4:45 | **RENDEZVOUS AT THE PAYLOAD**: four person icons fill as players arrive | |
| 4:50 | **OBJECTIVE COMPLETE** banner (yellow, tick) | |
| 5:00-9:00 | **DEFEND THE PAYLOAD** with **3:59 REMAINING** counting down from 4:00; under 10 s the number shows tenths: "10.0", "5.0", "0.0" | **PAYLOAD HEALTH: 100%** top right with a bar, 100 -> 91 over the phase; kill feed |
| 9:05 | PAYLOAD HEALTH BONUS! +820 PTS in the score box | |
| 9:10-11:45 | **ESCORT THE PAYLOAD**: a wide progress bar with the payload marker, a red bubble with the number of enemies near it, "CONTESTED!" below while they are | PAYLOAD HEALTH 91 -> 79 |
| 11:45 | OBJECTIVE COMPLETE | |
| 12:05-end | **KILL THE OR14-NS UNITS** with **5:58 REMAINING** (6:00 timer) and **0/4 ELIMINATED** counting up | PAYLOAD HEALTH BONUS! +706 PTS; the panel goes away |

So the widget our server opens (the 74E path: "DEFEND THE PAYLOAD / 0.0 REMAINING" plus PAYLOAD HEALTH) is the
third phase's, and its number is a **seconds timer** (the "0.0" is the under-10-s format at zero), not a health
ratio: the real payload entity carries a 4:00 timer the two ops (`9B7648C3`, `60146072`) over var 2190 read, while
the health panel tracks the entity's health. Assault's 0F4A uses var 2190 the same way (expressions, a StateStack,
a SendGameMessage to it), so 2190 is "the objective entity" and the ops are its native objective values. The
hack, rendezvous, escort and kill widgets are the 0x07E variants and 0x2CC on the 694 path, picked by the switch.
Next for the HUD: give the stand-in whatever component the ops read (the objective component the Assault point
entities have), or drive the 694-path variants with their own values.

## The phase is the director graph (2026-11-06, late): 0D4A defends, 0D4B escorts

Live probes with the gate-frame hook, the defend timer at 0 (scratchpad `gate/p*.png`):

- Turning 0D28's slots 1 (694), 3, 8, 9, 11+12, 13, 35 (A90) or 37 (717) on, with or without the director's
  slot 0, changes nothing in the objective HUD. The director's slot 0 off, or the 0E95 instance dropped from the
  frame, removes only the PAYLOAD HEALTH panel; the HUD bootstrap 0CB6 dropped removes everything; **the director
  0D4A dropped removes the top "DEFEND THE PAYLOAD / 0.0 REMAINING" box**.
- So the top box is the director's own, not 0E95's presenter 1: 0D4A's plain entry (node 3256) runs two
  client-only presenters at once, state 106 = widget 0x2CB (the timer: key 13904 <- a `108981C8` over v1303, key
  24854 <- a display text) and state 107 = widget 0x2CF (the title: key 388 <- display text `afff96b9...`, key
  22751 <- op `BCD1C634`), plus a 2 s Wait into a `100EF8C2` post of message FCE with the same text (the phase
  banner). Its remote slots: 0 posts 74E (the health panel), 1 ChaseVar on v1303, 2 StateStack on var 7898, 3
  DataFlowMapping on v1303, 9 a `9D7BF987` over 9879 and 1303 (4-8 silent).
- The other two 74E posters are the next phases' directors. **0D4B** (39 states, 0D1C and 1824 SubScripts,
  entity vars 9879 and 16927, instance vars 624, 6627, 2190; remote slots 0 = 74E, 1 StateStack on 6627, 3
  `691BFA55`, 5 `CD46AF93` over 2190 and 12903, 6 `9D7BF987`; 2 and 4 silent) swapped in for 0D4A (a new
  instance index, so the client destroys 0D4A) draws **"ESCORT THE PAYLOAD"** with the progress bar, the payload
  marker at the start and the checkpoint tick, and its slot 0 reopens PAYLOAD HEALTH (`gate/ph_d4b.png`). 0FEF
  (9 states, widget 0x2C8 only, entity vars 16927, 11057, 9879, declares 9472) swapped in after it left the escort
  box as it was; what it is (the kill phase? a helper) is open.
- Served: `match.pve_instances(..., escort=True)` once the countdown frame's v1303 reaches 0 (`_send_countdown`:
  `escort=now >= ends`), instance 5 = 0D4B with slot 0 on while the stand-in lives. The bar stays at 0 (the
  stand-in does not move) and nothing ends the escort phase.
- Still unknown: the hack and rendezvous phases (0E95's 694 path and its switch over var 9472, which 0D29 feeds
  by relaying a message's payload as 8C2; the 0x07E variants bind keys 1030, 11947-11949, 17560-17562), the
  OBJECTIVE COMPLETE banner and the PAYLOAD HEALTH BONUS, and the kill phase's director.

## Next

1. Mission phases: move the escort bar (0D4B's instance vars 624/6627 or its slots 1/5), end escort, find the
   kill-phase director (0FEF?) and the hack/rendezvous widgets (the 694 path), the Retribution/Snowball 0CB6 states.
2. Find what sets var 27183 and the 0x07E variant switch.
3. Omnic waves from 0D4A (which hero, how many, which markers per phase), their real health, weapons and models
   (the `0x0240` message texts and the bodies are outside the root CMF), and routes for them (objectives or
   marker-to-marker patrols, so the nav grid has points).
