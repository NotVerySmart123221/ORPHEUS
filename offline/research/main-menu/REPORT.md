# Retail main menu reached

Date: 2026-10-31. Build: `1.74.0.0.104319` at `<game-root>`. Local server: `tools/lobby/lobby.py` on `127.0.0.1:3724`. No game binary patch or injection was used.

![Verified main menu](main-menu.png)

## Reproduction

Run `python tools/lobby/run.py '<game-root>'` from the workspace in a normal desktop terminal. The launcher checks the executable hash, starts an owned local server and launches `_retail_/Overwatch.exe` with `--tank_TournamentMode --lobbyServer=127.0.0.1:3724` and a workspace log path.

On startup, set **Pod 1** and click **Start**. The red config-file warning remains but does not block this explicit-address route. Enter a synthetic local name, click **Enter Game**, and the main menu appears. Screenshot evidence is `main-menu.png` and `tournament-probe/connected.png`.

## Observed protocol and UI

`lobby.log` records the successful tournament connection beginning at local time 17:06:30:

- Client proof verified; server sent its 292-byte type-13 channel block.
- Client announced 86 groups in a 350-byte frame. Dynamic OUT_CONNECT index was 25; IN_CONNECT was 83.
- Client sent group 25 offset 0, 673 bytes. Server supplied records 0x5014, 0x5016, 0x5018 and 0x5019. The payloads match the published example vectors byte for byte.
- Client continued with offset 9 and later groups 29/80, alongside five-second Base keepalives. The menu was visible while the connection remained active for over a minute.
- Menu **Exit Game**, then **Yes**, closed the process normally with exit code **0** at about 17:07:53.

The startup screen and local name form are real UI gates; launching the process alone does not initiate the connection. Pod 0 fails validation. The missing-config warning was initially a suspected blocker; the valid Pod 1 + explicit-address run disproved that hypothesis. No `showmodeLobbyLocations.json` was created.

ShowMode was separately tested with Pod 1 and Group 1. It completed the same handshake and remained connected with keepalives, but stayed at Entering Game. No claim is made about the cause of that mode's extra requirements. The successful menu used TournamentMode with the same server and login records; no packet-order workaround was needed.

## Verification and limits

- Seven Python tests pass. The cipher passes the published independent known-answer vector; a socket integration test exercises fragmented framing, directional cipher continuity and login/keepalive exchange.
- Inverting the launcher's hash comparison made its unsupported-client test fail; restoring it returned the suite to green.
- The completed launcher was run with a ten-second timeout. Its owned game and server processes exited, and a fresh bind verified that port 3724 was released.
- An independent agent reviewed launcher ownership, readiness, validation and cleanup without finding an actionable defect.
- Original executable SHA-256 still matches `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`.
- A temporary WindowMode setting was tested and restored byte for byte. It did not make these special modes windowed. The reusable launcher never edits that settings file.
- Native window capture timed out, so screenshots used the existing project's read-only GDI client-area helper. Input used the desktop automation API against the selected game window.

Tournament mode's Alt-Tab restriction remains. The launcher defaults to an automatic close after 180 seconds and cleans up only its owned processes. No bots, match simulation, progression, purchases or loot-box opening were tested or implemented. The main menu is a verified base for further restoration, not evidence of gameplay support.

## Sources

[Corrected channel/login notes](https://rentry.co/3bswrgmw) supplied the type-13 trailer, announcement-driven indices and login layouts. [Public lobby research](https://github.com/Boi-027/Overwatch-1-v1.74-Lobby-Research) supplied the cipher and tournament handshake foundation. Runtime observations and local artifacts above are independent evidence for this installation.
