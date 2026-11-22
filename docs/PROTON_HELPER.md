# Linux Proton relay helper (`orpheus_proton`)

`orpheus_proton` is a manual Linux CLI helper that resolves the active Proton runtime, `WINEPREFIX`, and Wine sync flags (`WINEFSYNC`, `WINEESYNC`, `WINE_NTSYNC`, `PROTON_ENABLE_NTSYNC`, `WINEFUTEX2`) from a running game process and invokes a small x64 Windows helper (`owwfd_inject.exe`) inside that Wine session.

This is a single-shot developer CLI, not a background daemon or one-click launcher. Keep compiled `.exe`/`.dll` binaries, game installations, runtime packs, and local logs out of Git.

## Prerequisites

| Requirement | Version / Notes | Check command |
| --- | --- | --- |
| Rust toolchain | `>= 1.85.0` | `rustc --version` |
| MinGW-w64 C++ (Linux) | x86_64 target for `owwfd_inject.exe` | `x86_64-w64-mingw32-g++ --version` |
| Steam + Proton | Verified on Proton 10.0, 11.0, Experimental, Hotfix | Steam compatibility settings |
| Retail client & relay DLL | Build `1.74.0.0.104319` + local x64 `owwfd_relay.dll` | Local runtime directory |

## Build

### 1. Build the Linux host binary (`orpheus_proton`)

Verified on Linux:

```sh
cargo build --locked --bin orpheus_proton
```

### 2. Build the x64 Windows/Wine helper (`owwfd_inject.exe`)

- **Linux (MinGW-w64 — verified):**

  ```sh
  x86_64-w64-mingw32-g++ -O2 -Wall -Wextra -municode -static \
    -o /path/outside/repo/owwfd_inject.exe relay/owwfd_inject.cpp
  ```

  `-static` is required so MinGW does not leave a runtime dependency on `libwinpthread-1.dll` inside Wine (without it, Wine exits with code `53`).

- **Windows (MSVC x64 Native Tools Command Prompt — not verified on a Windows host):**

  ```bat
  cl /nologo /O2 /W3 /EHsc /DUNICODE /D_UNICODE relay\owwfd_inject.cpp /Fe:owwfd_inject.exe
  ```

Place `owwfd_inject.exe` outside version control and pass its path with `--inject-exe` or `ORPHEUS_INJECT_EXE`.

## Step-by-step usage

1. Start the native Rust server (`orpheus_server`) and launch Overwatch `1.74.0.0.104319` under Steam/Proton with `--BNetServer=127.0.0.1:1119` until the login screen appears.

2. List matching Windows processes in the active Proton session:

   ```sh
   cargo run --locked --bin orpheus_proton -- \
     --list --inject-exe /path/to/owwfd_inject.exe
   ```

   Expect one line per matching process:

   ```text
   win_pid=316	creation_time=134398083798092920	path=Z:\path\to\Overwatch\_retail_\Overwatch.exe
   ```

   If multiple Linux `Overwatch.exe` processes exist, pass `--runtime-pid <linux-pid>` to choose which process supplies `WINEPREFIX`, Proton tool, and Wine sync flags.

