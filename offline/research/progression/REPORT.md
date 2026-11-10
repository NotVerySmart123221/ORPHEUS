# Overwatch 1.74: XP, unlocks and loot boxes offline

Research date: 2026-11-07. Target: Windows retail `1.74.0.0.104319`. Research only: no game launch, live-process access, packet injection, executable patch, server-code change or live-client test. Inspected local source/assets and existing IDA dump through idalib. Public-source findings are collected separately in [official-sources-2026-11-07.md](<old-workbench>/research/progression/official-sources-2026-11-07.md).

## Finding

Offline progression is feasible through the existing local lobby. The client already has the gallery, currencies, portrait frames, loot-box models and reveal UI. Existing messages carry account state and hero ownership; preserved protocol research describes box grants/opening. Missing work is a durable earned account, correct reward eligibility, match settlement and request handling. Exact retail XP arithmetic and loot probabilities remain unresolved; the assets do not establish the full server algorithm.

Keep two concepts separate: **client compatibility** is largely mapped; **retail economy fidelity** needs further validation. A functioning reveal animation would prove the former, not the latter.

## Historical mechanics

Account XP is shared across heroes. A level grants one box; standard boxes contain four items with at least one Rare or better. Rewards are cosmetic or currency. Heroes do not gain damage, health or ability upgrades from levels. OW1's 32 original heroes should remain available independently of cosmetic ownership; OW2's original-hero unlock sequence was a new-account system introduced in October 2022. Eligible cosmetics can be purchased/equipped in the Hero Gallery. [Blizzard progression preview](https://overwatch.blizzard.com/en-us/news/20034301/), [OW2 systems announcement](https://news.blizzard.com/en-us/article/23857517/defense-matrix-activated-fortifying-gameplay-integrity-and-positivity-in-overwatch-2).

The November 15, 2016 patch retained introductory requirements for levels 2–13, reduced requirements for levels 14–100, and set every level after 100 to **20,000 XP without restarting the easy curve after promotion**. A 1.74 implementation must not use the original launch-era reset every hundred levels. Exact first-level thresholds were not recovered here. Portrait level/border and competitive SR are separate. Competitive eligibility historically required level 25. [November 2016 notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2016/11/), [Season 7 introduction](https://overwatch.blizzard.com/en-us/news/21173548/welcome-to-season-7-of-competitive-play/).

Late-OW1 XP sources included time, finishing a match, consecutive matches, wins, backfill and medals. Double XP multiplied existing bonuses, while First Win of the Day remained **1,500 XP**. Precise component ordering, rounding, time units, mode modifiers and reset windows need validation. [June 2020 rules](https://news.blizzard.com/en-gb/article/23445048/overwatch-double-xp-weekend-june-5-9).

Credits came from boxes and duplicates, and bought eligible cosmetics. The July 2017 change reduced duplicates and increased credit awards. This establishes duplicate protection as an important final-era behavior, but does not publish the complete rarity/pool algorithm. Neither a particular pity timer nor exact per-slot probabilities was verified. [Progression preview](https://overwatch.blizzard.com/en-us/news/20034301/), [July 2017 notes](https://overwatch.blizzard.com/en-gb/news/patch-notes/live/2017/07/).

Other reward tracks require their own eligibility: Arcade awarded up to three boxes weekly; endorsements gave periodic rewards; role incentives depended on demand. Event challenges could use games played with wins counting twice, while 2021 Archives challenges used mission stars. Golden weapons used **3,000 Competitive Points**, a separate balance from credits; official 2017 rules awarded 15 CP per win and 5 per draw. These are not reasons to invent local matchmaking, SR or endorsements for a first implementation. Sources and qualifications: [official-source note](<old-workbench>/research/progression/official-sources-2026-11-07.md).

The final-2022 boundary matters: box sales ended August 30, but standard boxes remained earnable; after September 14, unopened boxes automatically opened on login. Recommended offline target: ordinary OW1 earned-box behavior, explicitly retaining manual opening. This is a reconstruction policy, rather than reproducing the short sunset configuration. Do not let today's date expire historical events or auto-open the inventory. [Blizzard Anniversary Remix announcement](https://news.blizzard.com/en-us/article/23833172/overwatch-anniversary-remix-vol-3-is-now-live).

## What already exists locally

| Component | Current behavior | Progression implication |
|---|---|---|
| [Account record](<old-workbench>/tools/lobby/practice.py:55) | Hardcoded level 1, first border, no credits/boxes/account-wide ownership | Replace values with one persistent account snapshot |
| [Hero catalog](<old-workbench>/tools/lobby/practice.py:192) | Grants every extracted hero unlock; supplies gallery store entries | Advertise all eligible/catalog items, but own only defaults and earned items |
| [Store entry](<old-workbench>/tools/lobby/practice.py:124) | Defaults special-cased; other items use a not-for-sale entry | Credit purchasing needs correctly interpreted price/availability fields |
| [Career profile](<old-workbench>/tools/lobby/practice.py:232) | Repeats hardcoded account and cached hero records | Must use the same snapshot as login and reward updates |
| [Party](<old-workbench>/tools/lobby/practice.py:284) | Level 1 and first border repeated separately | Keep party level/border consistent with progression |
| [Gallery equip](<old-workbench>/tools/lobby/practice.py:318) | Handles 24500, replies 24902, persists equips in JSON | Reuse flow, add ownership/hero/slot validation for earned accounts |
| [Events](<old-workbench>/tools/lobby/practice.py:244) | Sends menu celebration; event-box field is -1 | Menu scene selection currently does not implement event rewards |
| [Assault finish](<old-workbench>/tools/lobby/vendor/ow174/game/match.py:642) | Computes winner and returns players to menu after result delay | First concrete match-settlement hook; round/match distinction matters when modes expand |
| [Lobby dispatch](<old-workbench>/tools/lobby/lobby.py:126) | Requests handled while waiting on TCP input | Rewards completed on game thread need a safe path to the lobby writer |

Existing evidence already shows account credits rendered in the Loot Box screen, hero ownership/gallery counts, equipped skins surviving login and Career Profile opening: [hero-menu research, results](<old-workbench>/research/hero-menu.md). Those earlier checks do not verify earned progression or loot-box opening on this installation.

Important ownership trap: removing locked items from both 24900 lists would hide the catalog. `+0x80` carries owned hero records; `+0x98` supplies the store/catalog. Keep the catalog complete while reducing ownership. Preserve defaults and existing equipped items consistently.

## Values recovered directly from 1.74 assets

Read with existing read-only CASC/STU helpers. Asset lookup verifies each payload against its CMF content hash. English labels were resolved from the `RDEV_LenUS_text` manifest and decoded using OWLib's two-byte string header convention. No pickle caches or live memory were needed.

### XP table

`STUGenericSettings_XPGains`, GUID **`0x0CA0000000000008`**, CKey **`b5d45413ddb1005ba3c5d9f9eb01189d`**:

| Type | English label | Shipped `m_amount` |
|---|---|---:|
| 0 | Bronze Medal | 25 |
| 1 | Silver Medal | 50 |
| 2 | Gold Medal | 75 |
| 3 | Match Finish | 75 |
| 5 | Time | 1.75 |
| 4 | Win | 250 |
| 7 | BACKFILL | 200 |
| 8 | CONSECUTIVE MATCH | 150 |
| 9 | First win of the day | 1500 |

The same asset contains three unnamed doubles of 1.2, plus 0.25, 0.3, 0.2 and an integer 20. **Do not label these as group/mode/leaver modifiers without tracing their consumers.** Likewise, `Time = 1.75` does not prove XP per second; `Match Finish = 75` does not prove the final retail award. Client presentation data can differ from authoritative server calculations or require additional scaling. Whether medals add together or only the highest contributes remains unverified.

Definitions: [XP settings](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STUGenericSettings_XPGains.cs), [gain entries](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STUXPGain.cs). Reproduction: `python -B tools/assets/stu_graph.py 0x0CA0000000000008 --width 450`.

### Prices and duplicate values

`STUGenericSettings_PlayerProgression`, GUID **`0x0CA0000000000018`**, contains nine pricing-like records in field `0x0C6A363E`. Nested entries are `STU_3B4E8CA6`: rarity `0xBB99FCD3`, values `0xCE91A110` and `0xAB5A6142`.

| Rarity | Base `CE91A110` | Seasonal types 1–6 `CE91A110` | `AB5A6142` |
|---|---:|---:|---:|
| Common 0 | 25 | 75 | 5 |
| Rare 1 | 75 | 225 | 15 |
| Epic 2 | 250 | 750 | 50 |
| Legendary 3 | 1000 | 3000 | 200 |

**Interpretation/inference:** `CE91A110` is purchase cost and `AB5A6142` duplicate compensation. Currency-kind record 1 contains 3000, consistent with golden-weapon CP pricing. Kind 2 contains 10/25/100/200, consistent with OWL pricing. The field names and enum values remain hashed; no purchase handler or store-price override was traced. These are strong candidate values, not proof every cosmetic is purchasable at the generic rate. Event-age discounts, promotions and non-purchasable items still need separate eligibility.

An additional base rarity-4 row contains 5000/1000. OWLib does not name rarity 4. Exclude it from a four-rarity random generator until identified. Account progression also has **300 level-award entries**, beginning at serialized levels 0, 11, 21, …, 101; these are unlock awards, not an XP threshold curve. Resolve their item references for borders rather than inventing border GUID arithmetic.

Definitions: [player settings](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STUGenericSettings_PlayerProgression.cs), [nested hashed types](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/Misc.cs:12967), [frame fields](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STUUnlock_PortraitFrame.cs), [rarity enum](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/Enums/STUUnlockRarity.cs).

### Loot rules and eligible pools

`STUGenericSettings_LootBoxes`, GUID **`0x0CA000000000011D`**, CKey **`a22155b04a69eb571ee47bd9d6931baf`**, has 11 serialized rule entries. They include separate rarity and currency-rarity lists. Example ordinary/default rarity tuples `(rarity, A07D6F99, CBE06580)` are `(1,1,20)`, `(2,1,40)`, `(3,1,100)`; seasonal types 1–6 use `(1,1,5)`, `(2,1,12)`, `(3,1,24)`. Shared integers include 21 and 50. **No field semantics were established for those integers: they are not verified percentages or pity intervals.** [Rules](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STULootBoxRules.cs), [rarity rule layout](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STULootBoxRarityRules.cs).

Structured pools can be recovered without scanning markers. Soldier's progression has box-type counts **0:61, 1:9, 2:12, 3:7, 4:6, 5:11, 6:11, 8:0**, plus 61 other unlocks. Types 1–6 correspond to Summer, Halloween, Winter, Lunar, Archives and Anniversary. Counted membership does not establish mixing weights, guarantees or eligibility of each “other” item.

The current [extractor](<old-workbench>/tools/assets/hero_content.py:94) instead scans all `-1` reference markers and flattens defaults, event, other and normal unlocks. It is sufficient for the all-unlocked gallery, **not a loot pool**. Preserve the boundaries exposed by [STUProgressionUnlocks](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STUProgressionUnlocks.cs) and [OWLib's progression model](<old-workbench>/OWLib-overwatch-1.74/DataTool/DataModels/ProgressionUnlocks.cs). Add global account pools from player settings, including icons/sprays/currency; hero pools alone are incomplete. Exclude defaults, achievement rewards, golden weapons, OWL/promotion/challenge items unless their specific source permits them.

## Native protocol path

Preserved local [channel/login/loot-box notes](<old-workbench>/Overwatch%201.74%20Lobby%20Protocol_%20Channel%20Setup,%20Login%20and%20Loot%20Boxes.htm) describe these semantics and report retail-route testing. They are reverse-engineering evidence, not Blizzard documentation or a fresh test here. [Original notes](https://rentry.co/3bswrgmw#overwatch-174-lobby-protocol-channel-setup-login-and-loot-boxes). Local [schema data](<old-workbench>/tools/lobby/vendor/data/schemas_174.json) confirms reply layouts; it omits the client progression-out group.

| Direction | Group CRC | Message | Role / evidence |
|---|---|---:|---|
| S→C | `779A8581` | 24300 / `5EEC` | Full account snapshot; already emitted locally |
| S→C | `779A8581` | 24302 / `5EEE` | Append boxes; note describes grant popup behavior |
| S→C | `779A8581` | 24304 / `5EF0` | XP/reward-shaped settlement structure; semantics/UI trigger unverified |
| C→S | `7F4F46CB` | 24201 / `5E89` | Open requested box ID; client group schema still needs importing/verifying |
| S→C | `779A8581` | 24305 / `5EF1` | Reveal results, exact requested box ID, status integer |
| S→C | `779A8581` | 24307 / `5EF3` | Updated credits balance |
| C→S | `7F4F46CB` | 24202 / `5E8A` | Client report after opening; note says no reply owed |
| S→C | `70E5A247` | 24900 | Hero ownership/loadouts plus complete catalog |
| C→S | `B68870B8` | 24500 | Equip hero/item/slot; existing handler replies 24902 |

Use protocol indexes from the client's announcement, never the example numeric indexes in captured notes. Use the existing JAM codec for reply structures and packed booleans. Account offsets below are **relative to the nested record** (`24300 +0x78`), not absolute message offsets:

| Record offset | Meaning |
|---|---|
| `+0x00` | Unopened-box entries: `{id16, type i32, unknown i32}` |
| `+0x30` | Account-wide owned unlocks `{GUID, seen}` |
| `+0x48` | XP, per preserved notes; total-vs-current-level interpretation needs checking |
| `+0x50` | Second i64; notes leave zero, current code sends 2000; next-level threshold is an unverified hypothesis |
| `+0x58`, `+0x60` | Portrait frame and player icon GUIDs |
| `+0x68`, `+0x6C`, `+0x70`, `+0x74` | Level, credits, CP, OWL tokens |

Box inventory is **one entry per box**, each with a persistent 128-bit ID. Its last i32 is not a quantity; preserved capture used 1, exact meaning unknown. Standard type is 0; event/special types differ.

Each 24305 result item has hero GUID (0 for account-wide), unlock GUID, duplicate-credit amount, order key, highlight type, duplicate flag and new flag. The client sorts items and displays the first four valid unlocks. Wrong/missing assets can suppress cards.

Critical opening behavior from the notes: **reply with the exact requested ID**. A mismatched reply can release the screen while leaving its box listed. No reply leaves opening locked, including Escape/Back. Duplicate cards need their duplicate amount/flag and a consistent 24307 balance; new items must persist in the server's canonical ownership and subsequent snapshots. Known consumed IDs should replay their committed result; unknown IDs need a tested failure response that releases UI without granting anything. An empty result is described as releasing the screen, but should not silently consume a legitimate box after an internal failure.

24302 can append boxes at any time. With its integer 4 and bool set, notes describe a queued grant popup shown at a later `ClientInLobbyStore 0x6724`; that store group/trigger is not currently implemented. A snapshot displaying inventory is therefore a smaller first milestone than promising the complete retail grant-popup/XP-animation sequence.

## Smallest complete offline implementation, later

1. **Build a typed static catalog.** Extend the existing asset extraction to keep box/source pools, rarity, hero association, defaults, currency unlock amounts and level-award references. Keep unlock GUIDs separate from skin-theme GUIDs. Reuse existing preload/content-key handling. Do not create another random-item database by hand.
2. **Persist one earned account.** Python's standard-library SQLite is sufficient for atomic updates shared by lobby and game threads. Store account XP/level, balances, owned GUIDs, loadouts, unopened box IDs/types, committed box results and settled match IDs. Keep current all-unlocked research account intact; start a separate earned profile. Do not silently replace corrupt progression with an empty account. Existing equips JSON is useful integration context, not a safe currency transaction model.
3. **Drive all displays from that account.** Login 24300, hero ownership/catalog 24900, Career Profile 20807, party level/frame and reconnect state must agree. Verify live ownership deltas or resend appropriate snapshots at a confirmed refresh point; delta message meanings are not established just by their shapes.
4. **Settle supported matches exactly once.** The local Assault winner is a concrete hook. Capture authoritative participation/result, apply an explicitly chosen XP policy, retain overflow across every crossed level, grant one box per level, update level awards, and commit with the match ID. Keep Practice Range/no-result sessions ineligible initially. Supported vs-AI rewards need an explicit offline policy until final retail multipliers are established. Do not award from damage packets, bot kills or client reports independently. The game thread should queue notifications for the lobby's writer; current TCP read blocking means unsolicited rewards need a wake-up/drain path.
5. **Open boxes transactionally through native UI.** Validate profile and box ID, generate four valid rewards from the appropriate typed pool, grant at least Rare, apply a documented duplicate policy, atomically remove the box/update ownership and credits/save the result, then send 24305 and balance updates. Retry/reconnect reuses committed results and never rerolls or pays twice. Generation at grant-time versus first-open time is not historically established; first-open generation committed before reply is a minimal explicit reconstruction choice.
6. **Add credit purchase handling.** Current not-for-sale catalog entries need usable prices; client purchase request/response semantics remain unverified. A server transaction validates item source, price, currency, ownership and availability before deducting/granting, then updates native UI. Equip must reject unowned/wrong-hero/wrong-slot items. Cosmetic source restrictions remain distinct from permanent ownership.

This order avoids building Battle.net, a paid shop, ranked matchmaking, a battle pass, synthetic endorsement farms or new UI. Event/Arcade/challenge tracks can follow the basic XP→box→unlock→equip loop; they are separate work, with some historical counters/reset rules still missing.

When implementation begins, minimum meaningful checks: multiple-level XP overflow, repeated match settlement, repeated box-open request, restart between commit/reply, exhausted rarity pool, mixed event/base rewards, currency drops versus duplicates, insufficient-funds purchase and unowned equip. Later client session should check four-card reveal, balance, ownership after relog, portrait/party/profile agreement and invalid-ID recovery. None of these live checks ran in this research task.

## Remaining evidence gaps

- Exact early XP curve; account XP field interpretation; final per-mode formula, time unit, medal rule, bonus order/rounding, AFK rules and daily reset.
- Loot rarity probabilities, pity counters, per-rarity duplicate protection, event/base mixing, direct-currency chances and generation timing. Hashed integer tables are leads, not a probability specification.
- Gallery purchase contracts, price overrides and effective availability. 24503's three-u64 shape alone does not establish a purchase operation.
- Native XP/level-up animation, settlement 24304 semantics, grant-popup store trigger and error behavior.
- Arcade intermediate thresholds/reset zone, endorsement reward schedule and full achievement/promotion/event conditions.

Graphify was queried first using its actual vocabulary (`progression unlocks account schemas`); relevant graph nodes pointed to the current account/extractor/codec paths, which were then checked directly because graph line numbers lagged recent edits. Read-only idalib search found `UnacknowledgedOpenedLootboxCount` at dump VA `0x7FF6D381AE80`, but its available xref was data-only; no opening handler or economy calculation was established. No IDB comments, types, names or bytes were changed or saved.

Asset research used a disposable scratch inspector in `.scratchpad/progression/`; durable findings are recorded above. No production implementation or runtime verification was performed. Principal risk: a plausible offline economy can be client-compatible while differing from historical retail XP/drop behavior.
