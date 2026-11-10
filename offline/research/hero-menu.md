# Hero menus: what makes every 1.74 hero show (2026-11-03)

Research only. Nothing in the repo or the game was changed or launched. Build 104319, assets read through the CKey-verified CMF path (`tools/assets`), en-US strings from `strings.pkl`. Scratch scripts: `.scratchpad/hero-menu/h*.py` (`h12.py` = unlock-list reader, `h14.py` = hero table, `h18.py` = skin themes).

Evidence tags: **[asset]** read from 1.74 data; **[ext]** from the pinned upstream lobby project's code/data (`Boi-027/overwatch-1.74-lobby-server`, MIT, commit `0fa61b9`, the same one `tools/lobby/vendor` comes from), read as plain text, with its claims not yet checked in game; **[inf]** inference.

## 1. The 1.74 hero list

56 STUHero assets (type `0x75`, GUID `0x02E0 0000 0000 nnnn`, class `7C3457DC`). **32 are playable heroes**: they have `m_category` (role), a `m_heroProgression` with 165-206 unlocks, ~65-82 skin themes and the flag field `62746D34 = 1`. The other 24 have none of those.

| Role | Heroes (GUID low part) |
|---|---|
| Damage (17) | Reaper 02, Tracer 03, Hanzo 05, Torbjörn 06, Pharah 08, Widowmaker 0A, Bastion 15, Symmetra 16, Genji 29, Cassidy 42, Junkrat 65, Soldier: 76 6E, Mei DD, Sombra 12E, Doomfist 12F, Ashe 200, Echo 206 |
| Tank (8) | Reinhardt 07, Winston 09, Roadhog 40, Zarya 68, D.Va 7A, Orisa 13E, Wrecking Ball 1CA, Sigma 23B |
| Support (7) | Mercy 04, Zenyatta 20, Lúcio 79, Ana 13B, Brigitte 195, Moira 1A2, Baptiste 221 |

(17 + 8 + 7 = 32.) Role identifiers: `0x0D8000000000084E` Damage, `..0850` Tank, `..0851` Support (inference from which heroes use them).

Not playable, no menu data (all have `m_skinThemes` = 1 entry, no category):

| Kind | Heroes |
|---|---|
| Training / practice bots | Training Bot `16B 16E 16F 170 171 172`, Friendly Bot `16C 16D` (vs-AI and Range use `16B..16E`) |
| Null entry | `165` (no name) |
| PvE (Junkenstein / Retribution / Uprising) | Eradicator `173`, Zomnic `175`, Shock-Tire `176`, Zombardier `177`, Slicer `178`, Nulltrooper `179`, Detonator `17C`, Trooper `1AC`, Sniper `1B8`, Heavy Assault `1BA`, Assassin `1BB`, Gunship `1C7`, Enforcer `1CE`, Boat `1F9`, Bob `21D` |

The PvE ones carry a progression asset with 4-5 poses only (`0xEA0..96`, `..9B`). Bob has no progression. AI entities: 15 playable heroes have one, Doomfist and Brigitte do not (`practice-vs-ai-data.md`); upstream's `extracted_heroes.json` has the same 56 entries with a `SupportsAi` flag **[ext]**.

## 2. Record 20505 and the unlock data model

### 20505 layout

Local `lobby.py` writes: `u32 n + n×u64` (heroes), `u32 n + n×u64` (second list, now 0), `u32 n + n×u64` (map headers), `u32 1 + 0x0308000000000001`. Upstream's schema for the IN_CONNECT group (`F5FF548A`) says message 20505 is **four u64 arrays at struct offsets +0x78, +0x90, +0xA8, +0xC0** **[ext]**. The second one (+0x90) is the **skin-theme preload list**: upstream's comment says skins newer than its capture (Luchador, Royal Knight) show the default model unless their theme is in this list, so it offers every skin theme in the item catalog plus built-in extras, and lobby maps that give heroes extra skins add them here too **[ext]**.

So: list 1 = heroes (`0x02E0...`), list 2 = **STUSkinTheme GUIDs** (type `0xA6`, GUID `0x0A50...`), list 3 = map headers (`0x079...`), list 4 = content sets (`0x0308...`). Element type of list 2: `u64` skin-theme GUID **[ext, asset-consistent]**: `STUHero.m_skinThemes[].m_5E9665E3` is exactly that type, and the 1.74 data has 8,819 such assets. How the client consumes the record: **not traced**. The idalib session is up, but the memory dump has no analysable handler for 20505 (message classes are vtable getters returning the id; `mov eax,0x5014 / 0x501E` exist, no `0x5019` or `0x5016` getter in the dump; the `20504` immediate hit was an unrelated WMI telemetry function). Nothing written to the IDB.

### GUID types (type = `resource_type`, prefix = top 12 bits of the GUID)

