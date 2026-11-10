# Soldier:76 body graph initialization

## Source and correction

Catalog: pinned `Boi-027/overwatch-1.74-lobby-server` commit `0fa61b950dfc9c915cb2353d2873d6d4689c2a5e`, [`data/game_graphs_174.json.gz`](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/data/game_graphs_174.json.gz). The local read-only copy used was `.scratchpad/game_graphs_174.json.gz`.

**Catalog fact:** Soldier:76 body `0x04000000000003CF` declares manager graph 21, weapon slot 1 graph 596, and body graphs 51, 75, 67, 593, 597, 599, 601, 2331, and 2230. Graph 46 is absent from both this body record and the catalog's graph index. Earlier references to body graph 46 were incorrect for this pinned catalog.

## Entry paths

**Manager graph 21:** root Entry at node position 0 links to node 72. That fan-out starts state 11 (`STUStatescriptStateBooleanSwitch`) and state 39 (`STU_CD46AF93`). State 11 tests dynamic variables 478 and 1956. Its true output enters client-only state 30 (`STUStatescriptStateDataFlowMapping`), mapping resource `0x04800000000005E2` into dynamic variable 31. Its false subgraph lists 34 states, including state 38, a client-only `STUStatescriptStateUXPresenter`. State 39 contains two dynamic values (9526 and 9573) and a subgraph reference to state 41. These paths are conditional/state-script structure; catalog data does not establish which branch runs in the current prototype.

The same manager graph has a second Entry at node position 56. It starts states 36 and 37, both BooleanSwitch states. Each condition reads a different dynamic variable (9783 or 9784) with default `true`; each false branch points into a client-only subgraph (states 34 and 33 respectively). A third entry-like node at position 78 leads to state 44 (`STU_9D7BF987`); it is not marked client-only in the catalog. Do not conflate this class with the explicit UXPresenter state at state 38.

The manager contains ActionSetVar nodes that target dynamic variable 30 with constants 1, 2, or 3, and nodes that copy between variables 30 and 31 or copy variable 16515 into 30. These are actual setter definitions, but their presence alone does not prove they execute on root Entry: the graph's state/event conditions and links govern reachability.

**Body graph 51:** its root Entry at node position 0 links to node 39, state 13 (`STU_CD46AF93`). That state references substates 4, 3, 0, 10, and 1 and connects their begin/input plugs. States 0, 1, and 3 are HealthPool states; state 4 is a BooleanSwitch reading dynamic variable 232; state 10 evaluates entity/config conditions. The body descriptor supplies schema variable `0x0D800000000000C7` as integer 200. Graph 51 also has other entries, including client-only and server-only message/event entries; these are not unconditional root initialization.

Other declared body graphs likewise have multiple Entry nodes and conditional/game-message routes. For example, graph 75's main Entry fans out into many states, while graph 597 has a client-only Entry/state route. Do not collapse all graph entries into the server's initial owner frame.

## State 11 condition: opcode comparison, not retail confirmation

The pinned catalog records state 11's condition as opcodes `[8, 0, 3, 4, 8, 1, 0]` and dynamic-variable operands `[478, 1956]` in that order. The pinned upstream tree contains no expression evaluator/parser (only graph and state serialization code), so opcode 8's retail meaning and runtime evaluation are not confirmed here.

The older beta parser at `<beta-project>\repo\Prometheus-Core\StatescriptExpressionParser.h` labels opcode 3 as a truthy conditional jump and opcode 4 as a falsey conditional jump. Under that parser's two-word opcode format, the `4` after `3` is the jump distance operand, not a second opcode; the stream resembles “load operand 0; if truthy jump to exit; load operand 1; exit,” an OR/short-circuit pattern. This is comparison evidence only, not proof of retail 1.74 semantics.

**Consequence:** even under the beta-parser interpretation, a false value for dynamic variable 478 does not make the branch true by itself; dynamic variable 1956 must also be truthy. Do not set or infer the manager's branch from variable 478 alone without confirming opcode-8 and opcode-3 semantics for retail or observing the native evaluation.

## Implication for the current prototype

**Evidence:** the current body frame is reported to contain only weapon graph 596 instance 1. That omits manager graph 21 and every graph listed in the catalog body descriptor, including graph 51's health pool and the manager's state/dataflow logic. Manager graph 21 contains a client-only UXPresenter state and a client-only dataflow mapping; the descriptor's graph 51 contains health-pool state definitions and a 200-valued schema entry.

**Inference, not runtime proof:** constructing only the weapon graph can leave native body/manager behaviors unavailable, so adding the catalog-declared manager and body graph instances is a grounded next diagnostic. The catalog does **not** prove that graph 21 or graph 51 alone produces the retail HUD or enables firing, nor does it establish the exact active state bitmap/variable initialization needed. Preserve each graph's Entry/state/event distinctions and owner-frame schema when forming a probe; do not synthesize one guessed flattened set of all Entry targets.

