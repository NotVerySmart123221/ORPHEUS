# Soldier presentation: offline follow-up, 2026-11-07

Final outcome: all five presentation items are saved and verified. Clean normal clients12312/34048 passed primary gun motion, Helix spiral, Sprint transitions, reload, Field ring/expiry and native cooldown countdowns. Both clients exited0;350 tests pass (12.84s). [Current progress](five-items-progress-2026-11-07.md), [primary video](verified-primary-2026-11-07/README.md), [Helix/Field evidence](verified-field-helix-2026-11-07/README.md). Other viewers retain the existing Helix projectile fallback. No user Range sign-off is claimed.

Original offline scope below: read-only retail assets, lobby source, existing Graphify graph, and IDALib session on `ida/Overwatch-state-analysis.dump.i64`. That phase involved no client launch, process writes, debugger attachment, ASI loading or database save; its intermediate unknowns are superseded by the verified outcome above.

## Voice: correct key table established

Prior note `voice-and-ring-findings.md` proposed matching message `+0x90` against `STUVoiceSet.m_FCF3C39E`. Static code now disproves that proposal.

`sub_7FF6D1FD5380` reads array at object `+88`, compares each u64 against its second argument, then returns entry `index * 64` from array at `+72`. OWLib's `TankLib/STU/Types/STUVoiceSet.cs` identifies those fields as `m_voiceLineGuids` (`F7FA2CAC`, +88) and `m_voiceLineInstances` (`F51B70C4`, +72). `m_FCF3C39E` lives at +120 and is not searched here. Processor `0x7FF6D1D354F2` calls `sub_7FF6D1E067F0`, then loads record `+0x18` and calls this lookup at `0x7FF6D1D35508`. Record `+0x18` corresponds to network message `+0x90` in prior dispatcher trace.

Primary asset: `0x07A0000000000056`, from `Manifest/Win_SPWin_RDEV_LenUS_speech_EExt.cmf`. Local payload is 431,776 bytes; size and MD5 match manifest. Parsed all 612 inline records exactly: parser consumed 16,364/16,364 bytes of `F51B70C4`; parallel `F7FA2CAC` contains 612 keys. Inline field `BC474019` gives embedded `STUVoiceLine` index; that object's `401F5484` gives stimulus.

Concrete candidates:

| Array index | Voice-line table index | Key for message +0x90 | Stimulus | Embedded debug label |
| --- | --- | --- | --- | --- |
| 150 | 2592 | `0x0760000000006FA4` | `0x0EE000000000000D` | `MtchStrt-Sn` |
| 159 | 2736 | `0x0760000000007022` | `0x0EE000000000000D` | `MtchStrt-Sn` |
| 164 | 2816 | `0x076000000000702D` | `0x0EE0000000000008` | `EnmyKlld-InKllcm` |
| 275 | 4600 | `0x07600000000070CF` | `0x0EE000000000000D` | `Lctn` |

Debug labels come from voice-line field `38F3ED5E`: its dynamic-data string header has length then string pointer. They identify authored context, not spoken text. Stimulus 0x0D includes both match-start and location lines, so choosing any line with that stimulus is insufficient to identify ordinary spawn speech.

Smallest next step: establish whether intended speech is match-start or ordinary spawn; inspect relevant line criteria and native 27500 argument semantics, then send one identified line at body-spawn seam. Lobby currently sends neither 27500 nor 27501 (`tools/lobby/vendor/ow174/game/match.py::_spawn_steps`; protocol already registered in `messages.PROTOCOLS`). Do not add a sender with guessed remaining fields.

Schema correction: `tools/lobby/vendor/data/game_schemas_174.json`, protocol `82A2F991`, proves 27501 is **not** merely 27500 plus appended fields. Both have three entity structs at +0x78/+0x7C/+0x80 and u64s +0x88/+0x90. 27500 then has int32 +0x98 and u64 +0xA0. 27501 instead has u64 +0x98, int32 +0xA0, u64 +0xA8, floats +0xB0/+0xB4, bool +0xB8 and byte +0xB9. Old note's shared-offset description should not guide a 27501 sender.

## Helix: model and effect references were missed

Current `tools/lobby/soldier.py` comment says native projectile 0x072C has no model. Retail definition contradicts it: `STUModelComponent` (class `191835F9`) field `2D124BE2` decodes to **`0x0D00000000001883`**; `9297ADB4` is look `0x0980000000001DB7`. Model payload exists, 30,784 bytes, with matching manifest MD5. This proves a model reference, not that it draws or contains three visible rockets.

