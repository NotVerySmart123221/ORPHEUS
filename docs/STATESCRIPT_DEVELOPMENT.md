# StateScript actions: development and measurement

Use the current `rust` module boundaries when extending the VM. This guide integrates useful verification work from the older local foundation experiment without replacing the subsequently merged implementation.

## Execution scope

The VM uses each node's declared execution scope for graph traversal, startup
and message scheduling, and direct dispatch. A client-only node does not run
server actions or start server states. Server-only nodes can execute on the
server while remaining absent from network projection.

## Declare an ordinary server state

An authored node can opt into the plain state lifecycle with
`"state_behavior": "plain"`. Set `server_only` to true and `client_only`
to false. Its `state` must index a slot in the graph's `states` table that
points back to the node. The loader assigns the plain dispatch class, so a
graph can link to that state and use its ordinary lifecycle plugs without
registering the original class or position. These nodes remain outside the
network projection. Graphs without the declaration still use their existing
dispatch and diagnostics.

## Declare a forwarding action

Set `"action_behavior": "forward"` on an authored server-only action whose
job is to follow its `m_outPlug`. Supply a plug with a link list and leave the
action out of the state table. Links run in their declared order with the
regular VM budgets. An empty plug does not take a transition. The action
requires no registration by class or graph position.

## Follow an action

1. [`script_graph.rs`](../rust/crates/statescript/src/script_graph.rs) loads authored graph metadata and preserves wire slots and link order.
2. [`script_init.rs`](../rust/crates/statescript/src/script_init.rs) exposes the component API; [`machine.rs`](../rust/crates/statescript/src/script_init/machine.rs) owns execution state and queues.
3. [`vm/nodes.rs`](../rust/crates/statescript/src/script_init/vm/nodes.rs) dispatches actions and writes variables. Reuse its shared write and routing mechanisms, including the newer nested-destination support.
4. [`script_expr.rs`](../rust/crates/statescript/src/script_expr.rs) evaluates live operands, while [`world.rs`](../rust/crates/statescript/src/script_init/world.rs) supplies host queries. Values, World reads and random draws must retain their event-time semantics.
5. [`script_snapshot_wire.rs`](../rust/src/script_snapshot_wire.rs) encodes the resulting state. Fixture agreement and original-client presentation are separate acceptance categories.

## Check graph links

With the Inspector enabled, startup logs show up to 16 decoded links whose
target position is absent from both the node and container tables. Each warning
identifies the graph, source node, plug path, and zero-based link index. You can
also inspect links through `GraphStore::dangling_links`. Explicit null targets
and declared containers are valid. The report is read-only and continues to
show structural problems even if the VM never reaches the source node.

Startup also counts nodes declared client-only by the loaded graph data. The
live Inspector reports active plain states under `active_states` and separates
known registration rejections from other unsupported nodes. The first 16
rejected graph and node locations appear in `registration_rejection_sites`.
The registration category covers the explicit camera placement guard and the shared guarded
state registration path. It records a rejection only if execution reaches the
guard and no handler accepts the node. An inactive or absent node in a single
snapshot gives no evidence about whether a run reached it.

## Pinned action contract

The [synthetic fixture](../fixtures/behavior/script-actions-v1.json) records 15 cases against Rust `f763beb9d7017b9364a3c8fc79ae6192df590043`. Its expected values were checked against that old executor before integration. The [consumer](../rust/crates/statescript/src/script_init/tests/actions_contract.rs) uses the current production loader and VM, checking scoped writes, dynamic values, error sites, RNG consumption, index behavior, work budgets and ordered links.

```powershell
cargo test --locked -p orpheus-statescript --lib actions_preserve_the_pinned_synthetic_reference_contract
```

This is a Rust regression contract with a historical reference, not a Python or original-server equivalence claim. It runs in both public workspace test lanes; the existing cross-language contracts remain separate. Do not update expected output merely to accommodate an implementation change: explain the evidence and version any intentional semantic change.

## Synthetic action benchmark

```powershell
cargo bench --locked --bench script_actions
```

The [Criterion harness](../rust/benches/script_actions.rs) uses production `GraphStore` and `Component` APIs. One workload parses, compiles and drops an 11-node synthetic graph; another processes 32 messages with changing assignment, conditional and Switch inputs. Component setup is outside the execution timing. Expected state is checked before sampling, including every conditional and Switch path. `cargo test --all-targets` runs both workloads in test mode without performance sampling.

For comparisons, keep the benchmark, dependency lock, compiler, feature set, profile and machine conditions identical. Save a baseline using `--save-baseline <name>`, then compare using `--baseline <name>` and the same `CRITERION_HOME`. Record absolute time and confidence intervals for loading and execution separately. Criterion's time estimate and change estimate use different statistics; do not derive its reported percentage from displayed slope estimates.

This measures synthetic loading and action execution only. Full tick latency, allocation cost, prepared encounters, collision solving and client performance require separate workloads. Raw benchmark results and runtime data stay local.

## Integration decisions

- Retained the 15-case contract, production-API benchmark and contributor guidance from the local experiment.
- Kept the current `script_init/`, `game_combat/` and `scene_assets/` splits. The superseded local split and older README/architecture snapshots are not applied.
- Deferred the experimental typed action IR. It predates current VM behavior, including nested destinations. Its earlier synthetic comparison measured roughly doubled load cost and about 9% lower execution time; those results do not describe the current branch. Any later IR proposal must preserve current contracts and justify loading, memory and execution tradeoffs against a fresh baseline.
- Follow [current architecture](NATIVE_SERVER_ARCHITECTURE.md), [verification state](STATE.md) and [roadmap](ROADMAP.md); the removed historical tracking documents are not restored.
