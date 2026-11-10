# Visible bot fire (2026-11-02)

Build 104319, image base `0x7FF6D1220000`. The Range shooters (graph B92 → BA3) fire through `STUStatescriptStateWeaponVolley`. This note records why their shots were invisible and what the server now sends instead.

## Result

- The bot's Volley runs in **projectile-entity mode**. Each shot is a networked entity, not a client-drawn tracer.
- Projectile entity `0x0400000000001476` holds only a projectile-visual component, a model component without a model, and child `0x040000000000148D`.
- The child holds only an `STUEffectComponent`, and it is what the client draws: an orange orb.
- Created from the server, `0x148D` renders. `0x1476` does not.
- The client ignores movement records for `0x148D`, because it has no mover.
- The server therefore destroys the orb every frame and re-creates it one step further, using the next entity-id generation (the low byte). In-game captures show orbs flying from the doorway bots to the camera and growing as they close (`.scratchpad/flash6-sheet.png`, session `20261102T031804.046040Z`).

## Volley config: why shot records do nothing

The client keeps one config per Volley key (`state+0x1C0`) in an 8-bucket table:
- lookup `0x7FF6D1FA6660`
- register `0x7FF6D1FA61E0`
- removal `0x7FF6D1FA51B0`

Its pointer chain is `state+0x78 → +0x10 → [+0x528]+8 ^ (rol(G,2) ^ 0xF320E42472A94F14) → +0x28 ^ 0x98DE97B50B3BE1E3 → +0x140 ^ (0x4E3494C633194824 − [0x7FF6D4104BD0]) → [0x22] → +0x20`, where G is `[0x7FF6D41047FC]` (`.scratchpad/read_cfg_table.py`).

Config `+0x11C` is a mode, set in `0x7FF6D1FA7AF0`:
1. an authored override, if nonzero;
2. otherwise 3 when the count `+0x30 ≥ 2` and the spread `+0x28 < 5.5`;
3. otherwise 2 when the speed-like `+0x4C < 1.2e-5`;
4. otherwise 1 when `+0x24 < 8`, and 2 above that.

The live bot config reads `+0x24 = 1`, `+0x30 = 1`, `+0x48 = 35.0`, `+0x4C = 5.0`, `+0x50 = 100.0` and `+0xC0 → +0x20 = 0x0400000000001476`, so the mode is **1**.

- **Remote-shot handler** `0x7FF6D1D07570` (dispatched by `0x7FF6D1D604B0` when the record key equals the state key and the record event equals `state+0x44`):
  - It copies the shot index to `+0x260` and the time to `+0x268`, and runs the per-shot effect list (`0x7FF6D1D064F0`, list `state+0x2F0`).
  - It builds a client-side projectile only when the mode is not 1.
- **Local fire loop** `0x7FF6D1D05D90`:
  - It runs only for mode 3, or when `+0x120` is set and `0x7FF6D2D92470` (owner check) is true.
  - The remote bot has neither.
- **The bot's one effect entry** has a null effect pointer (`+0x20`) and the muzzle hardpoint `0x0DC0000000000005` (`+0x28`). So the per-shot effects draw nothing either.

## Ch2 shot record (`world.shot_record`, sent for hero bots)

Ch2 after its run bit (reader `0x7FF6D1D5A490`): a bit for events (`0x7FF6D1D60130`), then a bit for a state snapshot (`0x7FF6D1D44600`). The events are a list: per event a 1 bit, a time (`var_b` for the first, a delta after), the record; a 0 ends the list. So one event is `1 1 1, var_b time, record, 0, 0`. Writing only two 1 bits (as an earlier note had it) makes the client read the state bit as set and crash in the snapshot parser (null read at 4).

The record is parsed from the same bits through a byte reader (`0x7FF6D1610AA0`, LSB first; the main reader resumes after it). Its format (`0x7FF6D1FA1F80`), in order:

