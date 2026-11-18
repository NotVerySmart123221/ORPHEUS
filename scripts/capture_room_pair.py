"""Collect two independent observations from each headless server on one machine."""

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
from dataclasses import asdict
from pathlib import Path

from golden_inputs import ReplayInputs, file_digest
from golden_reference import canonical, write_reference


def revision(root, paths):
    def git(*args):
        return subprocess.check_output(["git", "-C", root, *args], text=True).strip()

    if git("status", "--porcelain", "--untracked-files=normal", "--", *paths):
        raise ValueError("commit the adapter and simulation sources before recording")
    return git("rev-parse", "HEAD")


def validate_observation(path, inputs, bot_slot):
    data = json.loads(path.read_bytes())
    rows = data["ticks"]
    if data["status"] != "incomplete" or len(rows) != inputs.ticks:
        raise ValueError("expected one diagnostic row per offered tick")
    if type(data["bot_spawner_slot"]) is not int or data["bot_spawner_slot"] != bot_slot:
        raise ValueError("observer selected a different bot spawner")
    quantum = data["quanta_us"]
    if type(quantum) is not int or quantum <= 0:
        raise ValueError("unexpected observer clock")
    schedule = hashlib.sha256()
    for tick, row in enumerate(rows):
        if type(row["tick"]) is not int or row["tick"] != tick or row["elapsed_us"] != quantum * tick:
            raise ValueError("unexpected observed tick")
        offered = {k: row[k] for k in ("elapsed_us", "inputs", "missed", "tick")}
        schedule.update(canonical(offered) + b"\n")
    if schedule.hexdigest() != inputs.schedule_sha256:
        raise ValueError("observation does not contain the offered schedule")
    # Hash observations, including any recorded gaps, independently of formatting.
    return hashlib.sha256(canonical(data)).hexdigest()


def capture(args):
    schedule = args.schedule.absolute()
    runtime = args.runtime.absolute()
    out = args.out.resolve()
    if any(out.is_relative_to(p.resolve()) for p in
           (args.python_source, args.python_adapter, args.rust_source, runtime, Path(__file__).parents[1])):
        raise ValueError("choose a new private evidence directory outside source and runtime trees")
    out.mkdir(mode=0o700)
    python_adapter = args.python_adapter / "scripts/inspect_python_room.py"
    tracked = [
        (args.python_source, ["src", "tests", "pyproject.toml"]),
        (args.python_adapter, ["scripts"]),
        (args.rust_source, ["rust", "Cargo.toml", "Cargo.lock", ".cargo", "build.rs"]),
    ]
    revisions = [revision(root, paths) for root, paths in tracked]
    inputs = ReplayInputs.read(runtime, schedule)
    env = {k: v for k, v in os.environ.items() if not k.startswith("ORPHEUS_") and k != "PYTHONPATH"}
    env.update(ORPHEUS_RUNTIME_DIR=str(runtime), ORPHEUS_ROOM_SCHEDULE=str(schedule))
    results = {}
    for attempt in (1, 2):
        for role in ("reference", "candidate"):
            result = out / f"{role}-{attempt}.json"
            if role == "reference":
                command = [args.python, "-I", "-B", python_adapter, "--reference", args.python_source,
                           "--runtime", runtime, "--schedule", schedule, "--bot-slot", str(args.bot_slot),
                           "--private-output", result]
                cwd, expected = args.python_source, 2
            else:
                env.update(ORPHEUS_ROOM_OBSERVATION=str(result), ORPHEUS_ROOM_BOT_SLOT=str(args.bot_slot))
                command = ["cargo", "test", "--locked", "-p", "orpheus", "--lib", "same_schedule_practice_room",
                           "--", "--ignored"]
                cwd, expected = args.rust_source, 0
            with (out / f"{role}-{attempt}.log").open("xb") as log:
                proc = subprocess.run(command, check=False, cwd=cwd, env=env, stdout=log, stderr=subprocess.STDOUT)
            if proc.returncode != expected or not result.is_file():
                raise ValueError(f"{role} did not produce its diagnostic; see its private log")
            if ReplayInputs.read(runtime, schedule) != inputs:
                raise ValueError("inputs changed during recording")
            results[result.name] = validate_observation(result, inputs, args.bot_slot)
    if revisions != [revision(root, paths) for root, paths in tracked]:
        raise ValueError("source changed during recording")
    repeatable = all(results[f"{role}-1.json"] == results[f"{role}-2.json"]
                     for role in ("reference", "candidate"))
    receipt = {
        "status": "repeatable-diagnostics" if repeatable else "unstable-diagnostics",
        "inputs": asdict(inputs), "observation_sha256": results, "bot_spawner_slot": args.bot_slot,
        "platform": [platform.system(), platform.machine()],
        "python_source_rev": revisions[0], "python_adapter_rev": revisions[1], "rust_source_rev": revisions[2],
        "capture_sources_sha256": {p.name: file_digest(p).hex() for p in
                                   (Path(__file__), Path(__file__).with_name("golden_inputs.py"),
                                    Path(__file__).with_name("golden_reference.py"))},
    }
    if repeatable:
        reference = json.loads((out / "reference-1.json").read_bytes())
        if set(reference["missing"]) == {"provenance.effective_data_sha256"}:
            pending = out / "reference.pending.jsonl"
            with pending.open("xb") as stream:
                write_reference(stream, reference, inputs, revisions[0], revisions[1])
            with (out / "reference-validation.log").open("xb") as log:
                subprocess.run(["cargo", "run", "--locked", "--quiet", "-p", "orpheus-golden-trace",
                                "--bin", "compare_golden", "--", "--check-reference", pending],
                               check=True, cwd=Path(__file__).resolve().parents[1], env=env,
                               stdout=log, stderr=subprocess.STDOUT)
            pending.rename(out / "reference.jsonl")
            receipt["validated_reference_sha256"] = file_digest(out / "reference.jsonl").hex()
    with (out / "receipt.json").open("xb") as stream:
        stream.write(canonical(receipt) + b"\n")
    print(receipt["status"])
    return 0 if repeatable else 2


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("python-source", "python-adapter", "rust-source", "runtime", "schedule", "out"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--bot-slot", required=True, type=int)
    parser.add_argument("--python", default=sys.executable)
    args = parser.parse_args()
    for name in ("python_source", "python_adapter", "rust_source"):
        setattr(args, name, getattr(args, name).resolve())
    try:
        return capture(args)
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        parser.exit(2, f"Room capture incomplete ({type(error).__name__}). Check the private output directory.\n")


if __name__ == "__main__":
    sys.exit(main())
