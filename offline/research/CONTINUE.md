> **Historical (2026-11-01).** Superseded by [../STATUS.md](../STATUS.md), the current status and open list. Kept for the bot-firing and rifle notes it records; its paths (`<old-workbench>`) and test counts are out of date.

# Resume checkpoint — 2026-11-01 22:32 UTC

User requested a temporary interruption. Resume implementing the complete offline Practice Range slice: HUD, rifle firing/reload/damage, bots, player death/respawn, movement/collision agreement and menu return. Work remains incomplete; do not mistake protocol acceptance or unit checks for native gameplay verification.

## Clean stopping state

- Last diagnostic run: `research/main-menu/20261101T221710.929566Z`, phase `rifle-bot-remote-init`, game PID23472. Exited through the game menu normally; launcher reports `Game exit: 0`. TCP3724 and UDP3730 have no listener after cleanup. No temporary F6 binding was added in this run.
- Closed game log contains the known startup `/../.build.info` path-normalization warning; no assertion/exception was found. No game-install binaries were patched or DLLs injected.
- Focused checks: **58 lobby +40 research +10 asset =108 passing**. Commands from workspace root: `python -B -m unittest discover -s tools/lobby -p "test_*.py"`, and equivalent `tools/research`, `tools/assets`.
- `.scratchpad/body-phase.txt` still selects `rifle-bot-remote-init`. Scratch files are disposable; retain reusable tooling in `tools/`. All captured native pointers/PIDs below are stale after exit.

## Ordinary launcher now works

From `<old-workbench>`:

```powershell
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi
```

(The plugin skips Pod/name; without it: Pod1 → Start → local name → Enter Game.) Play → Practice Range → Select Soldier:76. Normal primary fire and R reload. Escape → Leave Game → Yes returns to Play. With the plugin Alt+Tab and the Windows key work (research/tournament-input-lock.md); without it tournament mode blocks them, so exit using the game menu.

Normal session `20261101T210119.507195Z` verifies the complete initial HUD/rifle setup without a research probe: ammo30→29, target200→180, ten hits remove the original Training Bot, respawn after3 seconds, R restores30. Leave/re-enter creates a fresh HUD,30 rounds and target. Permanent captures are `soldier-runtime/normal-launcher-{hud,shot29,target-removed,target-respawned,reload30}.png`.

Production integration: `tools/lobby/soldier.py:initialize_practice_soldier`, owned `practice.GameServer` sets `gameplay_enabled=True`/`body_setup`; vendor server/match pass optional body setup and apply it after the existing body delay. Research server explicitly disables this initializer so probes remain isolated. Login preloads Soldier and bot hero content. Regression compares normal frame bytes to the native-verified shared-HUD probe.

## Remaining functionality and limits

1. Numeric health reads correctly since 2026-11-04 (graph463 vars 1732/1737). Portrait, white/grey health segments, native damage indicators and low-health vignette work. Actual native component51 pools match replicated values (including40/200). Full ability/ultimate HUD remains unfinished.
2. Bot is damageable and respawns in normal mode; normal attacks/patrol remain disabled. Diagnostic `rifle-attack` patrols, applies timed cone damage and replaces dead player bodies after3 seconds. Visible bot shots, authentic projectile initialization, native death/corpse feedback and ordinary-launcher integration of those features remain unfinished.
3. Rifle muzzle feedback and original manual/empty-magazine reload animations have native evidence. Recoil/parallax and authentic hitboxes remain unverified. Target damage uses a sphere without wall occlusion.
4. Native client walk/jump/landing and one spawn-wall collision were observed. Server movement is flat without map collision/gravity and can diverge. No map-wide collision or line-of-sight restoration is claimed.

## Next: bot firing

Read `remote-bot-statescript.md`, `weapon-volley-dsr.md`, `training-target.md` first. Newly derived H0 serializer is in vendor `ow174/game/statescript.py:remote_full_frame`; normal gameplay does not use it yet. BA3 has32 owner states,21 remote states,13 instance-presence entries. Volley owner13 maps to remote7; data-flow owner7 maps to remote3. Live metadata independently confirmed both Volley slots point to the same schema object. H0 omits owner timing/count fields and per-instance event streams; present custom states add control-present plus desired-active before DSR.

