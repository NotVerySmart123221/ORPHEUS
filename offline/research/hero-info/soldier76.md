# Soldier: 76: Hero Info reference

Source: the five Hero Info clips, extracted with `tools/assets/hero_movies.py` (1280x720, 29.97 fps). Frame numbers below are clip frames (f30 = 1.00 s). Contact sheets of the key windows are next to this file in `Soldier76/*.jpg`.

| Clip file | Loadout | Ability | Length |
|---|---|---|---|
| `2_..8D_..098.mp4` | `0x0B9000000000008D` | Heavy Pulse Rifle | 180 f (6.0 s) |
| `3_..8E_..0B6.mp4` | `0x0B9000000000008E` | Helix Rockets | 105 f (3.5 s) |
| `0_..8F_..0C9.mp4` | `0x0B9000000000008F` | Sprint | 280 f (9.3 s) |
| `1_..90_..0AE.mp4` | `0x0B90000000000090` | Biotic Field | 284 f (9.5 s) |
| `4_..91_..0F2.mp4` | `0x0B90000000000091` | Tactical Visor | 340 f (11.3 s) |

Every clip ends with a ~0.3 s glitch transition (not gameplay). The footage is 2016-era (old HUD, 25-round magazine): see [README](README.md).

## Heavy Pulse Rifle (`rifle_fire.jpg`, `rifle_reload.jpg`)

- **Fire:** first shot f35. One muzzle flash every 3-4 frames (~9 shots/s). The gun kicks a little per shot; the view barely climbs. Ammo counts down per shot.
- **Target dies** ~f90, the player switches target and empties the magazine at f112.
- **Empty-magazine reload**, f114 → f158 (~1.47 s):
  - f114-122: gun rolls right, magazine side turns toward the camera.
  - f124-128: old magazine ejects up and right; left hand enters bottom-left.
  - f130-136: left hand lifts a glowing blue magazine high (top-left).
  - f138-146: left arm sweeps across and seats the magazine.
  - f148-150: hand drops away, gun rolls back.
  - f156-158: HUD shows full ammo; idle by f160.

## Helix Rockets (`helix.jpg`)

- f20: launch. Large blue burst under the barrel (f21-24); the gun kicks up and right, recovers by f26-28.
- f26-30: rockets in flight (corkscrew), impact ~f31 on a target ~15-20 m away.
- f32-39: white/blue flash; f40-46: blue sphere dissipates.
- Ammo unchanged (rockets do not use the magazine). Helix icon goes to cooldown.

## Sprint (`sprint_in.jpg`, `sprint_out.jpg`)

- **Enter** f61: Sprint icon lights orange; the gun swings down-left in ~5 frames (f62-66) and is carried low across the body. Run bob while held.
- **Exit** f211: icon clears; the gun drops out of view (f212-214), then rises from bottom-right to idle (f215-219, ~0.27 s).
- Sprint held for ~4.8 s with no cooldown shown.

## Biotic Field (`biotic_throw.jpg`)

- Starts at 50/200 health (red damage vignette).
- f81: gun lowers to the right. f82-83: left hand enters bottom-left holding the emitter (blue puck with a red cross).
- f84-90: hand raises the emitter (wind-up); f91-92: throws it down and forward. f93: gold beam column at the landing point.
- f94-99: gun returns from bottom-right. Whole cast ~0.6 s.
- Healing ticks from f96; 50 → 200 by ~f210. The beam stays until ~f240 (≈5 s from f93) and ends with a flash. Field icon shows 15 (cooldown).

## Tactical Visor (`visor_on.jpg`, `visor_off.jpg`)

- f32: Q (ult icon flashes).
- f34-36: gun drops off to the right.
- f36-54: left hand rises to the visor (top-left of the view) and comes back down.
- f46: small yellow reticle; f50-54: cyan lock ring expands; f56: side brackets.
- f56-60: gun back up. Gesture total ~0.9 s.
- f68 (1.2 s after the press): the ult icon becomes a draining gauge. Firing and white target-lock diamonds start ~f72-78.
- While active, bullets home onto locked targets. Helix still fires (f156).
- Ring and brackets end f247-248: 6.0 s after f68. Total from the press ≈ 7.2 s.
- A normal reload follows after the visor ends (f270-306).

## Against our build (2026-11-08)

Live capture: Practice Range, saved code before this change, `gameinput.py fg-rec` at 60 fps (32:9 client, half size). Sheets `Soldier76/range_*.jpg` (frame numbers at 59.8 fps).