## Soldier body UXPresenter inventory

The pinned catalog has eight UXPresenter states across the listed Soldier body graphs, using six distinct assets. Presenter `m_F2160520` resources were individually resolved through the read-only local retail bundle lookup; all six payloads hash-verified inside bundle `0x0D9088411C29B98D`. These opaque asset IDs have no semantic names in the catalog or local parser.

| Graph / state | UX asset | Entry/subgraph ancestry | Dynamic inputs recorded in presenter config |
|---|---|---|---|
| 21 / 38 (client-only) | `0x0A880000000002BC` | Entry 0 -> fan-out node 72 -> state 11 false-subgraph includes state 38; states 35 and 42 also parent 38 in the catalog | Own condition vars 3772, 7760; nested bindings 21446, 243, 3852, 3856 |
| 596 / 68 | `0x0A8800000000023F` | Nested under state 67 -> state 9 -> state 0. Entry 0 itself links to Stack state 64 and action node 127, not this chain. | 10964, 29677, entity var 31, instance var 28 |
| 596 / 69 | `0x0A88000000000240` | Nested under state 9/state 0 and under state 40; no direct Entry 0 link. | 10964, 29677, 699, 2575, 2355, 9743, 61, 472 |
| 596 / 70 | `0x0A880000000002AB` | Nested under state 67 -> state 9 -> state 0. Entry 0 itself links to Stack state 64 and action node 127, not this chain. | 10964, 29677, entity var 31, instance vars 28, 53, 198 |
| 597 / 31 | `0x0A88000000000240` | Nested under state 17. Entry 0's fan-out targets states 34 and 60; it does not directly start state 17. | 10964, 29677, 699, 2575, 61, 472 |
| 599 / 16 | `0x0A88000000000240` | Entry 0 -> state 7 -> state 16's false subgraph. | 10964, 29677, 699, 2575, 2355, 9743, 61, 472 |
| 601 / 32 | `0x0A880000000001EE` | Entry 0 -> state 15 -> state 32's false subgraph. | 10964, 29677, 342, 338 |
| 601 / 33 (client-only) | `0x0A88000000000276` | Nested under states 34 and 44; state 34 is itself under 44, which is under 39. No direct Entry 0 link. | 1030, 11947, 11948, 11949, 17560, 17561, 17858, 215, 567 |

No UXPresenter nodes occur in listed graphs 51, 75, 67, 593, 2230, or 2331. Graph 51 is the catalog-backed health-pool graph, but it has no UXPresenter; the body descriptor's integer 200 is a gameplay/schema value, not evidence of a visible health widget. Weapon graphs 596, 597, 599, and 601 reuse presenter asset `0x0A88000000000240`; their configs reference state variables such as 699, 2575, 61, and 472, while graph 596 additionally references ammo-like instance vars 53 and 198. This makes `0x240` a reasonable weapon-UI candidate, but no field name or catalog label identifies it as ammo, crosshair, or a main hero HUD. Graph 21's `0x2BC` is a conditional client-only manager presenter candidate. These findings do not establish that practice controller graph `0x20E4` is, or is not, the full player HUD.

Verified asset payload sizes and CKeys: `0x23F` 80 bytes / `F58C8CBF744EA75EC80C3996FE5650E2`; `0x240` 208 / `25E9064DEF190A65F290D1565155DF63`; `0x2AB` 260 / `1A009DF47E45DBA2F3EF777C6BECBBE3`; `0x1EE` 416 / `9B1206639619EB3ED639173F4490AE75`; `0x276` 217 / `2851CE82BF93BA632021C049714C4F57`; `0x2BC` 233 / `3BD21034D8CB1E942440CE2D71508ADF`. The lookup verifies local payload hashes only; no UX/STU schema decoder was available to recover asset field names.

## Minimal catalog-backed baseline

For a structural baseline, instantiate the catalog-declared manager graph 21 and the declared body graph 51 alongside weapon graph 596, using each graph's own Entry definitions and graph schema. Graph 51's body schema value 200 and manager 21's Entry paths are directly evidenced. The exact serialized active-state sets and dynamic-variable values remain unresolved; runtime validation is required before calling this the minimum sufficient frame.

## Minimal body and manager startup paths

Native session125210 accepts graph75's state19 and retains it after ten input-driven shots (Volley10, ammo20, target damage/removal/respawn). Entry Stack64 also activates, but weapon instance variable472 remains absent. Initializing these states does not restore the screen HUD or aligned muzzle effects. A separate manager-root probe accepts graph21 state11 branch0 and state39 (runtime vtable3891B20); client presenter38 remains absent because its client-subgraph host42 was omitted. The catalog maps host42 to owner32 and host43 to owner33; both use the same payload-free class as verified weapon state83.

