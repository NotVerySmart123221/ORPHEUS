"""Run an offline Soldier input schedule against the pinned Python room.

Schedule rows are JSON lines with tick, elapsed_us, missed and inputs, in the
golden comparison format. Output is a private incomplete diagnostic: the
immediate-ack fixture does not observe grouped sends or establish reload state.
"""

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

REFERENCE = "45044d0d62399d9eb876c08e788aaea1f56cecc9"
ROOT = Path(__file__).resolve().parents[1]


def git(source, *arguments):
    return subprocess.check_output(
        ["git", "-C", str(source), *arguments], text=True, encoding="utf-8",
    ).strip()


def decoded_command(inputs, room):
    from ow174.game.commands import SUBFRAME, Command

    if not isinstance(inputs, list) or len(inputs) > 1:
        raise ValueError("expected at most one decoded command per tick")
    if not inputs:
        return None
    value = inputs[0]
    keys = {"frame", "forward", "right", "yaw", "pitch", "buttons", "extra", "action", "entity", "click"}
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError("invalid schedule command fields")
    if value["entity"] != f"player/0/body/{room.player_generation}":
        raise ValueError("input targets a different body generation")
    for key, low, high in (
        ("frame", 0, 2**32 - 1), ("buttons", 0, 0xFFFFFF), ("action", 0, 255), ("extra", 0, 65535),
        ("forward", -128, 127), ("right", -128, 127),
        ("yaw", -(2**31), 2**31 - 1), ("pitch", -(2**31), 2**31 - 1),
    ):
        v = value[key]
        if type(v) is not int or not low <= v <= high:
            raise ValueError(f"invalid decoded command field {key}")
    fields = {k: v for k, v in value.items() if k not in {"entity", "click"}}
    click = value["click"]
    if bool(value["buttons"] & SUBFRAME) != (click is not None):
        raise ValueError("click sample must follow the command's subframe flag")
    if click is not None:
        if not isinstance(click, list) or len(click) != 3 or not all(type(v) is int for v in click):
            raise ValueError("invalid click sample")
        if not 0 <= click[0] <= 255 or any(not -32768 <= v <= 32767 for v in click[1:]):
            raise ValueError("invalid click sample")
        fields.update(subframe=click[0], click_yaw=click[1], click_pitch=click[2])
    return Command(**fields)


def record(reference, runtime, schedule, bot_slot):
    if git(reference, "rev-parse", "HEAD") != REFERENCE:
        raise ValueError("expected the documented Python reference revision")
    if git(reference, "status", "--porcelain", "--", "src", "tests"):
        raise ValueError("expected a clean Python reference source and test tree")
    manifest = runtime / "manifest.json"
    if not manifest.is_file():
        raise ValueError("prepared runtime manifest is missing")
    os.environ["ORPHEUS_RUNTIME_DIR"] = str(runtime)
    sys.path[:0] = [str(ROOT / "scripts"), str(reference / "src"), str(reference)]

    from golden_python_room import PythonRoom

    from ow174 import paths

    if reference != paths.SOURCE_CHECKOUT or runtime != paths.RUNTIME_DIR:
        raise ValueError("unexpected imported reference or runtime directory")
    room = PythonRoom(bot_slot)
    ticks = []
    last_input = None
    missing = {"provenance.effective_data_sha256"}
    with schedule.open(encoding="utf-8") as stream:
        for number, line in enumerate(stream):
            row = json.loads(line)
            if not isinstance(row, dict) or set(row) != {"tick", "elapsed_us", "missed", "inputs"}:
                raise ValueError(f"invalid schedule row at tick {number}")
            expected = (number, number * room.player.quanta, 0)
            clock = tuple(row[key] for key in ("tick", "elapsed_us", "missed"))
            if any(type(value) is not int for value in clock) or clock != expected:
                raise ValueError(f"unexpected schedule clock at tick {number}")
            command = decoded_command(row["inputs"], room)
            if command is not None:
                generation = room.player_generation
                if (last_input is not None and last_input[0] == generation
                        and command.frame != (last_input[1] + 1) % 2**32):
                    raise ValueError(f"noncontiguous command frame at tick {number}")
                last_input = (generation, command.frame)
            accepted, (state, gaps) = room.step(command)
            ticks.append({**row, "accepted": accepted if command is not None else None,
                          "state": state, "missing": gaps})
            missing.update(gaps)
    if not ticks:
        raise ValueError("schedule is empty")
    adapter_sources = (Path(__file__), Path(__file__).with_name("golden_python_room.py"))
    return {
        "status": "incomplete", "scope": "python-offline-room-state",
        "source_rev": REFERENCE, "manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
        "adapter_sources_sha256": {
            p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in adapter_sources
        },
        "platform": [platform.system(), platform.machine()], "toolchain": platform.python_version(),
        "quanta_us": room.player.quanta,
        "bot_spawner_slot": bot_slot, "transport": "immediate-ack combat fixture",
        "missing": sorted(missing), "ticks": ticks,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", required=True, type=Path)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--schedule", required=True, type=Path)
    parser.add_argument("--bot-slot", required=True, type=int)
    parser.add_argument("--private-output", required=True, type=Path)
    args = parser.parse_args()
    reference, runtime, output = (p.resolve() for p in (args.reference, args.runtime, args.private_output))
    if any(output.is_relative_to(p) for p in (ROOT, reference, runtime)):
        parser.error("write observations outside the source checkouts and runtime pack")
    if output.exists():
        parser.error("choose a new private output file")
    try:
        result = record(reference, runtime, args.schedule, args.bot_slot)
        data = json.dumps(result, allow_nan=False, sort_keys=True, separators=(",", ":"))
        with output.open("x", encoding="utf-8", newline="\n") as dest:
            dest.write(data + "\n")
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(2, f"{error}\n")
    print("incomplete")
    return 2


if __name__ == "__main__":
    sys.exit(main())
