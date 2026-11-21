# Contributing to ORPHEUS (`rust`)

Contributors are welcome across Rust implementation, behavior contracts, collision/navigation research, and 104319 client testing. Check [open issues](https://github.com/leafall903/ORPHEUS/issues) (`#60`–`#65` track current Rust modularization, contracts, inspector, navmesh, and ECS work).

- **`rust`:** Native Rust server and workspace crates (target branch for Rust PRs).
- **`main`:** Python reference server, launchers, and data extractors (target branch for Python/tool PRs).

---

## Development Setup

Install **Git**, **stable Rust** (`>= 1.85.0`) with `rustfmt` and `clippy`. On Windows, install **x64 MSVC Build Tools** and the **Windows SDK** (needed for vendored Lua 5.4):

```powershell
git clone --branch rust https://github.com/leafall903/ORPHEUS.git
cd ORPHEUS
```

For Python work, clone with `git clone --branch main https://github.com/leafall903/ORPHEUS.git`. Set the PR base to `main` for Python and `rust` for Rust, including after the planned default branch change on November 19, 2026 at 12:00 UTC. [Branch guide](docs/BRANCH_TRANSITION.md)

---

## Public Source Checks (CI Gate)

Every PR targeting `rust` must keep formatting, both feature lanes (`default` and `inspector`), MSRV (`1.85.0`), portable Linux tests, and all shared contracts green:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --features default -- -D warnings
cargo test --locked --workspace --all-targets --features default
cargo clippy --locked --workspace --all-targets --features inspector -- -D warnings
cargo test --locked --workspace --all-targets --features inspector
git diff --check
```

Optional local checks matching the other GitHub Actions lanes:

```powershell
# MSRV check (if toolchain 1.85.0 is installed)
cargo +1.85.0 check --locked --workspace --all-targets --features inspector

# Portable crates & gallery preview policy
cargo test --locked --all-targets -p orpheus-collision -p orpheus-golden-trace -p orpheus-runtime -p orpheus-statescript -p orpheus-wire
python3 -B scripts/check_gallery_preview_policy.py
```

Windows default runs the full workspace and verifies that the named shared Rust contracts passed. Pipelines for Windows Inspector and Linux run targeted checks for feature and platform behavior. Update the workflow's expected name when moving a guarded test. The six required jobs run on pull requests to `rust`. Build and workflow input changes also run them on the protected branch to refresh its cache. **Run workflow** repeats these checks on a selected branch. Enable **Run the archived Python server comparison tool tests** to include the legacy comparison tool tests.

To compare local cross-language gameplay evidence, see [the trace format, command and synthetic example](docs/GOLDEN_COMPARISON.md).

---

## Prepared-Runtime Tests (`--ignored`)

Regressions that require a real 104319 runtime pack are marked `#[ignore]`. Pass your local pack path and run specific tests by name:

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\path\to\authorized-base-runtime'
$env:ORPHEUS_BNET_DESCRIPTORS = Join-Path $env:ORPHEUS_RUNTIME_DIR 'bnet/descriptors.pb'
cargo run --locked --bin orpheus_server -- --check-runtime
cargo test --locked -p orpheus --lib prepared_worker_delivers -- --ignored
```

For two-room release timing benchmarks, see `docs/NATIVE_SERVER_ARCHITECTURE.md#two-room-headless-workload`.

### Observe the Python reference room offline

Use Python 3.12 with the dependencies from a clean [reference checkout at 45044d0d](https://github.com/leafall903/ORPHEUS/tree/45044d0d62399d9eb876c08e788aaea1f56cecc9), a prepared Practice Range pack, and a private input schedule. Choose the zero-based index of a hostile bot in the loaded range. The observer follows that spawner when its bot respawns.

```powershell
python -B scripts/inspect_python_room.py --reference C:\work\python-reference --runtime C:\private\pack --schedule C:\private\schedule.jsonl --bot-slot 2 --private-output C:\private\observation.json
```

Each schedule line contains `tick`, `elapsed_us`, `missed` and `inputs`. Start at tick and elapsed time zero, advance by the room's command quantum, and set `missed` to zero. Supply zero or one decoded command per tick using the Soldier comparison profile. The frozen room starts at frame 1000. Record actual offered frame numbers and let the command queue decide which it accepts. [Gameplay trace design](https://github.com/leafall903/ORPHEUS/pull/133)

The tool writes a new diagnostic outside both checkouts and the pack. A successful recording prints `incomplete` and exits with code 2. Check its `missing` entries before interpreting any result. It samples simulation state immediately after each room tick. The Soldier rifle's authored reload flag is initially null, which its boolean condition reads as false. During manual reload it remains true through the magazine refill.

The host uses the pinned fixture's `fixed-counter-v1` entropy source. The diagnostic records its counter before room construction and the separate initial seed passed to the stock player's CPython MT19937 script VM. It records the selected spawner's authored identity, startup roster size, first managed room tick, starting clock and tick quantum. In this Python setup the initial owner and bot creates are queued. They enter the observation when a scheduled send selects them, followed by its acknowledgement.

The production owner send path selects, budgets and submits records to an in-memory sink. The adapter groups only effects from successful sends and acknowledges each send afterwards. It records explicit magazine values, transmitted vital-pool updates, and body creation and retirement. A missing `effects` field means a required observation was unavailable. An empty array means the observed tick carried none of those effects. The diagnostic still requires common runtime-profile binding before use in a cross-language comparison. Keep the schedule and observations private. [Recorder work](https://github.com/leafall903/ORPHEUS/issues/131)

---

## Architecture & Behavior Contract Guidelines

For a focused VM walkthrough, the pinned action regression and synthetic performance harness, see [StateScript development](docs/STATESCRIPT_DEVELOPMENT.md).

- **Respect workspace boundaries:** Keep `orpheus-wire`, `orpheus-collision`, and `orpheus-runtime` free of server/ECS/socket dependencies (`cargo tree --locked -p <crate>`).
- **Shared contracts:** Keep protocol, World-query, profile-settings, and VM semantics aligned with Python via `fixtures/behavior/*.json` ([docs/BEHAVIOR_CONTRACTS.md](docs/BEHAVIOR_CONTRACTS.md)).
- **Original-client observations:** Keep fixture/headless results clearly separate from live 1.74.0.0.104319 client testing ([gameplay report form](https://github.com/leafall903/ORPHEUS/issues/new?template=gameplay-verification.yml)).
- **Pull requests & security:** Open PRs from a topic branch against `rust`, use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), disclose AI assistance (`Assisted-by:`), and never commit game binaries, keys, dumps, or private logs.
