# Statescript variable value wire format (client 1.74.0.0.104319, base 0x7FF6D1220000)

Static analysis only (IDB + runtime .text snapshot via `snapdis.py`). Nothing here was run in game.

## Reader chain

| Step | Address | What it does |
|---|---|---|
| Entity-var loop | `0x7FF6D1C2BD50` | per var: `w_var` id (0 ends), then value reader. Inner call at `0x7FF6D1C2BDFE` |
| Instance-var loop | `0x7FF6D1C2C270` | presence bit then value reader (call `0x7FF6D1C2C35C`); extras: `w_var` id loop (call `0x7FF6D1C2C40F`) |
| **Value reader** | **`0x7FF6D1C2D2C0`** (size 0x22b) | reads 4-bit tag (`0x7FF6D19ABDB0(reader, 4)` at `0x7FF6D1C2D367`); tag 15 reads 3 more bits; then payload; then binding list |
| Tag dispatcher | `0x7FF6D1F77DE0` | jump table (`base+0xD58258`, 16 entries) over the tag; tag 15 uses 2nd table (`base+0xD58298`, 8 entries, subtype) |
| Same, standalone | `0x7FF6D1F763A0`, `0x7FF6D1F779C0` | other callers (event params, array elements) |
| Binding reader | `0x7FF6D1C2D4F0` | the "compact binding count" and its entries |
| Primitives | `0x7FF6D19ABDB0` n-bit read; `0x7FF6D1C28E60` 1 bit; `0x7FF6D1F731B0` = server `w_var` (1b sel: 0->8 bit; 1 then 0->16, 1->32); `0x7FF6D1F73320` = server `w_u16` (0->4, 10->8, 11->16); `0x7FF6D1F73290` = sign bit + `w_u16`; `0x7FF6D2A6B470` = IEEE half -> float |

## Tag table (4-bit tag, LSB first, then payload)

Runtime type = the evaluated-value type enum stored at variant+0xC (1 Bool, 2 Int32, 3 Float, 4 GUID64, 5 string, 6 Vec3, 7 ScriptRef64, 8 4xU16, 9 UInt32, 0xA Entity32, 0xC array, 0xFF null).

| Tag | Payload bits | Runtime type | Evidence | Conf. |
|---|---|---|---|---|
| 0 | 1 bit | Bool | `0x7FF6D1F77E26` -> `0x7FF6D1C28E60`, setter `0x7FF6D2186AF0` | high (matches server) |
| 1 | 4 bits, unsigned | Int32 | `0x7FF6D1F77E40`, setter `0x7FF6D2186E10` | high |
| 2 | 8 bits, signed | Int32 | `0x7FF6D1F77E47` (movsx al) | high |
| 3 | 16 bits, signed | Int32 | `0x7FF6D1F77E64` (movsx ax) | high |
| 4 | 32 bits | Int32 | `0x7FF6D1F77E81` | high (matches server) |
| 5 | 4 bits unsigned -> float | Float | `0x7FF6D1F77E9D`, setter `0x7FF6D2186D00` | high |
| 6 | 8 bits signed -> float | Float | `0x7FF6D1F77EC1` | high |
| 7 | 16 bits, IEEE half -> float | Float | `0x7FF6D1F77EE5` (+`0x7FF6D2A6B470`) | high |
| 8 | 32 bits IEEE float | Float | `0x7FF6D1F77F0F` | high (matches server) |
| 9 | bit; if 1: 16-bit type, then if type!=0 `w_var` low32 (GUID = type<<48 or low32); if bit 0: 64-bit compact read (`0x7FF6D1F730E0`) | GUID64 | `0x7FF6D1F77F31` -> `0x7FF6D1F77A20` case 4 at `0x7FF6D1F77A87` | high for bit=1 path (matches server); bit=0 path not decoded |
| 10 | u16, u16 (angles, value-0x8000 scaled) -> unit Vec3 | Vec3 | `0x7FF6D1F77F44`, `0x7FF6D1FD1360` | medium (formula not worked out) |
| 11 | 3-bit mask (x,y,z); each set bit: 16-bit half float; unset = 0 | Vec3 | `0x7FF6D1F77F97` | high |
| 12 | 3-bit mask; each set bit: 32-bit float | Vec3 | `0x7FF6D1F7802A` | high |
| 13 | bit; if 1: 32 bits entity id; if 0: invalid-entity sentinel | Entity32 | `0x7FF6D1F78090` | high (matches server) |
| 14 | 3-bit kind, `w_var` instance, `w_u16` state, `w_u16` extra | ScriptRef64 (type 7) | `0x7FF6D1F780E2` | high (matches server) |
| 15 | 3-bit subtype `s` then: s=0 four u16 (type 8); s=1 u32 (UInt32); s=2 string, 8-bit chars to a 0 byte; s=3 no payload (null); s=4..7 array | see left | `0x7FF6D1F7813F`, table `base+0xD58298`; `0x7FF6D1C2D387`..`0x7FF6D1C2D3DB` | high s=0..3; array grammar (`0x7FF6D1F77430`) not decoded |

