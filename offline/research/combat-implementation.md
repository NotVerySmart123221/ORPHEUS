# Local hitscan geometry findings

## Implemented slice

`tools/lobby/combat.py` supplies only command-angle conversion and ray/sphere intersection. It can answer whether a ray hits a supplied sphere and how far along the ray the first intersection lies. It does not apply damage, infer a muzzle point, simulate weapon timing, account for map walls, or reconcile server movement with the client.

`tools/lobby/test_combat.py` covers signed-angle axes, normalized directions, hit/miss/tangent/range, inside-origin behavior, and invalid or unrepresentable geometry. Focused run: `python -m unittest -v test_combat` (5 tests pass).

## Evidence and limits

- `tools/lobby/vendor/ow174/game/commands.py:14-18` defines signed 16-bit yaw/pitch units (`2π/65536`), forward `(sin yaw, 0, cos yaw)`, and positive pitch looking down. `combat.aim_direction` extends those documented axes to pitch with `(sin(yaw) cos(pitch), -sin(pitch), cos(yaw) cos(pitch))`.
- `tools/lobby/vendor/ow174/game/match.py:128-139` applies input commands to the server mover; the client command record contains no position. `tools/lobby/vendor/ow174/game/mover.py:1-6,27-43` says the server mover omits collision, gravity, jumps, and per-hero speeds. Runtime confirmed W movement, but jump flight remains unverified.
- `tools/lobby/vendor/ow174/game/match.py:250-261` creates a body from the selected hero body definition, health, team, hero GUID, and server mover transform. `tools/lobby/vendor/ow174/game/world.py:146-149` encodes component 51 health; `world.py:226-260` creates an entity with chosen definition, transform, movement, and components.
- `tools/lobby/vendor/data/game_heroes_174.json:87-91` maps Soldier: 76 GUID `0x02E000000000006E` to generic body definition `0x04000000000003CF`. `tools/lobby/vendor/ow174/game/content.py:86-90` identifies Practice Range world `0x0800000000000688` and a spawn at approximately `(54.56507, 1.0, 42.14788)`.
- The checked local catalogs expose the map and hero/body records, but no original Practice Range training-bot prefab or bot definition. The custom bot code under `<beta-project>\repo\Prometheus-Core\Fixes\PracticeMode.cpp` is separate beta-era PvP practice behavior and does not establish an authentic retail range bot. Using the Soldier body definition as a target would be a generic-hero placeholder, not a verified practice bot.
- Weapon graph research in `research/soldier76-weapon-graph.md:7-12` identifies a Soldier weapon graph with one projectile, 9 shots/s, and a base health modifier initialized to 20. Its damage expression includes additional graph/config terms; no verified range-falloff formula or muzzle origin is established here.

## Integration boundary

Server-side sphere tests are useful only after a caller supplies a verified target center/radius and an honest shot origin. The current flat mover has no map collision and can diverge from the client's collision-aware position, so it cannot yet support claims of authentic wall blocking or reliable client-aligned hits. Do not decrement health merely because FIRE was pressed; require an actual ray/sphere hit. A single stationary target and a clear shot lane can be an explicitly limited prototype once a real target definition and placement are verified.
