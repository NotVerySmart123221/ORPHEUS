# Soldier: 76 first-person fire kick: static findings (build 104319)

Read-only. Image base `0x7FF6D1220000`. Code read from the runtime text snapshot (`runtime-weapon/text.bin`); many functions are wrapped in anti-tamper junk (opaque `jcc` pairs, `stc; jbe`, return-address range checks), so IDA often shows `JUMPOUT` stubs. Confidence tags: [H] high (instructions read), [M] medium (inferred from code shape), [L] low (guess).

## Verdict (short)

- The reader of `m_animRecoilImpulseFactor` was **not found**. [H that I did not find it]
- A server-driven Volley shot **does** go through the full per-shot executor (muzzle FX, tracer job, post-shot effects). No branch tied to "locally predicted vs server-sent" blocks it. [M-H]
- There is exactly one per-shot call that looks like "play an animation-ish STU on a component": `0x7FF6D1D06DA4`. Whether it runs for us, and what it plays, is unproven. Check it with a probe (below).
- No payload change is proven to fix the kick. The next step is a runtime probe, not another blind payload variant.

## 1. Lead 1: the recoil factor

- Reflected field entry (0x38 stride, name hash at +0x30): `0x7FF6D43FA7E8` = `{owner 0x7FF6D3801640, off 0x18, type 0x7FF6D41BFB80, flags 2, hash F3E60FB5}`. Siblings in the same class: `c9d22faa` @+0x8 (nested struct type `0x7FF6D43FD0D0`), `addace26` @+0x20 (same type as the factor). Class meta `F18160D1` at `0x7FF6D43F6630`: 3 fields, size 0x28. [H]
- Type `0x7FF6D41BFB80` is shared by every ConfigVar-valued field (offsets step by 8). So STU `+0x18` is a **pointer to an STUConfigVarFloat**, not a raw float. A scan for `movss/mulss [reg+0x18]` after a `+0xC0` load across all of `.text` found nothing in the weapon code (9 hits, all unrelated stack/array code at `0x7FF6D2B76429`, `0x7FF6D319912E` and similar). [H]
- Reason: the Volley does not read the STU at shot time. It reads a **resolved per-weapon info struct**:
  - `0x7FF6D1F7E030` (`this`): returns `[this+0x1E0]`. If empty it does a table lookup. `rbx = (0x4E3494C633194824 - [rip+0x2186B5D]) ^ [[entity+0x28]^0x98DE97B50B3BE1E3 + 0x140]`, then `0x7FF6D1FA6660(rbx[byte]+0x20, this+0x1C0)` keyed by the state's STU pointer `[this+0x1C0]`. [H]
  - The struct is held with a refcount at `[this+0x1E8]` and cleared on exit (`0x7FF6D1D06048`, `0x7FF6D1D07451`).
  - Offsets seen in it: `+0x24`, `+0x30` (shot count), `+0x40` (pointer, see section 3), `+0x4C` (float, compared at `0x7FF6D1D08A48`), `+0xC0` (pointer to a sub-struct whose `+0x20` is non-null), `+0x120`/`+0x123` (bytes tested in the update at `0x7FF6D1D05EA6`/`05ECE`).
  - I could not find the builder of this struct, so I cannot map `+0xC0` to `m_projectileVisual` with certainty. [M that `+0xC0` is the projectile-visual block: the shot executor refuses to spawn when `[[vol+0xC0]+0x20]==0` (`0x7FF6D1D071B4`-`071C9`), and copies `[[vol+0xC0]+0x20]` into the shot record at `+0x30` (`0x7FF6D1D072E6`-`072F1`).]
- Not determined: which component receives the impulse, and under which conditions.

## 2. Lead 2: path of a server-driven shot

Vtable `0x7FF6D38747A8`: `+0x70` = update `0x7FF6D1D05D90`, `+0x158` = DSR `0x7FF6D1F80400`, `+0x1D0` = **per-shot executor `0x7FF6D1D06DF0`** (read from `0x7FF6D3874978`). [H]