Array detail: decoded 2026-11-03, see `assault-mode-graph.md` (Capture HUD section). Older note: array detail seen only partly: s=4/5 read three `w_u16` (hashes ArrC/ArrS/ArrE), s=6/7 one `w_u16`, then elements via `0x7FF6D1F779C0` with per-element bit(s). Treat as unknown.

## After the payload: binding list (`0x7FF6D1C2D4F0`)

compact count (server writes `1` as bits `10`, none as `0`) then per binding: param count (compact, 0), bit "explicit instance" -> sign bit + `w_u16` instance (`'IIDS'` -> instance lookup `0x7FF6D2DAD3B0`), then the **state index**: width = graph `m_statesBitCount` (field +0x170) in an owner frame (header byte +0x4C set), else remote-table width (+0x174); then 3 flag bits: (a) output index `w_var` present (default 0, stored at binding+0x20), (b) numeric block (f32 or sign+`w_var`) present, (c) flag byte. The server's `StateBound` writes (a)=(b)=(c)=0.

Graph state_bits seen: graph 596 `m_statesBitCount`=7 (matches the verified `StateBound(...,11,7)`), graph 599 = 5.

## What a cooldown value is

**It is not a number the server needs to count down.** `out_CooldownVar` (StateAbility field `m_out_CooldownVar`, var 61) is a *state-output-bound* variable:

- Binding refresh `0x7FF6D2DC70B0` (loop at `0x7FF6D2DC7190`, call at `0x7FF6D2DC7281`): for each binding, if the bound state is active (`state+0x40` bit 0) it calls state vtable `+0x188` with the binding's output index and stores the result as the variable's value (copy to var+0x60). If the state is inactive the variable keeps its own sent value (fallback copied from var+0x50).
- StateAbility vtable `+0x188` = `0x7FF6D1EA4D00` (vtable `0x7FF6D383DB78`, +0x158 = DSR `0x7FF6D1EA4AC0`). Output index 0 returns a **Float (tag-8 type) of remaining seconds**: `max(0, [this+0x268] - (now - [this+0x260]) * 0.001 * [this+0x26C])`. Output index != 0 returns a Bool (`0x7FF6D2186AF0`). Constant 0.001 at `0x7FF6D3801868`.
- Those fields are the DSR timing block (`this+0x1E8` struct): `+0x268` duration f32 (seconds), `+0x26C` rate f32 (default 1.0), `+0x260` start time (same clock as `ctx+0x1E0`, scaled by 0.001, i.e. milliseconds). Wire (DSR `0x7FF6D1EA4AC0`, serializer `0x7FF6D1EA49A0`): after the u8 flags and linked-state bit: **timing bit; if 1: f32 duration, bit + optional f32 rate (absent = 1.0), `w_var` offset**. Start is rebuilt as `ctx[+0x430] * (clientFrame + 1) - offset` (`0x7FF6D1EA4C34`..`0x7FF6D1EA4C5F`); no timing bit -> duration 0, rate 1, start = now (`0x7FF6D1EA4C6B`..`0x7FF6D1EA4CCD`).
- The HUD reads var 61 as a number: value->float helper `0x7FF6D2184C80` accepts Bool, Int32, Float (and first element of an array), so the fallback payload may be Int (tag 4) or Float (tag 8). Dataflow expression [5] is `1 * (var61 mod var472)` (opcodes 6/8/8/26/24; op 26 = fmod at `0x7FF6D2DA2CE7`, op 24 = multiply `0x7FF6D2DA25F7`), expression [6] is var61 -> op 67.

### What to send for graph 599 var 61 (presence bit 5)

1. Variable: tag 8 `Float(15.0)` (fallback) wrapped as `StateBound(value, <graph599 instance index>, state=0, state_bits=5)` (StateAbility is owner state bit 0 of graph 599, `st=0`).
2. State 0 payload (StateAbility): `u8 flags, bit 0 (no linked state), bit 1 (timing), f32 15.0, bit 0 (rate default), w_var offset`. The offset is in the client's clock units (ms, see below): `offset = tickLen*(frame+1) - start`; offset ~ one tick length means "started now". While the cooldown has run its time the sweep reads 0 (clamped).
3. With no timing bit the output is `max(0, 0 - 0) = 0`: the ability reads as ready.

Not verified: that this is enough to make the sweep draw in game (nothing was executed).

## Could not determine

- Which tag the native `SetVar 3` (var61 = var472) would produce: var472 comes from config object `STU_7197C080` (STUConfigVarNumeric, fields Int 15, Int, conditional on var 9537/9528); its evaluator was not traced.
- Exact time unit of `ctx+0x1E0` / `ctx+0x430`: inferred ms from the 0.001 factor and the server's `quanta` range (1 ms to 1 s); not proven.
- Tag 10 exact angle formula; tag 9 bit=0 (64-bit) path; tag 15 array element grammar (`0x7FF6D1F77430`, `0x7FF6D1F779C0`).
- Whether the binding refresh (`0x7FF6D2DC70B0`) runs on every HUD read: HUD op 8 -> `0x7FF6D2DC3C80` -> `0x7FF6D2DC6AF0`/`6BE0` -> callers of `0x7FF6D2DC70B0` exist in that range (`0x7FF6D2DC6BB4`, `6C9C`, `6D10`), not traced further.