| Ability | Clip | Our Range | Verdict |
|---|---|---|---|
| Rifle fire | ~9/s, small gun kick | 30 rounds in ~3.0-3.3 s (server 1/9 s), same kick and flash | match |
| Empty reload | roll → eject up-right → lift glowing mag → sweep and seat → roll back, ~1.47 s | same sequence, f304-f376 (~1.2 s visible) | match |
| Helix | blue burst under barrel, gun kicks up-right and recovers in ~6 frames | same burst and kick (f490-f500) | match |
| Sprint enter | gun swings down-left in ~0.17 s, carried low | same, ~0.2 s (f796-f808) | match |
| Sprint exit | gun drops out, rises from bottom-right in ~0.27 s | same (f942-f960) | match |
| Biotic Field | throw anim; canister leaves the hand 0.4 s after the press, lands ~1-2 m ahead, gold beam | before: floor ring ~0.1 s after the press, before the throw. After: ring right after the throw, ~0.4 s after E (still at the feet) | **fixed, seen live** |
| Tactical Visor | gesture, then 6 s of lock-on; 7.2 s in all | before: no gesture, lock-on at the press, 6.0 s in all. After: gesture, lock-on, 7.2 s (below) | **fixed, seen live** |

Emitter model: the 1P hand holds the same blue puck with a red cross as the clip; the world canister (not flown yet) is the 1.74 black/gold asset (STATUS item 2). Not a defect.

### Live check after the fix (2026-11-08, `range_visor_cast.jpg`, `range_visor.jpg`, `range_field_release.jpg`)

Recording `visor.mp4` (59.8 fps). Server log anchors: Q at 10.922 (≈ f52), visor ends 18.124 (≈ f483), E at 19.606 (≈ f571).

- **Visor cast:** yellow tint f54-56; gun lowers f57-60; left hand rises f57-63, at the visor (top-left) f66-81, drops f84-87; gun back f90-93 (~0.7 s after Q; clip ~0.9 s). Lock ring from f66, side brackets ~f80.
- **Visor active:** firing kills both Range bots (ELIMINATED x2); ring and brackets present through f475, gone by f490: 7.2 s after Q, as in the clip.
- **Visor end (defect, fixed, seen live):** the yellow screen tint and the faint arcs at the screen edges stayed after the ring and brackets went (f500, f735; also in 2026-11-02's `research/abilities/visor.png`). They are effect state 3 (effect 5, `0x0300000000000ADE`, no hardpoint), which the end frame left out, and a looping effect left out keeps playing. The idle frame now sends it with its Begin bit cleared. Retest (`range_visor_end.jpg`, frames 20/300/480/520/620/799; server visor 13:58:07.997 → 15.187): tint, arcs, ring and brackets all on at f480, all gone by f520 and after. Effect 21 (`0x03000000000009DB` on hardpoint `0x0DC0000000000012`, switch 90's false branch) is not sent.
- **Field:** gun lowers f576-580, hand throws the emitter f584-592, faint gold glow f596-600, ring clear and expanding by f608 (server release ≈ f595). Clip: press → beam 0.4 s.

Biotic Field: graph 599's Wait 8 (0.4 s) gates Volley 4, which throws canister `0x072D`. The server used to create the field at the press. It now creates it `FIELD_RELEASE` = 0.4 s later, at the player's feet at that moment, and not at all if the player died meanwhile. The canister's flight (speed 50, gravity 9.8) is still not flown, and the client still receives the Volley state at the press.

Tactical Visor differed from the clip and from graph 601:

| Clip / graph 601 | Our server before this change |
|---|---|
| Press plays StateAnim 3 (alias `0x0C80000000000029` → Soldier 1P clip `0x0A00000000001C07`, 1.2 s): the hand-to-visor gesture | Anim not sent: no gesture |
| Wait 91 (1.2 s) holds Stack 4 (`var33`, the action-priority var the fire button checks) during the cast | Stack sent for the whole visor |
| ChaseVar 8 (the 6 s timer `var215`) and target tracking start when Wait 91 finishes | Timer and aim-assist started at the press; visor ended 6.0 s after the press |

Now: `visor_instance('cast')` for the first 1.2 s (Wait, `var33` Stack, gesture Anim, switch 90 false), then `'active'` (ChaseVar 6 s); `_visor_target` and no-falloff damage only in the active phase; visor ends 7.2 s after the press. Still not sent: TrackTargets (a lock marker already showed without it on 2026-11-02, research/abilities.md), looping Anim 5 (alias `0x0C80000000000033`, no 1P binding) and effect 10.

Graph 601 node map: Ability 2 (owner 0) → subgraph Stack 14 (owner 9, `var842` visor on), Wait 91 (owner 41) → subgraph Stack 4 (owner 2, `var33`), B5C87F3D 28 (owner 14), Stack 80 (owner 34, `var699`); switch 79 (owner 33, true) → effect 5 (owner 3), State 83 (owner 37) → BD02E168 92 (owner 42, HUD presenter parent), switch 90 (owner 40, `var1956` false) → on false Anim 3 (owner 1); Wait 91 on finished → ChaseVar 8 (owner 6).

Not taken from the clips (footage predates 1.74 balance): magazine size, cooldown numbers, Helix damage.
