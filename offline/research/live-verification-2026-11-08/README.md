# Mercy heal beam on unarmed allies — 2026-11-08

Retail 1.74 client, local lobby, `retail_menu.asi` only, `OW_VIEWER_FILE` mannequin as an ally.

- `mercy-heal-reinhardt-sheet.png`: ally Reinhardt (no `WEAPON_GRAPHS` entry), 2.5 s primary hold after the fix. Server logged `staff heal target=B0004001`. The beam reaches Reinhardt and the staff head opens; the beam's near end comes from off-screen left (world origin), not the staff. Before the fix (research/live-verification-2026-11-07/mercy-staff-mismatch.png) no beam drew at all.
- `mercy-heal-soldier.png`: same session, mannequin swapped to an ally Soldier: 76. Beam runs from the staff tip to him, head open.

Repeated in two further sessions with identical results. Sending Reinhardt an empty remote frame from the start (an established stream) did not move the near end, so stream history is ruled out. Next: tracer.asi's STU_9932D728 evaluator probe (research/asi-tracer.md) on both allies to compare var27 / var49 resolution.