1. DSR (`0x7FF6D1F80400`) stores time `+0x1F0` and count `+0x280`; `+0x2CA` = "count increased". [H, matches weapon-volley-dsr.md]
2. Update `0x7FF6D1D05D90`: `0x7FF6D1F7E030` (resolve info) -> `0x7FF6D1F81070(now - [this+0x1F0])` -> `0x7FF6D1F813D0`. [H]
3. Scheduler `0x7FF6D1F81070`:
   - Loop A: while `[+0x278] < t` and `[+0x270] < [info+0x30]`, call vtable `+0x1D0` (the shot executor).
   - Loop B: `[+0x260]++; [+0x268] = 0x7FF6D1F802E0([+0x260])` while `[+0x268] < t`. This is a time-to-shot-index fast-forward and has no callback. [H]
   - `0x7FF6D1F802E0` converts a shot index to a time with the rate and ramp floats at `+0x290..+0x2A0`.
4. Shot executor `0x7FF6D1D06DF0`:
   - It sets `[this+0x440]=1` and calls the **dedup filter `0x7FF6D1D07D50(this, shotIdx)`** (`0x7FF6D1D0714B`).
     - The filter keys a 24-byte history list at `+0x3D0/+0x3D8` on `(state event counter (+0x44)&0x3F, shot count +0x280)`. Entries older than 2000 time units are dropped.
     - Returns 2 if new, 1 if seen with a larger shot index, 0 if already played.
   - The shot is played when the result is 2, or when it is 1 and `[[this+0x78]+0x318]` bit 10 is clear (`0x7FF6D1D07154`-`0x7FF6D1D0716F`). Result 0 means skip. [H]
   - **This is the only predicted-vs-server branch I found.** It dedups a locally predicted shot against the server's confirmation. A fresh server shot returns 2 and plays. It cannot explain a missing kick, because the tracer comes from the same path. [M-H]
   - Then it requires `[this+0x1E0]!=0`, `[[info+0xC0]]!=0` and `[[info+0xC0]+0x20]!=0`, and builds a shot record: muzzle position via `0x7FF6D1F82020`, direction via `0x7FF6D1F81F30`, floats via `0x7FF6D1F7FF60`/`0x7FF6D1F7EAA0`, and fields up to `+0xE0`.
   - It submits the record as an async job (`0x7FF6D2A9A290` allocates `0x190` bytes, `0x7FF6D1FACFF0` fills it, `0x7FF6D2AE9010` submits).
   - It then calls `0x7FF6D1D064F0(this, shotIdx)` (post-shot effects), then `++[+0x270]` and recomputes `[+0x278]`/`[+0x268]`.
5. `0x7FF6D1D064F0` = post-shot effects:
   - Loop over the per-shot effect list `[this+0x2F0]` (count `[+0x2F8]`, 0x40-byte entries) matching `shotIdx % [+0x2A8]` to the entry key; spawns each through `0x7FF6D1BC0C90` on the manager at `entity+0x11B0`. This is likely the muzzle FX, which does play for us. [M]
   - **At the end (`0x7FF6D1D06D5B`-`0x7FF6D1D06DA4`):** if `[rsp+0x44]` (set at entry from `0x7FF6D17A74F0(comp, ownerEntity)`) is true and `rbx = [info+0x40]` is non-null, it calls `0x7FF6D17A1330(comp)` (decrypts a pointer, so likely a model or anim object), `0x7FF6D2D91D10`/`0x7FF6D1F7FB50(this, ..)` (time conversion), then **`0x7FF6D17EFB90(animObj, rbx, timeMs)`**. [H for the instruction chain, L for its meaning]
6. After the update (`0x7FF6D1D0601F`-`0x7FF6D1D0602B`): if `[this+0x440]` is set it runs `0x7FF6D1D03C70`. That is a **rollback snapshot** (`0x7FF6D1F81130` serializes `+0x1F0/1F8/260/268/270/278/280`, and `+0x40&1` gates a fuller dump). It is not animation. [H]

