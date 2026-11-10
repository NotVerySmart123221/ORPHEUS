# Hero weapons: what is served and what was seen (2026-11-04)

Per hero: the weapon graph, its model, primary fire, reload (and its animation), the right mouse and quick melee as the
local server serves them (`tools/lobby/kits.py`, `soldier.py`, `target.py`), and what the in-game sweeps showed
(`tools/research/weapon_sweep.py`, frames in `.scratchpad/ws/<hero>`, session `20261104T074455`). Facts from the catalog:
`tools/assets/weapon_kit.py` (`reload()`, `alt()`); wiki numbers: `hero-weapons-wiki.md`; melee: `melee.md`.

The [roster animation audit](roster-animation-audit.md) records the later timestamped primary, secondary, damage and ability captures for all 32 selectable heroes.

Logical buttons in the weapon graphs: 20 primary fire (bit 0x01), 29 secondary fire (0x02), 14 reload (0x400),
31 quick melee (0x800, graph 67 for every body), 11 scope toggle (Ana, Widowmaker), 37 ultimate (0x20).

## Columns

- **Model**: the weapon's CosmeticEntity state is on and var49 bound to it; seen in first person.
- **Fire**: the server runs the magazine and pulses the WeaponVolley; the client draws the shot and recoil pose.
- **Reload**: `Kit.reload`: the "reloading" switch's branch, its Stack and the reload Anim (alias `0x0C80000000000004`) on
  while the magazine refills, with var51; "-" = no magazine (`target.INFINITE`).
- **RMB**: what the right mouse is in the graph; "alt" = a weapon volley served by `kits.AltFire` (`target.ALT_KIT`);
  "ability" = a cooldown StateAbility (served as HUD only, `kits.ABILITIES`); "scope" = a camera mode (not served).
- **Melee**: graph 67, served for all; "seen" where a sweep grabbed the swing.

