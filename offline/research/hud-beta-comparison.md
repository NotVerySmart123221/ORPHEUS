# Beta GameMode/UI comparison

## Finding

Prometheus 0.8 has an explicit local-game-mode lifecycle. `player_spawner.cpp:267` calls `GameMode::OnLocalPlayerSpawned()` after creating/modifying the local model entity. `Fixes/GameMode.cpp:118-124` posts `Start()` to the main loop; `Start()` loads and spawns entity `0x0400000000000013`, then schedules separate `PlayersReady` and `Setup` messages (`GameMode.cpp:16-19, 82-105`). This gives a concrete beta event sequence to compare against retail's missing GameMode-scoped HUD parent.

Practice entry is a separate path. `Fixes/PracticeMode.cpp:24-27, 90-120` intercepts lobby game-type 3 and requests a local map/bot match; `Fixes/MainMenu.cpp:66-74` routes Play to `StartMatch`. The inspected beta code does not assign a retail-equivalent HUD ScriptRef or publish a `23604`-like variable. Do not transplant beta entity or message IDs.

## Assessment

- **Source evidence:** beta creates its GameMode entity only after local-player spawn, and advances its mode through game-message phase events.
- **Inference:** retail's absent GameMode UI parent could be downstream of missing mode-entity ownership or phase/event initialization. Beta source does not establish this as the retail cause or identify the retail producer for variable `23604`.
- **Graphify:** queried the existing `<beta-project>\graphify-out\graph.json` through its documented NetworkX fallback. The relevant extracted path is `OnLocalPlayerSpawned() -> Start() -> GameEntity()/SetPhase()`; it did not expose an additional UI-root assignment.

## Sources

- `<beta-project>\repo\Prometheus-Core\player_spawner.cpp:264-268`
- `<beta-project>\repo\Prometheus-Core\Fixes\GameMode.cpp:16-19, 63-105, 118-124`
- `<beta-project>\repo\Prometheus-Core\Fixes\PracticeMode.cpp:24-27, 90-120`
- `<beta-project>\repo\Prometheus-Core\Fixes\MainMenu.cpp:66-74`
