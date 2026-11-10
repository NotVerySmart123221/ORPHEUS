# Offline service

A second, separate way to run the 1.74.0.0.104319 client on one machine: a loopback lobby and game service with a
Practice Range, Practice vs. AI and local progression, plus optional client plugins and research tools. It was built
on its own before this branch existed and is imported here whole, so that its work is in one place with ORPHEUS and
can be moved into the ORPHEUS server piece by piece.

It is **not** part of the ORPHEUS server in `src/`. Nothing in `src/` imports it, the repository's own checks leave
it out, and it keeps its own launcher, tests and change logs.

## How it differs from ORPHEUS

| | ORPHEUS (`src/ow174`) | This folder |
| --- | --- | --- |
| Players | Several, over a network | One, on loopback |
| Login | Battle.net emulation through the relay | The client's tournament route, no relay |
| Heroes | Each hero's own statescript in the script runtime | Hand-built weapon and ability tables for 32 heroes, with the script runtime as an opt-in for the body |
| Client code | Unchanged apart from the relay | Optional plugins (`tools/asi`) for retail menus, 32:9 and research probes |

`tools/lobby/vendor/ow174` is this service's own copy of the upstream game-server code it grew from. It has the same
package name as `src/ow174` and has diverged from it. The service puts its copy first on the import path, so run its
tools and tests on their own, never in one process with ORPHEUS's.

## Layout

| Path | Role |
| --- | --- |
| `STATUS.md` | What works and what is open |
| `tools/lobby` | The lobby and game service, its launcher `run.py`, and their tests. [README](tools/lobby/README.md) |
| `tools/asi` | Client plugins, built with MSVC. [README](tools/asi/README.md) |
| `tools/assets` | Readers of the installation's own data |
| `tools/research` | Live-process and replay tools |
| `tools/blender` | Animation import and render scripts |
| `research` | Notes, and the mover recordings the tests replay |
| `heroes`, `tasks` | Per-hero fix logs, the roadmap and plans |

Paths inside these files are relative to this folder: `python tools/lobby/run.py` means run it from `offline/`.

Links in the notes to screenshots, logs and recordings point at evidence that stays in local storage and is not in
the repository.

## Setup

1. The repository's own environment (Python 3.12, `pip install -e ".[dev]"` at the root).
2. `pip install -r offline/requirements.txt`.
3. The local inputs listed in [tools/lobby/vendor/data/README.md](tools/lobby/vendor/data/README.md).
4. For the tools and tests that read the installation, the environment variable `OW174_GAME` set to the game
   folder.
5. For the plugins, `tools\asi\build.bat` with MSVC 2022 x64.

## Run

From `offline/`:

```powershell
python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi
```

It boots to the retail main menu. Each run writes its logs under `research/main-menu/`, which is ignored.

## Tests

From `offline/`:

```powershell
python -B -m pytest tools/lobby tools/assets tools/research tools/blender -q
```

640 tests passed on 2026-11-10 with the local inputs and the installation in place. A fresh clone of the branch
given the same local files passed 635 and skipped 5.

- 14 tests replay recordings through the map collision and read the mission markers. They need
  `tools/lobby/data/` and `tools/lobby/vendor/cache/`, which `run.py` builds from the installation the first time
  it starts. Without them those tests fail.
- 5 tests compare the weapon tables with a local damage snapshot written by `tools/research/hitdef_shadow.py`
  and are skipped without it.

## Seen in the client

2026-11-10, launched from this folder: retail main menu at 32:9, Training, Practice Range, hero select and spawn as
Soldier: 76. Against a still training bot placed by `tools/research/vm_live.py`: the bot's name and health bar, hits
logged down to zero health, the kill feed line, and the Tab board with eliminations, a gold medal and weapon
accuracy. The elimination notice was not caught in a frame.
