# Overwatch 1.74 local Practice Range and Practice vs. AI

Runs on Windows retail build `1.74.0.0.104319` with Python 3.11+ and `cryptography`. Services bind to IPv4 loopback. The default launcher uses the retail client directly.

Run from a normal desktop terminal, replacing `<game-root>`:

```powershell
python tools/lobby/run.py '<game-root>'
```

Set the local profile name with `--name Ranger7` (1–12 ASCII letters or digits; default `Player`):

```powershell
python tools/lobby/run.py '<game-root>' --name Ranger7
```

The name is used for login, Career Profile, the party card and the match roster. Include `--name` on each launch. Standalone `lobby.py` accepts the same option. The server uses this name regardless of the name-entry screen's value.

With `--asi tools/asi/build/retail_menu.asi` the Pod and name screens are skipped and the game boots to the main menu. Without it: on the Blizzard startup screen set **Pod 1**, then click **Start** (the missing-config warning can remain), enter a local name, click **Enter Game**, then wait for the main menu.

The default menu uses ordinary backdrops and a random hero chosen per login. Hero visibility requires the rebuilt `retail_menu.asi`. Use `--event farewell` for the farewell lineup; `--event none` explicitly selects the normal backdrop catalog. With `--map`, an event also hosts that map's event variant when it has one: `--event winter --map Hanamura` plays Hanamura (Winter). Other event presets retain their authored scenes.

Tournament mode blocks Alt+Tab and the Windows key unless `retail_menu.asi` is loaded (it removes that keyboard hook). Use **Exit Game** from the menu to close normally. By default the launcher has no time limit: it waits for the game to exit, then stops its server, and also cleans up on Ctrl+C in its terminal. To close both automatically, for example after ten minutes:

```powershell
python tools/lobby/run.py '<game-root>' [--timeout 600]
```

The launcher rejects any executable whose SHA-256 differs from the verified build, and refuses occupied TCP 3724 or UDP 3730. It writes each run's logs beneath `research/main-menu/` and does not edit user settings. The game keeps its own settings in `<game-root>/OfflineProfile/Documents/Overwatch/Settings/Settings_v0.ini`, not in your Documents folder, which Overwatch 2 uses for the same file. The first launch copies the file you had. The Options the client saves on the server (Gameplay and Social among them) are kept in `tools/lobby/data/settings.json`, and the Career Profile's stats in `tools/lobby/data/career.json` (every match but the Practice Range counts; delete the file to start over). Show mode was tested with Pod 1 / Group 1 and stayed at Entering Game; use **TournamentMode**.

The server supplies a synthetic local profile. **Play → Practice Range → Select Soldier: 76** loads the original range with its 15 bots. The launcher extracts map collision from the verified install as needed. Bots shoot, take damage and respawn. Soldier has numeric health and ammo, Sprint, Helix Rockets, Biotic Field and Tactical Visor. Press R to reload. **Escape → Leave Game → Yes** returns to Play.

Pass `--map Hanamura --difficulty medium` for a match against hero bots. The opt-in [retail-menu plugin](../asi/README.md) enables **Training → Practice vs. AI → Easy / Medium / Hard**. The 32 selectable heroes have varying weapon and ability coverage. Mercy can switch between her staff and blaster with 1 and 2. See the [roster animation audit](../../research/roster-animation-audit.md) and [weapons sweep](../../research/weapons-sweep.md) for retail evidence.

## Earned accounts

Add `--progression --name OfflinePilot` to use a separate earned account. Without `--progression`, the existing all-unlocked research account and its saved equips remain available.

```powershell
python tools/lobby/run.py <game-root> --asi tools/asi/build/retail_menu.asi --progression --name OfflinePilot --map Hanamura --mode 0x0230000000000014
```

Profiles live in `tools/lobby/data/profiles/<name>.sqlite3`. The launcher extracts a typed progression catalog from the verified installation when needed. Keep profile files to retain levels, credits, cosmetics, icons, loadouts and unopened boxes. A damaged or unsupported database fails closed; it is never replaced with a new account.

Completed local Assault matches award XP once and grant one standard box per crossed level. Practice Range, abandoned matches, Skirmish and unfinished PvE missions award none. Initial offline policy: 1.75 XP per match second, 75 completion XP, 250 for winning and 1,500 for the first win of each UTC date. Level 1 needs 2,000 XP; subsequent levels need 20,000, retaining overflow. This introductory curve/calculation approximates unresolved retail arithmetic.

Each standard box grants four rewards, at least one Rare or better, from the installation's typed normal pools. Default/achievement/event/OWL/golden-weapon rewards stay outside those pools. Offline rarity weights are 75:20:4:1 (Common:Rare:Epic:Legendary), with the first slot restricted to Rare or better. Unowned items take priority within the rolled rarity; exhausted pools pay 5/15/50/200 duplicate credits. Direct currency rewards use their asset amounts. Retail distribution and pity rules remain unresolved.

Currency, ownership and box receipts commit atomically. Repeated/concurrent opens replay the committed result; repeated match settlement cannot pay twice. Save failures retain unopened boxes and retry pending match rewards. Close the game normally before copying a profile database. The launcher clears native cosmetic override `2704A3F4` through CONFIG 36600 for earned accounts, retaining tournament networking; otherwise the client would treat every cosmetic as unlocked.

Only ordinary credit purchases are supported. Seasonal rotation/challenges, Arcade weekly rewards, endorsements, CP awards and OWL purchases are not implemented. Native four-item reveal, inventory consumption, relog and a 75-credit skin purchase were verified with an isolated QA profile. [Implementation evidence](../../research/progression/implementation-2026-11-08.md), [research](../../research/progression/REPORT.md).

## Development checks

```powershell
python -m unittest discover -s tools/lobby -v
python -m pytest tools/lobby tools/assets tools/research -q
```

Tests cover the menu protocol, launcher validation/liveness, malformed Practice Range requests, duplicate/input suppression, exact handoff bytes, authenticated UDP exchange, tampered datagrams, map-load message/quanta, Soldier initialization/acknowledgements and isolated aim geometry. Geometry tests do not establish damage in-game. Real-client menu evidence is recorded in `research/main-menu/REPORT.md`, with `main-menu.png`. Bounded launcher sessions confirmed normal exit and port release.

For protocol work, the server can run separately:

```powershell
python tools/lobby/lobby.py
```

Then launch the client from `_retail_` with `--tank_TournamentMode --lobbyServer=127.0.0.1:3724`. This manual route has no launcher timeout; close the game normally and Ctrl+C the server.

## Sources

- [Corrected channel setup/login notes](https://rentry.co/3bswrgmw): type-13 trailer, announcement-order group mapping, Base ack, login message layouts and boolean packing.
- [Lobby research source](https://github.com/Boi-027/Overwatch-1-v1.74-Lobby-Research/blob/main/server/lobbyserv.py): PANAMA stream implementation and tournament handshake. `panama.py` preserves the extracted cipher implementation; license is in `LICENSE.panama`. The old server's channel-0 experiments are superseded by the corrected notes.

Only use tournament mode with this server. Its unsigned handshake block relies on that route's zero session keys; it is not a general Battle.net service. Game-server debug logs include synthetic player names and decoded game messages. No real account credentials are needed.

Game transport/world code and data are an audited subset of a pinned MIT implementation; origin and local modifications are recorded in `vendor/README.md` and `vendor/LICENSE`.
