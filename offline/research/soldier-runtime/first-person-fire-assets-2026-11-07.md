# Soldier first-person fire assets — 2026-11-07

Retail asset and native audit. The later trace established an authored additive firing tree and a separate procedural recoil spring. Earlier candidate classifications below remain useful; their initial lack of a proven fire route is superseded by the following findings. Clean saved-code primary fire and the combined presentation/countdown rechecks passed in normal clients12312/34048. All350 tests pass (12.84s).

## Authored firing route and native replication correction

Graph596 Volley10's `m_onBeginPlug` starts Chase19, whose instance output1769 feeds Mapping18 and DataFlowF7. Mapping17 supplies F8 from1770; Chase20 is the Helix route. Tree25D uses F7/F8 parameters before proxying to tree10. Arms internal alias801D305F reaches clips1BF4/1BF5/1BF6/1BF7/1BEC (0.7667s) and supplementary1BF2 (0.0333s); the gun's internal alias801E6004 reaches3865/1D02. The held-burst timer starts once per Volley begin, rather than once per bullet. The first-person arm model's hardpoint0A attaches the gun at bone74.

Three failures had hidden this route. F7 stayed at constant100 rather than advancing with Chase19's authored Float output. C0 owner data frames left the native authoritative DSR clock unchanged. Finally, a static output binding cached Chase19's first answer (14ms) indefinitely. Native binding reader `1C2D4F0` stores the dynamic/cache flags at binding+41/+42; `2DC70B0` transfers dynamic status to variable flags, and `2DC6BE0` refreshes dynamic bindings when the context frame changes. Wire tail0011 enables a dynamic output cached once per frame; default000 preserves existing static bindings.

Isolated control81 uses C1 data frames, Float100 defaults, per-frame StateBound output19 and Chase19's elapsed time since burst begin. A bounded native watcher captured F7 resetting to0.007s, advancing through a1.8s burst and release, then resetting to0.014s for a second0.5s burst. F8 stayed100. Mapping18's actual answer ConfigVar is at schema+F0; copied native lookup confirmed its Float result. Body Anim39's procedural Spring output (DataFlow50) varied0..0.636. Its callback/replay counter is not a count of visibly rendered shots. The separate stable-Volley-counter/elapsed scheduler control82 caused excess ammo consumption and was rejected.

The first-person appearance uses slot0 and ModelComponent3; component40 is unrelated to the pose buffer. Copied `1F41470` returned the same point for authored gun hardpoint0A and root hardpoint14, indicating a fallback. Those returns do not prove bone74 selection or firing-bone motion and are excluded as evidence. No client process writes were used. The saved implementation follows control81. [Clean ordinary-input captures](verified-primary-2026-11-07/README.md) show burst/release motion and correct ammo. The combined Helix/Sprint/reload/Field sweep and fresh countdown/firing recheck passed; both normal clients closed with exit0.

## Authored model chain

Soldier hero `02E000000000006E` selects first-person entity `04000000000003D1`, arms model `0D00000000001289`, and blend-tree set `00400000000001D3`. Child entity `04000000000004BF` selects gun model `0D000000000013C6` and set `00400000000001E6`; it attaches through hardpoint `0DC000000000000A` to arms bone74 (`0x4A`). Gun motion can therefore come from the arms attachment without a separate animated gun clip.

Graph596's primary Volley10 has a `STUStatescriptWeaponProjectileVisual` (`F18160D1`) whose field `F3E60FB5` is `m_animRecoilImpulseFactor`, a ConfigVar with value0.1. The schema has fixed DataFlow entries, this factor and a HUD-warning ConfigVar; it has no clip field. The graph's StateAnim7/39 use reload alias `0C80000000000004`. No per-shot StateAnim was found in this graph.

## Short candidates disproved or unresolved

The old `research/blender-animations.md` assertion that1C08 is the best per-shot clip is unsupported by its authored route:

| Clip | Actual binding in arms set01D3 |
| --- | --- |
| `0A00000000001C08` | Item15, alias`0C80000000000029`, `m_onFinished` slot animation. The item's main clip is1C07, tree`0F800000000000A6`. |
| `0A00000000001BEA` | Item16, alias`0C8000000000001E`, `m_onFinished` slot animation. Main clip1BE9, same treeA6. |
| `0A00000000001BFD` / `0A00000000001BFB` | Item17, alias`0C8000000000000D`, two `m_onFinished` slot animations. Main clip1BFE, tree`0F8000000000005C`. |

These are exact serialized roles, not semantic names for the actions. The enclosing field is `560940DC` (`STUAnimBlendTreeSet_BlendTreeItem.m_onFinished`), followed by `6CB79D25` (`STUAnimBlendTree_OnFinished.m_slotAnims`) and `3F5B86A4` (`STUAnimBlendTree_SlotAnimation.m_animation`). Their small amplitudes do not establish primary-fire relevance.

