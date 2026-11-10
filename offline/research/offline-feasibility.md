# Overwatch 1.74 offline feasibility

**Subsequent result:** the retail main menu was reached on 2026-10-31 using the local server and TournamentMode. See [runtime report](<old-workbench>/research/main-menu/REPORT.md) and [launcher instructions](<old-workbench>/tools/lobby/README.md). The investigation below records the earlier static feasibility assessment; bot gameplay remains unproven.

Investigation: 2026-10-31. Target: `<game-root>`. Comparison: local Prometheus work under `<beta-project>`. No game executable was run or patched. Findings distinguish direct checks, historical beta test records, external claims, and engineering judgment.

## Assessment

An offline launcher is a plausible research project. Offline **bot gameplay on this retail build is not established**. The strongest near-term target is a local main menu, followed by one map, one controllable hero, and one damageable bot. A launcher alone cannot supply the missing game/session logic.

Prometheus offers useful implementation and test material, but its injected Core is built for beta `0.8.0.0.24919`. Do not load that DLL into retail or bypass its version check and assume compatibility. Some asset tooling already works against retail; native hooks and recovered engine layouts require a new port.

## Direct retail checks

- `.build.info:2` identifies `1.74.0.0.104319`. The matching build configuration independently says `prometheus-1_74_0_0-104319`, build number `104319`, product `Prometheus`, uid `pro`. Here, `Prometheus` is Blizzard's product name in the configuration; it does not mean the restoration project is installed.
- Executable: `_retail_\Overwatch.exe`, 52,045,144 bytes, AMD64. SHA-256: `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`. A byte-identical analysis copy and IDA database were saved under [ida](<old-workbench>/ida).
- PE inspection found `.text` entropy rounded to `8.00` bits/byte and only `USER32.dll!MessageBoxW` in the normal import table. This strongly suggests protected code/imports. It is not evidence that game code or other API use is absent.
- IDAlib opened the copied binary and surveyed its segments, entry points and strings. Readable strings include `LobbySystem` at `0x142615330`, `LobbySystem_C::LobbyConnectHelloErrorHandler` at `0x1426153e0`, and `ClientLobbyMessageSystem_C::__LobbyServerConnectionUp` at `0x1426160d0`. These confirm relevant lobby metadata exists. They do not establish working handlers or server capability. Automatic analysis was not requested; the raw static database is a starting point, not a decrypted engine analysis.
- No `*Server*.exe` was found beneath the install. That only rules out that filename pattern; it cannot establish whether server logic remains inside the client or archives.
- Local CASC storage contains approximately 27.8 GB across 66 files in `data/casc/data`, plus 502 files in `indices`. Size and presence do not establish a complete install.

### Verified reuse: existing CASC reader

The existing [casc.py](<beta-project>/tools/casc.py:19) was imported with an in-memory installation-path override. Neither it nor [owpaths.py](<beta-project>/tools/owpaths.py:5) was edited. The probe read retail local indices and decoded the encoding table and root manifest:

- 461,560 local index entries; 461,081 decoded encoding entries.
- Encoding table: 29,495,548 decoded bytes; MD5 matched configured content key `fcdff96196225fc1da536513bd467c76`.
- Root manifest: 20,119 decoded bytes; MD5 matched configured content key `0bcfc3ba3d42c373cd9ffb890d638beb`.

Both assertions passed. This verifies archive access and these two metadata objects, **not every map/hero asset or every encrypted chunk**. The disposable reproducer is `.scratchpad/probe.py`; preserve the facts here because scratch files may be deleted.

## What transfers from the beta work

Detailed source review: [prometheus-reuse.md](<old-workbench>/research/prometheus-reuse.md). Its `repo/`, `tools/`, and historical `research/` citations refer to `<beta-project>`.

1. **Local practice flow.** [PracticeMode.cpp](<beta-project>/repo/Prometheus-Core/Fixes/PracticeMode.cpp:90) starts a local map, requests bots and selects difficulty. Its create-game hook schedules local work instead of relying on a lobby server. This is an implementation pattern to port.
2. **Bot and combat replacements.** `BotBrain`, `AIRuntime`, `Bots`, `ServerNodes`, health/damage handling and player lifecycle work provide a reference for restoring behavior missing from the client. Their engine integration, resource IDs and offsets remain beta-specific.
3. **Repeatable acceptance scenarios.** The [saved beta baseline](<beta-project>/research/baseline-2026-10-30/REPORT.md:40) records six bots, player and enemy damage, injected player death followed by respawn, hero/map changes and clean exit. These are historical beta results, not tests repeated here. Later work still identifies incomplete ability and AI behavior.
4. **Inspection tools.** CASC metadata reading is now directly verified against retail. Screenshot/command, STU, navigation and IDA helpers can guide future work after checking paths and formats.

