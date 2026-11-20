# StateScript actions: development and measurement

Use the current `rust` module boundaries when extending the VM. This guide integrates useful verification work from the older local foundation experiment without replacing the subsequently merged implementation.

## Execution scope

The VM uses each node's declared execution scope for graph traversal, startup
and message scheduling, and direct dispatch. A client-only node does not run
server actions or start server states. Server-only nodes can execute on the
server while remaining absent from network projection.

Graph IDs within a bundle and node positions within a graph must be unique.
Repeated references in legacy entry, state and synchronization tables
keep their original order. Queued messages check the resolved node's
execution scope before binding parameters or following its output.

## Declare an ordinary server state

An authored node can opt into the plain state lifecycle with
`"state_behavior": "plain"`. Set `server_only` to true and `client_only`
to false. Its `state` must index a slot in the graph's `states` table that
points back to the node exactly once. The loader checks the link shape
of the optional begin, end, abort and finished output plugs. Omit a plug
or set it to null when it is unused. The loader assigns the common plain
dispatch class, so graphs can use the ordinary lifecycle without class or
position registration. These nodes remain outside the network projection.

## Declare a forwarding action

Set `"action_behavior": "forward"` on an authored server-only action whose
job is to follow its `m_outPlug`. Supply a plug with a link list and leave the
action out of the state table. Links run in their declared order with the
regular VM budgets. An empty plug does not take a transition. The action
requires no registration by class or graph position.

## Route an authored action by a condition

Set `"action_behavior": "branch"` on a server-only action, or declare
`"branch_action"` for an authored class. Supply `m_condition` and valid
`m_truePlug` and `m_falsePlug` outputs. The VM evaluates the condition
once at dispatch and uses the ordinary expression truthiness rules. It
follows the selected output in link order. An empty selected plug takes
no transition. Evaluation errors are reported at the node and take no
output. Use a client-only operand with a server fallback when an
authored condition needs a different value on the server.

## Share class behavior across a graph bundle

Add `authored_classes` to a version 1 bundle and use those names in the
`class` field of any node in the bundle.

```json
{
  "authored_classes": [
    {"class": "authored.latch", "behavior": "plain_state"},
    {"class": "authored.continue", "behavior": "forward_action"},
    {"class": "authored.indicator", "behavior": "client_only"}
  ]
}
```

Each name starts with `authored.` and has a nonempty suffix of ASCII
letters, digits, underscores or hyphens. Duplicate names are rejected.
The scope flags are still required. Set both to false to inherit a
declared class's scope, or set only the matching flag to true. Authored
states and actions inherit server-only scope. Client-only
nodes skip server execution and wire projection. Conflicting flags or
per-node behavior fail at load time with the graph and node positions.

The loader applies these declarations before checking state slots and
output plugs and building the wire tables. Nodes outside the catalog
follow the ordinary rules. The
[synthetic class contract](../fixtures/behavior/script-authored-classes-v1.json)
exercises the same declarations across two graphs.

## Send a message within a component

Set `action_behavior` to `"local_message"` on a server-only action, or
declare `"local_message_action"` for an authored class. Supply a nonzero
canonical message ID in `m_gameMessage.m_gameMessage`, an output plug,
and optionally `m_params` entries with `m_B5051BCE` and `m_value`.
Parameter IDs must resolve to distinct nonzero low-16-bit slots.
Leave `m_target` and `m_targets` unset.

The action uses the component's existing message queue and delivery rules.
It captures parameter values when the action runs and follows its output
plug. Message entries are indexed at graph load in their original order.
State listeners still need to be active when the message is queued and
delivered. Client-only listeners remain outside the server route.
See the [synthetic local message contract](../fixtures/behavior/script-local-message-v1.json).

## Send an authored message to another room body

Set `action_behavior` to `"room_message"` on a server-only action, or
declare `"room_message_action"` for an authored class. Supply a nonzero
message ID in `m_gameMessage.m_gameMessage`, an `m_target` expression that
yields a different live body, and a valid `m_outPlug`. Use up to 16 distinct
nonzero parameter slots in `m_params`. Parameters can hold scalar values or
finite vectors.

A receiving game-message entry declares `"room_message_entry": true` and
server-only scope. That declaration puts the entry on the authored room route.
The sender captures parameter values when the request is accepted and can
then follow its output. The room uses the same per-body FIFO and identity
checks as health requests, with a combined limit of 64 pending requests per
body. It schedules delivery for the receiving body's next VM frame.
An accepted message can expire if the body is replaced or retires, or if
the recipient has no matching listener. A receiver with too little queue
space rejects the whole batch of matching listeners.

See the [synthetic room message contract](../fixtures/behavior/script-room-message-v1.json).

## Request a health change

Set `action_behavior` to `"health_change"` on a server-only action, or
declare `"health_change_action"` for an authored class. Supply a `target`
expression yielding a bound entity reference, a `delta` expression yielding
a finite nonzero number, and a valid `m_outPlug`. Positive deltas heal and
negative deltas deal damage through the room's existing health rules.

