# Prepared owner delivery observation

Run the opt-in test with a prepared target-tagged 104319 Practice Range runtime
that does not contain bot VM graphs:

```sh
ORPHEUS_RUNTIME_DIR=/path/to/prepared/runtime \
  cargo test --locked -p orpheus --lib \
  game_service::tests::same_schedule::transport::prepared_owner_post_startup_loss_receipts \
  -- --exact --ignored --nocapture
```

Confirm that output includes both `running 1 test` and `1 passed; 0 failed`, and
the completion marker `completed: acknowledged H0 and post-startup delta`.
A successful command with `running 0 tests` is **not** a passing test. In
PowerShell, set `$env:ORPHEUS_RUNTIME_DIR` to the runtime path and run the same
`cargo test` arguments without the shell environment-assignment prefix.

The test requires the completion marker as well as one passing test. It fails
if the required assets, acknowledged owner H0, or later delivery observations
are absent. On an in-process fixed-clock encrypted loopback it captures actual
owner chunks in memory, including their wire header and exact bit length, from
startup onward. In the ordinary mode no datagrams, keys, profiles or chunks are
written to disk.

After startup, it queues one command per tick directly into the room input
queue, confirms an acknowledged post-startup owner delta, presses Sprint with
forward at elapsed tick 64, and, once that delta is acknowledged, drops the
first eligible owner-script datagram at or after tick 75 from the receive
window. The acknowledged delta must belong to the same BodyId and end before
the lost range. It accepts
the next actual datagram and sends its normal encrypted ACK. The test requires
server receipts identifying the lost and arrived records for the same BodyId
and their actual packet sequences and chunk ends. In this schedule, S+1 already
contains a cumulative owner chunk covering S; it is accepted, not suppressed.
The no-loss receive-window control shows accepting S would instead mark it
arrived; receipt checks also reject missing loss or arrival observations.

This extends startup loss coverage to later command-driven packets. It does not
decode Sprint state or prove semantic receiver convergence, retransmission
after the loss report, command ingress, original-client behavior, Windows, or a
900-frame session. It is not completion of issue #166.

## Receiver semantic convergence

With the same prepared runtime and a clean Python checkout at the pinned
reference used by `scripts/check_sprint_wire.py`, run:

```sh
python -I -S -B scripts/check_receiver_convergence.py \
  --python-reference /path/to/python/reference --runtime /path/to/prepared/runtime
```

This opt-in mode reuses the encrypted fixed-clock test and its authenticated
loss and arrival receipt checks. It presses Sprint at elapsed tick 76, drops
the first eligible packet at or after that tick, and stops moving at tick 110.
The Python reader applies only accepted chunks in sequence, from the exact bits
of the decrypted packets. A separate copy of the pre-loss receiver decodes S:
the Sprint root's 699 is absent or zero before S, S writes integer 1 with no
bindings, S is not accepted, and accepted S+1 writes 1. The later accepted
stream writes 0. Lifecycle, bounds and cumulative range guards are checked.
Suppressing S+1 only in the reader replay must fail the recovery semantic check;
the actual transport and ACK schedule remains unchanged. Private temporary
captures are removed automatically by this wrapper. The Rust test's
`ORPHEUS_RECEIVER_PROBE` mode itself uses `fs::write` at the supplied path and
does not provide private permissions or cleanup: do not invoke it with a public
or persistent path. The wrapper uses a private temporary directory.

The checker rejects a non-increasing owner CmFD before applying the chunk, as
the pinned statescript reader specifies, and rejects late full frames. It checks
that 699 is absent before loss, changes exactly once to 1 in S+1, remains 1
until a single 0 after release, and remains 0 through the final observation.
The probe records the last actual owner chunk and the final processed command
frame after the complete schedule; truncation immediately after the stop is
rejected. Controls mutate only the encoded CmFD of actual captured chunks for
S+1 and an earlier owner packet and reject both as stale. Synthetic controls
cover an early 1-to-0 pair and graph/codec symlinks escaping the runtime.

Run the public synthetic suite with `python -B -m unittest discover -s
scripts/tests`; the receiver guard tests require no runtime. The existing
pinned-reader synthetic tests skip unless `ORPHEUS_PYTHON_REFERENCE` is set;
the prepared receiver wrapper above is always explicitly opt-in.

This demonstrates cumulative delivery
before the loss report, not retransmission after that report; a broken
retransmission or ACK negative control remains required for issue #166. It
does not exercise command ingress, Windows, or the original client.

This test is ignored in the public CI test run. The combined-check scripts
`scripts/check_combined_rust.sh` and `scripts/check_combined_rust.ps1` also run
the full ignored library suite when supplied a runtime; that suite has other
asset requirements and is not equivalent to the exact command above.

## Observe a short weapon volley

With a prepared target-tagged Practice Range runtime, run:

```sh
cargo test --locked -p orpheus --lib stock_practice_owner_rifle_batched_tap_delivers_terminal_ammo -- --ignored
```

The probe submits adjacent press and release commands in one input batch.
It checks the posed shot and the acknowledged owner projection after the
batch. An inactive volley and lower ammunition in the final projection are
compatible with an earlier shot on the batch's press frame. The probe
cannot verify first-person animation, which the client draws locally.

The prepared `stock_practice_owner_gap_does_not_charge_for_an_unposed_rifle_round`
probe covers the next rifle round falling exactly on a missing command
frame. It checks that the skipped round spends no ammunition and that a
subsequent posed press fires and spends one round. The public synthetic
`a_volley_skips_unposed_rounds_and_bills_the_next_posed_round` test checks
the elapsed clock and ammo through a gap within the same volley.

To collect evidence for an intermittent animation failure, run an Inspector
build with `ORPHEUS_TRACE_OWNER_VOLLEYS=1` set in the server environment.
Set `ORPHEUS_OWNER_TRACE_FILE` to keep the trace in a separate file. The
server otherwise writes the trace to stderr. The bounded trace queue
reports how many rows were dropped if the output falls behind.
Correlate the `owner-volley` rows by body and command frame. Input rows include
button edges, held-button throttle changes, and unposed gap boundaries. Shot rows carry
their authored frame and corresponding pose frame when available. Delta
rows show the sent activation, its active flag, and explicitly written
ammunition. Delivery rows show whether the transport acknowledged each
chunk. Record when the animation fails so the relevant rows can be matched
to the same tap.