The [launcher](<beta-project>/repo/Prometheus-Launcher/main.cpp:157) accepts a specific beta MD5; [MainLoop.cpp](<beta-project>/repo/Prometheus-Core/Fixes/MainLoop.cpp:10) uses a fixed beta RVA. [Jam.h](<beta-project>/repo/Prometheus-Core/Fixes/Jam.h:7) documents creating messages locally and invoking client handlers. That design may avoid implementing an entire gameplay network service, but equivalent retail handlers and safe game-thread integration have not been identified.

### Graphify evidence

Used the existing `<beta-project>\graphify-out\graph.json`: 5,362 nodes and 10,718 edges. Vocabulary expansion selected `bot brain practice server jam launcher damage main loop`. The CLI query returned no output during the bounded attempt and was interrupted. The documented NetworkX fallback traversed the existing graph as an undirected BFS to depth 2 from `PracticeMode.cpp`, `BotBrain.cpp`, and `ServerNodes.cpp`, reaching 646 nodes.

Relevant graph nodes included `StartMatch()` (`repo/Prometheus-Core/Fixes/PracticeMode.cpp`, `L90`), `BotBrain local movement aiming and fire` (`repo/CHANGES.md`, `L126`), and `Practice vs AI local match flow` (`repo/CHANGES.md`, `L431`). Inspected source afterward. Structural graph edges establish code relationships, not runtime success. No graph rebuild or visualization was needed.

## Public research for this exact build

[Overwatch-1-v1.74-Lobby-Research](https://github.com/Boi-027/Overwatch-1-v1.74-Lobby-Research) targets build 104319. Its stated scope is menu/loot-box access rather than gameplay. It provides lobby cipher/handshake code and research notes, not a demonstrated bot server. Its README and older state document contain conclusions superseded by the follow-up below; do not treat the old dispatcher interpretations as settled.

The linked [channel setup and login notes](https://rentry.co/3bswrgmw#overwatch-174-lobby-protocol-channel-setup-login-and-loot-boxes) report a working menu sequence: use a type-13 channel in the handshake trailer, acknowledge the client's protocol announcement, then supply login/player/content records. They report a direct tournament route without a Battle.net tier, correct several older function labels, and say protocol indices must come from the announcement. This is a concrete protocol lead, **not reproduced on this installation**. Menu and loot-box access do not establish map loading or bots.

The notes therefore strengthen feasibility of an initial local lobby. Their claims should be tested against the installed binary before relying on any address or protocol interpretation. No external server/injection code was saved locally or executed in this investigation.

## Smallest useful development sequence

1. **Local menu, no gameplay patching.** Implement or adapt a loopback-only lobby using the corrected channel/login sequence. Check greeting/proofs, protocol announcement, connected state and visible menu. Add bounded frame parsing and cipher/serialization checks before accepting client traffic. Keep logs in this workspace. Success requires an observed menu, not an `ENTERING GAME` label or a forced state value.
2. **One local map and hero.** Identify retail world/resource and spawn paths using asset metadata and a runtime-decrypted view. Establish a safe game-thread call path. Check control, camera, collision, unload and return to menu. Do not transplant beta RVAs or run the beta DLL as a probe.
3. **One bot with measurable combat.** Port only needed lifecycle, controller and damage behavior. Verify movement, enemy health loss, player health loss, death and respawn. Determine which server-only handlers need replacement; current evidence does not answer that for retail.
4. **Package the launcher after the vertical slice works.** Start/stop the local service, validate exact executable identity, select supported map/settings and clean up only owned processes. Expand abilities and match objectives afterward.

If retail lacks callable local simulation paths, a gameplay server or larger client restoration will be necessary. That is the major unresolved cost. There is currently no evidence supporting an estimate for a complete retail bot mode.

## Limits

Verified: build identity, original/copy executable hashes, PE metadata, IDAlib static triage, graph traversal, beta source paths, and two retail CASC metadata content hashes. Reviewed: saved beta results and public protocol research. Not checked: launching retail, reproducing the lobby sequence, runtime decryption, local game hosting, bot execution, full asset completeness or a new build. Original game and beta project files were left unchanged.
