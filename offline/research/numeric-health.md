# Numeric health "0/200" — investigation log (2026-11-02)

Status: **resolved (round 7):** the number is graph 463's expression over its synced vars 1732 (current) and 1737 (max); the server now sends them.

## Ruled out

1. **Display smoothing stuck (dt or rate 0).** The health-HUD presenter's update `0x7FF6D1C05260` (vtable `0x7FF6D3855B90`, slot 58) moves six cached pool values toward their targets with `sub_7FF6D2A6B010(rate, cur, target, dt, snap)`: it snaps when `|target-cur| <= snap` (2.5, or the max when the snap flag is set), otherwise adds `diff × rate × dt` (rate 7.5 by default). It then writes UI vars `56C4/654E/602B/654D/6701/6552` (slots), `5340` (total max) and `61E3` (entity id, int) only when a value changed. A live read-only dump (session PID 29916; `.scratchpad/read_health_hud.py`) found two presenter objects for entity `A0000101`, both with cached slot0 `+0x46C = 200`, missing `+0x484 = 0`, total `+0x488 = 200`, getter echo `+0x490 = 200`. The presenter's inputs and smoothing are correct.
2. **Health getter.** `sub_7FF6D17DCEE0` sums replicated health parts (20-byte entries from component `+0xD8`: dword present, `+4` max, `+8` current, bytes `+0x11`/`+0x12` filter). Filtered parts (`+0x11 != 0`, `+0x12 == 0`) add current to slot0 and max to the slot0 cap; others go to slot1. Our encoding takes the filtered path, matching the live 200 values.
3. **Entity dynamic var 19452 (UI `0x6872`).** Graph463 node224 lists 19452 as a remote-sync var, and UI asset 019C maps `0x6872` to it. Experiment: the server sent `19452 = Float(123)`. The number stayed `0/200`, but the bar gained a light-blue section and an icon. 19452 is a pending health-change preview, not the number. The experiment was reverted.

## Other observations

- No game code contains the immediates `0x0D80000000006872` or `0x0D800000000062B3`; the presenter holds them in a two-entry id list at `+0x3D0`, so they are driven by data.
- A key scan for `0x0D800000000056C4` found binding tables (`ffffffff000056c4` pairs at `0x2512382f898`) and several cells, none holding 200.0. The UI store layout (map at context `+0x40`, find-or-insert `0x7FF6D1807CF0`, value cell tag at `+0x10`: 4 = float, 9 = int) is not yet decoded, so this is not proof that `56C4` stays 0.

## Round 2 (same day)

- **UI var store decoded** (`.scratchpad/read_ui_store.py`): map `+0` buckets, `+8` entries `{u32 hash, i32 next, u64 id}`, `+0x10` 32-byte value cells (tag byte `+0x10`: 3 entity, 4 float, 8 UI id, 9 int). Live HUD stores hold `56C4 = 200`, `5340 = 200`, other slots 0, `61E3 = A0000101`, `62B3 = UI 0x32F4`, `6872 = 0`. **The presenter's UI vars are correct.**
- **Graph463 node77** (client-only presenter for UI `0x0A8800000000019C`) maps only `62B3 = dyn8202 ? UI615C : (dyn8203 ? UI6084 : UI32F4)` and `6872 = dyn19452`. Experiment `8202 = true` tinted the bar purple with a purple icon (status mode); number unchanged.
- **The number widget** is in widget template `0x0BA00000000002D1` (strings `healthBar`, `textBlock`, `slash`). Its text binds to property paths `Health` (hash `0xC9762625`) and `MaxHealth` (`0x72A64911`), plus `HealthProgress` (`0xFFF294F2`). Neither the names nor the hashes appear in the client's code or data sections, so the property→source mapping is resolved through data (UI `019C` holds obfuscated STUv2 ids).
- More experiments, all reverted, number still 0: health component 51 on the player entity as well as the body; part filter byte `+0x11` cleared (bar turns fully green); component-51 footers u32 set to current health and 77.

## Round 3: hardware breakpoints (user-approved, 2026-11-02)

