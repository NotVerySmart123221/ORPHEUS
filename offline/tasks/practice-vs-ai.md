# Phase 3: Practice vs AI

Goal: from the menu, start a match on a real map against server-run hero bots, played to a result.

## Entry route (decided for now)

- The client in tournament mode greys out Training and locks the Game Browser ("unavailable while in a group"). No native Practice vs AI card.
- The PLAY screen's **Tournament Match card** opens the vs-AI match (`--map NAME [--mode GUID] [--difficulty easy|medium|hard]` on the launcher); the Practice Range card always opens the Range (2026-11-03). shortcut: the card still shows its padlock and the difficulty comes from the launcher, not an in-game choice (`research/play-menu.md`).
- The native Practice vs. AI card (graph `0x0B7F`) only shows in the retail PLAY layout. The tournament flag picks the tournament layout inside the client, so the server cannot change it (`research/play-menu.md`).

## Steps (each one verified in game, then committed)

1. **Real map loads.** Done: Hanamura Skirmish, native "Defend Hanamura" hero select, Soldier spawns with HUD (commit 125c591). Needed the map header in the login content record. Assault also plays on Temple of Anubis, Volskaya Industries and Horizon Lunar Colony (`--map NAME --mode 0x0230000000000014`; `research/assault-capture/*-attack.png`). Paris too (`paris-attack.png`); its collision mesh carries a second small mesh, which is skipped.
2. **Enemy hero bots at the enemy spawn.** Done: six Soldier: 76 hero bodies (component 123 = hero, Soldier hit volumes) on the enemy team's spawns. Checked by placing them on the player's spawn: the hero model renders, they shoot the player dead in ~3 s, the player respawns.
3. **Bots fight.** Done: Soldier's rifle (hitscan, 9/s, 30 rounds, 1.5 s reload, falloff, no headshots) with wiki difficulty damage 25/50/100% and a guessed aim error per difficulty (`--difficulty`, default medium). Checked by placing the bots on the player's spawn: 10 damage per hit at 1/9 s each. Left: no tracer or muzzle flash, bot targets beyond the player (done in step 6).
4. **Bots move.** Done: A* over a walkable grid sampled from the collision mesh (`nav.py`). Bots walk from the attacker spawn to Point A and stop to fight. Checked by routing them into the player's spawn: they walked ~200 m in, stopped at range and shot. Chase and spread: bots take their own slot on a ring round the point and hunt a fresh sighting near it (`Match._steer_bots`; ring, leash and memory are guesses). Checked from the server log on Hanamura. Engaged bots strafe 2 m to a random side each 0.5 s (`Match._strafe`, a guess).
5. **Objective mode.** Hanamura Assault (0x14) already loads with the assault emblem on hero select; phases on the objective widget, capture of A then B, round complete (`assault.py`). Checked by forcing an uncontested attacker on the point, and by walking the player from spawn onto point A with bots off (`tools/research/walkplan.py`, three replanned parts): "captured" fired and the widget turned to "Attack objective B" with 4:00 added. A time-out with nobody on the point logged "lost". Match end: 8 s after the round-complete screen the server sends 20304 and the client returns to the PLAY menu; a new match starts from there. Left: capture progress on the HUD (icons stay locked), the round-complete screen's round number and time (the score and, on a loss, the capture progress are sent: v7899 | v7901, v2198/v8018), a victory/defeat screen (not found in F4A's slots or SubScripts).
6. **Allies.** Done: five Soldier bots on the player's team; every hero bot shoots the nearest enemy in sight, player or bot. Attacking bots wait out setup. Checked in game: with the player idle in spawn, the allies took point A and fought at B. Allies ring round the player when it is within 30 m of the point and no enemy was just seen.
7. **Roster.** Done: each team draws its heroes from the 12 (`target.HERO_KIT`: health and primary fire from the last Overwatch 1 wiki revisions; hit volumes from each body's model). The login record advertises them so their bodies load; hero select then lists them, and any pick plays Soldier. Checked in game: all 12 bodies render. Projectiles fly at their wiki speed and can be dodged. Healer bots heal teammates and each hero bot uses one signature ability, server-only (nothing drawn). Left: drawing abilities, a player hero other than Soldier, Zarya's beam (her bots fire unseen). Every other hero's fire shows (ch2 shot events).

## Decisions (2026-11-02)

- First objective mode: Assault on Hanamura.
- Team size: the player alone against 6 bots until step 6.
- Bot heroes: Soldier: 76 only, the other 11 in step 7.

## Wiki facts (Practice vs. AI page, revisions 2019-11 to 2022-03, research/wiki/Overwatch+Wiki-20261102170340.xml)

- Six players against six AI. Listed under Training in the menu.
- Difficulty Easy / Medium / Hard: AI deals 25% / 50% / 100% damage; higher difficulty also aims better. Bots never headshot.
- Modes chosen at random: Assault, Control, Escort, Hybrid.
- AI heroes (2020-07 list): Roadhog, Zarya, Bastion, McCree, Mei, Reaper, Soldier: 76, Sombra, Torbjörn, Ana, Lúcio, Zenyatta. The 2019-11/2020-05 list also had Mercy, Reinhardt and Tracer instead of Zenyatta.
- 90% experience (for the later progression phase).

## Data

- Map catalog: `tools/lobby/vendor/data/game_maps_174.json` (spawns, objectives, placeables per mode).
- Mode/map/AI asset survey: `research/practice-vs-ai-data.md` (data agent).
