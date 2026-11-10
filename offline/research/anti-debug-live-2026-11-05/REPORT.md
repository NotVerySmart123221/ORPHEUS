# Hardware-breakpoint detector experiment — 2026-11-05

**No detector reaction reproduced.** A dormant hardware breakpoint remained armed on a worker thread for 1 and 5 seconds without debugger attachment. The game survived, the watch remained intact, and original worker registers were restored exactly. The window thread already had four occupied, enabled slots.

## Method

Used `tools/research/hw_detector_probe.py`, not the defective `hw_watch.watch()` event loop. Reused aligned context storage and DR7 encoding only. The probe validates executable hash, thread ownership and read-only header protection; requests debug-register context only; uses independently opened handles; and checks restoration before resuming. It never calls `DebugActiveProcess`, writes game memory, patches code, or replaces occupied slots in arm mode.

Watched the first 4 bytes of the read-only PE header, **write-only**, with local DR0 enable. Normal header reads do not trap. This tests whether merely arming a foreign watch is detected, not delivery or handling of an actual hardware exception.

Native disposable-process controls passed first: suspension/readback, unchanged DR writes, and dormant arming. Exact original registers were independently verified after each mode. Temporary harness: `.scratchpad/native_hw_probe_check.py` (disposable).

## Window-thread observations

Initialized run PID `16004`, window TID `13856`, module base `0x7FF71C0A0000`:

| Register | Before intervention |
| --- | --- |
| DR0 | `0x1E464324560` |
| DR1 | `0x7FF71EF82850` (image RVA `0x2EE2850`) |
| DR2 | `0x1E461C707A4` |
| DR3 | `0x7FF71F36FF88` (image RVA `0x32CFF88`) |
| DR6 | `0xFFFF0FF0` |
| DR7 | `0xFFFF0455` |

All four local slots are enabled for 4-byte read/write access. A fresh process also had four occupied window-thread slots. This establishes pre-existing state; the setters, handlers and purpose of those slots remain untraced.

- Initial safety refusal logged state without writing context: [read-native-1.jsonl](read-native-1.jsonl).
- Read-only suspension survived one second with identical state: [read-native-2.jsonl](read-native-2.jsonl).
- Unchanged-value write preserved DR0–DR3 and breakpoint configuration. Raw DR6 became `0`; DR7 became `0xFFFF0055`: [same-native-1.jsonl](same-native-1.jsonl). Initial raw-equality comparison reported restoration mismatch. That was a probe-validation defect, not a game crash. Windows documents sanitization of OS-controlled fields. Comparison now checks addresses, enables, type/length and classic trap-status bits, while logging raw values. [Microsoft SetThreadContext](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setthreadcontext)
- That game survived until the launcher's 240-second timeout. No foreign watch was added to its window thread.

## Worker-thread experiment

Fresh run PID `10400`, module base `0x7FF71C0A0000`, launcher log `research/main-menu/20261105T135720.643417Z/`.

Bounded read-only inventory sampled 24 threads. Many workers had empty debug state; one refused suspension with error 156 and was not modified. An initially selected startup worker exited before testing (`OpenThread` error 87), so no watch was armed there. Refreshed inventory selected live TID `2492`. Evidence: [inventory 1](thread-inventory-1.jsonl), [inventory 2](thread-inventory-2.jsonl).

| Test on TID 2492 | Duration | Result |
| --- | --- | --- |
| Suspension/readback | 1 second | Survived; registers stayed zero. |
| Write unchanged zero debug state | 1 second | Survived; exact restoration verified. |
| Arm DR0=`0x7FF71C0A0000`, DR7=`0xD0001` | 1 second | Survived; immediate/final readback matched; original zero state restored. |
| Same dormant watch | 5 seconds | Survived; watch remained present; original zero state restored. |

Evidence: [read](worker-read-2.jsonl), [unchanged write](worker-same-2.jsonl), [one-second watch](worker-arm-2.jsonl), [five-second watch](worker-arm-5s.jsonl). This game also survived until its launcher's normal 300-second timeout; neither initialized run ended with a protection crash.

Five-second trial command, run outside the sandbox:

```powershell
python tools/research/hw_detector_probe.py 10400 arm research/anti-debug-live-2026-11-05/worker-arm-5s.jsonl --seconds 5 --thread 2492
```

PIDs, TIDs and pointers above are historical, not reusable invocation values. Output files are exclusive-created; the command intentionally refuses to overwrite existing evidence.

## Interpretation and limits

These bounded trials contradict the universal claim that any one-shot hardware breakpoint immediately kills this client. They do not exclude detection specific to debugger attachment, actual hardware exceptions, gameplay state, other threads/addresses, or longer intervals.

The old watcher's behavior is a concrete confound: it overwrites all four window-thread slots and later zeroes them, despite pre-existing configuration. Whether that disruption, attachment, swallowed exceptions or another mechanism caused its earlier crash remains unproven. Do not retry it unchanged.

The caller guard at RVA `0xB7C60` remains a separate emulator-verified finding. This experiment did not invoke it. No actual breakpoint hit, exception-dispatch trace, detector branch address, bypass or Helix fix is demonstrated. Tests ran at the startup screen, not inside a loaded match.

The first sandboxed launch failed CASC initialization (`SHMEM_TIMEOUT`) before any register modification. Launcher and live probes needed access outside the sandbox; sandboxed window lookup could not see the initialized native game window. Those startup/refusal records are not anti-debug detections.

Validation: `python -m pytest tools/research -q` passed 55 tests; disposable native controls passed. Initial suspension-duration fields used a coarse clock and rounded to zero, so they are not timing evidence. The tool now uses a high-resolution performance counter for that field.
