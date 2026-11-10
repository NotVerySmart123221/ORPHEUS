# Biotic Field ring assets — 2026-11-07

Retail asset and native-loader audit, followed by a server-only movement fix. The authored ring is inside effect `0300000000000B06`; the corrected isolated fixture visibly renders its yellow circular floor boundary. No client-memory writes or game-file changes.

## Ring dependency chain

Graph600 (`0580000000000258`) node24 / StateEffect2 selects effect `0300000000000B06`. Its graph parent is TrackTargets0; this establishes the authored call route, not the correct target choice for a synthetic fixture.

Effect0B06 contains two decal components, `ECDL` at file offsets `0xE80` and `0xFA0`, under `ECMP` component indices13 and14. Their shader setup `ECSS` chunks at `0xEB0` / `0xFD0` refer to:

| Decal | Material | Material data |
| --- | --- | --- |
| 13 | `0E00F54194B22D5C` | `04D0DC7E58A3D104` |
| 14 | `0E00A153C3CF2267` | `04D08F1CC46F0384` |

Both material-data texture arrays reference `0C00000000003684`, input hash `49F172E8`. Its full-resolution payload is `0320000100003684`. Decoding the 512×512 BC4 texture produces the white circular ring with a fading inner band: [decoded texture](../../.scratchpad/field-decal-texture-3684.png). Scratch preview is disposable; the GUID chain above is durable.

Texture payload decoding follows local OWLib `TankLib/teTexture.cs` and `teTexturePayload.cs`: BC4 endpoints are stored as one u16 plane, followed by six index bytes per block. Directly treating the payload as interleaved BC4 produces a misleading corrupted image.

Both decals have start0.0s, duration3.0s and hardpoint0 in their ECMP timing records. Decal13 references tracks25/25/26, constants4.8/4.8/2.0. Decal14 references tracks39/39/26, constants4.5/4.5/2.0; the earlier claim that both use25 was wrong. Those references are consistent with projection dimensions. Additional references51/52 are computed tracks, not decoded into a named behavior here. No missing radius variable is established.

Native loader `0x7FF6D29C4120` selects ECDL loader `0x7FF6D29C75E0`. Registry table `0x7FF6D3999F50` identifies ECDL as runtime component type9, distinct from light ECLT/type11 and particle ECPR/type3. Loader copies the ECDL raw float0.1 to descriptor+112, track IDs25/25/26 to+116/+118/+120, and51/52 to+128/+130. This supplies a specific renderer discriminator: active effect/light does not establish creation of its type9 decals.

## Canister and material effect

Definition068A references model `0D00000000001ACF` and look `098000000000206E`. All12 submeshes across LOD masks1/2/4/8 fit approximately x/z±0.07m and height0.233m; there is no floor-sized plane. Look206E has three base materials and material-effect entry `02600000000003C3`, including a material replacement. Effect3398 is a single `ECME` referencing that same03C3 entry. This is the canister material-effect route, separate from the proven ring texture/decal chain in0B06.

## DataFlow and root definition

Graph600 Mapping5/node63 maps DataFlow identifier `048000000000011E` to Int1, using default entity selector `STU_02C0109E` at node66. This identifier is absent from the root CMF; it is not a physical asset available through `find_asset_payload`. The pinned graph catalog also uses it in Brigitte graph4798 and Torbjörn graph5921 with0/1 answers. Its semantic name and influence on the decals remain unproven.

Impact03DC has TargetTag, Pet, Model, `50BA775C` and Statescript components. Its ModelComponent has no model/look reference. Its `50BA775C` only serializes `A67C8B54=1`; OWLib names radius/height fields but leaves that flag unnamed. Assets alone do not establish that this component discards the create-position or that movement replication fixes it. Explicit canister068A has only TargetTag and Model components.

## Proven placement failure and fix

