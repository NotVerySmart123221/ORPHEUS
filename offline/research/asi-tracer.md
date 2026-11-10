# In-client tracing with ASI plugins (2026-11-06)

What the client does with the lobby's bytes could only be guessed from memory reads, and every debugger or
debug-register route crashes the 1.74 client (`soldier-runtime/helix-probe-asi.txt`). `tools/asi/tracer.asi`
logs it from inside instead, with the vtable-copy trick `retail_menu.asi` already uses: heap objects get a copy
of their class vtable with one slot replaced; the image itself is never written. Client 1.74.0.0.104319,
addresses from the IDA dump (base `0x7FF6D1220000`).

## Hook points (all virtual, so a vtable copy catches them)

| What | Where | How found |
|---|---|---|
| Every server message | JamMessage dispatchers (`sub_7FF6D19F2000` handles 20300..20322, 51 such switch functions share the string "Unhandled JamMessage, unable to dispatch") are registered by `sub_7FF6D19F2E10` as 48-byte delegates: vtable `0x7FF6D38368D8`, +24 the dispatcher, +16/+40 its owner. Slot 2 (`0x7FF6D205C210`) invokes +24 with (owner, message). Message id = u16 at +28 of the header returned by the message's own slot 2 (`sub_7FF6D1785260`). | xrefs to the dispatcher: a `lea` in the registration, not a call |
| Statescript state begin / end | State activation `0x7FF6D2D90790` and deactivation `0x7FF6D2D90480` call the instance's script system (`*(instance+16)`, vtable `0x7FF6D384C010`): slot 32 (`0x7FF6D1B9A1E0`) on begin, 33 on a per-frame refresh, 35 (`0x7FF6D1B9A310`) on end, all with (system, instance, state item). The system is the object the statescript world (`0x7FF6D385C160`, at system+1472) keeps at +80; its instances are at +8 (count +16). | activation decompile; world ctor `sub_7FF6D1C27F30` |
| The bits a state reads | `sub_7FF6D2D92060(item, reader, create_flag, 1)` = base part + **slot 43 of the state's class vtable**. The frame reader keeps the qword array at +0 and the bit position at +28 (`sub_7FF6D19ABDB0` reads n bits LSB first). Called from the remote-state reader `0x7FF6D1C2BE30` after the state is activated, so the begin hook can swap the item's vtable before the read. | `sub_7FF6D2D92060` decompile |