| Hero | Graph | Model | Fire | Reload anim | RMB | Melee | Seen in game (sweep) |
|---|---|---|---|---|---|---|---|
| Soldier: 76 | 596 | yes | yes (automatic) | st39 (hand-mapped) | Helix: ability volley, served | yes | earlier sessions; fire this session (6 shots) |
| Cassidy | 0x01D2 | yes | yes | st9 | Fan the Hammer: alt, volley 11 | yes | fire; **fan seen** (3 shots at 0.128 s, recoil pose); **melee seen**; **reload anim seen** (cylinder out, reloaded) |
| Roadhog | 0x01C6 | yes | yes | st6 | alt, volley 12 | yes | fire (muzzle burst); **reload anim seen** (gun raised, shell fed); **alt seen** (kick, smoke) |
| Torbjörn | 0x1683 | yes | yes | st8 | alt (shotgun), volley 13 | yes | fire; **reload anim seen** (molten rivets); **alt seen** (muzzle ring, 3 rounds spent) |
| Zarya | 0x0241 | yes | no Volley (beam) | st6 | grenade: alt, volley 16 | yes | primary drains ammo, no beam; **alt seen** (launch ring, 25 rounds); **reload anim seen** (cannon vents) |
| Baptiste | 0x1932 | yes | yes | st17 | heal grenade: alt, volley 26 (no effect) | yes | fire (muzzle flash); **reload anim seen** (magazine swap); alt pulsed and accepted, no grenade seen at sheet scale |
| Ashe | 0x14FC | **yes (fixed: var16922)** | **yes (fixed: switch 96 true)** | st69 (start only) | ADS: same volley 7; not served | yes | model seen; **fire seen** (3 shots at 0.256 s, recoil); **melee seen**; reload anim not checked |
| Ana | 0x0B94 | yes | yes | st8 | scope (button 11/29) | yes | - |
| Widowmaker | 0x001B | yes | yes | st33 | scope (buttons 11/29); scope anims 66/67 | yes | - |
| Sombra | 0x0B1B | yes | yes | st17 | Hack: ability | yes | - |
| Mei | 0x05E6 | yes | yes | st6 | icicle: alt, volley 16 (hand-mapped, 0.4 s windup) | yes | fire 13 shots; **reload anim seen**; **melee seen** |
| Bastion | 0x012E | yes (no CosmeticEntity) | yes | st8 | Self-Repair: ability | yes | - |
| Lúcio | 0x0776 | yes | yes | st9 | Soundwave: ability | yes | - |
| Reaper | 0x0019 | yes (two guns) | yes | st13 | none | yes | - |
| Zenyatta | 0x0164 | yes | yes | st8 | orb volley: button 16 sets var1592; release -> switch 22 false -> volley 18 (charged alt, 2026-11-06); var1945 = orbs | yes | **volley seen** (orbs leave the hands, `main-menu/alt-zenyatta-volley.png`) |
| Tracer | 0x0016 | yes | yes | st9 | none | yes | - |
| Pharah | 0x0023 | yes | yes | st9 | none | yes | - |
| Junkrat | 0x022B | yes | yes | st5 | none (mine is Shift) | yes | - |
| Orisa | 0x0BD4 | yes | yes | st5 | Halt!: ability | yes | - |
| Wrecking Ball | 0x1514 | yes (no CosmeticEntity) | yes | st8 | Grapple: ability | yes | - |
| Echo | 0x19E5 | yes (no CosmeticEntity) | yes | st24 | Sticky Bombs: ability | yes | - |
| Mercy | **0x006B (staff; was the blaster 0x006D)** | yes | no Volley (beams) | - | none | yes | **staff seen** (held, melee swing); no weapon switch, so no blaster |
| Hanzo | 0x00B9 | yes | yes (full charge) | - | buttons 29 -> same volley (draw cancel) | yes | - |
| D.Va | 0x02B8 (pilot) | yes | yes | st15 | none (mech graphs not in the catalog) | yes | - |
| Sigma | 0x187F | yes | yes | st5 (never: no magazine) | Barrier: ability | yes | - |
| Symmetra | 0x157C | yes | no Volley (beam) | st9 | orb: alt, volley 34 under StateAbility 40 (2026-11-06) | yes | **orb seen** (flown child `0x02EF`, hit a bot for 120, `main-menu/alt-symmetra-orb.png`) |
| Moira | 0x0F76 | yes (no CosmeticEntity) | no Volley (beam) | - | damage beam: button 28 -> Stack 27 -> TrackTargets 96, switch 22 true (false reaches undecoded `STU_9932D728`); served 2026-11-06 | yes | drain seen in the log; no beam drawn |
| Genji | 0x0304 | yes | **yes (fixed: button 38 under switch 12)** | st7 | fan: not found | yes | **fire seen** (throw animation, 3 shots at 0.29 s) |
| Doomfist | 0x0D0A | yes (no CosmeticEntity) | **yes (fixed: button 6 under switches 1, 13)** | - | Rocket Punch: ability | yes | **fire seen** (pellet tracer); **melee seen**; the CANCEL prompt (Rocket Punch's always-on `60F7` presenter) is gone since 2026-11-06 |
| Reinhardt | 0x0048 | yes | no Volley (hammer) | - | Barrier: ability | yes (generic, not the hammer) | - |
| Winston | 0x00FE | yes | no Volley (beam) | st8 | none | yes | - |
| Brigitte | 0x1248 | yes | no Volley (flail) | - | Barrier: ability | yes (generic, not the flail) | - |

## Shooting animations

The client draws the shot from the pulsed Volley: recoil pose and muzzle flash (Cassidy, Mei, Soldier seen this
session). The client's own Volley executor does not run for server-driven shots (`fire-animation-trace.md`), so
gun-kick (animRecoil) is not played; the camera climb and the first-person recoil pose do show.

## Reload timing (second pass, 2026-11-04)

What the weapon graphs author: SetVar var581 (reload length) and var6884 (refill fraction). The reload StateAnim
stretches the shared alias over var581. Wait var581 * var6884, then var53 = var198 (the magazine is full). Wait the
rest, then the reload ends.

Fixed server-side (`tools/lobby`):
1. var581 was a fixed 1.5 s for every kit. It is now the hero's reload time (Roadhog 2.0, Tracer 1.0, Wrecking Ball
   2.0, ...). Before, the anim was stretched to 1.5 s whatever the reload length.
2. The magazine refills at var6884 of the way in (`Weapon.refill`): firing is allowed from then on and cuts the
   rest of the anim, as in the graph.
3. Fire rates from `m_numShotsPerSecond`: Mei 0.05 s, Wrecking Ball 0.04 s, D.Va 1/7 s. Winston's reload 1.5 s.
4. The refill sends no update. A first version sent the full magazine at the refill point with the reload still
   on. In game every reload anim then restarted at that point and was cut by the end update: Soldier (rest at
   0.88 s, playing again at 1.18 s), Cassidy, Wrecking Ball, Winston, Mei. So the HUD shows the full magazine at
   the reload's end (or the first shot after the refill), not at the refill itself.