`rifle-bot-remote` accepted in session215255; `rifle-bot-remote-init` accepted in221710, adding authored Entry constants and remote3 mapping. Neither capture confirms a visible shot. Latest bounded read confirms entityB0000001, Volley flags0x40E1, local event+0x44=1, received event+0x2C8=1, owner count+0x280=0, hardpoint6973 kind4/value0x0DC0000000000005. Instance233=True,229=Int0,230=Float2.75,53/198=Int20,6884=Float.489 all arrived. Dynamic231 remains kind255/unset. **Do not infer that231 must be set: its semantics are unresolved.**

Additional observed fields: transient config pointer+0x1E0=null outside callbacks, nesting depth+0x1E8=0, byte+0x1EC=1, cursor+0x278=0, shots+0x270=0. These are observations, not proof the state never updates: +0x1E8 is incremented/decremented within callbacks. Native update `0x7FF6D1D05D90` gates scheduler/update work on byte+0x1EC and config fields. Trace its initialization and remote event path next; avoid repeating accepted packets without a new discriminating hypothesis.

Relevant native functions (full VAs, base0x7FF6D1220000): Volley DSR1F80400; OnBegin1F80880 via1D05700; update1D05D90; shared update1F813D0; config resolve1F7E030; event/projectile consumer1D604B0; virtual+0x1B8→1D04260, +0x1C0→1D07C40, +0x1D0→1D06DF0. Prefix abbreviated addresses with `0x7FF6D` (e.g.1D05D90→0x7FF6D1D05D90), not image-base addition. Their opaque branches need anchored disassembly; decompiler output is often partial.

Native projectile1476 transform-only creation survives but ordinary movement causes Missing movement component. Components50/97 expose no ordinary replicated MirrorData visitor;97 also exists on Soldier, so do not assume it initializes projectiles. Shot builder1D043F0 calls creation1D41790; the latter is not yet proven a network reader. Existing authored projectile/effect assets are present; missing content is not established.

Useful scratch diagnostics: `read_bot_volley.py`, `read_remote_table.py`, `find_hud_state.py`, `disasm_range.py`, `inspect_ba3_graph.py`, `asset_fields.py`. Fresh scan must rediscover native objects each process; old PID23472 state1994936361104 is stale. Bot burst evidence `.scratchpad/bot-remote-init-burst/` and contact sheet are disposable, not permanent tools.

## Numeric health: resolved (2026-11-04)

The card number is graph 463's expression over its synced vars 1732 (current health) and 1737 (max), not a health-presenter var; `soldier.health_variables` sends them in the body frame. See `numeric-health.md` round 7.

## Tools, evidence and safety

- Executable SHA256 `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`; only build104319 supported. Hash-guard bounded read-only metadata; no process writes, debugger injection or native function invocation.
- IDAlib session `7fe39bd8`, analysis DB `ida/Overwatch-state-analysis.dump.i64` (may expire). No full autoanalysis. Static snapshot helper uses pinned `.scratchpad/runtime-weapon/` text/rdata.
- Correct upstream address translation: old base0x7FF788F10000 → current0x7FF6D1220000. Old bit reader7FF78969BDB0 maps current7FF6D19ABDB0; old channel4 reader7FF78969BBC0 maps19ABBC0; body creator7FF78969DAB0 maps19ADAB0. Old world reader7FF7896DE110 maps19EE110. This calibration was independently checked.
- Graphify CLI stalled earlier; existing beta `<beta-project>/graphify-out/graph.json` was queried through skill-approved NetworkX fallback. Beta addresses are not retail evidence. User requires Graphify/idalib where supported.
- At most ONE delegated agent, existing `/root/prometheus_reuse`, now idle; no nested delegation. Respect Chisle/Ponytail and applicable skills. Never access TCRF or its subdomains/proxies.
- Native Computer Use uses `@oai/sky` through node_repl. Read its skill/guidance first. Screenshot capture historically times out; established read-only fallback is `<beta-project>/tools/screengrab.py`. Refresh observed state after every input; no blind coordinate chains. No live game remains.
- `apply_patch` occasionally partially applied/duplicated insertion hunks. Inspect before retry. Tests have unique method names after duplicate cleanup. No git repository; do not initialize one.