1. shot index: bit 0 + 4 bits, or bit 1 + 8 bits (255 escapes to 32 bits)
2. bit 0: no `+8` word
3. bit 0: keyed by state (1 is keyed by an id, with an optional position)
4. `w_u16` instance
5. `w_u16` owner state
6. 6-bit event (must equal the state's event counter `+0x44`)
7. bit 1: activation 1 (else a `w_u16`)
8. `w_u16` the key's instance field
9. 3×f32 origin
10. 16-bit yaw `(y − 0x8000)·π/32768`
11. 16-bit pitch `(p − 0x8000)·π/65536`, positive **up**: the parser turns the pair into `(sin yaw·cos p, sin p, cos yaw·cos p)` (`0x7FF6D1FD1360`). Corrected 2026-11-09; the lobby wrote it down-positive until then, so every record-driven shot was mirrored up/down.
12. bit 0: no entity list

The key it builds (`0x7FF6D1FA5180`) is `entity << 32 | instance field << 19 | owner state << 8 | activation`, the Volley's own key `+0x1C0` (Cassidy's bots: `0xB0000X01_00080B01`, state 11, activation 1).

## Server model (lobby `target.py`, `match.py`)

- A shooter fires a `Projectile`:
  - from its chest (+1.2 m) to the player's chest (+1.4 m)
  - at the authored 35 m/s (BA3 config var 299; lifetime var 297 = 5 s is never reached within the 30 m range)
- On arrival it deals 20 damage if the aimed point is within 0.1 m of one of Soldier's hit volumes (`hitboxes.md`).
- Approximations:
  - shortcut: the shot's own radius (0.1 m) is a guess
  - no wall collision (roadmap step 3)
  - no lead on moving targets
  - the orb's per-frame re-creates are not resent when lost, so a dropped datagram skips one step
- Entity ids are `0xB1000000 | index << 8 | generation`.

## Hero bots (2026-11-02)

Hero bodies fire through their own weapon graph, sent to each client as a remote (H0) statescript on the bot's body.

- **What makes Soldier's flash** (in-game variants, brightness of a crop over 30 frames; `match.remote_volley_frame`):

  | Frame | Flash |
  |---|---|
  | held + Volley + muzzle effect + rifle presence values | yes |
  | held + Volley + presence | yes |
  | held + muzzle effect + presence (no Volley) | no |
  | Volley only, no presence | no |
  | Volley + presence, no held | yes |
  | held + Volley, no presence | no |
  | Volley + var53 (ammo) only | yes |

  So the Volley needs only its ammo, var53. A graph's presence bits are its instance-scope sync vars in slot order (`graph_layout.sync_vars`); var53 is in every roster weapon graph.
- **Other heroes: the shot record.** The Volley update (`0x7FF6D1D05D90`) runs shots itself only when its config mode (`+0x11C`) is 3: more than one shot per activation with spread under 5.5, Soldier's automatic rifle. Any other weapon (surely the one-shot-per-activation ones: Roadhog, Zarya, Cassidy, Reaper, Torbjörn, Ana, Zenyatta; Lúcio's burst, Mei, Sombra and Bastion depend on their spread, not read) plays a remote shot only from a ch2 shot record (handler `0x7FF6D1D07570`). So each bot shot also goes out as a ch2 event on the bot body (`Match._shot_record`, in its own entity entry, not resent), keyed to the first Volley's owner state, aimed from the muzzle height at the target's chest.
  - Live, Cassidy's Volleys then follow the shot index (`+0x260`), set time and count (`+0x1F0`, `+0x280`), and their per-shot effect entry gets an effect handle (`+0x30`), where before it stayed -1.
  - In game: Ana's purple darts fly from the bots and splash (`assault-capture/bot-ana-darts.png`), Roadhog's scrap bursts (`bot-roadhog-sheet.png`) and Zenyatta's orbs (`bot-zenyatta-sheet.png`) show; Reaper and Sombra light up strongly; Soldier still flashes. No crash with any of the 12. Zarya's one Volley turned out to be her alternate fire's energy ball (exploding orbs), so she is left out.
  - Earlier probes, kept for the record: with only the Volley + var53 frame, Cassidy's scheduler counters (`+0x270`, `+0x278`, `+0x1F0`, `+0x280`) stayed 0 and its per-shot runner never ran; Volley configs: shot limit `m_A17BE89B` 99999 (Soldier, Bastion), 999 (Mei, Sombra), 4 (Lúcio), 1 (the rest).
