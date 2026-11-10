# LizRollin's lobby side vs ours (read-only comparison, started 2026-11-09)

Theirs = `lizrollin-lobby-server/ow174/{lobby,content,services}` at the pinned commit `9f8f3bc` (about 6,300 lines).
Ours = `tools/lobby/lobby.py` and `practice.py` (about 930 lines).
Nothing of theirs was run. A grep of their lobby, content, services, accounts and catalog folders, docs, WIP.md and both READMEs for text aimed at an AI found none.
The game side is in [lizrollin-runtime.md](lizrollin-runtime.md).

## 1. Login sequence, 2026-11-09

Sources: their `lobby/handlers/login.py`, `content/__init__.py` (`login_messages`), `content/retail.py` (`AT_LOGIN`) and `data/retail_templates.json`; our `lobby.login_records` and the `extra` list in `lobby.handle`.

- **How they build it.** Most of their login is a replay of a retail 1.68 capture (`retail_templates.json`: 70 recorded server messages, the account name anonymized). They swap in the player's ids and name, rebuild the parts they understand, and send the rest as recorded.
- **Ours** is built by hand, message by message. Without `practice` it is only 20500, 20502, 20504 and 20505.
- **Layouts.** Our vendored `schemas_174.json` holds 155 of their 707 message layouts, identical where both have them. A new message needs its layout rows copied from their file (same pin, MIT), as the earlier groups were.

### Both send

| Message | What | Difference |
| --- | --- | --- |
| 20500 | hello | Theirs fills a session id, the country and the game region. Ours sends zeros there. |
| 20502 | player card | Same fields, plus a timestamp (`+0x30`) in theirs. |
| 20504 | content keys | Same. |
| 20505 | loadable heroes, skins, maps | Theirs is the capture's list plus extra skins. Ours lists what we serve. |
| 38900 | events | Same. |
| 20700 | party | Same shape. Theirs adds ratings and an endorsement per member. |
| 24300 | account progression | Same. |
| 24900 | hero catalogue | Same. |
| 36600 | dynamic config | Theirs is retail's whole list (about 135 keys) plus our build and the menu hero. Ours sends 2 or 3 keys. |
| 20813 | leaver status | Same values. |
| 20802 | saved settings | Ours since 2026-11-09 (the kill feed fix). |
| 39802, 39810, 39804 | Arcade cards, dates, how busy | Same set. |

### Only theirs, built for the player

| Message | What | Use in single player |
| --- | --- | --- |
| 38902 | a hero challenge's win counter | Only with their challenges. |
| 52002, 52005, 52006 | endorsement level | Shown on the player card. |
| 27113 | own presence; offline friends again every 60 s | Social only. |
| 27202 | stat catalogue per hero and map (166 KB recorded) plus career stats | Career Profile statistics. Our profile has none. |
| 20812 | interface states: dialogs seen, "don't show again" | Unknown. A missing state reads as 0. |
| 58501 | priority passes | Role queue only. |
| 36302, 36300 | competitive ratings and seasons (92 KB recorded) | The Competitive card's rank. |
| 35500 | an empty data-centre list | Lets the group finder search. |
| 27100, 20809, 20402 | friends, their cards, the general chat channel | Social only. |
| 55500 | account features, 3 s and 7 s after login | Feature `0x948D` allows chat. Their note: the client drops the connection if the list is incomplete or the value is 1. |

### Only theirs, replayed as recorded

| Message | Recorded content |
| --- | --- |
| LOBBY 20814, 20806, 20805 | 4 bools; 1 bool; two numbers and a bool. Unidentified. |
| LOBBY 20821 | empty list |
| LOBBY 20820 | two messages, 118 KB together. Unidentified. |
| `AA91BE18` 40900, 40914 | empty lists |
| 43200 | Overwatch League live matches: none |
| 39800, 39801, 39803, 39806, 39807 | Arcade extras: a counter, three entries for one card, two card ids, two empty lists |
| `267FDE9E` 28001 | 154 ids. Unidentified. |
| `0DFEEFEF` 38300 | empty list |
| 36603, 36602 | two empty lists; the server clock |
| 51804, 51807, 51810, 51817, 51812 | replays: 9 entries, then empty lists |
| `34BB385D` 43301 | three numbers. Unidentified. |
| 26404, 26400 | store: empty; 11 real-money products ("Overwatch - 2 Loot Boxes", ...) |
| 27104 | news: a 6 KB blob |
| 23301 | 70 custom-game maps and 1 training map |

