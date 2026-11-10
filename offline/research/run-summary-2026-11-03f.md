# Unattended run summary (2026-11-03, sixth run)

Order asked: banners and round values, then the 11 remaining heroes, then the abilities. Every code change has its own local commit and was checked in game. Nothing was pushed. No TEMP probes are left; the game is closed.

## Steps

| Item | Result | Commits | Evidence |
|---|---|---|---|
| Banner / round values | Round-complete init data and banner probes written up; banner still unseen, round number and time still not found | 695f6c8 | `research/end-of-match.md` |
| 11 heroes | **All 31 heroes playable**: Zarya, Hanzo, D.Va (pilot), Sigma, Symmetra, Moira, Genji, Doomfist, Reinhardt, Winston, Brigitte. Kits without a Volley, no-ammo weapons, reach-limited melee and beam hits | 0887900, 185eec5 | `research/main-menu/kit-*.png`, `research/non-soldier-hero-scope.md` |
| Abilities | Ability HUD for 26 heroes: icons, presses, cooldown clock. Countdown shows for 32 of 44 pressed abilities | acd23e1 | `research/abilities.md`, `research/main-menu/ability-hud-*.png`, `ability-sweep-cassidy.png` |

## Findings worth keeping

- A BooleanSwitch always needs its 1-bit payload, also when reached through `m_subgraphPlug`; sent bare it misaligns the frame (Brigitte, assert `0xF0E7E025(85)`).
- The StateAbility payload must be built at send time (the callable `Volley.ability`). A fixed payload restarts the cooldown at every resend, so the icon sticks at duration + 1.
- `tools/assets/ability_kit.py` derives every hero's ability HUD from the graph catalog (button, icon presenters, var472 seconds, var61 bit). For Soldier it matches the hand-mapped graph 599.
- Zarya's Projected Barrier shows no countdown: its var61 setter runs only after TrackTargets finds a target.

## Left / next

1. Countdown missing: Reaper (both), Sombra and Pharah (Shift), Mei, Brigitte and Zarya (E); charge abilities (Tracer, Junkrat, Symmetra) show a red crossed icon. Right-mouse abilities were not pressed in the check.
2. Ability effects (movement, projectiles, status) and ultimates: none are served.
3. Torbjörn, Zenyatta, Hanzo and D.Va abilities: no var472 cooldown; D.Va's mech is not in the catalog.
4. Banner and round number/time (see `research/end-of-match.md`).
