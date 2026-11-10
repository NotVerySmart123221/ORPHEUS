# Hero Info clips (Hero Gallery reference)

Each hero's Hero Gallery > HERO INFO page plays one short clip per ability (STULoadout `m_infoMovie`, type 0x0B6). These are the reference for how a hero should behave and animate.

Extract them from the install (no game running needed):

```
python tools/assets/hero_movies.py research/hero-info "Soldier: 76"
```

- Output: `research/hero-info/<Hero>/<n>_<loadout>_<movie>.mp4` plus `index.json`. MP4s are git-ignored; rerun to regenerate.
- The movies are Bink 2 (`KB2j`), which ffmpeg cannot decode. The tool decodes them with the game's own `_retail_/bink2w64.dll` (loaded read-only into the Python process) and pipes frames to ffmpeg.
- `n` follows STUHero's loadout list, not the Hero Info page order. Map each clip by its loadout GUID (research/abilities.md has Soldier's).

Caveat: the clips are launch-era (2016) footage with the old HUD. Use them for behaviour, timing and animation. Do not take numbers from them: Soldier's clip shows a 25-round magazine, while 1.74 data has 30.

Per-hero notes:
- [Soldier: 76](soldier76.md)