### Reading

- Nothing here is known to be broken in our menus for lack of it, except Career Profile statistics (27202). Every "use" above is read from their comments and the recorded data, not checked live.
- The pieces most likely to show in single player: 27202 (statistics), 36300 with 36302 (the Competitive card), 23301 (Create Game's map list), 51804 (the Replays tab), 36602 (event and Arcade timers) and 20812 (first-time dialogs).
- The rest is social (friends, chat, presence, endorsements, passes, group finder) or unidentified.
- **To study (asked 2026-11-09):** how to get the Career Profile fully working offline. It is listed in STATUS.md under Menus.
- 20802 was the one message of this set with a proven effect in a match (section 16 of the runtime notes). Its save side (22200-22204) is the next bullet.

## 2. Saved settings, 2026-11-09

Sources: their `lobby/handlers/settings.py` and `content/player.py` (`settings`).

- **Theirs.** 22200-22203 each save one block of 20802 (`+0x78`, `+0x108`, `+0x10D`, `+0x120`). 22204 saves only the changed entries of the key/value list `+0x130`, matched by key (`+0x8`). All of it goes into the profile and comes back in 20802 at login. They also keep the interface states (22207, back in 20812) and the season intros seen (36200).
- **Ours before:** nothing was kept, so every login sent the defaults.
- **Ported:** `PracticeRange.save_settings` and `settings` (`tools/lobby/practice.py`), stored in `tools/lobby/data/settings.json`. Not taken: 22207 and 36200, which belong to messages we do not send.
- **Seen live:**
  - The client sends its saves when Options closes, not per click: 22200 and 22203 together for a Gameplay toggle.
  - Block `+0x120` is Options > Gameplay: `+0x4` Kill Feed Display, `+0x5` Always Skip Kill Cam, `+0x7` Tips Display. These are system 9's `+700`, `+701` and `+703`, the Options UI vars `0x6426`, `0x6047` and `0x62BC`.
  - Always Skip Kill Cam set to On came back On at the next launch.
- **Not checked live:** 22201, 22202 and 22204 (covered by the unit test only). Block `+0x78` (six arrays) came back empty in our saves, so what fills it is unknown.

### The client's own settings file

Video and sound settings are not in 20802. The client keeps them in `Settings_v0.ini`, in the folder Overwatch 2 uses for its own file of the same name.

- **Where the path comes from** (IDA):
  - `0x7FF6D2A00620` (system init) calls `SHGetFolderPathW` with `CSIDL_PERSONAL` (5) and writes `<Documents>\Overwatch` into the global at image `+0x32371E8` (`0x7FF6D4450410 + 28120`, 272 bytes).
  - `0x7FF6D2A304C0` builds each settings path from it as `%s/Settings/%s.ini`, with the name `Settings_v0` (or `ShowMode_Settings_v0`). A second format, `%s/ReplaySettings/%s.ini`, is used while `byte_7FF6D449A6CB` is set.
  - The same folder holds `Settings\GUI_Settings2.ini` (`0x7FF6D2AE3840`), `Cache/Shaders/`, logs and screenshots.
  - No command-line option moves it. The options found are `tank_logpath`, `tank_userlogpath`, `tank_CachePath`, `tank_DataPath` and `tank_DataRoot`.
- **The redirect.** The registry holds the Documents folder as `%USERPROFILE%\Documents`, and Windows expands it with the asking process's own environment. A test call of `SHGetFolderPathW` under a changed `USERPROFILE` returned the new folder once it existed, and "path not found" before. So `run.game_env` starts the game with `USERPROFILE` set to `<game-root>\OfflineProfile`, and the client's folder becomes `<game-root>\OfflineProfile\Documents\Overwatch`. No client patch is involved.
- **Live:** the game booted to the menu from the new folder in the same window, rewrote `Settings_v0.ini` there at exit, and left the file in Documents untouched (same timestamp, and the two files matched line for line after the first run's copy).
- **Limits:**
  - Everything else that reads `USERPROFILE` in the game's process moves too. Seen: the graphics driver's shader cache (`OfflineProfile\appdata\local\NVIDIA\DXCache`).
  - The file sits three folders deep, not directly in the game folder. A path such as `<game-root>\Settings` would need the client's global patched before its first read, which means loading a plugin before the game's init. The launcher loads plugins after the game starts.
  - A Documents folder moved off `%USERPROFILE%` (an absolute path in the registry) is not redirected; the launcher prints a note.

### To try later: the file directly in the game folder

Goal: `<game-root>\Settings\Settings_v0.ini`, with no `OfflineProfile\Documents\Overwatch` in between. Nothing below has been tried. Cheapest first:

1. **A folder link, no client patch.** Keep the `USERPROFILE` redirect, but make `...\Documents\Overwatch` a directory junction to the folder that should hold `Settings`.
   - Pointing it at `<game-root>` itself gives the exact path. The profile folder must then live outside the game folder (for example under `tools/lobby/data/`), or the link makes a loop inside the install.
   - Logs, `ScreenShots` and `Cache` would land in the game folder's root too.
   - To check: that the client follows a junction for its reads and its exit write, and that the Battle.net launcher's scan leaves the extra folders alone.
2. **Patch the client's folder global** (image `+0x32371E8`, 272 bytes, written once by `0x7FF6D2A00620`) to `<game-root>` before the first settings read.
   - The launcher injects plugins after the game has started, so first find when the first read happens: the callers of `0x7FF6D2A304C0` are `0x7FF6D2A2FF30`, `0x7FF6D2A2FFB0` and `0x7FF6D2A30030`. If the read comes before a plugin can load, the game reads the Documents file first and only later writes go to the new place.
   - Starting the game suspended (`CREATE_SUSPENDED`), loading the plugin, then resuming would close that gap. To check: that the packed client and `load_asi.py` both tolerate a suspended start.
3. **Hook `SHGetFolderPathW`** from a plugin loaded before init (same suspended start). The client resolves it by name from `Shell32.dll` at first use (`0x7FF6D12F25D0` shows the pattern), so the hook has to sit on the function itself, not on an import table. More code than 2 for the same result.

Whichever is used, `run.game_env` already seeds the new file from the old one, and the folder name is one constant (`USER_FOLDER`).

## 3. Career profile, leaderboard and ranked, 2026-11-09

Sources: their `content/career.py`, `content/ranked.py`, `content/leaderboard.py` and the recorded 27202; our `PracticeRange.career_profile`.

### What they do

- **Profile answer** (20807 to request 22206): account record, hero records, the competitive cards' ratings (`+0x98`), the stats twice (`+0xB0`, `+0xC8`), an endorsement level (`+0xE0`) and the name. Plus a summary 39002. Ours sends the record, the heroes and the name.
- **Stats are made up.** 500 hours on the menu hero, 50 down to 20 on four more, and the season's role-queue wins. Nothing is counted from matches.
- **A stat entry** is `{stat id, f64 value}`; a hero's entry is `{[stats], hero id}`; a category is `{[heroes], key}`. Stat ids are the game's `062` assets (GUID `0x0860...`): `0x21` Time Played in seconds, `0x39` Matches Won, and the scoreboard's `0x25` Eliminations, `0x29` Deaths, `0x1D1` Healing, `0x4B8` Hero Damage, `0x326` Objective Kills, `0x327` Objective Time.
- **27202 at login** carries the same stats (`+0x78`, `+0xA8`) and, replayed from the retail capture, the stat catalogue: `+0x90` lists 192 to 231 stat ids for each of the 32 heroes, 167 for "All Heroes" (hero id `0x02E00000FFFFFFFF`) and 3 for each role; `+0xC0` lists stats for 130 maps (14 for the first). 1,055 stat ids in all, 66 KB on the wire.
- **Ranked** (36300, 36302) is the capture's season 32 with the profile's ratings put in. **Leaderboard** (39000, answering 39100) is a Top 500 page built from their accounts. Neither has a use without other players.

### Experiments (`tools/research/career_live.py`)

The lobby sent 27202 and filled 20807's stats for All Heroes and Soldier: 76. Each value was the stat's place in its list plus 1000 times the category key.

| Sent | Overview | Statistics |
| --- | --- | --- |
| nothing (as today) | "Career profile is empty" | zeros, "No games played", mode list: Quick Play |
| stats in category 0 only, with the catalogue | not looked at | zeros, mode list: Quick Play |
| stats in categories 0, 1, 2, 5, with the catalogue | filled | filled |
| the same stats, no 27202 | empty | zeros, mode list: All Modes |
| the same stats, 27202 with an empty catalogue | empty | zeros, mode list: All Modes |

- **The catalogue is required.** The client shows a hero's stat only when 27202 `+0x90` lists it for that hero. Stats alone, in 20807 or in 27202, show nothing.
- **Filled Overview:** time played per mode (Game Browser 1 hr, Quick Play 17 min, Arcade, Competitive and Experimental 0), the most played heroes with Soldier: 76, and the hero comparison list. Frame: `lizrollin-lobby/career-profile-fed-2026-11-09.png`.
- **Filled Statistics:** the six highlights (Time Played, Games Played, Games Won, Time Spent on Fire, Objective Time, Kill Streak - Best) and a table with Total, Best and Avg/10 min per stat (Eliminations, Final Blows, Solo Kills, Healing Done, ...).
- **Category keys** (`0x7FF6D1662770`): the low byte is the kind, bits 8-15 a sub-kind, and for kinds 3 and 4 an id from bit 16.
  - 1 is Quick Play (1,000 s showed as 17 min) and 5 is Game Browser (5,000 s as 1 hr).
  - 3 is a competitive season (`season << 16`, as their notes say), and 4 takes a 16-bit id, likely an Arcade card.
  - 0 and 2 showed under no mode row but counted toward All Modes, which adds the categories up (8,052 games = 13 in each of four categories plus 8,000).
- **Positions seen in Soldier's list:** 2 is `0x21` Time Played, 13 is `0x38` Games Played, 14 is `0x39` Games Won.
- **Not done:** which message's stats the client reads (both carried them).

### Stat ids (`CAREER_VALUES=id`: each stat's value is its id, one category)

The client prints a stat's name itself, so a row names its id. All Heroes and a hero share no id: All Heroes' 167 are a set of their own, and the 32 heroes have 177 in common.

| Stat | A hero | Best game | All Heroes | Best game |
| --- | --- | --- | --- | --- |
| Time Played (s) | `0x21` | | `0x26` | |
| Games Played | `0x38` | | `0x385` | |
| Games Won | `0x39` | | `0x3F5` | |
| Games Lost | `0x430` | | `0x42E` | |
| Eliminations | `0x1F` | `0x2DA` | `0x25` | `0x1B9` |
| Final Blows | `0x2B` | `0x2DB` | `0x2C` | `0x1E2` |
| Solo Kills | `0x2D` | `0x36C` | `0x2E` | `0x36B` |
| Deaths | `0x2A` | | `0x29` | |
| All Damage Done | `0x1BF` | `0x2A4` | `0xC9` | `0x1E3` |
| Hero Damage Done | `0x4B7` | `0x4BB` | `0x4B8` | `0x4BA` |
| Healing Done | `0x1C1` | `0x2A5` | `0x1D1` | `0x1E4` |
| Objective Kills | `0x31C` | `0x31E` | | |
| Best Kill Streak | `0x223` | | `0x497` (table), `0x472` (highlight) | |
| Melee Final Blows | `0x37F` | | `0x381` | |

- Also seen for a hero: Win Percentage `0x3D1` (a fraction), Eliminations per Life `0x3D2`, Barrier Damage Done `0x515`, Environmental Kills `0x362`, Multikills `0x345`. All Heroes' Time Played was first read as the one unclaimed id under 60 in its list (38 s showed as "<1 min"); the live run with counted time confirmed it.
- The Avg/10 min column is a stat of its own, shown times 600 (Eliminations: `0x4D4` for a hero, `0x4B0` for All Heroes).

### Built, 2026-11-09

- **`tools/lobby/career.py`:** the counters, their file (`data/career.json`), the categories for the messages, and a catalogue of exactly the stats counted, for the 32 heroes and All Heroes. A short catalogue of our own is enough: the recorded one is not needed.
- **Counted in the match** (`Player.count` in the vendored `match.py`): time on the hero, eliminations and final blows, deaths, all damage and hero damage, and the best kill streak. The lobby adds games played, won and lost when a match ends, and each total's best game.
- **Sent:** 27202 at login, and again with the profile answer 20807, so a match played since the login shows.
- **Which matches count:** every match except the Practice Range, all under Quick Play (key 1). A match left early counts its numbers but no game.
- **Live:** 90 s in a Practice vs. AI match on Hanamura as Soldier: 76, then Leave Game. Without a relaunch the Overview showed Quick Play 1 min and Soldier: 76 as most played, and Statistics showed Time Played 1 min. Frame: `lizrollin-lobby/career-profile-real-2026-11-09.png`.
- **Not checked live:** a kill's rows with counted numbers, and Games Played and Won, which need a match played to its end. Both are covered by the unit tests, and their ids by the table above.
- **Left out:** healing (the match's heals do not say who healed), solo kills, objective time and kills, time on fire, the Avg/10 min column, ratings, endorsements and replays. The vs-AI and PvE cards have no category key of their own yet.

## 4. Gallery, loot boxes and shop, 2026-11-09

Sources: their `services/lootbox.py`, `services/shop.py`, `lobby/handlers/gallery.py`, `content/collection.py` and `catalog/boxes.py`; our `progression.py`, `practice.progression_request` and `tools/assets/progression_catalog.py` (research/progression/).

### Side by side

| | Theirs | Ours (`--progression`) |
| --- | --- | --- |
| Where items, pools and prices come from | The retail 1.68 capture's hero catalogue (5,968 store entries: price, event) plus an extracted unlock list | The 1.74 install itself (6,603 items: rarity, slot, box pools) |
| Levels and XP | None: the dashboard sets the level | XP per finished Assault match, level-ups, frames by level |
| How boxes are got | 50 standard and 10 of most other types at the start, refilled when under 5 | One standard box per level |
| Box types | 11: standard, six event boxes, Golden, Legendary, Legendary Anniversary, Wrecking Ball's | Standard only |
| Rarity odds | Per slot 1.93% Legendary, 4.98% Epic, 10% Rare; one slot Rare or better | Was 75:20:4:1 with the first slot Rare or better; **now theirs** |
| Duplicates | 5 / 15 / 50 / 200 credits | The same |
| Credits in a box | 50 / 150 / 500, in a fixed share of slots (5%, 5%, 50%: their guess) | The same three currency items, drawn like any item of their rarity |
| Prefers items not owned | Yes | Yes |
| Shop currencies | Credits, league tokens (OWL skins), competitive points (golden weapons) | Credits |
| Team skin pairs | An OWL skin comes with its home or away partner | None |
| Durability | A profile file saved by the caller | SQLite with receipts: a match or a box counted once, replayed on a retry |
| Messages | 24201 → 24305, 24203 → 24306, 24901 / 24301, 24302, 24500, 24501 | The same set |

### Where the two data sets agree

- **Box pools match exactly.** Every one of their 5,968 store entries is in our catalogue, and their event number equals our box type for each: 1,953 base items, 280 to 324 per event (1 to 6), one item in type 8, and 2,241 that never drop.
- **Base prices match exactly.** All 1,829 base items priced in both have the same price. Ours come from the rule 25 / 75 / 250 / 1000 by rarity.
- Our 635 extra items are mostly account-wide (303 frames and the like, 215 icons, 90 sprays), which their hero catalogue does not list, plus 26 skins and a victory pose their capture lacks.

### What ours cannot get

| Items | Count | Why |
| --- | --- | --- |
| Event cosmetics (events 1 to 6) | 1,773 | No event boxes are awarded, and we give them no price. |
| OWL skins | 1,530 at 100 league tokens (17 more never-dropping items cost 200) | No league tokens. |
| Golden weapons | 32 (3,000 competitive points) | No competitive points. |

- Their event prices are not just the rarity rule: 1,372 of 1,530 follow it, and 158 cost three times as much (75 / 225 / 750 / 3000), the newest items of an event. So pricing event items needs their recorded prices or a rule for which items are new.

### Ported

- **The rarity odds** (`progression.SLOT_ODDS`): 83.09% Common, 10% Rare, 4.98% Epic, 1.93% Legendary per slot, with one slot Rare or better. Their source gives these as the per-box figures of the game's own odds screen: an Epic in 18.5% of boxes and a Legendary in 7.5%. Ours gave an Epic in about 26%.
- The Rare-or-better item is no longer always the first of the four shown.
- Not checked live: only the weights changed, and the box opening itself was verified on 2026-11-08.

### Open, for a decision

1. Event cosmetics: sell them for credits (at which prices, and always or only with `--event`), award event boxes while an event menu runs, or both.
2. Golden weapons and OWL skins: add the two currencies and a way to earn them offline, or sell them for credits.
3. More box types (Golden, Legendary, a hero's box) as rewards.

## 5. Arcade and events, 2026-11-09

Sources: their `content/arcade.py`, `content/celebrations.py` and `catalog/events.py`; our `PracticeRange.events` and `arcade`, `MENU_EVENTS`, `ARCADE_GROUPS` and `PVE_CARDS`. Our side was built from theirs earlier (the card catalogue, the group lists and the event presets are upstream's).

### Side by side

| | Theirs | Ours |
| --- | --- | --- |
| Events at once | Any number, per profile | One, picked with `--event` |
| Presets | goodbye, lunar, halloween, winter, anniversary and its remix 1 and 2, summer, archives, reaper, cassidy, malevento, owl, watchpoint, ow2_credits | farewell, lunar, halloween, winter, anniversary, reaper, cassidy, malevento, owl |
| Event record 38900 | Adds map swaps (`+0x0`), login rewards (`+0x58`) and the event's loot box type (`+0xB0`) | Scene, content key and type only; no box (`-1`) |
| Event greeting 38901 | A splash or "What's new", one event box and any reward item, once per event | None |
| Hero challenges | A 38900 record, a win counter (38902) and rewards at set win counts | None |
| Arcade cards | Follow the events (Lunar brawls, Winter brawls, Junkenstein, Lucioball, Archives missions), then two fixed dailies, twelve rotating dailies, and Quick Play Classic and Mystery Heroes | Always Junkenstein, Archives and the challenge missions, then four everyday cards |
| What a card starts | A match for the modes their server runs; no PvE | Uprising (Story) loads and plays on its map; the other cards only show |

- The pieces of theirs we lack all lead to things we cannot run or have not decided: the seasonal brawls need mode scripts, the event box and the greeting belong to the loot box decisions of section 4, and hero challenges are a reward track of their own.

### Experiment: event map variants

Their 38900 record carries map swaps: for a celebration type, pairs of a map and its event variant (from their `extracted_maps.json`: Winter has King's Row, Hanamura, Ecopoint, Black Forest and Blizzard World; Halloween has Hollywood, Eichenwalde and Château Guillard; Lunar has Lijiang Tower and its three stages).

1. **The swap list alone changes nothing.** With `--event winter`, the five Winter swaps in 38900 and the variants' headers in the 20505 preload list, a Practice vs. AI match on Hanamura loaded the ordinary map (no snow on the street outside the attackers' spawn). The server names the map, and the client loads that one.
2. **Hosting the variant works.** `--map 0x08000000000006B5` (Hanamura (Winter)) extracted its collision and loaded the snowy map with the arcade's string lights. The same walk from the spawn reached the same street, so the layout matches. Frame: `lizrollin-lobby/hanamura-winter-2026-11-09.png`.

**Ported from that:** `practice.pick_map(name, mode, event)` picks a map's event variant by the catalogue's own names ("Hanamura (Winter)", "Hollywood (Halloween)", "Lijiang Tower (Lunar)") when the event runs and the mode can be hosted there. So `--event winter --map Hanamura` hosts Hanamura (Winter). The pick is covered by a test; the automatic pick itself was not run live, only the variant by its GUID.

Also seen in these runs: bots killing bots draw kill feed lines ("SOMBRA → ZENYATTA", "ZARYA → MEI"), which section 16 of the runtime notes had left unchecked.
