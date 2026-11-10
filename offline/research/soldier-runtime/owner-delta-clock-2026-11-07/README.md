# Owner data delta clock control

Build 104319, isolated server session `main-menu/20261108T010024.657069Z`. Fresh Soldier bodies; identical two bursts produce 22 server shots and ammo 8. Crops come from desktop-duplication captures. No client process writes.

- `c0-delta-stale-ammo.jpg`: Sprint-preserving data deltas with C0; HUD bounces and ends at 12.
- `full-frame-correct-ammo.jpg`: original rifle, full owner frames for every update; HUD ends at 8.
- `c1-delta-correct-ammo.jpg`: original rifle and unchanged delta body; only C changes to 1. HUD counts down and ends at 8, also confirmed by a later still.

Native `1C2EE10` applies CmFD to the state clock through `2DAE650` only for C1. Copied-native `1C2CA20` independently validated the ammo mask: 128 raw slots, selected slot 53, 24 mask bits, variant kind 4. No extra presence bit belongs there. Production correction sets C1 on authoritative owner data deltas; acknowledgement-only frames remain C0. Primary gun motion needs separate acceptance.
