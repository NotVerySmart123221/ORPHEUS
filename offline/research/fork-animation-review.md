# Review of the OW-finalbuild animation fork (2026-11-05)

`<old-workbench>` was an untracked copy of commit `1319615`. On 2026-11-05 its uncommitted changes were copied here: 24 changed files, 133 new files, run logs and captures, and its newer `ida/Overwatch-state-analysis.dump.i64`. The previous database is kept as `ida/Overwatch-state-analysis.dump.pre-fork.i64`. `casc.py` was skipped because it differed only in line endings. The scratch scripts (`.scratchpad/*.py`, including `reload_lab.py`, `ads_lab.py` and `roster_capture.py`) were copied without the PNG bursts. All 245 fork tests passed before the follow-up work below.

## What the fork did

| Area | Result in retail 1.74 | Where |
| --- | --- | --- |
| Aim down sights | RMB drives camera zoom and scope progress for Ashe, Ana and Widowmaker. The common graph 75 state 23 makes the animator reread category `81`. Ashe's sights center and Ana's and Widowmaker's rifles clear the view. Widowmaker's charge meter fills and resets per shot. | `kits.Kit.scope`, `SCOPE_POSE`, `frame_builder`, [ADS notes](ads-restoration.md) |
| Ashe reload | Open, then one round per 0.25 s, then close. Fire during the insert phase closes the reload early. The per-round wrist cycle was verified at 60 fps. | `Weapon.reload_phases`, `Rifle._advance_round_reload` |
| Reload under damage or abilities | Health and cooldown refreshes go out as owner deltas, so the reload Anim no longer restarts. | `Stream.reload_frame`, `owner_delta_layouts_174.json`, [reload refresh](reload-refresh.md) |
| Alternate forms | Mercy switches between staff and Blaster with 1 and 2, and the Blaster reload stays visible. Bastion switches between Recon and Sentry. Self-Repair deploys, holds, retracts and heals. | `match.Player.switch_mercy_weapon`, `switch_bastion`, `kits.bastion_body` |
| Continuous effects | Winston's Tesla arcs start and stop. Moira's yellow spray and Mei's muzzle spray work. Mei's Icicle has a 0.4 s windup. | `kits.weapon_instance` hero branches, `AltFire.windup` |
| Sombra Hack | The hand pose ends at 0.65 s or on interruption. The server adds a 5 s bot ability lockout. | `kits.sombra_hack_states`, `match.update_hack` |
| Assault result lock | Combat and movement freeze after the result, while inputs are still acknowledged. | `match.py`, `test_assault.py` |
| Blender | Arms and child clips are paired through shared aliases. Hanzo, Symmetra, Zarya, Lúcio and Brigitte were paired. | `animation_aliases.py`, `clip_pairs.py`, `anim_export.py` |
| Comparisons | A 32-hero showcase comparison and a reload, Blender and retail audit. | [showcase](hero-showcase-comparison.md), [roster audit](roster-animation-audit.md) |

## Still open

- Beam and tether visuals, all of which the fork tried without success:
  - Mercy's staff beams
  - Zarya's primary beam
  - Symmetra's beam and charged orb
  - Moira's damage tether
  - Sombra's Hack tether (owner TrackRay state 32's payload is unresolved)
- Charge and draw poses:
  - Hanzo's bow draw and nock
  - Sigma's orb recovery
  - Zenyatta's charged volley
  - Genji's fan
- Melee primaries:
  - Reinhardt's hammer
  - Brigitte's flail
  - shield deployments
- Forms and target effects:
  - Bastion Tank
  - Mei's freeze
  - Mercy's damage boost
- Tuning: scoped weapons still use hip-fire cadence and damage.

## Shortcomings

A separate code-review pass on the imported diff found the following. The status column covers this session.

