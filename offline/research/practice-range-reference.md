# OW1 Practice Range reference for retail 1.74

Scope: original Overwatch through retail 1.74 (2022), not the expanded Overwatch 2 range. Target installation is `1.74.0.0.104319`. This is a source-backed feature brief; it does not assert a particular asset layout or bot count.

## What OW1 supports

- The range’s documented purpose is safe hero practice. In a May 2016 interview, game director Jeff Kaplan described it as a place to play any hero “without fear of dying.” Blizzard’s Dec. 2019 patch notes describe it as solo or group practice against training bots. This supports a selectable-hero sandbox, not a PvP match loop. The sources do not document the exact hero-select interaction or any objective/score/win condition. ([Time, May 23, 2016](https://time.com/4344566/overwatch-interview/); [Blizzard, Dec. 10, 2019 patch notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2019/12/))
- Bots are shootable, destructible targets: Blizzard’s Oct. 2016 notes refer to their health bars and Ana’s Biotic Grenade effect; Mar. 2017 notes say destroyed training bots appear in the kill feed. Nov. 2016 notes confirm AFK players are removed from the range. These notes do not establish target counts, their precise placements, or a full spatial layout. ([Blizzard, Oct. 2016 patch notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2016/10/); [Blizzard, Mar. 2017 patch notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2017/03/); [Blizzard, Nov. 2016 patch notes](https://overwatch.blizzard.com/en-gb/news/patch-notes/live/2016/11/))
- Bots support some ability demonstrations. The June 2017 PTR notes say player abilities could move training bots, and that they should face correctly and return to their original location afterward. May 2018 notes describe the same reset behavior after ability hits. July 2018 notes explicitly mention friendly training bots and fix Moira’s Healing Orb not healing them. Thus, a damage/shooting demo against target bots and a separate support-healing demo on friendly bots are source-supported. The sources do not establish friendly-fire shooting or that every heal/utility ability works on them. ([Blizzard, June 22, 2017 PTR notes](https://overwatch.blizzard.com/en-us/news/patch-notes/ptr/2017/06/); [Blizzard, May 3, 2018 notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2018/5/); [Blizzard, July 2018 notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2018/07/))
- Respawning target bots is supported, with a historical reliability caveat: Feb. 2020 patch notes fixed an issue where training bots sometimes failed to respawn. The Oct. 2016 notes also confirm bot health bars. Exact HP, respawn delay, and whether all bot classes respawn identically are not established by these sources. ([Blizzard, Feb. 2020 patch notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2020/02/); [Blizzard, Oct. 2016 patch notes](https://overwatch.blizzard.com/en-us/news/patch-notes/live/2016/10/))
- Through 1.74-era documentation, treat the range as a modest practice sandbox against bots. Do not plan around modern selectable target-distance/movement settings, dozens of drills, or the expanded bot roster. Blizzard explicitly called the Aug. 2023 OW2 firing range a “brand-new” addition and the first major Practice Range update; it added selectable targets/bots, adjustable distance, movement, and practice settings. ([Blizzard, OW2 Invasion announcement](https://overwatch.blizzard.com/en-us/news/23964186/introducing-our-most-exciting-season-yet-overwatch-2-invasion/); [Blizzard, Invasion launch article](https://news.blizzard.com/en-us/article/23989473/overwatch-2-invasion-is-here-with-new-adventures-a-new-hero-and-new-ways-to-play))

## Build implications

Treat Kaplan's “without fear of dying” phrase as design intent, not proof of player invulnerability. Player health, damage and death rules still need direct verification for this build.

For a faithful first slice, prioritize hero selection, firing at respawning bots, and one friendly bot that can receive healing. Ability-driven bot displacement/reset is a useful secondary demo. Keep map-space claims and target/bot counts provisional until confirmed from 1.74 assets or original 2016–2022 footage; first-party patch notes verify mechanics but not the range’s room-by-room layout. There is no source-backed need for objectives, scoring, configurable bot AI, adjustable distance, or a broad bot roster.

## Local project comparison

- The beta fork's `<beta-project>\repo\Prometheus-Core\Fixes\PracticeMode.h:7-15` explicitly describes restoring **Practice vs AI** by switching to a selectable/random playable map and spawning enemies. `PracticeMode.cpp:33-37,77-83` supplies PvP map IDs and chooses among them. This is useful lifecycle/bot reference code, not an implementation of the original Practice Range map.
- The existing Graphify graph points to `Practice vs AI local match flow` at `repo/CHANGES.md`, `L431`, and the practice automation at `tools/practicescene.py`, `L1`. Those labels establish the same distinction, not retail resource compatibility.
- Read-only access to retail CASC root/encoding metadata remains verified by matching content hashes. English manifest entries inspected include `Manifest/Win_SPWin_RDEV_LenUS_text_EExt.cmf` and its speech counterpart. The beta `tools/owindex.py:21` instead hardcodes an APM content key and beta manifest/package layouts. Its GUID index cannot be assumed to cover retail unchanged.
- No plain `practice`/`training` filename appeared in the top-level root manifest. That manifest is not a map-name catalog; this is **not evidence that the map is absent**. The retail Practice Range header GUID, resource dependencies and native mode entry path remain unidentified.

## Initial acceptance target

1. Main menu → Training → Practice Range entry, or a documented local equivalent while the tournament menu is adapted.
2. Load the original range's geometry, collision and player spawn; control Soldier: 76; leave and return to the working menu.
3. Shoot one stationary hostile training bot; observe health loss, destruction and respawn.
4. Restore a moving target and a friendly healing target, then investigate remaining scripted bot/ability interactions.

Steps 1–2 are the immediate map/hero milestone. Full original-range fidelity is a later acceptance pass against build-specific assets/footage. No gameplay code or new client launch was performed during this research.

## Source limits

The official patch-note archive is sufficient to establish bot interactions and later feature boundaries, but not detailed geometry, exact target count, movement pattern, health values, respawn timing, or hero-switch UI. Search surfaced a 2016 beta-note excerpt in a Blizzard forum discussion, but not the original Feb. 9, 2016 patch-note page; this report therefore does not rely on it to date the feature’s introduction. Contemporary gameplay-video search results were available, but their video pages could not be fetched for frame-level verification, so no layout claim is attributed to them.
