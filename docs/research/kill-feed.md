# Kill feed

Build 1.74.0.0.104319. Seen live on 2026-11-09 in a separate offline service; see the
[index](README.md) for the evidence labels and the address convention.

## Summary

The server's part is small: give the dead body's script game message `003E.025`, and replicate
the pulser that results. Everything after that runs on the client, behind two gates on the game
mode entity's kill feed graph `0DEA`. The second gate reads an Options value that the client only
holds if the lobby sent it in message 20802. With that byte at zero the line is built and never
drawn.

## The path

1. The dead body's `01CF` takes `003E.025` (parameters: killer `0012`, crit `0095`, loadout
   `03E6`, assists `03E7`), stores the killer in `#392` and pulses `st108`, a ClientOnlyPulser.
2. On each client, `st108`'s client-only chain runs nodes 64, 65, then 63 or 62, then 66 or 67,
   and sends `0765.025` to the game mode entity (`STU_A3DEF368`).
   - Node 64 tests `STU_CD7F3617(owner)`, the body's name.
   - Node 65 tests `#392 && #392 != owner()`. When true, node 66 sends a line that names the
     killer. Otherwise node 67 sends one without.
3. The mode entity's `0DEA`, a client-only child of `0CB6`, takes `0765.025` and builds the line.

Read from data. Seen live: at the death the client evaluated `STU_CD7F3617`, then resolved
`STU_A3DEF368` to the mode entity, and in the same tick `0DEA`'s line path ran.

## Gate 1: `#9380e` lets the message in

- `0DEA`'s `0765.025` entry (node 4) is on only while `#9380e` is true (node 3). State 0, a
  BooleanSwitch on `#9380e`, also holds the feed's presenter (state 1).
- Stack state 2 sets `#9380e = !STU_A77AC2AC() && (STU_0D179CAF() || !STU_33C5B1DE() ||
  STU_CEFF290A())`. It is the idle subgraph of GameMessageEntry state 11, which waits for
  `07E3.025`. When that message comes, node 21 sets `#9380e` to false.
- So **`07E3.025` turns the feed off**. Its senders in the client data are `12B8` state 2 and
  `2983` state 0. `2983` posts `07E3.025` to the mode entity for the local
  player after `13B8.025`: the "hide the kill feed" side of a setting.

What the natives read (from code):

| Native | Offset | Meaning |
| --- | --- | --- |
| `STU_A77AC2AC` | `0xCE8580` | True only in game mode `0x0230000000000019`. |
| `STU_33C5B1DE` | `0x93A2E0` | The value of UI variable `0x0D800000000004AA`. |
| `STU_0D179CAF`, `STU_CEFF290A` | `0x92CFC0`, `0x92E0A0` | Unread: both jump into packed code. |

Seen live in the Practice Range: all four evaluated to 0, `0DEA` began states 0, 1, 2, 11, 17 and
21, so this gate was open without any help from the server.

## Gate 2: `#1962` shows the presenter

- The presenter (state 1) shows only while its condition `#1962` is true.
- Stack state 12 sets `#1962 = STU_016D65A8() ? (STU_F4C77F4A() || STU_CEFF290A()) :
  STU_CEFF290A()`.
- `STU_016D65A8` (`0x935C50`) is `!valid || [settings + 700]`. `valid` (`0x773E30`) is true when
  the game state is 4 and the account settings system is filled (its 16 bytes at `+224` and its
  byte at `+753`). The account settings system is entry 9 of the client's system table (vtable
  at image offset `0x26113C8`).
- `STU_F4C77F4A` (`0x1B7AB10`) evaluates `GetGameRulesetValue` on the mode entity for key
  identifier `0x2544`. The Practice Range's mode lists ruleset schema `0x0006`, the custom game
  general settings, so `0x2544` is probably its Kill Feed setting. That reading is an inference.
  The evaluator (`0xCCFAC0`) takes the expression's own value first.

Seen live: with byte `+700` at 0, `STU_016D65A8` and `STU_CEFF290A` both evaluated to 0, `#1962`
was false, and no line drew although gate 1 was open and the line was built.

## What fills byte `+700`

Read from code, in the Options screen:

- The copy to the UI (`0xAA74C0`) maps the settings system's `+700` to UI variable
  `0x0D80000000006426`, `+701` to `0x6047`, `+703` to `0x62BC` and `+704` to `0x5FE9`.
- The save (`0xAA78E0`) copies the struct at `+0x2B8` and fills byte `+4` from `0x6426`, `+5`
  from `0x6047`, `+7` from `0x62BC` and the int at `+8` from `0x5FE9`. That is the layout of
  20802's block `+0x120`, the block message 22203 saves.
- The Options defaults (`0xAA8650`) set `0x6426` and `0x62BC` to 1, and `0x6047` and `0x5FE9`
  to 0.

So retail's default has the feed on, but the settings system holds only what the lobby sent.
`+700` is `+0x2B8 + 4`: **byte `+0x4` of 20802's block `+0x120`**.

The block is the Gameplay page of Options: `+0x4` is Kill Feed Display, `+0x5` Always Skip Kill
Cam and `+0x7` Tips Display. Seen live: Always Skip Kill Cam set to On and saved through 22203
came back On at the next launch.