Layouts used: instance `+24` u16 id, `+0x1E8` graph GUID, `+0xA0/+0xA8` state items, `+16` system, `+92`
per-instance trace flag (the game's own trace output, slot 58 `0x7FF6D2DADFB0`, is an empty stub in retail);
state item `+64` flags (bit 0 active), `+112` node index, `+120` instance, `+0` class vtable. State items are
created by `sub_7FF6D2DB1460` from the class registry (`sub_7FF6D2E09EA0`, names all "Unknown").

The state-data path of `sub_7FF6D1C2BE30`: presence bit, start bit (custom classes: control bit, desired-active),
activate if needed (begin hook fires here), then `sub_7FF6D2D92060` unless the activation consumed the data
(`world+2320` cleared). So a `data` line's `at=` is the absolute bit position of that state's data inside the
frame, after the client's own reading of everything before it: the number to compare with the server's encoder.

## Rescan and its limits

Objects are found by scanning private read-write memory for the vtable value. A full pass on a loaded game took
45 s; skipping regions over 64 MB (asset caches) brings it to 0.4 s, run every second. Missed by design: the
first message of a family (20300 creates the handlers), and state begins in the second before a new system is
found. When a system is found, the items its instances already have are shadowed, so their later reads are still
dumped. Two crashes on the way: a 64-slot class vtable copy (a state class has more slots; 256 now, clamped to
the mapped region) and the 45 s pass (new systems were found too late).

Entity destroys can stall (2026-11-09): destroying a viewer mannequin sometimes left the client never acking that
datagram, so the server resent the destroy forever and the body stayed. First blamed on the tracer (seen on Mei and
Roadhog in traced sessions); later reproduced without it. The cause was the server: the new bot's 20301 with the
frame's other records made a datagram over the size the client takes, which it drops unacked, and every resend was the
same (research/bot-fire.md). The tracer is not involved.

## First results (Uprising Normal, `research/asi-tracer/uprising-2026-11-06.log`)

- Messages after the login: 20301, 20302 once; 20306 about once a second (654 in 10 min); all on dispatcher
  `img+0x7D2530` (`sub_7FF6D19F2530`). 20300 missed (see above). Result 1 for every message.
- `0D28` state 29 (the loading marker, class `img+0x27AC800`) reads **0 bits at frame bit 231** in the
  countdown frames: confirms `uprising-mission.md` ("the loading marker takes none").
- `0C90` (controller) node 1 reads 0 bits at 276, node 119 (class `img+0x265B9C8`) **6 bits `001000`** at 378
  of the create frame.
- `0D4A` (director) node 15 (class `img+0x263AC80`) reads 1 bit `0` at 316; its node 14 ends and 15 begins
  at the hero select.
- `0E95` (objective widget) states 0, 1, 2, 6, 26 begin together on spawn (flags `0x4020`), when the HUD opens.
- UX graphs dominate the begin/end stream (`1421`, `0225`, `0B7A` loop every frame); filter by graph.
- The classes seen so far: `27AF738`, `27B03B0`, `265FE20`, `2636898`, `2672478`, `2673C30`, `263B7E8`,
  `263AC80`, `2630450`, `2671B20`, `2637310`, `2620438`, `262F640`, `27AC800`, `266B7B0`, `265B9C8` (71 shadowed
  in the run). A class hash is read from the graph data by (graph, node), not logged.

## Data watchpoints work after all: PAGE_GUARD (`tools/asi/guard_probe.asi`)

Every debug-register route crashed the client within a second (`soldier-runtime/helix-probe-asi.txt`), but a
`PAGE_GUARD` watch does not: the probe guards the page of a given address, logs each `STATUS_GUARD_PAGE_VIOLATION`
(instruction, address, read/write), sets the trap flag to re-arm after the instruction, and the game keeps
running. Checked on a UX statescript instance (`graph 1215`, page `0x2908C3D6000`): 300 hits in a second, from
`img+0xA0E65B`, `img+0x23DC640`, `img+0x1B9270B..799`, `img+0x97B228..251`, `img+0x99C9E4/CAA4`, then the game
went on. The granularity is the page, so the log needs filtering by address; the single step costs two faults per
access, so watch a quiet page or expect a slower frame. This is the tool for the open "who writes Volley `+0x1EC`"
and "what does Helix creation return" questions (`CONTINUE.md`): write the live address to `build/guard_probe.addr`.

## How to use it

```bash
OW_PVE_HANDOFF=1 python tools/lobby/run.py <game-root> --asi tools/asi/build/retail_menu.asi --asi tools/asi/build/tracer.asi
```

Menu path for Uprising with background clicks (1274x720 window): PLAY (68,160), Arcade (527,360), Mission Archives
(520,275), Uprising (Story) (338,260), Normal (309,360), CONTINUE on the hero select (628,636). Then read
`tools/asi/build/tracer.log`: `grep "graph=580000000000D28"` for one graph, `grep " data "` for the bit dumps,
`grep " msg "` for the messages.

## Second run: the Range as Sombra and Moira (`research/asi-tracer/range-sombra-moira-2026-11-06.log`)

Launched without plugins (`enter_range.py` drives the Pod screens), the tracer loaded in the Range with
`load_asi.py` plus its `.go`; heroes picked with a background `H`, portrait click and SELECT (`weapon_sweep.PORTRAITS`;
the foreground key path found no foreground window). Owner full frames read one bit per owner state, then the
prelude and DSR: Sombra's `0B1B` states 0..77 line up with `kits.KITS[SOMBRA]` one by one, with the newly sent
TrackRay (node 32, 0 bits at 1084) and `STU_DEBD057B` (node 68, 0 bits at 1110) in place and the switches 75/76 and
the Stack 77 (`10`) after them reading the kit's values. Moira's E: graph `10E4` StateAbility 0 (2 bits idle, 44 with
the timing block), Volley 10 (33 bits), Anim 13 (5), Wait 16, `STU_DEBD057B` 17 (0 bits), base state 18 (0 bits), one
framing bit apart. Decodes recorded in `statescript-payloads.md`. Screen capture was black in this run (PrintWindow),
so the orb's flight is unseen; the client kept running.

Third run, Reinhardt's Fire Strike (`range-reinhardt-2026-11-06.log`): graph `0E2` after E reads Volley 0 (33 bits at
1624), Wait 1, Anim 3 (5), Wait 4, `STU_54FEB21C` 5 (0 bits at 1667), StateAbility 9 (44 bits at 1679), base state 10,
Wait 16: the whole cast path one framing bit apart.

Fourth run, Moira's and Zenyatta's right mouse (`range-moira-zenyatta-2026-11-06.log`; tracer `node` = full state
index, which equals the owner index in `0164` and not in `0F76`):

