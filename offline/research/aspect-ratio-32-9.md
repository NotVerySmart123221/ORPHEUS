# 32:9 Graphics aspect ratio: feasibility research

2026-11-07. Target: Overwatch 1.74.0.0.104319.

**Implementation follow-up, 2026-11-08:** added opt-in `aspect_ratio.asi`, with live 32:9 menu, persistence, matching render resources and measured horizontal FOV expansion. [Implementation, verification and limits](aspect-ratio-32-9/REPORT.md). The research-only scope and unverified findings below describe the initial investigation.

## Finding

**Feasible in principle as a client modification; no hidden 32:9 option or config-only unlock was found.** Native code explicitly represents the three existing choices, fits the viewport to their ratios, and adjusts camera FOV for the wide choice. Adding a fourth entry needs changes across those paths. Successful implementation and an injection/hook method remain unverified.

Resolution detection is separate. This research accepts the user's observation that a 32:9 display mode is already detected; it does not propose adding resolutions.

Research only: existing offline IDA database inspected through idalib. No game interaction, live memory reads/writes, debugger attachment, process suspension, restart, injection, binary/config/code changes, IDA patches/renames, or explicit database saves. Only this research note was created.

## Sources and scope

Primary binary: [Overwatch-state-analysis.dump.i64](../ida/Overwatch-state-analysis.dump.i64), image base **`0x7FF6D1220000`**. All addresses below are snapshot VAs, not addresses to reuse in another process. IDA reports input-dump SHA-256 `c03c97057c5243a6d45577baf2919d4f3413b66de77bcdf5c77f86983596f4f4`; this is not a claimed hash of the installed executable. The build association follows existing repository research and [ASI documentation](../tools/asi/README.md).

Graphify's existing 1,981-node graph was queried; it supplied config/camera research entry points, but contained no aspect-ratio finding. Conclusions below come from direct binary inspection, not inferred graph relationships.

