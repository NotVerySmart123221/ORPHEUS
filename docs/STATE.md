# Verification & Parity State (`rust`)

Tracks implemented capabilities, Python-vs-Rust parity, verified baselines, and remaining gaps on `rust` targeting **Overwatch 1.74.0.0.104319**. For crate boundaries and contribution rules, see [NATIVE_SERVER_ARCHITECTURE.md](NATIVE_SERVER_ARCHITECTURE.md); for open milestones, see [ROADMAP.md](ROADMAP.md).

The [pinned Python → Rust parity matrix](https://github.com/leafall903/ORPHEUS/blob/232486a430b11cb105585ec5c0bfa6c47ee2a6d7/docs/PARITY.md) records 24 behaviors, their evidence and the next acceptance check for each. Use that revision to reproduce a comparison. For later results record the implementation, commit, runtime and kind of evidence, as Python on `main` and Rust on `rust` can both advance. Earlier Python observations remain available at revision `9f8f3bcb`.

---

## Public CI & Workspace Coverage (No Game Data Required)

| Lane / Suite | Scope Verified in CI (`.github/workflows/public-checks.yml`) |
| --- | --- |
| **Portable crates (Linux)** | `orpheus-wire`, `orpheus-collision`, `orpheus-runtime`, and `orpheus-statescript` compile and pass isolated tests without server/ECS deps (`cargo test --locked --all-targets -p orpheus-collision -p orpheus-runtime -p orpheus-statescript -p orpheus-wire`). |
| **Root workspace (`default` / `inspector`)** | `rustfmt --check`, zero-warning `clippy`, root library `438` (`319` public / `119` `#[ignore]`) tests in `default`, `444` (`325` public / `119` `#[ignore]`) tests in `--features inspector`, and `orpheus-statescript` `144` (`131` public / `13` `#[ignore]`) tests, plus MSRV `1.85.0` check. Workspace members and integration targets bring public totals to `571` / `577`; the synthetic action benchmark also validates both workloads in test mode. |
| **Cross-language contracts (`shared-behavior-contracts`)** | Checks out pinned `main` (`python-reference`) and verifies identical outputs across `jam-framing-v1.json`, `world-queries-v1.json`, and `profile-settings-v1.json`, `12` public Python tests, and `38` exact StateScript VM contracts on `rust`. |
| **C++ gallery preview policy** | `scripts/check_gallery_preview_policy.py` compiles `relay/gallery_preview_policy.h` and checks `18` resource-selection + `6` owned-tree pruning cases against `gallery-preview-policy-v1.json`. |

[#61](https://github.com/leafall903/ORPHEUS/issues/61) is complete. [#79](https://github.com/leafall903/ORPHEUS/issues/79) records the four Rust consumers and [#80](https://github.com/leafall903/ORPHEUS/issues/80) records the Python StateScript consumers.

---

## Implementation and prepared-runtime observations

Local integration check (2026-11-16): rifle/reload and controller/training-bot startup prepared tests passed with the target-tagged base pack, as did runtime preflight and headless startup. The longer `scene_assets::tests::stock_soldier_owner_session_idles_fires_reloads_and_sprints` test failed its fixed-instance assertion: instance `24` contained graph `0x0F82`, not `0x05AD`. The message table was present elsewhere; diagnostic inspection also found switch instance `19` versus expected `18`. At that revision the fixed instance assertion prevented the later frame checks from running.

Prepared follow-up (2026-11-18, [#72](https://github.com/leafall903/ORPHEUS/issues/72)). On Rust base `a8f5b06b` and the tagged base pack `b11115f3`, the test now resolves each required graph by its authored root or exact parent and attachment slot, then follows that instance for all 900 frames. The table, omitted owner switches, rifle ammo, reload, idle, sprint and both projections pass. Selecting the adjacent attachment slot for the message table fails at startup.

Python `45044d0d` and Rust share 24 startup graph paths on this pack. Two storage entries differ. Both VMs stop root `08B6` during startup. Python retains its empty stopped record and Rust removes it. In manager `004B`, node 21 checks an excluded target tag. The Rust owner lacks that tag and activates child `01CB` on slot 2. The Python host falls back to false for this filter shape and leaves that slot empty. These observations explain the different instance numbering. Whether the original client expects either difference in the replicated startup state remains open.

Follow-up (2026-11-18): on the same prepared runtime, the long test passes using unique graph and parent/attachment identities; a separate test pins the ordered live startup tree and the absent IDs 9 and 26. The additional `01CB` child at `004B` attachment 2 precedes `0BA4` and `05AD`. A test reader decodes descriptor sections of the actual owner/observer baseline and deltas and checks live parent attachments against VM snapshots; synthetic tests exercise removal, lost introductions, cumulative descriptor repetition and rejection of deltas that skip an unacknowledged chunk. The long test does not inspect queue retransmissions. The reader does not decode state links, variables, or the remaining payload, and it does not establish startup creation/retirement events or original-client compatibility. The earlier fixed-ID failure remains a historical observation, not a current test result.

The 2026-11-20 headless actor matrix passed independent startup for **10 of 32** actors at `f506d762`. Two runs on the same 104319 pack produced identical reports. Hanzo was the newly passing actor. The ordinary Practice Range factory attached a player script for one actor. Body creation is a separate operation. The remaining 22 independent probes reported 55 blocking sites, including 16 unsupported config expressions, 14 registration rejections, 11 unsupported states, and eight persistent message registration failures. Client acceptance and ability coverage have separate gates.

The [prepared Python comparison](STATESCRIPT_DEVELOPMENT.md#compare-a-prepared-startup-matrix-with-python) ran Python `c73e5a49` against the same eight input digests. The ordinary Python path decoded the initial tree and first owner frame for **32 of 32** actors with its default compatibility policy. All 32 recorded a config fallback notice. Three recorded an unmodeled action and one recorded an omitted active state. A separate method-entry trace reached the same eight sender sites reported by Rust. All eight used Python's ordinary one-shot sender branch. The Rust count uses its own factory and projection requirements.

The next independent matrix at `934f4fec` used the same pack and removed one movement-sampler diagnostic each for Doomfist and Baptiste with no new sites. The matrix has **53** blocking sites across the same 22 actors. The #72 900-frame result matched the preceding run across all 476 observed sites.

At `d3da7e1a`, two same-pack runs retained 10 independent passes and 53 blocking sites. Two stat timer sites now report unsupported state after the placement checks were removed. Baptiste's remaining site reports an unavailable entity-variable read, which is a controller lookup in the startup expression. The #72 capture kept all 476 observation sites.

The `9485d72e` matrix cleared one frame-wait registration diagnostic for Wrecking Ball. Both runs reported **52 blocking sites** and **10 of 32** independent passes. The #72 capture still matched across all 476 observed sites. A separate local listing audit left Baptiste's controller value unresolved across the statically reachable controller graphs.

The `c2e75f7c` matrix cleared one ordinary health query diagnostic for Moira. Both same-pack runs reported **51 blocking sites**, **10 of 32** independent passes, and **1 of 32** ordinary Practice Range script attachments. Two selected-pool query diagnostics still need investigation. The #72 capture matched across all 476 observed sites.

At `eff202e3`, the loader routed an inherited base state through the class metadata path at arbitrary placements. The same-pack reports retained 51 blockers and 10 of 32 independent passes. The #72 capture matched across all 476 observed sites.

The `d747f926` reports matched byte for byte and cleared both remaining selected-pool health query diagnostics for Zenyatta. The 32 independent probes reported **49 blocking sites**, with **10** passing. The ordinary Practice Range factory still attached one player script. The #72 capture matched at all 476 observed sites. Original-client acceptance has a separate gate.

The Windows paired room checks at `d747f926` used 339 ticks each for idle and single-shot reload. Python `45044d0d` and Rust produced two repeatable, individually valid traces in each case. Strict comparison returned `incompatible` for the host and player VM RNG model and seed. The RNG diagnostic found no first difference in the recorded tick fields. The manifest and schedule digests matched the earlier ARM64 runs. The effective input tree digest differed.

At `b7f953ae` and `60b2b955`, the ordinary health pool and bound body angle changes retained **49 blocking sites**, **10 of 32** independent passes, and **1 of 32** ordinary Practice Range attachments. Two reports per revision matched byte for byte. Each revision matched the preceding startup and #72 captures apart from the recorded source revision. The isolated wave health regression passed for all six placements and four difficulty values.

The Windows idle and single-shot reload checks at `60b2b955` matched the prior Windows input and observation hashes for both implementations. Each schedule covered 339 ticks. The strict comparison retained the same four RNG setup differences.

At `8fbc2fa3`, the authored projected-message contract passed loader,
lifecycle, dispatch exclusion, and owner and observer wire tests. Two
runs on the existing 104319 pack matched each other byte for byte.
All 32 actor results and 49 blocking sites matched the preceding run,
with 10 independent passes and one ordinary Practice Range attachment.
The eight persistent registration diagnostics remain open. The #72
capture matched across all 476 observed sites.
Both 339-tick Windows schedules matched the preceding input and
observation hashes, with the same four RNG differences in strict
comparison.

At `c0fe55ad`, the synthetic fault regression confirmed that a halted
target tracker releases its child's registered host effect exactly once
and discards the due timer. The two same-pack actor reports retained
49 blocking sites, 10 independent passes, and one ordinary Practice
Range attachment. The #72 capture matched all 476 observed sites.

At `ad2d1b42`, synthetic startup tests verified that a one-shot player
query can read the frozen room frame before body attachment, and that
replacing a body twice in one frame keeps the old body out of both startup
views. The existing 104319 Practice Range pack reported no startup spatial
frame. Two same-pack reports matched byte for byte. Apart from the revision
and the added spatial-context field, the reports retained the previous
32 actor results and 49 blocking sites, with 10 independent passes and
one ordinary Practice Range attachment. The #72 capture matched all 476
observed sites.

The Windows #131 idle and single-shot reload schedules each produced 339
ticks. Both implementations repeated their observations and passed
individual trace validation. Every tick matched its preceding Windows
capture. Strict comparison still reports the same four RNG model and
seed differences between the implementations.

At `39932467`, the shared owned-entity count stopped rejecting queries
with an explicitly unbound owner. The old binary reproduced both count
diagnostics on the same prepared pack. Two updated reports matched byte
for byte, with **47 blocking sites** and **11 of 32** independent passes.
Zarya passed independent startup, and Symmetra cleared her count
diagnostic. The #72 capture matched across all 476 sites.

Ordinary Practice Range attachment remains **1 of 32**. Scene
preparation installs a Soldier-only factory there, and the factory
returns no runtime for a different hero. The independent probe calls
the per-hero factory builder directly. Ability coverage and original
client acceptance require separate verification.

At `9f6114ae` and `763b8e56`, two reports per revision from the same
prepared 104319 runtime matched byte for byte. All 32 actor rows matched
the preceding reports, with 11 independent passes, one ordinary room
attachment, and 47 blocking sites. The Phase 2 changes cover the shared
direction evaluator, scene placement collection, and parent-owned states
without completion operands. The #72 capture matched the earlier
900-frame run at all 476 observation sites on both revisions.

The prepared bot firing-placement regression passed on the earlier
binary and failed at selector evaluation after the first direction
change. The follow-up preserved the host's explicit no-target reply.
The same bot regression then passed. The legacy roll scenario stopped
during map preparation on both tested bundles because its map record
lacked spawn points.

The Windows #131 check at `763b8e56` used the same effective runtime
profile and schedules as the previous baseline. Idle and single-shot
reload each produced 339 ticks. Rust and Python traces both validated and
repeated, and every tick matched the preceding capture for its
implementation. The strict comparison retained the same four RNG model
and seed differences.

At `15a10d4e`, authored owner graphs gained a `BodyActive` query.
Synthetic tests covered initialization with and without room activity,
paused and resumed gains, health awards at dispatch, death, and stale
updates after same-wire replacement. Two same-pack startup reports
matched byte for byte. The 32 rows matched the preceding revision,
with 11 independent passes, one ordinary attachment, and 47 blocking
sites. The #72 trace matched all 476 observations across 900 frames.
On the same Windows #131 inputs, idle and single-shot reload each
produced 339 ticks. Both implementations repeated their observations,
passed individual trace validation, and matched their previous traces
tick for tick. Strict comparison retained the four RNG setup differences.

At `8d1d33a6`, native eye-position operands gained a player anchor from
the simulation mover. Potential foreign eye and direction selectors now
request a frozen room pose. The startup scene captured 15 living
candidates. Both same-pack reports matched byte for byte. All 32 actor
rows remained unchanged, with 11 independent passes, one ordinary
attachment, and 47 blocking sites. The prepared firing-placement
regression passed, and the #72 trace retained all 476 observations.
Simulation stance heights have no new original-client verification.

The Windows #131 idle and single-shot reload schedules each produced
339 ticks. Each implementation validated and repeated both traces.
Every tick matched its own preceding Windows capture. Strict comparison
still reports the four RNG model and seed differences.

At `e540fdc0`, the native local combat accumulator stopped using two
fixed placements to choose its no-tick state. The current owner body
reports the local accumulator absent to the VM. Hosts with available or
unknown context still report an unsupported state. Two same-pack startup
reports matched byte for byte. The 32 actor results remained unchanged,
with 11 independent passes, one ordinary attachment, and 47 blocking
sites. The #72 900-frame trace matched all 476 prior observations. The
Windows #131 idle and single-shot reload traces each matched their
preceding 339-tick captures for both implementations. Strict comparison
retained the same four RNG setup differences.

At `e6cfc337`, player, NPC, gate and mode scripts began reading one
mode-scoped engine settings table. The portable and native synthetic
suites distinguish missing keys, explicit null, empty tables and mode
isolation. The prepared Practice Range pack exercised the compatibility
lookup with its existing settings. The two same-pack startup results
matched byte for byte. The 32 actor rows remained at 11 independent
passes, one ordinary attachment and 47 blocking sites. The #72 trace
retained all 476 observations. This revision also includes the authored
eye observation recovery contract at `9b957ce7`.

At `9caac758`, the auxiliary view binding states began using host
context at any placement. The owned headless body also reports outline
absence through the existing outline branch. The same batch added
elapsed waits for direct numeric variable bindings and moved the
literal-delay cinematic boundary states to field and slot validation.
Synthetic tests covered dormant operands, cleanup, timer changes and
reentry, and the owner and observer boundary payloads.

Two reports from the same prepared Practice Range pack matched byte
for byte. All 32 actor rows and the #72 trace matched the preceding
revision, with 11 independent passes, one ordinary attachment, 47
blocking sites and all 476 observations. Both Windows #131 schedules
repeated and matched their previous 339-tick captures for each
implementation. Strict comparison retained the four RNG setup
differences. The cinematic payload tests used a synthetic graph.

At `0d3716e4`, a player body gained an explicit private charge-root
selection. Synthetic tests cover mount validation after the room adds
its roots, settled health delivery, and owner and observer projection.
The existing bridge takes damage eligibility from the room phase and
retains its startup projection. The local 104319 catalog gives each of
32 player bodies an authored cost binding read by the shared startup
graph. The selected charge route was exercised by the synthetic
fixture.

At `5b269875`, authored cues began rechecking listener liveness at
reliable admission. The synthetic case kills the listener with its
generation unchanged and verifies that the queued cue is discarded.

Two same-pack startup reports at `0d3716e4` matched byte for byte.
All 32 rows remained at 11 independent passes, one ordinary attachment
and 47 blocking sites. The #72 trace retained 476 observations. Both
Windows #131 schedules repeated and matched their preceding 339-tick
captures for each implementation. Strict comparison retained the four
RNG setup differences.

At `3dbfd5e4`, the native voice state keeps its lifecycle when the local
host explicitly has no voice context. Its verified empty state codec
preserves owner and observer projection. Two same-pack 104319 reports
matched byte for byte. Moira raised independent actor startup to
**12 of 32** and the blocking-site count fell to **46**. Ordinary Practice
Range admission still attached one actor. The #72 Soldier owner trace
retained all 476 observations. The two #131 schedules each reproduced
339 ticks per implementation and kept the same four RNG setup differences
in strict comparison.

At `f031b4c1`, the linked state, periodic self shield recharge and
mode script entity query use the shared VM and host paths. The locked
workspace tests and Clippy passed with script coverage. Two same-pack
104319 startup reports matched byte for byte, with all 32 rows
unchanged from `3dbfd5e4`. They retained **12 independent starts**,
one ordinary room attachment, and **46 blocking sites**. The #72 Soldier
owner trace kept all 476 observations. The local pack lacks the
Junkenstein NPC and map inputs, so the encounter shield lifecycle
has synthetic coverage in this batch.

At `6c54b054`, settled player and bot impacts gained optional hit tags
on the shared health event. Native tests check the actual applied
delta, tag availability, and source replacement. The private authored
charge test binds these tags on the next VM frame. The stock synchronous
charge route still has its own target snapshot and damage message.
The locked workspace suite and Clippy passed with script coverage.

Two same-pack 104319 startup reports matched with all 32 rows
unchanged from `f031b4c1`. They retained **12 independent starts**,
one ordinary room attachment, and **46 blocking sites**. The #72 trace
retained 476 observations. Both #131 schedules reproduced their earlier
339 ticks per implementation. Strict comparison retained the same four
RNG setup differences.

At `b9df99a1`, the horizontal cap uses a validated row shape and
applies on the next movement frame. The local 373-graph pack has
two matching selector-1 actions with integer bounds. Synthetic player
and NPC tests exercise queueing and rejection. Settled weapon damage
also carries the target's original flags, tags, and script state
through the host event. Tests verify event scope and generation checks.
The locked workspace suite and Clippy passed with script coverage.

Two same-pack 104319 startup reports matched and retained **12
independent starts**, one ordinary room attachment, and **46 blocking
sites**. All actor rows and the 476 observations in the #72 capture
matched the previous run. Both #131 schedules reproduced 339 ticks
per implementation, with the same four RNG setup differences.

At `001cd3ea`, the room captures the target of a positive heal when the
event has a distinct sampled source. Target facts use the same capture
path as impacts, with the actual healed delta and unavailable hit
attribution. Two resource-free state classes now register by checked
fields and raw state slots. The prepared boss graph probe passed its
successor, owner and observer codec, and retirement checks. The locked
workspace suite and Clippy passed with script coverage.

The unaltered `0B8B` passive subtree and `00EB` bonus receiver passed
the prepared graph-runner probe with a detached host. It reproduced the
one-second passive deadline, admission cancellation and re-arm, and
bonus capping. Admission and attribution for a running room remain
separate from this graph execution.

Two same-pack 104319 startup reports matched with all 32 actor rows
unchanged, including **12 independent starts**, one ordinary room
attachment, and **46 blocking sites**. The #72 trace retained its
476 observations. Both #131 schedules reproduced the previous 339
ticks per implementation and the same four RNG setup differences.

The opt-in directed numeric route preserves ordinary local delivery
and uses the room queue for a validated foreign target. The prepared
probe passed for the original `00EB` sender actions and receiver with
provided caller inputs. Room tests checked source and target generation
at settlement and delivery on the next receiver VM frame. Natural
target binding and native foreign timing remain unverified.

The shared bonus path can derive its payload from the actual settled
heal and bind the target in an authored graph. The room regression covers
a grant on the recipient's VM frame after routing, the meter cap, and
discarding a request for a replaced body. Positive NPC-local health
events collect committed target facts after the VM's local mutations.
Direct target filters use the event's tags and, in a configured room,
the original target's affiliation and post-settlement liveness. The
room preserves these facts if a lethal target retires before delivery.
The health event also carries the original body identity through VM
delivery. A subsequent direct request to that same wire expires if the
room now identifies a different body. The room regression includes a
fresh award to the replacement to exercise the normal path.
A prepared probe uses the original admission claim and local sender
with the stock cap. These additions are pending integration verification.

The full pseudocode and prepared bundle both show seven `0D9A` receivers
and no literal sender. The `0B9E` entry binds different slots from the
three instance operands read by its outgoing grant. The original
dispatcher, the origin of those three values, and the order relative
to health settlement still need original-build evidence.

Static model protection now uses the authored state slots and an
explicit host capability. The prepared boss graph regression passed its
link, encoding, and final restoration checks. The current encounter
host owns the model mask bookkeeping. Runtime rendering and collision
effects have separate verification.

The workspace suite, Clippy, Python bundle and coverage tests, and
the three prepared graph probes above passed before handoff on this
Phase 2 worktree. Shared-branch verification belongs to integration.

Immediate self damage uses validated local state fields for two encounter
forms. The self-instigated form reads a positive native integer amount.
The no-instigator cleanup form retains the reference's 10,000 validation
and skips an already dead VM owner. The existing NPC host still owns health
settlement. Other no-instigator amounts, external targets, and compound
modifiers require their own evidence. Integration verification is pending.

Two of the three remaining native target-tracking startup sites have
empty embedded plugs. One configures a second filter and an entity list
output. The other configures a count output. Both read a dynamic radius.
The remaining site links embedded states. Each configuration still needs
its native query and output semantics verified.

Six actor probes have one blocking site each. Mercy's paired tag query
now selects its shared VM path by authored fields. Its host still needs
the tagged placement inventory and live actor tags. Orisa and Sigma
still hit persistent message registration. Baptiste cannot read a
controller variable. Torbjörn's hardpoint query now reaches the shared
host interface and reports unavailable active-model membership.
Symmetra still reports an unsupported state.

The prepared 104319 bundle supplies voice stimulus references and gallery
unlock metadata. It lacks an authored stimulus-to-line mapping and playback
durations for the player voice service. The room already routes resolved
cues to listeners by body generation. A player voice provider still needs
the catalog and timing inputs before active-context playback can be verified.

| Workflow | Python reference | Rust implementation | Open client or parity gap |
| --- | --- | --- | --- |
| **Login, Profiles & Lobby** | Battle.net/JAM auth, profiles, settings (`22200`–`22204`), queues, Windows launcher + TLS relay. | Native BNet/JAM lobby, `profile-settings-v1` (CI verified), encrypted UDP seat handoff. | Standalone Rust launcher & public client connection flow open. |
| **Hero Gallery** | Loadouts, skins, equipped voice & emotes. | Loadout equip/persist, live skin body replacement, 23 NPC previews (C++ policy verified in CI). | Client Hero Gallery NPC framing & animation retest pending. |
| **Practice Range & Soldier: 76** | Full body/weapon VM, rifle, Sprint, Helix, Biotic Field, bots, format-5 collision. | Stock Soldier startup tree + rifle/Sprint/Helix/Biotic Field VM regressions; bots/projectiles in Bevy ECS + bounded versioned Lua `think()` patrol ABI (`v2`); all 43 movers match Python trajectories. | Client 60s stock-weapon mount, HUD/ammo sync, upstairs patrol, bot HUD/SFX (#3, #7–#9, #13), and original-client acceptance of the `08B6` / `004B -> 01CB` startup tree differences vs. Python pending. |
| **Movement & Collision** | GJK/contact solver, static ray queries, `0688` + 95 map caches. | `orpheus-collision` matches 10,148 Python movement records & 8,256 velocity cases; shared map-agnostic static navigation queries (`walkable_ground`, `capsule_clearance`, `walk_edge`, `find_path`, `valid_spawns`, `snap_spawn_points`) verified on synthetic fixtures and prepared `104319` Practice Range (`0688`), King's Row (`00D4`), Temple of Anubis (`005B`), Hanamura (`0165`), and Kanezaka (`0CD3`) collision caches ([#63](https://github.com/leafall903/ORPHEUS/issues/63), manifest SHA-256 `dad26628515e60e45e6128effd0c6408092dee8c8fc674ce00850b3463f8a90b`); player bodies live in `RoomSimulation` ECS ([#65](https://github.com/leafall903/ORPHEUS/issues/65)) and safe-boundary Lua AI hot reload with rollback landed in [#120](https://github.com/leafall903/ORPHEUS/pull/120). | Dynamic player-vs-player / player-vs-bot character collision blocking and live client checks (#7, #13) remain open. |
| **Doomfist & Wrecking Ball** | `leafall` merged into `main`: Slam, Rocket Punch dash/slow, Uppercut, Shield, Grapple, Quad Cannons, Piledriver, Minefield. | Rust has consumers for the movement, short-action retry, Slam placement and shockwave fixtures. | Verify the remaining ability behavior on the original client. |
| **Fault Isolation & Inspector** | Single-process dashboard & replay journals. | Room-owned errors & `catch_unwind` recovery (`room-isolation-v1`), `--features inspector` 3D viewer with bounded read-only physics, AI route/spawn, and StateScript VM overlays ([#62](https://github.com/leafall903/ORPHEUS/issues/62)), opt-in `--debug-control` local room pause/resume/single-step controls ([#84](https://github.com/leafall903/ORPHEUS/issues/84)), two-room worker timing benchmark. | Multi-player live room retest open. |

---

## Junkenstein's Revenge & Normal Cassidy (`0x3C`) Status

Headless offline regressions against pinned Python `e1128509` and prepared 104319 packs pass across the core encounter loop:

| Component / Mechanics | Rust Headless Coverage (`--ignored` with 104319 Packs) | Remaining Live / Client Gaps |
| --- | --- | --- |
| **All 14 Mode Cards & Gate** | Revenge/Endless/Challenge cards initialize; placed gate runs `0CA7`, health stages (`3` damage poses + destroyed), and team outcome projection (`MECGameMode` fields 7 & 9). Gate health/damage stages, encounter phases, and registration/rollback live in `RoomSimulation` ECS (`GateComponent`, `EncounterComponent`), verified by `9` self-contained unconditional unit tests (`game_combat::tests::encounter_ecs`); local base-pack runs lack Junkenstein graphs. | Gate visual state & post-match recap elapsed-time HUD binding remain open. |
| **Cassidy Primary & Fan (`0B8B`)** | Primary (`70`/`140` crit), Fan (`50` non-crit), ammo & passive/damage ultimate charge (`0.5`/`0.75` target-tag reductions): `scene_assets::tests::normal_cassidy_primary_and_fan_damage_all_boss_bodies`, `scene_assets::tests::normal_cassidy_common_npc_damage_charge_and_retirement`. | Live client attack feedback & HUD charge sync await client acceptance. |
| **Combat Roll (`01F0`)** | Collision-aware ~6.15 m Roll displacement across 5 directions, spawn-wall contact, and 5-to-6 ammo refill (`7b13651b` / `74d6ff78`). | Live directional/actor contacts & full hero matrix open. |
| **Flashbang (`224E`)** | Gravity/range flight blast (`800`-frame E-input replay), press-window gating, priority/excluded-tag checks, Reaper Wraith immunity rejection, velocity cap (`224E/31`), captured aim lock (`224E/34`), and targeted defender notices across all 5 Bosses. | Dynamic Boss spawn category bits & native hit-reaction animations await verification. |
| **Deadeye (`06AB` / `0CA5`)** | LOS target rays, lock progression, shot resolution, and natural two-cast 9-Boss Victory replay (`--junkenstein-validation` overrides tested separately). | Full live-client Deadeye lock visuals, SFX & natural Victory acceptance open. |
| **Bosses, NPCs & Score** | All 5 Boss body/controller pairs, common Zomnic/Zombardier/Shock Tire corpses (`8s` timer), Witch healing, Boss speech (`40E05F6B` / `junkenstein-boss-speech-v1`), `game_combat::tests::normal_encounter_first_reaper_finishes_intro_and_pursues_live_defender`, `game_combat::tests::normal_common_enemy_death_updates_authored_team_score`. | All 5 Boss intro cameras, Witch pistol swap, Zombardier combat, Reinhardt narration & Gallery client rendering await live 104319 client pass. |

*(Reference pack digests used for the original `2026-11-12` `13acf15` prepared-runtime baseline — Base `0688` SHA-256: `b11115f3201ebd0765cc8f0cf2ec0e72de6620b9913f8d17962c71649b528d58`; Enriched `0B92` SHA-256: `db7a7bdbddfb8b40e5e4c9a392d498ac36b13aed5144108311a985ea98e6d8f8`.)*

---

## Open Engineering Issues

| Issue | Scope |
| --- | --- |
| [#70](https://github.com/leafall903/ORPHEUS/issues/70) | Clean up transitional contract scripts, legacy prototypes, and stale branches. |
| [#121](https://github.com/leafall903/ORPHEUS/issues/121) | Diagnose and fix the 104319 client access violation (`0xC0000005`) during sustained Soldier Practice Range play. |
| [#131](https://github.com/leafall903/ORPHEUS/issues/131) | Complete the private verified Soldier gameplay trace comparison on the ARM64 runner and reconcile remaining RNG setup/parity items (`golden_python_room.py` now aligns `owner_component_start_frame = 1047` and `setup_delivery_ticks = 50`). |
