# Range bot death and reassembly

Build 104319. Restored 2026-11-08.

## What retail does

1. **Death.** The client plays it by itself when a body it still has reaches 0 health (max health set, current 0): the Training Bot bursts and its parts fall apart on the floor (ragdoll; the model component carries `m_ragdollConfig`, and the model has kind-4 ragdoll bodies). The same was found on the beta client (`<beta-project>\repo\CHANGES.md`, "Death and kill feedback"): deleting the entity at once skips it.
2. **Reassembly.** The bot's body graph `0x0580000000000BA8` has two StateAnim states, owner states 0 and 1, remote slots 0 and 1, both playing alias `0x0C80000000000451`. The bot's blend-tree set `0x04000000000005A8` maps that alias (parameter instance 145) to a random choice node over set-local aliases `…805A8019`/`…805A801A`, which name clips `0x0A000000000039D8` and `0x0A000000000039D7`. Both clips last 2.667 s at 60 fps; their first frame puts parts up to 33 m away and their last frame is the idle pose. In the graph the Anims sit behind a game-message entry and a branch on entity var 7143 (slot 0 when true, with an anim category; slot 1 when false, without one).

`anim_export.entity_aliases` misses this binding because the alias resolves through a choice node, not one clip.

## What the server does now

- `TrainingTarget.tick` keeps the dead body (no destroy 0.2 s after the death; the `removed` flag is gone).
- 3 s after the death a Range bot is revived as the same body where it fell: `Match._revive_bot` sends component 51 at full health and restarts its overhead presenter. A hero bot still comes back as a new body at home, so its old body shows the death for those 3 s.
- `Match._send_bot_rebuilds` sends the revived body a remote frame of graph `0BA8` (`target.REBUILD`, `Graph(0x0BA8, 17, 4, 11)`) with remote slot 1 active (`soldier.anim_payload()`), and a frame with no active state `REBUILD_SECONDS` (2.667 s) later, so the next revival starts it again. Health restored alone only pops the bot back whole.

## Evidence

Live, 2026-11-08: a scratch fixture put one Training Bot 5 m in front of the Range spawn and killed it server-side every 9 s; frames were grabbed in the background at about 20 fps.

- Death: burst at +0.03 s, parts on the floor until +2.95 s, ELIMINATED notice.
- Revival at +3.0 s: the ragdoll parts go, the torso drops in from above (+3.10 s), the arms fly in from the sides (+3.3 to +3.7 s), and the bot settles into its idle pose with its jets by about +4.7 s. The nameplate returns.
- Slots 0 and 1 looked the same on four kills; slot 1 is used.

![Death and reassembly](bot-death/death-and-rebuild.png)

## Limits

- shortcut: the 3 s respawn time is kept from before, not read from retail data.
- shortcut: a revived bot can be hit while it reassembles; whether retail protects it then is not known.
- The death of hero bots (vs AI, the viewer mannequin) relies on the same native path but was not checked live.
- What var 7143 and the game message in front of the Anims mean is not decoded; the server starts the Anim state directly.
