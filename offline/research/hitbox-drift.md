# Hitbox drift in the Practice Range (2026-11-08)

Report: after moving around for a while, shots stop landing where the bots are drawn.

**2026-11-09:** the server's mover is now LizRollin's port of the client's own (research/lizrollin-runtime.md sections 5–6, `tools/lobby/player_mover.py`). It dead-reckons the client's feet without the feed: 0.039 m at most over a 57 s Range walk, 0.000 m at the end. The old mover's findings below are history.

**2026-11-10:** every hero but D.Va now gets owner corrections (research/lizrollin-runtime.md section 21): the client is put on the server's state whenever the two differ, so the server's copy cannot drift from it. `OW_CLIENT_SYNC` is needed for D.Va only.

## Answer

1. **The hitboxes don't drift; the shooter does.** The client never sends its position. The server moves its own copy of the player from the inputs (`mover.py`), and every hitscan shot starts at that copy's eye (`Match.fire_at_target`: feet + 1.6 m). Nothing ever corrects that copy, so every small mismatch adds up. Respawning resets it, because the new body is created at the server's position.
2. **The biggest mismatch: the client backs up at 90% speed.** The server moved backward at the full 5.5 m/s. In the session the report came from, 484 backpedal frames alone put the server's copy about 1.9 m off. **Fixed:** `mover.BACKPEDAL = 0.9`.
3. **Two smaller sources:**
   - **Props (fixed, not yet checked live):** the client stops at the table right of the Range spawn, and the server's copy climbed it (0.9 m in one bump). The table was in the mesh, but its top passed between the mover's knee and waist rays. The props' convex hulls, such as the small one at the table's end, were also missing from the mesh. Both are fixed; details under Next steps.
   - **Ramps and diagonals (open):** the client takes about 40 ms to speed up and 60 ms to slow down, and its back-diagonals bend slightly sideways. That adds roughly ±4 cm per leg and about 10 cm per back-diagonal leg.
4. **Tools.** The new tools draw the server's hit volumes over the game and measure the drift directly from the client's memory (read-only).
5. **Exact fix (opt-in, 2026-11-08):** `OW_CLIENT_SYNC=PORT` feeds the client's own position, tagged with its command frame, back to the lobby. After every command step the server's copy is moved onto it (section "Resync"). Live, the drift at rest stays 0.000 m after a long walk and stays under 5 cm while moving. Since the same day the feed runs inside the client (`client_feed` in `ow_fixes.asi`): at most 0.001 m while moving.

## Tools

The `OW_*` settings are environment variables, not `run.py` arguments. In PowerShell, set them before launching; they last for that window:

```powershell
$env:OW_HITBOX_VIEW = '47123'
$env:OW_CLIENT_SYNC = '47124'
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi
```

Remove one with `Remove-Item Env:OW_CLIENT_SYNC`.

| Tool | What it does |
|---|---|
| `OW_HITBOX_VIEW=47123` environment variable | `tools/lobby/hitbox_feed.py`: each match update sends the first player's server eye and aim, and every damageable bot's posed volumes, as one UDP datagram to 127.0.0.1. Off by default. |
| `tools/research/hitbox_view.py` | Click-through overlay on the game window. It projects the volumes **from the server's eye**, so they show exactly where the crosshair must be for the server to count a hit (red = head). It hides while the game is not focused. `--snapshot OUT.png` saves a capture with the overlay drawn in. `--drift` adds the client-position reading to the caption. |
| `tools/research/client_position.py` | Prints `frame F drift dx dy dz` (client minus server, metres, both at command frame F) twice a second. Works at any time, moving or not; the first find takes about 8 s. |
| `OW_CLIENT_SYNC=47124` environment variable | `tools/lobby/client_sync.py` resyncs the server's copy onto the client each command step. The `client_feed` module of `ow_fixes.asi` sends it the client's position (the game inherits the variable from `run.py`). Off by default. |

## The client's mover

The client's own mover object holds the simulated feet and the command frame they belong to (found 2026-11-08 from the spawn-position copies; 1.74.0.0.104319):

