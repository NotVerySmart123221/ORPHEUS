# Verification & Parity State (`rust`)

Tracks implemented capabilities, Python-vs-Rust parity, verified baselines, and remaining gaps on `rust` targeting **Overwatch 1.74.0.0.104319**. For crate boundaries and contribution rules, see [NATIVE_SERVER_ARCHITECTURE.md](NATIVE_SERVER_ARCHITECTURE.md); for open milestones, see [ROADMAP.md](ROADMAP.md).

*(Historical Python `9f8f3bcb` notes reside on `main`; this document tracks the Rust native server.)*

---

## Public CI & Workspace Coverage (No Game Data Required)

| Lane / Suite | Scope Verified in CI (`.github/workflows/public-checks.yml`) |
| --- | --- |
| **Portable crates (Linux)** | `orpheus-wire`, `orpheus-collision`, and `orpheus-runtime` compile and pass isolated tests without server/ECS deps (`cargo test --locked --all-targets -p orpheus-collision -p orpheus-runtime -p orpheus-wire`). |
| **Root workspace (`default` / `inspector`)** | `rustfmt --check`, zero-warning `clippy`, `532` (`406` public / `126` `#[ignore]`) tests in `default`, and `536` (`410` public / `126` `#[ignore]`) tests in `--features inspector`, plus MSRV `1.85.0` check. |
| **Cross-language contracts (`shared-behavior-contracts`)** | Checks out pinned `main` (`python-reference`) and verifies identical outputs across `jam-framing-v1.json`, `world-queries-v1.json`, and `profile-settings-v1.json`, `12` public Python tests, and `38` exact StateScript VM contracts on `rust`. |
| **C++ gallery preview policy** | `scripts/check_gallery_preview_policy.py` compiles `relay/gallery_preview_policy.h` and checks `18` resource-selection + `6` owned-tree pruning cases against `gallery-preview-policy-v1.json`. |

---

## Core Workflow Parity Matrix (`main` vs. `rust`)

