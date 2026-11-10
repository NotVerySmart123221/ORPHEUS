# Bot nameplates and animated idle hit volumes

Build 104319. Implements the follow-up to `bot-feedback-2026-11-07.md`; no game-file changes or process writes.

## Native overhead widget

Range body graphs lead through BE2 -> C18 -> D24. D24 state 35 creates schema `0A88:120` under the game mode's world-items presenter (variable 14206, collection 5912). Its health child is state 51/schema 019C; state 53/schema 128 supplies the simple localized Training Bot name. Retail screen `09A:629` and templates `0BA:3BC/3BD/3BE` render the result.

Two separate replication details prevented it from appearing:

- Full-frame reader `0x7FF6D1C2BE30` skips client-only states. Owner-delta reader `0x7FF6D1C2C620` indexes the unfiltered 77-state table and accepts client state 35. The show delta has no DSR payload. Its variable mask has 32 slots, not the four filtered full-frame slots.
- Public visibility input 8807 belongs to **instance** scope (filtered presence 0); setting entity variable 8807 has no effect on this input. The resulting IsVisible property 27A3 must be true. Simple-name input 14680 is filtered presence 3.

Entity variable **29415 is the authored target-body override**, read by client BooleanSwitch 72 and Stack 71 into instance variable 8154. It must contain the bot body, not its controller. The native Stack owns this binding; a one-time network override can be overwritten by client initialization.

The local server hosts D24 on the stable bot controller, isolated from hero weapon scripts on the body. It supplies the body override plus initialized entity inputs 478=false, 11264=false and 20017=true. The body create must be acknowledged before the graph is sent, and the graph's initial full frame must be acknowledged before one reliable delta starts state 35. Entity records expose their positive delivery acknowledgement for this dependency. Current/max health come directly from body component 51. Death or removal destroys the presenter; respawn reuses the controller's stream with monotonically increasing chunk numbers and a fresh body override.

`Stream.delta_frame` marks data deltas for retransmission and delivery accounting. This differs from acknowledgement-only deltas, which are intentionally disposable. Tests decode the packet independently, cover both create-ack barriers, lost creation packets, death, controller reuse and body generation changes. Inverting the graph-create acknowledgement gate makes the lifecycle regression fail.

## Hitboxes

The former bind-plus-hover table still used straight forward-pointing arms. Native read-only hardpoint evaluation (`0x7FF6D1F41470`) identifies the rendered pose as **3952 frame 0 + additive breathing 3950**. Six independently captured hardpoints match the extracted hierarchy within 3 mm RMS and 5 mm maximum error, including both gun muzzles and shoulders.

`tools/assets/range_bot_hitboxes.py` uses the existing hash-verified asset reader, skeleton and animation parsers. It samples position linearly and rotation with shortest-arc slerp, composes parents and uniform scale, and transforms the model's original bone-local spheres/capsules. It encloses the whole four-second additive loop rather than guessing the client's unsynchronized phase. Sampling at quarter-frame intervals plus 2 mm margin produces <=3.8 cm radius expansion. Independent eighth-frame samples, offset between extraction samples, remain inside the served volumes.

The server table now covers the folded guns and correct torso/head/lower-body positions. It removes the obsolete forward gun capsules. Regression shots hit a low folded arm and miss the old forward ghost volume.

Limit: this is an idle-motion envelope, not a synchronized per-frame skeleton. Hero bot limb animations remain at their previous bind poses; other Range animation states need separate authored poses if served later.

## Verification

Live session `main-menu/20261107T203146.769813Z` uses a stationary 8 m bot fixture with production entity, nameplate and combat paths; only placement/roster are controlled. Body creation and graph creation are separate acknowledged packets. Earlier same-packet initialization reproduced missing first-life widgets, although later respawns worked; the ordered path fixes that case.

First-spawn label is present before any foreground input. A real mouse-aimed shot on the folded lower gun registers `distance=8.29 critical=False health=180` at frame 9079. The label remains visible and its bar decreases. A body burst reaches zero at frame 9517, posts `ELIMINATED TRAINING BOT 100`, and removes the overhead presenter. Replacement body B0000002 returns with health 200 and a full label/bar. No diagnostic activation packets were used in this session.

- [First-life name and full bar](main-menu/bot-nameplate-normal.png)
- [Folded-arm aim](main-menu/bot-limb-aim.png)
- [Damaged bot with label and reduced bar](main-menu/bot-nameplate-damaged.png)
- [Elimination and removed overhead widget](main-menu/bot-nameplate-dead.png)
- [Full-health respawn](main-menu/bot-nameplate-respawn.png)

The initial broad screenshot predicate counted red gun parts and falsely passed absent labels. It was replaced with a camera-projected nameplate-only crop, tested against the failed captures (both return false) and the visible respawn capture (true). All final captures were also inspected manually.

227 lobby unittest cases and 90 focused pytest cases pass. The latter include retail-asset pose calibration and off-grid envelope coverage. `git diff --check` passes. The fixture game closes cleanly with exit 0.

Normal launcher session `main-menu/20261107T203539.270151Z` runs all 15 authored Range bots. A read-only native scan confirms 15 D24 state-35 presenters active and attached to the world HUD, one per stable controller B0000000 through B0000E00. [Visible bot labels in the full roster](main-menu/bot-nameplates-full-roster.png) were inspected manually. The normal session is left running; no fixture overrides or diagnostic packets are present.
