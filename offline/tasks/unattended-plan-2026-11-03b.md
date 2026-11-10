# Unattended plan (2026-11-03, third run)

Follows `research/run-summary-2026-11-03b.md`. Same rules as `tasks/unattended-plan-2026-11-03.md`:
- No game-file patches, DLL injection or process writes; memory reads are OK.
- One local commit per verified step; never push; no attribution lines.
- TEMP probes reverted before each commit; kill Overwatch.exe after each run.
- Background input only (`gameinput.py bg-*`); no firewall prompts; no hardware breakpoints.
- A step that hits its time box is written up and left; the run moves on.
- Lifted in part (user, 2026-11-03): **ASI plugins are allowed** when a fix needs client code changes. Server-side first; game data files stay untouched.

## Order

| # | Step | Box | Done when |
|---|---|---|---|
| 0 | **Background scan** (starts first, runs while 1-2 go on): which statescript graphs reference the "Victory"/"Defeat"/"DEFEAT" strings (`0x0DE…13C6`, `13C7`, `0DC3`, `1685`, `259B`, `9305`, `C082`, `C083`, `C530`) and "Difficulty"/"EASY" (`6FB3`, `22E3`, `2DA2`, `2DD6`). `.scratchpad/pva/menu_refs.py`, ~45 min. | - | output saved to `research/` |
| 1 | **Difficulty in game.** Log what the client sends when the player types in match chat (lobby RX and the game server's reliable messages). If the text reaches the server, a chat line `easy`, `medium` or `hard` sets `Match.difficulty` for the bots from then on. | 2 h | in game: typing `hard` changes the bots' damage per hit (10 → 19 for Soldier bots), logged; or a write-up of where chat goes |
| 2 | **Tournament card padlock.** Write `tools/research/dump_lobby_schemas.py`: read-only memory read of the running client's lobby protocol descriptors (CRC, first/last message id) and field layouts, checked against the one known schema (`074DAD18` in `vendor/data/schemas_174.json`). Dump service `0x692C511B`; decode the card's 26-byte request; look for a server-to-client message that announces a tournament match and send it at login. | 3 h | the card shows without the padlock, or the dump and the messages tried are written up in `research/play-menu.md` |
| 3 | **Victory/defeat screen.** From step 0: the graph that draws the banner, how it starts (SubScript of 0F4A/0C35, own entity, game message), and send it at match end before the 20304 return to menu. | 3 h | after a forced win and a forced loss, the screen shows Victory / Defeat |
| 4 | **Round-complete booleans.** 0C35 presence bits 8 (v7967), 0 (v8002), 1 (v8003) as Bool, one at a time, looking for the round number and time remaining. | 1 h | values found and sent, or listed as tried |
| 5 | **Helix retest.** Re-add the bot Helix sender (`research/bot-fire.md`, "Ability Volleys") now that fighting bots face their target; one Soldier bot standing still 8 m in front of the player, placed on a nav node (the ring placement put bots in walls). | 1.5 h | a rocket flies at the player and is committed, or the result is written up |
| 6 | **Capture HUD arrays.** Objective-entity v18109/v16927 as arrays naming, in turn: the objective entity itself, the player, the mode entity with v18108 = 0/1; values 11947.. set on the named entity. | 2 h | a padlock changes, or the tries are written up |
| 7 | **Zarya reader.** In IDA, find how a bot entity lists its script instances, and read Zarya's 0241 instance through it (the current scan misses idle remote instances). Then add her beam slots back one at a time. | 2 h | the instance's active states are read, and the beam shows or the slot that breaks it is known |
| 8 | **Cassidy, code only** (scope doc steps 1-2): a generic weapon on `Player` driven by `HERO_KIT` (cadence, magazine, reload), Soldier unchanged; Cassidy's weapon instance built from `WEAPON_GRAPHS`. Unit-tested; the pick gate stays. | 2 h | tests pass, Soldier plays as before in one live run |
| 9 | **Close-out.** Full tests, no TEMP probes, game closed; `research/run-summary-2026-11-03c.md`; tasks and CHANGELOGs; commit. | 0.5 h | - |

| 10 | **All 1.74 heroes in the menus (research)**, after step 9: what makes the Hero Gallery / hero select / main-menu background show every hero with skins, emotes and animations (login records 20505 hero list, unlock/inventory records, the main menu's random hero). Start with what the login records send today. | open | `research/hero-menu.md`: what controls each, and a plan |

Total ≈ 17 h of boxes; steps 6-8 are dropped first if time runs short.

## Why this order

- 1 and 2 finish the user's Play-menu request (difficulty from the game, no locked look).
- 3 and 4 finish the round-end screen.
- 5-7 are the open drawing problems; 8 starts the next roadmap item with no game risk.

## Risks

- Step 1: in tournament mode chat may go nowhere (no chat service answered). Then the fallback is a write-up.
- Step 2: the descriptor layout in memory is not known yet; the box may go to finding it.
- Step 3: the banner may be in a graph only a retail server starts with data we don't have.
- Step 0 ties up one CPU core for ~45 min; game runs still work alongside (seen this run).

## Progress

- Step 0: done. "Victory"/"Defeat"/difficulty strings: only `0x027F` (-> `0DC3`) and `0x0BE9` (-> `2DD6`) reference any (`.scratchpad/victory-scan.txt`).
- Step 1: done by another route (ASI, allowed mid-run): the retail menu's Training > Practice vs. AI > EASY / MEDIUM / HARD sets the difficulty before the match (`research/play-menu.md`, "Retail menu via ASI"). Chat is not needed.
- Step 2: superseded for the retail menu (no Tournament card there); still open for the plain tournament route.
- Step 4: done (write-up). Bit 8 = "SWITCHING SIDES"; bits 0, 1 not the round number or time (`research/assault-mode-graph.md`).
- Step 3: partial: banner graph `0x027F` found and decoded (`research/end-of-match.md`); its starter is not found yet.
- Steps 5-7: not started this run; carried over. Step 8: scope step 1 done (generic `Rifle`).
- Step 9: done (`research/run-summary-2026-11-03c.md`). Step 10 started (`research/hero-menu.md`).
