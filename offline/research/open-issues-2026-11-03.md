# Open issues (2026-11-03)

Historical snapshot. Several issues below were fixed on November 4–7. Use [STATUS.md](../STATUS.md) and the [fork review](fork-review-2026-11-07.md) for current work; retain this file as the original investigation record.

Snapshot of what is broken, missing or approximated. Details live in the linked docs.

Fixed 2026-11-03 (later pass, `tools/lobby/CHANGELOG.md`): charges count down (`kits.Charges`, checked in game); Dynamite self-damage checked in game; unit-tested only: rockets and casts fly through friendly bots, Biotic Field heals the team's hero bots; `tasks/todo.md` refreshed; new run logs and wiki dumps are gitignored. A knowledge graph of the repo is in `graphify-out/` (`graph.html`, `GRAPH_REPORT.md`; screenshots and crash logs left out).

## 1. Ability effects (first slice committed: projectile casts)

- **Cast animation:** on a press the server now pulses the ability's path and Volley states (14 abilities, `kits.Ability.cast`). The client accepts them (no crash), but nothing shows: no throw or cast animation in first person. **Fixed:** all 13 cast animations play in first person (`abilities/cast-anim-*.png`; `statescript-payloads.md`): the 33 ms pulses never reached the client, so casts now hold their states 1 s (`match.CAST_HOLD`), and the StateAnim payload is 5 bits.
- **Damage:** 10 thrown or fired abilities hurt bots server-side (`target.ABILITY_HITS`, wiki values). Shortcuts:
  - damage only: no stun, sleep, snare, anti-heal, burn, knockback or healing
  - throws fly straight (no arc); mines, traps and bombs go off where they land
  - throw ranges are guesses
  - self-damage only for Dynamite and Sticky Bombs, at half the splash (Dynamite checked in game: 25 at the feet)
- **Projectile visuals:** the drawn child of each Volley's projectile entity is flown (bot-shot method). Seen: Venom Mine, Symmetra's turret orb, Ana's grenade. Steel Trap has no child (invisible). Symmetra stays in the client's own turret placement (DEPLOY / CANCEL) after Shift.
- **Not started:**
  - movement abilities (Blink, Wraith, Jump Jet, Combat Roll, dashes): they need the server mover
  - barriers and deployables
  - ultimates for non-Soldier heroes
  - Moira's orb (heal or damage, picked with left/right mouse after E) and Reinhardt's Fire Strike (piercing) cast and deal their numbers since 2026-11-06; the orb does not bounce

## 2. Ability HUD (committed)

- Charges count down (`kits.Charges`: one per press, one back per cooldown, var11 resent every 0.5 s). Checked in game for Tracer and Junkrat (`abilities/charges-*.png`); Symmetra and Brigitte not pressed.
- Sombra's Stealth icon keeps a red overlay; its countdown works.
- Right-mouse abilities (Bastion, Orisa, Wrecking Ball, Doomfist) were never pressed in a test.
- No HUD at all for Torbjörn, Zenyatta, Hanzo and D.Va abilities (no var472 cooldown).
- The countdown refresh is a 0.5 s server resend; the native refresh is not traced.

## 3. Heroes (committed)

Weapons pass 2026-11-04 (`weapons-sweep.md`): Ashe's rifle shows and fires, Genji and Doomfist fire, reload animations
play for the kit heroes, quick melee (V) swings and hits for every hero (`melee.md`), and five right-mouse weapon volleys
run (Cassidy, Roadhog, Torbjörn, Zarya, Baptiste). Left:

- D.Va plays as her pilot only: the mech graphs are not in the pinned catalog.
- Reinhardt, Winston and Brigitte show their weapon but no shot (no WeaponVolley); Zarya, Symmetra and Moira have no beam.
- Scopes (Ana, Widowmaker, Ashe ADS) are not served. Since 2026-11-06 every right mouse with a route is: Symmetra's
  orb (full charge only, drawn), Zenyatta's volley (drawn), Moira's beam (damage only; its locked branch now holds,
  but no beam or hand pose draws: like every target-to-entity beam, asi-tracer.md), Mei's icicle (earlier); Baptiste's grenade only flies the volley (no heal).