- `tools/research/hw_watch.py` armed DR0-DR3 (read/write, 4 bytes) on all 148 threads for the current and max fields of both live copies of the health part (`0x1ad1c350120`, `0x1ad2a478af0`, max at `-4`). **0 hits in 10 s**, the post-run disarm failed on every thread, and the game crashed with `0xC0000005` shortly after detach (no crash dump). On a dummy process the same tool arms and disarms correctly (4/5; the fifth thread had exited).
- Reading: the client's protection hides threads from the debugger or resets debug registers, then crashes on the leftover state. **Hardware breakpoints are not usable on this client**; don't retry without a different approach.

## Round 4: decoded UI ids (stu_refs.py)

- UI `0x0A8800000000019C` declares the presenter vars `5340, 56C4, 602B, 6701, 654E, 654D, 6552, 62B3, 6872, 61E3` and a child UI `0x0A88000000000252` (field `0619C597`) bound to var `0x406`. Code at `0x7FF6D1CE62C0` writes `0x406` as a looked-up id next to `0x6745` (card identity), not health. Experiment: entity var 1030 (`0x406`) = 123 → number unchanged.
- Child `0252` binds `2EAB/2EAC/2EAD` and `4498/4499/449A` (graph463 remote-sync vars 11947–11949, 17560–17561). Experiment 11947 = 123 → number unchanged.

## Round 5 (2026-11-02, after the Assault capture work)

- **The presenter's current-health var is live.** Range, Helix rocket at own feet (server: 200 -> 160): the HUD var store (`.scratchpad/read_ui_store.py`) went `56C4` 200 -> 160 with `5340` = 200, and the bar shows 160, but the text still reads `0/200`. So the text's `Health` is not bound to `56C4`.
- **Property names are plain CRC32:** `crc32("Health") = 0xC9762625`, `MaxHealth = 0x72A64911`, `HealthProgress = 0xFFF294F2`. Only widget templates (0x0BA) carry them (2D1, 086, 08B, 1F7, 2DD, 2FA, 30D, 30E, 314, 31F, 329, 3BC, 3C0); no UI asset (0x0A8) does, and type 0x0D8 ids are not root-CMF assets. Each binding record is `{count 1, name length, crc32, -> name string}`, then a u32 that differs per binding (`0x5D6C1E6B` after Health and MaxHealth in 2D1, `0x5C8ACF54` after HealthProgress). It is not the CRC32 of a common property name.
- In live memory the name hashes appear only inside loaded template copies, with no 0x0D8 id beside them. The binding records' class vtables (`0x7FF6D3988DB8`, `0x7FF6D3988ED8`) are plain STU vtables, so the resolver is not reachable that way.
- Not it: body entity vars 7786/7787/7788 (graph51 HealthPool amounts) sent as Float 111/122/133 in the body frame. The number stayed `0/200`.

## Round 6 (2026-11-03, option 2 closed)

