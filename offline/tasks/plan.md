# Proposed next milestones: one local practice session

Status: original Practice Range loads through a loopback game service; Soldier selection, first-person walking/camera, held rifle and repeated teardown work. Native jumping/landing and spawn-wall collision are verified. HUD, firing, damage and bots remain pending; server physics still diverges from client collision. See [entry evidence](../research/practice-range-entry/REPORT.md) and [Soldier evidence](../research/soldier-runtime/REPORT.md).

## Starting point

The retail main menu and normal exit are verified in [the runtime report](../research/main-menu/REPORT.md). The Python service now advertises minimum available content and hands Practice Range to an owned loopback UDP service. Retail request/handoff schemas were independently confirmed from running-client registrations. An audited, pinned MIT game-server subset supplies transport/world/hero selection; the beta project's practice/bot code remains a behavioral reference with build-specific native hooks and layouts.

## Recommended order

1. Trace Training → Practice Range through the game-instance handoff, identifying the smallest route into the original Practice Range.
2. Load that map, then create one locally controlled Soldier: 76.
3. Add one stationary target with measurable damage.
4. Add one attacking bot, death and respawn.

Practice Range is the chosen first target, following the user's direction. Resolve its assets locally before implementation; report missing dependencies rather than silently substituting another map. Do not assume beta map GUIDs or recovered function addresses carry over. The [historical reference](../research/practice-range-reference.md) separates OW1 behavior from later OW2 additions.

## First investigation

Add protocol CRCs to existing index/offset logs and record a bounded Training/Practice Range attempt. Establish whether tournament-menu data needs enabling before the entry can be selected. Recover only the schemas needed by that path. The [corrected protocol notes](https://rentry.co/3bswrgmw) identify 0x5078 as an instance handoff; that is a research lead, not a complete game-connection implementation or a proven local simulation entry point.

Use local CASC metadata to verify one map's resource chain. Inspect the retail client's actual world/spawn path with IDAlib and runtime evidence. Then decide between a small client-side restoration, a local game-instance service, or a combination. The deciding evidence is whether usable native world, player and simulation paths can be called in retail; lobby success does not answer that.

Keep this investigation bounded: deliver a trace, resolved resources and a supported route decision before writing a gameplay server or porting broad beta systems. If evidence is insufficient, record the exact missing observation rather than selecting an architecture by assumption.

## Reuse

Reuse `tools/lobby/`, the already verified CASC-reader technique and beta acceptance scenarios. Reference the beta's `PracticeMode`, `MapSwitch`, `PlayerLife`, `BotBrain`, `Damage` and `ServerNodes` implementations. Reuse their behavior and integration ideas only after locating and validating retail equivalents.

## Success criteria

First visible milestone: enter one map, look around, leave, return to the working menu and repeat without a crash.

First playable milestone: walk, look, jump and fire as one hero; damage a target; then support an enemy bot that can damage the player, with death/respawn and clean teardown.

Every new capability needs observable state/effect checks. A button changing screens, firing animation or draining ability charge alone does not prove the corresponding gameplay effect.

## Risks and later work

The biggest unknown is retained retail simulation support. Protected native code and build-specific layouts make beta hooks unsafe to transplant unchanged. Asset metadata access does not prove every map dependency is present or decryptable.

Keep the current hash guard, owned-process cleanup and bounded desktop tests. Tournament focus restrictions remain; any display workaround must be reversible and tested independently.

Hero Gallery and local cosmetic/progression records could be a smaller UI milestone, but would not resolve the gameplay dependency. Full hero kits, objectives, additional maps and multiplayer follow a proven one-bot loop.

Tasks and checkpoints: [todo.md](todo.md). Estimates are intentionally omitted until the retail world/spawn path is understood.