- Moira (`0F76`): on the press the client predicts the whole beam itself (flags `4060`/`4020` before any server data):
  Stack 27, state 25, TrackTargets 96, ChaseVar 59, switches 85, 91, 22, and switch 22's **false** branch,
  `STU_9932D728` (owner 30, node 32) with effect 47. `STU_9932D728` is the beam: effect `0x1F12` from hand hardpoint
  `0x0A` to the entity in var1181 (instance-synced, presence bit 12). The server's frame sends switch 22 = 1: effect 75
  begins, reads `10` and ends in the same frame, and the client re-enters the false branch (switch 22 is evaluated
  client-side). Sending var1181 and var375 (the effects' target vars) as the bot's entity, resending the frame every
  0.192 s, or pointing them at Moira's own body: the beam state stays active for the whole hold, nothing draws, and the
  hand's beam pose does not play either. Same as Sombra's tether and Mercy's staff (`roster-animation-audit.md`):
  every target-to-entity beam is still missing. Not yet known: whether the Entity value reaches the instance var
  (the tracer does not log vars) or the effect needs something the bots lack.
- Zenyatta (`0164`): on release the server's frame begins Volley 18 (33 bits) and switch 22 (`0`); the volley ended in
  the same frame, its end set var173 = 0.6 (recovery), switch 10 flipped true and the idle subtree (buttons 11 and 16,
  switches 15, 65, Stack 56) ended again every frame. The volley fires var1945 orbs; the client computes var1945 from
  var1392 in 22's onFalse action chain, after the server-started volley has begun. Sending var1945 (presence bit 1)
  and var1392 (21) with the weapon instance: the volley runs (47 ms here), the idle subtree stays, and the orbs draw.

Fifth run, Moira's beam gate (`range-moira-locked-2026-11-06.log`): switch 22's condition is `var38 && var2318 &&
var375 && filter(var375)` (relationship `0x0D6000000000003A`). var38 and var2318 come from client Stacks 103/104:
var14517 (the entity TrackRay 21 hits) if it passes entity filters 64 and 128. var375 is TrackTargets 96's output.
TrackTargets' radius and TrackRay's range, var10881, are set only by Stack 110 on the graph's Entry chain (Entry ->
SetVar var12389 = 0.5 -> Stack 110), which no kit sent. The client finds none of these itself, and our bots may simply
not be hit by its ray or not pass the filters. With var375, var38, var2318 and var14517 sent as the bot (and Stack
110), switch 22 holds true: effect 75 (`0x201B` at var375) begins and stays, and the false branch no longer runs. Still
nothing is drawn, and the right hand's beam pose does not play either, so a second layer is missing. Leads: the beam's
effects (`0x201B`, ActionEffect `0x201C`) bind their endpoint to an entity; the pose may come from the body's
animation-category variables, as Mercy's staff and the scopes needed (`kits.frame_builder` vars 1226/1273). The same
gate pattern (TrackRay hit -> filtered entity var) is worth checking in Sombra's `0B1B` (owner states 49-52 are
`STU_9932D728`), Brigitte and Mercy.

Sixth run, the beams draw (`range-moira-beam-2026-11-06.log`, `range-sombra-tether-2026-11-06.log`). The missing
second layer was the beam states themselves. `STU_9932D728` is the beam: effect `3AA1ACFD`, source hardpoint
`F745334E` on the owner, far end the entity in var `2FE19ED6` (or an expression `FB9FD621`) at hardpoint `0B057C92`.
Its DSR (vtable `img+0x262D1A0` `+0x158`) is the empty stub, so it takes no payload bits. The client begins a child
owner state only when the server's frame lists it, even under a switch it evaluates true itself; switch 22 (full 24)
being true started only the effect the kit already sent.

