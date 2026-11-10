# WeaponVolley state DSR (retail 1.74)

Read-only IDA review of the installed 1.74 client. Graph 596 state-list entry 10 is a WeaponVolley node; graph evidence and its links are summarized in [soldier76-weapon-graph.md](soldier76-weapon-graph.md#findings). Runtime addresses below use image base `0x7FF6D1220000`.

## Constructor and dispatch

- Graph 596 state 10 schema getter: RVA `0x16303F0`, metadata at VA `0x7FF6D43F7300`.
- Metadata class key object: RVA `0x2DFD2D0`; constructor registry resolves it to stub RVA `0xAE64E0`. The stub branches to true constructor RVA `0xAE3280`.
- True constructor assigns primary vtable VA `0x7FF6D38747A8`. Vtable `+0x158` dispatches to DSR VA `0x7FF6D1F80400`, **RVA `0xD60400`** (`sub_7FF6D1F80400`). Vtable `+0x168` points to `0x7FF6D2F05640`, whose bytes are `C2 00 00` (`ret 0`): no sync-variable descriptors.

## DSR reader grammar

At `0x7FF6D1F80400`, `a3` gates the first update block and `a4` gates the additional integer/trailer block. Owner-frame evidence maps the normal custom-state call to `H=1` and constant `r9=1`, so both branches execute for the owner Volley state. The fields' exact gameplay semantic names remain unknown.

- If `r8b` is set, `sub_7FF6D2D936A0` reads a signed variable-width integer: one sign bit; one width bit; when extended, one more width bit; then 8, 16, or 32 magnitude bits. The helper negates the magnitude when the sign bit is set. DSR uses the value to update its internal timing/index state and invoke downstream state callbacks.
- If `r9b` is set, `sub_7FF6D1F80580` reads an optional integer. A zero first bit yields value `1` with no more payload. A one first bit calls `sub_7FF6D2A7BA50`, which reads a second selector bit and then 4, 8, or 16 value bits: first selector zero gives 4 bits; first one plus second zero gives 8; both one gives 16. DSR stores this value and dispatches callbacks.
- The DSR then consumes a 6-bit field through `sub_7FF6D19ABDB0`.

The bit widths follow the branch values passed to the generic bit reader in those helpers. The first signed value feeds an internal time/index calculation (`[this+0x48]`). In the `a3` block, a one-bit gate selects whether the signed value is stored at `[this+0x1F0]` and the compact integer at `[this+0x280]`, or whether **both fields are explicitly reset to zero**. The decoder sets `[this+0x2CA]` only when the newly decoded `[this+0x280]` is greater than its previous value. The `a4` branch resolves an optional integer and calls vtable slot `+0x148`; the final 6-bit value is stored at `[this+0x2C8]`. These offsets do not establish gameplay names or prove every counter increase causes a projectile. DSR is not a self-contained firing trigger: graph 596's entry actions, active-state transitions, runtime flags, and callback effects still matter. A manually active Volley state is not by itself proof of a valid shot.

## Counter continuity and Stop payload

This matters for local frame construction. In the current [`soldier.py`](../tools/lobby/soldier.py), `Rifle.command()` queues each accepted tap as `active=True,count_update=True` followed by `active=False,count_update=True`, with the same serial (lines 59–60); reload-complete also uses `count_update=True` (line 50). `volley_payload()` serializes the count gate at line 72 and includes time/count values when true (lines 66–76). The prior false-gate behavior is still relevant to wire semantics: native gate false is not “leave the old count alone”; branch VA `0x7FF6D1F804CD` sets `[this+0x1F0]=0`, then VA `0x7FF6D1F804D8` stores zero in `[this+0x280]`. Gate true decodes time and count; VA `0x7FF6D1F804E1` marks whether count increased.

Inference for the client sender: the current Stop payload's `count_update=True` and same serial preserves the monotonic counter without setting its increase flag. This avoids the native reset caused by the earlier false-gate version. Native riflefire still has not shown a confirmed tracer/recoil in the tap/stop case, so successful hit damage does not prove the client's Volley visual event was emitted.

Timing detail: the decoder stores its second signed value at `+0x1F0`; vtable method `+0x150` (VA `0x7FF6D1F812B0`) tests `+0x1F0` and `+0x280` before serializing them, and uses the signed timing value with the current time base at `+0x48`. This supports treating `+0x1F0` as timing state, but its units/name are not established. Vtable `+0xB8`/OnBegin thunk reaches VA `0x7FF6D1F80880`; vtable `+0xC0`/OnEnd thunk reaches VA `0x7FF6D1D09960`. Neither function body directly writes `+0x1F0` or `+0x280`; OnEnd directly handles lifecycle collection fields at `+0x388` and `+0x3B0`. Thus the explicit DSR gate-zero branch is the proven reset path in this flow.

## Native update consumers and transient Start

Static code search across `.text` found only two accesses to `[this+0x2CA]`: DSR clears it at VA `0x7FF6D1F80416` and writes the counter-increased result at `0x7FF6D1F804E1`. No native code read of that byte was found. `[this+0x280]` is loaded/stored by DSR and compared/serialized by vtable `+0x150` at VA `0x7FF6D1F812B0`; no other access was found. Therefore the native image does not establish that `+0x2CA` is an FX queue flag or that `+0x280` directly triggers a projectile.

After decoding, DSR calls the *parent context* vtable slot `+0x148` at VA `0x7FF6D1F80549`. For the resolved context implementation this dispatch reaches VA `0x7FF6D2D91570`, which handles context flags and queues a type-2 state/context event containing the state index. It does not read Volley offsets `+0x280` or `+0x2CA`. Thus the decoder hands state changes to the context synchronously, but the native evidence still does not connect the count/dirty fields to projectile rendering.

The timing field has a concrete per-update consumer: vtable `+0x70` points to VA `0x7FF6D1D05D90`. It obtains current time, subtracts `[this+0x1F0]` (VA `0x7FF6D1D05E22` and `0x7FF6D1D05E8A`), calls scheduler VA `0x7FF6D1F81070`, then runs update VA `0x7FF6D1F813D0`. Scheduler `+0x70` uses internal cursor/deadline fields (`+0x260`, `+0x268`, `+0x270`, `+0x278`) to process time-relative work; it does not read `+0x280`. A separate vtable method `+0x188` at VA `0x7FF6D1F80140` also computes interpolation from `[this+0x1F0]`.

This makes a one-update hold a reasonable diagnostic for a Start that produces no visible tracer/recoil: the time-relative handler only runs from the state's update path, while Stop can immediately end that state. It is not proved that a held update is required; the code search did not find a reader for `+0x2CA` or a projectile callback explicitly keyed by `+0x280`. The local sender already emits Start and Stop as separate owner frames, waiting for the previous frame's acknowledgment. An additional simulation tick between them remains an experiment, not a native-proven requirement. The parent runtime reports server-authoritative target damage and respawn succeeding while the tap/stop sequence still lacks a confirmed visible tracer; those are separate validation results.

## Local evidence anchors

Session `20261101T062207.207709Z` validates a further counter correction. The serializer at VA `0x7FF6D1F813A8` reads runtime word `+0x44` and emits its low six bits, so the old label “trailer” concealed a state event counter. Diagnostic Start/Stop now carry the same `serial & 63`; after ten native F6 inputs, guarded reads show `+0x44=10`, `+0x2C8=10`, and shot count `+0x280=10`. Earlier constant-zero frames left both event fields zero. Multiple captured frames show a blue effect (`soldier-runtime/volley-counter-tracer.png`), still apparently originating near the feet. This establishes event delivery and repeated visible effects, not aligned native muzzle fire or authentic hitboxes.

For the separate graph596 state21 effect, the final runtime vtable is VA `0x7FF6D384DBA8`, installed late in constructor VA `0x7FF6D1BA4FE0`; the initial `0x7FF6D383BDF0` assignment is an intermediate base. Final `+0x158` points to `0x7FF6D1BA4540`, which consumes two bits and stores the event counter at `+0x44/+0x260`. Its helper classification is true; the diagnostic payload is Begin plus counter2. The one-bit-only probe asserted in `20261101T052528.512133Z`; the corrected frame survives initialization and actual fire inputs in `20261101T054036.140808Z`. Do not infer DSR from an intermediate constructor vtable.

- Retail graph mapping and Volley nodes: [soldier76-weapon-graph.md](soldier76-weapon-graph.md#findings), especially the graph-596 state indices and node references.
- Binary build and image-base context: [soldier-runtime/REPORT.md](soldier-runtime/REPORT.md).
- Runtime analysis addresses: `Overwatch.exe` RVA `0x16303F0`, `0xAE64E0`, `0xAE3280`, `0xD60400` (`sub_7FF6D1F80400`), `0xD60580` (`sub_7FF6D1F80580`), `0x1B736A0`, `0x185BA50`, and `0x78BDB0`. DSR parser branch sites: VA `0x7FF6D1F804AF` (gate), `0x7FF6D1F804CD` (clear fields), `0x7FF6D1F804D8` (store count), and `0x7FF6D1F804E1` (increase flag). OnBegin/OnEnd: VA `0x7FF6D1F80880`, `0x7FF6D1D09960`; count/time serializer: VA `0x7FF6D1F812B0`.
