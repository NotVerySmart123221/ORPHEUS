# Helix Rockets visual: findings (2026-11-05, open)

In retail, Helix fires three rockets that spiral out of the rifle (reference footage from the user). What the lobby draws:

- **Production (5eceaf4):** the drawn child `0x072E` is destroyed and re-created a step further every frame (`Match._projectile_updates`). Each create restarts its effect, so the flight leaves a row of orange glows behind it instead of a rocket with a trail ([helix-rocket-sheet.png](helix-rocket-sheet.png); the user's capture shows the same row).
- **The authored Volley 38 (graph 596) fires one projectile per shot:** `m_numProjectilesPerShot` 1, `m_numShotsPerSecond` 1, speed 50, lifetime 5, `m_projectileMotions` null.
  - Its projectile `0x1C86` (or `0x072C`, chosen by var842) holds an `STUProjectileVisualComponent`, a target tag, a model component and the child `0x072E`.
  - `0x072E` is only an `STUEffectComponent`.
  - So the three spiralling rockets are not three Volley projectiles. They are most likely drawn by the native projectile path from that one projectile [unverified].
- **Experiment A** (run `20261105T021541`, reverted): `0x1C86` created once at the eye with a create-time velocity of 50 m/s along the aim. Only the native muzzle flash shows; no rocket ([helix-projectile-once-velocity.png](helix-projectile-once-velocity.png)).
- **Experiment B** (reverted): `0x072E` created once, 1 m ahead of the eye and facing the aim, then left in place. One orange-white glow stays where it was created until the shot lands (about 0.4 s) and does not fly ([helix-child-once-static.png](helix-child-once-static.png)). The effect has no motion of its own.
- **Reading:** the server can only place a glow. The spiral needs the client's own projectile simulation, started by its Volley (the shot builder `0x7FF6D1D043F0` → creation `0x7FF6D1D41790`; `research/fire-animation-trace.md`).
- **Leads:**
  1. Let the client predict Volley 38. The instance descriptor's prediction bit made held-fire Volleys run (fire-animation-trace.md, prediction experiments). It sits on graph 596's whole instance, so the rifle changes too: camera recoil compounded when begin times were sent.
  2. Move one created `0x072E` with ch3 movement records instead of re-creating it. That removes the trail of glows but still draws one glow, not three rockets. A non-mover projectile (`0x1476`) reported a missing movement component when moved.

## Client prediction (2026-11-05, reverted)

The server's own glow was turned off in each of these runs, so anything seen came from the client.

- **Run C, prediction bit:** the instance descriptor's prediction bit was set on graph 596 (the bit after the source entity in `statescript._descriptor`).
  - Helix: the muzzle jet only; no rocket ([helix-predicted-instance.png](helix-predicted-instance.png)).
  - Rifle: normal. Tracers draw, and pitch climbed to −199 after 6 shots (about −33 per shot; no runaway recoil).
  - So the bit alone does not make the client fire Volley 38. The client never predicts the button press itself (fire-animation-trace.md, prediction experiment 2).
- **Run D, shot record at the start:** a ch2 shot record on the player body, keyed to owner state 16 with event = serial (`world.shot_record`, the bot-shot method), sent right after the Helix start frame. The client accepted it; no rocket.
  - Live read of Volley 16 (`.scratchpad`-style read-only scan): flags `0x4060`, event counter 1, config pointer `+0x1C0` = 0, shot index `+0x260` = 0, and the per-shot effect entry's handle still −1.
  - For comparison, Cassidy's bot Volleys set `+0x260`, `+0x1F0` and the handle once their record arrived (bot-fire.md).
- **Run E, start held 0.5 s, record with the stop:** the same counters (`+0x1C0` = 0, `+0x260` = 0, handle −1). No rocket ([helix-shot-record.png](helix-shot-record.png)).
- **Reading:** on the owner's body, Volley 16 never resolves its config (`+0x1C0`, set when the Volley begins: `0x7FF6D1F80880` via `0x7FF6D1D05700`). The remote-shot handler (`0x7FF6D1D07570`) therefore has nothing to fire.
- **Next step:** trace why OnBegin does not run (or does not keep `+0x1C0`) for a server-started Volley on an owner instance. Begin with the dedup filter `0x7FF6D1D07D50` and the `[Volley+0x40]&1` owner/predicted flag. Alternatively, test the shot record on a remote (H0) copy of Soldier's weapon graph, which worked for bot Volleys. No server-side change draws the native three-rocket spiral yet.

## IDA trace: who writes the Volley key `+0x1C0` (2026-11-04, static only)

Correction to the runs above: `+0x1C0` is the **shot key**, not the config pointer (the rifle's key during a burst is `A000010100080A01`, fire-animation-trace.md). The config is `+0x1E0`, looked up from the key in table `0x7FF6D1FA6660`.

Volley vtable `0x7FF6D38747A8`. Every write to `+0x1C0` in the Volley code:

| Slot | Function | What it does |
|---|---|---|
| `+0x1B8` | `0x7FF6D1D04260` → `0x7FF6D1F7EE00` | next shot: key = `0x7FF6D1FA5180(entity, word, [+0x70], old index + 1)`; config `+0x1E0` from the table; sets `+0x1EC` = 1 |
| `+0x1C0` | `0x7FF6D1D07C40` → `0x7FF6D1F81950` | same, with an explicit shot index; sets `+0x1EC` = 1 |
| `+0x1E0` | `0x7FF6D1D062A0` → `0x7FF6D1F7F490` | rollback restore: walks the history list at `+0x3C0` (records: `+0x38` key, `+0x40` tick) for the newest record older than the current tick, restores the key and fields from it, then resolves the config (`0x7FF6D1F7E030`) |
| teardown | `0x7FF6D1F7DDC0`, `0x7FF6D1F81880` | releases the key |

- **The empty-history branch** of `0x7FF6D1D062A0` (no record older than the tick) sets `+0x1C0` = 0, calls `0x7FF6D1F81710`, and sets byte `+0x1EC` = 0. `+0x1EC` is the byte the Update `0x7FF6D1D05D90` gates on, so no shot runs.
- The prune `0x7FF6D1D04040` (called from the same restore) drops every history record that does not match the restored (key, tick).
- **Reading [unverified]:** an owner Volley gets its key only from the client's own shot path (slots `+0x1B8`/`+0x1C0`, which also record history). A start sent by the server makes the client roll back to its history. Soldier's client never fired Volley 16 itself, so the history is empty, and the restore leaves key 0 and `+0x1EC` 0. That matches runs D and E (`+0x1C0` = 0, `+0x260` = 0). It also explains why the rifle works: the client fires it itself from the LogicalButton.
- **Not traced:**
  - which code calls slots `+0x1B8`/`+0x1C0` (no direct call in the Volley code ranges; OnBegin `0x7FF6D1F80880` ends in obfuscated code)
  - which code adds the records to `+0x3C0`
  - what calls slot `+0x1E0`
  - Other `+0x1C0` writers in the same range (`0x7FF6D1D02FB0`, `0x7FF6D1D03750`, `0x7FF6D1D057C0`) belong to a different class (vtable `0x7FF6D38745E0`) and are not the Volley's.

### Live check (2026-11-04, production lobby, read-only): the empty-history reading is refuted

[helix-volley-history.txt](helix-volley-history.txt): a 2 ms sampler of both Volley objects. Volley 16 has the same vtable as the rifle's, `0x26547A8`.

- **Rifle (state 10):** during the burst, flags `40E1`, key `A000010100080A01`, gate `+0x1EC` = 1, `+0x3C8` = 1 (once 4), shot index 1–2.
- **Helix (state 16), production server-sent start:**
  - Key `A000010100081001`, gate 1, `+0x3C8` = 1.
  - At the press the shot index went to 1, then back to 0 about 3 s later.
  - So on the owner's body Volley 16 resolves its key and runs one shot.
  - The key byte `0x10` is the state index; the rifle's is `0x0A`.
- **Not explained:**
  - Volley 16's key first appeared during the rifle burst, before any Helix press. The lobby keeps state 16 in the resent weapon states, which may restart it [unverified].
  - Both Volleys' `+0x3C0` heads cycle through the same five nodes (`…C10`, `…C60`, `…CB0`, `…D00`, `…D50`), so `+0x3C0` is likely a shared pool rather than a per-Volley list [unverified].
- **Runs D and E** read `+0x1C0` = 0 with the server glow off and different payloads. Those reads do not describe production.
- **New reading:** Volley begin and the shot both run for Helix on the owner's body. The missing rocket is downstream of the shot: the shot builder `0x7FF6D1D043F0` → projectile creation `0x7FF6D1D41790`, or the drawing/lifetime of a predicted projectile with no server confirmation [unverified].
- **Next step:** read the shot builder's output for Volley 16. Either list the client's projectile entities around the press, or check whether creation `0x7FF6D1D41790` is skipped for an owner shot.

## Projectile creation trace (2026-11-04, IDA plus read-only live reads)

Evidence: [helix-projectile-creation.txt](helix-projectile-creation.txt).

### Path (static)

1. **Executor `0x7FF6D1D06DF0`.** It always advances the shot counter (`+0x270` → `+0x260`), even when it skips the projectile, so the "shot 1" above proves less. It queues the projectile job `0x7FF6D1D088A0` only when all three gates pass:
   1. **Dedup `0x7FF6D1D07D50`.** Records are (time, `+0x280`, event `+0x44&0x3F`, shot), kept 2 s, stored at Volley `+0x3D0`/`+0x3D8`. A record already in the window returns 0 (skip). A new one returns 2 (fire). 1 means fire only if bit 10 of the instance's `+0x318` is clear.
   2. Config `+0x1E0` is non-null.
   3. Projectile info `[config+0xC0]` is non-null, and its `+0x20` (the projectile entity) is non-zero.
2. **Job → shot builder `0x7FF6D1D043F0`.** It builds `config+0x24` projectiles. It skips creation when an entity with the pre-assigned id already exists; otherwise it calls creation `0x7FF6D1D41790`.
3. **Creation `0x7FF6D1D41790` returns a status.** The component ids come from the STU hash → id switch at `0x7FF6D21725E0`. Refusals:

   | Status | Refused when |
   |---|---|
   | 2 | no asset |
   | 3 | definition load failed |
   | 9 | the entity has component 22 |
   | 4 | the entity lacks component 50 (ProjectileVisual) |
   | 10 | speed `config+0x4C` > 0 and the entity has component 9 (Effect) |
   | 5 | `config+0xB0` is set and the entity lacks component 49 |
   | 1 | (component 47 or config byte `+0x107`) and the caller's flag |
   | 7/8 | spawn failed |

   Other ids: Model = 4, TargetTag = 76.

### Assets (asset data, decoded with the retail bundle lookup)

- **Rifle tracer `0x03D4`** (and `0x1C85`): ProjectileVisual, TargetTag, Effect.
- **Helix `0x072C`** (and `0x1C86`): ProjectileVisual, TargetTag, Model, and a child entity (`0x072E`, Effect only).
- `0x072C` passes every refusal that could be mapped: it has 50, has no 9 and no 22, and has no 47 or 49 to be checked.

### Live (production lobby, read-only)

- **Shot configs:**
  - Rifle key `…0A01`: config `+0x24` = 1, shots 99999, projectile entity `0x03D4`.
  - Helix key `…1001`: `+0x24` = 1, shots 1, projectile entity **`0x072C`**.
- **Dedup:** each Helix press adds two records for its event (press 3 → event 3), so gate 1 passes.
- **Control:**
  - The lobby never sends `0x03D4`, yet during a rifle burst `0x03D4` qwords in the watched heap rose 24 → 41.
  - So the method sees projectiles the client creates itself.
- **Helix press (serial 6):**
  - A new `0x072C` qword appeared at `1A7057C4060` in that same pool.
  - At the same sample the `0x03D4` count dropped by one, so the Helix projectile reused a freed tracer slot.
  - It was still there 65 s later. Freed slots keep their old GUID, so this does not give the lifetime.
  - A repeat press (serial 7) reused the same slot, so the count could not change. The planned movement sample of that object did not run before the session timed out.

### Reading

- At the press the client writes a `0x072C` reference into the same pool the rifle tracers use.
  - Corrected after the sampling runs below: this does not prove an entity exists. The projectile job the executor queues also holds the projectile GUID (job `+0x30`, next to config `+0x20`, key `+0x28` and graph `+0x38`).
- The rocket is still not seen. What is left:
  1. It is destroyed right after creation, for example because it is predicted and the server never confirms it [unverified].
  2. It flies but its model is not drawn [unverified].
  3. It flies from or to the wrong place [unverified].

### Next step

During a Helix press, read the slot at `1A7057C4060` every 30 ms and look for position floats that change and for the slot being freed or reused. The slot address changes after a restart; find it again with `proj_watch.py`'s per-address output.

## Sampling the Helix projectile (2026-11-04, fresh session, read-only, 3 runs)

Evidence: [helix-projectile-sampling.txt](helix-projectile-sampling.txt). The rifle fired first so the projectile pool was in the watched regions. Then one Helix press per run.

- **Run 4:**
  - Three new `0x072C` copies appeared: a thread-stack address, the config slot, and heap `24584A8AB20`.
  - The stack copy was overwritten within 0.16 s.
  - The heap copy kept `0x072C` for at least 1.7 s with no changing floats nearby. It was sampled only from about 3.6 s after the press, because the copies were followed one after another. It is likely a cached reference, not a flying rocket [unverified].
- **Run 5:** all new copies were followed together, every 20 ms, over a window from −0x200 to +0x600.
  - One fresh heap copy, `245B00966D0`, held `0x072C` for under 0.17 s; then the memory held pointers (freed and reused).
  - No float in the window changed smoothly (no position track).
- **Run 6:** the press was logged (serial 3), but no fresh copy was seen. A short-lived copy either reused a slot that already held `0x072C`, or lived less than one sample (about 100 ms).
- The config slots `…912A0`/`…912D0` swap between `0x072C` and `0x03D4` about every 0.5 s, with or without firing. Their role is not identified.

**Reading:**
- No moving `0x072C` object was found.
- The only fresh copy lived under 0.17 s. That fits the projectile job (queued, run, freed) and also an entity destroyed within a few frames. These reads cannot tell the two apart.
- The 3-run limit is reached: this stays open.

**What would settle it:**
- A breakpoint on creation `0x7FF6D1D41790`'s status write (`*a3`) for the Helix job. That needs a debugger attached to the game; the earlier task's constraints rule out writing to the game process.
- Or find the client's entity list and read the entity count around the press. The entity list is not traced yet.

## Execution breakpoints (2026-11-05, allowed by the user since 2026-11-04)

Evidence: [helix-breakpoint-attempt.json](helix-breakpoint-attempt.json); run `20261105T040101.083925Z`.

- `hw_watch.py --execute --rva` (now in the repo) armed four instruction breakpoints on all 148 threads:
  - the executor's job queue `0xAE724B`
  - the job entry `0xAE88A0`
  - the shot builder `0xAE43F0`
  - creation's return site `0xAE4609`, where `[rsi+0xF8]` = status and `rax` = entity
- The game exited about 1.1 s after the attach (`0xC0000005`, access violation; lobby: client closed), before the Helix press. There were 0 hits.
- This matches round 3 of numeric-health.md (data breakpoints: 0 hits, crash after detach). The client's protection blocks debugger attach. Software breakpoints also need an attached debugger, so they would hit the same wall [unverified].
- **Ways left to read creation's status:**
  - Emulate `0x7FF6D1D41790` in `live_emu.py` with a Helix request built from the live config, and read `*status`.
  - Or log it from inside the client with an ASI plugin hook. That needs the user's go-ahead: it is a new plugin, and the hook writes code.

## In-process hardware breakpoints (2026-11-05, `helix_probe.asi`)

Evidence: [helix-probe-asi.txt](helix-probe-asi.txt).

- The plugin was loaded into the running game. It registers a vectored exception handler and sets DR0–DR3 on every thread of its own process (no debugger), with no game code or data written.
- **Run 1, breakpoints on the shot path:**
  - 146 of 147 threads armed; the next pass found them still set.
  - The game crashed (`0xC0000005`) within about 1 s, with no hit logged.
- **Run 2, control:** all four breakpoints on a function inside the plugin that no game thread runs. Same result: armed on 146 threads, then a crash within about 1 s.
- **Reading:** the client crashes as soon as its threads carry debug-register breakpoints, whether or not one is hit. Together with the debugger attempts (numeric-health.md round 3; the attach above), hardware breakpoints of every kind are unusable on this client. Software breakpoints need writable code, which the image refuses (tools/asi/README.md), or an attached debugger.
- **Left for creation's status:** emulation (`live_emu.py` over pages copied from the live client), with a Helix request built from the live shot config.

## Root cause: no predicted-entity ID reservation (2026-11-05, emulation)

Evidence: [helix-creation-emulation.txt](helix-creation-emulation.txt). Tool: `tools/research/creation_emu.py`.

**Method:**
- Emulated creation `0x7FF6D1D41790` in unicorn over live memory (`live_emu.py`).
- The request was built the way the executor builds it:
  - `+0` = game tick (`sub_7FF6D2D91860` on a Volley)
  - `+0x08` = Soldier entity
  - `+0x20` = live shot config
  - `+0x30` = projectile asset
- Each config lives only about 30 ms in the info map, so its pages are snapshotted the moment they are complete.

**Results:**

| | Rifle (state 10) | Helix (state 16) |
|---|---|---|
| Projectile | `0x03D4` | `0x072C` |
| Config type `+0x11C` | 3 | **1** |
| Creation | passes every refusal, reaches the spawn call | **status 8**, with caller flag 0 or 1 |

**Why Helix fails:**
1. Type-1 configs take a separate path. After the same refusals, creation asks the owner object (vtable RVA `0x260BE40`) for a spawner via `sub_7FF6D19A7C70`. It gets one: vtable RVA `0x260D2C0`, create function `0x7FF6D19AF250`.
2. The spawner first asks `sub_7FF6D1DF59D0` for a **predicted entity ID**. That function searches a reservation table on the owner (`+0x90`).
   - Records are 24 bytes: definition GUID, tick range [start, end), base ID, count per tick.
   - The ID is `base + count·(tick − start) | 0xA0000000`.
   - With no matching record it returns the invalid ID (`dword_7FF6D4461288` = 0).
3. **The live table is empty** (count 0), so the ID is invalid, the spawner returns null, and creation reports status 8. No Helix rocket is ever created on the client.

**Reading:**
- Helix rockets are client-predicted networked entities. The client may only create one under an entity-ID block the server has reserved for that projectile definition and tick window.
- The rifle tracer (type 3) is a local cosmetic and needs no ID.
- The lobby never sends these reservations. That explains every earlier observation:
  - Volley 16 runs its shot.
  - The executor queues the projectile job.
  - Nothing appears.
- Limit: the emulated request leaves the other builder fields zero. The refusal comes from the empty reservation table, which does not read those fields.

**Next step (a server-side fix, in line with the project rules):**
1. Find the network message that fills the reservation table: the writer of the owner's `+0x90` records (`sub_7FF6D2955C30` grows the related `+0xA0` list).
2. Have the lobby grant a block for `0x040000000000072C` (and `0x1C86`, the other choice of var842) covering the match's ticks.

## The reservation message (traced 2026-11-05)

**Where the table comes from (IDA):**
1. The owner is **component 90** of the shooter's entity. Creation (`0x7FF6D1D41888`) looks it up with `sub_7FF6D1DF5B90`, which gives mask word 1, bit 26 (1·64 + 26 = 90).
2. Component 90's object (vtable `0x7FF6D3830488`, 0xB0 bytes) holds its data struct at `+24`: **STU_41FFE61E** (0x88 bytes, parent STU_54D6A5F9, registered at `0x7FF6D142DFB0`). Its visit method (`0x7FF6D17F4540`) hands that struct to the serializer.
3. STU_41FFE61E has one field: **`0x63398BCB` at `+0x78`**, an inline array (OWLib serialization type 5) of **STU_5E1D1776**, with field flags 0. In the component that is `+0x90`/`+0x98`, the allocator's table.
4. STU_5E1D1776 (24 bytes), field flags 0:

   | Field | Type | Offset | Meaning (from the allocator) |
   |---|---|---|---|
   | `0xA58EE665` | u64 | 0 | projectile definition GUID |
   | `0xF2A58A6D` | u32 | 8 | first tick |
   | `0xFE67B51B` | u32 | 12 | end tick (exclusive) |
   | `0xF036B16E` | u32 | 16 | base ID |
   | `0xDF2DA8B3` | u8 | 20 | IDs per tick |

**So the carrier is an ordinary component update** (entity channel ch4, op 2 update, `world._write_components`), not a new message and not ch5:
- The update body reader `0x7FF6D19AB870` reads the u8 mask length and the mask, then walks the struct's fields (`sub_7FF6D2E00130`). Each mask bit gates one field, in order. Here there is one field, so the mask is 1 byte, `0x01`.
- An inline array (`sub_7FF6D2E00050`): a u32 count (`sub_7FF6D16108F0`), then each element's fields in order, unmasked.
- A scalar with flags 0 (`sub_7FF6D2E00300`, default branch): its size in raw bytes. The "id" byte-mask form only applies with field flag 2.
- Per record on the wire: `<QIIIB` = def, start, end, base, count (21 bytes).

**Emulator proof** ([helix-reservation-injection.txt](helix-reservation-injection.txt)):
- One record `{0x040000000000072C, 0, 0xFFFFFFFF, 0x100000, 1}` was written into component 90's table, in emulator memory only.
- The allocator then returned `0xA01030AD` = `0xA0000000 | (0x100000 + tick 12461)`.
- The spawner reached its spawn call (`0x7FF6D19AF416`). Without the record: ID 0, status 8.

**Lobby change (made later on 2026-11-05; see "Reservations served"):**
1. Add `90: ["reservations"]` to `COMPONENT_FIELDS`, with a `_write_value` kind that pads, then writes a u32 count and `<QIIIB` per record. This is the same pad-then-count framing as `u32[]`.
2. Send it once in Soldier's entity update: records for `0x040000000000072C` and `0x0400000000001C86`, ticks [0, 0xFFFFFFFF), count 1. Each record needs its own base ID block, high enough that `base + tick` never meets a lobby entity ID and stays under `0x1FFFFFFF`.

**Not known yet [unverified]:**
- What the client does with a predicted rocket that the server never confirms: does it time out, or wait for a create carrying the same ID?
- Whether the lobby's per-frame flown `0x072E` glow must then stop.
- The tick units, beyond "the value `sub_7FF6D2D91860` returns" (12461 at the first Helix shot of this run).

## Reservations served (2026-11-05, game PID 35556)

The lobby now sends component 90 in Soldier's body create (`soldier.HELIX_RESERVATIONS`).

**Seen live (read-only):**
- Component 90's table holds both records: `{0x072C, 0, 0xFFFFFFFF, 0x01000000, 1}` and `{0x1C86, …, 0x08000000, 1}`.
- Creation was emulated without any injection. The live table issued `0xA10030EA` and creation reached the spawn call.
- Every press issued IDs on two consecutive ticks (issued list `+0xA0`):
  - `0x4EA9`/`0x4EAF`
  - `0x662F`/`0x6630`
  - `0x7D82`/`0x7D83`
  - `0xCA87`/`0xCA88`
- The tick is the command frame. The lobby logged `helix serial=1 frame=20137` = `0x4EA9`. Later presses issue IDs 2 to 3 frames after the frame the server logs.
- Each issued ID (`0xA0000000 | id`) appears twice in the entity heap within 0.2 s of the press, and the `0x072C` GUID count rises (196 → 308). IDs from earlier presses are still there minutes later; the first press's ID is gone, and a control ID scores 0.
- **Nothing draws.** No rocket showed in a 58-frame burst (30 ms apart) after a press, facing the spawn-room wall.

**Reading [unverified]:** the client now creates predicted Helix rocket entities. They seem to wait for something from the server, which never confirms them: no rocket shows, and the entities outlive the 5 s lifetime.

**Next step:** find how a server confirms or adopts a client-predicted entity. Candidates:
1. A ch4 create carrying the same ID.
2. The frame-level ch5 list the lobby always sends empty.

## How the server confirms a predicted entity (traced 2026-11-05)

**Spawner `0x7FF6D19AF250` (IDA):**
1. It gets the ID from component 90 and refuses an invalid ID, or an ID whose entity already exists. That existence check is the entity map lookup at `0x7FF6D19AF3B3`.
2. Before spawning (`sub_7FF6D2A50A50`), it adds component 91 (`a3[2920] = 91`) and, if absent, component 22 to the spawn description.
3. After spawning, it looks up component 91 on the new entity (`sub_7FF6D17F7540`: word 1, bit 27) and fills it. That code is obfuscated with anti-disassembly junk from `0x7FF6D19AF4FB`.

Component 91 marks a client-predicted entity [unverified: inferred from where it is added].

**Entity-channel create `0x7FF6D19AC360`:**
1. It reads the create body (`sub_7FF6D19ADAB0`).
2. It looks the incoming entity ID up in the entity map (`0x7FF6D19AC91E`–`0x7FF6D19AC954`).
3. If the ID exists, it takes that entity (`0x7FF6D19ACA88`) and **skips the spawn**. Otherwise it spawns with `sub_7FF6D2A50A50` (`0x7FF6D19ACAAE` / `0x7FF6D19AD229`).
4. Both paths reach `0x7FF6D19AD2EE`, which looks up component 91 on the entity. This is the only other user of `sub_7FF6D17F7540`. What it does next is obfuscated.

**Reading:** a server create (ch4 op 1) carrying the predicted ID is how the client adopts its predicted entity. The ch5 list is not involved. What the component-91 block does on adoption is not read [unverified].

**Blocker: the lobby can't know the ID.** The ID depends on the tick at which the client's projectile job runs:

| Press | Lobby frame | Issued ticks | Offsets |
|---|---|---|---|
| 1 | 20137 `0x4EA9` | `0x4EA9`, `0x4EAF` | +0, +6 |
| 2 | 26156 `0x662C` | `0x662F`, `0x6630` | +3, +4 |
| 3 | 32128 `0x7D80` | `0x7D82`, `0x7D83` | +2, +3 |
| 5 | 51842 `0xCA82` | `0xCA87`, `0xCA88` | +5, +6 |

So each press yields two IDs, at a variable 0–6 frames after the frame the lobby sees.

**Proposed way round (from the allocator's code, untested):**
1. Reserve with **IDs per tick = 0**. Then `base + 0·(tick − first)` = `base` for every tick.
2. The first creation gets `0xA0000000 | base`. Any later one, such as the second tick of the pair, gets the same ID, so the spawner refuses it because that entity exists.
3. The lobby then knows the rocket's ID. After the press it sends a ch4 create for that ID, then a fresh component 90 record with a new base for the next shot. The 6 s cooldown leaves time.
4. One record per definition (`0x072C`, `0x1C86`), since var842 picks one of the two.

## Fixed-ID reservation and confirm create (in game 2026-11-05, PID 37764)

- The lobby now reserves 0 IDs per tick (`soldier.helix_reservations`). On each press it sends a create for `0xA0000000 | id` (definition `0x072C`) to the shooter, re-reserves the next ID 0.5 s later, and destroys the rocket after 5 s.
- Lobby log: `helix rocket confirm entity=A1000000` … `A1000004`, one per press, at the press frame.
- Client heap: `0xA1000000` had 4 copies and `0xA1000001` 1 (a control ID: 0). So the client knows these entities. Whether its predicted rocket was adopted could not be read: the component-91 block is obfuscated.
- Screens: recoil and the Helix cooldown show; no rocket. The lobby's own `0x072E` glow was not seen either. The view stayed pitched at a ceiling 3–8 m away in every burst (≈110 grabs/s), so these captures do not settle whether rockets draw [unverified].
- **User check (same run, 2026-11-05 01:44):** Helix fired into the open room. It looks as before: one bright glow where the rocket goes, and **no rockets**.

**Status: open (stop rule: 3 in-game runs since the reservation fix without a visible rocket).**

What is proven:
- The client now issues predicted IDs from component 90 and creates `0x072C` entities.
- The lobby confirms the fixed ID with a create.

Not known:
- Whether the create adopts the predicted entity (component-91 block obfuscated).
- Why neither the predicted nor the confirmed entity draws.

Candidates for a next session, untested:
1. Emulate the create handler's existing-ID path (`0x7FF6D19AD2EE`) to read what it does with component 91.
2. Check whether `0x072C` needs movement or other components from the server, as the drawn child `0x072E` does today.

## Native visual startup and missing wire link — 2026-11-07

ProjectileVisual schema `2B1A5E8C` fieldA18274A3 is effect0977 at+08 GUID/+10 resolved pointer; field6CB564CC is0976 at+18/+20. Volley post-FX `1D064F0` starts0977 after an appearance lookup. Copied-memory control in PID33136 captured nativeHelix16 `15856B12A30`, config`158388317C0` selecting072C, one appearance. Lookup return at`1D0662A` was nonnull`158397BE448`; native effect creation returned handle`228410` at`1D06AA8`, without a fault. These gates passed in emulation; this does not prove that0977 or the projectile was visibly rendered in the live client.

Effect0976 is the type2 impact effect, selected by`1D4EC50` from schema+20 and queued into render-record+B0. Snapshot counter+100 drives it through`1D52960`. Initializing that counter to1 falsely plays the impact at the projectile's starting position. Its threeECEF→1953 references are not the travelling spiral.

Native initializer`1D42900` calls`1F8E590`, which sets component50+76=1. Snapshot handler`1D44600` uses the live component only when that byte is set; otherwise it queues the decoded state for later initialization. Thus a ch2 snapshot alone does not initialize a plain server-created072C.

The concrete initialization link is the optional entity list at the end of a ch2 shot event. Parser`1FA1F80` stores this list at event+60/count68. Pending-event processor`1D60D40` matches newly created projectile IDs against those lists (`1D61230`), requires a resolved shot config at event+90, then calls`1D42900` at`1D61BAC`. Existing `world.shot_record` sent no list. Its first list ID uses`1FD0F00`/`2A7B790`: two kind bits, then0+8 bits or1+16 bits, withFFFF followed by32 bits. This differs from snapshot/entity-channel IDs. Later IDs encode +1 directly or a signed4/8/32-bit magnitude.

World helpers encode these records. Offline pinned-image native decoders consumed all233 snapshot bits,219 single-ID shot bits and306 mixed-ID shot bits; decoded values matched nonzero position/velocity, counter1 and the intended IDs. Counter widths0/16/256/FFFFFFFF also passed. Tests preserve existing shot-only bytes and verify ch2 ordering, wire bounds and send/ACK/loss handling. `Match` now queues native startup after the ownerState16 frame is acknowledged, retains that state until the linked payload arrives and cleans up the projectile on impact/lifetime/body change. The owner's stepped072E fallback is suppressed; other viewers retain their existing presentation. Server damage, range and50m/s speed remain unchanged.

First live linked fixture in PID28288 initialized component50, but its shot time0 caused native simulation to catch up from tick1 through4285/8569/12853/17137. Static`1D6182E`→`1D61839` copies the pending shot event's time directly into the initialization request. `EntityUpdate.shot_time` now optionally supplies the current tick; default0 preserves existing callers. The first diagnostic also used forward/right as aim by mistake; the corrected fixture uses mover.yaw/pitch.

Corrected fixtureB2F40002 component50`1B8ED528500` was observed with+76=0 and zero state, then+76=1, sourceA0000101, current tick40690 and velocity(≈0,0,-50). Native position moved smoothly from z40.146 to8.991, where collision zeroed velocity and advanced both event and rendered-event counters1→2. The entity subsequently left the native map. An active Helix copied appearance lookup selected slot0 for keyB00010001, resolved handle74BC00 to entity1B8B620E448 and took no third-person fallback. A separate5m/s control, B2F40003, visibly rendered three dark rocket heads and spiral blue curves ([durable detail](verified-field-helix-2026-11-07/helix-spiral-5-diagnostic.png)).

Fresh B3000000 in PID34060 moved continuously at50m/s from(50.6275,2.8599,31.3071) to(42.1286,6.8067,7.9084), ticks14810–14882. Model flight was correct, but initial impact counter1 created a blue explosion beside the camera. Removing client reservations, changing local button state, holding Stop, and correcting muzzle position did not remove that explosion. A100ms link delay moved it farther away because the compensated snapshot position advanced; it did not fix the cause. No delay or local-button omission is saved.

The single-variable counter0 control removes the premature impact and shows the travelling cluster at50m/s. Final clean normal client PID33876 accepted the saved sender on ordinary RMB: [frame01](verified-field-helix-2026-11-07/helix-clean-50-f01.png) and [frame02](verified-field-helix-2026-11-07/helix-clean-50-f02.png) show the native cluster/blue curves flying without the camera explosion. Repeat/cooldown log: serial2/B3000001 at20:57:42.290, no cast from the20:57:43.451 cooldown press, serial3/B3000002 at20:57:49.713. No diagnostic wrappers were loaded. The sender starts with counter0 and keeps the muzzle event at the original launch point while advancing create/snapshot position. Regression exercises actual StartACK, native records, counter0, original muzzle, other-viewer fallback, repeat casts and impact/lifetime/body-switch/disconnect cleanup. All344 tests pass; final Field verification passed in the same normal client.
