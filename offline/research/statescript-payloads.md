# Native statescript DSR bit reads

Build: retail `1.74.0.0.104319`; analysis-only IDA database `ida/Overwatch-state-analysis.dump`, image base `0x7FF6D1220000`. State factory registry resolves the Bool and Stack type keys to these runtime DSR callbacks at vtable offset `+0x158`.

| State type | DSR RVA / function size | Additional reader bits when third argument `r8b` is nonzero | Stored fields |
| --- | --- | --- | --- |
| `STUStatescriptStateBooleanSwitch` (key `0x2DF2DF8`) | `0x1B992D0` / `0xDC` | 1 bit | `this+0x238` |
| `STUStatescriptStateStack` (key `0x2DF2E38`) | `0x1B96460` / `0x1EE` | 2 bits, in order | `this+0x238`, then `this+0x239` |

Both routines read from a bit reader whose cursor is at `+0x1C` and bit count at `+0x18`. Each read checks cursor against count; on exhaustion it uses false without advancing. The Bool routine masks the next bit to one boolean, updates `this+0x238` only when changed, then dispatches state callbacks. The Stack routine similarly updates the two consecutive boolean fields, then runs its callback path. Neither routine reads additional serialized bytes or bits after those fields. When `r8b` is zero, each skips its DSR bit reads; the Stack routine still runs its post-read update path.

These counts are DSR-consumed bits only; the outer graph parser consumes the node's active/state bit before invoking the callback. Do not label the two Stack fields as “present”, “active”, or branch selectors without further evidence: this disassembly establishes their order and storage offsets, not their semantic names. The `r8b` flag's caller-side meaning is also unresolved here.

## Header mode and Begin ordering

The parsed per-entity context's byte at `+0x4C` (`+76`) is assigned by `sub_7FF6D1BD4160`: its first read from the context's subreader is stored there; only when that bit is true does it consume another bit into `+0x4D` (`+77`). State iteration in `sub_7FF6D1C2BE30` reads `+0x4C` to choose the owner-state table, then passes that byte unchanged as `r8b` to wrapper `sub_7FF6D2D92060`. The wrapper preserves `r8b` across its remote-state prelude and tail-calls the class DSR at vtable `+0x158`; there is no inversion. The local owner-frame writer emits `lead=0,H=1`, and the existing runtime-verified body owner frame successfully applied its owner state and rendered the rifle. This ties the subreader's first bit to H for that tested owner frame; the exact caller instruction that advances past `lead` was not recovered in this pass.

When an active state changes, `sub_7FF6D1C2BE30` stores the candidate pointers at object offsets `+0x910` and `+0x918`, calls generic Begin `sub_7FF6D2D90790`, checks whether `+0x910` still points at the candidate, and only then calls DSR wrapper `sub_7FF6D2D92060`. Generic Begin has no explicit bit-reader argument and no direct reader-cursor access in its recovered body. It dispatches indirect callbacks including graph/context vtable `+0x108` (`0x7FF6D2D908D1`), state-object vtable `+0x68` (`0x7FF6D2D908DD`), and state-object vtable `+0xE0` (`0x7FF6D2D908FA`). Any bit consumption during Begin would have to occur inside an indirect callback; the wrapper/DSR path is the only direct reader path shown by this function.

Evidence: IDA `analyze_function` assembly for `0x7FF6D2DB92D0` and `0x7FF6D2DB6460`; the registry-to-vtable mapping was read from the verified retail state-factory registry. Full function analysis was used because existing IDB function chunks decompile as `JUMPOUT`; IDB changes were not made. A bounded `undefine` request was rejected by automatic review, so analysis stayed read-only.

## StateAnim (`CA60E86E`, 2026-11-04)