Gun clip `0A00000000001CEB` is a static pose despite its0.166667s duration. Direct payload parsing found23 raw bone tracks and zero position/rotation/scale channels with multiple keys;21 bones match the gun skeleton in the existing export. It is reused through set01E6's internal category/movement branches (tree7A and tree15), and alias0D selects it through tree117 → internal alias`0C800000801E6023` → tree5C. It cannot itself supply per-shot motion.

Alias`0C800000000000A9` resolves to arms clip`0A00000000003A5F`, duration2.533334s, via tree`0F8000000000001E`. Generic model graph21's StateAnim19 can select it. No authored primary-Volley-to-this-StateAnim edge was established; calling it a fire clip, or definitively an equip clip, would exceed the evidence.

## Numeric recoil route

Existing native trace identifies Volley post-shot call `0x7FF6D1D06DA4` → `0x7FF6D17EFB90`, with resolved recoil configuration `[info+0x40]` and current-frame target from `0x7FF6D17A1330`. Fresh static inspection confirms17EFB90 copies numeric configuration into target offsets`14F8..154C`; it does not request a blend-tree animation alias in that block.

Consumer `0x7FF6D17EB910` checks active state at`0x7FF6D17EBAE9` (`[this+1550]`). At`0x7FF6D17EBF41`, state1 integrates elapsed`+1554`, uses timing value`+1524`, advances to state2 at`0x7FF6D17EBF94`, and computes easing scalars multiplying`+1558/+155C`. It adds those values to its second-argument output (`RBP`), for example at`0x7FF6D17EBFD5..17EBFDA`. This proves a procedural numeric envelope, not skeletal clip playback. The output's eventual gun-bone consumer was not established here.

Confidence: high for the GUID chains, finish-slot classifications, static1CEB pose and numeric recoil route; insufficient to identify the complete source of visible gun-model kick. Historical polling of a once-resolved17A1330 target cannot settle this because the target changes with frame history.

## Native input and State61 correction

Fresh-body diagnostic confirms native held input arrives: the instance context's held list (`+440/+448`) contains logical button20/key111 during LMB, and LogicalButton8 private`+240` becomes1. Input registration is not the blocking seam. After omitting manually forced owner states10/13/21/23/29/57/68, Volley execution`+270` reaches1, then resets0 within approximately2ms; count`+280` stays0 while held input persists approximately600ms. The authored chain can start a native shot. Rapid rollback/reset, rather than a permanently blocked input gate, is now the discriminating symptom; authoritative-update causality remains under test.

State61 is plain `STUStatescriptState` (`7C37840C`), node514/owner57, with a transition link to Volley10. Generic activation strategy`383BD78+48` → `2D8EBA0` has no own already-active latch check. Its call through instance context vtable`+138` resolves to`1B9A170`, which calls`1C2A0D0(context+5C0,...)`. That path records a diagnostic event under trace gates; it is not the state scheduler or activation gate. Earlier interpretation of that notification as scheduling was wrong. Permanent owner57 remains an invalid confounder for a prediction-only fixture, but omission plus the observed native shot does not support it as the sole cause of missing sustained gun motion.

## Reproduction and sources

Targeted reference decoding: `.scratchpad/shooting_asset_audit.py` (disposable). It follows both blend sets and every directly referenced blend-tree asset, decodes references with the payload's field hash/CRC, and reads inline blend-tree items before assigning aliases. Payload retrieval uses the existing manifest/MD5-verified `find_asset_payload` path.

MD5: set01D3=`ba9ec656d8abb81ea09d0aec6e5321a4`; set01E6=`f3ca4caeec1e817b6e775f268c237584`; clip1CEB=`95c90fca139b71ed9e7fb857ae07396b`.

Primary format sources: local OWLib `TankLib/STU/Types/STUStatescriptWeaponProjectileVisual.cs`, `STUAnimBlendTreeSet_BlendTreeItem.cs`, `STUAnimBlendTree_OnFinished.cs`, `STUAnimBlendTree_SlotAnimation.cs`, `STUAnimNode_AnimBlendTreeProxy.cs`; retail payloads above; native IDALib database `ida/Overwatch-state-analysis.dump.i64` (session`da801ffd`). Supporting repository readers: `tools/assets/animation_aliases.py`, `tools/assets/stu_graph.py`, `tools/assets/anim_export.py`. Earlier runtime work and its corrections: `research/fire-animation-trace.md`, `research/soldier-runtime/five-items-progress-2026-11-07.md`.
