# Linux Proton Relay Helper (`orpheus_proton`)

`orpheus_proton` is a Linux CLI helper that resolves the active Proton runtime, `WINEPREFIX`, and Wine sync flags (`WINEFSYNC`, `WINEESYNC`, `WINE_NTSYNC`, `PROTON_ENABLE_NTSYNC`, `WINEFUTEX2`) from a running game process and invokes a small x64 Windows helper (`owwfd_inject.exe`) inside that Wine session.

It is an explicit, manual CLI tool rather than a one-click launcher. Do not commit compiled `.exe`/`.dll` binaries, game installations, runtime packs, or local logs.

## Developer Build Instructions

### 1. Build the Linux host binary (`orpheus_proton`)

Requires Rust 1.85+ (verified on Linux):

```sh
cargo build --locked --bin orpheus_proton
```

### 2. Build the x64 Windows/Wine helper (`owwfd_inject.exe`)

- **On Linux (MinGW-w64 — verified):**

  ```sh
  x86_64-w64-mingw32-g++ -O2 -Wall -Wextra -municode -static \
    -o owwfd_inject.exe relay/owwfd_inject.cpp
  ```

  `-static` is required so MinGW does not leave a runtime dependency on `libwinpthread-1.dll` inside Wine.

- **On Windows (MSVC x64 Native Tools Command Prompt — not yet verified on a Windows host):**

  ```bat
  cl /nologo /O2 /W3 /EHsc /DUNICODE /D_UNICODE relay\owwfd_inject.cpp /Fe:owwfd_inject.exe
  ```

Place the compiled `owwfd_inject.exe` outside version control (for example in your local runtime folder) and pass it via `--inject-exe` or `ORPHEUS_INJECT_EXE`. Binary distribution (if any) is left to maintainers.

## User Guide

1. Start the native Rust server (`orpheus_server`) and launch Overwatch `1.74.0.0.104319` under Steam/Proton to the login screen.
2. List matching Windows processes inside the active Proton session:

   ```sh
   orpheus_proton --list --inject-exe /path/to/owwfd_inject.exe
   ```

   If multiple Linux `Overwatch.exe` processes exist, pass `--runtime-pid <linux-pid>` to choose which process supplies `WINEPREFIX`, Proton tool, and Wine sync flags.

3. Inject explicitly using the exact `win_pid`, `creation_time`, and `path` printed by `--list`:

   ```sh
   orpheus_proton \
     --inject-exe /path/to/owwfd_inject.exe \
     --relay-dll /path/to/owwfd_relay.dll \
     --win-pid <PID> \
     --creation-time <FILETIME> \
     --game-exe '<WINDOWS_PATH>'
   ```

   - Add `--start-worker` when using a relay DLL build that exports `worker` (such as builds from `main` PR #22) if Wine's loader lock delays the `DllMain` thread. Omit `--start-worker` for relay DLLs built from unmodified `rust` `relay/owwfd_relay.cpp` (where `worker` is `static`).
   - Verify relay readiness in the relay log and by logging into the game; a `0` exit code confirms the DLL loaded (and `worker` thread was created if `--start-worker` was passed) while the target process was alive at check time.

## Isolated Wine Verification (No Game or User Prefix)

To verify `owwfd_inject.exe` safely without touching an existing game or Steam prefix:

```sh
TMP=$(mktemp -d)
WINEPREFIX="$TMP/pfx" HOME="$TMP/home" WINEFSYNC=1 \
  "/path/to/Proton/files/bin/wine" "Z:/path/to/owwfd_inject.exe" \
  --list --exe-name owwfd_inject.exe
rm -rf "$TMP"
```

## Scope and Limitations

- **Manual Windows target selection:** `--runtime-pid` resolves the Wine environment only and does not assert Linux-to-Windows PID identity.
- **Fail-closed runtime and process checks:** relative paths in `WINEPREFIX` or `STEAM_COMPAT_TOOL_PATHS`, unmapped Proton tools in `/proc/<pid>/maps`, unreadable candidates, or mismatched `image_path` / `creation_time` abort without guessing.
- **Timeouts:** `orpheus_proton` kills only its spawned helper child on timeout (never `wineserver`) and reports `outcome unknown` without retrying.
- **Tested vs. untested environments:**
  - Verified in isolated Wine prefixes: `Proton 10.0` (`proton-10.0-4b`) and `Proton 11.0` (`proton-11.0-2e-x86_64`).
  - Verified live to main-menu login (`1.74.0.0.104319`, `enUS`, `--start-worker`): `Proton 10.0`, `Proton 11.0`, `Proton Experimental`, and `Proton Hotfix`.
  - Not yet verified with this Rust/C++ port: Steam Deck, `Proton GE`, `CachyOS Proton`, Windows MSVC build of `owwfd_inject.cpp`, and non-default container-isolated Steam runtimes.