How found, statically (no running client): the STU class registration site holds the class hash as an immediate (`mov r8d, 0CA60E86Eh` at `0x7FF6D14773D0`) next to the class descriptor `off_7FF6D4004588`; the runtime state factory records that point at that descriptor (`sub_7FF6D1EDC8E0`, kind 1 like StateAbility's; `sub_7FF6D1B2D780`, kind 2, a subclass) name the create function, whose constructor (`0x7FF6D1EDC1B0`) sets vtable `0x7FF6D3880420` (kind 2: `0x7FF6D3843540`, same DSR). The same walk from `C38B92B4` reaches StateAbility's known vtable `0x7FF6D383DB78`, which checks the method.

- vtable `+0x158` DSR `0x7FF6D1EDD370`, `+0x188` outputs `0x7FF6D1EDD170` (0..4: the five animation slots, a Bool each; 5+: extra slots), `+0xB8` Begin `0x7FF6D1EDD510` (starts the authored animations: `sub_7FF6D2D92F90` for slots 0..4).
- vtable `+0x168` is the base class's (`0x7FF6D2F05640`): **no mirror**, so the owner prelude reads nothing before the DSR.
- DSR: `sub_7FF6D2D91D80` (the owner prelude: `sub_7FF6D2D937D0` reads **one bit**, a linked-state reference, 0 = none; the same bit StateAbility's DSR starts with), then **3 bits** into `state+0x44` (u16; what it selects is not traced, 0 works), then **1 bit**: when 1, another bit; when that is 1 too, a compact value (`0x7FF6D2A7B800`: bit 0 -> 8 bits, bit 1 + bit 1 -> 32 bits, bit 1 + bit 0 -> 16 bits) into `state+0x348`, else `+0x348 = 0`.
- Minimal payload: **5 bits**, `0 000 0` (`kits.anim_payload`). A 4-bit payload (no linked bit) activated the anim state but ate the next state's present bit, so the Volley and path states after it never activated.
- Sent with the cast pulse (`kits.Ability.anim`, from `tools/assets/ability_kit.py anim()`: the path from the StateAbility to its first StateAnim, as `cast()` finds the Volley).

### In game (2026-11-04, Junkrat's Concussion Mine): the throw plays

**Result:** with the 5-bit payload and the cast's states held on for 1 s (`match.CAST_HOLD`), the first-person throw plays: the launcher swings down, the mine appears, the left hand tosses it, the launcher comes back (`abilities/cast-anim-junkrat.png`, 40 frames from the logged press). Sweep of the other 12 casts the same day: every one plays (Cassidy, Ana x2, Pharah, Widowmaker, Junkrat's trap, Ashe x2, Echo, Baptiste, Sigma, Symmetra; `abilities/cast-anim-*.png`).

**Two findings on the way** (live client, `.scratchpad/anim-peek.py`: finds the graph instance by its GUID qword at `inst+0x1E8`, instance vtable `img+0x262E740`, state objects at `inst+0xA0` by full state index, active = `state+0x40` bit 0; reads chunked to 512 bytes):
1. **33 ms pulses never reached the client.** With the start frame and the stop frame 33 ms apart, no state object of graph 2410 was ever created and the active set stayed `(0, 34)`; the same for Soldier's Helix Volley (graph 596 state 16, never created). The client applies owner frames at its own pace and the pulse was superseded before it looked. Held on for 1 s, the states activate. So until now no cast pulse (Helix, Biotic Field's throw, the 14 ability casts) had ever run on the client; their visuals were all server-flown entities.
2. **The 4-bit anim payload misaligned the frame:** Anim 2 activated (flags `0x4161`, `+0x344 = 0`, not blocked) but Volley 8, State 10 and Wait 13 never appeared. With the linked-state bit first, all of them activate and the client runs its own follow-ons (states 38, 39). The client's own anim state 7 (off node 14's finish) carries `+0x44 = 1`; ours carries 0 and plays, so the 3 bits are not the animation choice.

Before these two fixes: sent with the cast on a Shift press, the client accepted the 4-bit payload (no assert, no crash, the mine flew and the charge counter dropped), but no throw animation showed.

Where it blocks, as far as read: Begin (`0x7FF6D1EDD510`) first calls `sub_7FF6D1EE19A0`, which starts with the resolver `sub_7FF6D1EDF4C0` and then evaluates the node's animation fields (`+248`, `+256`, `+272..+328` of the authored STU node) through their expression objects (vtable `+0x20` with the graph context); `state+0x344` is set when the resolver returns 0, and then no `sub_7FF6D2D92F90` slot is started. The catalog shows those fields as null (expressions, not decoded), so the animation to play probably comes from an expression over a variable the server never sets, or from the entity's animation component, which the body may lack. Not traced further; `sub_7FF6D1EDF4C0` and `sub_7FF6D1EE5810` do not decompile in the analysis DB. Next: read `state+0x344` and the resolver's result on a live client after the press (`tools/research/live_peek.py`), or compare with a state the client runs itself (Soldier's sprint Anim 15 in graph 597).