- Melee damage is 30 for everyone (Reinhardt, Brigitte, Torbjörn's hammer differ).
- Ashe's per-bullet reload plays its start only; reload animations were watched for six heroes.
- Weapon stats are approximations (`target.PLAYER_KIT` shortcuts: full-charge arrows, Symmetra tier 1, one Tesla target, ...).

## 4. Match flow

- Victory banner: payload decoded (`STU_B9898052`), still never shows (`end-of-match.md`).
- Round number and time remaining: not found (`assault-mode-graph.md`).
- Capture HUD padlocks unchanged by the objective/player arrays.

## 5. Bots and combat

- In the Helix retest, no bot showed on screen; the rocket itself was never seen (`bot-fire.md`).
- Bot projectiles are destroyed and re-created every frame (no mover component).
- Friendly bots no longer stop rockets, and Biotic Field heals the team's hero bots (unit-tested; two Hanamura runs had no hurt ally in the field's radius, so not seen in game).

## 6. Soldier: 76 leftovers

- Fixed 2026-11-05, checked in game ([CHANGELOG](../tools/lobby/CHANGELOG.md)):
  - The primary-fire effect no longer runs from spawn: graph 596's state 21 is now sent off ([live read](soldier-runtime/fire-sfx-spawn-state21.txt)). The sound itself was not captured: the game's audio does not reach the Voicemeeter capture devices.
  - The Sprint animation plays: var1273 = `0x017D` and graph 75's state 23 ([capture](soldier-runtime/sprint-anim-sheet.png)).
  - Helix Rockets show in the world: the drawn child `0x072E` is server-flown ([capture](soldier-runtime/helix-rocket-sheet.png)).
  - Biotic Field's throw animation plays: Anim bit 2, held 1 s ([capture](soldier-runtime/field-throw-anim-sheet.png)).
- Helix Rockets look wrong (open): retail shows three rockets spiralling; the lobby re-creates one glow per frame, leaving a row of glows. The authored Volley fires one projectile, so the spiral likely needs the client's own projectile simulation [unverified]. Client prediction (descriptor bit) and owner-body shot records did not start it. A live read (2026-11-04) shows the production Volley 16 does get its key and runs one shot on the owner's body, so the missing rocket is past the shot. The client writes a reference to the Helix projectile `0x072C` (model plus child glow) at the press, and it passes every creation refusal that could be mapped. Read-only sampling (3 runs) found no moving `0x072C` object; the only fresh copy lived under 0.17 s and may be the projectile job rather than the entity. Root cause found 2026-11-05 by emulating creation: Helix's config is type 1 (client-predicted entity), and the client's spawner needs a server-granted predicted-entity ID reservation for `0x072C`. The live reservation table is empty, so creation fails (status 8). The carrier is found: an ordinary update of component 90 (STU_41FFE61E, field `0x63398BCB`: an array of {def, first tick, end tick, base ID, IDs per tick}). The lobby now sends it (Soldier body create): the client issues IDs and creates the rocket entities, but they still do not draw; confirmation is a ch4 create carrying the predicted ID (create handler 0x7FF6D19AC360 skips the spawn for an existing ID); the lobby now reserves a fixed ID and confirms it on the press (in the client heap), but no rocket shows (user check 2026-11-05; open after 3 runs); see helix-visual-findings.md. See [helix-visual-findings.md](soldier-runtime/helix-visual-findings.md).
- Biotic Field ring (open, 3 runs): graph 600 builds on impact entity `0x03DC` with the canister and ring states active, but nothing draws. Production still creates the canister `0x068A` at the feet. Findings and next step: [voice-and-ring-findings.md](soldier-runtime/voice-and-ring-findings.md).
- Spawn voice line, every hero (open): hero lines come from ClientInVoice message 27500/27501, which the lobby never sends. The voice director (graph 458, inside common graph 75) is server-only. The spawn stimulus and some message fields are not identified. Same findings file.
- Sprint: toggle mode, re-sprint delay and cancel buttons are not modelled.
- Numeric health on the HUD: fixed 2026-11-04 (`numeric-health.md` round 7).

## 7. Housekeeping

- `tasks/todo.md` refreshed (Soldier spawn, firing and the one-bot loop ticked; limits point here).
- New run folders under `research/main-menu/` and the `research/wiki/*.xml` dumps are gitignored; the 182 run folders already committed (95 MB) stay in history until someone decides to prune them.