| Thing | Type | GUID prefix | STU class | Example |
|---|---|---|---|---|
| Hero | `0x75` | `0x02E` | `STUHero` | Soldier `0x02E000000000006E` |
| Unlock (item: skin, spray, voice line, emote, victory pose, highlight intro, icon, frame, weapon skin, currency, loot box) | `0xA5` | `0x025` | `STUUnlock_*` | `0x0250000000000886` |
| Skin theme | `0xA6` | `0x0A5` | `STUSkinTheme` `42270D59` | `0x0A500000000016C7` |
| Hero loadout entry (ability/weapon card) | `0x9E` | `0x0B9` | `STULoadout` | `0x0B9000000000008F` Sprint |
| Progression / unlock pool | `0x58` | `0x0EA` | `STUProgressionUnlocks` `8CE1D110` | Soldier `0x0EA000000000006B` |
| Preview emote entity | `0x04` | `0x040` | `STUEntityDefinition` | Soldier `0x0400000000001438` |

Unlock classes, with total asset counts in 1.74: SkinTheme 2,472; SprayPaint 1,710; VoiceLine 1,043; AvatarPortrait (player icon) 844; Pose (victory pose) 338; PortraitFrame 300; Emote 292; POTGAnimation (highlight intro) 212; Weapon 64; CompetitiveCurrency 50; Currency 23; OWLToken 15; LootBox 12; one `1EB22BDB`. The Pose / POTG split is by name: default entries read "Heroic" for pose `0x..886`, highlight `0x..227`, emote `0x..904` (Soldier), plus "RANDOM" entries. Highlight intro = POTGAnimation, victory pose = Pose is an inference from those names.

Common `STUUnlock` fields: `m_00B16A0B` (u64 item id, e.g. `0x0D5517`), `m_8A5CC633` (u64), `m_name` (string ref), `m_rarity`, `m_A7B393BF` (kind enum: 1 pose, 2 emote, 3 highlight, 4 spray, 5 voice, 6 weapon, 7 icon, 8 currency, 9 comp. currency, 12 loot box; skins have none). Per class: SkinTheme `m_skinTheme` (`4C167404`, ref to the theme); Emote `m_emote` (`0x0C8...` STUAnimAlias) + `m_emoteBlendTreeSet`; Pose `m_pose`; POTG `m_animation`; Spray `m_DAD2E3A2` + inline `1B25AB90`; VoiceLine `m_F57B051E`.

### How a hero reaches its unlocks

- `STUHero.m_heroProgression` (`485AA39C`) -> `STUProgressionUnlocks` (type `0x58`). Inside, unlock refs are 16-byte array elements (`u64 -1`, `u64` mangled ref); they decode with field hash **`0xDB803F2F`** (`m_unlocks`), see `h12.py:unlock_refs`. Nine arrays: a 61-item pool, seven loot-box pools (9, 12, 7, 6, 11, 11, 8) and a final 61-item `m_otherUnlocks` pool (events, 53 of them skins) [pool meaning is inference]. `m_160B0466` chains to a second progression asset (`0xEA..0A1` for Soldier, zero unlocks found), `m_lootBoxesUnlocks` holds 8 box-type records.
- `STUSkinTheme.m_16051DC7` (a plain `u64`, not an asset ref) = the hero GUID the theme belongs to. 8,818 of 8,819 themes name a hero; 198 per hero are team-colour variants (`m_teamColor`, GUIDs `0x0A508xxxxxxxxxx`, `m_02F889AE` = base theme) that are not in the hero's own list.
- `STUHero.m_skinThemes` (`84625AA3`, inline `STU_63172E83[]`): theme ref (`5E9665E3`) + flags (`7B95C2A2`, `743E6C23`). The `0029461B` ref in each element points to a type-`0x76` "STUSkin" GUID (`0x0AE...`) that is **not in the retail CMF** (dangling; inference: legacy field).
- `STUHero.m_heroLoadout` (`77FED604`): the 5 `STULoadout` cards (Soldier: Sprint, Biotic Field, ... all `0x0B9...`).
- Not a hero field: the unlock's own link to a hero. Skin unlocks reach the hero through their theme; sprays, voice lines, emotes, poses and highlights only through the hero's progression pools.

### Soldier: 76 (`0x02E000000000006E`), counts

| What | Count |
|---|---|
| `m_skinThemes` list | 77 (73 have an unlock in the progression; 4 do not: `0x0A5..1B25, ..1C05, ..47D5, ..47F5` map to unnamed unlocks `0x025..0E75, ..11F7, ..4B56, ..4B76`; inference: default and internal skins) |
| Skin themes tagged to him (incl. team colours) | 275 |
| Unlocks in `m_heroProgression` | 186 = 73 skins + 39 sprays + 33 voice lines + 12 icons + 11 victory poses + 9 emotes + 7 highlight intros + 2 weapon skins |
| Named examples | skins Classic `0x0A5..16C7`, Olive, Russet, Smoke, Jet, Bone, Golden, Strike Commander Morrison, Commando: 76, Night Ops: 76, Soldier: 1776; "Heroic" pose/highlight/emote as default; Helix, Lights Out highlights |

