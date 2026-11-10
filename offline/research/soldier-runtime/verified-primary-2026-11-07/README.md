# Clean saved-code primary capture

Retail build 104319, normal client PID 12312, session `main-menu/20261108T013004.383316Z`. Normal launcher and saved server code; no diagnostic wrappers or client process writes. The combined sweep also passed Helix, Sprint transitions, reload8→30 and Field ring/expiry. It exposed frozen ability countdown text, corrected by enabling per-frame StateAbility output bindings. Fresh normal PID34048, session `main-menu/20261108T013545.061237Z`, passed the final countdown/firing recheck. Both clients closed with exit0. All five Soldier presentation items are saved and verified;350 tests pass (12.84s).

- `primary-normal-60fps.mp4`: desktop-duplication capture of held primary fire, release, then another burst. Gun raises and moves during firing, then lowers on release.
- `primary-gun.jpg`: selected gun poses from that capture.
- `primary-ammo.jpg`: HUD 30→23→15→13→8→8→8. Server logged 22 shots ending at 8; HUD remains consistent after release.
- `sprint-transition-f00/f14/f59.png`: transition sequence samples while cooldown data updates. These precede the separate countdown-text correction.
- `reload-complete.png`: ammo30 after reloading from8. [Helix/Field after-primary captures](../verified-field-helix-2026-11-07/README.md) preserve the remaining sweep results.
- `final-countdowns-60fps.mp4` and `final-countdowns-overview.jpg`: fresh saved-code cast, Sprint enter/exit and primary fire after the cooldown correction.
- `final-countdowns-hud.jpg`: Field15→14→…→2 and Helix6→5→4→3→2→1→ready, with ammo30→24 for six logged shots. `final-countdowns-ready.png` confirms both abilities ready after expiry.

The authored route is Volley10.onBegin→Chase19/output1769→Mapping18/DataFlowF7→the first-person firing tree. C1 data frames supply the native command clock. A per-frame output binding prevents the previous static cache at 14ms: isolated native control81 measured F7 restarting at 0.007s and advancing through a 1.8s held burst, then restarting at 0.014s for a second 0.5s burst. F8 stayed 100. [Native route and limits](../first-person-fire-assets-2026-11-07.md); [separate C0/C1 ammo controls](../owner-delta-clock-2026-11-07/README.md).

First-person hardpoint queries returned a common fallback for gun/root IDs and are excluded as bone-motion evidence. Native recoil callback/replay counters are not counts of visibly rendered shots. The normal video and correct ammo are the saved-code visual evidence. Native Helix is verified for its local owner; other viewers retain the pre-existing stepped projectile fallback. No user Range sign-off is claimed.
