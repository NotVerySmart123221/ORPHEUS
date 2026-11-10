# Earned offline accounts — 2026-11-08

Implemented XP → box → unlock → equip/purchase behind `--progression`. Default all-unlocked research sessions retain their old account/equips. Existing work from the earlier agent was preserved. No real account/service or settings backup was changed.

## Implementation

- `tools/assets/progression_catalog.py` separates hero/global box pools, defaults, other-source cosmetics and portrait-frame awards. Build-104319 payloads are content-hash checked; the existing login keyring decrypts required bundles. Cache: 6,603 item records and 301 frame references.
- `tools/lobby/progression.py` persists named SQLite profiles. Match/opening receipts commit with credits, ownership and inventory. Repeated/concurrent requests replay results; failed saves roll back. Damaged existing account rows/tables/inventory never trigger a fresh profile.
- Actual Assault completion queues settlement for the lobby writer, including while TCP is idle. Database failures retain settlement for retry. The game thread does no database work. Practice Range, abandonment, Skirmish and unfinished PvE award none.
- Login, party and career share level/frame/icon state. Gallery retains its complete catalog while ownership contains only defaults/earned items. Equips validate ownership, hero and wheel index. Sparse wheel slots survive relog; player icons persist separately from hero loadouts.
- Native opening uses 24201 → 24305 with the same persistent box ID, then ownership/balance updates. Save failure releases reveal and restores retained inventory through a readable snapshot; otherwise inventory returns on relog.
- Credit purchases use 24203 → 24306 `{unlock, hero}` plus ownership/balance updates. Store price is `u16 +0x14`; `+0x10` is unlock level.

## Native ownership override

Tournament mode enables a separate unlock-all cosmetic setting. Reduced owned lists alone left Gallery at N/N with UNLOCK disabled. Static investigation found the flag at RVA **`0x2E336F0`**, registry key **`0x2704A3F4`**. CONFIG 36600 accepts text `"0"` for this bool.

Earned login now sends that entry through the existing protocol. Read-only inspection of the live QA process confirmed **cosmetic override 00, TournamentMode 01**. Gallery then displayed actual earned ownership, locks, prices and an active UNLOCK button. No new ASI or executable-code patch was needed. Ordinary all-unlocked sessions omit this config. [Purchase contract research](purchase-contract-2026-11-08.md).

## Native verification

An isolated `XPVerify` profile received a synthetic 120-second completed-match win: **2,035 XP**, level **2**, overflow **35**, one box. This seeds a test fixture; no full live Assault match was played.

1. Menu showed level 2 and one box: [menu](earned-menu.png).
2. Native opening sent group `7F4F46CB`, offset 1, 18 bytes. Four rewards rendered, including a Rare player icon. Stored ownership increased **317 → 321** and inventory became empty: [reveal](box-reveal.png).
3. Fresh client retained level 2 and no box badge. Config correction restored reduced ownership and native locks/prices: [gallery](earned-gallery.png), [75-credit locked skin](locked-skin-price.png).
4. Purchase QA seeded this profile with **1,000 test credits**. The user clicked the native UNLOCK confirmation for Junkrat's Bleached skin. Client sent group `7F4F46CB`, offset 3, 10 bytes. Stored credits became **925**, ownership **321 → 322**. Screen showed **1/20 skins**, NEW on Bleached and EQUIP enabled: [purchase](purchased-skin.png).
5. Another fresh client retained **925 credits and 1/20 skins**: [purchase after relog](purchase-relogged.png). The prior purchase session exited normally with code 0.

Both Windows capture APIs timed out despite app approval. Existing `frame_grab.asi` captured the D3D back buffer; native app keyboard navigation handled menus. The purchase-button click/confirmation was supplied by the user. Evidence images preserve capture pixels in lossless PNG. No ordinary profile database or saved equips was changed.

## Explicit offline policy

First level: 2,000 XP; later levels: 20,000. Match awards: 1.75 XP/second +75 finish +250 win +1,500 first win per UTC date. Overflow persists. Backward clock changes do not repeat the daily award.

Rarity odds per slot (since 2026-11-09, from LizRollin's loot box code; research/lizrollin-lobby.md section 4): Common **83.09%**, Rare **10%**, Epic **4.98%**, Legendary **1.93%**, with one slot Rare or better, shown in a random position. Before: 75:20:4:1 with the first slot Rare+. Prefer unowned items within rolled rarity; duplicates pay **5/15/50/200** credits. Direct currency uses asset amounts **50/150/500**. These are documented approximations, not the complete retail server formula/distribution. No medals, group/AFK multipliers or retail pity system are claimed.

Seasonal/Arcade/endorsement/challenge/CP/OWL reward tracks remain outside the basic implementation. Native XP settlement animation and grant-popup timing are not implemented; account state updates directly. Real Assault completion/once-only delivery is covered through the actual Match path in tests.

## Validation

`python -m pytest tools/lobby tools/assets tools/research -q`: **384 passed**. A temporary mutation disabling box-receipt replay was caught by its regression; the original source was restored byte-for-byte before this full run.

Coverage includes multiple-level overflow, first-win/reset/backward clocks, repeated settlement, concurrent/replayed opens, rollback through a real SQLite trigger failure, failure-then-retry delivery, unknown box IDs, corrupt/missing account state, currency rewards, purchase double-charge prevention, invalid equips, sparse wheels, global/hero icons, typed array bounds/content hashes, native override isolation and launcher opt-in forwarding.