All 32 heroes, same method (`h14.py`, `h27.py`):

| Hero | GUID | Role | Skin list | Skin unl. | Spray | Voice | Emote | Pose | Highlight | Icon | Weapon | Progression asset |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Reaper | `0x02E0000000000002` | Damage | 80 | 76 | 46 | 31 | 9 | 12 | 6 | 14 | 2 | `0x0EA0000000000002` |
| Tracer | `0x02E0000000000003` | Damage | 82 | 78 | 47 | 32 | 12 | 11 | 7 | 15 | 2 | `0x0EA0000000000001` |
| Mercy | `0x02E0000000000004` | Support | 81 | 77 | 52 | 32 | 9 | 12 | 6 | 16 | 2 | `0x0EA000000000002A` |
| Hanzo | `0x02E0000000000005` | Damage | 76 | 72 | 38 | 36 | 9 | 11 | 7 | 14 | 2 | `0x0EA000000000002C` |
| Torbjörn | `0x02E0000000000006` | Damage | 76 | 72 | 36 | 34 | 9 | 11 | 7 | 14 | 2 | `0x0EA000000000002B` |
| Reinhardt | `0x02E0000000000007` | Tank | 80 | 76 | 36 | 32 | 10 | 10 | 6 | 12 | 2 | `0x0EA0000000000029` |
| Pharah | `0x02E0000000000008` | Damage | 78 | 74 | 39 | 33 | 8 | 10 | 7 | 11 | 2 | `0x0EA0000000000017` |
| Winston | `0x02E0000000000009` | Tank | 75 | 71 | 37 | 35 | 9 | 10 | 7 | 14 | 2 | `0x0EA000000000003D` |
| Widowmaker | `0x02E000000000000A` | Damage | 80 | 76 | 38 | 33 | 8 | 10 | 7 | 14 | 2 | `0x0EA0000000000016` |
| Bastion | `0x02E0000000000015` | Damage | 80 | 76 | 37 | 26 | 11 | 9 | 6 | 13 | 2 | `0x0EA000000000003E` |
| Symmetra | `0x02E0000000000016` | Damage | 78 | 74 | 44 | 34 | 8 | 10 | 7 | 13 | 2 | `0x0EA000000000003F` |
| Zenyatta | `0x02E0000000000020` | Support | 77 | 73 | 40 | 33 | 9 | 10 | 7 | 15 | 2 | `0x0EA0000000000040` |
| Genji | `0x02E0000000000029` | Damage | 80 | 75 | 40 | 31 | 10 | 10 | 6 | 14 | 2 | `0x0EA0000000000041` |
| Roadhog | `0x02E0000000000040` | Tank | 78 | 74 | 37 | 33 | 9 | 10 | 7 | 14 | 2 | `0x0EA0000000000069` |
| Cassidy | `0x02E0000000000042` | Damage | 79 | 75 | 42 | 34 | 9 | 10 | 7 | 15 | 2 | `0x0EA0000000000065` |
| Junkrat | `0x02E0000000000065` | Damage | 77 | 73 | 39 | 34 | 10 | 11 | 7 | 14 | 2 | `0x0EA0000000000067` |
| Zarya | `0x02E0000000000068` | Tank | 78 | 74 | 37 | 32 | 9 | 11 | 6 | 11 | 2 | `0x0EA000000000006C` |
| Soldier: 76 | `0x02E000000000006E` | Damage | 77 | 73 | 39 | 33 | 9 | 11 | 7 | 12 | 2 | `0x0EA000000000006B` |
| Lúcio | `0x02E0000000000079` | Support | 79 | 75 | 40 | 33 | 10 | 11 | 7 | 12 | 2 | `0x0EA0000000000068` |
| D.Va | `0x02E000000000007A` | Tank | 79 | 75 | 48 | 33 | 8 | 10 | 7 | 15 | 2 | `0x0EA0000000000066` |
| Mei | `0x02E00000000000DD` | Damage | 79 | 75 | 38 | 36 | 11 | 10 | 7 | 13 | 2 | `0x0EA000000000006A` |
| Sombra | `0x02E000000000012E` | Damage | 79 | 75 | 42 | 35 | 9 | 10 | 6 | 18 | 2 | `0x0EA0000000000090` |
| Doomfist | `0x02E000000000012F` | Damage | 76 | 72 | 37 | 32 | 8 | 11 | 7 | 13 | 2 | `0x0EA0000000000097` |
| Ana | `0x02E000000000013B` | Support | 77 | 73 | 43 | 35 | 10 | 10 | 7 | 14 | 2 | `0x0EA000000000008F` |
| Orisa | `0x02E000000000013E` | Tank | 74 | 70 | 40 | 32 | 9 | 11 | 7 | 13 | 2 | `0x0EA0000000000094` |
| Brigitte | `0x02E0000000000195` | Support | 76 | 72 | 38 | 32 | 9 | 10 | 6 | 12 | 2 | `0x0EA000000000009A` |
| Moira | `0x02E00000000001A2` | Support | 73 | 69 | 40 | 34 | 9 | 10 | 6 | 13 | 2 | `0x0EA0000000000098` |
| Wrecking Ball | `0x02E00000000001CA` | Tank | 73 | 69 | 39 | 31 | 8 | 10 | 7 | 12 | 2 | `0x0EA000000000009D` |
| Ashe | `0x02E0000000000200` | Damage | 74 | 70 | 48 | 33 | 8 | 10 | 7 | 18 | 2 | `0x0EA000000000009E` |
| Echo | `0x02E0000000000206` | Damage | 65 | 61 | 43 | 26 | 8 | 10 | 6 | 10 | 2 | `0x0EA00000000000CD` |
| Baptiste | `0x02E0000000000221` | Support | 73 | 69 | 45 | 34 | 9 | 8 | 6 | 12 | 2 | `0x0EA00000000000C0` |
| Sigma | `0x02E000000000023B` | Tank | 70 | 66 | 37 | 29 | 9 | 9 | 6 | 10 | 2 | `0x0EA00000000000C4` |

