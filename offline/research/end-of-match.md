# End-of-match banner (2026-11-03)

Goal: show VICTORY! / DEFEAT at match end, before the server sends the player back to the menu.

## The banner graph

- `0x058000000000027F` (591 instances) is the only graph that uses the end strings: VICTORY! (`0x0DE0000000000DC1`, instance 468), `0x0DC2` (465), DEFEAT (`0x0DC3`, 484), DRAW (`0x4AC6`, 266).
- Branch node 452 (`STU_3387AB5D`, client-only) reads v7753: true sets UX var 2464 (title) to VICTORY! via SetVar 456/460; false sets DEFEAT (472/476).
- v7753 := `v7525 == STU_65ABFCCC(target STU_8985E00A)` (SetVar 448, expression 487, ops `08 00 0E 00 25 00`): the winning team compared with the local player's team (inference from the strings).
- Its inputs (`STU_EF930FD7` entries 507, 574, 576): v7525, v2328, v13784. Entries: 510 (`1E50E1A8`, to state 2) and 522 (`BB4AC733`, to state 9).

## Who starts it

- Not in client code (no `0x058000000000027F` bytes or immediates in the IDA dump).
- Not referenced by any entity (`0x003`, 3509), game mode (`0x0C5`, 25), map header (`0x09F`, 130) or `0x0C6` (63) asset (`.scratchpad/pva/refscan.py`).
- Not referenced by any statescript graph either (`0x01B`, all 1,586 scanned, `.scratchpad/refscan-27f.txt`). So nothing in the data starts it: the remote-instance route below is the one left.

## Next

- If a graph starts it as a SubScript: send that state with the inputs, like the round-complete screen (`ROUND_COMPLETE`, `research/assault-mode-graph.md`).
- Otherwise try it as a remote instance on the mode entity with v7525 set, after the round-complete screen.

## Remote-instance probe (2026-11-03)

- Flow in the catalog: entry `STU_BB4AC733` (params 2328, 13784 from `0x0A4` assets) begins st9 `STU_B9898052` (remote slot 0); its plug `B6A27F9A` runs node 46 (client-only vars 11008, 4912), then st10 and st0 Wait. Wait's end tests entity var 12421 against the local team (node 18): true sets v7753 (VICTORY!), false goes on to a v7525 == -1 check.
- Sent `Graph(0x027F, 2, 2, 2)` as instance 4 on the mode entity with remote slot 0 on (no payload), during the round-complete phase; once also with entity var 12421 = the player's team and both sync vars = 1 (`.scratchpad/ban1`, `ban2`). No banner, no error, no crash.
- Likely missing: st9's DSR payload (`STU_B9898052` is not decoded) or the entry with its parameters. Next: find `STU_B9898052`'s DSR in IDA.


## `STU_B9898052` payload (2026-11-03, headless idat)

- DSR `0x7FF6D1C1D760` (decompiled with `idat.exe -A -S`, script `.scratchpad/idadecomp.py`): reads **1 flag bit, then 8 bits** (a counter, stored at `state+588`). Not in the Begin-bit classifier `0x7FF6D217E960`, so no extra Begin bit.
  - First update (`state+584 == -1`): if the flag (or byte `+240` of the instance's owner object) is set, `+584 = counter - 1` and it calls `0x7FF6D2D92DC0(state, 1)` (fires the state); else `+584 = counter`, no fire.
  - Later updates always fire. Then `0x7FF6D1C268C0` drops queued entries older than the counter.
- Graph 027F's owner states are st9 (`B9898052`, remote slot 0) and st15 (`STU_9D7BF987`, slot 1); everything else is client-only.
- Probe (`.scratchpad/banner_patch.py`, reverted): instance 4 `Graph(0x027F, 2, 2, 2)` on the mode entity during round complete, slot 0 with flag 1 + counter 1; once also with presence bits 0/1 (v2328, v13784) = 1 and entity vars 12421 = the player's team, 8078/7887/9435/13049 set. No banner, no error, no crash.
- Next: slot 1 (`STU_9D7BF987`, payload not decoded); or the banner sits under the full-screen round-complete UI: try it after 0C35 closes.

## More banner probes (2026-11-03, later)

- 027F slot 1 is `STU_9D7BF987` (the base-state class with node-level remote-sync vars, see `assault-mode-graph.md`); 027F's sync vars 2328, 13784 are its instance presence.
- Sent in every phase (attack and round complete): instance 4 `Graph(0x027F, 2, 2, 2)` on the mode entity, slot 0 with flag 1 + counter 1, slot 1 bare, presence bits 0/1 = 1, entity var 12421 = 1. No banner, no error.
- Left: whether a client creates 027F at all as a mode-entity remote instance (a live memory read of the mode entity's script list would tell), or which entity owns it.

## Retail mode-entity probe (2026-11-04)

- Hanamura Assault timeout in session `20261104T174548.793180Z`. The diagnostic launcher appended `027F` to the result frame with slot 0, flag 1, counter 1 and both instance presence values set to 1.
- The read-only instance scan found graph `027F`, 16 states, with only state 2 active at the sampling point. State 9 was inactive in that sample. The scan did not return `0C35`. The probe showed no defeat banner and returned to Play after the diagnostic result interval.
- The graph descriptor reaches the client. Trace the path to state 9 and sample across the transition to distinguish a missed pulse from an activation that ends before the scan.
