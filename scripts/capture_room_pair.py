"""Collect two independent observations from each headless server on one machine."""

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from dataclasses import asdict
from pathlib import Path

from golden_inputs import ReplayInputs, file_digest
from golden_reference import canonical, write_trace


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


NATIVE_TEST = "game_service::tests::same_schedule::same_schedule_practice_room"
ROOT = Path(__file__).resolve().parents[1]


def built_artifact(records, manifest, target, is_test):
    found = []
    for line in records:
        entry = json.loads(line)
        if (entry.get("reason") == "compiler-artifact" and entry.get("executable")
                and Path(entry["manifest_path"]).resolve() == manifest.resolve()
                and entry["target"]["name"] == target and entry["profile"]["test"] is is_test
                and ("lib" if is_test else "bin") in entry["target"]["kind"]):
            found.append(Path(entry["executable"]))
    if len(found) != 1:
        raise ValueError("build did not return exactly one requested executable")
    return found[0]


def build_once(cwd, private_dir, out, env, native):
    if native:
        kind, target, manifest = "native", "orpheus", cwd / "Cargo.toml"
        args = ["test", "--no-run", "-p", "orpheus", "--lib"]
    else:
        kind, target, manifest = "validator", "compare_golden", cwd / "rust/crates/golden-trace/Cargo.toml"
        args = ["build", "-p", "orpheus-golden-trace", "--bin", target]
    with (out / f"{kind}-build.jsonl").open("xb") as records, (out / f"{kind}-build.log").open("xb") as log:
        subprocess.run(["cargo", *args, "--locked", "--message-format=json"], cwd=cwd, env=env,
                       stdout=records, stderr=log, check=True)
    with (out / f"{kind}-build.jsonl").open("rb") as records:
        executable = built_artifact(records, manifest, target, native)
    frozen = private_dir / (kind + executable.suffix)
    shutil.copy2(executable, frozen)
    return frozen


def bind_role(out, validator, role, observation, inputs, revisions, platform_name, toolchain):
    if set(observation["missing"]) - {"provenance.effective_data_sha256"}:
        return None
    pending = out / f"{role}.pending.jsonl"
    destination = out / f"{role}.jsonl"
    try:
        with pending.open("xb") as stream:
            write_trace(stream, observation, inputs, role=role,
                        source_revision=revisions[0] if role == "reference" else revisions[2],
                        reference_revision=revisions[0],
                        adapter_revision=revisions[1] if role == "reference" else revisions[2],
                        platform=platform_name, toolchain=toolchain)
        with (out / f"{role}-validation.log").open("xb") as log:
            subprocess.run([validator, f"--check-{role}", pending], stdout=log, stderr=subprocess.STDOUT, check=True)
        pending.rename(destination)
        return file_digest(destination).hex()
    finally:
        pending.unlink(missing_ok=True)


def recording_environment(runtime, schedule):
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("ORPHEUS_", "OW174_")) and key != "PYTHONPATH"}
    env.update(ORPHEUS_RUNTIME_DIR=str(runtime), ORPHEUS_ROOM_SCHEDULE=str(schedule))
    return env