- **Zarya's beam (2026-11-03, not solved).** In weapon graph `0x0241` (edges: `tools/assets/graph_edges.py 241`), fire button 5 (LogicalButton) runs Stack 40 (remote 23), then Stack 17 (remote 8). Stack 17 starts Effect 7 (remote 2), which leads to Effect 8 (remote 3); a SetVar into server-only TrackRay 43; and state 85 (`BD02E168`, remote 40, no payload). State 85 starts client-only TrackRay 14, with client-only children 38 (`B63ECEB8`), 48 (DataFlowMapping), 64 (`BA4C2296`), 15 (`9932D728`) and TrackRay 84. Both effects play an effect on entity var 49 (the weapon CosmeticEntity's output, state 0, remote 0) at one hard point. Effect 7 also has two hard-point fields.
  - Tried in game, with six enemy Zaryas 6 m round an unkillable player (Hanamura setup room), each variant adding to the last:
    1. Stacks 23 and 8 (2 bits each) and state 40 while firing.
    2. Effects 2 and 3 (Begin and counter).
    3. CosmeticEntity remote 0, plus entity var 49 bound to state 0 (`StateBound(Entity(0), 1, 0, 7)`).
  - No beam showed (`assault-capture/zarya-no-beam.png`), and nothing crashed. Zarya's var53 presence bit is 3.
  - Reading: `BD02E168` gates client-only children that look like first-person or local-player only; graph 601's `BD02E168` 92 likewise parents the visor's HUD presenter. So the beam a remote viewer sees probably comes from another path: the server-only TrackRay 43 chain or a remote-sync var the effects read (both effects carry one `m_8BF03679` remote-sync var).
  - Memory check (2026-11-03, TEMP sender rebuilt: Zarya in `WEAPON_GRAPHS` as `((0x0241, 69, 20, 42), 0, 23, 3)`, remote slots 23 and 8 = Stack `1` (2 bits), 40 with no payload, 2 and 3 = Effect Begin + 2-bit counter; ch2 shot record off). With six Zaryas in view, `tools/research/scriptlist.py` finds **no 0241 instance** in any variant (all five slots; slots 23, 8, 40 only; slots forced on all the time), and `guidhits.py` finds 37 copies of the GUID, none at an instance (no instance vtable at -0x1E8). In the same run it found Roadhog's 01C6 instance (active [9]), but only once; the allied Anas' 0B94 never showed.
  - Reading: either the client drops Zarya's frame (a wrong count or payload in one of the slots would do it, and would also explain no beam), or scriptlist misses remote instances that are idle. Follow-up: with Zarya's frame carrying no active slot, scriptlist again found no 0241 instance, and no 01C6 or 0B94 either. So scriptlist does not see idle remote weapon instances, and the check above is **inconclusive**. Next: find remote weapon instances another way (from the bot entity's script list, or `guidhits.py` with the right instance vtable for remote graphs) before judging the slots.
- **Ability Volleys (2026-11-03, partial, not committed).** Soldier's weapon graph `0x0254` has a second Volley, state 16 (remote 6), with projectile speed 50 and lifetime 5 (constant config vars): Helix Rockets. Ana's second Volley, state 44 (remote 28), has no speed: her scoped hitscan shot, not Biotic Grenade. Sending Soldier's Volley 16 per use, the same way as primary fire, draws something:
  - What was sent: the remote slot gets `1, 0, 6-bit uses`, and a ch2 shot record keyed to owner state 16 with event = uses.
  - In game, with only the ability sent and primary fire held back, each use shows an orange blast on the floor at or beside the Soldier bot (`assault-capture/bot-helix-floor.png`), not a rocket flying at the player.
  - The blast stays the same when the record aims straight up, with the origin moved 1 m forward, and with yaw conventions `atan2(-dx,dz)`, `atan2(dz,dx)` and `atan2(-dz,dx)`. So the record's direction is probably not what aims the remote projectile.
  - Lead: `TrainingTarget.patrol` turns a strafing bot to face its walking direction, so engaged bots face sideways. If the client fires a remote shot along the body's facing, the rocket hits the scenery beside the bot. That also fits the Ana dart flying sideways in `bot-ana-darts.png`.
  - Done 2026-11-03: `TrainingTarget.patrol` no longer turns a fighting hero bot (`hero` and `next_attack_at` set) to its walking direction. Live check with six Ana bots round the player (`assault-capture/bot-ana-facing-sheet.png`): inconclusive, the bots stood far off and darts show only as short purple streaks. The Helix remote-Volley sender was not committed, so the Helix retest needs it re-added.
  - Retest 2026-11-03 (sender re-added and committed: `target.ABILITY_VOLLEYS`, `remote_volley_frame(..., uses)`): TEMP all-Soldier roster, enemy bots moved to nav nodes 8 m from the player at spawn, allies removed. The Helix hits every 6 s and nothing crashes, but in four runs no enemy bot showed on screen (why not checked: the match may steer them away at once, or the nodes were out of view). So the rocket is still unseen (`assault-capture/bot-helix-retest-sheet.png`). Next: log the bots' positions per tick, or walk the player to the defenders.
  - Was: keep an engaged hero bot facing its target while it strafes (in `patrol`, skip the facing change when `hero` and `next_attack_at` are set), then re-run with one Soldier bot standing still in front of the player.
- **Effect states crash:** Roadhog's muzzle effect state (owner 19, remote 14) sent on crashes the client (EXCEPTION_ACCESS_VIOLATION reading 0x1F8). Effect states are not sent.
- Weapon graphs (body `STUWeaponComponent`, ref field `4D5597A8`), as Graph(index, owner, sync, remote), with the Volley states (class `EED6D952`) as state → remote slot:

| Hero | Graph | Volley states | Effect states (`691BFA55`) |
|---|---|---|---|
| Roadhog | (0x01C6, 56, 14, 37) | 9→4, 12→6, 14→7 | 19→14 |
| Zarya | (0x0241, 69, 20, 42) | 19→9 | 7→2, 8→3 |
| Bastion | (0x012E, 46, 16, 28) | 14→7 | none |
| Cassidy | (0x01D2, 63, 18, 38) | 11→5, 15→8 | none |
| Mei | (0x05E6, 65, 20, 45) | 13→7, 16→9 | 11→5, 12→6, 32→23, 47→33 |
| Reaper | (0x0019, 44, 11, 29) | 6→2 | none |
| Soldier: 76 | (0x0254, 81, 23, 49) | 10→4, 16→6 | 21→12, 38→25 |
| Sombra | (0x0B1B, 92, 42, 61) | 6→1 | 23→15, 31→18, 96→50 |
| Torbjörn | (0x1683, 49, 17, 27); also 0x168C | 10→5, 13→7 | none |
| Ana | (0x0B94, 84, 22, 51) | 5→0, 44→28 | 42→26, 48→30 |
| Lúcio | (0x0776, 61, 19, 39) | 14→7 | none |
| Zenyatta | (0x0164, 104, 22, 73) | 14→7, 18→9 | 33–37→24–28, 41→32 |

## Mannequin fire after the silent-slot fix (2026-11-09)

Live, Practice Range, viewer mannequin (`OW_VIEWER_FILE`), branch `statescript-oracle`:

- The lobby logs the mannequin's shots continuously: about 9 a second for Soldier: 76, and Lúcio's bursts every ~0.23 s.
- On screen, shots appear only when the mannequin appears, and they land at its feet. The user saw this; captures show an orange glow at Lúcio's feet for about 1 s ([capture](hitbox-drift/lucio-mannequin-appear-2026-11-09.png)). The D.Va mannequin did the same before (research/dva-mech.md).
- Mei shows no spray when she appears ([capture](hitbox-drift/mei-mannequin-appear-2026-11-09.png)), and Soldier: 76 shows no tracers in 16-frame bursts.
- Before the fix, Lúcio's Volley presence bit was read one slot off (`target.WEAPON_GRAPHS` lacked `Graph.silent`; tools/lobby/CHANGELOG.md 2026-11-09), so nothing of his fire could draw.
- Open then: why later Volley updates draw nothing, why the shots land at the feet, why Mei's spray never shows. The next section answers the first.

## Automatic Volleys fire one magazine per start (2026-11-09)

Live, viewer mannequin, `tracer.asi` injected into the running game, then read-only memory reads (`tools/research/volley_probe.py --records GRAPH`).

- **The Volley never restarts.** The tracer shows Lúcio's Volley (graph `0776` node 14) staying active through every burst; each frame only re-reads its 7 bits, a multiplier bit and the 6-bit event counter (4, then 5, ...). The begin happens only when the mannequin appears.
- **Shot records reach it but do nothing.** The record check is Volley slot 22 (`0x7FF6D1D05CD0`) → `0x7FF6D1D604B0`: each pending record of the entity (component 35, list `+0xB0`, count `+0xB8`, 176-byte entries: event `+24`, key `+144`, consumed byte `+164`) whose key equals the Volley's `+0x1C0` and whose event equals its counter `+0x44` goes to the handler `0x7FF6D1D07570` and is marked consumed. Live, every record matched (key `B000440100080E01`) and was consumed, yet the shot index `+0x260` stayed at 20.
- **Why:** the handler does nothing when the shot config's mode (`+0x11C`) is 3. A mode-3 Volley fires from its own update (`0x7FF6D1D05D90`, executor via vtable `+0x1D0`) while `+0x270` (shots fired) is below `+0x284 / +0x288`. Live, `+0x284` equals the ammo var53 the server sends: Soldier 30, Mei 120, Lúcio 20, the magazines (`PLAYER_KIT`). After one magazine, at the mannequin's appearance, `+0x270` = `+0x284` and the Volley never fires again.
- **Modes, read on each mannequin** ([volley-modes-2026-11-09.txt](bot-fire/volley-modes-2026-11-09.txt)):

  | Mode | Heroes | How the client fires a remote shot |
  |---|---|---|
  | 3 | Soldier: 76, Mei, Bastion, Lúcio | itself, up to the ammo var per Volley start |
  | 2 | Sombra, Reaper, Roadhog, D.Va | one per shot record (`+0x260` follows the records) |
  | 1 | Cassidy, Torbjörn, Ana, Zenyatta | one per shot record |

- **Fix (lobby, `target.AUTOMATIC`):** for the mode-3 heroes the Volley is off while the bot reloads (`bot.rounds == 0`) and starts again with the next magazine. Checked live: Lúcio's shot index restarts after each reload (11 → 20), and both Lúcio's and Soldier's fire now shows every magazine, with a pause for the reload ([Lúcio](bot-fire/lucio-restart-sheet-2026-11-09.png), [Soldier: 76](bot-fire/soldier-restart-sheet-2026-11-09.png)). Before, it showed only when the bot appeared.
- Bots also send their aim pitch now (movement `+66`, at the target's chest from the bot's eye). It did not move the fire.
- Found on the way (LizRollin's `direction16`, checked in IDA): the shot record's pitch is up-positive, and `world.shot_record` wrote it down-positive. Fixed, with a test that reads the record back through the client's formula. This also explains the Helix blast on the floor even when the record aimed "straight up" (above): straight up was read as straight down.
  - Live with the D.Va mannequin (mode 2, record-driven): her tracers now run to the player's chest, at the bottom center of the view, for the whole fight ([sheet](bot-fire/dva-record-pitch-sheet-2026-11-09.png)). They still start at her feet, with the muzzle flashes on the floor there: the same unbound var49 as above.

### The fire drew at the feet: var49 was unbound (solved for 7 heroes)

- The Volley's own aim fields are empty: its descriptor's aim sub-object (`desc+0x1D0`) has no position var (`+0x28`) and no direction var (`+0x20`), so the muzzle position (`0x7FF6D1F82020`) and direction (`0x7FF6D1F81F30`) fall back to the constant `0x7FF6D4461710`. The shot builder (`0x7FF6D1D043F0`) is obfuscated past its first block.
- Reading: the shots and muzzle effects resolve their start on the weapon entity (var49, the CosmeticEntity's output) and fall back to the body's origin when it is unbound. For the owner, binding var49 moved the shots onto the held rifle (rifle-initialization.md).
- First try: begin each automatic hero's CosmeticEntity in the remote frame with var49 bound to it by its owner state index (Soldier 11, Mei and Lúcio 0, 7 bits). It failed:
  - Lúcio: the flashes at his feet were gone and no gun was drawn ([capture](bot-fire/lucio-gun-bound-2026-11-09.png)). The tracer showed the client beginning states we never sent (nodes 6, 15, 18, 20, 22 of graph 0776): everything after the binding was misread.
  - Soldier: 76: the client froze when his body arrived ([log](bot-fire/soldier-gun-binding-hang-2026-11-09.log)), most likely the same misread.
- **Cause: in an H=0 frame a binding names a remote slot, not a state.** The binding reader (`0x7FF6D1C2D4F0`) checks the frame's H bit (`[ctx+0x4C]`, the same flag the frame dispatcher `0x7FF6D1C2EE10` runs the owner CmFD work under):
  - H=1: `m_statesBitCount` bits (`[graph+0x170]`), a state index.
  - H=0 (`0x7FF6D1C2D726`): `m_remoteSyncNodesBitCount` bits (`[graph+0x174]`), an index into the remote node list, then that node's state.
  - Graph 0776 has 7 state bits and 6 remote bits, so our 7-bit binding put every later field one bit off. The vendored decoder had the same mistake, which is why the oracle passed the bad frames.
- **Fix** (`target.WEAPON_MODELS`, `remote_volley_frame`): the bot frame begins the CosmeticEntity (remote slot, no payload), sends var478 = false (its gate is `var34 || var478`, var34 is already false) and binds var49 to that remote slot in 6 bits, as an entity variable or at its presence bit, by the scope the graph gives var49 (`m_60DB8F99`). From the graph catalog:

  | Hero | gun state / remote slot | var49 |
  |---|---|---|
  | Mei, Lúcio, Sombra | 0 / 0 | entity |
  | Cassidy | 27 / 16 | entity |
  | Ana | 61 / 34 | entity |
  | Roadhog | 16 / 9 | presence bit 12 |
  | Soldier: 76 | 11 / 5 | presence bit 7 |

  - Left out: Torbjörn (his gate also reads var842), Reaper and Zenyatta (var49 is not a synced variable of theirs), Bastion and D.Va (no CosmeticEntity in the weapon graph). Their fire still starts at the feet.
- `decode.read_bindings` reads H=0 bindings as remote slots now. The roster test reads every bot frame through it and checks var49's binding lands on the CosmeticEntity, in the right scope.
- Live (2026-11-09, viewer mannequins, the tracer showing only the gun state and the Volley begun):
  - Lúcio holds his amp; its red flash pulses on the amp and his shots grow there, not at his feet ([amp](bot-fire/lucio-amp-in-hand-2026-11-09.png), [sheet](bot-fire/lucio-gun-fire-sheet-2026-11-09.png)).
  - Soldier: 76 no longer freezes the client; the flash is on his rifle at chest height ([sheet](bot-fire/soldier-gun-fire-sheet-2026-11-09.png)).
  - Roadhog's scrap-gun blasts leave the gun ([sheet](bot-fire/roadhog-gun-fire-sheet-2026-11-09.png)); Sombra's tracers start at chest height ([sheet](bot-fire/sombra-gun-fire-sheet-2026-11-09.png), behind a stale Roadhog, see below); Ana's rounds cross at chest height.
  - Mei and Cassidy fire in the client but draw nothing ([probe](bot-fire/cassidy-mei-volley-probe-2026-11-09.txt)):
    - Mei holds her blaster ([capture](bot-fire/mei-blaster-in-hand-2026-11-09.png)). Her Volley is on and fires itself (gate cycling, shot index climbing), yet no spray shows at her gun ([zoom](bot-fire/mei-firing-no-spray-2026-11-09.png)).
    - Cassidy's Volley consumes one shot record per 0.5 s (event counter and shot index climb with each), and every shot hits on the server. A 60 fps recording of about 8 shots shows no flash or tracer at him ([zoom](bot-fire/cassidy-firing-no-flash-2026-11-09.png)).
    - Neither was ever seen firing before the gun binding either (the 2026-11-02 list above has no Cassidy or Mei). Both are answered below: Mei's spray is two effect states, Cassidy's shot a server-made projectile.
    - Both mannequins also ran in place about 15 m out while the server landed Mei's 10 m beam on the player, so the server and the client disagreed on the distance (hitbox-drift.md).

### Mei's spray: two effect states the remote Volley never begins (solved)

- Mei's primary Volley (state 13) has a subgraph with two STU_691BFA55 effect states, played on var49's entity: state 11 (remote slot 5, effect `0x0300000000001556`) and state 12 (remote slot 6, effect `0x0300000000000E26`). A remote Volley does not begin its subgraph, so they never ran.
- Fix (`target.WEAPON_EFFECTS`, `remote_volley_frame`): while the bot fires, the frame also begins those slots (payload: Begin + a 2-bit counter). They need var49 bound: with it unbound the client crashed reading `[0 + 0x1F8]`.
- Live: the spray shows at her blaster (run 13 and 16, viewer mannequin).

### A swap stalled the channel: datagrams over the client's size (2026-11-09)

- After some mannequin swaps the client never acked a datagram; the server resent it forever and every later reliable message waited behind it. This, not the tracer, was the earlier "tracer caveat" (asi-tracer.md is corrected).
- First reading (wrong): the destroy of a body whose fire still plays, or a fire-off frame next to a new bot's 20301. The workarounds for it (two-step swap, dead bots' fire off, a swap hold until a mannequin's projectiles were gone) only changed what shared the datagram, so the stall became rarer but stayed (below).
- Cause: the client's transport accepts a datagram's seq (`0x7FF6D1619FC0`, called from `0x7FF6D1619210`) before the payload is read, so a datagram that arrives is always acked. One that is never acked never arrived. A new bot's 20301 is ~1180 bytes (1175 for the Range with Mei). With the swap's other reliable message, the destroy and the movement records, the frame went over the 1230-byte payload (`MAX_DATAGRAM` 0x4F0 less the 34-byte header). `send_frame` only cut entity records, and only down to one. Run 29 logged it: a 1237-byte frame (20301 + one entity record) at the Ana swap, resent forever.
- Fix (`Client.send_frame`): hold back reliable messages first, then entity records, down to none, until the frame fits; log a frame that still does not (one message over the size would need the bulk fragments, kind 15, which the server does not send).
- Live: run 30, 14 mode-1 swaps (Torbjörn, Zenyatta, Cassidy, Ana, Mei), with the swap hold removed: 0 resends, 0 oversize frames, 1 frame that held a message back (the Cassidy swap, whose tracers then drew: [sheet](bot-fire/cassidy-after-full-frame-2026-11-09.png)).
- Kept: the two-step swap and dead bots' fire going off. Neither is needed for the stall any more.

### Cassidy, Ana, Torbjörn and Zenyatta: the shot is a server-made projectile (2026-11-09)

- The shot-record handler (`0x7FF6D1D07570`) reads the Volley's runtime config (`[volley+0x1E0]`, set only during the call from the entity's info map; `volley_probe.info_map`). Its mode (`[cfg+0x11C]`), read live on the mannequins:

  | mode | heroes | what a remote shot record does |
  |---|---|---|
  | 1 | Cassidy, Ana, Torbjörn, Zenyatta | only the per-shot muzzle effect (`0x7FF6D1D064F0`) |
  | 2 | Roadhog, Reaper, Sombra | builds the shot itself (projectile path via `[cfg+192]+32`), then the muzzle effect |
  | 3 | Soldier: 76, Mei, Bastion, Lúcio | ignored: the Volley fires itself (AUTOMATIC) |

- The table follows from each Volley's authored data, by LizRollin's rule (`projectiles._sync`, "as the PS4 build resolves it"; research/lizrollin-runtime.md): `m_5E4A4E85` if set (Cassidy's is 1, the others 0); else 3 for `m_A17BE89B` >= 2 (shots a volley: Lúcio 4, Soldier 99999) at `m_numShotsPerSecond` >= 5.5; else 2 without `m_projectileLifetime` or with more than 7 `m_numProjectilesPerShot` (Roadhog 25, Reaper 20 and no lifetime); else 1. All 11 heroes match. Sombra matches only if her rate, the dynamic var 8421, counts as 0 when the mode is resolved: at any rate of 5.5/s or more she would be 3 (the var's value was not read). D.Va's mech graph comes from `graph_record.py`, which keeps no Volley config, so she is unchecked. Test: `test_the_shot_modes_read_live_follow_the_volleys_authored_sync`.
- Per-shot muzzle effect entries (`[volley+0x2F0]`, 0x40 bytes, count `+0x2F8`): `+0` shot-index list matched against `index % [volley+0x2A8]`; `+0x20` key `state << 32 | 0x00010001`, naming the CosmeticEntity state (Cassidy 27, Ana 61); `+0x28` hard point (`0x0DC0000000000006`); `+0x30` the effect handle, which changed with every Cassidy shot. So his muzzle effect spawned, but nothing was visible at him at 70 fps.
- What draws a mode-1 shot is its Volley's `m_projectileEntity` (Cassidy `0x04000000000002A7`, Ana `0x04000000000015B7`, Torbjörn `0x0400000000002A66`, Zenyatta `0x0400000000000236`; each has an `STUProjectileVisualComponent`), made by the server and named in the shot record, exactly like the owner's Helix rocket (`_tick_rockets`).
- Live tries, in order:
  1. Creating `0x2A7` bare and re-making it each frame (the `Projectile` path): nothing drawn.
  2. Creating it once and leaving it: nothing drawn.
  3. Helix recipe (create, shot record with `entities=(it,)` and `shot_time`, ch2 ProjectileVisual snapshot with position, velocity and source = the body): **drawn**. Cassidy's golden tracer and muzzle star run from his revolver to the target on every shot ([sheet](bot-fire/cassidy-tracer-2026-11-09.png)); Ana's darts ([sheet](bot-fire/ana-dart-sheet-2026-11-09.png)) and Torbjörn's molten rivets ([sheet](bot-fire/torbjorn-rivet-sheet-2026-11-09.png)) fly from their guns.
  4. The same without naming it in the record: nothing drawn. The link is what makes the client draw it.
  5. LizRollin's recipe (`projectiles._show_others`): created under the map at (0, -1000, 0), named in the record, no ProjectileVisual snapshot (run 31). Cassidy's tracer draws as before ([sheet](bot-fire/cassidy-record-only-tracer-2026-11-09.png)); Ana shows only her muzzle flare and no dart ([sheet](bot-fire/ana-record-only-no-darts-2026-11-09.png)), Torbjörn no rivet ([sheet](bot-fire/torbjorn-record-only-no-rivets-2026-11-09.png)), Zenyatta nothing. So a flying projectile needs the snapshot; only the hitscan tracer starts from the record alone.
- Fix (`target.WEAPON_SHOTS`, `match._bot_shot`): each mode-1 shot gets that triple per viewer, flown at the weapon's speed. A hitscan shot (Cassidy) gets only the create, under the map, and the record, as try 5 showed is enough; destroyed `SHOT_LINGER` later. This replaced a guessed 400 m/s flight for her tracer (run 32: [sheet](bot-fire/cassidy-hitscan-record-only-2026-11-09.png); Ana's darts unchanged). When it lands, a stopped snapshot at the impact point; `SHOT_LINGER` (0.5 s) later, the destroy.
- ProjectileVisual snapshot reader (`0x7FF6D1F88980`): bit `+4` (stopped), bit `+1`; when stopped, bits `+2`, `+3` and no velocity block. `world.projectile_state(stopped=True)` writes that.
- Swap stalls seen while adding this (runs 21-28: about 1 in 7 mode-1 body destroys, whatever was done to the projectiles first) were the oversize datagram above, fixed in run 30.
