# Config-var classes of the 1.74 client (2026-11-06)

`tools/research/config_var_scan.py` over the pinned runtime snapshot (`.scratchpad/runtime-weapon`, taken in
tournament mode at the Range). A config var is a 16-byte STU class whose runtime vtable slot 4 (the evaluator)
starts by asking its context for vtable slot 0xB0; 51 such classes exist, names stripped ("Unknown"). 36 of them
end in a plain global byte (a cvar): forcing one is a vtable copy with slot 4 replaced, as `tools/asi/retail_menu.c`
does for six of them. "default" is the byte in the snapshot, so it already has the tournament presets applied
(2B33EE38 reads 1 = tournament on). The other 15 compute their value (player state, queue state) and are not listed
here; see the script output for them. Only one `tank_*` string survives in the image (`tank_DistributedAgent`), so
the cvars cannot be named from the binary; the way to learn what a switch does is to force it and look.
(2026-11-06: wrong for the runtime dump. Its strings hold the command-line switches `tank_Console`, `tank_TournamentMode*`,
`tank_ShowMode`, ... and 89 named debug cvars; see `dev-console.md`.)

| hash | vtable | evaluator | cvar byte | snapshot value | known as |
|---|---|---|---|---|---|
| `C3C3F350` | `7ff6d3845dc8` | `7ff6d1b3f430` | `0x7ff6d4053678` | 0 |  |
| `98E9109B` | `7ff6d3883cb0` | `7ff6d1f0cec0` | `0x7ff6d4056748` | 1 |  |
| `3E4D41C5` | `7ff6d3882450` | `7ff6d1efb170` | `0x7ff6d4053718` | 0 |  |
| `A80B7FAE` | `7ff6d3845c40` | `7ff6d1b53600` | `0x7ff6d449a6cd` | 0 |  |
| `CAC97777` | `7ff6d38838c0` | `7ff6d1f09d80` | `0x7ff6d4058d78` | 1 |  |
| `56FC3685` | `7ff6d3882e78` | `7ff6d1f03780` | `0x7ff6d40605b0` | 0 |  |
| `F94506DB` | `7ff6d38839a0` | `7ff6d1f0a8b0` | `0x7ff6d404f420` | 0 |  |
| `B4F19440` | `7ff6d3881408` | `7ff6d1eef050` | `0x7ff6d404f420` | 0 |  |
| `39798686` | `7ff6d3881c20` | `7ff6d1ef4100` | `0x7ff6d405a090` | 0 |  |
| `B36E804B` | `7ff6d3845598` | `7ff6d1b3af30` | `0x7ff6d4059138` | 1 |  |
| `2B33EE38` | `7ff6d38840d8` | `7ff6d1f0f750` | `0x7ff6d404f448` | 1 | tournament flag (retail_menu forces 0) |
| `5C045879` | `7ff6d3884880` | `7ff6d1f14430` | `0x7ff6d404f4b0` | 0 |  |
| `C2EE5DED` | `7ff6d3880c28` | `7ff6d1ee9b60` | `0x7ff6d404f528` | 0 |  |
| `CD14BCC8` | `7ff6d384a790` | `7ff6d1b69090` | `0x7ff6d4054360` | 1 |  |
| `61171E59` | `7ff6d3848e50` | `7ff6d1b5a820` | `0x7ff6d4054be0` | 1 |  |
| `CC34C103` | `7ff6d3847900` | `7ff6d1b4ffd0` | `0x7ff6d4053f48` | 0 |  |
| `0B034E2B` | `7ff6d3849f88` | `7ff6d1b64470` | `0x7ff6d4053678` | 0 |  |
| `A7864F38` | `7ff6d3845b98` | `7ff6d1b3e160` | `0x7ff6d404f690` | 0 |  |
| `EB424844` | `7ff6d3846e48` | `7ff6d1b4b100` | `0x7ff6d405dbe0` | 0 |  |
| `58DC96D7` | `7ff6d38480b8` | `7ff6d1b53600` | `0x7ff6d449a6cd` | 0 |  |
| `F493471A` | `7ff6d3848c90` | `7ff6d1b59890` | `0x7ff6d4051738` | 1 |  |
| `F312D164` | `7ff6d38462e8` | `7ff6d1b43890` | `0x7ff6d4055778` | 1 |  |
| `6A1280D1` | `7ff6d38464e0` | `7ff6d1b44fe0` | `0x7ff6d405f8d0` | 0 |  |
| `3F784E6C` | `7ff6d3848438` | `7ff6d1b55650` | `0x7ff6d405deb8` | 1 |  |
| `63DD95AD` | `7ff6d3848d70` | `7ff6d1b5a010` | `0x7ff6d405a0e8` | 1 |  |
| `1B3683C0` | `7ff6d38472a8` | `7ff6d1b4cd30` | `0x7ff6d405a138` | 1 |  |
| `EE69FF10` | `7ff6d3845e00` | `7ff6d1b3f3d0` | `0x7ff6d40526a8` | 0 |  |
| `74BA7BFC` | `7ff6d38450e0` | `7ff6d1b38120` | `0x7ff6d40528b0` | 0 | PLAY card, presenter 604 (forced 1) |
| `6AFF3600` | `7ff6d3846a58` | `7ff6d1b48830` | `0x7ff6d40526d0` | 0 |  |
| `DEA278FD` | `7ff6d384a720` | `7ff6d1b69030` | `0x7ff6d4052680` | 0 | PLAY cards, presenters 339/814 (forced 1) |
| `27FF1349` | `7ff6d384b3d0` | `7ff6d1b6ff20` | `0x7ff6d4052888` | 0 | Practice vs. AI card (forced 1) |
| `06DCE3AD` | `7ff6d3848240` | `7ff6d1b548f0` | `0x7ff6d4053880` | 1 |  |
| `DD66B7EA` | `7ff6d3846a20` | `7ff6d1b48890` | `0x7ff6d40537e0` | 1 |  |
| `AD27479D` | `7ff6d38481d0` | `7ff6d1b54700` | `0x7ff6d449a6cd` | 0 |  |
| `A0339627` | `7ff6d38475f0` | `7ff6d1b4e660` | `0x7ff6d4053858` | 1 |  |
| `888494B6` | `7ff6d384b868` | `7ff6d1b72580` | `0x7ff6d40591d8` | 0 |  |

Computed (no plain byte): `D08A15F9`, `946E87CE`, `D1CBBF22`, `18D3FC1E`, `319757ED`, `40A335F4`, `1198EF21`, `7FE21161`, `61F2815B`, `72339B62`, `5A771239`, `8B4BD0DC`, `BE984B95`, `072FF5EB`, `07AF5658`

Next: force the 1-default ones off and the 0-default ones on in a throwaway `retail_menu` build, one at a time at the
main menu, and note what changes (candidates for debug HUDs, dev menus, or hidden cards). A cvar read by many
functions (`2B33EE38` has 8 code xrefs, most have 1) is a mode switch rather than a card gate.