Blizzard's July 27, 2020 support reply lists only 16:9, 16:10 and 21:9, and describes black borders for unsupported ratios. This corroborates the original game's supported list; it does not establish mod feasibility. [Blizzard support reply](https://us.forums.blizzard.com/en/overwatch/t/cant-play-at-1280x1024-resolution-without-black-borders/531559/2).

## Confirmed native settings and menu mapping

Two Boolean settings back the aspect-ratio selection:

| Setting | Registration | Current-value byte | Getter |
|---|---|---|---|
| `LimitAspectRatio` | `0x7FF6D14DCAC0` | `0x7FF6D4447148` | `0x7FF6D29DF660` |
| `Use219AspectRatio` | `0x7FF6D14DF3E0` | `0x7FF6D44471A8` | `0x7FF6D29DF990` |

Registration uses objects `0x7FF6D4447110` and `0x7FF6D4447170`. Both getters are a byte load followed by return. The named strings are at `0x7FF6D404602C` and `0x7FF6D404613C`. The string search found these two aspect-setting names; it did not find `Use329AspectRatio`.

Graphics UI variable **`0x0D800000000065CA`** carries the selection. UI-to-settings function **`0x7FF6D1CD1460`**, instructions `0x7FF6D1CD174C`–`0x7FF6D1CD1778`, maps it to settings-structure bytes `+25` and `+26`:

| Menu index | Resulting ratio path | LimitAspectRatio | Use219AspectRatio |
|---|---|---|---|
| 0 | 16:9 | 1 | 0 |
| 1 | 16:10 / bounded unfixed-ratio path | 0 | 0 |
| 2 | 21:9 | 1 | 1 |
| Any other value | 16:9 fallback | 1 | 0 |

Settings-to-UI function **`0x7FF6D1CD05E0`**, instructions `0x7FF6D1CD09CF`–`0x7FF6D1CD09E9`, performs the reverse mapping: limit off → index 1; limit on and wide off → index 0; both on → index 2. Consequently, adding index 3 to UI data alone would still apply 16:9 and would not round-trip as 32:9.

Settings snapshot function **`0x7FF6D29E1190`** loads the two current bytes into structure offsets `+25/+26`. Apply function **`0x7FF6D29DD530`** writes them back at `0x7FF6D29DDE85` and `0x7FF6D29DDEC3`, notifies setting bindings, and requests graphics reconfiguration when relevant values change. Comparison/revert helpers include `0x7FF6D29E2020`, `0x7FF6D29E20C0` and `0x7FF6D29E2A80`.

There is a fourth Boolean combination `(0,1)`, but **it is not native 32:9 support**. The UI collapses it to index 1; rendering does not interpret it as 32:9. Reusing it could avoid a new persistent setting, but would require auditing every consumer and changing both menu mappings. No encoding choice was implemented.

## Confirmed viewport restrictions

The ratio helpers compute constants using obfuscated integer arithmetic:

| Function | Returned ratio |
|---|---|
| `0x7FF6D29DE840` | `10/16` |
| `0x7FF6D29DE900` | `16/9` |
| `0x7FF6D29DE970` | `(21 + 0.5)/9` |
| `0x7FF6D29DEA00` | `9/16` |
| `0x7FF6D29DEA70` | `9/(21 + 0.5)` |

**The option labelled 21:9 actually uses 21.5:9 in these paths.** Disassembly and the referenced `0.5f` constant at `0x7FF6D3815A48` confirm the extra half unit. A later 32:9 implementation must return `32/9` and `9/32`; changing only the integer 21 to 32 would produce 32.5:9.

**`0x7FF6D2C6F8F0`** fits dimensions into a supplied rectangle. With limit on, it chooses fixed 16:9 or 21.5:9 through the two getters. With limit off, it clamps the supplied ratio between 16:10 and 16:9; clearing `LimitAspectRatio` therefore does not unlock arbitrary widths.

For example, its existing wide path mathematically fits a 5120×1440 rectangle to **3440×1440**. A 32:9 path must permit the full **5120×1440**. This is a calculation from the code, not a screenshot or runtime observation.

Dimension helper **`0x7FF6D2C154E0`** also selects these ratios, and **`0x7FF6D2C13910`** uses them for a minimum-width fallback. `0x7FF6D2C88D00` stores the supplied dimensions, then calls the fit helper for its fitted dimensions. The fit helper has multiple graphics callers; changing only one call site would leave inconsistent sizing paths.

## Camera behavior: filling the display versus expanding the view

**`0x7FF6D18B8740`** converts a horizontal angle to a vertical angle using `2*atan(tan(H/2)*ratio)`, selecting `9/16` or `9/21.5` through the wide-setting getter.

In camera update **`0x7FF6D18BF3E0`**, instructions **`0x7FF6D18BF990`–`0x7FF6D18BF9F2`** check both Boolean settings. When both are on, the code applies a 16:9 → 21.5:9 FOV conversion before calling camera setter `0x7FF6D2C97E70`. Its effective transform on the incoming angle V is:

```text
V_wide = 2 * atan(tan(V / 2) * (16 / 21.5))
```

This reduces the vertical angle. When projection aspect matches the chosen ratio, the transform preserves the original horizontal angular extent. It is direct evidence for a cropping adjustment in this camera path, not a claim that every spectator, scope or cinematic camera has been audited.

Two possible 32:9 behaviors follow:

- **Extend the existing wide-mode behavior:** use a factor `16/32`. Display can fill at 32:9 while vertical view becomes narrower.
- **Show more world horizontally:** preserve the appropriate vertical view and let the wider projection expand horizontal view. Ratio/viewport changes alone cannot guarantee this; the camera adjustments also need changing consistently.

Illustrative projection math with a 103° horizontal reference: 16:9 vertical view is 70.5328°; the existing wide conversion gives 55.5119°; preserving the same horizontal extent at 32:9 gives 38.9451°. Preserving the original vertical view at 32:9 instead gives 136.6228° horizontal. These are mathematical examples, not measured current settings or a newly verified FOV limit.

Python assert checks independently verified the integer arithmetic behind both wide helpers and the horizontal-tangent invariant of the conversion. No game tests were run. Microsoft's documentation also separates vertical FOV and aspect as projection inputs; it supports the general math, not a claim that Overwatch calls that API. [Perspective projection documentation](https://learn.microsoft.com/en-us/windows/win32/api/directxmath/nf-directxmath-xmmatrixperspectivefovlh).

## Scope of a later implementation

1. Add a fourth dropdown entry and its 32:9 label in the UI data/binding path. The selection variable and native handlers are identified; the precise dropdown asset and localization resource remain unidentified.
2. Extend selection decoding/encoding and persistence so all four choices survive Apply, reopening Graphics, restart and revert/defaults. Preserve the three existing choices.
3. Make viewport fitting, dimension fallback and reciprocal ratio consumers consistently select 32:9. Replacing the existing wide helpers globally would sacrifice the original 21:9 behavior and would not add a fourth option.
4. Apply a consistent camera policy, then verify gameplay, ADS/scopes, spectator views, menus, HUD placement and cursor hit testing. Extra scene visibility can also expose culling or presentation assumptions; those were not audited.

These are client graphics/UI changes. No evidence found that adding a server resolution message would address the identified restrictions; the native settings apply path already reaches the graphics system.

**Implementation constraint:** existing [ASI documentation](../tools/asi/README.md) and [retail_menu.c](../tools/asi/retail_menu.c) report image-protection changes failing and use writable heap-object vtable copies instead. The direct ratio helpers above are not themselves demonstrated virtual hook points. A later implementation must establish a working interception/data modification route. [Protection research](anti-debug-2026-11-05.md) also cautions that a protection-change failure does not prove every patching approach impossible. This research demonstrates targets and logic, not a tested inline patch or working plugin.

No implementation estimate, universal rendering guarantee, exact UI-asset patch, or tested hook method is claimed. Running game and other agent's session were untouched.