3. Inject explicitly using the exact `win_pid`, `creation_time`, and `path` printed by `--list`:

   ```sh
   cargo run --locked --bin orpheus_proton -- \
     --inject-exe /path/to/owwfd_inject.exe \
     --relay-dll /path/to/owwfd_relay.dll \
     --win-pid 316 \
     --creation-time 134398083798092920 \
     --game-exe 'Z:\path\to\Overwatch\_retail_\Overwatch.exe'
   ```

   Confirm that stdout reports:

   ```text
   Loaded Z:\path\to\owwfd_relay.dll into win_pid=316 at base=0x... (target process alive at check; verify relay readiness in game/logs).
   ```

   - Pass `--start-worker` if you are using a relay DLL build that exports `worker` (such as builds from `main` PR #22) and Wine's loader lock delays the `DllMain` thread. Omit `--start-worker` for DLLs built from unmodified `rust` `relay/owwfd_relay.cpp` (where `worker` is `static`).
   - Confirm relay readiness in the relay log (`wfd.log` next to `owwfd_relay.dll`) and by logging into the main menu in-game. Exit code `0` confirms the DLL loaded while the bound process was alive; it does not prove socket hook readiness by itself.

## CLI reference

| Option | Required | Description |
| --- | --- | --- |
| `--list` | One of `--list` or inject args | Enumerate matching Windows processes (`win_pid`, `creation_time`, `path`). |
| `--inject-exe <path>` | Yes (or `ORPHEUS_INJECT_EXE`) | Path to x64 `owwfd_inject.exe`. |
| `--relay-dll <path>` | Inject mode (default `relay/owwfd_relay.dll`) | Path to x64 `owwfd_relay.dll`. |
| `--runtime-pid <pid>` | Optional | Linux PID used **only** to resolve `WINEPREFIX`, Proton tool, and sync env. |
| `--win-pid <pid>` | Inject mode | Explicit Windows PID from `--list`. |
| `--creation-time <u64>` | Inject mode | Exact Windows `FILETIME` creation timestamp from `--list`. |
| `--game-exe <win-path>` | Inject mode | Expected full Windows executable path from `--list`. |
| `--exe-name <name>` | Optional (default `Overwatch.exe`) | Process name filter for `--list` and Linux PID scan. |
| `--timeout <secs>` | Optional (default `30.0`) | Positive finite timeout for the Wine helper subprocess. |
| `--start-worker` | Optional | Also start exported `worker()` via remote thread after `LoadLibraryW`. |

### Exit codes

- `0`: `--list` succeeded with all candidates readable, or DLL loaded into the bound process.
- `1`: Runtime resolution, process binding, or DLL load failed (or `--list` found no matches / unreadable candidates).
- `2`: Invalid or missing CLI arguments.
- `3`: Helper timed out or thread wait failed after `CreateRemoteThread` (**outcome unknown**; remote path buffer is retained until process exit and `wineserver` is left untouched).

## Isolated Wine verification (no game or user prefix)

Verify `owwfd_inject.exe` in a disposable prefix without touching an existing game or Steam prefix:

```sh
TMP=$(mktemp -d)
WINEPREFIX="$TMP/pfx" HOME="$TMP/home" WINEFSYNC=1 \
  "/path/to/Proton/files/bin/wine" "Z:/path/to/owwfd_inject.exe" \
  --list --exe-name owwfd_inject.exe
rm -rf "$TMP"
```

Confirm exit code `0` and output containing `win_pid=`, `creation_time=`, and `path=`.

## Troubleshooting

| Symptom | Cause | Action |
| --- | --- | --- |
| Wine exits with code `53` | `owwfd_inject.exe` was linked dynamically against `libwinpthread-1.dll` | Rebuild with `-static` as shown in Build step 2. |
| `none of the STEAM_COMPAT_TOOL_PATHS candidates are mapped` | Target PID is not running under any listed Proton tool or `/proc/<pid>/maps` is unreadable | Verify `--runtime-pid` points to the live `Overwatch.exe` process. |
| `creation time ... does not match expected` | Original process exited and Windows/Wine reused its PID | Re-run `--list` and pass the new process identity. |
| `export 'worker' not found in relay DLL` | `--start-worker` passed with a DLL whose `worker` is `static` | Re-run without `--start-worker`. |
| `timed out; outcome unknown` (exit `3`) | Loader thread did not finish before deadline | Do not retry automatically; check `wfd.log` or restart the game client cleanly. |

## Scope and verification status

- **Manual selection only:** `--runtime-pid` resolves the Wine environment only and does not assert Linux-to-Windows PID identity.
- **Verified in isolated Wine prefixes:** `Proton 10.0` (`proton-10.0-4b`) and `Proton 11.0` (`proton-11.0-2e-x86_64`), including live PID-reuse rejection via `creation_time`.
- **Verified live to main-menu login (`1.74.0.0.104319`, `enUS`, native `orpheus_server`):** `Proton 10.0`, `Proton 11.0`, `Proton Experimental`, and `Proton Hotfix` with `--start-worker`, plus `Proton Experimental` without `--start-worker` (starting `worker` from `DllMain`).
- **Not verified:** MSVC-built relay DLL from unmodified `rust` `relay/owwfd_relay.cpp` (uses MSVC SEH), Windows MSVC build of `owwfd_inject.cpp`, Steam Deck, `Proton GE`, `CachyOS Proton`, and non-default container-isolated Steam runtimes.