- Moira (`0F76`, full -> owner index): switch 24 -> 22 true runs the action `0FF09DD0`, effect 79 -> 75 (`0x201B` on
  var375) and switch 102 -> 92 (`var375 && filter(var375)`), whose true side is switch 88 -> 81
  (`cv(1FF67CA6) == op11(var375, 0)`, not decoded) -> beam 89 -> 82 (`0x2061`) or beam 25 -> 23 (`0x1F31`, hand `0x0A`
  to var375's `0x01`). Switch 102's false side is two fallback beams to var432 (107, 111, gated by var38 / var2318),
  also `0x1F31`. Sending 92 = 1, 81 = 0, 23: the beam draws on the target for the whole hold.
- Sombra (`0B1B`): the hack channel is Wait 54 (owner 46, timeout var670 = 0.65 s), started by the action chain after
  `STU_3387AB5D` 1668. Its subgraph holds beams 59..62 (owner 49..52; `0x1AC7` from hardpoint `0x21`, `0x1B30`..`0x1B32`
  from an expression-picked hardpoint) to var8909 (instance var, presence bit 29). Sending 49..52 and var8909 = the
  target: the tether draws for the channel and the target shows hacked.

The other three beams differ:

- Zarya (`0241`) and Symmetra (`157C`): the beam is client-only (`STU_9932D728` 15 / 83) under a client-only TrackRay
  (14 / 104) under `STU_BD02E168` 85 / 122 (owner 67 / 94; empty DSR: registry key RVA `0x2DF2F28` -> constructor RVA
  `0xA06370` -> vtable `0x7FF6D3857310`, `+0x158`/`+0x168` = stub). Sending `BD02E168` while firing runs all three
  (Zarya: begin to end 0.6 s, no assert) but at first only a glow at the barrel drew. Source: var49 (weapon model,
  hardpoint 6), sent. Far end: `var8154 ? var432 : (var16633 ? var1180 : var432)`, TrackRay 14's outputs (432 = hit).
- Seventh run (`range-zarya-symmetra-beam-2026-11-06.log`): **both beams draw** once the server sends var1648, the
  TrackRay's length. TrackRay does not cast on its own: each tick its update (`sub_7FF6D1B17580` ->
  `sub_7FF6D1B19270`) looks up the ray recorded for that tick in its list (`+0x210`, 64-byte entries: tick, origin,
  direction) and, when there is none, records one from the node's origin/direction (`+0x1D0`/`+0x1E0`) and processes
  it (vtable `+0x1E0`, results at `+0x368..+0x464`). The direction block (`STU_9E9028E6`) reads its length from
  var1648, which only the Entry chain's Stack sets (Zarya Stack 80 / owner 66, Symmetra 119 / 91) to
  `15 × STU_FDBB0B67(self)` (Symmetra 12 ×): a per-entity factor that evaluates to 0 for our player, so the ray had
  length 0. Sending that Stack ran it (2 bits, aligned) and changed nothing; var1648 is an instance sync var (Zarya
  bit 19, Symmetra 21), so the kit now sends it as 15 / 12 m (`kits.BEAM_RANGE`). How found: tracer begin lines now
  carry the state object (`item=`), and `guard_probe.asi` takes `ADDRESS LENGTH` to log only one object's accesses;
  watching the beam showed the effect created (handle at `+0x210`, `sub_7FF6D1BA9180`) and updated each frame, and
  watching the TrackRay showed the ray recorded and processed, which left the ray's length.
- Moira's unlocked free beam (STU_9932D728 30, to TrackRay 29's hit) likely has the same zero range (her Stack 110
  sets var10881 the same way); unchecked.
