# Hero Gallery purchase contract

Research date: 2026-11-08. Target: retail PC 1.74.0.0.104319. Read-only source, preserved capture and static IDA/data inspection. No game/UI/live-process access, production edits or actual progression-database access.

## Corrections needed before implementation

**Purchase is client message 24203 in PROGRESSION_OUT `0x7F4F46CB`, carrying the unlock GUID at `+0x78`.** It is not gallery message 24503. **Store price is `u16 +0x14`; `u32 +0x10` is unlock level.** Earlier `research/hero-menu.md:114` labels `+0x10` as price; that label is incorrect. The existing store-entry values already match the intended default/not-for-sale forms, but a purchase implementation must put its price at `+0x14`.

Evidence: pinned upstream [gallery handler](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/lobby/handlers/gallery.py), [shop](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/services/shop.py), [collection](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/content/collection.py), [schema](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/data/schemas_174.json).

## Wire contract and response order

Offsets are in decoded native message objects, not raw wire byte positions. JAM/schema encoding and dynamically announced group indexes remain required.

| Direction/group | Message | Fields | Evidence limit |
|---|---:|---|---|
| C→S PROGRESSION_OUT `7F4F46CB` | 24203 | `u64 +0x78 = unlock GUID` | Upstream registered purchase route; 1.74 static schema confirms shape |
| S→C PROGRESSION_IN `779A8581` | 24306 | `u64 +0x78 = unlock GUID`, `u64 +0x80 = hero GUID or 0` | Upstream purchased-unlock response and tests; order matters |
| S→C HERO_CATALOG `70E5A247` | 24901 | `u64 +0x78 = hero GUID`, `u64 +0x80 = unlock GUID`, `bool +0x88 = new` | General hero-item grant, not the same field order as 24306 |
| S→C PROGRESSION_IN | 24301 | `u64 +0x78 = account unlock GUID`, `bool +0x80 = seen` | General account grant; upstream sends `False` for newly granted item |
| S→C PROGRESSION_IN | 24300 | Full account/progression snapshot | Upstream refreshes all three currency balances after purchase |
| C→S GALLERY_OUT `B68870B8` | 24503 | Three `u64`: `+0x78`, `+0x80`, `+0x88` | Shape known; semantic purpose not established here |

Pinned handler order: validate/charge/grant/save canonical profile; send **24306 for the bought item**; send general grants for any bundled partner items; send **24300** with updated balances. Its tests assert that 24306's first GUID is the unlock. The collection source explicitly warns that sending hero first caused the client to charge the player while leaving the item locked. These are upstream compatibility evidence, not a new local runtime test. ([Handler](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/lobby/handlers/gallery.py#L72), [response construction](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/content/collection.py#L284), [purchase packet tests](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/tests/test_lobby_purchase.py))

No error-status field exists in 24306's extracted schema. Upstream rejects invalid/owned/unaffordable purchases by logging and returning without reply. This does **not** establish correct native failure handling or whether the client waits indefinitely. Invalid-ID/insufficient-funds recovery remains a runtime test; do not invent a zero-GUID failure response and call it verified.

Exact additional PROGRESSION_OUT shapes from the pinned schema: **24200 has no fields**. **24201** has one struct at `+0x78`, size 16, containing a fixed pair of u64 at inner `+0x0` (box ID). **24202** has one struct at `+0x78`, size 40: inner `+0x0` is the same 16-byte ID struct; inner `+0x10` is a variable array of 16-byte entries `{u64 +0x0, bool +0x8, bool +0x9}`. The GUID/boolean meanings in those receipt entries are not established. The group includes IDs 24200–24227; this pass inspected only 24200–24203. Do not populate unchecked messages with placeholder schemas or use 24202 to authorize new rewards.

## Store record

Each `24900 +0x98` outer entry has an item array at `+0x0` and its hero GUID at `+0x18`. The 24-byte item record is:

| Offset | Type | Meaning / qualification |
|---|---|---|
| `+0x00` | `u64` | Item unlock GUID |
| `+0x08` | `u64` | Achievement association when nonzero; direct 1.74 cross-check below |
| `+0x10` | `u32` | Unlock level; `65535` means not a level reward |
| `+0x14` | `u16` | Purchase price; 0 on defaults and unsold items |
| `+0x16` | `i8` | Box/event source; upstream uses `-1` for items never in boxes |
| `+0x17` | `bool` | Default flag; defaults are excluded from gallery unlock counts |

