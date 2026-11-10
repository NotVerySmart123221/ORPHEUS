# Training Bot remote statescript frame

Build104319, image base `0x7FF6D1220000`; executable SHA-256 `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`.

Prior bot probes used owner H1 frames. Native reader `0x7FF6D1C2BE30` selects graph table+0x60/+0x68 for H1 and+0xF0/+0xF8 for H0. A bounded read-only capture in session `20261101T212245.858855Z` verifies BA3's owner table has32 entries and remote table21. Owner entry13 and remote entry7 point to the identical Volley schema object (`0x23E7AE259D8`, process-specific); schema vtable `0x7FF6D3980188`. This independently confirms the remote-array mapping from the CKey-verified BA3 asset.

H0 full-frame layout from `0x7FF6D1C2ECA0` and related readers:

- Lead0,H0; no owner C/CmFD/LBSS fields.
- Existing descriptor list and sentinel; entity-variable gate/map.
- Per instance, the same filtered sync-variable presence list (BA3:13 bits), followed by optional extra-variable map (`0x7FF6D1C2C270`).
- Remote-state vector (BA3:21 slots), with one presence bit per remote entry. A present custom-state update adds a control-present bit and desired-active bit (`0x7FF6D1C2BF33` through `0x7FF6D1C2C08B`).
- H0 has no per-instance event list. Two empty frame-level event streams terminate with00 (`0x7FF6D1C2CCA0`).

For remote Volley, runtime DSR `0x7FF6D1F80400` receives r8=0,r9=1 via wrapper `0x7FF6D2D92060`. It skips the owner time/count block, reads an optional integer (default bit0 yields1), then a six-bit event counter. Thus the isolated activation is remote slot7 present1, control-present1, desired-active1, optional integer0, counter1. It supplies the previously verified hardpoint5 through instance presence12. This is a traced diagnostic packet, not a verified visible shot.

Session `20261101T215255.813983Z` accepts `rifle-bot-remote` without an assertion/disconnect. A guarded read identifies the bot by entityB0000001 and confirms runtime flags0x40E1, event counter1, owner-count field0 (omitted by H0), and hardpoint6973 as kind4/value0x0DC0000000000005. Thus the remote grammar/event delivery is native-verified. Recorded burst `.scratchpad/bot-remote-first-burst/` still shows no confirmed muzzle flash/projectile. Event delivery does not establish visible firing.

`remote_full_frame` has an independent bit-reader regression check for header, controls and event-list alignment. A second phase, `rifle-bot-remote-init`, adds the authored Entry constants, including229=Int0 and230=Float2.75 used by the launch config, ammo-like198/53=Int20, and remote data-flow slot3 (owner state7) with233=True. Session `20261101T221710.929566Z` accepts this frame and exits normally0. A guarded native read confirms all those values plus6884=Float.489, local/received event1, flags0x40E1 and hardpoint5; Dynamic231 remains kind255/unset. No visible shot appears in the sampled burst. Outside callbacks, +0x1E0 is null, +0x1E8 nesting depth0, byte+0x1EC=1, cursor+0x278=0 and shots+0x270=0. Native update `0x7FF6D1D05D90` gates work on +0x1EC and resolved config fields; these observations do not prove the state never updates. No semantic label is assigned to the unknown launch fields. Volley F2AE0897 is independently confirmed as projectile count1; Dynamic231 is a separate unnamed config/output at3FFC9EB9, not that count. Follow-up pointers and remaining work are in `CONTINUE.md`.