Union of all 32 `m_skinThemes` lists: 2,459 themes (the candidate content for list 2).

## 3. What else the menus need (beyond 20505)

20505 only makes content **loadable**. What is **owned/equipped** comes from other login messages, which the local lobby does not send **[ext]**. Upstream's login order (IN_CONNECT 20500, 20502, 20504, **20505**, events 38900/38902, **PARTY 20700**, endorsements/presence, **PROGRESSION_IN 24300**, **HERO_CATALOG 24900**, MODE_RULES 27202, retail set, ranked 36302/36300, PING 35500):

| Msg | Group (CRC, announced by the client: yes) | Carries | Layout [ext, struct offsets] |
|---|---|---|---|
| 24900 | HERO_CATALOG `70E5A247` | Per hero: owned items and loadout; the Hero Gallery and the hero-select skin picker read it (inference) | `+0x78 u32`; `+0x80` array of hero records (size 160): `+0x00` owned `{u64 unlock GUID, bool true}[]`, `+0x18` `{u64,u64}[]`, `+0x30` hero GUID, `+0x38` Skin, `+0x40` HighlightIntro, `+0x48` VictoryPose, `+0x50` WeaponSkin, `+0x58` Spray[], `+0x70` VoiceLine[], `+0x88` Emote[]; `+0x98` store `{ {item, achievement GUID @+0x8, unlock level u32 @+0x10, price u16 @+0x14, ...}[] , hero }[]` (corrected 2026-11-08; progression/purchase-contract); `+0xB0` `{u64,u64}[]` |
| 24300 | PROGRESSION_IN `779A8581` | Account state: loot boxes, account-owned items (icons, sprays, frames), frame, icon, level, credits, comp points, league tokens | inner struct at `+0x78`: `+0x00` boxes, `+0x30` account items `{guid,true}[]`, `+0x58` frame, `+0x60` icon, `+0x68` level, `+0x6C/0x70/0x74` credits/comp/tokens, `+0x78/79` two bools |
| 20700 | PARTY `2411DE56` | Party card, includes the menu hero | not read (field layout unknown) |
| 36600 | CONFIG `1732EBF2` | Key/value list: `+0x78` array of `{u32 key @0, string @8}`; key `0x04227E56` = supported builds, key **`0x87053C32` = menu hero** (value: hero GUID as `"0x02E..."` text, 16 hex digits). The client re-reads it live | |
| 24500 / 24902 | GALLERY_OUT `B68870B8` / HERO_CATALOG | Equip request `{hero, item GUID, slot}` from the gallery and the echo; hero 0 or an icon updates the player card via 20502 | |
| 24301/24901/24302/24306 | | Unlock granted (account / hero), loot-box merge, purchase result | |

Item GUIDs in 24900/24300 are **unlock GUIDs** (type `0xA5`, `0x025...`); upstream's catalog file names items `xxxx.0A5` and skin themes `xxxx.0A6` in the dotted form **[ext]**. "Owned by default" is a catalog notion upstream takes from a 1.68 capture (every hero gets default items + the unlocks the profile owns). The STU data has no default flag. Practical default set: the "Heroic" pose/highlight/emote and the unnamed skin unlocks, plus whatever you decide to own (everything is allowed offline).

The local lobby's vendored schema code (`ow174/jam`, `self.schemas.encode`) only knows the handoff group. Sending these needs: the group's wire index from the client's announcement (`groups[crc]`, the lobby already keeps it; all five CRCs above appear in the client's list in `lobby.log`) and a schema for each message.

