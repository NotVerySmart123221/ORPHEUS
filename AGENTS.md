# ORPHEUS Rust Development Guidelines

- **Target build:** Overwatch 1 retail `1.74.0.0.104319`. Complete Soldier: 76 and Practice Range bot acceptance before starting Doomfist, and prefer evidence pinned to `104319`.
- **Branches:** Target the default branch, `rust`, for the native server and Rust workspace. The unmaintained Python implementation and its extraction tooling remain on `main` as references. Select the base explicitly when opening a PR.
- **Architecture & migration:** Follow [NATIVE_SERVER_ARCHITECTURE.md](docs/NATIVE_SERVER_ARCHITECTURE.md) (keep Rust authoritative for server/room ECS, preserve crate boundaries, keep MSRV `1.85.0` green, and run locked Cargo checks) and update [STATE.md](docs/STATE.md) / [ROADMAP.md](docs/ROADMAP.md) when parity or acceptance changes.
- **Automated work vs. manual client runs:** Code, tools, tests, and docs run headlessly. Leave game client startup and live UI/gameplay acceptance to manual sessions unless explicitly requested.
- **Behavior contracts:** Supply or update synthetic JSON fixtures under `fixtures/behavior/` for behavior changes ([docs/BEHAVIOR_CONTRACTS.md](docs/BEHAVIOR_CONTRACTS.md)) and separate headless evidence from original-client acceptance.
- **Commits & hygiene:** Keep commits focused, use Conventional Commits (`fix: ...`, adding `Assisted-by:` trailers when AI-assisted), update `CHANGELOG.md` for user-visible fixes, keep upstream license notices in [LICENSE](LICENSE) and credits in [CREDITS.md](CREDITS.md), and never commit game binaries, asset dumps, keys, credentials, profiles, or raw captures.
- **Protected branches:** Open PRs from topic branches against `rust` and wait for required CI checks.
- **PR scope:** Use one topic branch and PR per user-visible milestone. Add related fixes and their tests as focused commits to the open topic. Split work when a change can be reviewed and accepted independently.
