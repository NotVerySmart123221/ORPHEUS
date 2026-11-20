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

## Load runtime state classifications

A prepared graph bundle may include `runtime_state_metadata`. The
`plain_classes` list names ordinary runtime state classes. It applies to
nodes with a reciprocal state slot and no explicit behavior override.
VM handlers and their existing guards run before the plain fallback.
For legacy bundles, the loader also records the 104319 inherited base
factory classification for `STU_F08E1453` through the same path.
[Pinned factory research](https://github.com/leafall903/ORPHEUS/blob/c73e5a499ec73d499c97eaae46357af30c9e4b39/docs/ENTITY_CONTROL_STATE.txt#L109-L117)

The `client_states` list identifies instances by graph index, state
slot, RID, and raw class. All four must match. A client classification
skips execution while preserving the raw class, scope flags, and
owner and observer slot tables. A class may have different scopes in
different graphs.

An authored bundle can declare `projected_message_topics` in this metadata.
Each topic is a unique, nonzero literal message ID. The loader requires a
matching static client receiver and rejects a matching server receiver
across all loaded graphs. The declaration reserves the topic in the VM's
message dispatcher. An eligible fixed-self sender with reciprocal state
and remote slots in a networked instance uses the ordinary
state lifecycle and existing `send` codec. Its parameter records stay in
the graph for client use. Server evaluation and delivery are skipped.

```json
{
  "runtime_state_metadata": {
    "projected_message_topics": ["0x02400000000000F1"]
  }
}
```

Verified wire codecs still come from the separate codec table. A plain
classification alone supplies no codec or payload layout. The portable
contract exercises ordinary lifecycle and scope. Native tests verify
owner and observer bytes and missing-codec diagnostics.
See the [synthetic runtime state contract](../fixtures/behavior/script-runtime-state-metadata-v1.json).

Use the offline importer with a local graph bundle and evidence archive
or extracted evidence directory. It writes a separate gzip bundle.
Keep both source data and the generated bundle in your private runtime
workspace.

```text
python scripts/build_runtime_state_metadata.py --graph-bundle path/to/game_graphs_174.json.gz --evidence path/to/classification.zip --output path/to/new/game_graphs_174.json.gz
```

## Build the private evidence reports

Use the graph index, RID, state slot, and raw class as the verified
identifiers for local evidence. Record the input digest and importer
version so two runs can be compared. Keep extracted records private.
Shared tests use original graphs and values.

Use these reports to clear independent actor startup failures first.
Repeat the startup matrix on the same inputs and keep room admission
separate from independent factory results. Then finish the shared
Phase 2 paths and run their regressions before starting Phase 3
ability support.

1. **Field contracts**. Group nodes by capability and class. Record
   field paths, operand types, presence counts, and incompatible shapes.
   Validate candidate contracts against the Rust loader and evaluator.
2. **GameMessage catalog**. Index message IDs, send and receive
   placements, and parameter-to-variable bindings. Record dynamic or
   unresolved routes explicitly.
3. **Timing contracts**. Separate literal values, literal ranges,
   and unknown expressions. Compare observable timer order and frame
   boundaries with a headless trace.
4. **Variable access**. Track read, write, and initialization edges by
   scope. Report missing initializers separately from queries whose
   runtime inputs could supply the value.
5. **Coverage baseline**. Join the graph inventory to a pinned trace.
   Report reached, skipped by scope, unsupported, and unreached nodes
   with enough trace provenance to reproduce the Phase 3 ordering.
6. **Synthetic test scaffolds**. Generate only the verified operand
   shape and lifecycle outline. Use invented values, graphs, IDs and
   expected results derived from the server contract.

### Phase 2 shared VM paths

- The native pitched direction operand reads a bound entity through the
  host pose provider. The existing roll adapter still supplies its
  compatibility vector. A host's explicit no-target reply completes the
  operand before selector evaluation.
- The scene placement collection action uses a literal required tag and
  a scene placement inventory from its host. An empty inventory and an
  unavailable inventory produce distinct results.
- Timed graph states with no completion operands or alternate links use
  parent-owned lifecycle at any placement. The prior configured-duration
  path remains a compatibility case. Configured predicates and alternate
  subgraphs need separate contracts.
- Owner graphs can read `BodyActive` as an activity gate with no hero or
  graph-specific bindings. The prepared scene provides the initial level.
  The room updates the current body by generation.
- Native eye-position operands use the player's simulation eye anchor.
  Native eye and direction expressions with potential foreign selectors
  request a frozen room pose.
- A native local combat accumulator enters as a plain state when its host
  confirms the local service is absent. It leaves spatial operands dormant.
  Hosts that report the service present or return an unknown result still
  produce an unsupported-state diagnostic.

- Player, NPC, mode and gate scripts read native engine settings through
  the mode's authored table or its captured compatibility table.

Native target tracking still needs ordered samples, optional typed
output bindings, and the lifecycle of its embedded state slots. The
authored `track_entities` node owns separate child graph instances.
Start with a validated query-only shape and an empty embedded plug.
Preserve the difference between unavailable queries and empty results,
then verify the output lifetime and owner and observer projections.
The other pending paths include migration of the legacy charge bindings,
health effects, spatial queries, control effects, and voice recipients.

The prepared Python ordinary sender trace takes its compatibility branch.
That branch leaves the persistent recipient wrapper unresolved, so it
supplies no evidence of delivery or registration. Keep the existing
persistent registration diagnostic until host policy is modeled or the
topic has a verified client-only receiver set. Use projected message
topic declarations for the latter case.

### Generate the static reports

```text
python scripts/summarize_statescript_evidence.py --evidence path/to/classification.zip --graph-bundle path/to/game_graphs_174.json.gz --families path/to/capability-families.json --output-dir path/to/new-private-report
```

The output directory holds the four static reports and
`evidence-manifest.json`. The manifest identifies the normalized source
and report version. The command accepts an extracted evidence directory
as well as a ZIP. It requires a new output directory.

In `field-contracts.json`, field shapes from confidently attributable
listing constructors appear under `listing`. Missing rendering stays
distinct from field absence.
The optional prepared bundle adds exact matches by graph, RID, slot,
class, and scope. Its field counts use the matched state population.
Prepared shapes include array-element paths and one example per kind.
Use an optional JSON object under `--families` to group raw classes by
capability. Values are arrays of class names.

The `gamemessage-catalog.json` report groups literal IDs and lists sender and receiver
locations, observed parameter GUIDs, low-16-bit slots, and direct
variable bindings. Dynamic and unresolved endpoints retain their own
records. Treat endpoint pairs as candidate routes. Runtime traces
establish delivery. Reports retain message IDs and binding slots.
Other field values are reduced to structural kinds.

The `timing-contracts.json` report identifies direct wait-state operands
by graph, RID, state slot, and scope. It records finite literals and
ordered literal random bounds. Dynamic and unsupported operands keep
their location and classification. A null unit indicates that the
listing lacks an explicit unit. Use VM traces to verify elapsed time
and event order.
The v3 report generator requires exactly one wait operand before inferring
timing. A malformed multi-argument wait stays unresolved.

The `variable-access.json` report records definite reads and write sites,
schema default declarations, direct message bindings, and uncertain
contexts for each graph. It keeps shared slots separate from instance
slots. For a matching prepared graph, the report also lists instance
slots declared by a recognized public schema. A source candidate means
the listing shows a read and lacks a visible value source. Review the
prepared schema, cross-graph input, and runtime ordering before treating
it as a missing initializer. The report omits source expressions and
default values.

### Collect a headless coverage baseline

Run the existing 900-frame owner test with the `script-coverage` feature
and a local prepared pack that includes target tags. The capture tracks
up to 4,096 distinct positions per component starting at VM initialization.
The live Inspector response remains capped at 128 positions per component.
Set a fresh output path and record the full revision used for the run.

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\private\runtime'
$env:ORPHEUS_SCRIPT_COVERAGE = 'C:\private\owner-coverage.json'
$env:ORPHEUS_SCRIPT_COVERAGE_REVISION = git rev-parse HEAD
cargo test --locked -p orpheus --features script-coverage --lib stock_soldier_owner_session_idles_fires_reloads_and_sprints -- --ignored
```

Join observations with the exact graph bundle and a local hero catalog
to identify graph position, RID, state slot, and class.

```text
python scripts/summarize_statescript_coverage.py --graph-bundle path/to/game_graphs_174.json.gz --hero-catalog path/to/game_heroes_174.json --observation path/to/owner-coverage.json --output path/to/new-coverage.json
```

The input catalog supplies direct body roots. The report follows declared
`track_entities` child graph links from those roots and any observed
graphs. `static_dependencies` lists those references and their source
nodes. `missing_dependency_graphs` identifies absent targets. A graph
with origin `referenced_child` has a static reference and no observed
site in that scenario. Dynamic graph choices need a capture. Use repeated
`--observation` options for separate runs. With no observation, the report
marks inventory nodes as having no sampled scenario. The report lists each
node's declared
`scope` and effective `server_scope`. Its `runtime_classification` identifies
an exact client state or a candidate for plain lifecycle fallback. Review
scope skips, registration rejections, and unsupported sites separately.
An observed target passed the VM scope check. Missing rows in a capped
observation have unknown coverage. The scripted inputs exercise one hero
and one path. Use scenario-specific captures to prioritize other paths.

### Inspect actor startup readiness

Use the same local prepared pack to check startup across the actor catalog.
The report records available prerequisites, ordinary room admission, and
an independent attempt through the production body factory. Each row
describes setup and initial diagnostics. Ability inputs, subsequent ticks,
and client presentation require their own scenarios.

```powershell
$env:ORPHEUS_RUNTIME_DIR = 'C:\private\runtime'
$env:ORPHEUS_ACTOR_STARTUP_REPORT = 'C:\private\actor-startup.json'
$env:ORPHEUS_ACTOR_STARTUP_REVISION = git rev-parse HEAD
cargo test --locked -p orpheus --features script-coverage --lib prepared_practice_actor_startup_matrix -- --ignored
```

Use a new private output path for each run. The report records input
digests and the source revision, and leaves existing output files intact.
In schema version 2, a factory rejection also records its failure stage.
This distinguishes scene prerequisites, root layout, VM startup steps,
snapshot projection, and full snapshot encoding. An incomplete projection
lists a bounded set of typed diagnostics with graph and node positions.
An unavailable entity member read reports `entity_variable_unavailable`.
The row carries the script site and omits the entity ID and variable key.
For an unsupported config diagnostic, `operand_class` records the
bounded identifier from the evaluator when available. The containing
node class remains a separate field. A class identifier shows where
the evaluator attributed the error, including shape and context failures.
The independent factory probe retains these details when the ordinary
room admission reports a generic unsupported error.

An owned-entity count with a valid definition and an explicitly unbound
owner leaves the caller's initial value intact. A bound owner requires a
count supplied by its host. A known empty registry yields integer zero.
An unavailable registry still requires a diagnostic.

A state-managed recipient set reports `unsupported_message_registration`
when its persistent registration path is unavailable. The probe keeps
the failure blocking and records the sender node. The ordinary one-shot
send path remains separate from registration of a persistent sender.

The 49-site prepared review found eight fixed-self senders across two
message topics, including the sole startup blocker for Orisa and Sigma.
The prepared bundle and listing audit found ten matching receivers
marked client-only. The actor graph closures still contain unresolved
references, and the wider bundle contains unresolved server-side message
selectors. The declared projection contract has synthetic coverage for
the fixed-self sender shape and preserves parameter bindings. The existing
prepared pack still reports the eight registration diagnostics.

The report's `probe_roster` declares the synthetic admitted player slots
used by both attempts. In a live room, the player host reads the admitted
roster at startup and refreshes it on departure. The count includes members
whose body is unspawned or dead and excludes NPCs. With a known roster, an
empty team yields zero. Asset-only factory preparation has no room roster.
The roster rule follows the existing Python ready-switch adapter at
`c73e5a49` in `src/ow174/game/match.py`.

### Compare a prepared startup matrix with Python

Run the comparison with a clean Python checkout and its full commit SHA.
Use the same runtime and the Rust report generated above.

```powershell
$pythonReference = 'C:\work\orpheus-python'
$pythonRevision = git -C $pythonReference rev-parse HEAD
python -I -S -B scripts/compare_actor_startup.py --python-reference $pythonReference --python-revision $pythonRevision --runtime $env:ORPHEUS_RUNTIME_DIR --rust-report 'C:\private\actor-startup.json' --private-output 'C:\private\actor-startup-comparison.json'
```

The comparison verifies the SHA-256 of all eight inputs from the Rust
matrix and matches the exact hero and body IDs. Python uses the ordinary
room and body switch with the existing in-memory client. It decodes the
initial full tree and the first owner frame after acknowledging the tree.
The report records the Python strict flag and counts distinct compatibility
notices observed per actor. Silent compatibility paths require separate
observation. Rust retains its recorded independent factory result and
its separate room factory result. This comparison measures startup
milestones. Frame parity, gameplay, and original-client acceptance have
separate tests.

### Generate a synthetic test scaffold

```text
python scripts/generate_statescript_scaffold.py --field-contracts path/to/field-contracts.json --output path/to/new-scaffold.json
```

The scaffold contains an original server-only plain state and example
fields for allowlisted operand kinds. It uses invented identifiers,
values and expected behavior. Use `--family` or `--class` to select
operand kinds for an exact family or class in the private report. The
selected names remain private. Operand examples are inert. Review the
draft, add the behavior to verify, and register it with a Rust contract
consumer before counting it as a regression test.

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

## Lock a logical button for a state

Set `"state_behavior": "button_lock"` on a server-only state, or declare
the `button_lock_state` behavior for an authored class. Set `fields.button`
to a logical button from 1 through 255. A positive integer
`fields.duration_frames` finishes the state after that many VM frames.
Omit the duration to let another graph action end or abort the state.

Locks on the same button stack. The VM releases each claim when its state
finishes, aborts, or is destroyed. Acquisition discards buffered ability
presses for that button. Logical listeners resynchronize against the
held input as counts change, and their existing bypass applies.
This operates on VM input. Room movement and the native whole-body input
gate use their own controls.

A directed room-message entry can begin the lock in the recipient's VM.
The lock takes effect when its begin runs. Commands and state transitions
continue to follow the component's event order.

See the [synthetic button lock contract](../fixtures/behavior/script-button-lock-v1.json).

## Scale movement speed for a state

Set `"state_behavior": "movement_scale"` on a server-only state, or declare
`movement_scale_state` for an authored class. Set `fields.factor` to a
finite number from zero through one. An optional positive integer
`fields.duration_frames` finishes the state on the VM timeline. With
no duration, the state stays active until it ends, aborts, or its instance
is destroyed.

The state adds one multiplier in the existing speed slot. Overlapping
scales multiply under the normal modifier order. Each claim belongs to
its own instance and state. Movement consumers see additions and
removals at their next frame boundary. Player movement and scripted AI
gate approaches read this slot. A factor of zero scales the speed limit
to zero. Acceleration, momentum and collision still follow their mover
rules.

See the [synthetic movement scale contract](../fixtures/behavior/script-movement-scale-v1.json).

## Use priorities for control effects

A server-only state can declare `"state_behavior": "stack_claim"`, or use
an authored class with `stack_claim_state`. Set `fields.variable` to an
object with `scope` set to `"entity"` or `"instance"` and a nonzero integer
`id`. Entity scope is shared by graph instances on the same body.
Instance scope is local to the running graph.

Set `fields.priority` to an object containing an integer `level` from
-128 through 127 and a boolean `above_equals`. A higher level wins. An
incoming claim at the same level wins only if it declares
`above_equals: true`. Set `fields.value` to the value expression that the
claim contributes. A positive `fields.duration_frames` schedules the
claim's end. Claims also release on abort or instance destruction.
Maps have no supported stack snapshot representation. A map result
produces a diagnostic and leaves the current binding untouched.

Use `{"$":"StackAllows","variable":...,"priority":...}` with the same
variable and priority objects in an authored branch. It checks whether
the new claim would lead the current stack at the point of evaluation.
For a directed room message, run the check in the recipient's graph.
The predicate reads the stack and leaves it untouched. An unavailable
context produces a diagnostic.

Use the existing `m_topSubgraphPlug` and `m_underSubgraphPlug` outputs to
start effects at the corresponding stack position. Each output's
`m_FF3DAF1E` array lists the state slots it owns, so the VM can end those
states when the claim changes position or ends.

See the [synthetic stack claim contract](../fixtures/behavior/script-stack-claim-v1.json).

The existing timed projectile bridge shares one native compatibility binding
for the shot, wrapper, duration override, and NPC guards. Recipient scripts
queue the existing wrapper and hit notification in order. The room can
suppress its native control timer while the graph still handles the
notification.

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

## Send a cue to selected players

A server-only action can declare `"action_behavior": "room_cue"` or use
an authored class with `room_cue_action`. Supply literal, nonzero
`fields.stimulus` and `fields.line` identifiers in canonical 16-digit hex
form, an `audience` expression that returns an array of entity references,
and a valid `m_outPlug`. A graph can build the audience with
`EntitiesInRadius` and its team, liveness, and tag filters.

The VM keeps the first occurrence of each entity and accepts at most 64
distinct entries. An empty audience continues immediately. For other
audiences the room keeps current living player bodies and ignores other
entities. The batch uses the sender's existing request budget. The action
continues once the batch is accepted.

The notice carries the sender as its anchor and speaker. Before reliable
delivery, the transport checks both body generations and waits for both
identity acknowledgements. Each player has at most one unacknowledged
authored cue in reliable delivery. When the pending combat queue is full,
a new cue is dropped. Cue identifiers describe resources supplied by the
author.

See the [synthetic room cue contract](../fixtures/behavior/script-room-cue-v1.json).

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

## Set mode and actor rules

The optional `mode_rulesets` section of a version 1 graph bundle supplies
immutable rules for the running mode. This example uses synthetic IDs.

```json
{
  "mode_rulesets": [
    {
      "mode": "0x00000000000000A1",
      "rules": [
        {"id": 7, "value": false},
        {"id": 7, "body": "0x00000000000000B1", "value": true},
        {"id": 8, "value": 1.0}
      ],
      "engine_settings": [
        {"id": 7, "value": 4}
      ]
    }
  ]
}
```

Use a nonzero canonical GUID for the mode and any `body` override. The
body key is the actor's profile GUID, shared by the player and AI host
lookups. A rule ID is a 32-bit unsigned integer. Values accept Boolean,
signed 64-bit integer, finite floating-point, or explicit `null`. A mode
must appear once, and each rule ID must be unique within its scope.

The VM checks its existing entity variable first when it has a value.
The native host then looks for a body override, a mode default, and an
existing compatibility value in that order. An explicit `null` is an
authored result and prevents compatibility fallback. A fully missing
rule retains the VM's existing integer-zero fallback. Graph bundles
without the optional section retain the existing host rules.

A mode row can also declare `engine_settings`. This is the separate table
used by native engine-setting expressions and watchers. Keys are unsigned
32-bit hashes. Values use the same scalar types as rules. The table is
mode-wide, with unique keys. Player, NPC, mode and
scene-object scripts read the table for their current mode.

An omitted table keeps the existing mode-specific compatibility lookup.
An authored table, including an empty list, supplies the whole mode table.
A missing key in that table keeps the expression's initial value, while
an explicit `null` supplies a null result. A row may declare engine settings
without declaring body rules.

See the [synthetic mode ruleset contract](../fixtures/behavior/script-mode-rulesets-v1.json).

## Receive a settled health event

A graph can subscribe to room-authoritative health changes with an
`EntryHostEvent` node declared as `"host_event_entry": "health_applied"`
and `"server_only": true`. Bind the known parameter slots in `m_params`
and continue from `m_outPlug`.

| Slot | Value |
| --- | --- |
| 1 | Source entity, or null if unavailable |
| 2 | Target entity at settlement |
| 3 | Actual signed pool delta, positive for healing and negative for damage |
| 4 | Whether damage killed the target |

The enabled room records successful weapon damage, room-settled script
and AI requests, field healing, and VM-local AI damage and healing.
Pool mitigation and caps are already reflected in the delta. Changes
that apply zero produce no event. For overlapping pulses from one owner,
the record carries the actual total healed per target. Pool capacity,
respawn, and shield recharge have independent lifecycles.
The room checks each recipient's body identity after settlement and
schedules delivery at the start of that component's next VM frame.
Living source and target actors can subscribe. A self-effect is delivered
once. A killed target can still be reported to the living source.

Host entries use the bounded VM event queue and parameter binding. Their
subscription is distinct from local and directed room messages. Queue
rejection discards the notification and leaves the settled pool result
in place. The same room queue delivers VM-local outcomes after the
corresponding simulation phase.

See the [synthetic host health contract](../fixtures/behavior/script-host-health-events-v1.json).

## Attach a server-only graph to a body

Use a body's optional `server_graphs` list to install an authored
graph as an isolated root. Each entry uses the same graph index and
sync bindings as a `graphs` entry. Sync bindings seed instance variables
before the root's startup entries run.

The VM installs numbered roots first. Private roots and the scripts they
start within the same component allocate IDs from the top of the
instance range. The public sequence grows from the low end. The VM
reports exhaustion when the ranges run out of free IDs and keeps retired
IDs reserved for the component lifetime. Isolated instances stay outside owner and
observer projection.

Use instance-scoped variables for private balances. A graph that writes
shared entity variables or requests room effects still participates in
those shared interfaces. Compare both wire views when adding a binding
to a client-facing body.

See the [synthetic root allocation contract](../fixtures/behavior/script-server-root-ids-v1.json).

## Compose a graph-owned resource meter

Bind the same meter graph to the actors that use it. Each body's
graph-binding sync list can seed its instance variables for the starting
balance, limit and gain rates. Instance variables keep the configured
balance local to that graph instance.

A `health_applied` entry can award credit after checking that the actor is
the source. Use the signed delta and `EntityMatches` with the room's
affiliation data to route damage and healing. An interval state can update
the same balance independently. Ordinary assignment and numeric clamp
operations apply the limit.

Use `{"$":"BodyActive"}` in the gain branches when the meter depends on
room and body activity. A live owner in a regular room is active. An
encounter supplies its existing pre-result activity level after its
runtime is installed. Death closes the gate through authoritative vitals.
A detached body with no supplied room
activity reports an unavailable host read. A deferred health event reads
the level when its graph handler executes. Gating interval gain keeps
the interval schedule anchored to startup. The debug scheduler can
freeze the VM clock independently.

The [synthetic meter contract](../fixtures/behavior/script-authored-meter-v1.json)
runs this graph for two differently configured body definitions and checks
an actor without the binding. Its rates, limits and test actors are
original examples. The fixture verifies execution and scoped balances.
The [activity contract](../fixtures/behavior/script-meter-body-activity-v1.json)
checks when gains stop, resume, or belong to a replacement actor.
Adding a root to a client-facing body also requires an owner and observer
projection regression, since body graph bindings participate in instance
ordering. The existing charge bridge uses synchronous damage delivery.

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

In an assignment or subscript binding, a fallback can supply a config
group. The selector flattens the selected group's values in authored
order. An entry with no fallback contributes no value inside a group.

## Read an entity's position or direction

Use `EntityPosition` or `EntityDirection` as the `$` tag of an authored
operand with one `entity` field. The field must evaluate to a bound
entity reference. On each evaluation, the VM reads the position or
forward vector from the host and returns its finite components verbatim.
A missing or nonfinite response is an unavailable-entity query error.
The origin is a valid position.

In native room scripts, an actor reads its own live pose, including
before the room has sampled it. Other living actors use the room's
existing frozen spatial frame. Changes to those actors appear in the
next sample. The frame keeps position and packed aim together, and
the host derives forward vectors on demand. A server operand requests
this frame without requiring tag data. Static anchors remain separate
from living body observations.

Player startup reads the room's frozen snapshot when available.
Replacement views exclude the previous body by its owner at capture time.
An owner can replace the body repeatedly in one room frame while other
actors keep their frozen poses and facts. The new body joins the room
sample after admission. The factory probe captures the prepared scene
before starting its temporary runtime.

The [synthetic spatial contract](../fixtures/behavior/statescript-entity-spatial-v1.json)
shows both operands applied to the same entity at two points in time.
The [room pose contract](../fixtures/behavior/script-room-actor-pose-v1.json)
checks how the native room captures actor movement, aim, and player eye
anchors.

The native eye-position shape uses `STU_21A13145` with one `m_entity`
selector. A player anchor combines feet position, the existing stance
height, and the mover's spring offset in movement units. It is seeded
with the spawn pose and refreshed on each owner advance. Other bodies
read the same frozen sample as their pose. NPC bodies and static
placements have no player eye anchor. The VM reports an unavailable
anchor when the host returns none.

The loader requests a spatial frame for native eye and direction
expressions with selectors that may refer to other entities. Direct
self reads and client-only operands without a server fallback do not
request the frame. See the [pose demand contract](../fixtures/behavior/script-native-pose-demand-v1.json).

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

## Check whether a segment is clear

Use `SegmentClear` as the `$` tag with `origin` and `target` expressions.
Each expression must return a finite three-component vector. The VM
evaluates the origin first, evaluates the target second, and spends one
work unit on the geometry request. The result is a Boolean for the
prepared room's static collision scene. The target endpoint can touch
the scene. An obstruction strictly before it returns false.

Player and AI scripts use the same geometry as
`EntitiesInRadius.line_of_sight`. A room with no available collision
sample produces an unavailable visibility error. An `observe` state
can retry on the next frame and keep its previous successful value.
The loader enables spatial queries for this expression in server
fields and in a `ClientOnly` server fallback. Client-only values do
not require the service.

See the [synthetic segment contract](../fixtures/behavior/statescript-segment-clear-v1.json).

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