## TrackRay, `STU_DEBD057B`, TrackTargets, `STU_9D7BF987`, GameMessageEntry (2026-11-06, tracer-verified)

Found statically (class hash -> registration descriptor -> constructor -> final vtable: DSR `+0x158`, mirror collector
`+0x168`) and checked in the client with `tools/asi/tracer.asi` (`research/asi-tracer.md`): a `data` line gives the
frame bit where a state's DSR starts and how many bits it reads, so a wrong width shows as the next states' positions
drifting. Runtime objects are made by `sub_7FF6D2DB1460`: the registry's factory for the node's class, else a plain
base state (`sub_7FF6D2D8F760`, vtable `0x7FF6D39CC800`, whose `+0x158`..`+0x170` are all the empty stub
`0x7FF6D2F05640`).

| Class | Runtime vtable | Bits after the framing | Evidence |
|---|---|---|---|
| TrackRay `164EF13B` | `0x7FF6D3841BF8` (ctor `0x7FF6D1B11FE0`) | **none**: DSR `0x7FF6D1B16DB0` only prunes the ray list by the current tick (`sub_7FF6D1B1C1C0`), collector empty | Sombra's weapon graph `0B1B`, owner state 32: `at=1084 bits=0`, the switches after it still read the kit's values |
| `STU_DEBD057B` | `0x7FF6D38594B8` (ctor `0x7FF6D1C26740` -> `sub_7FF6D1C22AB0`) | **none**: DSR and collector are the empty stub | `0B1B` owner state 68: `at=1110 bits=0`; Moira's orb graph `10E4` state 17: `at=1712 bits=0` between the Wait (1711) and the base state (1713) |
| TrackTargets `41E3C7A7` | `0x7FF6D383EAE8` | has-targets bit; when set a server `w_u16` count, then per target a 32-bit entity and a `w_var` tick offset (collector empty) | `kits.sombra_hack_states` already sends one target this way; `'targets'` kind = the no-target bit |
| `STU_9D7BF987` | none registered -> base state | **none** | static only: no factory, so the base stubs run |
| GameMessageEntry `9D6CF8AC` | DSR `0x7FF6D1C20BE0` -> the system's vtable `+0x148` with (state, 0) | one bit (0) after the start bit, as `statescript.message_state` sends | calibrated live on `0D28` earlier |
| ChaseVar `6AE5D1A1` | DSR `0x7FF6D2DB8400` | as `soldier.chase_payload` (research/abilities.md) | Tactical Visor |

Framing, measured on owner full frames (Torbjorn `1683`, Sombra `0B1B`, Moira `10E4`): one bit per owner state, then
the state's mirror prelude and DSR; a LogicalButton's prelude adds one bit before its 4-bit DSR. So a payload-free
class costs the vendor one bit (`None` in a kit), whatever its runtime class.

The classifier `0x7FF6D217E960` (called by the state readers `sub_7FF6D1C2BE30` and `sub_7FF6D1C2C620`) is not a
Begin-bit switch: it answers "does this node's class hash have a registered state class", true for every class above
including `9FC136D7` and `9D7BF987` (whose runtime objects are the base state) and false only for
`STUStatescriptState` itself (`7C37840C`). In the remote reader the false branch reads one bit fewer (desired-active
only, no control bit) before the DSR; no kit sends a `7C37840C` node as a remote slot, so that path is unmeasured.
Frame readers found on the way: `sub_7FF6D1C2C270` reads the instance sync vars (presence bit each, then the extra
list), `sub_7FF6D1C2BA30` the per-instance events (2-bit codes until 3).

`STU_54FEB21C` (Reinhardt's Fire Strike path, graph 226 state 5; also in the melee graph 67): registration key RVA
`0x2DE0A68`, constructor `0x7FF6D1B17CC0` -> `sub_7FF6D1B125B0`, final vtable `0x7FF6D383D060`, whose `+0x158`..`+0x170`
are the empty stub: **no bits**. Verified as Reinhardt (E): graph `0E2` Volley 0 (33 bits at 1624), Wait 1, Anim 3 (5
bits), Wait 4, `STU_54FEB21C` 5 (0 bits at 1667), StateAbility 9 (8-bit prelude, 44-bit DSR at 1679), base state 10,
Wait 16 (`research/asi-tracer/range-reinhardt-2026-11-06.log`). No class on a kit route is left undecoded.
