# Restoration Roadmap (`rust`)

Target build: **Overwatch 1 retail 1.74.0.0.104319**.
- For Rust vs. Python implementation parity, see [RUST_PARITY.md](RUST_PARITY.md).
- For architectural and crate refactoring tracks (`#60`–`#65`), see [RUST_FOUNDATION_REVIEW.md](RUST_FOUNDATION_REVIEW.md).
- For active tasks, see [GitHub Issues](https://github.com/leafall903/ORPHEUS/issues).

---

## Project Milestones

| Milestone | Exit Criteria | Current `rust` Status |
| --- | --- | --- |
| **M0 · Research & Replay** | Pinned build provenance, extractors, deterministic encrypted protocol replay | Pinned to 104319; synthetic fixture CI & runtime pack validation live. |
| **M1 · Body & Weapon Init** | Soldier: 76 spawns cleanly, keeps model/weapon/ammo HUD for 60s across 10x switch & rejoin | Authored Soldier startup & encrypted loopback pass; live client 60s retest open. |
| **M2 · Rifle & Prediction** | Sustained firing, reload, and Sprint agree with client under loss | Authored rifle, reload & Sprint VM timelines pass offline; live client prediction open. |
| **M3 · Single-Player Combat** | Training bots take damage, die, respawn cleanly with visible health/kill-feed/SFX | Bot combat, respawn & nameplate wire delivery pass offline; client UI/audio open. |
| **M4 · Two-Client Combat** | Two clients agree on movement, LOS, damage & respawn under loss | Multi-room fault isolation & two-room worker timing benchmark live; two-client live match open. |
| **M5 · Modes & Heroes** | Full match rules & declared abilities across supported heroes | Practice Range & Normal Junkenstein offline replays pass; Doomfist/WB/other heroes open. |
| **M6 · Reliability & Cutover** | Measured p95/p99 tick latency, clean deployment, Rust matches/exceeds Python | `orpheus-wire`/`collision`/`runtime` extracted; MSRV `1.85` & Linux CI green. |

---

## Original-Client Acceptance Gates

### 1. Practice Range & Stock Soldier: 76
- [ ] Verify stock weapon attachment, ammo HUD, Sprint, Helix Rockets, and Biotic Field in live client `104319`.
- [ ] Verify upstairs patrol route geometry matches contact solver bounds and hitscan wall queries (use `--features inspector` to diagnose).
- [ ] Resolve bot overhead health bars (#7), kill feed localized names (#8), headshot SFX (#9), and Practice Range boundary death (#13).

### 2. Junkenstein's Revenge (Normal Cassidy, Card `0x3C`)

*Headless offline replays already pass Natural Deadeye Victory, Early/Late Defeat, Team Score, Boss Speech, Combat Roll, Flashbang E-input, and all 15 Boss damage/retirement cases. All items below require live 104319 client acceptance before checking off:*

- [ ] **Castle gate & recap HUD:** Gate mesh states (intact, 3 damaged poses, destroyed) match health visually; post-match recap displays non-zero elapsed seconds alongside Victory/Defeat heading.
- [ ] **Cassidy abilities & score:** Live primary/Fan damage, `0B8B` ultimate charge HUD, Combat Roll camera/refill, Flashbang throw & stun reaction animation, Deadeye locks & fire, and HUD team score.
- [ ] **Enemy AI & intros:** Zomnic walking speed/door attacks, Zombardier target acquisition & projectile visuals, Witch heal-vs-pistol target switching, and once-only Boss intro cameras/pursuit/voicelines across all 5 Bosses.
- [ ] **NPC Hero Gallery:** Verify visual selection, model framing, and idle animation on client for the 23 NPC Hero Gallery entries.

---

## System Checklist

- [ ] **Research & Replay:** Generate deterministic replays covering random seeds, loss schedules, and multi-client sessions (`#61`).
- [ ] **Navigation & Collision ([#63](https://github.com/leafall903/ORPHEUS/issues/63)):** Build map-agnostic walkable-surface / NavMesh extraction so PvP maps without cached nav data (including King's Row) support general AI movement, alongside shared player-vs-player and player-vs-bot character collision blocking.
- [ ] **ECS & Script Boundary ([#65](https://github.com/leafall903/ORPHEUS/issues/65)):** Unify player bodies and encounter gates under `bevy_ecs` alongside bots/projectiles, and expose a bounded script ABI for hot-reloadable AI `think()` logic.
- [ ] **Workspace Modularity ([#60](https://github.com/leafall903/ORPHEUS/issues/60), [#64](https://github.com/leafall903/ORPHEUS/issues/64)):** Finish splitting `script_enemy.rs`, `script_expr.rs`, `game_replication.rs`, `script_snapshot_wire.rs`, and extract `orpheus-statescript`.
- [ ] **3D Inspector & Reliability ([#62](https://github.com/leafall903/ORPHEUS/issues/62), [#12](https://github.com/leafall903/ORPHEUS/issues/12)):** Add raycast/nav/VM state visualization to the web inspector and guard recycled body IDs against stale combat notices.
