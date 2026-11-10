# Bot movement: jitter and wall clipping (2026-11-06)

All server bots (Range training bots, Practice vs. AI heroes, Uprising omnics) jittered and lurched across the map, and some walked through walls. Two separate causes.

## 1. Jitter: frame numbers drifted from wall time

- The client's command clock runs on wall time: 7 ms frames (`TOURNAMENT_QUANTA`, 142.9/s) in our matches, measured 142.93 frames/s over 28 s.
- The server stamped each world frame `tick * 16000 // quanta`, with `tick` counted once per loop pass. On this machine the loop makes about 60.4 passes/s, not 62.5, so the stamped frames fell 3.4% behind wall time. The client's lead over our frames grew from 187 frames (1.3 s) to 357 (2.5 s) in 34 s. It starts large because the tick counter runs from lobby start.
- Bot positions are computed from wall time (`now`), so every movement state was stamped with a frame that was too old by a growing amount. The client then drew the bots far off their real line, overshooting and snapping back. It got worse the longer the match ran.
- Fix (`server.Client._payload`): a gameplay frame is `FIRST_TICK + round((now - match.started) / quanta)`, the same clock the bots move on. The tick count no longer matters.
- Clip: [bot-movement/bot-jitter-before-after.webp](bot-movement/bot-jitter-before-after.webp) (left before, right after).
- Measured with one test bot walking a straight 10 m line across the view, tracked on screen (red-ring centroid, ~13 fps capture): mean frame-to-frame jitter (|second difference| of screen x) went from 66 px (max 247) to 8.6 px. What remains is the turnaround at the line's ends. Sending zero velocity made no difference (10.2 px), so the velocity field stays as retail sends it.

## 2. Clipping: strafes and straight patrol legs ignored walls

Found with `tools/research/bot_walls.py`, which runs a match headlessly and checks every per-tick move against the solid collision mesh at chest and head height.

- Hanamura vs. AI, before: 226–292 wall crossings in 120 s, 98% from `Match._strafe`. A strafe took the nav node nearest to a 2 m sidestep and walked straight to it, across any wall in between. Fix: the strafe holds unless `nav.passable(mesh, bot, node)`.
- Practice Range: patrols walk straight lines between the map's hint points. Bot 408's route passes through a ~1 m solid block and more geometry (all of its legs, and the walk from its spawn), and one leg of bot 506 is blocked. Fix: `nav.loop` replaces each blocked leg with a smoothed nav-grid route and starts the loop at the point nearest the bot's spawn that it can walk to straight. The Range now has a nav grid (`data/0688.nav`), covering `target.patrol_points()` and pre-built by run.py (~12 s, once).
- Grid routes zigzag through 1 m cells and the bot snaps its facing at every cell. `nav.smooth` drops a corner when the bot can walk straight past it and the floor stays within `LEVEL` (0.3 m) of the line every half metre. That last rule keeps bots from sinking into stairs or gliding off ledges; a looser 0.6 m check left bots up to 1.3 m off the floor. Smoothing costs about as much as the A* itself (~11 ms per path on Hanamura).
- `nav.passable` checks knee, waist and head lines (0.7, 1.1, 1.6 m). The waist line was added after a smoothed shortcut crossed a waist-high obstacle on King's Row. Nav cache `VERSION` 3: old grids rebuild on first use.

After: 0 crossings on Hanamura (3 seeds), King's Row Uprising (2 seeds) and the Range. Ticks off the floor are back to the pre-change baseline (0–4: 3 m drop links). Live in vs. AI (Hanamura, Medium) the bots walk, strafe and fight in the point room without lurching.

## Limits

- Knee-height (0.45 m) checks still see stair risers: bot height is interpolated linearly between route points.
- A bot that walks off a drop link (up to 3 m) still glides down across about 1 m instead of falling.
- The match uses the unfiltered mesh for these checks, so strafes and shortcuts also stop at non-solid barriers (spawn-exit triggers) that the nav links pass through. That is stricter than needed, never looser.
- Uprising was checked headlessly only, not on screen; it goes through the same frame and steering code.