Run this action from events after the body receives its first room frame.
During component initialization, the host can lack a room frame and reject
the request. Admission requires a living target with health pools in that
frame and free space in the source body's queue. The output runs when the host accepts it.
The room settles accepted requests before retirement and checks the target's
generation again, so an accepted request can expire before it changes health.
The queue belongs to the source runtime and is dropped if that runtime is
retired before settlement. Effects follow queued order within each source body. Death and health updates
use the existing room lifecycle. These authored actions also enable the room
body snapshot in bundles that use no spatial expressions.

See the [synthetic health contract](../fixtures/behavior/script-health-change-v1.json).

## Apply a radial health change

Set `action_behavior` to `"radial_health"` on a server-only action, or
declare `"radial_health_action"` for an authored class. Supply an `area`
object using the ordinary `EntitiesInRadius` expression, a finite nonzero
numeric `delta` expression, and a valid `m_outPlug`. Team and tag filters,
cones and static line of sight follow the same query rules. Distances
come from the room's position sample used for membership.

An optional numeric `edge_scale` expression ranges from zero to one
and defaults to one. The action scales `delta` linearly from its full
value at the center to the specified fraction at the edge. A zero-radius
query applies the full value at the center. A target contributes at most
one change, and zero changes use no request slots.

The room admits the whole ordered batch against the body's shared request
limit. The output continues after acceptance or an empty selection.
Each target's binding is checked again when the room settles its request.
A target that retires before settlement can expire independently of the
other requests in that batch.

See the [synthetic radial health contract](../fixtures/behavior/script-radial-health-v1.json).

## Run an output at a frame interval

Set `state_behavior` to `"interval"` on a server-only state, or
declare `"interval_state"` for an authored class. Provide a positive
integer `interval_frames` within the `u32` range and a valid
`on_interval` output plug. Connect this output to a health change action
to apply a periodic effect. The action reads its target and amount at
each pulse.

The first pulse is due one period after activation. The ordinary VM timer
queue runs the output once when a frame catches up to a due pulse. Missed
pulses are dropped and the next deadline keeps the activation's cadence.
Ending the state cancels its pending timer. Reactivation starts a new
cadence. See the [synthetic interval and health contract](../fixtures/behavior/script-interval-health-v1.json).

## Track entities with child graphs

Set `state_behavior` to `"track_entities"` on a server-only state, or
declare `"track_entities_state"` for an authored class. Supply a
`targets` expression yielding a flat array of entity references, a
`child_graph` index in the same bundle, and a positive `target_variable`
ID. Each selected entity receives one child instance. Its local target
variable is available before the child runs its startup entries.

The tracker samples once per VM frame after activation and accepts up
to 64 unique targets. It deduplicates each valid sample and opens new
children in ascending entity ID order. Membership is keyed by VM entity ID.
A retained reference keeps its child. A later sample can create a new child
after removing that reference. Unavailable or invalid samples leave the
previous set in place.
A successful empty result removes all children. Removing an entity or
ending the tracker tears down the corresponding child, scoped descendants
and timers. The tracker and its child instances execute on the server.

See the [synthetic target child contract](../fixtures/behavior/script-target-children-v1.json).

## Declare a client-only operand

Use `{"$": "ClientOnly", "value": ...}` for an authored operand that the
server must skip. Its `value` is opaque to the server. An optional
`server_fallback` is evaluated in the same scalar or ordinary context as
the enclosing operand. Without a fallback, the evaluator retains the
caller's initial value. Wrappers still consume work and nesting budget.
Ordinary unknown operands continue to produce diagnostics.

## Read an entity's position or direction

Use `EntityPosition` or `EntityDirection` as the `$` tag of an authored
operand with one `entity` field. The field must evaluate to a bound
entity reference. The VM reads the current position or forward vector
from the host on each evaluation and returns its finite components
verbatim. A missing or nonfinite response is an unavailable-entity
query error. The origin is a valid position. These reads use the
ordinary expression budget and the host's existing data sources.

The [synthetic spatial contract](../fixtures/behavior/statescript-entity-spatial-v1.json)
shows both operands applied to the same entity at two points in time.

## Find room entities within a radius

Use `EntitiesInRadius` as the `$` tag, with a `center` expression
that returns a finite vector and a `radius` expression that returns a
finite nonnegative number. The result is an array of unique entity
references ordered by ID. The boundary is inclusive in three dimensions.
A query spends expression budget on each examined candidate and yields an
error if it runs out of budget or the room sample is unavailable. An
available sample can return an empty array.

Prepared rooms enable the candidate service when a server graph contains
this expression. Player and AI scripts share the living body positions
captured at the first input or simulation work of the room frame. Position
and membership updates appear in the following frame's sample. The sample
contains player and bot bodies and is supplied during room runtime events.
A component initialized before it joins the room has no candidate sample.

To filter by team, add a `filter` object with a `relation` of `ally` or
`opponent` and a `source` expression that yields a living entity reference.
The query finds that entity's affiliation in the same snapshot, even when
the source lies beyond the radius. Both relations exclude the source.
Allies share an assigned team. Independents, including free-for-all
participants, are opponents of every other entity. A missing source or
conflicting snapshots for its affiliation yield an unavailable query.
Omit `filter` to include every entity in range.

