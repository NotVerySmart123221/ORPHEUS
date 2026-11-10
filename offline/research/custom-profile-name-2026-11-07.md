# Custom local profile name — 2026-11-07

Custom names are feasible with high confidence from static source and binary analysis. `Player` is supplied by our local server in four places. No current launcher setting selects another name. Changing those sources should cover the menu, Career Profile, party card and match roster; custom-name rendering has not been tested in game.

Research only: no game interaction, live process inspection, sockets, restarts, builds, code/config edits or runtime tests. Binary inspection used read-only idalib queries against the existing `ida/Overwatch-state-analysis.dump.i64`; no patches, renames or database saves.

## Where `Player` originates

1. **Login/menu:** [`login_records`](../tools/lobby/lobby.py#L84) assigns `name = b'Player\0'` at line 86. Record 20500 (`0x5014`) contains it twice; 20502 (`0x5016`) contains it once. The serializer concatenates NUL-terminated names rather than reserving a fixed six-character field. Existing [login research](login-skip.md#L26) records that the menu displays the lobby-supplied name.
2. **Career Profile:** [`PracticeRange.career_profile`](../tools/lobby/practice.py#L233) supplies `'Player'` at line 240 in message 20807, profile field `+0x138`.
3. **Party card:** [`PracticeRange.party`](../tools/lobby/practice.py#L284) supplies `'Player'` at line 290 in message 20700, member-card field `+0x40`.
4. **Match identity:** [`practice.py:447`](../tools/lobby/practice.py#L447) passes `(1, 'Player', SOLDIER, 0, True)` to `create_match`. [`GameServer.create_match`](../tools/lobby/vendor/ow174/game/server.py#L312) forwards the name into `Match.add_player`. [`Match.game_struct`](../tools/lobby/vendor/ow174/game/match.py#L1406) serializes `p.name` into participant field `+0x50` for messages 20300/20301. [`_player_create`](../tools/lobby/vendor/ow174/game/match.py#L1496) binds component 74 to that participant slot.

The login-screen bypass separately defines [`LOGIN_NAME "Player"`](../tools/asi/retail_menu.c#L45) and writes it into the hidden entry form at [line 138](../tools/asi/retail_menu.c#L138). That value passes the entry gate; the server independently supplies the displayed name. A server-side display-name change should therefore require no ASI rebuild. Changing only `LOGIN_NAME` would leave the four server sources unchanged.

Manual entry currently cannot drive these server values: [`lobby.handle`](../tools/lobby/lobby.py#L141) recognizes the connect request's protocol group and offset, then constructs `login_records` without decoding the entered name. The [launcher arguments](../tools/lobby/run.py#L155) and [server arguments](../tools/lobby/lobby.py#L188) contain no name option.

## Length and encoding evidence

The local [JAM encoder](../tools/lobby/vendor/ow174/jam/codec.py#L159) writes string types 12/13 as UTF-8 followed by NUL. Its [decoder](../tools/lobby/vendor/ow174/jam/codec.py#L99) reads through NUL. Neither imposes a character limit. Party, career-profile and game-participant name fields are type 12. Their schema `size: 40` describes in-memory layout, not a proven 40-character wire limit; the Python string writer does not use it for truncation. Login records 20500/20502 use the manual serializer above, outside the vendored schema table.

Static native evidence, image base `0x7FF6D1220000`:

- Name-screen gate **VA `0x7FF6D1CB3880`, RVA `0xA93880`** reads UX name variable `0xD800000000001D0` at `0x7FF6D1CB38BF`. At `0x7FF6D1CB391C` it rejects lengths above 12 or below the mode-dependent minimum: 1 with TournamentMode enabled, otherwise 3.
- The count helper **`0x7FF6D162D920` → `0x7FF6D162ACE0`** parses UTF-8 and counts decoded characters for valid input, rather than simply counting bytes. This does not establish Unicode font coverage or every UI's behavior.
- The ASCII letters-only loop at `0x7FF6D1CB3950`–`0x7FF6D1CB3962` runs when ShowMode (`0x7FF6D404F420`) is enabled. With TournamentMode (`0x7FF6D404F448`) enabled and ShowMode disabled, that loop is skipped. Do not generalize the older note into a supported non-tournament login route.
- **`0x7FF6D1858320`** checks configured patterns and returns success/failure. It is a validator, not a demonstrated name-storage setter. This refines the description in [login-skip.md](login-skip.md#L18).
- Event handler **`0x7FF6D1CB3FC0`** calls the gate and closes the screen on success at `0x7FF6D1CB4026`.

The 1–12 limit belongs to the native entry form, not a proven limit on server-supplied profile strings. A conservative initial implementation policy would allow 1–12 ASCII letters/digits and reject NUL/control characters. Longer names, Unicode, punctuation and layout limits need separate rendering checks. The generic encoder's removal of embedded NUL is not a substitute for validating a future name setting, especially because login records are assembled manually.

## Smallest future change

Use one validated server-side name across the four sources above, keeping the synthetic account IDs unchanged. A shared constant would support a fixed custom name; a configurable name would additionally need a launcher/server input. Neither has been implemented here.

The existing match roster already accepts a supplied name. Human overhead labels and killfeed rendering with a custom name remain unverified; no chat-name path was established. Bot labels follow their own [nameplate presenter](bot-nameplates-2026-11-07.md#L7) and are outside account renaming.

External corroboration: the [current upstream project's README](https://github.com/Boi-027/overwatch-1.74-lobby-server#how-to-start), checked 2026-11-07, documents entering a name when joining a server and reusing it next time. Our local option list above determines what this checkout supports today.

Graphify supplied orientation using existing vocabulary `[account profile login name lobby tournament]`: relevant nodes included `login_records()` (`tools/lobby/lobby.py`, `source_location: L84`) and `.career_profile()`. Claims above were checked against current source and native decompilation; graph locations can become stale.

Skipped implementation and live verification. Mechanism is supported by static evidence; exact custom-name appearance across native UIs remains unverified.