- Option 2 is already covered: `stu_refs.py` decoded 019C's ids in round 4, and the text binds by property name, not by a 0x0D8 id.
- Binding record in template `0x0BA00000000002D1` (Health at `0x38C4`): `u32 1, u32 name length, u32 crc32(name), u64 offset -> name string`, then after the padded name `0x540641B9` (the same for Health, MaxHealth and HealthProgress). Round 5's per-binding u32 was misread.
- `0x540641B9` is a reflection type hash: `sub_7FF6D14BFDD0` registers it (`sub_7FF6D2E09EA0`, size 24, align 8) under the name "Unknown" (names are stripped in this build). So it names the binding's value type, not its source. `5D6C1E6B`/`5C8ACF54` and `C9762625` do not occur in code.
- Also: child UI `0252`'s vars (11947-11949, 17560-17561, 0x406 = 1030) are the capture-HUD vars of graph 1824 (`assault-mode-graph.md`), so 0252 is the objective widget's data, not health.
- Left: find who consumes type `0x540641B9` records at runtime (the registration's callbacks `sub_7FF6D28B63D0`, `sub_7FF6D28B5B80`) to reach the property resolver. Low odds; stays parked.

## Round 7 (2026-11-04): resolved

- **The number is not the health presenter's.** `HUD_Hero_Common` (UX screen `0x09A000000000059D`) puts the player card element on template `0x0BA00000000002FA` with its data context bound to the path `PlayerCard`; that template's text blocks bind `CurrentHP` (the number) and `Health.TotalAmount` (the max), and hand the `Health` object to the bar template `0x0BA00000000002D1` (whose `Health`/`MaxHealth`/`HealthProgress` were the hashes found earlier). Templates are STUv1 (`DUTS`); their binding records (class `7D211E22`, path `B1A08D7B`, segments `80C108B9` = `{crc32(name) | len << 32, name}`) carry names only.
- **Names map to identifiers at runtime, in the binding objects.** Live (`.scratchpad/memscan.py`, `dump_ux_props.py`): each binding instance (vtable `0x7FF6D39D5B90`, 0xC0 bytes) holds the interned name key (`+0x18`, tagged pointer), the data context (`+0x28`) and the resolved identifier (`+0x80`): `PlayerCard` -> `0x0D80000000006092`, `CurrentHP` -> `0x0D8000000000612F`, `Health` -> `0x0D80000000000C7`. The card's data context (vtable `0x7FF6D39E8DD8`) has its own var map (`read_ui_store.py 0x0D8000000000612F`): `612F` = Int 0, `00C7` = the 019C health presenter object, `2E4B` name, `070F` portrait texture. The native health update (`0x7FF6D1C05260`) never writes `612F`; the registered dependency properties (`sub_7FF6D2F2F7C0`, 1057 live) do not include these names, and data-driven view-model types (`0x0C6` assets, registered from `0x0460000000000107` in the text CMF) do not either.
- **Who writes 612F: graph 463.** UI schema `0x0A88000000000125` declares `612F` (float) beside the `00C7` child presenter (-> UI `019C`); the HUD card schema is `0x0A88000000000280`. Graph `0x05800000000001CF` (463) binds them in its player-card presenter: instance 926 (`E870E3E8`, m_id `612F`) -> instance 927, an expression config var (`919FD47C`) over dynamic vars 478, 33688, 1737, 1732 (instances 928-931, all remote-sync vars of graph 463), constants (0.0, 1.0) and the bytecode `08 00 05 06 08 01 04 06 06 00 00 | 08 02 08 03 25 04 06 08 02 00 | 08 03 43 08 02 06 01 18 3f 00` (`.scratchpad/graph463.txt`; dyn records at +0x3e00/+0x3e30/+0x3e48 of the graph's dynamic area).
- **The expression VM** is `sub_7FF6D2DA1210` (class `919FD47C` vtable slot 5 -> `sub_7FF6D2DA0630`): `08 n` push operand n, `06 n` push constant n, `04 d`/`05 d` pop and jump by d if false/true, `25` equal, `18` subtract, `3f` min, `43` truncate to int, `00` end. So `CurrentHP = 478 or 33688 ? 0 : (1737 == 1732 ? 1737 : min(int(1732), 1737 - 1))`: **1732 is the current health, 1737 the maximum**, 478 the dead flag the server already sends, 33688 a hide flag (unset). The max shown comes from `Health.TotalAmount`, the native presenter's `5340`.
- Fix: `soldier.health_variables(health, most)` = `{1732: Float, 1737: Float}`, in the body frame's extra variables at body setup (`soldier.py`, `kits.py`) and on every player health change (`Match._send_player_health`, which marks the body dirty so the frame is re-sent). Tools: `tools/assets/stu_graph.py` (instances), `.scratchpad/find_id_users.py` (which assets reference an identifier), `.scratchpad/duts_keys.py`, `ui_bindings.py` (template bindings), `dump_ux_props.py`, `memscan.py` (live).

## Status: resolved

Checked in game (Tracer, `research/main-menu/numeric-health-tracer.png`): the card reads `150 / 150` from the first HUD frame. Not yet checked: the number after damage (`_send_player_health` re-sends the body frame with the new 1732).
