# Does the client contain bot AI code? First pass (2026-11-02)

Status: **inconclusive.** Name-based searches can't answer it for this binary.

## What was checked

- Decrypted runtime image `ida/Overwatch-runtime.dump` (58 MB): 72,208 ASCII/UTF-16 strings. No AI, bot, navmesh, pathfinding, behaviour-tree, blackboard, perception or difficulty identifiers. The only matches were UI words (`Bottom`, `NavigateUpElement`) and `input.GamePadAimAssistEnable`.
- MSVC RTTI type names: only 71, all from third-party libraries (Enlighten, DGTF). Blizzard's own classes have no RTTI names, so their absence says nothing.

## Why this doesn't settle it

The binary carries almost no names for game code. Missing strings don't prove missing code.

## Next evidence (before work step 2, bots shooting back)

1. **Statescript node registry:** `tools/research/state_types.py` can already list the client's registered state/node factories. Compare the list against node types that only an AI driver would need (move-to, aim/track target, choose target, path follow).
2. **Data side:** look in CASC for AI/bot behaviour assets referenced by the Range bots and the Practice vs. AI mode, and check whether their STU types have a matching client deserializer.
3. **Range bots specifically:** existing research (`remote-bot-statescript.md`, `weapon-volley-dsr.md`) shows the Training Bot's weapon runs through the shared Volley statescript, which is data plus shared code. Range bot firing may therefore not need dedicated AI code at all.

## Second pass: AI node classes (2026-11-02)

- With OWLib's type names (`tools/assets/stu_names.py`), the Range bot graphs use AI-specific node classes (unnamed in OWLib, parent `STUStatescriptState` or the `STU_E474EECD` family): `ECAEE401` (priority, string, target vars), `E88BA88B`/`D7F9E1C3` (identifier-driven states in the shooter graph B92), `194D744E`/`AB9B30EF`/`2C18F116` (patrol graph B8E), plus `STUECAISpawnerInstanceData` and `STUAIBehaviorComponent` (difficulty profiles, per-mode behaviour scripts).
- The client **registers** every one of them: each has a deserializer registration (e.g. `0x7FF6D1461D80` for `ECAEE401`: name "Unknown", size 0x160, ctor/serialize callbacks) and appears in the class-hash switch `sub_7FF6D2172FF0`.
- In that switch, `ECAEE401` and its neighbours all branch to the same shared target `0x7FF6D2172E50`, so the switch says nothing about AI-specific logic. Still open: whether the node runtime (OnBegin/update virtuals of the registered objects) is real code or stubs.
- Practical consequence for the Range: the spawner data names the behaviour (static / B8E patrol / B92 shooter), and the server rebuilds it (`tools/lobby/target.py`), per the roadmap rule for missing or unproven client AI code.
