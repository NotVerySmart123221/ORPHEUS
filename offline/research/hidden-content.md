# Hidden and unused content in 1.74 (2026-11-09)

Question: what hidden, cut or never-released content is still in the 1.74 data, and can the offline build bring it back?

Research only. No tool, lobby or game file was changed. The survey (sections 1-7) was read offline from the CASC install (`<game-root>`), plus a string scan of `ida/Overwatch-runtime.dump`. Then the top candidates were tried in the running game; results are in [Live results](#live-results-2026-11-09). Scratch scripts are in `.scratchpad\ow-hidden\` (deletable): `scan_unlocks.py`, `orphans.py`, `maps.py`, `mapchunks.py`, `modes.py`, `binds.py`. They reuse `.scratchpad/pva/pvalib.py`. The live probes are in `live\` there (see [Live results](#live-results-2026-11-09)).

Evidence tags: **[asset]** read from the 1.74 data; **[bin]** a string in the runtime dump; **[web]** a public source; **[live]** seen in the running game; **[inf]** inference.

## Short answer

1. **The shipping manifest drops assets that nothing references.** Unused animations, cut ability scripts and old poses are mostly gone. What survives is either still referenced somewhere, or comes from a whole asset type that ships complete: unlocks, map headers, logical buttons and ability cards.
2. **Most promising goodies, by effort:**

| # | Goodie | What's there | Effort | Route | Live check |
|---|---|---|---|---|---|
| 1 | **Full-roster OWL All-Stars skins** (Atlantic and Pacific, all 32 heroes) | 64 unlocks, themes in every hero's skin list | Low | Server only: put the unlock in the Skin slot | **Works in a match** (Atlantic rifle livery). Gallery hangs; the menu backdrop shows a gray uniform |
| 2 | **Brigitte "Bmed - Temp"** dev skin | 1 unlock with a 34-entry override theme | Low | Same as 1 | **Renders** on the menu backdrop: white armour, magenta trim. Gallery hangs |
| 3 | **Hidden DEBUG controls**: GOD MODE, UNKILLABLE, GHOST, TELEPORT TO RETICLE, CAMERA FREE VIEW, MOVE 0.5x/3x, ... | 16 logical-button assets with names | Medium | Make them bindable, then serve the effect in our server | Category stays hidden, even with the dev gate. Binding needs the keybind message decoded |
| 4 | **Assault Test** game mode | Own mode asset and graph `0x29A6` on Anubis, Hanamura, Volskaya | Medium | Swap the mode graph in the vs-AI Assault flow | **Client accepts it** ("DEFEND", Hanamura loads). No objectives until the server runs `29A6` |
| 5 | **Retribution** (Rialto, disabled header) and **Storm Rising** missions | Full map data (64.7 MB) plus mode `0x025` / `0x043` | Medium-high | Same route as Uprising | Not tried |
| 6 | **ShowMode** (show-floor/kiosk mode, `--tank_ShowMode`) | Flag, own settings ini, pod->lobby config, "Demo complete" text | Medium | Launch flag plus pod config | Boots to the show-floor **Pod screen**. START is blocked by a hidden group field |
| 7 | **VPP Green Room** map (Blizzard video production) | Map header plus small map data | ~~Medium~~ Not feasible | Add to 20505's map list and load it | Loading screen names it, then never finishes: its map chunks are stripped |
| 8 | **Unused menu backgrounds** (Mardi Gras, OWL HeroPanel/Atlantic, MvP Zarya, Moon, ...) | ~30 lobby maps absent from the vendored map table | Medium | Event menu route, or force the lobby map | Not tried |
| 9 | OWL home/away uniforms with no team colours | 75 unnamed unlocks plus Baptiste's "Overwatch League Gray/White" | Low (low value) | Same as 1 | Winston's base was equipped; same gallery hang |
| 10 | 3 orphan sprays: Primal Punch, Snowcer, Snowscientist | Unlocks, no pool references them | Low-medium | Server-side spray slot | Not tried |

3. **Not recoverable from the data:**
   - Tracer's original "Over the Shoulder" pose.
   - Old ability logic (old Torbjörn and Symmetra): only their HUD cards are left.
   - An "old Numbani" (its second header uses nearly the same data).
4. **Already in the offline build:** every limited cosmetic in a hero's progression. Examples: Pink Mercy, Noire, Overgrown Bastion, BlizzCon, Origins, OWL team skins, event skins. `hero_content.py` reads the whole progression asset, so 24900 already marks them owned.

## 1. Cosmetics nobody can unlock

Method **[asset]**: read all 7,377 `STUUnlock` (type `0xA5`) assets with name (`m_name` `B48F1D22`) and availability text (`m_53145FAF`, OWLib's `AvailableIn`). Then collect every 0x025 reference from these asset types:

- heroes `0x75`
- progression `0x58`
- generic settings `0x54`
- arcade cards `0xD9`
- unlocks `0xA5`
- game modes `0xC5`
- map headers `0x9F`
- every other type with up to 1,200 assets

Result: 7,377 unlocks, **184 referenced by nothing**. The 51 currency/competitive-point orphans were dropped; they turned out to be referenced by type `0xC7` (season rewards). Graphs and entities (types with more than 1,200 assets) were not scanned.

| Orphans | Count | GUIDs | Notes |
|---|---|---|---|
| "Overwatch All-Stars Atlantic" skins | 32 (every hero up to Echo/Sigma) | `0x0250000000004B3D`..`4B5C` | **Never sold.** The six one-hero sets are separate, referenced unlocks: "2018 Atlantic/Pacific All-Stars" Tracer `..1973` / Genji `..198E`, 2019 Mercy `..279E` / Lúcio `..27BC`, 2020 Reinhardt `..43B8` / D.Va `..3F7B` **[asset][web]** |
| "Overwatch All-Stars Pacific" skins | 32 | `0x0250000000004B5D`..`4B7C` | Same |
| "Bmed - Temp" skin, Brigitte (`0x02E0000000000195`) | 1 | unlock `0x02500000000049E5`, theme `0x0A50000000004740` | A dev placeholder name. Its theme overrides 34 assets: effects `0x030...`, models `0x0D0...` and others. Look unknown |
| Unnamed skins | 75 (2-3 per hero) | e.g. Soldier `..0E75` (theme `0x0A50000000001B25`), `..11F7` (theme `..1C05`) | **OWL uniform bases.** All 198 of Soldier's team-colour variants (`0x0A508...`) use one of these two as their base (`m_02F889AE`: 113 and 85), and they share the OWL models `0x0D00...53C0/5400/5401/56E6`. Equipped, they would show a blank home/away uniform **[inf]** |
| "Overwatch League Gray" / "White", Baptiste | 2 | `..27C1`, `..27C2` | The named Baptiste equivalent of the bases above |
| Sprays | 3 | Primal Punch `..0860`, Snowcer `..0D7B`, Snowscientist `..0D7C` | No pool references them |
| Loot boxes | 4 | Golden `..1160`, Legendary Anniversary `..198F`, Wrecking Ball `..1A65`, Legendary `..4B0C` | Granted by the server in retail |
| OWL tokens, credits, `STU_1EB22BDB` `..1965` | rest | | Not cosmetics |

Feasibility for skins **[asset][inf]**:

- 134 of the 142 orphan skins have their theme in the hero's own `m_skinThemes`, so `lobby.skin_themes()` already sends them in 20505's preload list. That includes all 64 All-Stars, Bmed and both Baptiste skins.
- The lobby owns the loadout: `practice.equipped()` -> 24900 Skin slot `+0x38`, saved in `data/equips.json`. Putting an orphan unlock GUID there is a server-only change.
- The Hero Gallery builds its lists from the client's own progression data, so the orphans can't appear there. They can only be equipped by the server.
- **[live]** The client renders the theme in a match and on the menu backdrop, but not in the Hero Gallery. Details in [Live results](#live-results-2026-11-09).

## 2. Hidden DEBUG controls category

Under the Controls category strings (`0x0DE...0F3F` MOVEMENT, `0F42` WEAPONS & ABILITIES, `0F43` COMMUNICATION) there is **`0F44` DEBUG** **[asset]**. The logical buttons are `STU_C5243F93` assets (type `0x96`): `m_name` `C08C4427`, `m_logicalButton` `3C9341F5` (the `STULogicalButton` value), `m_modeFlags` `F6D8234A`.

| Button (value) | Asset | modeFlags |
|---|---|---|
| GOD MODE (4) | `..0022` | **1** |
| UNKILLABLE (73) | `..0095` | **1** |
| GHOST (5) | `..0029` | 0 |
| LOG STATESCRIPT (6) | `..003D` | 0 |
| TOGGLE SOUND (1) | `..0008` | 0 |
| TOGGLE CAMERA VIEW (24), CAMERA FREE VIEW (25), CAMERA FREE INPUT MODE (26), CAMERA FREE SMOOTHING MODE (27) | `..0062`, `..0063`, `..01B2`, `..01B3` | 0 |
| DEV SPECTATE NEXT (28) | `..0064` | 0 |
| MOVE 0.5x / MOVE NORMAL / MOVE 3x (32-34) | `..0075`-`..0077` | 0 |
| TELEPORT TO RETICLE (61) | `..007C` | 0 |
| TOGGLE UI (62) | `..0091` | 0 |
| LAG PEDAL (63) | `..0092` | 0 |
| REPLAY PAUSE / STEP / FORWARD / BACK | strings `29D5`, `29D6`, `29CE`, `29CF` | n/a (no button asset found) |
| SOUND OUTPUT PAUSE / PREV | strings `29E1`, `29E6` | n/a (no button asset found) |

- `modeFlags = 1` is also set on JUMP, CROUCH, RELOAD, PRIMARY FIRE, ABILITY 1-3 and QUICK MELEE, i.e. the buttons that reach the server, while the menu buttons have 0. **[inf]** So GOD MODE and UNKILLABLE most likely arrive in the command stream, and the effect lives on the server. Retail servers ignore them; ours could serve them (no damage, no death).
- The flag-0 buttons are client-side. Whether their handlers survive in retail is unknown. GHOST and TELEPORT also move the player, which needs the server to agree (`debug.DisableErrorCorrection` is a live cvar, [dev-console.md](dev-console.md)).
- Command bits are not `1 << button`: QUICK MELEE is button 31, but [melee.md](melee.md) has it as bit `0x800`.
- Open: how to bind them. **[live]** The dev gate `dev_console.asi` sets (`0x404F5C8`) does not show the category: Controls ends with the retail system bindings. Bindings are not in `Settings_v0.ini`; the client sends them to the lobby (see [Live results](#live-results-2026-11-09)). Left to try: `STULocalPersistedBinding` (`19C333CE`), or serving a binding for a DEBUG button. After that, press each one with `tracer.asi` on and see what the lobby receives.

## 3. Debug and dev modes in the client

| Item | Evidence | Notes |
|---|---|---|
| Developer console (Alt+`) | done | [dev-console.md](dev-console.md) |
| **ShowMode** `--tank_ShowMode` | **[bin]** "Start up this application in show mode", settings file `ShowMode_Settings_v0` (`%s/Settings/%s.ini`), `showmodeLobbyLocations.json` ("pod->lobby lookup config"), and `ShowMode` / `FeatureFlags` among the UX data-binding variables (next to `IsPlatformPC`). **[asset]** "Demo complete. Thanks for playing!" (`0x0DE...2AC5`, among the matchmaking messages) | The show-floor build mode (BlizzCon/PAX pods). Flag `0x7FF6D404F420` is already known ([login-skip.md](login-skip.md), [tournament-input-lock.md](tournament-input-lock.md)). It writes its own ini, so the user's settings stay untouched. **[live]** Boots to a Pod screen; see [Live results](#live-results-2026-11-09) |
| `--tank_FeatureFlags`, `--tank_DynamicConfig` | **[bin]** "Start up this application with specific feature flags set" | No flag names found yet |
| `--tank_hotfixJSON` / `hotfixPath` / `hotfixKey` / `hotfixName` | **[bin]** local Hotfix folder `%LOCALAPPDATA%\Blizzard Entertainment\Overwatch\104319\Hotfix`, "Hotfix Load" | A local hotfix might switch content without data edits. Format unknown |
| `--tank_HideDevDisplays`, `tank_SaveInspectorReports`, `tank_EnableHotReloading`, `tank_ServerSkinLoadMode`, `replayViewerMode` / `replayTeamSlot` | **[bin]** | Dev displays exist (there is a switch to hide them) |
| DEV LOGIN button | **[asset]** `0F52` next to LOGIN / `0F55` "CONFIGURATION:" | Players saw it briefly in 2017-2019 and it did nothing **[web]** |
| "Custom Game (DEV ONLY)" | **[asset]** `1D1E`, next to "CUSTOM GAME" | Where it shows is not traced |
| "Debug - Unified Enabled" | **[asset]** `11C0A`, among the Workshop action strings | A Workshop debug entry |
| "TEST - Default", "TEST - Other" | **[asset]** `AA76`/`AA77`, after the server region names | Test regions |

## 4. Maps

Method **[asset]**: all 132 `STUMapHeader` (type `0x9F`): display name, internal path (ASCII in the header) and state (`m_A125818B`). Then a check of how many map chunks (`0x0DD...`) each one has in the manifest.

- **VPP Green Room**: header `0x06C7`, `Maps\VPP\BVid_Green_Room`, display name "VPP Green Room". Not in `vendor/data/game_maps_174.json`. 20 chunk rows, 2 readable, 0.2 MB. That is the same shape as working small maps such as the farewell menu `0x0F14` (2 readable, 0.1 MB). **[inf]** A video-production room (the public Workshop Green Screen is `0x0D4C`). Spawns unknown. **[live]** Not loadable: its collision chunk `0x0DD00011000006C7` is not in the install, and the client never leaves its loading screen.
- **Rialto (Retribution)**: `0x079F`, state Disabled, `Venice - Rialto - Archives`, mode `0x025`. 10 readable chunks, 64.7 MB; only 6 of its 20 chunk types match PvP Rialto, so it is a real separate map. King's Row (Uprising) `0x06FE` looks the same way, and it already loads ([uprising-mission.md](uprising-mission.md)). Storm Rising: Havana `0x0AFE`, mode `0x043`.
- **Numbani** `0x01D4` (Disabled, `Numbani - PvP`) and `0x0751` (`Numbani_DoNotUse`, the enabled one): 19 of 20 chunk types are identical. Not an older Numbani.
- **Lobby (menu background) maps missing from the vendored table**: `0664` Kyoto, `0675` Gibraltar, `0676` London, `0677` Russia, `0678` Hollywood, `06E0` Castle Halloween 2020, `0700` London Winter, `0710` Busan Lunar, `0715` China Lunar **DONOTUSE** (21 MB), `0764` King's Row Archives, `0794` Moon (Disabled), `07A0` SummerGames Sydney, `0817` World Cup event, `0862` OWL HeroPanel, `0864` OWL Atlantic, `0A13` OWL Grand Finals, `0B2D` OWL All-Star 2020, `0BCE` SummerFun 2020, `0BEF` OWL 2021, `0C46` Cairo (Mercy), `0C83` OWL Lobby, `0C8F` **Mardi Gras**, `0CCF` Anniversary 2020, `0CE0` **MvP - Zarya**, `0D0A` OWL Playoffs 2020, `0D2C` Reaper Challenge, `0D2D` OWL Grand Finals 2020, `0D67` Symmetra Challenge India, `0D77` London Winter 2020, `0D7A` Antarctica Winter 2021, `0DAD` China Lunar 2021, `0E1F` OWL Sombra 2022, `0E44` Cairo (Cassidy), `0E83` Best of Overwatch, `0EEE` OWL Genji, `0F14` Goodbye.
  - Several are already mapped from events ([hero-menu.md](hero-menu.md) §4: `0x664`..`0xF14`).
  - The others have no known event route. A plugin hook, or a way to force the lobby map, would be needed.
  - The map-catalog blobs (type `0x39`) did not decode with the plain ref scan: refs are mangled per field, as noted in hero-menu.md.

## 5. Game modes

25 `STUGameMode` (type `0xC5`) **[asset]**. All are known except:

- **Assault Test** `0x0230000000000067`, on Anubis, Hanamura and Volskaya. Strings `0x0DE...11194/11195/1124F/11262` are late-OW1 indices.
  - Differences from Assault `0x...0014`: mode graph `0x05800000000029A6` instead of `0F4A`, and one team entry fewer.
  - The new graph has 4,539 instances against 4,309, with the same six subgraphs. It adds 21 SetVar actions, 15 int config vars and 8 containers.
  - **[inf]** A late tuning variant of 2CP. Playing it means serving `29A6` in place of `0F4A` (`graph_record.py` can produce its record).
  - **[live]** The client takes the mode as is. Details in [Live results](#live-results-2026-11-09).
- Unnamed PvE modes `0x00F` and `0x01A` (both on King's Row Uprising).

Rulesets (type `0xC0`, 112): 2 of them hold Workshop script text (`0C3`, `0CA`) and no card references them. The arcade schedule comes from the server, so "unreferenced" here doesn't mean unused. Workshop preset blurbs exist for modes that may never have been on a card, e.g. `0xC4D0` "This simple demo features a shrinking shell of fire" (not matched to a ruleset).

## 6. Heroes and abilities

- 24 non-playable `STUHero` ([hero-menu.md](hero-menu.md) §1): Training/Friendly bots, the PvE units (Talon Trooper/Sniper/Heavy Assault/Assassin/Enforcer/Gunship, Null Sector units, Zomnics, Junkenstein units), Bob, Boat.
  - Also hero **`0x165`**: no name, but a full gameplay entity `0x04000000000011F8` (health, mover, weapon, statescript) on Bastion's model `0x0D00...0B44`. **[inf]** A tutorial or PvE Bastion unit.
  - Our server picks the hero, so playing as a PvE unit is possible in principle. Each needs its weapon kit served.
- 6 of 196 ability cards (`STULoadout`, type `0x9E`) are on no hero:
  - **Molten Core** `0x0B9...001A`, the pre-2018 Torbjörn text ("temporarily upgrade a level 2 turret to level 3").
  - **Shield Generator** `..00F9`, pre-3.0 Symmetra.
  - Healing Boost `..00A2`.
  - Yeti Trap `..017F`, Thaw `..02F9` (event modes), Quick Melee `..02F5`.
  - Only the card (icon texture, name, description) is left; no ability graph is referenced. Restoring those kits would mean writing them from scratch.

## 7. Not in the data

- **Tracer's original "Over the Shoulder" pose** (replaced in April 2016 after the March 2016 forum complaint **[web]**).
  - Pose unlock `0x025...082E` -> `STULineupPose` `0x07D0...003A` -> animation `0x0A00...3B51`.
  - The launch-era neighbours are Salute `..3965` and Sitting `..3969`. The indices between them (`3966`-`3968`) are **absent from the manifest**, and all 306 lineup poses are in use. The original was most likely `3966`-`3968` and was stripped.
  - Bringing it back would mean re-animating it (e.g. in Blender from footage). That is a new asset, not a restoration.
- Old ability logic (Scatter Arrow, old Symmetra/Torbjörn/Mercy): not searched graph by graph. Given the stripping above and the two cards with no graph, expect none.
- Voice lines: not surveyed (unused lines would need the speech manifest scan in `voice_lines.py`).

## Live results (2026-11-09)

All runs used `tools/lobby/run.py` with `ow_fixes.asi`. Overrides came from scratch probes in `.scratchpad\ow-hidden\live\` (deletable), which monkeypatch the lobby at launch and leave the repo unchanged:

- `run_probe.py`: `run.py` with extra game arguments (`PROBE_GAME_ARGS`). With `PROBE_MAP` set, it skips the collision extract.
- `lobby_probe.py`: `lobby.py` with skin themes forced into component 75 (`PROBE_SKINS`) and a fixed menu hero (`PROBE_MENU_HERO`).
- `inject_map.py`: adds a bare map entry to the vendored table (`PROBE_MAP`, `PROBE_SPAWN`).

`data/equips.json` was edited for test 1 and then restored. The user's `Settings_v0.ini` is unchanged.

### 1. Orphan skins (All-Stars Atlantic/Pacific, Bmed, OWL base)

Equipped through `equips.json`: Soldier Atlantic `..4B56`, Tracer Pacific `..4B7A`, Brigitte Bmed `..49E5`, Winston OWL base `..0E76`.

| Where | Result |
|---|---|
| Hero Gallery | The skin name shows ("OVERWATCH ALL-STARS ATLANTIC"), but the model **stays in the spawn beam**. Same for all four. Control: Mercy's normal skin renders |
| Main-menu backdrop | **Bmed renders**: white armour, magenta trim. Soldier Atlantic renders as the **league-neutral gray OWL uniform**, without the All-Stars colours |
| Hero select | Previews Classic |
| In a match (player) | The stock server sends no theme in component 75. With the probe setting it, **the first-person rifle wears Atlantic**: navy and teal, "ATLANTIC" lettering, green grips |
| In a match (bots) | Stay Classic, even with the theme injected into the bot controllers' component 75 |

**[inf]** The client can draw the orphan themes; the gallery's preview path refuses them.

**Done (same day):** `practice.PracticeRange.skin_of` now serves the theme of the saved Skin equip ([tools/lobby/CHANGELOG.md](../tools/lobby/CHANGELOG.md)). To wear an All-Stars skin, add `[hero, unlock, 0, "Skin"]` to `tools/lobby/data/equips.json`. Checked live with Soldier Atlantic and the stock launcher. Left: what bot bodies read instead of component 75, and the gallery hang.

### 2. DEBUG controls

- With `dev_console.asi` loaded, Options > Controls has no DEBUG category. The list ends with the retail system bindings.
- A rebind (F7 -> Toggle UI, for the test) is not written to `Settings_v0.ini`. The client sends it to the lobby on `SOCIAL_OUT` (`0x75D32AE2`, group 41, a 60-byte message). Message 22200 carries 6-byte `{u16, u8 x4}` entries, likely the binding table.
- `practice.py` ignores these messages, so a rebind lasts one session.
- Next step: decode those entries and serve one that binds GOD MODE (button 4).

### 3. ShowMode (`--tank_ShowMode`)

- It forces a fullscreen window at the desktop size, so `PrintWindow` captures black and posted clicks don't register. Use foreground input and a screen grab.
- With `ow_fixes.asi`, login completes but the front end stays black.
- Without plugins, it boots to the **show-floor Pod screen**: Blizzard logo, a POD field, RESET and START.
- The pod -> lobby config is `_retail_\showmodeLobbyLocations.json`, keys `pod<N>` (the key prefix "pod" is an encrypted string in the binary): `{"pod1": "127.0.0.1:3724"}`. Without it, the POD field shows a red error.
- START also needs a hidden group value of 1 or 2: cvar `PartyId` (`0x7FF6D4178418`), checked in `0x7FF6D1CB3B40`, plus UX var `0xD8000000000656B`.
  - `--PodId=1` applies (`0x7FF6D41783A8` reads 1).
  - `--PartyId=1` doesn't: the hidden field resets it to 0. Tab from the POD field doesn't reach it.
- Stopped there (time box). The config file was removed afterwards. Next step: an ASI that sets `PartyId` before START, or find the UX binding that fills the hidden field.

### 4. Assault Test `0x0230000000000067`

- Launched with `--map Hanamura --mode 0x0230000000000067`, then Practice vs AI > Medium.
- Hero select reads **"DEFEND · VERSUS AI MEDIUM · HANAMURA"** with the assemble timer. The map loads, the player spawns, and bots play. `Overwatch.log` has no errors.
- No objective HUD and no setup timer. The vendored server only runs the objective flow for `OBJECTIVE_MODES` (Assault/Escort/Hybrid, graph `0F4A`), so this mode plays as a plain team fight.
- Next step: serve graph `29A6` as the mode graph with its own phase states. Adding `0x67` to `OBJECTIVE_MODES` alone would replay Assault's flow under a new name.

### 5. VPP Green Room `0x06C7`

- The map table has no entry, so `inject_map.py` added one: Skirmish, no placeables, spawns at the origin.
- `map_collision.py` fails: chunk `0x0DD00011000006C7` is missing from the install.
- Without a collision mesh, the match runs on the server: loaded (21601), assemble, heroes assembled. The client shows a loading screen ("VERSUS AI · MEDIUM · VPP GREEN ROOM", default sky) and **never leaves it** (over 2 minutes). Nothing in `Overwatch.log`.
- **Not feasible**: the map data is stripped (2 of 20 chunks ship). Drop it from the list.

## Sources

- Tracer pose history: [Know Your Meme](https://knowyourmeme.com/memes/events/tracers-pose-controversy), [TechRaptor](https://techraptor.net/gaming/news/blizzard-replaces-tracers-over-shoulder-pose-in-new-overwatch-update)
- All-Star skin releases: [Newsweek](https://www.newsweek.com/overwatch-league-all-star-skins-leak-reddit-mercy-lucio-atlantic-pacific-1400598), [esports.gg](https://esports.gg/news/overwatch/how-to-get-overwatch-2-all-stars-skins-atlantic-and-pacific/)
- Dev login reports: [Blizzard forums](https://us.forums.blizzard.com/en/overwatch/t/what-is-dev-login/405304)
- Field names: local OWLib 1.74 (`DataTool/DataModels/Unlock.cs`: `AvailableIn` = `m_53145FAF`, esports team = `m_0B1BA7C1`)