Sweep (`weapon_sweep.py HERO OUT fire reload`, frames 0.3 s apart by file time, `.scratchpad/sweep/<hero>`), first
version (before item 4):

| Hero | var581 | Refill | Fire seen | Reload anim | HUD full by |
|---|---|---|---|---|---|
| Soldier: 76 | 1.5 | 0.73 s | 16 ticks a shot (9/s) | plays, ends by 1.77 s | 0.88 s |
| Tracer | 1.0 | 0.70 s | 11 rounds in 0.6 s | plays, ends by 1.18 s | 1.18 s |
| Roadhog | 2.0 | 1.03 s | 1 shot, pump | plays | 1.19 s |
| Mei | 1.5 | 0.90 s | 11 rounds in 0.6 s; no spray drawn | plays | 1.18 s |
| D.Va | 1.4 | 0.91 s | 5 rounds in 0.6 s | plays | 1.18 s |
| Wrecking Ball | 2.0 | 1.00 s | 15 rounds in 0.6 s | plays, restarts at the refill | 1.18 s |
| Winston | 1.5 | 1.07 s | ammo drains, no beam (known) | plays, restarts at the refill | 1.18 s |
| Cassidy | 1.5 | 0.80 s | 2 shots | plays, restarts at the refill | 0.89 s |
| Sombra | 1.4 | 0.56 s | 13 rounds in 0.6 s | plays, restarts at the refill | 0.89 s |

After item 4 (re-sweep, `.scratchpad/sweep2`): Winston, Soldier, Cassidy and Wrecking Ball play one continuous
reload anim, back at rest at 1.47 s (Winston), 1.77 s (Soldier, Cassidy; 1.5 s reloads) and 2.07 s (Wrecking Ball,
2.0 s). No restart.

Authored clips (catalog scan, per kit hero, clips whose length equals var581): 21 of 27 kits have a first-person clip
of exactly the reload length (Soldier `0x1C00` arms + `0x1D8C` gun; Roadhog `0x125F`; Tracer `0x0065`; Wrecking Ball
`0x54C2`; Orisa `0x4122`; Winston `0x0A66`; ...). None for Torbjörn, Sombra, Reaper, Ashe, D.Va and Sigma; the
StateAnim stretches the alias over var581, so the length need not match. Third-person body clips of that length
exist for Roadhog, Torbjörn, Wrecking Ball, Orisa, Lúcio, Zarya, Winston, Baptiste, Zenyatta and Widowmaker.

Third person: not checkable in game. The player only sees their own first-person view, and the Range bots are
training bots, not heroes. The third-person body plays from the same graph states (var51 and the reload Anim), so it
gets the same var581 length.

## Open

- Ashe's primary fire was blocked by switch 96 (the fire button's child, reads the entity's action-priority var33):
  with it on its true branch the client sends the fire bit and the server's shots run (fixed 2026-11-04). The same
  check (switches on the Volley's parent chain the kit does not send) lists Lúcio 49, Widowmaker 80/8, D.Va 26,
  Sigma 23, Symmetra 36, Moira 19, Genji 55/2/1/12: Lúcio, Widowmaker and D.Va fire anyway (an earlier session);
  Genji is a candidate for his missing fire.
- Scopes (Ana, Widowmaker, Ashe ADS), beams (Zarya, Symmetra, Moira, Winston) and the no-Volley melee weapons
  (Reinhardt, Brigitte) are unchanged from `open-issues-2026-11-03.md`.
- Per-hero melee damage (Reinhardt 85, Brigitte 35, Torbjörn's hammer 55) and the melee hit on a bot are not done.
- Genji and Doomfist fire after their button-20 LogicalButton and the switches above it were hand-mapped (their
  Volley's parent chain does not pass through the button, so `weapon_kit.kit()` finds none). Doomfist's screen
  shows a CANCEL prompt: it was already in the 2026-11-03 ability sweep's frame (his right-mouse Rocket Punch HUD), not from this pass.
- Reload animations seen: Mei, Cassidy, Roadhog, Torbjörn, Zarya, Baptiste. The other kits use the same derived
  route and payload but were not watched; Ashe's start-only reload was not watched either.
