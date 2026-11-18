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

Note: `public-checks.yml` also verifies all **38** `shared_*_contract` tests by `--exact` name. When splitting modules or moving tests, preserve the exact test module path expected by `.github/workflows/public-checks.yml` and [docs/STATE.md](docs/STATE.md).

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

---

## Architecture & Behavior Contract Guidelines

For a focused VM walkthrough, the pinned action regression and synthetic performance harness, see [StateScript development](docs/STATESCRIPT_DEVELOPMENT.md).

- **Respect workspace boundaries:** Keep `orpheus-wire`, `orpheus-collision`, and `orpheus-runtime` free of server/ECS/socket dependencies (`cargo tree --locked -p <crate>`).
- **Shared contracts:** Keep protocol, World-query, profile-settings, and VM semantics aligned with Python via `fixtures/behavior/*.json` ([docs/BEHAVIOR_CONTRACTS.md](docs/BEHAVIOR_CONTRACTS.md)).
- **Original-client observations:** Keep fixture/headless results clearly separate from live 1.74.0.0.104319 client testing ([gameplay report form](https://github.com/leafall903/ORPHEUS/issues/new?template=gameplay-verification.yml)).
- **Pull requests & security:** Open PRs from a topic branch against `rust`, use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), disclose AI assistance (`Assisted-by:`), and never commit game binaries, keys, dumps, or private logs.
