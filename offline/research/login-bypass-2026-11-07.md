# Bypassing Pod and ENTER NAME (2026-11-07)

**Implemented and verified in a fresh launch:** `retail_menu.asi` now selects the native startup graph's route around Pod and ENTER NAME. The inspected startup instance had neither screen state instantiated; the normal handshake, menu, Training and Practice Range entry worked.

Scope: Overwatch `1.74.0.0.104319`, image base `0x7FF6D1220000`. Static evidence comes from read-only IDALIB analysis of [Overwatch-state-analysis.dump.i64](../ida/Overwatch-state-analysis.dump.i64), existing-dump disassembly and hash-verified installed assets. Runtime evidence below followed implementation; game files and the analysis database were not patched.

## Recovered owning graph

Graph `0x058000000000041A`, SubScript instance 16/state 1, owns startup graph `0x0580000000000194` through graph-reference instance 84. Graph `0194` is a loose 11,672-byte asset, CKey `c0a7f1be38596f97508746f9c72cad76`. Its selector reads local dynamic variable `1693`: value 0 activates Pod (instance 32/state 2, class `236136A0`); value 1 activates Name (instance 51/state 1, class `FE50BBFC`).

Entry 122 resets the selector, then BooleanSwitch 130 reads entity dynamic `2275`; true plus TournamentMode selects value 2, avoiding both screen branches. This is an existing graph route, rather than a direct main-menu jump. The original meaning of dynamic `2275` remains unnamed.

Name success normally reaches action `DDADAD14`, instance 70, with `m_name` sourced from dynamic `464` (`0x1D0`), then posts message `0x0240000000000115`. The [generated schema](../OWLib-overwatch-1.74/TankLib/STU/Types/Misc.cs#L47808) confirms its name field. Message `0x024000000000009B` also selects `1693=2`, but changes entity variables in graphs `0147` and `02BA`; the implementation avoids broadcasting it.

Reproduce graph contents with [stu_graph.py](../tools/assets/stu_graph.py), [graph_edges.py](../tools/assets/graph_edges.py) and [graph_layout.py](../tools/assets/graph_layout.py), passing GUID `0x0580000000000194` or graph index `0194` as each helper expects. Asset scans read 1,586 of 1,587 root graphs.

## Current implementation

[`login_value`](../tools/asi/retail_menu.c#L86) calls the original dynamic evaluator first and acts only for identifier `2275`, nonzero entity scope, graph `0194` and TournamentMode. With an explicit nonempty `lobbyServer` and idle native lobby, it sets the address and calls normal login with argument `Player`, once for that graph instance. The guard is set before the native calls to prevent duplicate starts during reentry. Only native connection states 3–7 force the result true; other states retain the original result.

The launcher now [injects immediately](../tools/lobby/run.py#L125). The [ASI worker](../tools/asi/retail_menu.c#L148) waits for expected unpacked vtable entries, then repeats pointer scans with a 10 ms sleep until requesting login. `retail_menu.go` remains a separate, **10 s post-login** gate for the six retail-menu overrides ([run.py:24](../tools/lobby/run.py#L24)). The previous [30-frame screen automation](login-skip.md) was removed.

The ASI retains the true tournament value during login ([retail_menu.c:174](../tools/asi/retail_menu.c#L174)). An early tournament override selects the unsupported Battle.net route. The local server still requires its normal tournament handshake, protocol announcement/acknowledgement and connect request before sending account/content records ([lobby.py:101](../tools/lobby/lobby.py#L101), [lobby.py:131](../tools/lobby/lobby.py#L131)). It independently supplies displayed `Player` in login records ([lobby.py:84](../tools/lobby/lobby.py#L84)).

## Native APIs and historical screen behavior

The implementation uses `0x7FF6D2058610` to copy the configured address into `lobby+48`, then normal start API `0x7FF6D2058770(lobby, "Player")`. That API sets `lobby+732` and enters direct-address connection through `0x7FF6D2058BD0`; this release does not consume its name argument. These calls run from graph evaluation, not the ASI worker. Command-line helpers `0x7FF6D2A86670` / `0x7FF6D2A59760` supply the same explicit address used by the Pod gate. Their RVAs are computed as VA minus image base, with self-check assertions; the first implementation test crashed because two manually converted RVAs were wrong.

The old Pod gate `0x7FF6D1CB3B40` and Name gate `0x7FF6D1CB3880` led through common completion `0x7FF6D2D911D0` / `0x7FF6D2D91160`, which queues state-script work and sets flag bit `4` (`*byte40 |= 4`). The new route avoids creating those screen states, so it does not fake their completion. Name helper `0x7FF6D1858320` is a validator, not the name-storage setter. The old Name gate's owner-vfunc call at `0x7FF6D1CB3A98` remains unnamed; the bypass uses the normal native login API instead.

## Verification and limits

Fresh launch PID `26476`, [run directory](main-menu/20261108T032723.296331Z/): the [lobby log](main-menu/20261108T032723.296331Z/lobby.log#L7) records the native handshake, then [account/content records](main-menu/20261108T032723.296331Z/lobby.log#L98). Captures show the [main menu](main-menu/20261108T032723.296331Z/menu.png), [Training](main-menu/20261108T032723.296331Z/training.png), [Practice Range hero selection](main-menu/20261108T032723.296331Z/range-hero-select.png) and [return to menu](main-menu/20261108T032723.296331Z/return-menu.png). Map load completed with [21602](main-menu/20261108T032723.296331Z/lobby.log#L183). The [startup-state inspection](main-menu/20261108T032723.296331Z/startup-states.txt) records graph instance `0x1A7F1CFB510` with Name/state 1 and Pod/state 2 slots both null and no startup input sent.

The [MSVC native-boundary self-check](../tools/asi/test_retail_menu.c) passes, including reentry suppression and original-result fallback at failure state 2. An inverted TournamentMode guard in a disposable source copy fails the first assertion as expected. All **251 lobby unit tests** pass. These checks do not alone establish native failure/retry UI behavior.

Return from Practice Range through Training to the main menu passed; normal exit returned code 0. An unavailable-lobby test against port 1 stayed alive but showed a black frame after 22 s, so native failure/retry UI recovery is not verified and is under investigation. The override attempts login for one graph instance per process and leaves unrelated dynamic evaluations unchanged. Early installation depends on unpacking and worker scheduling; evidence covers the tested build and launch, not every hardware/plugin ordering.

Graphify supplied source orientation from existing vocabulary `[retail menu pod login enter name profile account screen]`. Some protected native functions decompile incompletely. No direct readiness-flag writes or game-file patches were introduced.
