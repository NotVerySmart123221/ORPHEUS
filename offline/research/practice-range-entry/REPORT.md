# Practice Range entry — build 104319

2026-10-31. Unchanged retail executable SHA-256 `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`. Owned loopback TCP 3724 lobby and UDP 3730 game service; no injection, binary patch, firewall or antivirus changes.

## Verified

- Main menu → Play → Practice Range emits CRC `A6E53896`, message 24000, kind 2 / flags 4. Main-menu Training remains disabled in tournament mode.
- Live-client registration records independently confirm the request and 20600 handoff schema (`074DAD18`). Runtime section snapshots and bounded schema parser are disposable analysis under `.scratchpad/`; no process memory was written.
- Handoff 20600 establishes authenticated AES-256-GCM UDP SYN/ACK and sends reliable 20300 for world `0800000000000688`, mode `0230000000000018`, tournament quanta 7000 microseconds.
- Initial test stalled at Entering Game. Replacing empty 20505 availability arrays with Soldier `02E000000000006E`, map header `0790000000000688` and content set `0308000000000001` unblocked loading. Skin availability and decryption-key lists remained empty.
- Client reports 21601 and 21602; original range geometry renders. Hero selection offers Soldier: 76. Selecting him sends game message 1361 and produces a first-person spawn-room view; mouse camera responds.
- Escape → Leave Game → Yes returns to Play after roughly three seconds, with authenticated FIN and match cleanup. A second entry creates a new match and reaches the same selection/first-person state.

Successful run: `research/main-menu/20261031T215035.262288Z/lobby.log`. Failed empty-availability comparison: `research/main-menu/20261031T214603.443495Z/lobby.log`.

Screenshots: `preload-loading.png` (hero selection), `soldier-spawn.png` (first person), `soldier-fire-test.png` (camera changed, rifle absent), `hero-change-check.png` (returned Play menu), `range-second-entry.png` and `second-soldier.png` (repeat).

## Remaining acceptance criteria

Walking and mouse camera control are verified. In a follow-up run (`research/main-menu/20261031T220307.407818Z`), bounded forward pulses visibly moved the viewpoint toward the doorway (`range-walk-forward.png`); input logs independently show forward throttle 127. Space reaches the server as button 0x40, and primary mouse input includes FIRE bit 0x01. Jump flight/landing and collision remain unverified. The first-person view has no rifle or normal HUD; a primary-fire test did not establish a shot or damage. Upstream explicitly documents missing weapon/ability/HUD statescript simulation. Do not call this a completed menu → walk/jump/fire → menu acceptance test.

A bounded HUD experiment activated graph 20E4's sole owner state (the direct result of its unconditional Entry). No visible HUD appeared; the experimental change was reverted. Both nonempty graph sync variables are ENTITY-scoped, so the existing zero instance-presence count is correct. The follow-up session also exited normally with code 0 and both ports released. Earlier successful two-cycle run exited normally with code 0 as well.

The local asset parser independently resolves the range's manifest GUIDs/CKeys, but loose encoding lookup misses them. Actual client map loading proves runtime availability for this map; standalone package/bundle extraction remains unresolved. See `../practice-range-assets.md`.

## Implementation and checks

Existing menu handshake retained. `tools/lobby/practice.py` validates one bounded create-game request, suppresses duplicate matches and serializes the dynamically indexed handoff. Game transport/world/hero selection reuse an audited MIT subset pinned at `0fa61b950dfc9c915cb2353d2873d6d4689c2a5e`; origin, license and local modifications are under `tools/lobby/vendor/`.

Eleven lobby/launcher tests pass, including exact handoff bytes, malformed requests, duplicate suppression, input deduplication/diagnostics, tampered UDP rejection, authenticated map-load exchange and launcher failure detection. Three independent CMF parser tests pass. No bot, damage, progression, cosmetic service or general graph interpreter was added.

Final launcher timeout smoke test (`research/main-menu/20261031T221019.825787Z`) reached its ten-second limit, closed both owned processes and released TCP 3724 / UDP 3730. Final game executable hash still matches the original fingerprint.

## Sources

- [Pinned game server](https://github.com/Boi-027/overwatch-1.74-lobby-server/tree/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/game).
- [Handoff builder](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/lobby/matchmaker.py).
- [Availability semantics](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/content/retail.py) and [recorded 20505 data](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/data/retail_templates.json).
- [Unfinished gameplay support](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/WIP.md); Soldier graph research in `../soldier76-weapon-graph.md`.