| Workflow | Python (`main`) | Rust (`rust`) | Open Client / Parity Gap |
| --- | --- | --- | --- |
| **Login, Profiles & Lobby** | Battle.net/JAM auth, profiles, settings (`22200`–`22204`), queues, Windows launcher + TLS relay. | Native BNet/JAM lobby, `profile-settings-v1` (CI verified), encrypted UDP seat handoff. | Standalone Rust launcher & public client connection flow open. |
| **Hero Gallery** | Loadouts, skins, equipped voice & emotes. | Loadout equip/persist, live skin body replacement, 23 NPC previews (C++ policy verified in CI). | Client Hero Gallery NPC framing & animation retest pending. |
| **Practice Range & Soldier: 76** | Full body/weapon VM, rifle, Sprint, Helix, Biotic Field, bots, format-5 collision. | Stock Soldier startup tree + rifle/Sprint/Helix/Biotic Field VM regressions; bots/projectiles in Bevy ECS + Lua patrol; all 43 movers match Python trajectories. | Client 60s stock-weapon mount, HUD/ammo sync, upstairs patrol & bot HUD/SFX (#3, #7–#9, #13) pending. |
| **Movement & Collision** | GJK/contact solver, static ray queries, `0688` + 95 map caches. | `orpheus-collision` matches 10,148 Python movement records & 8,256 velocity cases. | Cross-map NavMesh (incl. King's Row) tracked in [#63](https://github.com/leafall903/ORPHEUS/issues/63); player ECS migration in [#65](https://github.com/leafall903/ORPHEUS/issues/65). |
| **Doomfist & Wrecking Ball** | `leafall` merged into `main`: Slam, Rocket Punch dash/slow, Uppercut, Shield, Grapple, Quad Cannons, Piledriver, Minefield. | All 47 fixtures synced in `fixtures/behavior/`; Rust VM consumers tracked in [#61](https://github.com/leafall903/ORPHEUS/issues/61). | Port remaining Doomfist/Wrecking Ball node handlers once #61 completes. |
| **Fault Isolation & Inspector** | Single-process dashboard & replay journals. | Room-owned errors & `catch_unwind` recovery (`room-isolation-v1`), `--features inspector` 3D viewer, two-room worker timing benchmark. | Inspector ray/nav/VM overlays tracked in [#62](https://github.com/leafall903/ORPHEUS/issues/62); multi-player live room retest open. |

---

## Junkenstein's Revenge & Normal Cassidy (`0x3C`) Status

Headless offline regressions against pinned Python `e1128509` and prepared 104319 packs pass across the core encounter loop:

| Component / Mechanics | Rust Headless Coverage (`--ignored` with 104319 Packs) | Remaining Live / Client Gaps |
| --- | --- | --- |
| **All 14 Mode Cards & Gate** | Revenge/Endless/Challenge cards initialize; placed gate runs `0CA7`, health stages (`3` damage poses + destroyed), and team outcome projection (`MECGameMode` fields 7 & 9). | Gate visual state & post-match recap elapsed-time HUD binding remain open. |
| **Cassidy Primary & Fan (`0B8B`)** | Primary (`70`/`140` crit), Fan (`50` non-crit), ammo & passive/damage ultimate charge (`0.5`/`0.75` target-tag reductions): `scene_assets::tests::normal_cassidy_primary_and_fan_damage_all_boss_bodies`, `scene_assets::tests::normal_cassidy_common_npc_damage_charge_and_retirement`. | Live client attack feedback & HUD charge sync await client acceptance. |
| **Combat Roll (`01F0`)** | Collision-aware ~6.15 m Roll displacement across 5 directions, spawn-wall contact, and 5-to-6 ammo refill (`7b13651b` / `74d6ff78`). | Live directional/actor contacts & full hero matrix open. |
| **Flashbang (`224E`)** | Gravity/range flight blast (`800`-frame E-input replay), press-window gating, priority/excluded-tag checks, Reaper Wraith immunity rejection, velocity cap (`224E/31`), captured aim lock (`224E/34`), and targeted defender notices across all 5 Bosses. | Dynamic Boss spawn category bits & native hit-reaction animations await verification. |
| **Deadeye (`06AB` / `0CA5`)** | LOS target rays, lock progression, shot resolution, and natural two-cast 9-Boss Victory replay (`--junkenstein-validation` overrides tested separately). | Full live-client Deadeye lock visuals, SFX & natural Victory acceptance open. |
| **Bosses, NPCs & Score** | All 5 Boss body/controller pairs, common Zomnic/Zombardier/Shock Tire corpses (`8s` timer), Witch healing, Boss speech (`40E05F6B` / `junkenstein-boss-speech-v1`), `game_combat::tests::normal_encounter_first_reaper_finishes_intro_and_pursues_live_defender`, `game_combat::tests::normal_common_enemy_death_updates_authored_team_score`. | All 5 Boss intro cameras, Witch pistol swap, Zombardier combat, Reinhardt narration & Gallery client rendering await live 104319 client pass. |

*(Reference pack digests used for the original `2026-11-12` `13acf15` prepared-runtime baseline — Base `0688` SHA-256: `b11115f3201ebd0765cc8f0cf2ec0e72de6620b9913f8d17962c71649b528d58`; Enriched `0B92` SHA-256: `db7a7bdbddfb8b40e5e4c9a392d498ac36b13aed5144108311a985ea98e6d8f8`.)*

---

## Open Engineering Issues

| Issue | Scope |
| --- | --- |
| [#12](https://github.com/leafall903/ORPHEUS/issues/12) | Guard recycled body IDs against stale reliable combat notices. |
| [#60](https://github.com/leafall903/ORPHEUS/issues/60) | Split remaining large `rust/src/` modules (`script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, `script_player.rs`, `game_service.rs`). |
| [#61](https://github.com/leafall903/ORPHEUS/issues/61) | Add Rust consumers for the 4 remaining synced `main` fixtures and port 19 VM contracts back to `main`. |
| [#62](https://github.com/leafall903/ORPHEUS/issues/62) | Add raycast, contact-normal, nav-path, and live StateScript inspection overlays to `--features inspector`. |
| [#63](https://github.com/leafall903/ORPHEUS/issues/63) | Generalize walkable-surface / NavMesh extraction across maps (including King's Row). |
| [#64](https://github.com/leafall903/ORPHEUS/issues/64) | Extract portable `orpheus-statescript` workspace crate. |
| [#65](https://github.com/leafall903/ORPHEUS/issues/65) | Unify player/gate entities under `bevy_ecs` and expose bounded hot-reloadable AI `think()` scripting. |
