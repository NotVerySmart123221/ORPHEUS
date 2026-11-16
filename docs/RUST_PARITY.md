# Python & Rust Parity (`rust`)

Python (`main`) is the behavioral reference; `rust` is the native dedicated server. Track high-level acceptance in [ROADMAP.md](ROADMAP.md), architecture boundaries in [NATIVE_SERVER_ARCHITECTURE.md](NATIVE_SERVER_ARCHITECTURE.md), and cross-language CI fixtures in [BEHAVIOR_CONTRACTS.md](BEHAVIOR_CONTRACTS.md).

---

## Core Workflow Parity Matrix

| Workflow | Python (`main`) | Rust (`rust`) | Original-Client Gap |
| --- | --- | --- | --- |
| **Login, Profiles & Lobby** | Battle.net/JAM auth, profiles, settings (`22200`–`22204`), queues, Windows launcher + TLS relay. | Native BNet/JAM lobby, `profile-settings-v1` (CI verified), encrypted UDP seat handoff. | Standalone Rust launcher & public client connection flow open. |
| **Hero Gallery** | Loadouts, skins, equipped voice & emotes. | Loadout equip/persist, live skin body replacement, 23 NPC previews (C++ policy verified in CI). | Client Hero Gallery NPC framing & animation retest pending. |
| **Practice Range & Soldier: 76** | Full body/weapon VM, rifle, Sprint, Helix, Biotic Field, bots, format-5 collision. | Stock Soldier startup tree + rifle/Sprint/Helix/Biotic Field VM regressions; bots/projectiles in Bevy ECS + Lua patrol; all 43 movers match Python trajectories. | Client stock-weapon mount, HUD/ammo sync & upstairs patrol acceptance pending. |
| **Movement & Collision** | GJK/contact solver, static ray queries, `0688` + 95 map caches. | `orpheus-collision` crate matches 10,148 Python movement records & 8,256 velocity cases. | General cross-map NavMesh (incl. King's Row) tracked in [#63](https://github.com/leafall903/ORPHEUS/issues/63); player ECS migration in [#65](https://github.com/leafall903/ORPHEUS/issues/65). |
| **Doomfist & Wrecking Ball** | `leafall` merged into `main`: Slam, Rocket Punch dash/slow, Uppercut, Shield, Grapple, Quad Cannons, Piledriver, Minefield. | Behavior fixtures mirrored on `rust`; Rust VM consumers tracked in [#61](https://github.com/leafall903/ORPHEUS/issues/61). | Port remaining Doomfist/Wrecking Ball node handlers once #61 completes. |
| **Room Isolation & Inspector** | Single-process dashboard & replay journals. | Room-owned errors & `catch_unwind` recovery (`room-isolation-v1`), `--features inspector` 3D collision/timing viewer. | Inspector ray/nav/VM overlays tracked in [#62](https://github.com/leafall903/ORPHEUS/issues/62); multi-player live room retest open. |

---

## Junkenstein's Revenge & Normal Cassidy Parity

Substantial offline parity against pinned Python `e1128509` and prepared 104319 runtime packs has been implemented on `rust`:

| Component / Mechanics | Rust Implementation & Headless Regressions | Remaining Live / Client Gaps |
| --- | --- | --- |
| **All 14 Mode Cards & Gate** | Revenge/Endless/Challenge cards initialize; placed gate runs `0CA7`, health stages (`3` damage poses + destroyed), and team outcome projection (`MECGameMode` fields 7 & 9). | Gate world-transform visual acceptance & recap elapsed-time HUD binding remain open. |
| **Cassidy Primary & Fan (`0B8B`)** | Primary (70/140 crit), Fan (50 non-crit), ammo & passive/damage ultimate charge (`0B8B` + `0.5`/`0.75` target-tag reductions), verified by `scene_assets::tests::normal_cassidy_primary_and_fan_damage_all_boss_bodies` and `scene_assets::tests::normal_cassidy_common_npc_damage_charge_and_retirement`. | Live client attack feedback & HUD charge sync await client acceptance. |
| **Combat Roll (`01F0`)** | Collision-aware ~6.15 m Roll displacement across 5 directions, spawn-wall contact, and 5-to-6 ammo refill (`7b13651b` / `74d6ff78`). | Live directional/actor contacts & full hero matrix open. |
| **Flashbang (`224E`)** | Gravity/range flight blast (`800`-frame E-input replay), press-window gating, priority/excluded-tag checks, Reaper Wraith immunity rejection, velocity cap (`224E/31`), captured aim lock (`224E/34`), and targeted defender message delivery across all 5 Bosses. | Dynamic Boss spawn category bits & native hit-reaction animations await verification. |
| **Deadeye (`06AB` / `0CA5`)** | Line-of-sight target rays, lock progression, shot resolution, and natural two-cast 9-Boss Victory outcome replay (`--junkenstein-validation` overrides tested separately). | Full live-client Deadeye lock visuals, sound & natural Victory acceptance open. |
| **Bosses, NPCs & Score** | All 5 Boss body/controller pairs, common Zomnic/Zombardier/Shock Tire corpses (`8s` timer), Witch healing, Boss speech (`40E05F6B` / `junkenstein-boss-speech-v1`), `game_combat::tests::normal_encounter_first_reaper_finishes_intro_and_pursues_live_defender`, and `game_combat::tests::normal_common_enemy_death_updates_authored_team_score` pass offline. | All 5 Boss intro cameras, Witch pistol swap, Zombardier combat, Reinhardt narration & Gallery client rendering await live 104319 client pass. |

---

## Reference & Cutover Rules

1. **Same inputs & clock:** Compare Python and Rust using identical prepared `104319` graph bundles, collision caches, commands, and packet-loss schedules.
2. **Evidence hygiene:** Distinguish synthetic fixture agreement, `--ignored` local-pack replays, and live 1.74.0.0.104319 client acceptance; never check prepared runtime packs or captures into Git.
3. **Cutover criteria:** `rust` replaces Python as default only after exceeding Python's verified hero/mode coverage and passing original-client validation on the audited public checkout.