`tools/assets/stu_refs.py::PREFIXES` excludes model prefix `0x0D00` and effect prefix `0x0300`. Consequently absence from its default decoded-reference output cannot prove absence from an authored asset. Directly decode known fields with `retail_bundle_lookup._decode_asset_ref` and STU header CRC.

Native projectile 0x072C `STUProjectileVisualComponent`:

- `A18274A3` -> effect `0x0300000000000977` (40,224 bytes).
- `6CB564CC` -> effect `0x0300000000000976` (1,488 bytes).
- Child entity 0x072E `STUEffectComponent.58D56DD4` -> effect `0x0300000000000978` (32,064 bytes).

Effect0976 has exactly three `ECEF` chunks at offsets 0x120, 0x1C0, 0x260, each referencing effect `0x0300000000001953` (43,024 bytes). This is a concrete three-child authored visual lead; relation to visible spiral is unproven. Chunk headers checked for serialization tag 0xC65C; layout follows `TankLib/teChunkedData.cs` and `Chunks/teEffectComponentEffect.cs`.

Current `_confirm_rocket` sends only native definition072C and transform, no component payload or movement; current `_projectile_updates` still destroys/recreates effect child072E each frame. Existing reservation/fixed-ID confirmation experiments proved entity creation, never native drawing. Do not repeat those experiments as new fixes. Next useful read: native ProjectileVisualComponent's selection/start of effects0976/0977 and model1883, then identify missing server component/owner data. Replacing straight glow with three independently flown server projectiles would alter the authored single-projectile gameplay and does not establish native spiral.

## Biotic Field: exact effects, same unresolved attachment seam

Retail impact definition `0x04000000000003DC` owns graph `0x0580000000000258` (600). Graph600 node24 (`STU_691BFA55`, remote/state index2) uses config-effect node37 -> **`0x0300000000000B06`** (21,616 bytes). Node92 (same state class, state index9) uses node93 -> **`0x0300000000003398`** (384 bytes), targets dynamic var2712 through node95, and carries identifier `0x0D80000000000A98` in its mapping fields. Node81 CosmeticEntity outputs var2712 through node99 and uses canister definition `0x040000000000068A` through node97. All effect payload MD5s match manifest.

Current `throw_field` creates only canister068A with team bits and transform; it does not create impact03DC or run graph600. Prior reverted impact/graph600 runs already activated CosmeticEntity3 and effect9 without rendering, so simply restoring that frame is not a demonstrated fix. Effect3398 is a tiny `ECME` effect, whereas effect0B06 contains particle/lighting chunks; do not assume both are interchangeable ring assets. OWLib has no local ECME parser.

Smallest discriminating check remains read-only: while impact03DC/graph600 exists, inspect CosmeticEntity3 output +0x1E0 and effect9's resolved effect/target/hardpoint. A null var2712 output isolates creation/attachment failure; a valid output moves investigation to effect startup/mappings. Without that check, no complete ring fix is established.

## Acceptance still outstanding

Offline assertions prove schema/assets and message table lookup, not rendering/audio. Practice Range acceptance must verify three spiralling rocket visuals without duplicate glow artifacts; correct field ring at floor position for its five-second lifetime; correct ordinary spawn speech once per appropriate spawn; existing damage/heal/cooldown behavior unchanged. No live check performed in this follow-up.

Graphify query used existing 1,981-node graph, finding `soldier.py`, `helix_states()`, `field_instance()`, and existing Biotic Field/match nodes. New conclusions above rely on reread primary source and assets; graph alone lacks these decoded fields. No graph rebuild or source-fork changes.

## Matrix native follow-up requested during research

DeflectProjectiles class `0EC57928` registration record is `0x7FF6D401D300`. Preserved state registries have null prototypes; direct constructor trace resolves it: thunk `0x7FF6D1D064C0` -> `0x7FF6D1D030A0` -> final primary vtable `0x7FF6D38743E0`. This avoids interpreting a null prototype as unsupported state.

Bounds' `STU_FDBB0B67` multiplier has an entity input (`A83C2C26`, OWLib `Misc.cs`), not an authored scalar literal. Its evaluator is `0x7FF6D1F0C030`; it resolves entity input, calls `0x7FF6D1F3E970` for a float, compares it with 1.0, and registers a dependency if it differs. Getter enters obfuscated entity/component lookup. Exact quantity and default were not established, nor was box transform relative to eye/body/aim. Parent agent independently decoded four authored box vectors; this trace does not justify claiming their world transform is exact.