Hero select in the match (statescript graph `0x288B`, `heroselect.py`): today only advertised heroes (20505 list 1) are offered, picks of anything but Soldier become Soldier (`match.py`). A chosen skin is sent to the lobby with 24500 and the body wears it.

Hero Gallery data source (inference): hero assets (`m_previewEmoteEntity` `AC91BECC`: model + voice + preview statescript graph `0x580000000000BEA` for 29 of 32 heroes; extras `E57` Genji, `FD0` Cassidy, `20D9` Sigma; `m_previewWeaponEntities` `FCD2B649`; `m_skinThemes`; `m_heroLoadout`) plus 24900 for what is owned. The only statescript graph holding the "HERO GALLERY" (`0x0DE00000000023B8`) and "LOOT BOX" (`..23B9`) strings is `0x0580000000001214`, i.e. the main-menu button list; it holds labels, not hero data (scan finished after 39 min).

## 4. The main menu's background hero

**Correction (2026-11-07):** the negative asset scan below used mistyped class hash `47F95AF4`. Native registration and local OWLib declare **`47F99DF4`**. A fresh content-verified search found that class in shipped graphs `0x0B4A` and `0x1215`; `0x0B4A` selects the menu hero and creates its Showcase cosmetic entity. The claim that shipped data contains no single-hero consumer/live-service placement is disproved. An empty random cache also cannot prove the getter was never called when a valid override returns first. See [current investigation](main-menu-hero-selection.md). Earlier observations below are historical; their absence/closure conclusions are superseded.