| Finding | Status |
| --- | --- |
| The delta layout file lacked graphs the lobby sends (75, 109, 118, 303, 314, 463). A change in any of them during a reload refresh raised `KeyError` in `Match.update`. Graph 4429 is not in the catalog at all. | **Fixed.** The generator covers the six missing graphs. `_owner_changes` returns None when the instance list differs or a changed graph has no layout, and `reload_frame` then sends a full frame. Regression test `test_reload_delta_falls_back_when_instances_change_or_lack_a_layout`. |
| Deltas paired instances by position (`zip(strict=True)`), so a changed instance set raised or wrote a wrong delta. | **Fixed** by the same index check. |
| Hero special cases in generic code: Ashe's phase states, Winston, Mei and Moira effect states in `weapon_instance`, and Mercy, Widowmaker and scope logic in `frame_builder`. | **Partly fixed.** Ashe's phases are now `Kit.phases`, which Doomfist also uses. Effect states and the Widowmaker and Mercy branches remain. |
| `match.Player` holds about 15 per-hero fields, and `switch_hero` resets them by hand. | Open. Move them to a per-hero state object when the next form hero lands. |
| Mercy's 1/2 switch reads `command.action`, which is carried forward from earlier commands. A stale 2 after respawn could select the Blaster. | Open. Not reproduced. |
| Bastion repair can heal several seconds' worth in one step after paused ticks. | Open. Reset `bastion_repair_tick` when updates resume. |
| Widowmaker's charge sends a full owner frame every 50 ms while scoped. | Open. Quantize the charge or send it as a delta. |
| `_owner_changes` and `_delta_mask` have only smoke tests. | Open. Add a decode test that checks the masks for more than 8 slots. |
| The graph catalog `game_graphs_174.json.gz` lives only in `.scratchpad/`, which can be cleared. | Open. Re-download it from the pinned upstream commit if it is lost (see `tools/lobby/vendor/README.md`). |

## Picked up this session

**Doomfist's Hand Cannon recharge.** In graph `0x0D0A`:

- Each shot sets var173 to 1/var9805, a 0.333 s timer, and ChaseVar 12 runs it down to 0.
- Root switch 17 then opens while ammo is below capacity and no shot is pending.
- Its Wait 19 lasts 0.65 s (var10949) and plays Anim 20 with Stack 54. When it finishes, node 35 adds one round and loops back.

The lobby previously refilled all four rounds in one event about 2.9 s after R.

What changed:

- `Weapon.recharge` and `Rifle._advance_recharge` now follow the graph's timing. A shot aborts the Wait, R does nothing, and an empty magazine recharges.
- The kit serves switch 17, Wait 19, Anim 20 and Stack 54 for each round.
- Approximation: the 0.32 s shot interval stands in for the 0.333 s var173 timer.

Retail check, `research/main-menu/20261105T010016.197785Z/`, about 7 fps:

- Two shots took the HUD from 4 to 2.
- Measured from the first shot, the count showed 3 by +1.48 s and 4 by +2.16 s. The graph timing predicts +1.29 s and +1.94 s, and frames were about 0.14 s apart.
- The cannon stayed forward with lit knuckles until full, then returned to the idle fist.

The frame rate is too low to compare the knuckle motion with the showcase. Captures: `research/anim/doomfist-recharge-cannon.jpg` and `doomfist-recharge-gauntlet.jpg`.

**Retail tooling note.** `enter_range.py` reported "in range" while the client was still on the name screen. Its Play and Practice Range coordinates also miss the current main menu. On this run, Enter on the name field logged in, Play was at (70, 160) and Practice Range at (420, 360).

## Suggested order for what remains

1. Hanzo's draw. It is a hold-to-charge weapon state, like Widowmaker's charge, which already works.
2. Zenyatta's and Symmetra's charged secondaries. They use the same charge-then-release path.
3. Effect states as Kit data. This removes the Winston, Mei and Moira branches before more beams are added.
4. Beams and tethers last. They need TrackRay and TrackTargets payloads that are still undecoded.
