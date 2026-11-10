# Live verification — 2026-11-07

Retail build 1.74.0.0.104319; local lobby only. Session: `research/main-menu/20261107T162846.965666Z/`. Only `retail_menu.asi` loaded, with explicit user authorization. Launcher has no timeout. Held-input tests used the approved existing `gameinput.py` idle wait and cancellable countdown.

[Decisive gameplay records](events.log) preserve the timings below without the full login/account traffic.

## Passed

1. **Desktop/runtime recovery.** Normal sandboxed commands and desktop input work after the ownership repair described in [desktop-control recovery](../desktop-control-recovery-2026-11-07.md). All 3,283 Git metadata files were byte-verified. Sandbox deny ACEs were re-applied; setup reports `errors=[]`.
2. **Alt-Tab.** ASI loaded; its log at 12:28:50 reports the keyboard hook removed. A guarded test asserted the game initially held foreground focus, sent Alt+Tab, and asserted foreground changed away from the game. Prior desktop focus was then restored. No timeout was added.
3. **Matrix cannon suppression and depletion.** Primary fired serials 1–3 before activation. With both triggers physically held, Matrix began at 12:34:19.219, command frame 42547. No cannon shot occurred during its two-second active interval. On depletion, frame 42833 at 12:34:21.219 fired serial 4; serials 5–10 followed. Native green Matrix frame is visible in [active capture](matrix-active.png) and clears afterward in [released/depleted capture](matrix-cleared.png).
4. **Matrix protection from actual Range-bot fire.** Character was stationary at approximately (53.30,-2.00,-5.67), facing the shooters. The first protected window was 12:39:50.482–12:39:52.480; health stayed 580. Adjacent hits were at 12:39:50.417 and 12:39:54.170: a 3.753 s gap. After full resource recovery, the second window was 12:40:00.545–12:40:02.548; health stayed 500. Adjacent hits were at 12:40:00.141 and 12:40:03.823: a 3.682 s gap. Both exceed the bots' 2.5 s maximum firing interval, while unprotected hits continued for 20 damage each. [First protected](range-protected-first.png), [second protected](range-protected-second.png), [unprotected](range-unprotected.png). An assert-based log check verifies no damage inside either window. This establishes protection at this tested position/aim, not exact native-volume parity everywhere.
5. **Mercy staff stow at the server input boundary.** Primary remained physically held for 2 s. Weapon 2 was pressed after 0.75 s. At 12:45:58.464 the server cleared `staff_target` and fired the pistol's first shot with 19 rounds; further pistol shots continued until primary released at 12:45:59.768. Staff targeting did not persist until trigger release. This validates real weapon-key delivery and cleanup, independently of the rendering problem below.
6. **Moira release, normal hero-switch flow and reacquisition.** [Native grasp](moira-grasp.png) ran against the enemy mannequin. During a held-grasp switch test, the server locked `B0004101` at 12:49:23.390, opened hero selection at 12:49:24.424 and cleared grasp at 12:49:24.432. Soldier replaced the body at 12:49:25.907 with [no lingering beam](soldier-after-moira.png). After returning to Moira, the same still-living entity `B0004101` was locked again at 12:50:54.856, then released at 12:50:57.357; [reacquired beam](moira-reacquired.png). Opening hero selection itself released grasp in this client flow, so this is not separate proof of a still-locked body being replaced; the unit regression covers that seam.

## Presentation issue reproduced

Mercy still appears to hold the pistol while server staff targeting is active, and the target-length staff beam is not visible in this run's capture. [Capture during held heal](mercy-staff-mismatch.png). Server logs confirm the ally lock from 12:44:09.924 to 12:44:11.924.

Read-only native inspection found active graph 006B, active CosmeticEntity model state 0 and active DataFlow parents 74/77; the cosmetic output at +0x1E0 was nonzero. No confirmed 006D instance explained the apparent pistol at that point. Thus graph startup and physical weapon-key input are established; the resolved cosmetic model/rendered weapon and beam attachment still need investigation. No speculative model or protocol fix was enabled.

## Limits and next check

- 309 automated tests pass after the verification-tool path fixes. Existing physics and damage regressions remain green.
- Matrix's server geometry still assumes scale 1 and an eye/aim coordinate frame. Tests prove the observed scenario, not full native transform/calibration.
- Death-specific beam teardown, leaving while beaming, complete Soldier presentation and the user's Range acceptance remain open. Soldier Helix/field/voice evidence is in [the static investigation](../soldier-runtime/presentation-next-steps-2026-11-07.md).
- Windows Graphics Capture still times out on this game. PrintWindow capture works; this did not require loading another native plugin.
- Nine representative images are versioned here. All 397 raw frames remain locally under the existing ignored `research/anim/live-verification-2026-11-07/` convention. Original run logs remain under `research/main-menu/20261107T162846.965666Z/`.
- Temporary Matrix toggle was restored before this session. Game remains open without an automatic close timer.