- **Not found in the assets or the client code.** No STU asset or graph read here names a "menu hero" pool; the IDA dump has no reachable parser. The menu scene itself changes per session (King's Row-like street, a Rialto-like view in `research/main-menu/*.png`), so the scene is chosen client-side, and in those runs **no hero stands in it** (the local lobby sends no 24300, 24900, 20700 or 36600).
- Evidence for how the hero arrives **[ext]**: (1) the server tells the client through CONFIG 36600 key `0x87053C32` (hero GUID text) and the PARTY 20700 hero for the party card; (2) with "random" or "none" upstream sends nothing and the hero stays whatever the event scene brings, so the random choice is client-side (its own comment: "the OWL lobby's Genji"); (3) upstream also rerolls a per-login random hero for the party card server-side. The pool the client draws from is **unknown**. [inf] most likely owned heroes (24900) or the 20505 list; nothing here proves it.
- Test order: send 24900 with all 32 heroes owned first and look for a hero in the menu; if none, add 36600 `0x87053C32 = "0x02E000000000006E"` (Soldier) to see whether a forced hero renders at all.

### Superseded conclusion (2026-11-05): no single menu hero in 1.74 data

- **Event -> menu scene is decoded.** Generic settings asset `0x0CA000000000012C` (class `STU_46AB3B40`, `STULobbyMapCatalogInfo[]`, stored as a nested blob whose refs are XOR-mangled per field: celebration key `D8 08`, catalog key `14 C4`) maps each celebration to a map catalog (type `0x01C`), and the catalog to its menu maps: lunar `0x105` -> `0x25C`/`0x25A` -> map `0xDAD`; `0x11F`, OWL `0x125` and Watchpoint `0x126` -> `0x2D2` -> map `0xEEE`; Cassidy `0xF7` -> `0x33E` -> `0xE44`; Malevento `0xFA` -> `0x342` -> `0xE49`; Halloween `0xFB` -> `0x1B9`; Winter `0xFF` -> `0x346`; Reaper `0x104` -> `0x348`; Anniversary `0x10E`/`0x112`/`0x118` -> `0x34A` -> `0xE83`; farewell `0x128` -> `0x361` -> `0xF14`; no event -> `0x1B3` (ten maps: `0x664`, `0x675`, `0x677`, `0x678`, `0x794`, Junkertown `0x801`, Rialto `0x8BE`, `0xA84`, Paris `0xAF0`, Havana `0xB55`) and `0x216` (`0x676`). The event maps decrypt with the vendored content keys.
- **The OWL scene loads.** With `--event owl` the menu is the Hanamura courtyard (`main-menu/events/owl-map-eee-no-hero.png`), and live memory holds 16 references to map `0x0800000000000EEE` against 3 for the others: that courtyard *is* the OWL lobby map. Retail put an OWL-skinned Genji in it; nothing does here.
- **Nothing asks for the menu hero.** Live (`.scratchpad/gallery/menu_probe.py`, disposable: reads system 9 through the obfuscated system table): the 36600 override is stored (`0x02E000000000012E`), the random pick cache `+0x298` is 0 and the hero stack `+0x1350` is empty, so the getter `0x7FF6D1844170` never ran. Its only code caller is the evaluator of config-var class `STU_47F95AF4` (vtable `0x7FF6D384B168`); that class has one live object, which is the class registry's prototype (a table of vtable pointers 16 bytes apart, referenced from the registration table), not a data instance. The class is created only through the STU registration (`sub_7FF6D1B8F7C0` / `sub_7FF6D1B94DC0`), and no readable asset (root and en-US text manifests, keys loaded, including every chunk of maps `0xEEE` and `0xF14`) holds its hash. The other caller of the random pick, `sub_7FF6D187BE20`, is obfuscated (junk code, `rdtsc`), and the recorded 43200 (OWL live) carries only a bool and an empty list.
- Conclusion: in 1.74 the single menu hero is not in data the client ships; it was a live-service placement. The farewell lineup (map `0xF14`, created by its graph `0x0580000000000F14`'s `STUStatescriptStateCreateEntity`) is the only hero scene available. 36600 key `0x87053C32` and the 20700 party hero stay as upstream sends them; they cost nothing.

## 5. Plan: server-side changes to try, in order

Each step is a lobby change, then `run.py --asi` retail-menu run, check the in-game result and `Overwatch.log` for `ERR`/"Missing entity".

1. **All 32 playable heroes in 20505 list 1** (`login_records` heroes). Keep the bot heroes. Check: hero select lists 32 portraits; a pick still plays Soldier; no "Missing entity definition" and no crash on login (32 bodies preload; watch load time). If a body is missing, drop that hero and log it.
2. **Skin themes in list 2.** First Soldier's 77 (`m_skinThemes` of `0x02E000000000006E`, order as in the asset), then all 32 heroes' lists (2,459 GUIDs, `.scratchpad/hero-menu/h18.py` gives them). Skip the 198 team-colour variants per hero at first. Check: hero select skin picker (the screen only offers skins while `v25377` is on, `heroselect.py`) shows skins and a pick renders; log size/login time stays sane.
3. **24900 hero catalog.** One record per playable hero, `owned` = default items plus every unlock from its progression asset (the lists in section 2), slots set to the hero's default skin/"Heroic" items, sprays/voice/emote lists empty or the first few. Send it after 20505 with `groups[0x70E5A247]`, offset 0. Check: Main menu > HERO GALLERY opens, shows all 32 and their skins/emotes/poses as owned; the screenshot goes to `research/main-menu/`. Reply to 24500 with 24902 so equipping works.
4. **24300 progression.** Level, credits and the account items (icons, frames, sprays). Check: Career/Loot Box screens stop being empty; icons selectable.
5. **Menu hero.** Add CONFIG 36600 with key `0x87053C32` = `"0x02E000000000006E"` and PARTY 20700 (needs its layout; capture or schema). Check: the hero appears in the main-menu scene. Then remove the key and see whether the client picks a random hero on its own (decides the pool question).
6. Only then: unlock `Missing` content in the match (hero picks other than Soldier) with `non-soldier-hero-scope.md`.

Risks: wrong struct layouts crash or stall the client (probe one message at a time and keep the previous login as fallback); the 24900/24300 layouts come from a 1.68 capture decoded with 1.74 schemas **[ext]** and are not yet verified against this client.

## Open items (revised 2026-11-05)

Done since 2026-11-03 (details in `tools/lobby/CHANGELOG.md`): 24500 is answered with 24902 and equips are kept across logins (`data/equips.json`); every hero's loadout slots carry its defaults; 20700 is sent with upstream's layout (party of one). The Hero Gallery counts and lists what the 24900 store list (`+0x98`) carries, so "where the gallery reads its data" is answered for the owned/count side; the model and categories come from the hero asset.

- **The 12 uncounted Soldier unlocks are his 12 player icons.** The gallery has no icon category (Skins, Emotes, Victory Poses, Voice Lines, Sprays, Highlight Intros, Weapons) and does not count defaults: Soldier reads 72/8/10/32/39/6/1 = 168 (`main-menu/hero-gallery-soldier-categories.png`), i.e. 186 - 12 icons - 6 defaults; Orisa 69/8/10/31/40/6/1 = 165 = 184 - 13 icons - 6 (`hero-gallery-orisa-categories.png`). The grid counts of Ana 174, Ashe 172 and Reaper 176 fit the same rule. Hero icons show on Career Profile > Player Icon instead (`career-player-icons.png`).
- The nine progression arrays' meaning (one pool, seven loot-box pools, `m_otherUnlocks`) is still inference; nothing in the lobby depends on it.
- The client-side handler for 20505: the IDA dump lacks the pages and a live trace is not possible (a debugger attach and in-process hardware breakpoints both crash the 1.74 client, `soldier-runtime/helix-visual-findings.md`). Not needed: the list is applied (skins render).

## Results

- **Step 1 done (2026-11-03):** the login record lists all 32 playable heroes (`PracticeRange.menu_heroes`). Login is unchanged; hero select shows the full roster (8 / 17 / 7, `main-menu/hero-select-32.png`); a non-Soldier pick still plays Soldier; the Hero Gallery lists all 32, each "0/0" (`main-menu/hero-gallery-32.png`). A hero's gallery page opens with its categories (Skins ... Weapons) but shows no model and 0 credits (`main-menu/hero-gallery-soldier-empty.png`): unlock data is step 2 on.
- **Step 2 done (2026-11-03):** 20505's second list carries every advertised hero's skin themes (2,459, `tools/assets/skin_themes.py` writes `tools/lobby/data/skin_themes.json` once). Login is unchanged (20 KB record). No visible change: the gallery still reads 0/0, shows no hero model, and a hero's SKINS list says "EMPTY" (`main-menu/hero-gallery-skins-empty.png`). So the list only preloads; owned items need step 3 (24900).
- **Step 3 done (2026-11-03):** 24900 is sent after 20505 (`PracticeRange.hero_catalog`). Owned lists alone (`+0x80`, each hero's 6,003-in-total unlocks) changed nothing: still 0/0. Adding the per-hero store lists at `+0x98` (same unlocks, upstream's not-for-sale entry `{guid, 0, 65535, 0, -1, False}`) made the gallery count and list them: every hero shows N/N (Soldier 174/174, `main-menu/hero-gallery-owned.png`), Soldier's page shows his model and SKINS 73/73, rarity-coloured, with EQUIP (`main-menu/hero-gallery-soldier-skins.png`). Left: loadout slots are 0 (no equipped items); no 24902 reply to 24500 yet, so equipping is not saved; the gallery counts 174 of Soldier's 186 unlocks (the other 12 not checked; perhaps items it never lists). All three resolved later (Open items, 2026-11-05).
- **Step 4 done (2026-11-03):** 24300 is sent after 24900 (`PracticeRange.progression`): level 1, first portrait border, `+0x50` 2000 and the two flags as upstream sets them. No visible change at 0 credits; a TEMP probe with 1,234 credits showed on the Loot Box screen (`main-menu/progression-credits-probe.png`), so the record is read. Career Profile is a separate request: it sends `0x75D32AE2` (group 41, offset 6, 38 bytes) and spins until answered.
- **Step 5 partial (2026-11-03):** login sends 20700 (party: the player alone, leader, hero, skin 0) and 36600 (`0x04227E56` = `"[104319]"`, `0x87053C32` = lowercase hero GUID text; upstream's form). The party card appears top right (`PLAYER`, with a "-75% XP / 0 matches remaining" strip). No hero shows in the menu scene, with or without Soldier's Classic skin (`0x02500000000004C7`) in 20700 and his 24900 loadout. Client findings (IDA, base `0x7FF6D1220000`, live reads with `.scratchpad/live_peek.py`):
  - key `0x87053C32` is a config var at `0x7FF6D4058D80`; its parsed GUID sits at `0x7FF6D4058DA0`. Read live it holds the hero we send, so 36600 is applied.
  - the menu-hero getter `0x7FF6D1844170` (on system object 9 of the client's system table) returns, in order: that override if it loads as a hero asset; the top of a stack at `+4936`; else a cached pick at `+664`, filled by `0x7FF6D1845AE0`.
  - `0x7FF6D1845AE0` is the **client-side random hero**: a random member of system 70's hero set (`+104`/`+112` hash map, value `+48` = hero GUID), skipping heroes a service at `0x7FF6D407A670` rejects (vfunc `+136` on the hero's id at `+28`). Read live, the set is exactly our 32 heroes (from 20505/24900), so the pool question is settled; the cached pick stayed 0.
  - the getter's one data user is the evaluator (vtable slot 4) of config-var class `STU_47F95AF4` (runtime record `0x7FF6D418D528`, STU registration `0x7FF6D13DDF30`). No scanned asset holds an instance of it, so whatever places the hero in the scene is not in the graphs, entities, maps or modes scanned so far.
  - A byte scan of every readable asset in the root manifest (`.scratchpad/pva/bytescan.py`, all types) finds no `F4 5A F9 47`; the same method finds `STU_2B33EE38` in graph `0x0B7C`. So no readable asset places the hero.
  - Lead **[ext]**: upstream says the menu hero comes with the event scene ("the OWL lobby's Genji"), and its 38900 event record `+0x40` "picks the catalog of main menu maps"; preset `goodbye` is the 1.74 farewell lobby (celebration `0x128`, content key `0x0F100000000001A3`). Event content is encrypted: the 189 content-key assets (`0x0F1...`, resource type `0x90`) do not decrypt with our TACT keys (`tools/assets/casc.py KEYS`), and the client gets their keys from login 20504, which we send empty. Upstream ships the key bytes as `data/resource_keys_174.json` (pinned commit; 189 entries, `0x1A3` = key name `28B8EBB462B83EEA`).
- **Step 5 done via the farewell event (2026-11-03):** `resource_keys_174.json` is vendored; login sends 20504 with every named content key (178) and 38900 with celebration `0x128` (key `0x1A3`) running. The main menu now shows the 1.74 farewell scene: the launch heroes lined up together (`main-menu/farewell-lobby-heroes.png`). Same scene on two logins.
  - With the keys every asset in the root manifest decrypts (471,024), and still none contains `STU_47F95AF4`. So in 1.74 data nothing asks for the single menu hero; the 36600 override (verified stored) and the client's random pick have nothing to show. The override did not change the lineup (Mei, then Bastion picked).
- **Other events (2026-11-03):** `--event NAME` sends another of upstream's presets (`practice.MENU_EVENTS`). One login each: lunar = lantern hall, halloween = Eichenwalde at night, winter = Blizzard World in snow, anniversary = Watchpoint, reaper = King's Row at night, malevento = Malevento; owl and cassidy = a plain map (Hanamura, Temple of Anubis). None shows heroes, and the menu-hero override (stored each time) never appears. Upstream's anniversary note says its scene puts skinned heroes in the menu; its 8 skins are already in our 20505 list, so that scene may be one of several the event rolls (not tried further). Screens in `main-menu/events/`.
- **Default loadouts (2026-11-03):** `STUProgressionUnlocks` field `0x7846C401` lists each hero's 8 default items (Soldier: Heroic pose, Heroic and RANDOM intro, DEFAULT weapon, Classic, "I've still got it", Heroic emote, RANDOM pose). 24900 now equips them per slot (`+0x38` skin ... `+0x88` emotes) and gives them retail's default store entry, so the gallery leaves them out of the count (Soldier 168/168). No hero has a default spray.
- **Career Profile (2026-11-03):** 22206 answered with 20807 (account record, hero records, name, the request's id): the screen opens with name, level 1 and "Career profile is empty"; the Player Icon tab lists the hero icons (`main-menu/career-profile.png`, `career-player-icons.png`).
- **Equip (2026-11-03):** the gallery's equip request 24500 (group `0xB68870B8`: hero, item, slot) answered with 24902 marks the item equipped (`main-menu/hero-gallery-equip-olive.png`). Kept across logins since 2026-11-03: stored in `tools/lobby/data/equips.json` and worn by the next 24900 (24902 sent at login is ignored by the client) (`main-menu/hero-gallery-equip-kept.png`).
  - Upstream's other presets (Lunar New Year, Halloween, Winter, Anniversary) are other 38900 records with their own keys; not tried.
- **"-75% XP / 0 matches remaining" strip (2026-11-03):** not from the 24300 account record (its unset ints +0x6C/+0x70/+0x74 = 5/7/9 change nothing) nor from the 20700 party flags (+0x94/+0x95/+0x96/+0x98 all off change nothing). **Resolved 2026-11-05:** it is the leaver penalty. The strip lives in the party member tile, widget template `0x0BA000000000005E` (used by UX screen `0x09A00000000004DD`): state group `LeaverIndicatorStates` on property `IsLeaver`, texts `XpPenaltyText` ("-75%% XP", string `0x0DE000000000488B`) and `MatchRemainingText` (`%1$s match(es) remaining`, `0x0DE000000000488A`, bound to `LeaverGamesLeft`). No asset maps those names to ids and the names are not in the client; live, the binding objects (vtable img+`0x27B5B90`, name string at `+0x18`, id at `+0x80`) resolve `IsLeaver` -> `0x0D80000000006388`, `LeaverGamesLeft` -> `0x0D800000000643C`, `MemberStatus` -> `..64B7`, `OnlineStatus` -> `..652D`, `BattleTag` -> `..5BC3`, `IsMe` -> `..6397` (`.scratchpad/gallery/bind_scan.py`, disposable). The party VM writer `0x7FF6D1C0C8D0` sets `IsLeaver = IsMe && (|ratio - threshold| <= 0.004 || ratio > threshold)` with `ratio = popcount(system9+0x310) / system9+0x2F8` and `threshold = system9+0x308` (getter `0x7FF6D18440D0`): with nothing sent, 0/x vs 0.0 reads as a leaver. Those fields are LOBBY message **20813** `{+0x78 u64 leave bitmask, +0x80 u64 games, +0x88 f64 penalty ratio, +0x90 f64 warning ratio}`; retail's capture sends `0, 20, 0.3, 0.2`. The lobby now sends it at login (`PracticeRange.leaver_status`). Checked in game: the card shows only `PLAYER`, and the live `IsLeaver` reads 0 (`main-menu/party-card-no-leaver-strip.png`).
- **Anniversary scene (2026-11-03):** two more logins with `--event anniversary` both show Watchpoint without heroes. The celebration asset (`0x0430000000000118`) is not in the root CMF (it is event content behind its key), so its scene list is not read yet; event boxes, rewards and map swaps wait for loot boxes.