| Offset | Content |
|---|---|
| +0x00 | vtable `img+0x260E7C8` (IDA `0x7FF6D382E7C8`) |
| +0x20 | second vtable `img+0x260E880` |
| +0x70 | f32 x, y, z: the feet **after** that frame's command, always on the 1/1024 m grid |
| +0xA0 | u32: the command frame, equal to the server's `Command.frame` |
| +0xE0 | u32 7000: the command quanta (µs) |
| +0x2A0 | f32 5.5, 4.95: run and backpedal speed (the 0.9 backpedal is the client's own value) |

- The objects sit in an array 0x1610 bytes apart (16 found per map). The free ones hold zero positions and default speeds (5.5, 5.5).
- No image global points at the array; the entity pointers on the way are XOR-obfuscated (`volley_probe.entity_world`). So `client_position.ClientMover` scans private memory for the vtable (about 8 s in Python), then keeps the slots.
- A new body (respawn, hero switch) takes another slot, sometimes just outside the 16 found (`slot[0] - 0x1610` on a hero switch). The reader checks ±16 neighbours before scanning again.
- **Frame alignment:** a strafe's input changed at frame 74106, and the slot tagged 74106 had already moved. So the tagged position is after that command, the same point as the server's mover after `step`.
- The position and the frame are written separately. The reader takes a sample only when two reads agree.
- The client updates the slot about every 8 ms (some frames simulated in pairs). The client's other copies are render-interpolated (every ~4 ms) or history: a 64-entry ring of position, rotation quaternion and a double timestamp; it holds no frame.

How the old `client_position.py` worked (until 2026-11-08):
- It scanned for the server's spawn position, which appears verbatim in client memory right after a spawn (about 100–125 copies, found in 4.6 s).
- It then picked the copies that kept changing once you walked.
- It was exact only while standing still. Its "smoothed copy 1.3 m higher" was one of the render copies.

Projection:
- Vertical FOV is 70.5328° at every aspect ratio (research/aspect-ratio-32-9/REPORT.md).
- The viewport is the largest centred 16:9 rectangle in the window, or 32:9 when `aspect_ratio.asi` has it on.
- At spawn, the volumes sit under the bots' nameplates ([overlay-at-spawn.png](hitbox-drift/overlay-at-spawn.png)), including with a 4.4° up-pitch ([overlay-pitched-up.png](hitbox-drift/overlay-pitched-up.png)).

The drift as first seen:
- Old mover, after about 15 s of walking, jumping and strafing.
- One set of volumes sits about 1 m left of its bot, and another lands on a friendly bot ([overlay-after-walk.png](hitbox-drift/overlay-after-walk.png)).

## Client movement, measured

Method:
- The client's position was sampled at about 1 kHz from the live copies (render-rate updates every ~4 ms).
- It was compared with the server's replay of the same logged command frames.
- Game run windowed at 2560×720, Soldier: 76, flat floor in the Range spawn room.

| Input | Client | Server before |
|---|---|---|
| W | 5.5 m/s steady. Speeds up in ~40 ms, slows down in ~60 ms: about one extra 7 ms frame of distance per walk (+3.8 cm) | 5.5, instant |
| S | **4.88–4.95 m/s** (5.926 m over 171 frames, where the server moved 6.579 m) | 5.5 |
| A, D | 5.5 m/s (2.740 vs 2.739 m) | 5.5 |
| W+A | 5.55 m/s | 5.5 |
| S+D | 5.14 m/s, bent about 49° from straight back (the server bends 45°) | 5.5 |
| Crouch, either way | 2.95–3.0 m/s (no backpedal penalty) | 3.0 |
| Sprint | about 8 m/s (noisy) | 8.25 |
| Jump | v₀ 5.95 m/s, g 17.43 m/s², apex 1.014 m, 0.69 s in the air | 6.0 / 17.5 |

Other readings:
- The client's movement heading was within 25–54 s16 units (0.13–0.3°) of the command yaw.
- A client collision box read 1.0 m wide around the feet position (half-width 0.5). The server mover uses a 0.4 m radius, but that box was not confirmed to be the mover capsule.

### Model fit

The logged command blocks were replayed through candidate movers and compared with the client's resting positions: 16 legs, including two reversals.

| Mover | RMS error per leg |
|---|---|
| Before (no backpedal penalty) | 44 cm |
| `BACKPEDAL = 0.9` on the backward input component, after normalising | 5.9 cm |
| Plus fitted accel/decel ramps | 4.75 cm |

- The ramp gain is about one command frame of key timing, so ramps were left out.
- No ramp model explains the continuous walk's 0.46 m sideways drift. That drift came from the table, which the client bumps into and the server's mover missed (Next steps, item 2).

## Live result with the fix

The same 10-leg walk (`w 800 s 1000 w 600 s 800 a 400 d 400 s+a 500 w+d 500 w 400 s 600`), each leg 0.4–1.0 s:

| Run | Drift at the end |
|---|---|
| Continuous | 0.556 m |
| With a 1.2 s stop after each leg | 0.244 m |
| Before the fix (replayed) | about 1.85 m |

Per leg, with stops:
- **S legs:** 0.4–2.2 cm.
- **W, A, D legs:** +3 to 5 cm of overshoot each, cancelling on the way back.
- **S+A:** 15 cm. **W+D:** 8 cm.

## Resync (`OW_CLIENT_SYNC`, 2026-11-08)

How it works:
- `client_feed` (`tools/asi/client_feed.c`, in `ow_fixes.asi`) polls the mover slot every 0.5 ms inside the client. It sends each new `(frame, x, y, z, slot)` as a 24-byte UDP datagram to the lobby. Until 2026-11-08 this was `client_position.py --feed`, a separate read-only process.
- `client_sync.Resync` keeps the server's feet after each stepped frame and the client's feet per frame, 256 frames each.
- After each `mover.step` it takes the newest frame both sides have and moves the copy by client minus server at that frame. Frames up to it are used up, and the server's later ones move along.
- No datagrams, no change: the dead reckoning goes on alone.
- A new mover (respawn, hero switch) starts a fresh history.
- **The previous body's slot:** the client keeps simulating the old body for at least one frame after a switch. Without a guard, the first hero switch 4.6 m from the spawn moved the new body 4.5 m back to the old one. Samples from the previous body's slot are now ignored unless they agree within 1 m.

Live results (Practice Range, Soldier: 76 then Mei, 2026-11-08):

| Run | Drift at the same frame |
|---|---|
| Long walk, sync on: strafe into the spawn table, backpedals, diagonals, jumps, crouch, turns (about 15 s of input, 2046 frames logged) | 0.000 m at rest; moving: median 0, p99 3.6 cm, max 4.3 cm, none over 5 cm |
| Hero switch 4.6 m from spawn (Soldier: 76 to Mei), then a walk with a jump | no pull back; 0.000 at rest; max 7.7 cm on one airborne frame |
| Same long walk, feeder stopped (fallback) | 3.15 m at the end (peak 3.85 m) |
| Feeder restarted | corrected in one frame (`client sync frame=57968 moved 2.735 0.000 -1.568`), then 0.000 |

The overlay after all of that, 10 m forward through the spawn doorway: the volumes sit on both visible training bots, red head rings on their heads ([overlay-synced-after-walk.png](hitbox-drift/overlay-synced-after-walk.png)). The third set is a bot behind the wall; the overlay ignores occlusion.

Limits:
- **About one frame behind.** The client's sample for frame F usually reaches the lobby just after the lobby steps F (the client sends its command and the feeder reads the slot at about the same time). For that one frame the server's own mover fills in. That is about 4 cm while speeding up or stopping. In the air it is 3.6–7.7 cm, because the client keeps its air momentum and the server's mover has none.
- **One local client.** The datagram has no player id; the first player that steps gets it.
- **First find:** about 8 s per map. Until then (and whenever the feeder is off) the server dead-reckons.
- The previous-body guard assumes the client puts the new body where the server does (it does: the spawn point).

Finding for the props work: backing up from the spawn table (x about 59, z 44.0–44.5), the server's mover stepped up 0.39–0.50 m every frame (to y 1.39–1.50), onto something within its 0.5 m `STEP`. The client stayed on the floor (y 1.00, then 0.90). With sync on, each frame is pulled back down. Without it, that walk ended 3.15 m off.

## Dead end: native error correction

- The client registers `debug.DisableErrorCorrection` ("Disable movement error correction for local players"), and its reader at `0x7FF6D1A213D0` blends the corrected position over time.
- **Frame stamping works.** A ch3 movement record's frame is the packet frame minus a u32 (`0x7FF6D1E86D00`), so records can be stamped at the exact command frame the server simulated.
- **Creating the own body works.** A create stamped at that frame instead of 0xFFFFFFFE is harmless.
  - Correction 2026-11-08 (heroes/dva.md, known limit): stamped at the newest command frame, the spawn stood but the
    Range dummy took no cannon hits in two runs. Without the spawn's movement record, the body fell through the floor.
- **But records drop the camera.** Per-frame own-body records, stamped at the command frame or at the packet frame, put the camera several metres under the floor and keep it there ([own-movement-records-under-floor.png](hitbox-drift/own-movement-records-under-floor.png)). Walking still moves it sideways.
- **Why:** the block is a delta against a zero state (`0x7FF6D1E7F680`; about 20 fields), so every field we don't send reaches the client's own mover as zero. The server cannot fill them without the client's real mover.
- The code is obfuscated; Hex-Rays fails on the reader. The experiment code was removed.

## Next steps

1. **Exact feedback: done 2026-11-08, opt-in.** `OW_CLIENT_SYNC=PORT` (section "Resync"). To do:
   - Make it the default once the user has played with it.
   - The one-frame lag of the Python feeder is gone with the in-client feed (2026-11-08: at most 0.001 m over a 63.5 m walk, 54 samples at 2 Hz). Air momentum in the mover is still missing for when no datagrams arrive.
   - The feed is now the `client_feed` module of `ow_fixes.asi`, with the other client fixes. It still runs on its own thread and only reads game memory, and the lobby's side is unchanged.
2. **Props: done 2026-11-08, live check pending.** Details in research/map-collision.md, "Props".
   - **The table:** it is in chunk 0x11's static mesh. Its top is 0.70–0.95 m above the floor, starting 2.23 m along +x from the spawn and at z ≤ 41.7, so rays along the spawn's z miss it. The mover's rays at 0.55, 1.1 and 1.7 m passed between the top's faces, and the body stepped onto its 0.25 m shelf and climbed it. `mover.WALL_HEIGHTS` now runs every 0.2 m from 0.55 to 1.75 m. Strafing right at the spawn's yaw, the server's body now stops at x ≈ 56.3 and slides along the table edge instead of walking over it.
   - **The hulls:** the props' triangle shapes are baked into the static mesh, but their convex hulls are separate records in the same chunk (36 on the Range). `map_collision.py` now fan-triangulates them into the `.collision` file (`OWCOL2`; `run.py` re-extracts older files and `nav.py` rebuilds stale grids). Hull 32 (model `0x1217`), at the table's +z end, is 0.3 m wide with its edge 2 cm past the spawn's z. A centre-line mover slides off it, so `mover.SIDES` adds side lines at ±0.5 and ±0.9 of the radius. Walking +x along the spawn's z, the body now goes round it (z 42.15 → 42.52).
   - **Checks:** ray tests, synthetic narrow-post and table-top tests, and `bot_walls.py` (Range and Hanamura: 0 wall crossings).
   - **Cost:** about 3.2 ms per moving mover step in the spawn room (was 0.37 ms).
   - **Still to do:** the live check (`OW_HITBOX_VIEW=47123`, `client_position.py` at spawn, strafe right into the prop) has not been run, so the client's own deflection is unmeasured.
   - **Seen 2026-11-08:** backing up from the table, the server's mover steps 0.39–0.50 m up where the client does not (section "Resync", last paragraph).
   - **Not extracted:** the chunk's sphere and capsule records (none on the Range), and entity (chunk 0x0B) collision.
3. **Crouched eye height: done 2026-11-09, checked live.**
   - Live check (Range, Soldier: 76): the server eye was feet + 1.55 m standing (2.55) and + 1.085 m crouched (2.08).
   - The overlay's volumes sit on the drawn bodies in both stances: the mannequin's head circle on its head, and the far Range bots on their bodies ([standing](hitbox-drift/eye-standing-2026-11-09.png), [crouched](hitbox-drift/eye-crouched-2026-11-09.png)).
   - With the old 1.6 m eye, the crouched volumes would sit about 0.5 m low. Before this, every shot started 1.6 m above the feet, so crouched shots came from too high.
   - Shots now leave from `Player.eye()`: each body's own `STUFirstPersonComponent` heights, standing and crouched (`target.EYE_HEIGHTS`, from `tools/assets/eye_heights.py`).
   - Soldier: 76 is 1.55 m standing and 1.085 m crouched. Torbjörn is 1.0 m standing, Genji 1.2 m and Sigma 2.1 m.
   - The overlay feed draws from the same eye.
   - Shortcut: the stance switches at once, while the client's camera eases between the two.
4. **Shots at the shooter's view: done 2026-11-09, click aim and view delay checked live.**
   - Three single shots with the mouse still logged `click=(-32768, 0)` equal to `aim=(-32768, 0)` ([log](hitbox-drift/click-log-2026-11-09.txt)). A swapped yaw/pitch would read `(0, -32768)`, so LizRollin's yaw-first order holds.
   - On loopback `extra` is 82–84 ms and the sub-frame varies (192, 124, 255).
   - The view time lands 20–28 ms behind the server's clock: about 6 cm for a bot at patrol speed. The rewind matters little on loopback; the click aim and the eye matter more. This was ported from LizRollin's runtime and checked against our IDA (research/lizrollin-runtime.md).
   - A hitscan shot is tested against the bots as the shooter's client drew them. That is at the command frame's time less the view delay the command carries: the u16 extra plus the click's sub-frame lead, as the client's own hit test computes it (0x7FF6D17CEEC0). Each bot keeps 64 ticks of poses (`TrainingTarget.pose_at`).
   - A click aims where the crosshair was at the click (`Command.aim()`), not at the frame's end.
   - The `[game] click` log line prints both aims and the view delay. With the mouse still, the two aims must match; if the click aim looks swapped, the sub-frame's yaw/pitch order (LizRollin's reading) is wrong.
   - Projectiles still test the current poses at launch.
