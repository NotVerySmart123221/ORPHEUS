# Elimination packet boundary and reload regression

User confirmed controller manual reload and empty-magazine suppression worked, then reported shots losing feedback while holding fire through reload and old behavior after Practice Range re-entry.

## Root cause

The failing normal session `main-menu/20261108T020951.936100Z` re-entered match7BB2. Soldier's native replication manager stopped at chunk21/CmFD2012. All5,983 later queued chunks began at23 or later; none covered missing22. The first bot elimination coincided with shot8/frame2013. Server hits continued while client ammo/input/reload updates could no longer apply.

A controlled server-side elimination reproduced the same fault without any reload or trigger input: native client33812 stopped at chunk322/CmFD6838; queued ranges began324, leaving323 missing. This ruled out reload timing as the source of that freeze.

Message25005 ends with Bool `+0x89`. Its fields occupy137bits, although the byte-oriented JAM encoding contains144bits. Native unpacker17870E0 and Bool reader1610A30 retain the final one-bit cursor. Message-channel return1E8AB52..68 resumes the frame at that exact bit. Numeric header readers16108F0/1610870 align intermediate messages, so only the channel's final unused bits must be omitted. Copied-native execution confirmed both Bool values and a following numeric-header sentinel. In a concrete packet, old padding decoded frame acknowledgement2013 as2684483424 and hid the following entity data despite UDP acknowledgement.

## Correction and checks

`Schemas.read_bits` exposes the exact end while `Schemas.read` preserves its byte-oriented API. Game-link message writing trims only the final unused bits; reading preserves that final cursor and still aligns intermediate headers. No weapon timing or transport acknowledgement policy changed.

The regression first failed360vs353bits on the old writer. It covers an elimination after a hit, an unreliable final elimination, and a Bool-ending message followed by a numeric-ending message, then checks the next frame acknowledgement and marker. Full suite:353 passed in12.10s; focused message/feedback/Soldier checks:45 passed. Independent native-format review found no actionable defect.

Patched diagnostic client38172, session `main-menu/20261108T025014.578452Z`: controlled elimination at22:51:31; native chunks continued262→313→384 and the queue drained to0. Subsequent guarded held-fire/manual/empty reload recording accepted71shots, ended at9ammo, and native replication reached chunk1159 with queue0. [Held-fire recording](reload-boundary-after.mp4) and [ammo samples](reload-boundary-after-ammo.jpg) preserve the check. Diagnostic client exited0.

Final normal launch: client37900, session `main-menu/20261108T025326.276954Z`, exact user command with `retail_menu.asi`. Left match98D6 and entered97DF in the same process. Guarded manual reload followed immediately by held fire, and held fire through empty reload, accepted151shots across both recordings. The final recording killed B0000501 at22:57:36 and B0000502 at22:57:42, then continued damaging respawn B0000503 to120health at22:57:46. The native manager reached chunk1630/CmFD25160 with queue0; instance53 was26, matching server ammo26, and instance51 wasFalse. [Normal re-entry bot-hit recording](reload-boundary-reentry-bots.mp4) and [overview](reload-boundary-reentry-bots.jpg) preserve the check. This normal game remains open for the user.

No client files or game-process memory were modified. Physical controller retry of this latest correction remains unverified; the preceding input correction was confirmed by the user.
