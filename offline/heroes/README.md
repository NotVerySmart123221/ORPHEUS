# Hero fix logs

One file per hero: the issues reported for it, and what each fix changed, how it was checked, and what is still open.
Work goes hero by hero; the session is cleared between heroes, so each file must stand on its own.

| Hero | File | State |
|---|---|---|
| D.Va | [dva.md](dva.md) | in progress |

## Format of a hero file

1. **Issues**: the reported list, one checkbox each (`[x]` fixed and checked, `[~]` fixed but not checked live, `[ ]` open).
2. **Fixes**: one section per issue: cause, change (files), proof (tests, live check), limits.
3. **Open**: what is left, with leads.

## Recipes shared by every hero

- **Spawn voice line.** `python tools/assets/voice_lines.py <body entity GUID> --label HrSlct` gives the hero's
  `HrSlct-EntrMtch` key (stimulus `0x2B`); `--stimulus 0x2C` lists its `Rspwn` keys. Add the pair to
  `soldier.SPAWN_VOICES`. The lobby sends ClientInVoice 27500 once the body's data is acknowledged.
- **Reference clips.** `python tools/assets/hero_movies.py research/hero-info "<Hero>"` (Hero Gallery > Hero Info).
- **Live check.** `python tools/lobby/run.py <game-root> --asi tools/asi/build/ow_fixes.asi`, then
  TRAINING > Practice Range > hero > SELECT. `tools/research/weapon_sweep.py` drives fire/ability presses.
- Graph notes and older research per hero live under `research/` (D.Va: `research/dva-mech.md`).
