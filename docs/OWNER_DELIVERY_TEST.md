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
startup onward. No datagrams, keys, profiles or chunks are written to disk.

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

This test is ignored in the public CI test run. The combined-check scripts
`scripts/check_combined_rust.sh` and `scripts/check_combined_rust.ps1` also run
the full ignored library suite when supplied a runtime; that suite has other
asset requirements and is not equivalent to the exact command above.
