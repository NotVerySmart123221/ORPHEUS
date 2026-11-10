# Tab board

Build 1.74.0.0.104319. Seen live on 2026-11-10 in a separate offline service; see the
[index](README.md) for the evidence labels and the address convention.

## Summary

The scoreboard key did nothing because the client holds the board shut until a game message
that only a server-sent state of the mode graph posts. Turning that one state on opens the board
in every mode tried. The board's own numbers are component 64 of the local player's entity, with
the layout already described in `game/stats.py`, except that its stat GUID fields use the compact
id encoding.

## Why the key does nothing

Read from data, then traced live in the client:

1. The HUD's host graph `041C` runs on a client-made entity. It starts the board's graph `01BE`
   and, beside it, graph `0CC9`.
2. In `01BE` the key works. While logical button 77 is held, a Stack (state 15) sets
   variable 1729, "board open". Traced: state 15 began and ended with the key, and the board's
   presenter (state 24) was on the whole time.
3. `01BE` also holds variable 1729 off with a second Stack that has a priority (state 17), for
   as long as variable 6699 is off. Traced: state 17 never ended.
4. Only `0CC9` turns variable 6699 on: while game message `06DF.025` is being posted (its
   state 3) and `13BF.025` is not (its state 13).
5. `06DF.025` is posted by `STU_38EE1100` states of the mode graphs. There are 19 of them in the
   data, each a remote-synced state. A client runs a mode graph's remote states only as the
   server sends them.

So the server has to carry the mode's own graph on the game mode entity, with the one state that
posts `06DF.025` on.

The state's remote data is one bit after its start bit: 0, for "no optional integer". This was
calibrated live on another mode graph (`0D28`). Without the bit every later slot of the frame is
read one bit off.

## Which state, by mode

Slots are indexes into the graph's remote-synced state list.

| Mode | Mode graph | Remote slot | Seen live |
| --- | --- | --- | --- |
| Practice Range `0x18`, and its other mode `0x5A` | `13C0` | 4 | Board opens. |
| Skirmish `0x10` | `0D76` | 2 | Board opens (Hanamura). |
| Assault, Escort, Hybrid | `0F4A` | Per phase: assemble 4, setup 7, attack 10, round complete 26 | Opens in Assault's setup and attack, and in the attack phase of Escort (Dorado) and Hybrid (Eichenwalde). The other phases were not run. |
| Control `0x17` | `0EC2` | 1 | Board opens, and the mode's capture header shows (0%, lock, 0%). |
| Deathmatch `0x1D` | `0F60` | 0 | Board opens with the mode's own stats (Final Blows, Solo Kills). |
| Team Deathmatch `0x1E`, `0x70` | `0F73` | 0 | Board opens. |
| Elimination `0x07` | `0CE8` | 1 | Board opens. |
| Capture the Flag `0x09` | `0D0B` | 6 | Board opens. |
| Mode `0x46` (stats: Enemies Frozen, Teammates Thawed) | `1A59` | 6 | Board opens. |
| Uprising, Retribution, Snowball Offensive | `0D28`, `100B`, `0CE4` | 0, 0, 5 | Not run. |

- A mode graph also starts client-only HUD states. Control's capture header above is one. Mode
  `0x67`, an Assault variant with graph `29A6`, put an "UNSPECIFIED 0.0" objective header up and
  was left out.
- Not run: Lucioball `0x20`, Yeti Hunt `0x29`, Junkenstein `0x03` and `0x2A`.

## What the board shows

- The map, the mode and the difficulty.
- A tile per player entity of each team, bots included, with a dead one crossed out.
- The local player's own numbers, from component 64 of that player's entity.

## Component 64

`game/stats.py` and `game/onfire.py` describe the layout. Two things were settled live:

- **The 14 stat GUID fields (5 to 18) are the compact id kind**, a byte mask followed by the
  non-zero bytes, not a raw u64. Sent as raw u64, the board showed three "UNKNOWN STAT" entries.
  Sent as ids, every stat had its name.
- **The slot assignment in `stats.py` is right.** Each stat was sent with a number of its own:
  fields 5 to 12 are the mode's player stats, with their values in 21 to 28 and their medals in
  37 to 44, and fields 13 to 18 are the hero's own stats, with their values in 29 to 34. The
  client names each stat and prints its value by the stat's display type (`11`, `4,444`,
  `00:33`, `42%`).

Sent in the entity's create and again when a number changed, at most twice a second: a kill
showed "1 ELIMINATION" with a gold medal and "35% WEAPON ACCURACY", and a death "1 DEATH".

For Soldier: 76 in the Practice Range the lists were Eliminations, Objective Kills, Objective
Time, Hero Damage Done, Healing Done and Deaths, then Weapon Accuracy, Kill Streak - Best, Final
Blows, Helix Rocket Kills and Tactical Visor Kills.

## On Fire

- Component 64's field 36 burns the portrait, as `game/onfire.py` reads it. Seen live: no flames
  after two kills of the same bot, flames from the fourth.
- The rule behind the score is not in the client. `003F.054`'s second table is 20 keyed
  settings (key 19 = 90.0, key 7 = 0.04 and so on), not a rule.
- The rule used was a community reading of retail
  ([PCGamesN, 20 June 2016](https://www.pcgamesn.com/overwatch/overwatch-on-fire-guide)): on
  fire from 250 points, 5 points a second draining all the time, and an elimination worth the
  percent of the target's health the player took. That percent is the score byte of
  message 25005.

## The hero switch line

`game/notices.py` already sends it. Seen live in Skirmish: "Player switched to Reaper (was
Soldier: 76)", one line, once. The name is the player entity's component 29 `m_battleTag`, which
also names the player's tile on the board and the label under the ultimate meter.

## Still wrong on the board

- **Every tile's ultimate charge reads 0%**, the local player's included. Ruled out live: the
  body's variables 342 and 338 copied to the player entity, component 64's field 35, and
  component 74's fields 1, 4 and 8.
- **The board's time stays at 0:00.** Its text reads variable 2499 through the game mode entity
  (`01BE`'s presenter), but a mode entity given variable 2499 = 125.0 still showed 0:00.

## Compared with ORPHEUS

- `combat.mode_frame` carries only the mode UI graph `0CB6` on the game mode entity, with its
  state 21. No frame carries a mode graph such as `13C0`, so nothing posts `06DF.025` and the
  board stays shut, as [WIP.md](../WIP.md) reports.
- `game/onfire.py` declares fields 5 to 18 of component 64 as `u64`. `game/world.py` already
  has the `id` kind those fields need.
- Proposed changes: add the mode's graph to the mode entity's frames with the slot above on, and
  declare the 14 stat GUID fields as `id`.
- How the two graphs sat together in the offline service's Practice Range frame: `13C0` was
  instance 1 with remote slot 4 on, and `0CB6` was instance 2, listed as the child of
  instance 1's state 21. ORPHEUS lists `0CB6` alone as instance 1, so its frames would change
  shape. That change is not worked out here.

## Open

- The ultimate charge on the tiles and the board's time, above.
- Medals were gold for any counted stat above zero. Ranking among teammates was not tried.
- The tools used for the live reads, a statescript tracer plugin and a script that lists every
  mode graph's posting slots, are not in this repository.