def capture(args):
    schedule = args.schedule.absolute()
    runtime = args.runtime.absolute()
    out = args.out.resolve()
    if any(out.is_relative_to(p.resolve()) for p in
           (args.python_source, args.python_adapter, args.rust_source, runtime, Path(__file__).parents[1])):
        raise ValueError("choose a new private evidence directory outside source and runtime trees")
    out.mkdir(mode=0o700)
    tracked = [
        (args.python_source, ["src", "tests", "pyproject.toml"]),
        (args.python_adapter, ["scripts"]),
        (args.rust_source, ["rust", "fixtures", "Cargo.toml", "Cargo.lock", ".cargo", "build.rs"]),
        (ROOT, ["scripts", "rust/crates/golden-trace", "Cargo.toml", "Cargo.lock", ".cargo"]),
    ]
    revisions = [revision(root, paths) for root, paths in tracked]
    inputs = ReplayInputs.read(runtime, schedule)
    env = recording_environment(runtime, schedule)
    with tempfile.TemporaryDirectory(prefix="orpheus-capture-") as tmp:
        private_dir = Path(tmp)
        native = build_once(args.rust_source, private_dir, out, env, native=True)
        validator = build_once(ROOT, private_dir, out, env, native=False)
        toolchain = subprocess.check_output([env.get("RUSTC", "rustc"), "--version"], cwd=args.rust_source,
                                            env=env, text=True).split()[1]
        results = record_attempts(args, out, inputs, env, native)
        if revisions != [revision(root, paths) for root, paths in tracked]:
            raise ValueError("source changed during recording")
        repeatable = {role: results[f"{role}-1.json"] == results[f"{role}-2.json"]
                      for role in ("reference", "candidate")}
        receipt = {
            "status": "incomplete" if all(repeatable.values()) else "unstable-diagnostics",
            "repeatable": repeatable, "inputs": asdict(inputs), "observation_sha256": results,
            "bot_spawner_slot": args.bot_slot, "platform": [platform.system(), platform.machine()],
            "python_source_rev": revisions[0], "python_adapter_rev": revisions[1], "rust_source_rev": revisions[2],
            "capture_source_rev": revisions[3], "native_test_sha256": file_digest(native).hex(), "validator_sha256": file_digest(validator).hex(),
            "capture_sources_sha256": {p.name: file_digest(p).hex() for p in
                                       (Path(__file__), Path(__file__).with_name("golden_inputs.py"),
                                        Path(__file__).with_name("golden_reference.py"))},
        }
        return finish_receipt(out, validator, inputs, revisions, "rustc-" + toolchain, receipt)


def record_attempts(args, out, inputs, env, native):
    results = {}
    for attempt in (1, 2):
        for role in ("reference", "candidate"):
            run_env = env
            result = out / f"{role}-{attempt}.json"
            if role == "reference":
                command = [args.python, "-I", "-B", args.python_adapter / "scripts/inspect_python_room.py", "--reference", args.python_source,
                           "--runtime", args.runtime.absolute(), "--schedule", args.schedule.absolute(), "--bot-slot", str(args.bot_slot),
                           "--private-output", result]
                cwd, expected = args.python_source, 2
            else:
                run_env = env | {"ORPHEUS_ROOM_OBSERVATION": str(result), "ORPHEUS_ROOM_BOT_SLOT": str(args.bot_slot)}
                command = [native, NATIVE_TEST, "--ignored", "--exact"]
                cwd, expected = args.rust_source, 0
            with (out / f"{role}-{attempt}.log").open("xb") as log:
                proc = subprocess.run(command, check=False, cwd=cwd, env=run_env, stdout=log, stderr=subprocess.STDOUT)
            if proc.returncode != expected or not result.is_file():
                raise ValueError(f"{role} did not produce its diagnostic; see its private log")
            if ReplayInputs.read(args.runtime, args.schedule) != inputs:
                raise ValueError("inputs changed during recording")
            results[result.name] = validate_observation(result, inputs, args.bot_slot)
    return results


def finish_receipt(out, validator, inputs, revisions, rust_toolchain, receipt):
    validated = {}
    for role, stable in receipt["repeatable"].items():
        if stable:
            try:
                data = json.loads((out / f"{role}-1.json").read_bytes())
                platform_name = "-".join(data["platform"] if role == "reference" else receipt["platform"]).lower()
                toolchain = "python-" + data["toolchain"] if role == "reference" else rust_toolchain
                digest = bind_role(out, validator, role, data, inputs, revisions, platform_name, toolchain)
            except (ValueError, TypeError, KeyError, subprocess.CalledProcessError):
                receipt.setdefault("invalid_roles", []).append(role)
            else:
                if digest is not None:
                    validated[role] = digest
                    receipt[f"validated_{role}_sha256"] = digest
    code = 2
    if len(validated) == 2:
        report_path = out / "comparison.json"
        with (out / "comparison.log").open("xb") as log:
            result = subprocess.run([validator, out / "reference.jsonl", out / "candidate.jsonl",
                                     "--report", report_path], check=False, stdout=log, stderr=subprocess.STDOUT)
        report = json.loads(report_path.read_bytes())
        status = report["result"]
        if {"match": 0, "mismatch": 1, "incompatible": 2, "incomplete": 2}.get(status) != result.returncode:
            raise ValueError("unexpected comparator status")
        receipt["status"] = status
        if status == "incompatible":
            receipt["incompatible_fields"] = report["fields"]
        code = result.returncode
    with (out / "receipt.json").open("xb") as stream:
        stream.write(canonical(receipt) + b"\n")
    print(receipt["status"])
    return code


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