Seen live, with the settings system's bytes logged on change:

| Sent at login | Settings system | Result |
| --- | --- | --- |
| No 20802 | `+700` = 0 | No line. |
| 20802 with the five flags of `+0x108` on | No change at `+700` | No line. |
| 20802 with `+0x120` bytes `+0x4` and `+0x7` on | `+0x2BC` = 1, `+0x2BF` = 1 | `STU_016D65A8` = 1, the line draws. |

The last run set both default-on bytes together. Byte `+0x4` is the one `STU_016D65A8` reads;
`+0x7` alone was not tried.

The drawn line read "TRAINING BOT" (red, with the bot's icon), then "SOLDIER: 76" (blue, with the
hero's icon), beside the "YOU WERE ELIMINATED BY TRAINING BOT" notice.

## Bodies without `01CF`: the Practice Range bots

- A hero body carries `01CF`: `st108` is remote slot 15 and `#392` is presence bit 15. A presence
  bit counts the graph's instance-scope sync variables in slot order.
- The Range's four bot bodies (`1403`, `143E`, `146E`, `14ED`) list `0033`, `0BA8`, `0BE2`,
  `0BA3` and `174F`. `0BE2` references `0C18`, one of the 15 graphs in the data that post
  `0765.025`.
- `0C18` is `01CF`'s death part alone: entry `003E.025`, then `#10155` false, then state 8, a
  ClientOnlyPulser at remote slot 1, then the two `0765.025` sends. Its `#392` is presence
  bit 0. Its conditions are `01CF`'s: `STU_CD7F3617()`, then `#392 && #392 != owner()`, then
  `!#7082e || !#392.id(7082)` with a killer and `!#7082e` without.
- The client posts one line per pulser count it has not seen. A hero bot's body dies once, so
  its pulser goes out fresh at count 1. A Range bot is revived as the same body, so its count is
  its number of deaths.

Seen live: two Range bots killed gave two lines "SOLDIER: 76" to "TRAINING BOT", marked as the
local player's own; a second death of one of those bodies added one line. A Reaper bot gave
"SOLDIER: 76" to "REAPER" at its death and again for its next body, and the player's death read
"REAPER" to "SOLDIER: 76".

## Compared with ORPHEUS

- `game/combat.py` documents the same path and gives `003E.025` to the dead body's script.
  `game/script/driver.py` names both pulsers (`KILL_FEED_PULSERS`), and `combat.mode_frame`
  keeps `0CB6` on the mode entity. Instance variables are filtered by each graph's own
  server-only list (`replicated_vars`), so `01CF`'s `#392` is not held back by another graph's
  list.
- `content/player.py` builds 20802 from the schema's empty value and copies `+0x120` only when
  the profile has saved it. Before `GAMEPLAY_DEFAULTS` was added there, a new profile's
  **block `+0x120` went out as zeros**. By this note that hides the feed's presenter.
  It would also explain a feed that appears only after a player has opened and saved Options
  once, since 22203 then stores the client's defaults.
- Changed since: when `+0x120` was never saved, 20802 now carries the client's Options defaults
  for it, bytes `+0x4` and `+0x7` set (`GAMEPLAY_DEFAULTS`). A saved block is sent as saved.
  Covered by `tests/runtime/test_player_settings.py`.

## Seen live against ORPHEUS

2026-11-10, original client, with the option default above in the server and a profile that had
no saved settings. Practice Range, Soldier: 76, driven by a virtual gamepad through an input
automation tool.

- The enemy training bot was killed 12 times. The server logged each kill: five or six 25001
  hits, one 25005, then the respawn three seconds later. On screen the bot fell apart and
  came back.
- **No kill feed line drew.** Sixteen frames were saved about three seconds apart. Eight show
  the bot down, one of them with the elimination notice on screen, and none has anything in
  the kill feed corner.
- The elimination notice read "ELIMINATED" with no name after it.
- **The client took the option.** In the same session its Options screen, Gameplay page,
  showed Kill Feed Display On, Always Skip Kill Cam Off and Tips Display On: the three values
  the server sent. The client saved no option block in that session (no 22203), so the screen
  showed what login gave it. The byte itself was not read from memory, and the screen was not
  compared with a login that sends zeros.

So the option arrives and is still not enough for a bot kill on ORPHEUS. What this run does not
show:

- Whether the bot's fresh pulser went out. `combat.kill_feed_allowed` holds it back until the
  mode script is delivered and the client has both the bot's and the killer's identity.
- Whether the client's own conditions passed. The first one in `0C18` is `STU_CD7F3617()`, the
  body's name. The notice had no name for this bot, so a nameless body is a candidate for
  both symptoms. That is a hypothesis.
- A player's death, which runs through `01CF` instead, was not tried.

## Open

- Where the chain stops for a bot kill on ORPHEUS: the bot's pulser or the client's name test.
  A statescript tracer on the client would tell.
- The meaning of `STU_0D179CAF` and `STU_CEFF290A`.
- A line's critical-hit mark, ability icon and assists (the other `003E.025` parameters) were
  not exercised.
- The tools used for the live reads, a statescript tracer plugin and a read-only memory reader,
  are not in this repository.
