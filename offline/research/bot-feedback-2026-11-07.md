# Bot feedback and crash recovery

Native hitmarkers, named elimination notifications and the initial hover-height correction were verified in the normal server. The subsequent [nameplate and animated-pose follow-up](bot-nameplates-2026-11-07.md) restores overhead widgets and supersedes that hitbox correction.

## Native feedback

Message 25001 carries target EntityID, actual damage, flags (bit 0 marks critical damage), and a frame timestamp. Handler `0x7FF6D18E2D40` increments native hit counters; crosshair widget 0x8A consumes those counters. Timestamp matches the packet clock: `1000 + round((now - match.started) * 1_000_000 / player.quanta)`. Read-only live emulation confirmed the counter increment. Frame captures confirm white body and red critical markers.

Message 25005 carries shooter body, victim body, credited damage percentage and an assist flag. Handler `0x7FF6D18E2B50` emits message 0xF76, consumed by native graph 0x1FC0. Practice mode must start its authored graph 0x13C0 -> 0xCB6 common HUD. A controller with components 26/74/44/75 and a roster name resolves the victim's display name. Controller IDs and participant slots survive bot body replacement. Later human participants receive unused slots rather than overwriting bot names.

`Match._damage_bot` centralizes damage, ultimate charge, health replication and feedback for hitscan, rocket/ability landings, grasp and damaging orbs. Zero damage/dead targets send no feedback. Credit resets on respawn. Limit: only the finisher receives elimination credit; assists are not implemented.

## Hit volume correction

Training model 0x312E's skeleton bind position for bone 2 is Y0.8236272931. Idle clips 0x39FC/0x39FD place it at Y1.3236273527. The entity stays at floor height, so bind-pose volumes were 0.5 m too low. Applying that authored offset fixes the reproduced head miss without widening volumes. The same ray at 8 m now reduces health 200 -> 160. Sway and animated limb poses remain approximated by fixed volumes.

## Crash

The temporary probe sent component 123 (`MECCharacterBody`) to Range body 0x1403. The client rejected its schema and crashed. That body supports no 123; the normal server does not send it. The probe was removed, and a regression guards Range body creation against this component. Regular hero bodies retain 123.

## Verification

Normal launcher session `main-menu/20261107T183111.557843Z` runs all 15 authored Range bots. Shooting stationary bot B0000201 at 35.7 m registers 32-point critical hits; body bursts eliminate it. The native widget reads `ELIMINATED TRAINING BOT 100`. Replacement B0000202 also accepts damage, confirming controller rebinding after respawn. No additional crash occurred.

- [Red hitmarker during normal-server fire](main-menu/bot-feedback-hitmarker.png)
- [Named elimination in the normal Range](main-menu/bot-feedback-eliminated.png)
- 221 lobby unittest cases and 77 focused asset-hitbox/feedback/target pytest cases pass; `git diff --check` passes.

## Initial unresolved investigation (superseded by follow-up)

Overhead name/health widgets remain absent. Controller identity alone does not enable them. Probes of supported component 86, body filter bits, health footer fields, game-mode participant array and authored message 0x724 did not restore them. None of those unsuccessful field changes remain in the normal server. Their meaning is not established by negative results; do not promote them or repeat the unsupported 123 experiment.
