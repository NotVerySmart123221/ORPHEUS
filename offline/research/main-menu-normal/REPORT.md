# Normal menu backdrops and randomized heroes

Verified 2026-11-08, retail build `1.74.0.0.104319`. Ordinary backdrops, per-login random heroes and random owned menu skins render with the rebuilt `retail_menu.asi`. Gallery/Training hide the menu model; returning from Gallery or gameplay restores it. Equipped skins remain unchanged. Test clients exited cleanly.

Two independent blockers were reproduced. Forced event maps selected authored scenes without the ordinary Showcase placement: OWL map `0xEEE` and farewell map `0xF14` contain no entity `0x12B4`; ordinary maps `0x664`, `0x675` and `0x676` do. Clearing the event list activated entity `12B4`/graph `0B4A`, but its hero remained invisible. Read-only inspection found valid hero940/entityDef1206/skin8172 values and unset visibility variable1962. Cosmetic creation at native `0x7FF6D1BCC42A` tests descriptor field `+0x128`; its expression is `!var1962 || !var12884`. Graph `0B4A`'s message `0x024000000000062C` listener normally drives a Stack writing1962=true while the menu is visible; that Stack was inactive offline.

The lobby defaults to `--event none` and sends message38900 with an empty celebration list. Existing server-side per-login random hero selection stays intact. The plugin's dynamic-variable hook enables entity variable8646 only on tournament-route UI graph`0134`. That graph's native local input29323 controls publisher48/state27 for message062C. The message drives `0B4A`'s listener162/Stack167, so native push/pop owns visibility1962. Input29323 was observed true on the main screen and false in Training. Before the fix, publisher27 was null on both screens; with the fix it was active on Main and idle in Gallery. Bool38's only enabled subtree is Bool43, whose only true child is this publisher. Ready flag12884 remains untouched.

An initial workaround seeded1962=true and rendered the hero, but later transition testing exposed a tiny menu actor in the distant Rio Training backdrop. That permanent seed has been removed. The same backdrop now hides the actor in Training and restores it on Main. Earlier screenshots below document the initial restoration, not the final visibility implementation.

The plugin also seeds empty Showcase skin input13242 from a cached random owned skin, using native catalog/conversion APIs. Explicit hero/skin previews, Gallery equips and gameplay loadouts are retained. [Native route and tests](owned-skins.md).

| Session log directory | Observed result |
| --- | --- |
| `main-menu/20261108T035752.216960Z` | OWL courtyard, no hero; graph `0B4A` absent |
| `main-menu/20261108T040320.040768Z` | Ordinary King's Row backdrop; `0B4A` active, no hero before visibility initialization |
| `main-menu/20261108T041732.016977Z` | Winston posed on Rialto; plugin logged initialization once |
| `main-menu/20261108T041920.756706Z` | Widowmaker posed on Volskaya; different hero and backdrop, initialization once |
| `main-menu/20261108T042301.082347Z` | Final rebuilt plugin, including tournament-only guard: Wrecking Ball on Route 66 |
| `main-menu/20261108T044349.027483Z` | Symmetra, random Seoul Dynasty skin; Gallery and gameplay retained Classic; exposed distant actor in Rio Training with the initial visibility seed |
| `main-menu/20261108T045412.650518Z` | Current native-lifecycle build: Soldier on Rio, random choice Classic from73 candidates; Gallery/Training hide, menu restores; Echo gameplay returned to hidden Training on Havana, then restored Soldier on Main |

![Winston on Rialto](winston-rialto.png)
![Widowmaker on Volskaya](second-login.png)

[Initial-build capture](final-build.png) documents the earlier tournament-only guard. [Current main menu](native-lifecycle-main.png) and [Training on the same Rio backdrop](native-lifecycle-training.png) verify the final native lifecycle.

Before-fix captures: [event scene](event-no-hero.png), [ordinary backdrop without visibility](normal-before-visibility.png). Images came from the existing D3D frame-capture plugin after desktop capture timed out; no Computer Use input was sent after the user's cancellation.

Regression checks were run red before each fix: lobby event/default tests, native visibility restoration, owned-skin selection and the replacement publisher gate. Current native tests cover precise graph/scope/tournament gating, preserved screen-visibility false/true, no writes to visibility1962, selected-hero resources, cached skin selection, unavailable/non-skin filtering, explicit preview inputs, native fallback/retry, unchanged owned entries and existing login/recovery. MSVC production build passes. Combined lobby/assets/research suite:352 passed. Native test: `retail login, menu visibility and owned-skin checks passed`.

Initial transition session `main-menu/20261108T043001.576992Z`: Echo appeared on Rialto. Soldier's Olive preview reverted to equipped Classic when backing out, and Echo returned on Main. Soldier gameplay returned through Training to Echo on Hanamura. Those backdrops did not reveal the distant-actor bug subsequently found on Rio; the final publisher fix above supersedes the initial visibility result.

Captures: [Gallery grid](transition-gallery-grid.png), [Olive preview](transition-olive-preview.png), [menu after Gallery](transition-main-after-gallery.png), [Hero Select](transition-hero-select-soldier.png), [gameplay](transition-gameplay-settled.png), [pause menu](transition-gameplay-pause.png), [Training after gameplay](transition-main-after-gameplay-settled.png), [restored menu hero](transition-main-after-gameplay-final.png).

Final-build evidence: [Gallery grid](native-lifecycle-gallery-grid.png), [isolated Gallery model](native-lifecycle-gallery-soldier.png), [restored hero after Gallery](native-lifecycle-after-gallery.png), [Hero Select](native-lifecycle-select.png), [gameplay](native-lifecycle-gameplay.png), [pause menu](native-lifecycle-pause.png), [hidden Training after gameplay](native-lifecycle-after-gameplay-training.png), [restored Main after gameplay](native-lifecycle-after-gameplay-main.png). Random-skin isolation: [Seoul Dynasty menu skin](random-skin-first.png), [Gallery Classic](random-skin-gallery-classic.png), [Hero Select Classic](random-skin-select-classic.png), [gameplay Classic](random-skin-gameplay-classic.png), [same menu skin after gameplay](random-skin-after-gameplay.png).

Remaining limit: every hero, skin, backdrop and screen transition was not exhaustively exercised. Native addresses/layouts are specific to the verified build and opt-in retail-menu plugin. In-process account/ownership changes were not tested. Existing event scenes remain selectable, including `--event farewell`.