PID33136 control: root`B2F20001` was created at server position(54.56507,1,42.14788), but its native component1 attachment`1585F728A20` retained zero position at+130, identity matrix at+60 and no parent at+288. This was checked against the player's same component: body attachment`1585F72C960` decoded through native`2D23150` to(54.56543,1,42.14746), scale1. The original Field root therefore lacked the actual transform; this conclusion no longer rests on the ambiguous identity matrix alone.

One-variable control recreated the same03DC/owner graph600 fixture as root`B2F20002`, adding only `movement=world.Movement(position)` to `world.create`. Native root attachment`1585F71ACC0` then held the expected translation. [Frame04](../../.scratchpad/field-resolve-movement/04.png) visibly shows the yellow circular floor boundary; frames24/44 retain it. Frame44 is4.855s after00. A [later still](../../.scratchpad/helix-model-transform/08.png),103.445s after00, retains the ring without counter rearming. The3s ECMP span does not bound the held native effect's visible lifetime.

Saved server path now creates03DC with initial movement, then owner graph600 instance1 with active states2/3. Sync slot0 binds `StateBound(Entity(0),1,3,4)`, the native CosmeticEntity3 type7 output. Plain Entity32 and kind4 ScriptRef do not satisfy that attachment. The existing five-second Field deadline destroys the root; no replacement glow, periodic rearm or separate canister entity was added.

Normal-client PID18040 saved-code cast showed the ring and canister, then both disappeared by [frame52](../../.scratchpad/field-saved-code-lifetime/field-lifetime-f52.png), after the five-second deadline. [Frame15](../../.scratchpad/field-saved-code-lifetime/field-lifetime-f15.png) exposed a checker material on the canister. A one-variable [paired control](../../.scratchpad/field-material-pair/15.png) proved State9 causes it: left, states2/3, has the correct black/gold canister and ring; right, states2/3/9, has the checker texture. The final sender omits the separate material effect3398. Its missing material mapping remains unknown; the ring does not require it.

Regression exercises `Match.throw_field`, checks the raw create's movement flag and captured throw position, matches the native-verified states2/3 graph frame, covers two viewers and the five-second destroy. Existing healing/radius test remains green. Final clean normal client PID33876 accepted saved states2/3: [active](verified-field-helix-2026-11-07/field-clean-active.png) shows the full gold floor ring, beam and black/gold canister; [expired](verified-field-helix-2026-11-07/field-clean-expired.png) shows the ring and canister gone. No material checker remains. All344 tests pass;122 cover world/Soldier/rifle/target.

Static decal discriminator, not a separate live-head acceptance claim: FX dispatch table`44B0740`, stride88hex, type9 initializer`2B4BC20` checks FXinstance+1C0 bit40 and dimension evaluation`2B4D660`. Per-component runtime stores its projection matrix at+00, dimensions+44/+48/+4C and decal linked-list head+50. Renderer`2B4E990` traverses that list. Actual visible circle, not generic light/particle activity, establishes rendering in the control above.

## Integrity and sources

All payloads were read through `find_asset_payload`, which verifies manifest size and MD5. Selected MD5s: effect0B06=`b894d96a20a21da13f38fcf5d52adecd`; effect3398=`f8713928082536bbb1c8b4f23525ece9`; model1ACF=`bf4b1a237bf7fadd1dde029a3adc6623`; look206E=`23a7c57dda296572617f770925e6fdf2`; texture3684=`9d7aa9c871b077acbd11a8869c0b75bf`; full-resolution payload=`682c7bd89332ce0feecd104da14dea5f`.

Primary format sources: local OWLib `TankLib/teChunkedData.cs`, `Chunks/teEffectChunkComponent.cs`, `Chunks/teEffectChunkShaderSetup.cs`, `teMaterialData.cs`, `STU/Types/STUModelLook.cs`, `STU/Types/STUModelMaterial.cs`, `STU/Types/Misc.cs` classesD75EA2E1/C7D04A1A/50BA775C; native IDALib database `ida/Overwatch-state-analysis.dump.i64`. Read-only scratch reproduction: `.scratchpad/field_asset_audit.py`.