Native instance IDs retain their graph association: reusing graph75 instance2 for manager21 leaves graph75 present. A rifle-only full frame removes it; recreating manager/health instances afterward yields state counts45/31. The graph75 probe now uses distinct instance75. Do not change an existing ID's graph without first removing that instance.

Graph 75 has a directly rooted startup dataflow. Entry 0 links to ActionSetVar nodes 2 and 7. Node 2 copies graph variable 338 to entity variable 338. Node 7 copies graph variable 982 to entity variable 982, then starts state 19, a `DataFlowMapping` for resource `0x04800000000005FD` whose answer is instance variable 982. Soldier's graph-75 schema supplies variable 338 as `Int(1650)` and variable 982 as `Int(1)`. The catalog has 32 sync-var slots, with 16 non-null descriptors (12 entity-scope, four instance-scope); the four filtered instance-scope descriptors are raw slots 15/18/22/30 for vars 14063/2034/982/27004. Variable 338 has no sync-var slot. The graph has 42 states, of which 39 are owner-networked after excluding client-only states 33/34 and server-only state 35; state 19 maps to owner bit 19. **Minimal Entry-output candidate:** activate owner bit 19, set filtered instance presence bit 2 (raw slot 22, var982) to `Int(1)`, and set entity vars 338=`Int(1650)` and 982=`Int(1)` to reproduce the two Entry ActionSetVar outputs. Catalog data does not name variables 338 or 982 as aim or animation, nor prove the entry's non-state ActionSetVar effects are executed for a server-created instance.

Graph 21 Entry 0 links to node 72 (`STU_12F5D52D`). Its two values write entity-scoped variables 9526 and 9573; node 72 then starts state 11 (owner bit 11) and state 39 (`STU_CD46AF93`, owner bit 29). State 39 consumes 9526/9573 and has two remote-sync resources `0x0D80000000002536` and `0x0D80000000002565`. State 11 tests entity variables 478 and 1956. Its true branch enters client-only DataFlowMapping state 30, which maps resource `0x04800000000005E2` to entity variable 31; state 30 has no server owner bit. Variable values and runtime truth of this branch remain unknown.

### Graph 21 client presenter gate

Graph 21 state 42 is node 76 (`STU_BD02E168`) and maps to owner bit 32 after filtering client-only states. Node 27/state 11's false-subgraph lists state 42. State 42's nested group contains states `[28, 26, 35, 38, 27, 24, 29, 31, 32]` and begins at node 50, client-only BooleanSwitch state 28. The switch condition uses config vars `STU_B5A0CAF0` and `STU_1FF67CA6` with expression `[14, 0, 14, 1, 37, 0]`; opcode 37 is the equality operator in the retail VM. Its true branch starts states 29 and 26; its false branch is empty. State 29 is a Stack that sets entity var 7760 to `Bool(true)` and starts state 31. State 26 has its own condition over `STU_5E75FF05` and `STU_BCD1C634` (resource ID `0x0D80000000006161`, type `0x0C8800000000008E`) and a subgraph edge to Stack state 35.

State 35 (node 61) writes entity var 3852 from an expression over the same config-var classes, with constants `1.2` and `0.2`; its subgraph edge begins state 38 (node 64), a client-only UXPresenter. That presenter references asset `0x0A880000000002BC`. Its own condition uses entity vars 3772 and 7760 plus `STU_CC623180`. Parent state 23 (node 42, owner bit 23) sets entity var 3772 to `Bool(true)` and its subgraph edge begins state 42. State 43 (node 77, owner bit 33) is a separate `STU_BD02E168` subgraph containing only state 30; catalog links do not make it a prerequisite for this presenter path.

This gives a concrete catalog route to the presenter: state 42, then the client-only state-28 equality gate, then state 26's subgraph Stack 35, then UXPresenter state 38. The config-var class semantics and state-26 condition result are not established by this graph data alone; only the field identities, values, and links above are asserted. Evidence: pinned [retail 1.74 graph catalog](https://github.com/Boi-027/overwatch-1.74-lobby-server/blob/0fa61b950dfc9c915cb2353d2873d6d4689c2a5e/data/game_graphs_174.json.gz), graph 21 node/state records; state-to-owner-bit mapping excludes client-only state entries.

Graph 51 Entry 0 starts state 13 (`STU_CD46AF93`, owner bit 8). Its subgraph list includes states 4/3/0/10/1; the linked HealthPool states are 0/1/3 (owner bits 0/1/2). HealthPool nodes use entity variables 7786, 7787, and 7788; graph 51 sync-var slots are 32 wide, with 10 non-null descriptors (six entity-scope and four instance-scope). Soldier's graph-51 schema provides `Int(200)` at variable ID 199 (`0x0D800000000000C7`), but no catalog edge maps that schema value to HealthPool amounts 7786/7787/7788. Therefore the 200 value and the HealthPool route are both catalog facts; their direct dataflow connection is unresolved. Graph 51 has no UXPresenter state, so this path alone is not evidence for a visible health HUD.
