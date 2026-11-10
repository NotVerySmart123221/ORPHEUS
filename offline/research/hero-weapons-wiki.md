# OW1 (build 1.74) hero weapon facts from the Overwatch Wiki dump

Source: `research/wiki/roster/<Hero>.wiki` (last revision before 2022-10-01; see that folder's README). Citations are `File:Lnn` (line of the wikitext). Only the quick-melee defaults come from outside the dump: wiki page `Melee`, revision 2022-08-14, fetched through the fandom MediaWiki API (`rvstart=2022-10-01T00:00:00Z&rvdir=older`). No other web access. No page contained instructions aimed at the reader.

Conventions
- Damage "a - b" on the wiki is far - near (min - max). Falloff "x to y m" = full damage up to x, linear down to the minimum at y.
- "Reload" gives the animation length and, where the wiki says so, the moment ammo is restored.
- `target.py` reads `spread` as a half-angle for pellet weapons (Roadhog 20.1 deg -> 10.05, Reaper 6 -> 3.0); Tracer's 3.6 is the wiki "Max" figure used as-is. Keep that in mind when comparing spread values.
- Doomfist and Orisa pages hold an OW1 section and an OW2-beta section; only the OW1 section is used (Doomfist L37-157, Orisa L35-138). Ignore Doomfist L158+ and Orisa L139+.
- Secondary fire key. "Alt fire" = part of the weapon, uses the weapon's ammo, no cooldown. "Ability" = own cooldown, though the wiki lists the key as `secondary fire`.

## 1. Per-hero table

| Hero | Weapon / mode | Type | Damage | Rate of fire | Ammo | Reload | Proj. speed | Falloff | Pellets / spread | Headshot | Cite |
|---|---|---|---|---|---|---|---|---|---|---|---|
| Ana | Biotic Rifle (primary) | projectile unscoped, hitscan scoped | 70 (heal 70) over 0.58 s | 0.8 s recovery | 12 | 1.5 s anim, ammo back at 0.592 s | 125 m/s unscoped | none | pinpoint | no | Ana:L34-51 |
| Ana | Biotic Rifle scope (secondary, hold) | alt fire (zoom) | same | scope in 0.24 s, out 0.16 s; -65% move speed | - | - | hitscan | - | - | no | Ana:L37,49,51 |
| Ana | Sleep Dart | ability 1 (not RMB), 12 s cd | 5 | 0.3 s cast + 0.67 s recovery | - | - | 60 m/s | - | pinpoint | no | Ana:L62-80 |
| Ashe | The Viper hip fire | hitscan semi-auto | 12-40 | 0.256 s (~4/s) | 12 | 0.5 s initial + 0.25 s per bullet, 3.5 s full | - | 20-40 m | max 1.85 deg | yes | Ashe:L31-43,L38 |
| Ashe | The Viper ADS (secondary, hold) | alt fire (weapon mode) | 22.5-75 | 0.656 s | 12 | same | - | 30-50 m | pinpoint | yes | Ashe:L56-71,L64; scope 0.192 s in / 0.160 s out, -25% move |
| Ashe | Coach Gun | ability 1 (Shift), 10 s cd | 6 x 15 pellets = 90 | cast 0.16 s + 0.4 s | - | - | hitscan | - | 15 pellets | no | Ashe:L101-117 |
| Ashe | Dynamite | ability 2 (E), 12 s cd | 20-50 + 100 burn | - | - | - | 25 m/s | - | - | - | Ashe:L79-98 |
| Baptiste | Biotic Launcher primary | hitscan 3-round burst | 7.2-24 per round | burst per 0.58 s (0.096 s between rounds) | 45 | 1.5 s anim, ammo back at 0.896 s | - | 25-45 m | 3 rounds | yes | Baptiste:L45-57,L52 |
| Baptiste | Biotic Launcher secondary | alt fire, lobbed heal grenade | heal 70 direct / 50 splash, 3 m | 0.9 s | 10 (shares reload) | same | 60 m/s arc | - | - | - | Baptiste:L64-76,L71; L60 reload refills both |
| Bastion | Configuration: Recon | hitscan auto | 6-20 | 8/s (0.125 s) | 35 | 1.5 s anim, ammo back 1.04 s | - | 25-45 m | max 1.2 deg | yes | Bastion:L39-52,L46 |
| Bastion | Configuration: Sentry (Shift/ability 1 switch) | hitscan auto | 4.5-15 | 31/s (0.032 s) | 300 | 1.984 s anim | - | 30-50 m | 1.8-2.7 deg | no | Bastion:L56-69,L63 |
| Bastion | Configuration: Tank (ultimate, 8 s) | projectile | 205 direct, splash 42-140, r 4 m | 1 shot/s | inf | - | 60 m/s | - | - | no | Bastion:L113-128 |
| Bastion | Self-Repair | ability on secondary fire, 1 s cd | heals 90/s up to 3.33 s | - | - | - | - | - | - | - | Bastion:L94-103 |
| Brigitte | Rocket Flail | melee weapon (infinite) | 35 | 1 swing/0.6 s | inf | none | - | - | range 6 m | no | Brigitte:L50-61 |
| Brigitte | Whip Shot (E/ability 1), Shield Bash (primary while shield up) | abilities | 70 (4 s cd) / 5 (7 s cd) | - | - | - | whip 80 m/s | - | - | no | Brigitte:L87-102, L127-144 |
| Brigitte | Barrier Shield | ability on secondary fire, 5 s cd if destroyed | - | - | - | - | - | - | - | - | Brigitte:L110-120 |
| Cassidy | Peacekeeper primary | hitscan revolver | 21-70 | 0.496 s | 6 | 1.5 s anim, ammo back 0.8 s | - | 20-40 m | pinpoint | yes | Cassidy:L33-46,L40 |
| Cassidy | Fan the Hammer (secondary) | alt fire, dumps remaining rounds | 15-50 per shot | 0.128 s per shot | uses up to 6 | same | - | 20-40 m | constant 6.5 deg | no | Cassidy:L52-66,L59,L62 (L68: "alternate fire is also called Fan the Hammer"). No cooldown. |
| D.Va | Fusion Cannons (mech) | hitscan shotgun | 0.6-2 per pellet | 0.144 s | inf | none | - | 10-20 m | 11 pellets, constant 4.15 deg | yes | DVa:L45-58 |
| D.Va | Light Gun (pilot) | projectile auto | 14 | 7/s (0.128 s) | 20 | 1.4 s | 50 m/s | none | pinpoint | yes | DVa:L65-75 |
| D.Va | Defense Matrix (mech, secondary) | ability, resource meter, 1 s cd | - | 0.5-2 s | - | - | - | - | - | - | DVa:L84-88 |
| Doomfist | Hand Cannon | projectile shotgun | 1.8-6 per pellet (19.8-66 per shot) | 0.32 s | 4 | auto: 0.32 s initial + 0.656 s per bullet | 80 m/s | 15-30 m | 11 pellets, constant 2.15 deg | yes | Doomfist:L53-69 |
| Doomfist | Rocket Punch | ability on secondary fire, 4 s cd | 50-100 (150 wall) | charge 0.192-2 s | - | - | 45 m/s | - | - | - | Doomfist:L114-125 |
| Echo | Tri-Shot | projectile | 17 per pellet (51/shot) | 0.3 s | 12 | 1.5 s anim, back 1.04 s | 75 m/s | none | 3 pellets triangle | yes | Echo:L39-51 |
| Echo | Sticky Bombs | ability on secondary fire, 6 s cd | 6 bombs: 5 impact, 25 explosion | 0.064 s per bomb + 0.3 s | - | - | 50 m/s | - | 6 bombs | - | Echo:L55-67 |
| Genji | Shuriken primary | projectile burst | 29 per star | 3 stars per 0.88 s (0.096 s apart) | 30 | 1.5 s, back 0.912 s | 60 m/s | none | pinpoint | yes | Genji:L57-71 |
| Genji | Shuriken secondary | alt fire, fan of 3 | 29 each | 3 stars per 0.68 s | 30, uses 3 | same | 60 m/s | none | spread 9 deg | yes | Genji:L75-89,L86 |
| Hanzo | Storm Bow | projectile arc, charge | 27.2-125 | 0.5 s recovery | inf | none | 25-110 m/s | none | pinpoint | yes | Hanzo:L44-58 |
| Junkrat | Frag Launcher | arc projectile, bounces | 120 direct; splash 10-80, r 2 m | 0.64 s | 5 | 1.5 s anim, back 0.848 s | 25 m/s | none | pinpoint | no | Junkrat:L44-58 |
| Junkrat | Concussion Mine | ability 1; detonated with secondary fire | 30-120 | 8 s per charge x2 | - | - | 25 m/s | - | - | - | Junkrat:L65-81 |
| Lucio | Sonic Amplifier | projectile 4-round burst | 20 per round | burst per 0.92 s (0.112 s apart) | 20 | 1.5 s, back 1.088 s | 50 m/s | none | pinpoint | yes | Lucio:L51-65 |
| Lucio | Soundwave | ability on secondary fire, 4 s cd | 25 + knockback | 0.56 s primary recovery | - | - | 8 m range | - | - | - | Lucio:L72-82 |
| Mei | Endothermic Blaster primary | projectile-like beam | 2.75 per 0.048 s (55/s) | continuous | 120, drain 20/s | 1.5 s, back 0.896 s | 20 m/s | none, range 10 m | - | no | Mei:L31-45,L40-42 |
| Mei | Icicle (secondary) | alt fire projectile | 75 | 0.8 s | 120, 10 per shot | same | 115 m/s | none | pinpoint | yes | Mei:L55-70,L66 |
| Mercy | Caduceus Staff primary / secondary | targeted beams (heal 55/s; damage boost +30%) | - | continuous | - | - | - | range 15 m | - | - | Mercy:L55-62 (heal 55/s at Mercy:L61 in the Staff block; range 15 m in that block) |
| Mercy | Caduceus Blaster | projectile auto (second weapon, swap key) | 20 | 5/s (0.2 s) | 20 | 1.4 s | 50 m/s | none | pinpoint | yes | Mercy:L74-87 |
| Moira | Biotic Grasp primary (hold) | heal beam, 70/s | - | continuous | 160 energy, 12.5/s | - | - | range 15 m | - | no | Moira:L33-45 |
| Moira | Biotic Grasp secondary (hold) | damage beam | 50/s (9.6 per 0.192 s), heals self | continuous | inf (drains nothing; regens energy) | none | - | range 20 m | - | no | Moira:L56-66,L62,L64-65 |
| Orisa | Fusion Driver | projectile auto | 11 | 12/s | 150 | 2.5 s | 120 m/s | none | constant 1.2 deg | yes | Orisa:L39-52; -30% move while firing |
| Orisa | Halt! | ability on secondary fire, 6 s cd | pulls/slows | launch 0.1 s, detonate 0.45 s | - | - | 25 m/s | - | r 5 m | - | Orisa:L56-68 |
| Pharah | Rocket Launcher | projectile | 120 direct; splash 24.6-80 (self 12.3-40), r 2.5 m | 0.848 s | 6 | 1.5 s, back 0.6 s | 35 m/s | none | pinpoint | no | Pharah:L47-65 |
| Reaper | Hellfire Shotguns | hitscan shotgun | 1.8-6 per pellet (36-120) | 0.496 s | 8 | 1.5 s, back 1.224 s | - | 10-20 m | 20 pellets, spread 6 | yes | Reaper:L44-58,L51-52 |
| Reinhardt | Rocket Hammer | melee weapon | 85 | 0.96 s (0.416 s windup, damage 0.208 s) | inf | none | - | range 5 m | - | no | Reinhardt:L53-64 |
| Reinhardt | Barrier Field | ability on secondary fire (hold), 5 s cd if destroyed | - | - | - | - | - | - | - | - | Reinhardt:L69-80 |
| Roadhog | Scrap Gun primary | projectile shotgun | 1.98-6.6 per pellet (49.5-165) | 0.848 s | 5 | 2 s anim, back 1 s | 80 m/s | 15-30 m | 25 pellets, constant 20.1 deg | yes | Roadhog:L29-44,L37-38 |
| Roadhog | Scrap Gun secondary | alt fire, flak: 50 pre-detonation then 25 pellets | same pellets | 0.848 s | 5 (shared) | same | 80 m/s | 15-30 m from detonation | 25 pellets | yes | Roadhog:L48-62,L55,L58 |
| Sigma | Hyperspheres | arc projectile pair | 55 direct each, splash 9-30, self 2.25-7.5, r 3 m | 2 shots per 1.48 s (0.192 s apart) | inf | none | 50 m/s | none | range 22 m | no | Sigma:L31-43,L39 |
| Sigma | Experimental Barrier | ability on secondary fire, 2 s recall / 5 s destroyed cd | - | - | - | - | 16.5 m/s | - | - | - | Sigma:L54-66 |
| Soldier: 76 | Heavy Pulse Rifle | hitscan auto | 6-20 | 10/s (0.096 s) | 30 | 1.5 s, back 0.736 s | - | 30-50 m | (spread not in dump) | yes | Soldier_76:L32-43,L39 |
| Soldier: 76 | Helix Rockets | ability on secondary fire, 6 s cd | 120 direct, splash 40-80, self 20-40, r 3 m | 0.496 s primary recovery | - | - | 50 m/s | - | - | no | Soldier_76:L51-63 |
| Sombra | Machine Pistol | hitscan auto | 2.4-8 | 20/s (0.048 s) | 60 | 1.4 s, back 0.544 s | - | 15-35 m | max 2.4 deg | yes | Sombra:L44-57,L50-51 |
| Sombra | Hack | ability on secondary fire, 8 s cd | - | cast 0.65 s | - | - | range 15 m | - | - | - | Sombra:L61-71 |
| Symmetra | Photon Projector primary | beam, damage tiers 60/120/180 per s | - | continuous | 70, drain 7/s (+7/s vs barriers) | 1.5 s, back 1.088 s | - | range 12 m | - | no | Symmetra:L29-42,L36,L40 |
| Symmetra | Photon Projector secondary (hold) | alt fire, charged orb | 120 direct, splash 15-60, r 2 m | charge up to 9 rounds/s, 0.25 s recovery | 70, 1-10 per shot | same | 25 m/s | none | - | no | Symmetra:L54-67 |
| Torbjorn | Rivet Gun primary | arc projectile | 70 | 0.608 s | 18 | 2 s, back 0.992 s | 70 m/s | none | pinpoint | yes | Torbjorn:L32-44 |
| Torbjorn | Rivet Gun secondary | alt fire, shotgun | 3.75-12.5 per pellet (37.5-125) | 0.8 s | 18, 3 per shot | same | 120 m/s | 10-20 m | 10 pellets, constant 4.15 deg | yes | Torbjorn:L52-67,L59-65 |
| Torbjorn | Forge Hammer (weapon 2, swap key) | melee | 55 | 0.848 s (0.352 s windup) | - | - | - | - | - | no | Torbjorn:L74-85 |
| Tracer | Pulse Pistols | hitscan auto | 1.8-6 per bullet, 2 per shot | 20 shots/s (0.048 s per pair) | 40 | 1 s anim, back 0.686 s | - | 12-20 m | 2 bullets, max 3.6 deg | yes | Tracer:L31-46 |
| Widowmaker | Widow's Kiss scoped (secondary, hold) | hitscan charge | 12 (0% charge) to 120 (100%); headshot x2.5 | charge up to 0.9 s, 0.5 s recovery | 35, 5 per shot | 1.5 s, back 0.816 s | - | 70-100 m | pinpoint | yes | Widowmaker:L31-48,L39,L43,L45; scope 0.32 s, -65% move |
| Widowmaker | Widow's Kiss automatic | hitscan auto | 3.9-13 | 10/s | 35 | 1.5 s | - | 20-40 m | max 3.0 deg | yes | Widowmaker:L59-72,L66 |
| Winston | Tesla Cannon | multi-target beam | 5.76 per 0.096 s (60/s) | continuous | 100, drain 20/s | 1.7 s, back 1 s | - | range 8 m | 6 m wide | no | Winston:L30-42,L37 |
| Wrecking Ball | Quad Cannons | hitscan auto | 1.5-5 | "25/s", "1 round per 0.032 s" (wiki disagrees with itself: 0.032 s = 31/s) | 80 | 2 s, back 0.976 s | - | 15-25 m | ~2 deg | yes | WreckingBall:L27-39,L34-36 |
| Wrecking Ball | Grappling Claw | ability on secondary fire, 5 s cd | 50 at speed | - | - | - | range 23 m | - | - | - | WreckingBall:L46-57 |
| Zarya | Particle Cannon primary | beam | 75/s at 0% charge (14.4 per 0.192 s) to 170/s at 100% | 20 rounds/s | 100 | 1.5 s, back 1.04 s | - | range 15 m | - | no | Zarya:L47-59 |
| Zarya | Particle Cannon secondary | alt fire, arc grenade | 9.4-47 (0%) to 19-95 (100%), self half; r 2.5 m | 1 s recovery | 100, 25 per shot | same | 25 m/s | none | - | no | Zarya:L65-81,L77 |
| Zenyatta | Orb of Destruction primary | projectile | 48 | 0.4 s | 20 | 1.5 s, back 1.182 s | 90 m/s | none | pinpoint | yes | Zenyatta:L28-43 |
| Zenyatta | Orb of Destruction secondary (hold) | alt fire, volley | 48 per orb, max 240 | 0.524 s per orb charge, 0.144 s per orb fired, 0.6 s recovery | 20, up to 5 per volley | same | 90 m/s | none | pinpoint | yes | Zenyatta:L47-61,L57 |

Notes on the table
- Hanzo: Storm Bow only. Sonic Arrow, Scatter Arrow and Dragonstrike are abilities (not covered here).
- Winston and Pharah have no secondary fire in the OW1 pages (Winston Jump Pack and Pharah Jump Jet are other keys).
- Cassidy's Fan the Hammer is the one "classic" secondary fire that is an alt fire with ammo use, not a cooldown ability. Soldier Helix (6 s), Orisa Halt (6 s), Lucio Soundwave (4 s), Echo Sticky Bombs (6 s), Doomfist Rocket Punch (4 s), Sombra Hack (8 s), Wrecking Ball Grapple (5 s), Sigma Barrier, Bastion Self-Repair, Reinhardt Barrier, Brigitte Barrier are RMB abilities with cooldowns. Ashe Coach Gun is Shift (ability 1) and Dynamite is E (ability 2), not RMB (Ashe L106, L84). Ana Sleep Dart is ability 1, not RMB (Ana:L62-80, `key = ability 1`).
- Infinite ammo / no reload: Hanzo, Sigma, Reinhardt, Brigitte, D.Va mech Fusion Cannons; Moira (energy, regen); Doomfist (auto per-bullet reload); Symmetra primary uses a 70-ammo meter with reload (not infinite); Torbjorn rivet gun auto-reloads after 2 s with hammer out (Torbjorn L46,L69).

### Quick melee
| Hero | Damage | Cooldown | Range | Cite |
|---|---|---|---|---|
| Default (everyone else, incl. Doomfist, Winston, Wrecking Ball, Genji, D.Va, Moira) | 30 | 1 s; starts on use, active 0.528 s | 2.5 m | Melee (web, rev 2022-08-14) L5 |
| Reinhardt | none; Rocket Hammer 85 is his melee | 0.96 s | 5 m | Reinhardt:L59-62; Reinhardt:L482 (no quick melee) |
| Brigitte | none; Rocket Flail 35 | 0.6 s | 6 m | Brigitte:L58-61 |
| Torbjorn (hammer equipped) | quick melee button swings Forge Hammer 55 | 0.848 s | - | Torbjorn:L80-81; Melee L10 |
| Genji during Dragonblade | no quick melee; sword 110 | 0.9 s | 5 m | Genji:L144-147 |
| Winston during Primal Rage | no quick melee; 40 per swing | 0.448 s recovery | 4 m | Winston:L92-95 |
| Others noted: D.Va (both arms in mech, random), Moira (both hands random), Sombra (right hand) | 30 | 1 s | 2.5 m | DVa:L479, Moira:L358, Sombra:L511 |

## 2. Mismatches with `tools/lobby/target.py` (HERO_KIT / PLAYER_KIT)

Everything not listed matches the wiki (damage, far damage, falloff, shot interval, magazine, reload, speed, pellets, reach). The 12 bot heroes and 22 player-only entries were each checked. Only real differences and the arguable ones follow.

| # | Hero | Field | Project | Wiki | Wiki source |
|---|---|---|---|---|---|
| 1 | Soldier: 76 | shot_seconds | 1/9 = 0.1111 (comment: "the player's rate (soldier.Rifle)") | 0.096 s (10/s) | Soldier_76:L40 |
| 2 | Bastion (Recon) | spread | 0.0 | max 1.2 deg | Bastion:L52 |
| 3 | Sombra | spread | 0.0 | max 2.4 deg | Sombra:L57 |
| 4 | Widowmaker (auto fire) | spread | 0.0 | max 3.0 deg | Widowmaker:L72 |
| 5 | Moira | damage / purpose | 0 damage, rate 0.192 (the heal beam); the damage beam (secondary fire) is omitted | secondary fire: 50/s = 9.6 per 0.192 s, range 20 m | Moira:L62 (range L65) |
| 6 | Wrecking Ball | shot_seconds | 0.032 (31/s) | "25 shots per second" = 0.04 s, yet the same cell says 0.032 s; wiki is self-contradictory | WreckingBall:L36 |
| 7 | Ashe | reload_seconds | 3.5 (full from empty) | per-bullet: 0.5 s + 0.25 s per bullet (12 bullets = 3.5 s); cancellable | Ashe:L42 |
| 8 | Doomfist | reload_seconds | 0.32 + 4 x 0.656 (full from empty) | per-bullet auto reload, cancellable by firing | Doomfist:L66 |
| 9 | Hanzo | damage | 125 (full charge) | 27.2-125 by charge; speed 25-110 m/s (project uses 110 only) | Hanzo:L50,L52 |

Arguable or convention items (no change needed unless the engineer wants wiki-literal values)
- Roadhog spread 10.05 and Reaper 3.0 are half the wiki "20.1" and "6" (project convention); Tracer 3.6 and Ashe 1.85 are the wiki's "Max" values used directly; Doomfist 2.15 and Orisa 1.2 are used directly as "Constant" values. The conventions are mixed, so the wiki's "constant" vs "max" wording is not applied uniformly.
- Lucio: project treats the 4-round burst as one 0.23 s cadence (0.92/4); wiki: first round instantly, then 0.112 s apart, 0.928 s recovery (Lucio:L60). Genji (0.88/3) and Baptiste (0.58) are the same style.
- Cassidy's 70/21 and every other direct damage number match.
- Mercy, D.Va Light Gun, Sigma, Symmetra, Winston, Brigitte, Reinhardt, Zarya, Hanzo, Echo, Orisa, Pharah, Junkrat, Tracer, Torbjorn, Ana, Zenyatta, Roadhog, Reaper, Cassidy, Genji, Baptiste, Mei, Lucio: no field mismatches.
- No heroes in target.py carry quick melee or secondary-fire data; section 1 lists what exists.

## 3. First-person animation notes (what the wiki says)

The wiki records almost nothing about animations. Everything below is stated in the dump; where the dump is silent, that is said.

Reload animation structure
- Ashe: reloads one bullet at a time; longer the emptier the magazine; cancelling keeps the bullets already loaded (Ashe:L45-47, L160). Time: 0.5 s start + 0.25 s per bullet, 3.5 s from empty (Ashe:L42).
- Doomfist: Hand Cannon reloads automatically, 0.32 s initial + 0.656 s per bullet (Doomfist:L66); the wiki's patch note says quick melee no longer pauses the reload (Doomfist:L638).
- Roadhog and Junkrat: the wiki does NOT say they reload per shell or that the reload can be interrupted. Roadhog: 2 s animation, ammo restored after 1 s (Roadhog:L41); Junkrat: 1.5 s animation, restored after 0.848 s (Junkrat:L56). Treat these as normal single-motion reloads; the "ammo returns before the animation ends" pattern holds for most heroes (the `reload_time` cell says "animation / restores ammo after X").
- Reaper: ammo returns roughly when the shotguns are fully released and drop out of view, so the rest of the animation can be cancelled by acting (Reaper:L164-165); Wraith Form also reloads (Reaper:L165).
- Sombra: reload completes slightly after the magazine is inserted, before the animation ends; Hack can cancel it (Sombra:L160-161); she can reload while cloaked (Sombra:L174).
- Soldier: 76: Biotic Field's activation animation interrupts the reload (Soldier_76:L141).
- Torbjorn: the Rivet Gun reloads on its own after the Forge Hammer has been out 2 s (Torbjorn:L46,L69); the ammo is loaded earlier in the animation since a patch (Torbjorn:L534).
- Wrecking Ball: reloads on its own after 2 s in Roll form (WreckingBall:L42,L155).
- Mercy: the Caduceus Blaster reloads on its own while the Staff is out (Mercy:L184).
- Bastion: the other configuration reloads itself after 1.5 s out of that mode (Bastion:L90, L157).
- Cassidy: Combat Roll fully reloads the Peacekeeper "without a reloading animation" mid-roll (Cassidy:L76, L173, L195).
- Tracer: Recall fully reloads (Tracer:L81).
- Baptiste: one reload restores both the burst and the grenade ammo (Baptiste:L60).
- Hanzo, Sigma, Reinhardt, Brigitte, D.Va mech cannons: no reload. Moira: no reload (energy). Symmetra and Winston have normal reloads (1.5 s and 1.7 s) with a 70/100 ammo meter that drains while the beam is held.

Handedness and melee animation
- Quick melee: left hand for most heroes; Sombra uses her right hand (Sombra:L511); D.Va (mech) and Moira pick a hand at random (DVa:L479, Moira:L358).
- Reinhardt and Brigitte have no quick melee; their primary fire is the melee swing (Reinhardt:L165,L482).

Alt-fire / secondary-fire animations (timings the wiki gives)
- Ana scope: 0.24 s in, 0.16 s out (Ana:L49); Widowmaker scope 0.32 s in/out plus 0.9 s charge (Widowmaker:L47); Ashe ADS 0.192 s in / 0.160 s out (Ashe:L69).
- Cassidy Fan the Hammer: 0.128 s between shots, constant 6.5 deg spread, empties up to six rounds (Cassidy:L60,L62,L66). Reload after is the normal 1.5 s animation.
- Soldier Helix Rocket: 0.496 s recovery for the primary after firing (Soldier_76:L62).
- Roadhog secondary: the flak shell detonates mid-flight into a 25-pellet burst (Roadhog:L53-58); same 0.848 s recovery as the primary.
- Torbjorn secondary: 0.8 s recovery; Genji fan 0.68 s; Mei Icicle 0.4 s windup + 0.4 s recovery (Mei:L69); Baptiste grenade has a 0.144 s recovery for the primary (Baptiste:L57,L74); Zenyatta secondary charge 0.524 s per orb, 0.6 s recovery (Zenyatta:L55).
- Symmetra secondary: charges up to 9 rounds/s, fires on release, 0.25 s recovery (Symmetra:L62).
- Zarya grenade: 1 s recovery, 0.48 s before the primary can resume (Zarya:L74,L81).
- Orisa Halt! launch 0.1 s, detonation press 0.45 s (Orisa:L67); Junkrat mine throw 0.128 s, detonate recovery 0.112 s (Junkrat:L76); Ashe Coach Gun 0.16 s cast + 0.4 s recovery (Ashe:L112); Sombra Hack 0.65 s cast, 0.3 s recovery (Sombra:L71).
- "Recovery stops while using melee" is attached to the fire-rate cell of most projectile weapons (e.g. Ana:L43, Lucio:L60, Pharah:L57): melee does not extend a weapon's recovery.
- Wiki notes with no animation detail: Ashe, Cassidy, Roadhog, Junkrat reload or fire "feel" is never described; confirm those in-game.

## 4. Items the wiki does not settle
- Soldier: 76, Torbjorn secondary spread vs pellet cone, Hanzo spread, Genji deflect-frame timing: not in the dump (Soldier has no spread cell).
- Quick melee animation and recovery are only from the web `Melee` page (first-party data). The 0.528 s "active" figure and the 1 s cooldown are from it; the dump has no per-hero quick-melee entry.
- Orisa's Augmented Fusion Driver (Orisa:L143-154) and Doomfist L158+ belong to Overwatch 2 and were excluded.
