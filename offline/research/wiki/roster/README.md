# Practice vs AI roster wiki pages

Wikitext of the 12 Practice vs AI heroes' pages (and, below, ten more for `PLAYER_KIT`) on the Overwatch Wiki (overwatch.fandom.com), each at its last revision before 2022-10-01: the final Overwatch 1 state, before Overwatch 2's launch. Fetched with the MediaWiki API (`action=query&prop=revisions&rvstart=2022-10-01T00:00:00Z&rvdir=older&rvlimit=1`). Wiki content is CC BY-SA.

| Hero | Revision | Primary fire used in `tools/lobby/target.py` `HERO_KIT` |
|---|---|---|
| Ana | 2022-09-26 | Biotic Rifle |
| Bastion | 2022-09-20 | Configuration: Recon |
| Cassidy | 2022-09-30 | Peacekeeper |
| Lúcio | 2022-09-30 | Sonic Amplifier |
| Mei | 2022-09-30 | Endothermic Blaster |
| Reaper | 2022-09-26 | Hellfire Shotguns |
| Roadhog | 2022-09-30 | Scrap Gun |
| Soldier: 76 | 2022-09-30 | Heavy Pulse Rifle |
| Sombra | 2022-09-30 | Machine Pistol |
| Torbjörn | 2022-09-30 | Rivet Gun |
| Zarya | 2022-09-30 | Particle Cannon |
| Zenyatta | 2022-09-13 | Orb of Destruction |

Health is the infobox `health` plus `armor` and `shield`.

Player-only heroes (`target.PLAYER_KIT`), fetched the same way on 2026-11-03: Tracer (2022-09-30, Pulse Pistols), Pharah (2022-09-30, Rocket Launcher), Widowmaker (2022-09-13, Widow's Kiss automatic fire), Junkrat (2022-09-30, Frag Launcher), Orisa (2022-09-21, Fusion Driver; OW1 health 200 + armor 200), Wrecking Ball (2022-09-30, Quad Cannons), Ashe (2022-09-30, The Viper hip fire), Echo (2022-09-30, Tri-Shot), Baptiste (2022-09-26, Biotic Launcher), Mercy (2022-09-26, Caduceus Blaster). Later the same day: Hanzo (Storm Bow), Symmetra (Photon Projector), Genji (Shuriken), D.Va (Light Gun, pilot), Doomfist (Hand Cannon), Moira (Biotic Grasp), Sigma (Hyperspheres), Reinhardt (Rocket Hammer), Winston (Tesla Cannon), Brigitte (Rocket Flail); Zarya is in the bot roster.
