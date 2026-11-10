# Menu-only random owned skins

Verified native route in the 104319 dump, image base `0x7FF6D1220000`. The client already holds ownership received through hero catalog24900; no new lobby field, sidecar or equipped-loadout change is needed.

`0x7FF6D1A13BB0` obtains system70 through:

```text
table = (0x4E3494C633194824 - qword[0x7FF6D4104BD0])
        XOR qword[qword[0x7FF6D41786E0] + 320]
catalog = qword[table + 70*8]
```

Native map lookup `0x7FF6D2F2E900(catalog+0x58, &heroGUID)` returns a borrowed hero record, or null. Its owned-unlock array is at+0x00, u64 count at+0x08, hero GUID at+0x30, equipped Skin unlock at+0x38 and equipped WeaponSkin unlock at+0x50. Entries are16 bytes, unlock GUID at0 and flag at8. List membership establishes ownership; the flag's meaning is unresolved and it is not used as an ownership filter.

`0x7FF6D1A14540(unlockGUID)` resolves the loaded asset through `0x7FF6D295BA30`, validates SkinUnlock RTTI/class`E05E4446`, and returns its theme GUID at native+0xF8, or0. The asset reference starts at+0xF0; that first qword is its cached handle, not its GUID. No caller release is required. The same loaded-asset lookup filters unavailable themes. Read borrowed pointers only during selection on the graph thread; retain GUIDs only.

The plugin chooses one candidate with reservoir sampling using the CRT's `rand_s`, caches it for the current menu hero, and seeds local input13242 through the existing native variable setter. Seeding occurs only on tournament-route graph`0B4A` when local hero940 evaluates to a hero resource, and both explicit inputs13241/13242 are empty. Explicit Gallery previews and authored skin branches retain their inputs. An empty/unavailable pool leaves native fallback intact and can retry. Saved Gallery equips and the native ownership record are never written.

Native-boundary test was red before implementation (no skin write). It now covers owned non-skin filtering, unavailable themes, repeat reads, cached selection on a fresh graph, explicit hero/skin preview inputs, empty-pool fallback/retry, unchanged owned entries, and the existing visibility/startup checks. MSVC build and native test pass. Combined lobby/assets/research suite:352 passed.

First live session: `main-menu/20261108T044349.027483Z`. Symmetra selected unlock`0250000000001520`, theme`0A50000000001DC5`, from74 candidates. The menu rendered the Seoul Dynasty skin. Gallery, Hero Select and gameplay retained Classic. Returning from Gallery and gameplay restored the same menu skin on the new backdrop. Additional overlap checks are recorded in the main report.

Final native-lifecycle session `main-menu/20261108T045412.650518Z` selected Soldier's Classic unlock`02500000000004C7`, theme`0A500000000016C7`, from73 candidates. Classic is a valid random outcome. Gallery and Training hid the menu model; after Echo gameplay, Training stayed clear on the new Havana backdrop and Main restored Soldier. Skin selection code was unchanged between these sessions. [Screenshots and the replacement visibility lifecycle](REPORT.md).

PARTY20700's skin field was not established as a bridge to the menu graph. Native skin-stack push/remove functions are called by paired statescript lifecycle methods; using the owned catalog avoids inventing a new protocol path. Theme field`ECCC4A5D` points to text resources in sampled Soldier themes and is not a reverse unlock reference.

Limit: addresses and layouts are build-specific. Selection assumes ownership/preloaded resources stay stable during the process's single-account session; account switching and live entitlement revocation were not tested.