The local [1.74 schema](<old-workbench>/tools/lobby/vendor/data/schemas_174.json:463) confirms widths/offsets. Semantic names come from the pinned [shop](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/services/shop.py#L1) and [collection implementation](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/content/collection.py#L106), plus the captured records. Price does not encode the currency kind. Upstream chooses credits for ordinary items, CP for WeaponSkin and tokens for OWL-category items; those classification policies are distinct from the wire layout.

Capture provenance matters: upstream [RetailTemplates](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/catalog/templates.py#L1) identifies its templates as **retail 1.68 recordings decoded with 1.74 schemas**; changed groups were excluded except manually converted ranked state. This is not a captured final-2022 store inventory.

The pinned [public template](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/data/retail_templates.json) contains 5,968 hero-store entries: 5,710 nondefaults have unlock level 65535; 258 defaults have level 0. There are 64 nonzero `+0x8` associations. Example: unlock `0x0250000000000761` associates achievement `0x0E60000000000164`, with price 0, level 65535 and box type -1. Ordinary priced entries instead have `+0x8=0`, level 65535, positive `+0x14` and an appropriate source. These observations support separating achievement/source eligibility from pricing.

Direct local 1.74 verification: `python -B tools/assets/stu_graph.py 0x0E60000000000164 --width 450` identifies root class `0x0CC07049` as **STUAchievement**. Decoding field `0xF5087894` (`m_unlock`) with existing `stu_header_crc64`/`_decode_asset_ref` yields **`0x0250000000000761`**; an equality assertion passed. Thus this capture's `+0x8` achievement association independently matches a current-build asset. Definitions: [STUAchievement](<old-workbench>/OWLib-overwatch-1.74/TankLib/STU/Types/STUAchievement.cs:28), [achievement enumeration](<old-workbench>/OWLib-overwatch-1.74/DataTool/ToolLogic/List/ListAchievements.cs:39), [reference decoder](<old-workbench>/tools/assets/retail_bundle_lookup.py:68).

Defaults retain `{+0x8:0,+0x10:0,+0x14:0,+0x16:-1,+0x17:True}`. Unsold nondefaults use `{+0x8:0,+0x10:65535,+0x14:0,+0x16:-1,+0x17:False}` when no achievement association applies. For an ordinary purchasable cosmetic, put the validated price into `+0x14`; do not replace the unlock-level sentinel with that price.

## Ownership nuance

Upstream's current implemented split uses `hero_of`: a hero-tagged icon/spray is listed only under its hero; an item without a hero goes in account ownership. A source comment reports that duplicating a hero icon into the account-owned list prevented the client from sending its equip request. The file's earlier blanket `ACCOUNT_LEVEL_TYPES` comment conflicts with this later implementation; do not treat every icon/spray as account-only. Verify the corrected split on the local client. ([Owned lists](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/ow174/content/collection.py#L166))

## Static IDA evidence and remaining limits

### Native ownership override and existing CONFIG transport

The tournament preset enables a separate cosmetic ownership override at **RVA `0x2E336F0`**, VA `0x7FF6D40536F0`. Preset function `0x7FF6D2A27F10` writes 1 at instruction `0x7FF6D2A27F6D` (`C6 05 7C B7 62 01 01`). Clearing this byte restores ownership membership checks without clearing TournamentMode at RVA `0x2E2F448`.

The existing **36600 CONFIG** message can clear it with **key `0x2704A3F4`, string value `"0"`**. Static chain in the preserved IDB:

- Dispatcher `0x7FF6D19C9DD0`, case `0x8EF8` (36600), calls `0x7FF6D1997B10`.
- Handler iterates the message's `+0x78`/`+0x80` key/string array and calls registry setter `0x7FF6D2A04940`.
- Setter matches the object's `u32 +0x10` key. Target object `0x7FF6D40536D0` contains key `0x2704A3F4`, restriction field `u32 +0x14 = 0`, and bool value at `+0x20 = 0x7FF6D40536F0`. Its zero restriction permits the runtime setter.
- Bool vtable slot 5, `0x7FF6D2A02AD0`, calls parser `0x7FF6D2AE7930` on object `+0x20`; string `"0"` or case-insensitive `"false"` writes false. Adjacent key `0x2069C32C` belongs to a different object and must not be used.

Hero ownership predicate `0x7FF6D20424C0` and account ownership predicate `0x7FF6D2042FD0` return true immediately when the override byte is nonzero; otherwise they search actual unlock-array membership. Gallery record builder `0x7FF6D1C3F610` and selected-item action `0x7FF6D1C3B080` also read the same override directly. Thus changing one predicate alone would leave other Gallery paths bypassing ownership. No ASI or image-code patch is required for this override.

**Live observations reported by the primary agent, not performed by this research agent:** CONFIG `0x2704A3F4:"0"` changed native ownership override to `00` while TournamentMode remained `01`. A native 75-credit purchase then reduced credits from 1,000 to 925 and added one owned unlock; after relog, Gallery showed 925 credits and `1/20`. These observations confirm this local implementation's successful purchase path; invalid-purchase recovery remains unverified here.

Read-only idalib session used existing [Overwatch-state-analysis.dump.i64](<old-workbench>/ida/Overwatch-state-analysis.dump.i64), image base `0x7FF6D1220000`, with auto-analysis disabled. Registration/schema records were read directly:

| Message | Registration VA | Field metadata VA |
|---|---|---|
| 24203 | `0x7FF6D3EE6B00` | `0x7FF6D3EE71D0`: u64 at +78 then terminator |
| 24306 | `0x7FF6D3ED1950` | `0x7FF6D3ED1A10`: u64 at +78/+80 then terminator |
| 24901 | `0x7FF6D3ECCC90` | `0x7FF6D3ECCD10`: u64 at +78/+80, bool at +88 then terminator |
| 24503 | `0x7FF6D3EE3230` | `0x7FF6D3EE2FC0` |

No handler xrefs were available for these registration objects. `purchaseRequest` string xrefs led to a web-checkout function, not a proven Hero Gallery handler. No message semantic claim above relies on those unrelated strings. Session closed with `save:false`; no IDB renames/comments/types/patches saved.

Early XP thresholds were not recovered. Pinned upstream profile/collection code hardcodes displayed XP/level defaults rather than implementing an XP curve; its tree has no progression/experience module. Existing PlayerProgression asset definitions do not name an XP threshold table. Retain the earlier report's uncertainty.

No upstream code was executed/imported. The public source's equip validation, silent purchase failure and unreadable-profile/reset policies are not recommended implementation patterns. Local successful-purchase compatibility is reported above; failure recovery remains unverified here.