Add a `cone` object to restrict the same query by direction. Its `direction`
expression must yield a finite nonzero vector, and `half_angle_degrees`
must yield a number from 0 to 180. A cone includes its angular boundary and
excludes bodies exactly at its center because they have no bearing.
Use `EntityDirection` to read a direction supplied by the host. A cone
can be combined with a team filter and uses the same candidate budget.

Set `"line_of_sight": true` to require a clear segment from the query
center to each remaining body position. The query runs through the prepared
room's static collision scene after applying radius, cone, and team
conditions. Bodies use the same snapshot and geometry for both player and
AI scripts. Each surviving entity spends one extra work unit for visibility.
An unavailable segment or conflicting candidate positions returns a
visibility error and leaves the observation's previous value in place.

See the [synthetic radius contract](../fixtures/behavior/statescript-entities-in-radius-v1.json).

## Match a room entity against authored conditions

Use `EntityMatches` as the `$` tag and supply an `entity` expression
that yields a bound entity reference. An optional `filter` can specify
`alive`, a team `relation` with its `source` expression, and `tags`.
A known entity that satisfies the configured conditions returns true.
An entity absent from an available room snapshot returns false once the
filter source, if supplied, is resolved. An `alive` condition can select
either living bodies or registered dead bodies in that snapshot.

Tag conditions are `all`, `any`, and `none`. Each takes a list of
nonzero hexadecimal tag IDs. Empty lists impose no condition. A known
mismatch rejects the entity. An unresolved condition returns an unavailable
query instead of assuming a tag is missing. Use the same conditions in
`EntitiesInRadius.filter` to narrow its living candidates before visibility
checks. A relative radius query still needs a living source. Both
expressions use the same affiliation rule and room frame.

The room collects body metadata when a server graph uses these conditions.
It copies tag data only for graphs that request tag checks. Player and AI
scripts read the same snapshot, including tag grants and suppressions
that were active when that frame began. An observation can
retry on the next frame and keep its previous result while facts are
unavailable.

See the [synthetic entity filter contract](../fixtures/behavior/statescript-room-entity-filter-v1.json).

## Observe an expression across frames

Declare `"state_behavior": "observe"` on a server-only state or use
`"observe_state"` in the bundle class catalog. Give it a `value`
expression and a writable `output` variable. An optional `on_change`
plug follows the first successful sample and each changed sample after it.
The state writes the current value before following the plug.

Sampling starts in the VM frame after activation and runs at most once
per VM frame. That lets a body initialize before the room publishes its
candidate snapshot. Unavailable scene data records an observation diagnostic
and preserves the last successful result without failing the runtime.
The state retries on the next frame. Invalid expressions still report
regular evaluation errors. Finish and abort stop polling and leave the last written value
available. The ordinary lifecycle plugs can clear it when needed.

Use `EntitiesInRadius` as the value to follow membership of an area.
The [synthetic observation contract](../fixtures/behavior/script-observe-radius-v1.json)
exercises entry, departure, recovery, and state lifetime.

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

Startup also counts nodes declared client-only and nodes explicitly
declared as authored plain states. Those counts include unreached nodes.
The live Inspector reports active plain states under `active_states` and
separates known registration rejections from other unsupported nodes. The first 16
rejected graph and node locations appear in `registration_rejection_sites`.
The registration category covers explicit placement checks in action and
state dispatch and the shared guarded state registration path. An accepted
fallback produces no rejection. When dispatch is unresolved, a rejection
means all evaluated registration checks denied the node. If an explicit
placement passes but a separate field check leaves the node unhandled,
the fallback still reports it as unsupported.
A node's absence from a snapshot gives no evidence that execution reached it.
The Inspector VM summary displays counts by category and the bounded list
of rejected graph and node locations.

## Compare pseudocode with VM diagnostics

In the 104319 pseudocode listings, use the numeric suffix of a State label
to look up the decoded node's `rid`. The number after `state` in the header is
the state slot, and Inspector registration rejections report `pos`. Join on
graph ID and `rid`, confirm the class and state slot, then read `pos` to locate
the registration check. If the prepared graph has no matching node, record
the missing match before interpreting the listing.

Pseudocode shows the authored fields and links. An exporter annotation such as
`plain State` describes its interpretation of the node. Check Rust dispatch
and runtime evidence before drawing conclusions about the node's effects.

## Inspect observed node targets

Inspector builds collect up to 128 distinct graph and node positions per
script component. The VM panel shows the count and flags any observations
omitted after the list fills. The existing `/api/debug/v1/state` response includes each component's
`node_observations` with the positions and the `client_scope_skipped` flag.
A false flag means the target passed the server scope check. Other
diagnostics can still describe why its dispatch failed.

This records resolved link targets, action dispatch, state starts and
delivery of queued messages. It does not record every callback or
candidates filtered before those routes. An absent position was not
observed at the instrumented points, and a capped list can also omit later
positions. The observations are local to the component and copied with a
cloned VM snapshot. Default builds do not collect them.

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
