# Project status

Offline Overwatch 1.74.0.0.104319: a local Python lobby/game service plus opt-in ASI plugins.

Last updated: 2026-11-08. Update this file whenever a step is verified or an open item changes.

- Plan and rules: [tasks/roadmap.md](tasks/roadmap.md)
- Current audit and continuation order: [research/fork-review-2026-11-08.md](research/fork-review-2026-11-08.md) (earlier: [2026-11-07](research/fork-review-2026-11-07.md))
- Latest live checks: [research/live-verification-2026-11-07/REPORT.md](research/live-verification-2026-11-07/REPORT.md)
- Historical open list: [research/open-issues-2026-11-03.md](research/open-issues-2026-11-03.md)
- Per-change history: `tools/*/CHANGELOG.md`

## Phase

Phase 1 of 4 is the Practice Range. Weapon kits cover all 32 heroes, with uneven ability coverage; hero switching and parts of vs-AI already exist ahead of plan.

1. Practice Range (current)
2. Hero switching
3. Practice vs. AI
4. Tutorial (optional)

The Range phase ends when the user plays one session and signs off. After that, restore the backed-up `Settings_v0.ini` (the game runs windowed until then).

## Working

1. **Boot and menus**
   - Launcher: `python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi`. It boots straight to the retail main menu. `ow_fixes.asi` holds every client fix (retail menus, 32:9, hitbox-drift feed; [tools/asi/README.md](tools/asi/README.md)).
   - Hero gallery and Career Profile work. Ordinary menu backdrops with randomized heroes and owned skins are the default with the retail-menu plugin. Native show/hide lifecycle keeps the menu model out of Gallery/Training and restores it after gameplay; equipped skins are retained. `--event farewell` keeps the farewell lineup. [Transition and skin verification](research/main-menu-normal/REPORT.md).
   - Opt-in `--progression --name OfflinePilot` adds a separate persistent earned account: finished Assault XP, level-up boxes, native four-item opening, credits, ownership and purchases. Native box reveal/relog and a 75-credit skin purchase are verified; retail XP math/introductory curve/drop odds remain approximations. [Usage and limits](tools/lobby/README.md#earned-accounts), [evidence](research/progression/implementation-2026-11-08.md).
   - Training > Practice vs. AI and Quick Play (vs-AI) open a map.
2. **Soldier: 76 slice**
   - HUD and numeric health.
   - Rifle, reload, Helix and melee.
   - Bots shoot back; the player dies and respawns; leaving returns to the menu cleanly.
   - Reference: the Hero Gallery's Hero Info clips (`tools/assets/hero_movies.py`, [research/hero-info](research/hero-info/README.md)). Soldier compared 2026-11-08: rifle, reload, Helix and Sprint match; Biotic Field release and the Tactical Visor cast were corrected and checked live ([comparison](research/hero-info/soldier76.md)).
3. **All 32 heroes have weapon kits**
   - Primary fire, reload and quick melee.
   - Ability HUD entries for 29 heroes, including Soldier's dedicated implementation.
   - Many right-mouse attacks and cast animations: Moira's grasp, Sombra's hack tether, Zarya's and Symmetra's beams, Symmetra's orb, Zenyatta's Orb Volley and Mercy's heal/boost beams (near end wrong for allies without a served weapon graph; see Beams).
4. **Bots:** jitter and wall-clipping fixed on 2026-11-06 (66 → 8.6 px of jitter per frame, 0 wall crossings). See [research/bot-movement.md](research/bot-movement.md).
   - 2026-11-07: native overhead name/health widgets, white/red hitmarkers and named ELIMINATED notifications work, including damage and bot respawn. Range hit volumes cover the authored folded-arm idle pose and breathing envelope. [Evidence and remaining limits](research/bot-nameplates-2026-11-07.md).
   - 2026-11-08: Range bots fall apart when killed (the client's own death on a kept body) and reassemble where they fell 3 s later (graph 0BA8's StateAnim, clips 0x39D7/0x39D8). Hero bots show their death for 3 s, then respawn at home (not checked live). [research/bot-death.md](research/bot-death.md).
   - Movement abilities (2026-11-10): Wraith Form, Fade, Cryo-Freeze, Charge and Stealth move the hero as their graphs say, on the client and on the server's copy; every hero but D.Va gets owner corrections. Blink, Jump Jet, Jump Pack, Combat Roll and Swift Strike move the server's copy as the client's own prediction does (measured on a VM body), and the client follows by correction. Pharah's Hover Jets, Mercy's Angelic Descent and Echo's Glide run on both sides (the client plays their states itself). Echo's Flight: the server plays the launch, both sides fly the state's own mover along the aim. Coach Gun's kick and Rising Uppercut's three launches are played at their times. Wrecking Ball rolls (third-person camera, but still the walker's model). Genji's second jump and Hanzo's Lunge are the server's launches from the graphs' numbers. Open: Guardian Angel, wall climb, wall ride, the teleports and knockback. See [research/lizrollin-runtime.md](research/lizrollin-runtime.md) sections 21, 23, 25 to 30, 32, 33 and 35 to 40.
   - Match HUD, Tab board (2026-11-10): opens in the Practice Range, Skirmish, the objective modes, Control, Deathmatch, Elimination and Capture the Flag, with the teams, the player's stats and medals and the hero's own, updated live. The portrait burns when the player is on fire (three or four kills in a row; healing fills it too). Healing Done is counted (section 31). Control maps load (they crashed before). The chat shows the hero switch line in PvP modes. Open: tiles' ultimate charge and the board's time read 0; objective, healing and hero-ability stats are not counted; PvE is untested; On Fire counts eliminations only. See [research/lizrollin-runtime.md](research/lizrollin-runtime.md) section 22.
5. **Uprising (PvE)**
   - Boots and loads.
   - The omnics spawn and walk to the players.
   - The DEFEND timer runs 4:00, then the mission switches to ESCORT. See [research/uprising-mission.md](research/uprising-mission.md).
6. **Debugging tools** (`tools/asi/`)
   - `tracer.asi`: logs every server message and each statescript state's begin/end and frame reads.
   - `guard_probe.asi`: data watchpoint that doesn't crash the client.
   - `frame_grab.asi`: screenshot fallback when normal capture comes back black.
   - `dev_console.asi`: the hidden ImGui console (Alt+`) and the 1P arm offsets viewer. See [research/dev-console.md](research/dev-console.md).
   - `tools/lobby/viewer.py`: 3P mannequin; edit the hero name in the file it watches to swap heroes.
   - `tools/lobby/oracle.py` (2026-11-09): reads every statescript chunk the lobby tests send the way the client does, using LizRollin's vendored decoder with the client's codec table. A test fails when the client would crash on, refuse or misread one of its chunks; `OW_ORACLE=1` prints the report. Its first sweep found the bot weapon frames missing their silent slots: Lúcio bots' fire never showed and Mei's frames were misread. Fixed, not yet seen live. See [research/lizrollin-runtime.md](research/lizrollin-runtime.md).
   - `tools/assets/anim_export.py all`: every hero's 1P/3P animations to JSON and `.blend`.
   - `tools/research/hitbox_view.py`: the server's hit volumes drawn over the game, as the server's aim test sees them (environment variable `OW_HITBOX_VIEW=47123`; PowerShell: `$env:OW_HITBOX_VIEW = '47123'` before `run.py`). `client_position.py`: how far the server's copy of the player has drifted from the client, at the same command frame, read-only from the client's own mover. See [research/hitbox-drift.md](research/hitbox-drift.md).

## Open items

- Shots drift from the bots the longer you move, because shots start from the server's copy of the player and nothing corrects it ([research/hitbox-drift.md](research/hitbox-drift.md)). 2026-11-08: the client's 90% backpedal speed is matched, which was the biggest error (about 1.85 m → 0.24 m on a test walk). Still open: props missing from the collision mesh (the server walks through the table by the Range spawn), the client's speed ramps, and the crouched eye height. Exact fix, opt-in since 2026-11-08: `OW_CLIENT_SYNC=47124` on the launcher feeds the client's own position (read-only) back to the lobby, which moves its copy onto it every command step. After a long walk the drift is 0.000 m at rest and under 5 cm while moving (one frame of lag; up to 7.7 cm on airborne frames). It falls back to dead reckoning when the feed is absent. Not the default until the user has played with it. 2026-11-09 (branch `statescript-oracle`): shots now leave from each hero's own eye height, standing or crouched, instead of 1.6 m. They are tested against the bots where the shooter's client drew them (the command's view delay, from LizRollin's runtime) and aim where the crosshair was at the click. Click aim, view delay and both eye heights were checked live ([items 3-4](research/hitbox-drift.md)).
- Exact animation clocks and hero limb poses remain unsynchronized. Range hitboxes use idle-motion envelopes with <=3.8 cm extra radius. See [nameplate/hitbox follow-up](research/bot-nameplates-2026-11-07.md). Unsupported component 123 remains excluded from Range bodies.

### Menus

- **Career Profile offline: first version built 2026-11-09.** Matches other than the Practice Range now count time played, eliminations, final blows, deaths, damage, kill streaks and games played, won and lost per hero, and the profile's Overview and Statistics tabs show them (section 3 of [research/lizrollin-lobby.md](research/lizrollin-lobby.md)). Open: healing, objective stats, time on fire and the Avg/10 min column are not counted; every match counts as Quick Play; a finished game's rows and a kill's rows were not checked live; ratings, endorsements and replays were not studied.
- **To try later: the settings file directly in the game folder.** The launcher now gives the game its own user folder, so `Settings_v0.ini` sits at `<game-root>/OfflineProfile/Documents/Overwatch/Settings/`. Getting it to `<game-root>/Settings/` is written up, with three ways to try, under "To try later" in section 2 of [research/lizrollin-lobby.md](research/lizrollin-lobby.md).

### Housekeeping

1. [x] Commit the dev console, 3P viewer and animation export work (committed 2026-11-06).
2. [x] [research/CONTINUE.md](research/CONTINUE.md) is marked historical and points here (2026-11-06).

### Beams

3. [ ] Finish beam mechanics and layout decoding. Moira's grasp, Sombra's hack tether, Zarya's and Symmetra's primary draw. Mercy's heal beam draws from the staff to a Soldier: 76 ally. The 2026-11-07 "pistol/staff mismatch" was the staff (same model as the d4e4c96 captures) healing an ally Reinhardt, whose remote frame was never sent because he has no `WEAPON_GRAPHS` entry; fixed 2026-11-08. Open: for such allies the beam's near end falls to the world origin ([evidence](research/live-verification-2026-11-08/README.md)).
   - Causes: the beam is its own `STU_9932D728` state, which the client starts only when the server lists it; Zarya's and Symmetra's ray length (var1648) came out 0. Notes in [research/asi-tracer.md](research/asi-tracer.md) ("Sixth run").
   - Earlier Mercy evidence: heal status graph `0x0079` uses var27 presence bit 3, not `graph_layout.py`'s 5; boost status graph is `0x006C`. Current staff targeting works server-side, but the resolved cosmetic model/beam attachment needs investigation. Boost still has no damage effect.
   - `graph_layout.py`'s presence order is wrong for some graphs (`0x0079`, and its count for `0x0D28`); the rule is unknown.

### Weapons

4. [x] Ashe's primary reaches the server; her reload inserts rounds individually. Repeated insert animation remains incomplete.
5. [ ] Ana, Widowmaker and Ashe have native scoped/ADS views, and Widowmaker has a charge overlay. Scoped damage, cadence and ammo still need tuning; see [research/ads-restoration.md](research/ads-restoration.md).
6. [ ] Reinhardt's and Brigitte's native primary attacks remain incomplete. Winston's Tesla effects work, with approximate single-target damage.
7. [ ] D.Va spawns in her mech and fires the Fusion Cannons (2026-11-07, [research/dva-mech.md](research/dva-mech.md)). Earlier live evidence: Boosters fly for 2 s, Matrix shows while held, Micro Missiles stream 18 missiles; icons count down. Live follow-up confirms Matrix cannon suppression, fire resuming on depletion, and protection from Range shots in two stationary windows. Exact native transform/scale remains unverified. Open: Matrix meter UI, Boosters' collision hit, Self-Destruct, eject/pilot, Call Mech.
8. [ ] Melee does a flat 30 damage for every hero.
9. [ ] Weapon stats are approximations (`target.PLAYER_KIT` shortcuts).

### Abilities

10. [ ] Movement abilities: twenty-six are built (2026-11-10, [research/lizrollin-runtime.md](research/lizrollin-runtime.md) sections 23, 25 to 30, 32, 33, 35 and 41): Wraith Form, Fade, Cryo-Freeze, Charge, Stealth, Blink, Jump Jet, Hover Jets, Jump Pack, Combat Roll, Swift Strike, Angelic Descent, Glide, Flight, Coach Gun, Rising Uppercut, Roll, Genji's second jump, Lunge, the mine jump, Recall, Exo Boots, Shadow Step, Guardian Angel, plus Boosters and Sprint from before. Left:
    - Not started (what is known of each: section 34): Genji's and Hanzo's wall climb, Lúcio's wall ride, Translocator, Teleporter, Doomfist's Rocket Punch and Seismic Slam, Wrecking Ball's grapple and piledriver, Widowmaker's grapple, Shield Bash, Primal Rage's leap.
    - Knockback on the player (his own rockets and blasts, and anything a bot would do); Junkrat's mine jump is built.
    - Shortcuts in what is built: Blink's and Jump Jet's own effects are not sent; Jump Pack does no landing damage; Swift Strike does not reset on a kill and its end point is not the graph's ray; Charge pins nobody; Cryo-Freeze has no ice block; Sombra in Stealth is still drawn; the Hover Jets have no fuel gauge; Wrecking Ball rolls as the walker's model; D.Va's Boosters are a fit; Flight cannot be ended early; Rising Uppercut hits nobody; Recall slides her back without the rewind's look; Guardian Angel stops 2.5 m short of the ally whatever the two bodies, and a jump does not sling her on; Shadow Step lands on the client's marker with the client's feed on, and without it where the server's port of the targeting query says (to the centimetre for 533 of the 541 recorded aims that have a place; the 8 left, to chase later, are listed in section 40 of research/lizrollin-runtime.md: seven level aims where the line lies flat on a table or the floor, one of 2 cm); the second jump and the Lunge have no animation, and the second jump's speed is read off the graph, not checked.
11. [ ] Barriers, deployables and non-Soldier ultimates aren't started.
12. [ ] Most status mechanics remain absent: stun, slow, knockback, anti-heal and similar effects. Healing already exists for Soldier's field, Mercy's staff, Moira's orb and Bastion's repair. Mercy's damage boost is visual only.
13. [ ] Torbjörn, Zenyatta and Hanzo have no ability HUD.

### Match flow

14. [ ] The victory banner never shows ([research/end-of-match.md](research/end-of-match.md)).
15. [ ] The round number and time remaining haven't been found ([research/assault-mode-graph.md](research/assault-mode-graph.md)).
16. [ ] Uprising: the hack and rendezvous phases, the kill phase, the moving payload bar and the banners are still missing.

### Bots

17. [ ] Bot projectiles are destroyed and re-created every frame (no mover component).
   - Hero bots' remote fire (2026-11-09, viewer mannequin): the automatic weapons (Soldier: 76, Mei, Bastion, Lúcio) fired only when the bot appeared, because the client fires them itself, at most one magazine (the ammo var) per Volley start, and the server never ended the Volley. Fixed on `statescript-oracle`: the Volley ends at each reload and starts again with the next magazine. Checked live: Soldier's and Lúcio's fire now repeats every magazine.
   - The fire drew at the bot's feet: the gun's var49 was unbound. Now the bot frame begins the weapon graph's CosmeticEntity and binds var49 to it by remote slot (an H=0 binding names a remote slot, not a state; the first try used the state index, which misread the frame and froze the client for Soldier). Checked live: Lúcio, Soldier, Roadhog, Sombra and Ana hold their guns and fire from them. Mei's spray shows now (two effect states the remote Volley never begins). Cassidy, Ana, Torbjörn and Zenyatta (shot mode 1) draw their shots from a server-made projectile named in each shot record. The swap stalls of the reliable channel were datagrams over the size the client takes (a new bot's 20301 plus the frame's other records); the server now holds messages back until a frame fits (14 swaps, 0 resends). Gun binding not done: Torbjörn, Reaper, Zenyatta, Bastion, D.Va. The shot record's pitch was also written upside down (fixed; D.Va's tracers now reach the player). See [research/bot-fire.md](research/bot-fire.md).
18. [ ] No death animation.

### Phase sign-off

19. [ ] The user plays one Range session and signs off.
20. [ ] Restore the backed-up `Settings_v0.ini`.

### Reliability work completed after import

21. [x] Cap near-wall walking and Boosters movement to the requested command distance.
22. [x] Clear Mercy/Moira beam locks when stowing the staff, dying, replacing a body or leaving, as applicable.
23. [x] Publish PvE marker JSON only after successful extraction and parsing; failed output is removed so a launch can retry.

Soldier's five presentation items (user-confirmed 2026-11-07), all required before Range sign-off:

1. [x] Native Helix cluster/spiral at50m/s, verified on ordinary RMB in clean normal client33876; repeat and six-second cooldown checks pass. Initial impact counter0 prevents a false explosion at launch.
2. [x] Biotic Field gold floor ring, beam and correct black/gold canister, verified for the five-second lifetime and cleanup in clean normal client33876.
3. [x] Correct spawn voice queued once after body delivery; entry playback confirmed. Respawn routing tested; respawn audio remains unchecked.
4. [x] Authored walking/Sprint transition animations in both directions, preserved through weapon/HUD updates and verified in a clean normal session.
5. [x] First-person shooting animation: clean saved-code PID12312 shows burst/release gun motion and correct22-shot ammo8. Fresh normal PID34048 confirms firing and ability countdowns after the timer-binding correction.

See [five-item progress and remaining blockers](research/soldier-runtime/five-items-progress-2026-11-07.md), [earlier Soldier findings](research/soldier-runtime/presentation-next-steps-2026-11-07.md) and [fork review](research/fork-review-2026-11-07.md).

Soldier presentation work complete: all five items are saved and verified. Clean normal PID12312 passed primary motion, Helix, Sprint, reload and Field checks; fresh PID34048 passed firing and native cooldown countdowns. Both clients closed with exit0. [Primary video](research/soldier-runtime/verified-primary-2026-11-07/README.md), [Helix/Field captures](research/soldier-runtime/verified-field-helix-2026-11-07/README.md) and [progress](research/soldier-runtime/five-items-progress-2026-11-07.md) record results. Other viewers retain the existing Helix projectile fallback; isolated scheduler experiments remain outside production. User authorized `retail_menu.asi`, untimed launches and the existing guarded helper. Matrix, Mercy stow cleanup and Moira's switch/reacquisition have [live evidence](research/live-verification-2026-11-07/REPORT.md); Mercy staff presentation remains open. No user Range sign-off.

## Verification note

Controller follow-up: user confirmed manual reload and empty-magazine suppression. Subsequent lost feedback came from seven padding bits after the elimination message's final Bool, corrupting the following entity data and freezing weapon replication. Shared message boundaries now preserve the exact native bit cursor. Controlled elimination and normal-launch Range re-entry/held-fire checks pass: two bot eliminations, further respawn hits, native/server ammo26 and queue0. The patched normal Range remains open; physical controller retry of this latest correction remains unverified. [Controller reload audit](research/soldier-runtime/controller-reload-2026-11-07.md), [packet-boundary evidence](research/soldier-runtime/verified-reload-boundary-2026-11-07/README.md).

2026-11-09: 433 tests pass (13.9s), with the statescript oracle checking every lobby test's chunks. 2026-11-08: 418 tests pass (15.2s), plus the two C self-checks in `tools/asi/build.bat` (aspect ratio, retail menu). 2026-11-07: 353 tests pass (12.10s). Regression coverage includes movement, beam lifecycle, marker extraction, Matrix mechanics, model/effect references, Soldier voice/Sprint preservation, Field placement/lifetime, native Helix codec/ACK/cleanup, C1 owner data clocks, per-frame primary/cooldown outputs, reload/empty native input controls and final/intermediate JAM Bool boundaries. Command: `python -B -m pytest tools/lobby tools/assets tools/research tools/blender -q`. Current live results and limits are in [the verification report](research/live-verification-2026-11-07/REPORT.md) and [Soldier progress](research/soldier-runtime/five-items-progress-2026-11-07.md); they do not constitute user Range sign-off.
