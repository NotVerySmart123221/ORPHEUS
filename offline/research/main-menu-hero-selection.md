# Main-menu hero selection versus Hero Select

**Implementation follow-up (2026-11-08):** ordinary backdrops, randomized Showcase heroes and random owned menu skins are restored. Native UI message062C now controls show/hide; Gallery and gameplay-return checks pass without changing equipped skins. [Fix, evidence and remaining limits](main-menu-normal/REPORT.md). The body below records the preceding research-only investigation.

Build: retail `1.74.0.0.104319`. Research date: 2026-11-07. Scope: local source, existing research, read-only installed assets; no running-client access or runtime changes.

**Finding:** The client ships both a dedicated menu-hero selector and an authored cosmetic-entity path that consumes it. Main-menu graph `0x0B4A` uses the selected hero's **Showcase** entity. Hero Select follows a separate user-pick controller; a shared low-level renderer remains unproven. Backdrops use event map catalogs. Farewell's lineup is authored separately. The earlier claim that single-hero presentation was missing from shipped data was based on a mistyped class hash and is disproved below.

## Confirmed from current source

- Login advertises available heroes, skins and map headers in message 20505. `PracticeRange.menu_heroes()` returns the playable hero table; login combines it with the bootstrap and handoff heroes. Sources: [`lobby.py:93`](../tools/lobby/lobby.py#L93), [`lobby.py:145`](../tools/lobby/lobby.py#L145), [`practice.py:186`](../tools/lobby/practice.py#L186), [`content.py:41`](../tools/lobby/vendor/ow174/game/content.py#L41).
- Local lobby already chooses one hero per login with `secrets.choice(practice.menu_heroes())`, then sends that same GUID through PARTY 20700 and CONFIG 36600. This is server-side random choice, distinct from the native client's fallback random picker. CONFIG key is `0x87053C32`, formatted as lowercase GUID text; PARTY stores hero at `+0xB8`, skin at `+0xC0`. Sources: [`lobby.py:149`](../tools/lobby/lobby.py#L149), [`practice.py:79`](../tools/lobby/practice.py#L79), [`practice.py:291`](../tools/lobby/practice.py#L291), [`practice.py:303`](../tools/lobby/practice.py#L303).
- Backdrop/event input is independent: `PracticeRange.__init__` defaults to `event='farewell'`; preset farewell is celebration `0x128`, content-key asset index `0x1A3`. Message 38900 sends the selected celebration at record `+0x40`. Sources: [`practice.py:65`](../tools/lobby/practice.py#L65), [`practice.py:167`](../tools/lobby/practice.py#L167), [`practice.py:244`](../tools/lobby/practice.py#L244).
- Hero Select uses controller graph `0x13C1` in Practice Range, `0x0C90` in PvP, host `0x288A`, screen graph `0x288B`, and team entry `0x288D`. Controller frames instantiate that chain; entity variables `2438` and `31435` open the screen, with assembling variable `25377` controlling the skin picker. Sources: [`statescript.py:47`](../tools/lobby/vendor/ow174/game/statescript.py#L47), [`heroselect.py:39`](../tools/lobby/vendor/ow174/game/heroselect.py#L39), [`heroselect.py:63`](../tools/lobby/vendor/ow174/game/heroselect.py#L63), [`heroselect.py:70`](../tools/lobby/vendor/ow174/game/heroselect.py#L70).
- Hero Select choice travels as game message `0x0240000000001361` inside 21616. Current parser scans parameters for a recognized hero GUID; its comment explicitly says exact parameter layout remains uncertain. The handler validates the hero, looks up its equipped skin, switches body when needed, and closes the screen outside assembling. This path does not randomly choose a hero. Sources: [`heroselect.py:46`](../tools/lobby/vendor/ow174/game/heroselect.py#L46), [`heroselect.py:101`](../tools/lobby/vendor/ow174/game/heroselect.py#L101), [`match.py:1857`](../tools/lobby/vendor/ow174/game/match.py#L1857), [`match.py:1871`](../tools/lobby/vendor/ow174/game/match.py#L1871), [`match.py:1900`](../tools/lobby/vendor/ow174/game/match.py#L1900).

These source findings describe the restoration server and its data contract. They do not by themselves prove the retail client's internal preview renderer or any original online-service policy.

## Confirmed asset structure; rendering interpretation qualified

`STUHero` contains separate gameplay and presentation entity references: gameplay hash `950BBA06`, `322C521A`, `26D71549`, `8125713E`, and preview-emote hash `AC91BECC`. Local OWLib calls the latter presentation paths **Showcase**, **HeroGallery**, **HighlightIntro**, and **PreviewEmote** when exporting skins. Those are OWLib's labels, not recovered native symbols. Sources: [`STUHero.cs:11`](../OWLib-overwatch-1.74/TankLib/STU/Types/STUHero.cs#L11), [`STUHero.cs:29`](../OWLib-overwatch-1.74/TankLib/STU/Types/STUHero.cs#L29), [`SkinTheme.cs:39`](../OWLib-overwatch-1.74/DataTool/SaveLogic/Unlock/SkinTheme.cs#L39).

Read-only extraction of Soldier: 76 asset `0x02E000000000006E`, using existing `find_asset_payload`, `instances`, and `_decode_asset_ref`, produced:

| STUHero field | Decoded entity GUID | OWLib label |
| --- | --- | --- |
| `950BBA06` | `0x04000000000003CF` | Gameplay3P |
| `322C521A` | `0x04000000000003D0` | Showcase |
| `26D71549` | `0x04000000000009FC` | HeroGallery |
| `8125713E` | `0x0400000000000DDA` | HighlightIntro |
| `AC91BECC` | `0x0400000000001438` | PreviewEmote |

Extraction checked root class `7C3457DC` before decoding. Parser/source locations: [`retail_bundle_lookup.py:66`](../tools/assets/retail_bundle_lookup.py#L66), [`retail_bundle_lookup.py:119`](../tools/assets/retail_bundle_lookup.py#L119), [`stu_graph.py:20`](../tools/assets/stu_graph.py#L20). Distinct entity references show why selecting a hero GUID alone does not establish which posed model, animation, camera or lighting gets instantiated.

The installed Hero Select graph `0x058000000000288B` parsed as 101,357 bytes / 1,948 STU instances, including four client-only `STUStatescriptStateUXPresenter` objects (indices 17, 307, 498, 763). Installed farewell graph `0x0580000000000F14` parsed as 2,019 bytes / 32 instances, including `STUStatescriptStateCreateEntity` class `BB1054F4` at instance 10. These are separate authored graphs. Graph inspection alone does not identify a shared low-level renderer or resolve Hero Select's exact 3D-preview entity factory.

## Historical evidence and remaining uncertainty

Prior research maps farewell celebration `0x128` to catalog `0x361`, map `0xF14`, and records the lineup surviving forced Mei/Bastion overrides. It also records alternate-event maps loading without a standalone hero. Sources: [`hero-menu.md:137`](hero-menu.md#L137), [`hero-menu.md:176`](hero-menu.md#L176), [`hero-menu.md:178`](hero-menu.md#L178); existing capture: [farewell lineup](main-menu/farewell-lobby-heroes.png). These observations were not rerun in this investigation.

The earlier conclusion that a single menu hero was exclusively a missing live-service placement is unsupported. Primary investigator found that registration uses class hash **`47F99DF4`**, whereas earlier notes and their negative scan used **`47F95AF4`**. That negative scan searched the wrong bytes and cannot establish absence. Also, cached random GUID `0` cannot prove the getter never ran: a valid override returns before the random cache is filled. [`hero-menu.md:139`](hero-menu.md#L139), [`hero-menu.md:140`](hero-menu.md#L140), and [`hero-menu.md:174`](hero-menu.md#L174) require those corrections.

A fresh targeted scan of parsed class tables and raw bytes for **`47F99DF4`** (`F4 9D F9 47`) found no occurrence in Hero Select graphs `0x288A` / `0x288B` or farewell graph `0xF14`. This establishes absence in those three graph blobs only; it says nothing about referenced UI objects, entities, other graphs, map chunks or service data.

**Inference:** Hero Select preview machinery may supply reusable hero/skin/entity presentation components. Its user-pick controller is not evidence for menu randomization, and changing the menu hero override cannot replace farewell's lineup unless the selected scene actually consumes that getter.

**Unknown:** exact Hero Select preview factory; whether it shares the menu's Showcase resource evaluator; current activation of the menu model entity; random backdrop weighting/timing; semantic names of the native picker's availability filters. No implementation or runtime experiment was attempted.

## Native menu selection

Primary source: [`Overwatch-state-analysis.dump.i64`](../ida/Overwatch-state-analysis.dump.i64), image base `0x7FF6D1220000`, input-image SHA-256 `c03c97057c5243a6d45577baf2919d4f3413b66de77bcdf5c77f86983596f4f4`. Addresses below are this database's absolute VAs; subtract its image base for RVAs. Read-only idalib decompilation was checked against assembly where the decompiler omitted arguments or assignments.

`0x7FF6D1844170` returns, in order:

1. Global override `qword_7FF6D4058DA0`, if nonzero and resolvable as a hero resource. Existing protocol evidence associates this with CONFIG key `0x87053C32`.
2. Nonzero top item of system9's hero stack: buffer `+0x1348`, count `+0x1350`.
3. Cached random hero at `+0x298`, invoking `0x7FF6D1845AE0` if empty.

`0x7FF6D1845AE0` enumerates system70's catalog hash table (`+0x68` bucket count, `+0x70` buckets), copying each record's hero GUID at `+0x30`. `0x7FF6D1856F30` explicitly supplies system ID70. It draws a PRNG value modulo candidate count, resolves the selected hero, and checks service virtual slot `+0x88` against hero ID `+0x1C`. An additional gate rejects a hero when its byte `+0x1BE` is set and `0x7FF6D29DF980() <= 1`; that function reads global `dword_7FF6D4446CD4`. These predicates' product meanings are not established. If the selected, resolved hero fails those predicates, the function walks candidates and takes the first acceptable one; it does not repeatedly sample uniformly from a filtered list. An unresolvable initial pick clears the cache. A changed cached GUID triggers `0x7FF6D2DAED40`.

This makes the native random result **cached**, with a potentially biased fallback after availability rejection. Historical runtime evidence says the catalog contained the 32 advertised playable heroes; that catalog count was not reread from the running game today. Current restoration login instead already chooses a random GUID server-side and sends it as the higher-priority override.

The picker also has a direct caller at `0x7FF6D187E452`, inside `0x7FF6D187BE20`. Assembly shows `0x7FF6D1877E10` invokes that updater for pending numeric state5; the updater skips work if already in state5. Thus the getter is not the only reroll trigger. The meaning/timing of state5 is not established here.

Getter evaluator `0x7FF6D1B6E7E0`, vtable `0x7FF6D384B168` slot4, returns the hero as a resource value. Registration `0x7FF6D13DDF30` contains **`mov r8d, 47F99DF4h`** at `0x7FF6D13DDF90`. Matching source declaration exists at [`Misc.cs:15734`](../OWLib-overwatch-1.74/TankLib/STU/Types/Misc.cs#L15734). Earlier notes searched `47F95AF4`, a different value.

## Shipped consumer and model creation

A fresh, content-verified scan for bytes `F4 9D F9 47` covered all 228 root-manifest bundles and additional loose assets of resource types 3/27/121/197/278: **276,731 unique assets, zero read/hash errors**. This is not every loose asset in the install. It found two parsed class instances:

| Graph | Size | Content MD5 | Getter instance |
| --- | ---: | --- | ---: |
| `0x0580000000000B4A` | 11,684 | `ad3a1f362a7054f3ead58f7173e0aa6b` | 159 |
| `0x0580000000001215` | 24,198 | `805cb509a698376528c185ddeb2cc951` | 26 |

Graph `0x0B4A` has 177 instances. Its startup path is:

```text
hero input var13241, otherwise native menu getter instance159
  -> Stack65: hero var940
  -> Stack69: STU_93CBF16B(hero940) -> entity-definition var1206
  -> skin selection -> skin var8172
  -> entity-definition truth check + hero/skin resource-status checks
  -> CosmeticEntity instance1/state0(entityDef=var1206, skin=var8172)
```

Expression157 references getter159 through its dynamic-area `m_configVars` array, alongside dynamic input13241. Its bytecode is `08 00 03 04 0E 00 00`: dynamic input, OR short-circuit, config-var call, end. The normal skin expression152 uses the same pattern with input13242 and class`D4B06BC5(hero940)`. Opcode interpretation follows existing [`play-menu.md:53`](play-menu.md#L53); precise generic-expression value semantics were not separately rederived today.

**Showcase field confirmed by native assembly.** Class`93CBF16B` registration `0x7FF6D13BF0B0` leads through constructor `0x7FF6D1B8F190` to vtable`0x7FF6D384A7C8`; its slot4 evaluator is `0x7FF6D1B691A0`. After evaluating/resolving the hero, instruction `0x7FF6D1B69227` loads `rdx = [hero + 0x88]`. Native field registration at `0x7FF6D24E09F7`/`0x7FF6D24E0A00` associates field hash`322C521A` with offset`0x80`; its asset-reference GUID follows eight bytes of padding. This identifies the returned GUID as that hero's **Showcase** entity, the same field labeled by OWLib. Source layout: [`STUHero.cs:29`](../OWLib-overwatch-1.74/TankLib/STU/Types/STUHero.cs#L29), [`teStructuredDataAssetRef.cs:7`](../OWLib-overwatch-1.74/TankLib/STU/teStructuredDataAssetRef.cs#L7). The decompiler omitted this critical load; assembly supplied it.

Control route: Entry61 -> Stack65 -> SetVar75 -> Bool79(entityDef1206) -> Bool84(hero940) -> Bool90(skin8172) -> state98 -> CosmeticEntity1. Each BooleanSwitch takes its true-subgraph path. The last two predicates use class`8CE91499`; native evaluator `0x7FF6D1EF2C00` returns `sub_7FF6D293BF90(0, resource) == 3`. **Inference:** status3 means the resource is ready. The exact status enum was not recovered. Skin evaluator`D4B06BC5`, `0x7FF6D1B74FB0`, consults menu/system9 skin lookup `0x7FF6D1844230` and falls back through `0x7FF6D1A14500`; its exact equipped/default policy remains unresolved. A special skin branch also exists; no policy name is assigned to it here.

Entity definition `0x04000000000012B4` contains a StatescriptComponent referencing graph `0x0B4A` (inline graph field `C71EA6BC`, payload offset `0xD0`). Its 216-byte payload has content MD5 `bb497009f7255f342d0a3a55a1521f12`. This supplies a concrete entity to inspect for scene activation. Current instantiation/transform is untested.

Graph`0x1215` separately consumes getter26 in Stack24, assigning hero variable11669 (`0x2D95`). Its entry1 starts game-message listener5 for message`0x02400000000009AF`; the message also supplies that hero variable. Graph`0x0134` references`0x1215` through node198/state22 and inline graph-with-overrides instance201, while another child references the known main-menu button graph`0x1214`. This establishes the menu UI relationship; graph`0x1215` is not itself proof that the 3D hero entity is running.

Incoming-reference search checked 5,120 of 5,140 selected entity/graph/generic assets, with 20 encrypted/unreadable using the older scanner's key set. Positive references above were content-verified. Its negative results are not treated as exhaustive.

## Backdrops and practical implication

Read-only extraction reconfirmed catalog`0x01C00000000001B3` holds ten map/header pairs with indices`664,675,677,678,794,801,8BE,A84,AF0,B55`; catalog`0x216` holds`676`. Farewell catalog`0x361` holds only`F14`. Generic settings`0x0CA000000000012C` associate celebration`0x128` with`0x361`; some other/default entries also carry condition DataFlow ID`0x048000000000063D`. Lookup of that ID as a standalone root-CMF asset failed; its evaluator remains unresolved. Catalog data does not establish selection weights or reroll timing. Types: [`STULobbyMapCatalogInfo.cs:8`](../OWLib-overwatch-1.74/TankLib/STU/Types/STULobbyMapCatalogInfo.cs#L8), [`STUMapCatalog.cs:7`](../OWLib-overwatch-1.74/TankLib/STU/Types/STUMapCatalog.cs#L7).

Farewell is the restoration server's explicit default, and its graph creates its own lineup. The random single-hero selection/Showcase path already exists in shipped data. **Next investigation, not an implemented fix:** trace creation of entity`12B4`/graph`0B4A` in an ordinary menu scene, then inspect hero940, skin8172, entityDef1206 and status gates. The first missing activation/value would distinguish a scene setup problem from a resource/skin gate. Switching the celebration alone is not evidence that either is solved. Reusing Hero Select's entire controller is not supported by these findings.

Verification: read-only idalib xrefs/decompilation/assembly; CKey-verified asset extraction; assertions on graph `0B4A` instance159, expression arrays and bytecode; source review. Disposable extraction scripts live in `.scratchpad/`. No live client/process memory reads, debugger attachment, game launch, configuration edits, server messages, application-code changes or runtime tests. Graphify supplied discovery leads only; its broad query was truncated, so every reported mechanism was checked against local source/assets/binary. Graph traversal itself used no LLM API; session token cost was not measured.