Flags worth knowing: `[this+0x40]&1` (owner/predicted flag, used in `0x7FF6D1D089A7` and `0x7FF6D1F81165`), `[this+0x2E0]` (calls `0x7FF6D1D08950` before the shot, which iterates the same `[+0x2F0]` list and the `-1` slots at `entry+0x30`; likely pre-fire or continuous effects), `[this+0x78]+0x318` bit 10.

## 3. Lead 3: the one animation-like trigger

- `0x7FF6D17A74F0(comp, entity)` returns `entity != 0 && entity == 0x7FF6D17A3990(comp)` (real body at `0x7FF6D17A76B7`). [H]
  - `comp = 0x7FF6D1B99210(0x7FF6D2D91C50(this))`.
  - `0x7FF6D2D91C50(this)` = `[this+0x78]->vtbl[+0x70]()` (the context's entity accessor). [H]
  - `0x7FF6D1B99210` fetches a typed component (`[0x7FF6D1F2D9B0(x)+0x28] ^ 0x98DE97B50B3BE1E3`, then a vtbl `+8` type check against the type object at `0x7FF6D3F1F278`; hash bytes `9E A1 8B B1 E3 F1 A6 08`). [H]
  - `r13 = 0x7FF6D1EE6EF0(this)` = the state's own entity.
- So the final "play" runs only when that typed component belongs to the same entity that owns the statescript. If the component sits on a different entity (for example the first-person weapon or view entity), the predicate is false. [M]
- `[info+0x40]` is a pointer, not a flag. Its STU class is unknown. I could not identify what `0x7FF6D17EFB90` does (anti-tamper-wrapped body).
- No separate "anim event" or blend-tree parameter write was found in the Volley path. The Volley holds no animation id beyond `[info+0x40]`, consistent with graph 596 having only reload Anim states. [M]

## What a server-driven shot is missing

Not proven. Ranked candidates:

1. [M] The `0x7FF6D1D06DA4` play is skipped or silently no-ops: either `[info+0x40]==0` for this volley, or `0x7FF6D17A74F0` is false because the typed component's owner is not the statescript's entity. This is a server-world/entity-binding question, not a Volley-payload question.
2. [L-M] The kick comes through the async shot job (record `+0x30`, `+0x70`/`+0xD4` floats, `+0xE0` block) when it is consumed by the owner's projectile/view system. I did not locate that consumer.
3. [L] The kick is driven by something other than the Volley (a LogicalButton-driven weapon animation layer). Nothing in the Volley path or graph 596 supports this either way.

I found no Begin flag, time-delta or prediction flag that gates the animation path. The only owner/predicted flag in the path is the dedup filter, which a fresh server shot passes.

## Minimal change to try

None proven. Cheapest discriminating experiments, in order:

1. **Runtime probe, no payload change:** break or hook at `0x7FF6D1D06D5B` (the `test al,al` before the play) and `0x7FF6D1D06DA4`. Read `[rsp+0x44]` (predicate), `rbx` (`[info+0x40]`), and the class hash of the STU `rbx` points to. Outcomes:
   - `rbx==0`: the animation is not data-driven here, so look elsewhere (lead 2 in the list above).
   - `rbx!=0` and predicate false: fix the entity/component binding on the server side. The class hash of `rbx` tells what animation it is.
   - Both true: the play happens, so the problem is downstream (`0x7FF6D17EFB90`, the animation layer on the first-person model).
2. Read the return of `0x7FF6D1D07D50` (`0x7FF6D1D07150`): confirm it is 2 for each server shot.
3. Re-run the previously tried payloads only after the probe. The holds, ChaseVars and Stack/DataFlowMapping variants already tried touch none of the code above.

## Could not determine

- The consumer of `animRecoilImpulseFactor`, and whether it is applied for server-driven shots.
- The builder of the resolved info struct (`0x7FF6D1FA6660`), so exact mapping of `+0x40`/`+0xC0` to STU fields is unconfirmed.
- The meaning of `0x7FF6D17EFB90`, `0x7FF6D17A1330`, and the type at `0x7FF6D3F1F278`.
- The consumer of the async shot job.
- Whether the camera recoil is driven from this same executor or elsewhere.

## Runtime check (2026-11-02, read-only)

Tools: `tools/research/volley_probe.py` (memory reads) and `tools/research/live_emu.py` (runs client functions in unicorn over copied pages; nothing is written to the game). Session `20261102T152451`, PID 18520.

1. **The rifle Volley is one long-lived object** at a heap address, vtable RVA `0x26547A8`. Its fields fill only while it is active: key `+0x1C0` = `A000010100080A01` and flags `+0x40` = `0x40E1` during a burst; both reset (key 0, flags `0x4060`) when idle.
2. **Info lookup decoded:** `entity = ROL64([img+0x2EE47FC], 2) ^ 0xF320E42472A94F14 ^ [[[inst+0x10]+0x528]+8]`, `world = [entity+0x28] ^ 0x98DE97B50B3BE1E3`, `table = [world+0x140] ^ (0x4E3494C633194824 - [img+0x2EE4BD0])`, component `table[34]`; `+0x20` holds 8 buckets (stride `0x90`) of `(info, key)` pairs.
3. **Info for the rifle:** `+0x28` = 9.0 (shots per second), `+0x30` = 99999 shots. **`[info+0x40]` is not null.** It points to the anim-recoil block. Its floats are 0.1 (`animRecoilImpulseFactor`), 0.8, 4.0, 0.4, 0, 0.1, 0.05, 0.3 and 1.0.
4. **All gates pass:** owner predicate `0x7FF6D17A74F0` = true, and anim target `0x7FF6D17A1330(component)` is non-null. Emulated under the TLS of all 146 game threads; same result every time.
5. **What the play call does:** `0x7FF6D17EFB90(target, anim, ms)` writes a kick slot at `target+0x14F8`: a count, an anim pointer and the floats from item 3, plus an active flag at `+0x1550`. It sets that flag to 1 for every shot age tried (0, 50, 299, 301, 1000 ms and −100).
6. **Live, the slot is filled but never re-triggers.** It already holds the item 3 floats with active = 0 and elapsed ≈ 0.3 (finished). Across a 14-shot server-driven burst, polled every ~5 ms, neither the active flag nor the elapsed time changed.

**Verdict:** the gun-kick call is not reached for server-driven shots. Its data and gates are fine. The slot being filled shows it ran at least once this session (cause unknown). The skip happens upstream of the spawner's tail: either the executor `0x7FF6D1D06DF0` does not run per shot, or it leaves before calling the spawner at `0x7FF6D1D073D5`.

Next check (needs an idle desktop for foreground fire): watch the Volley's executor fields (`+0x270` shots run, `+0x278` next shot time, `+0x440` executor flag) during a burst. If they stay still, server counts feed the visuals through another path (`0x7FF6D1D07570`, the second spawner caller) and the executor loop is gated (scheduler `0x7FF6D1F81070` compares `+0x270` with `[info+0x30]`, and `+0x278` with the time).

## Runtime check 2 (2026-11-02, PID 12408)

Fields polled every 1 ms (`.scratchpad`-style watcher; same read-only calls as `volley_probe.py`). Foreground fire has a 10 s countdown, so watch windows must be at least 25 s.

1. **Only one Volley object exists in the process** (one hit each for the primary vtable, the secondary vtable at `+0x8`, and the one at `+0x2D0`). It is the rifle's.
2. **Before the first shot the kick slot is empty** (count 0). The first burst fills it. So the play call does run live, but rarely.
3. **During an ordinary held burst the Volley never activates.** Over 14 shots (serials 31–44) and again over 23 shots (to serial 83), `+0x40` stayed `0x4060` and `+0x1C0`, `+0x270`, `+0x278` and `+0x280` all stayed 0. The ammo HUD still counted down, the muzzle flash still drew, and the camera still climbed.
4. **It activated only as the magazine ran out** (2-round burst, serials 29–30):

   | Time | Fields |
   |---|---|
   | t+0 | Key `A000010100080A01`, `+0x260` = 1, `+0x270` = 1 (one shot executed), `+0x278` = `0x6F`, `+0x280` = 1, rate `+0x290` = 9.0 |
   | t+0.14 s | `+0x260/270/278` reset |
   | t+0.19 s | Key cleared, rate back to 1.0 |

**Reading:** the server's per-shot owner frames (one full frame per rifle update, `match.py`) update the instance's variables but almost never leave Volley 10 running. The one activation at the empty-magazine/reload boundary is the only time the executor, and with it the gun-kick play call, ran. The muzzle flash comes from a separate effect state, not the Volley.

Leading hypothesis (not proven): graph 596 is the local player's predicted instance. The client rolls the server's state back to the command frame and re-simulates it with its own prediction, and that prediction deactivates the Volley in the same tick, so the 1 ms watch never sees it. Evidence for prediction: `[Volley+0x40]` bit 0 (the owner/predicted flag in the executor's dedup filter) is set while active (`0x40E1`) and clear while idle (`0x4060`). If the hypothesis holds, the fix is to make the client's own prediction fire the Volley: the LogicalButton route and its gate variables, which the server must keep consistent. More server-sent Volley payloads would not help.

## Prediction experiments (2026-11-02)

All of these were tested in game, then reverted. Normal play is unchanged (verified: steady recoil of about −45 pitch units per shot).

1. **The client does roll back and replay.** With fire triggered only by the server (the client not holding fire), the Volley went active and its executor ran shots. Its counters jumped back within the same millisecond (count 3 → 1, executed 2 → 1). In that mode the kick slot fired on every shot (active flag on, elapsed 0 → 0.083 s), but its output was only about ±0.0003, and the gun marker stayed within ±1 px.
2. **Held fire leaves all 85 states untouched.** Over a whole held burst none of graph 596's state flags changed (`+0x40` of every state in `[instance+0xA0]`). The client's button state st8 counter (`+0x44`, a 16-bit press counter that the DSR unwraps from 4 bits within a ±8 window) stayed 0, so the client never predicted a press. The LogicalButton's own update and begin/end slots are empty `ret 0` stubs; presses go through an input manager handle at `+0x50`.
3. **The gate variables are not the block.** var842 = Tactical Visor active (set by graph 601). var2355 = Helix firing (set by graph 597). var22750 is read by every hero's weapon button and written by no graph (an engine setting).
4. **The instance descriptor's flag bit is a prediction switch.** Setting it on the rifle instance (statescript `_descriptor`, the bit after the source entity) made held-fire Volleys run: Volley active for the whole burst, the executor running shots, and the kick slot firing on each shot. The client accepted it with no assertion.
5. **The Volley's first signed DSR value is "ms since the Volley began".** The DSR sets `begin = CmFD × 7 ms − value` (checked: `0x14D6E` = 12194 × 7). The server always sends 0. Sending the real elapsed time (from the burst's first shot, measured at the frame's CmFD) stopped the rollbacks (shots 1 → 10 executed cleanly). **But camera recoil then grew quadratically** (−35, −249, −823, −1839, −3411, … up to ±15000, view on the floor), with or without the prediction flag. Each update makes the client re-run every shot since the begin time, and camera recoil is applied on each re-run. With 0 it re-runs exactly one shot per update, which is the current steady recoil.

**Where this leaves the gun kick:** the anim-recoil block (`[info+0x40]`, impulse 0.1) drives a spring on the owner's per-frame history object, and it shows as camera motion. In none of these runs did the first-person gun model visibly move. A faithful fix needs the client's prediction to own the Volley (flag set) while server updates neither replay nor re-apply shots. Likely ingredients: send the begin time that matches the client's own prediction, and confirm (ack) every frame so replays stay short. Untested; this is the next step if the kick is pursued.
