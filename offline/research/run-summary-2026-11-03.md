# Unattended run summary (2026-11-02 to 2026-11-03)

Practice vs AI plan, steps 1-8, plus one bonus item. Every step that changed code was checked in game and has its own local commit. Nothing was pushed. No TEMP probes are left in the tree, and Overwatch.exe is not running.

## Done

| # | Step | Commit | Evidence |
|---|---|---|---|
| 1 | Bot projectiles fly at their wiki speed and can be dodged | 8a327a0 | tests |
| 2 | Hero bots strafe while they fight | 0126417 | `assault-capture/bot-strafe-sheet.png` |
| 3 | Allies gather round the player near the point | 84e4da8 | `assault-capture/allies-follow-log.txt` |
| 4 | Visible fire for 11 of the 12 heroes (ch2 shot events) | 77b3028 | `assault-capture/bot-*-sheet.png`, `bot-ana-darts.png` |
| 5 | Capture HUD: research only, the padlocks are still unexplained | 2cc5ebc | `research/assault-mode-graph.md` |
| 6 | Healer bots heal (Ana, Zenyatta, Lúcio), server-only | d0f6e75 | `assault-capture/bot-heal-log.txt` (80 to 200 HP in 0.8 s) |
| 7 | One signature ability per hero bot, server-only | 28350b4 | `assault-capture/bot-ability-log.txt` (20 uses in a minute) |
| bonus | More maps: Temple of Anubis, Volskaya Industries and Horizon Lunar Colony Assault | f19d8d7, eeb14d2 | `assault-capture/{anubis,volskaya,horizon}-attack.png` |

## Notes

- **Abilities:** each is one of five kinds (`target.Ability`):
  - Mends: Take a Breather, Cryo-Freeze and Self-Repair.
  - Guards: Particle Barrier and Overload.
  - Bursts: Flashbang, Helix, Biotic Grenade and Soundwave.
  - Leech: The Reaping.
  - Discord: Orb of Discord.
  Sombra has none. Nothing is drawn yet; the shortcuts are listed at `target.ABILITIES`.
- **Capture HUD:** these vars changed nothing, on the objective entity or on the mode entity:
  - graph 1824's presenter vars 11947-11949 and 17560-17562;
  - 1030, 18108, 16928, 26613, 12600 and 25460.
  The next lead is decoding the `m_expression` bytes of the presenters, to see which entity each one reads.
- **Maps:** the new maps needed a collision-parser fix. Some chunks carry extra record blocks after the triangles. Paris still fails (a triangle index past the vertex block).
- **Known issue, not new:** the health number reads `0/200` while the bar is correct (`tasks/roadmap.md`).

## Left / next

1. Capture-progress HUD (the padlocked icons).
2. Round-complete screen score and time, and a victory/defeat screen.
3. Difficulty chosen from the game menu (the `--difficulty` flag works today).
4. A player hero other than Soldier (scope only).
5. Drawing abilities and heals, and Zarya's beam.
6. Paris collision.
