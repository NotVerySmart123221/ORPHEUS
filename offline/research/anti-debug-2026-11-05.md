# Anti-debug investigation — 2026-11-05

Confirmed a destructive caller-integrity guard and defects in the hardware-watchpoint tool. **The specific hardware-breakpoint detector is not identified; usable hardware breakpoints and a Helix visual fix are not demonstrated.** No live debugger attached, game launched, game memory written, or executable patched during this investigation.

Later live follow-up: a dormant foreign breakpoint on an unused worker-thread slot survived 1- and 5-second trials without debugger attachment; the window thread already had four enabled slots. No detector was triggered. See [controlled live experiment](anti-debug-live-2026-11-05/REPORT.md). The statement above describes the initial static investigation only.

## Evidence and address convention

- Retail executable SHA256: `ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2`.
- Analyzed `ida/Overwatch.exe.i64` and `ida/Overwatch-state-analysis.dump.i64` through idalib. Existing database analysis was used without launching a fresh auto-analysis pass or saving database edits.
- Runtime snapshot SHA256: `c03c97057c5243a6d45577baf2919d4f3413b66de77bcdf5c77f86983596f4f4`.
- Snapshot image base: `0x7FF6D1220000`. Addresses below are **RVAs**; use the current process's module base when resolving them. Snapshot offsets equal RVAs.
- Graphify located prior protection observations and watcher code. Those observations guided inspection; they do not establish a detector's implementation.

## Confirmed: caller guard at RVA 0xB7C60

This helper mixes two caller-provided qwords with constants and a process-specific value read through `GS:[0x60]+0x110`. It also validates its return address. This is caller-integrity protection, not yet evidence of a DR0–DR7 inspection.

| RVA | Observed operation |
| --- | --- |
| `0xB7CAF` | Read PEB pointer from `GS:[0x60]`. |
| `0xB7CFF` | Read qword at PEB `+0x110`; rotate right 3, add `0x316BFB92484F568D`, XOR into first argument. Field name not established here. |
| `0xB7D31` | Load return address from `[rsp]`. |
| `0xB7D3A–0xB7D55` | Require return address at/above image `+0x1000`, with displacement no greater than `0x25B2808`. |
| `0xB7DD4` | Require byte at return address minus 5 to be `0xE8` (direct relative call). |
| `0xB7DDA–0xB7DEB` | Sign-extend call displacement and require its target to equal helper entry. |
| `0xB7E3F–0xB7E5B` | Failure: reload PEB, load pointer at `+0x18`, zero `0x1000` bytes through it using `rep stosb`. |
| `0xB7EA1` | Success: XOR second argument with `0x316BFB92484F568C`, restore RDI and return. |

Microsoft's documented x64 PEB layout identifies `+0x18` as the loader-data pointer. Thus the failure path destroys loader data and surrounding memory; its exact live crash timing is untested. [Microsoft PEB definition](https://learn.microsoft.com/en-us/windows/win32/api/winternl/ns-winternl-peb)

The helper bytes `0xB7C60–0xB7EB3` are identical in both existing runtime snapshots. Opaque branches conceal real instruction boundaries: for example, `clc; shl ah,0; jae` reaches `0xB7CAF`, and complementary conditional branches reach `0xB7CFF`. Linear disassembly crosses junk bytes; current Hex-Rays output is incomplete. A syntactically visible `rdtsc` in skipped bytes is **not** proof of a timing detector.

### Isolated execution check

Executed the captured machine code in Unicorn with synthetic TEB, PEB, loader page, arguments and stack. No game process or real loader structures were accessed. Temporary harness: `.scratchpad/verify_caller_guard.py`; disposable, not a supported tool.

Command: `python .scratchpad/verify_caller_guard.py`

| Case | Result |
| --- | --- |
| Actual direct call at `0x24A335F`, return `0x24A3364` | Loader page unchanged; helper returns; both argument transformations match. |
| Return address outside game text | Entire synthetic 4 KB loader page zeroed. |
| In-image return, preceding call opcode changed in emulator only | Entire loader page zeroed. |
| Real `E8` call at `0x250B2D8` targeting another function | Entire loader page zeroed. |

All assertions passed. Failure cases stop immediately after the zeroing path; no claim about subsequent live execution. This validates the guard independently of the previously observed debugger crashes.

Implication: invoking protected helpers from injected or debugger-owned call sites can trigger destruction even if debugger-presence checks are concealed. Existing `tools/research/live_emu.py` already supplies real call-site return addresses in an emulator; this guard explains why that convention matters. This finding does not establish that hardware traps trigger this particular guard.

## Confirmed: current dump omits import trampolines

Retail IDB exposes only `MessageBoxW` and `EnumerateLoadedModules64` as ordinary named imports. Searches in the runtime image did not find names for `NtSetInformationThread`, `NtGetContextThread`, `NtSetContextThread`, `GetThreadContext`, `SetThreadContext`, `IsDebuggerPresent`, `CheckRemoteDebuggerPresent`, `NtContinue` or `AddVectoredExceptionHandler`. Their absence does **not** establish absence of those operations.

Runtime indirect-call entries include:

| Image RVA | Captured target VA |
| --- | --- |
| `0x25B4328` | `0x1CB37B606AD` |
| `0x25B45C8` | `0x1CB37BE0524` |
| `0x25B48D8` | `0x1CB37C70536` |

These targets lie outside the captured image range `[0x7FF6D1220000, 0x7FF6D49B9000)`. Their code and destination APIs cannot be reconstructed from these image-only dumps. Resolving them needs a capture containing those allocations from the **same process**, or a new bounded read-only capture of the corresponding live allocations. Old heap addresses must not be reused in a new process.

## Watchpoint failure has competing causes

The prior health experiment recorded zero hits, failed disarming and a later access violation. Earlier research labels hardware registers detected, thread hiding, and a suspension watchdog; these remain historical observations/hypotheses rather than newly traced implementations. Error 87 from `VirtualProtect` alone also does not identify an ACG policy or prove every debugging approach impossible.

Current `hw_watch.py` overwrites original debug-register state, retains exited thread handles, consumes unowned single-step/breakpoint exceptions, and ignores important API failures. Cleanup runs after continuing the final debug event and has no verified restoration transaction. Event thread handles do have documented context and suspension rights. Full evidence and Microsoft citations: [debugger API review](debugger-api-review.md).

Before another live watchpoint attempt, repair and verify that tool against a disposable process: preserve/restore original DR state; track live handle ownership; pass unowned exceptions through; record API errors and register readback; coordinate restoration while stopped. This is prerequisite instrumentation work, not a demonstrated anti-debug bypass.

Then distinguish attachment-only failure, suspension-only failure, and register-arming failure with separate controlled observations. A changing DR readback would support register clearing; a traced native thread-hide call would support thread hiding. Neither was observed here.

## Helix relevance and limits

No evidence connects this caller guard to missing Helix world visuals. Existing `research/statescript-payloads.md` records that 33 ms pulses never activated client states and that holding them for 1 s did; `research/bot-fire.md` records bot visibility/placement uncertainty. Keep those protocol/presentation failures separate from debugger failure.

Next protection target: recover the private import/exception machinery omitted from the snapshot, then identify the actual context inspection/reset or thread-hide path. No detector patch, live hardware breakpoint, protection bypass, or Helix restoration is claimed by this report.