- Mercy's staff (`006B`): switch 38 (heal / boost) -> switch 32 / 34 -> `STU_431F3FCC` 33 / 35, which has no runtime
  factory (registry key RVA `0x2DE0CA8` not in the constructor table) and names a status asset, `0x04A...19` (heal) /
  `...16` (boost): `STU_76F2C784` {graph `0x0079` / `0x006C`, 5.0}. The heal status graph `0x0079` runs on the healed
  entity; Entry -> `STU_3387AB5D` (target != local player) -> client-only beam 2 / 16 (effect `0x1F5`, owner var27,
  from `var27.var49` hardpoint 6 to the entity's hardpoint 1). Sending a `0x0079` instance on the ally bot with var27
  (bit 5) = Mercy's body draws the beam to the bot, but its near end is not the staff: `var27.var49` does not resolve
  to her weapon model from another graph. Next: what var49 holds on her entity in retail (the CosmeticEntity's
  entity; ours is a StateBound into the weapon graph), or the descriptor's `source_entity`.
  - 2026-11-07: the near end is the world origin. From the Range spawn (54.6, 1.0, 42.1, yaw -170.3) the origin is
    69 m away, 43 degrees left of forward, 2.6 m below the eye: about (156, 370) on screen, where the beam ends.
  - Expression VM opcode 0x0F (case 15, `0x7FF6D2DA1CE9` -> `sub_7FF6D2DA0C30`) takes the entity id under it via
    `sub_7FF6D2184C20`: kind 10 (entity id) as is, kind 12 (array) its first element, anything else invalid. A valid
    id: entity by id (vtable +368), its statescript component (+376), then the var from the component's store (+496)
    through `sub_7FF6D2DC3800`. An invalid id falls back to reading the var on the current object
    (`sub_7FF6D2185300`). Our var49 is native kind 7 (state reference, `research/rifle-initialization.md`).
  - Branch condition 323 is `0E 00 0E 01 26`: `STU_B5A0CAF0` (self) != `STU_1FF67CA6` (local player) -> beam 2 (state
    index 2, effect `0x1F5`), else beam 16 (another effect). Both read `var27.var49` and use the same hardpoints.
  - var27 is presence bit 5 (instance-scope sync vars: 336, 1606, 40, 41, 393, 27); no state in `0x0079` writes it.
  - Live runs (ally Soldier mannequin, staff held 0.6 s), all with the same near end at the origin:
    1. Mercy's var49 not sent: the staff model still draws (var49 is not needed for it).
    2. Mercy's var49 = `Entity(her body)` (body frame resent on target change).
    3. The healed bot's entity var49 = `Entity(her body)` (the fallback path).
    4. Mercy's var49 = `StateBound(Entity(her entity | 0xF0), 1, 0, 6)` (a named staff entity).
  - So no entity we can name gives the beam a source position: either the beam's source resolution is not this
    expression as read, or hardpoint 6 is on none of these entities and a missing hardpoint gives the origin.
  - Traced in memory (`tracer.asi` evaluator probe, 2026-11-07). Beam state: class key RVA `0x2DE8538` ->
    constructor `0x7FF6D1BA8310` -> vtable `0x7FF6D384D1A0`; its own slots 16 (`0x7FF6D1B9FB90`) and 17. The
    descriptor is the graph's node array entry (`sub_7FF6D2D91C90`); config vars evaluate through
    `[field + 8]` slot 4. Beam 2's fields: +0xF0 effect (`0x1F5`), +0xF8 owner var27 (dynamic var evaluator
    `sub_7FF6D2D96650`; required, an invalid id ends the beam), +0x100 source expression, +0x108 hardpoints (6, 1),
    +0x120 self entity, +0x148 a bool.
  - Cause: +0xF8 evaluated to the healed bot (`B0004001`), so var27 was not ours, and +0x100 was kind 255 (none).
    A marker entity per presence bit (`A00001F0 + bit`) showed var27 is presence bit 3 of graph `0x0079`, not 5
    (`graph_layout.py`'s slot order: 336, 1606, 40, 41, 393, 27). With bit 3, +0x100 is Mercy's var49 as sent:
    kind 7, a state reference (instance 1, state 0), and the beam starts at the staff tip. Kind 7 resolves fine
    here; the opcode 0x0F note above is about kind 7 in its entity-id step, not the final value.
  - Open: the rule behind `0x0079`'s presence order (bag records and flags do not explain it; `0x0D28`'s count is
    off by one too).
- Mercy's damage boost and the staff (2026-11-07, `range-mercy-staff-2026-11-07.log`, `006B` / `006C` begins):
  - Boost: switch 38 false -> Stack 31, 72 (var690), switch 34 -> `STU_431F3FCC` 35 (status graph `0x006C` on the
    target), Wait 43, `BD02E168` 76, effect 36 (the blue glow at the tip). Heal mirrors it with 30, 71, 32, 33, 42,
    75, 37 (`kits.mercy_staff_states`). In `0x006C` var27 is no sync var: `extra` variables did not reach it (beam
    from the origin), the instance descriptor's `source_entity` did (var27 falls back to it).
  - Staff animation: graph `006B` has no `StateAnim`. The staff model (entity `0x040000000000185F`, blend-tree set
    `0x0040000000000071`) reads DataFlow channels (`048…`), which client-only `DataFlowMapping` states answer with
    graph vars: states 22 / 23 answer with var689 / var690 (heal / boost on). They sit under `BD02E168` 74, which,
    with `BD02E168` 77, is in state 1's true subgraph next to states 0 and 2. The kit sent only 0 and 1, so the
    client never started 74 / 77. Sending owner states 51 / 54 (74 / 77) opens the staff head for the beam (wider for
    boost, mode light turns to the blue bolt) and closes it on release; the staff tip also glows at rest now.
  - Not needed for that: switch 53 under 77 tests var233 (Stack 7 under switch 28, `var699 && var414`), so switch
    55 (var689) and its `STU_F4876F8D` states 54 / 56 still do not begin.

Recipe for a beam: find the `STU_9932D728` states (`graph_layout.py --states`), their parents (`graph_edges.py`), the
far-end var and its sync slot; send the states the client won't start itself and the var. The same holds for every
non-client state in a subgraph the kit enters: the client starts none of them itself (Mercy's 74 / 77).

Next: the right-mouse routes `weapon_kit.alt`
cannot find (Mei, Zenyatta, Moira: no LogicalButton 29 or no Volley under it); log the per-frame refresh (slot 33)
when a state's re-read matters; find the client's send path for Ashe's primary fire (not in this tracer).
