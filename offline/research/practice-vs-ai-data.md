# Practice vs. AI: what the 1.74 data says (build 104319, 2026-11-02)

Method: all assets read through the CKey-verified CMF/bundle path (`tools/assets`), en-US strings from `Manifest/Win_SPWin_RDEV_LenUS_text_EExt.cmf`. Class/field names from `tools/assets/stu_names.json` (OWLib; the hashes used here were confirmed present in 1.74 data). Scratch scripts: `.scratchpad/pva/` (`pvalib.py` = bulk loader; `t57.py` AI table, `t44.py` map census). Nothing written to the game. No hostile/injected text was met in any file read.

## 1. Game mode / queue

- **There is no dedicated "Practice vs AI" STUGameMode.** The 25 STUGameMode assets (type `0x0C5`, GUIDs `0x0230000000000003..70`) are: Assault `..14`, Escort `..15`, Hybrid `..16`, Control `..17`, Skirmish `..10`, Elimination `..09`, CTF `..07`, TDM `..1D`, DM `..1E`, Lúcioball `..20`, Practice Range `..18`/`..5A`, Tutorial `..19`, PvE (Junkenstein `..03/2A`, Retribution `..25`, Storm Rising `..43`, Uprising unnamed `..0F/1A`), others. The English strings exist ("Vs. AI", "PRACTICE VS. AI") but belong to the **play-menu UI graphs** only: `0x580000000000B7F` (card text "PRACTICE VS. AI" `0xDE00000000042A0`, "Hone your skill by playing against AI-controlled heroes." `..42A1`) which embeds `0x580000000000BE9` (queue card "Vs. AI", buttons EASY/MEDIUM/HARD with the "Play against easy/medium/hard difficulty AI-controlled heroes." strings `..2DD3..2DDE`; it calls sub-graph `0x5800000000002240`). **Inference:** Practice vs AI = a normal objective mode (Assault/Escort/Hybrid/Control) with AI controllers filling slots; the AI/difficulty choice is lobby-side, not a mode asset. Not provable from data alone.
- **Team size:** Assault/Escort/Hybrid/Control/Skirmish/TDM: 2 teams, `170AA4B8`=6 (team size), `8B3CD15B`=12 (max incl. extra/spectator, guess). Hero restrictions: `59C86C8D` = instance index of `STU_C45DE560.m_heroes` (empty = all).
- **Mode graphs:** Control `0x580000000000EC2`; Assault, Escort and Hybrid all use `0x580000000000F4A`; shared team body/controller scripts `0xC8E` / `0xC90`.
- **Settings schemas (custom game):** Control mode refs `0xA3000000000000C` (Control), `..06` (General), `..08` (Heroes); Assault `..09`, Escort `..0B`, Hybrid `..0A`. Difficulty setting in PvE schema `0xA30000000000037`: options 0x0D8000000000195E "Normal" (default), `..195F` "Hard", `..26DF` "Expert", `..1960` "Legendary".
- **Difficulty:** see sec. 2. The "ADD AI" / "Edit AI" / "Hero Roster" strings (`0xDE..1B6A, 23A3, 5AE1, 5983`) are not referenced by any STU asset except `Hero Roster` in graph `0x580000000000DFF`: the custom-game AI-slot UI is native/other-format, **not answerable from STU data**.
- **Map eligibility:** headers (`0x079...`, type `0x09F`) have `m_supportedGameModes` (`D608E9F3`, 16-byte-element array) but no AI flag. Best proxy: maps whose entity chunk contains AI hint volumes (`STUECAIHintVolumeInstanceData` `298FC27F`) and a nav chunk `0x12` (sec. 3). Present: Assault (Temple `0x5B`, Hanamura `0x165`, Volskaya `0x1DB`, Horizon `0x6D3`, Paris `0x891`), Hybrid (King's Row `0xD4`, Numbani `0x1D4`, Hollywood `0x2AF`, Eichenwalde `0x68D`, Blizzard World `0x75E`, Paris `0xAF0`), Control (Nepal `0x4B7`, Lijiang `0x662`, Ilios `0x66D`, Oasis `0x69E`, Busan `0x7E2`) plus their Elimination/CTF sub-maps. **Escort maps Gibraltar `0x184`, Dorado `0x2C3`, Route 66 `0x5BB`, Junkertown `0x756` have zero hint volumes** (Rialto `0x79F` and Havana `0xAFE` differ: 10 / 0), so they look AI-unready (inference).

## 2. AI definition

Hero body entity (`STUHero.m_gameplayEntity`) -> field `04A8896C` (playable) / `8B9A461F` (PvE & Training bots) -> a small **AI entity** with `STUAIBehaviorComponent` (`8FE674F9`), `STUStatescriptComponent`, `STU_993B1B10` (weapon fire info) and a `STUTargetTagComponent`. Found by scanning all 3,509 parseable entity assets (51 have the component).

`STUAIBehaviorComponent` fields (names via OWLib, mostly unnamed): seven `STUGameModeSpecificBehaviorScripts` slots `099695F2, 3B8A79C3, 4A10B1EF, 7D218322, 853DD825, 9325CAD8, B6DCC63B` (each = 2-3 statescript graphs + a schema), and `m_difficultyProfiles` `97259BF6` (`STUAIDifficultyProfile` `CB198A7C`: identifier `13E9E420` + ref `AFEEA472`). The 7 slots look keyed by game-mode kind; **slot to mode mapping is unresolved** (needs client reader code or graph content).

Difficulty profiles: three per hero, always in order [`0xD..195F` Hard, `0xD..195E` Normal, `0xD..1960` Legendary], each pointing at an AI parameter asset (type `0x0AE`, `0x0B5000000000nnn`, ~36 identifier -> float entries). Soldier: `0xB5..04/05/06`. Value ramp (aim/reaction-like keys, e.g. `..1995`: 8 / 20 / 40, `..1CD0`: 20 / 40 / 250) shows set `05` is easiest, `04` middle, `06` hardest. **Guess:** UI Easy/Medium/Hard = `195E`/`195F`/`1960`.

| AI entity | Heroes using it | Param sets (diff ids 195F/195E/1960) |
|---|---|---|
| `0x400000000001853` (generic) | Tracer, Widowmaker, Reinhardt, Pharah, Hanzo, Winston, Genji, D.Va, Orisa, Symmetra, Wrecking Ball, Ashe, Sigma | `B5..04/05/06` |
| `0x4000000000011F4` | Soldier: 76 | `B5..04/05/06` |
| `0x40000000000118E` | Reaper, Moira, Echo | `B5..07/08/09` |
| `0x400000000000D57` | Bastion | `02/03/0A` |
| `0x400000000001468` / `147E` / `14BA` | Zenyatta / Roadhog / Zarya | `0B-0D` / `0E-10` / `11-13` |
| `0x4000000000015F0` / `160D` / `1635` | Cassidy / Ana+Baptiste / Lucio | `14-16` / `17-19` / `1A-1C` |
| `0x40000000000195B` / `195C` / `1667` | Mercy / Junkrat / Torbjorn | `23-25` / `20-22` / `1D-1F` |
| `0x400000000001868` / `178D` | Mei / Sombra | `33-35` / `30-32` |

**Doomfist and Brigitte have no AI entity.** The 15 playable-hero AI entities pull in 43 distinct graphs (850 KB); 13 are common to all (`0x340,456,778,B50,B5D,B6F,BA7,C04,C0E,C83` + `6D9,6FE,C11`); the rest are hero-specific (e.g. Soldier `0xB15,B16`). Training Bots (`0x1404`, `0x14AF`: 1-2 graphs, no profiles) and PvE enemies (Trooper, Sniper, Enforcer...: 4 profiles, no slots) are simpler. Table JSON: `.scratchpad/pva/ai_table.json`.

## 3. Navigation / AI map data

Every real map has entity chunk `0x0DD000..0B<idx>` and **chunk `0x12`** (`0x0DD000120000<idx>`, 4-35 MB). Chunk `0x12` is an STU-v1 (`DUTS`) container holding `STU_3BC32A6E` (OWLib: `m_9DCB15AF = STUAICellData[]`; sibling classes `STU_9199BF6B/CC7A1969/34F42343/110E3A19` hold `STUAICellBytes.m_data`). So it is the **AI cell/nav grid** (the earlier "pathing or regions" note is now explained). Not decoded: layout starts `u64 0x20, u64 offset, u64 1, u64 offset`, `DUTS` at +0x20 and again near the end. Practice Range (8.3 MB) and Tutorial have it too. Also per map: AI hint volumes in chunk `0x0B` (19 on Ilios, 23 Oasis, 26 Nepal), `STUAILevelPlacedMetaData` (1-4) and waypoints (entity `0x122`, `STUWaypointInstanceData`: 9 on Ilios, 208 on Oasis), probable hint-volume entity `0x8C` (20/32). Collision (chunk `0x11`) is separate.

## 4. Candidate first map

Cheapest per asset size is **Horizon Lunar Colony** (Assault, `0x6D3`: hdr `0x07900000000006D3`, world `0x08000000000006D3`, collision 22 MB, nav 5.8 MB) but Assault is the heaviest objective logic. **Recommended: Oasis (Control)**, `0x69E`:

- Header `0x079000000000069E`, world `0x080000000000069E` (912 B), modes Control `0x230..17` + Skirmish `..10`, Control team size 6v6. Chunks: `0x0B` 994 KB (3,145 placeables), `0x11` collision 45 MB, `0x12` nav 15.9 MB. (Sub-maps `0x74A/74C/74D` carry CTF/Elim modes only.)
- **Spawn points (inference, high confidence):** entity `0x40000000000183C` (target-tag + mirrored component, placed in groups of 6 = one team of 6). Six clusters, each tagged by a trailing plain ref `0x0D60000000000nnn` in its override: `3CC` (77.9, 6.6, -310.8); `341` (219.3, 1.1, 169.9); `30D` (204.2, 6.6, -185.0); `310` (66.1, 1.1, 323.3); `3CD` (-187.9, 20.1, -89.9); `3CE` (-188.1, 20.1, 90.5). Tag set identical on Ilios, so the pairs are team slots per round (3 rounds x 2 teams; `3CD/3CE` is an obvious mirror pair).
- **Objectives:** the capture-area entity (`0x400000000000037`, `STUCaptureAreaComponent` `B70590E4`, graph `0x580000000000028`) is **not placed in any standard map** (only unnamed `0x664/0x794`). Control points are therefore area placeables (chunk `0x0A`, 61 areas) or spawned by script, linked to the mode graph by UUID: **unresolved**. Map-logic entity `0x4000000000007A4` (one per map, target tag + statescript) at (-206, 19, 4). To resolve: decode chunk `0x0A` boxes (OWLib `teMapPlaceableArea`) and the statescript graph referenced by entity `0x7A4`'s instance data.

## Open items

1. Slot-to-mode mapping of the 7 behaviour slots; 2. Easy/Medium/Hard to difficulty-id mapping (needs the UI graph `BE9` button -> value wiring); 3. chunk `0x12` format; 4. how AI controllers are created for slots (not in any STU asset; hero-AI entity is referenced only by body entities and PvE graphs, so likely native client + server); 5. IDA was not used (MCP offline).
