"""Compare private Rust startup observations with a Python offline room run.

The result keeps decode and strict startup results separate. It contains no
frame payloads, variable values, warning text, or original-client evidence.
"""

import argparse
import hashlib
import json
import logging
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_INPUTS = frozenset({
    "collision/0688.col",
    "data/ai_spawners_174.json",
    "data/collision_props_174.json",
    "data/game_graphs_174.json.gz",
    "data/game_heroes_174.json",
    "data/game_movers_174.json",
    "data/game_stats_174.json",
    "data/statescript_codecs_174.json",
})
SCENARIO = "practice-owner-startup-v2"
START_FRAME = 1000
PROBE_ROSTER = {"kind": "synthetic_admitted_players", "team_slots": [0], "free_for_all": False}
WARNING_KINDS = {
    "[script] config var %s is not computed on the server: null": "missing_native_config",
    "[script] node class %s has no model: it only follows m_outPlug": "unmodeled_action",
    "[script] state class %s is on but cannot be sent": "omitted_active_state",
    "[script] expression opcode %d is not modelled: null": "unmodeled_operator",
    "[script] expression %s could not run (%s): null": "expression_failed",
}


def file_sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_runtime_inputs(runtime, inputs):
    if not isinstance(inputs, dict) or inputs.keys() != EXPECTED_INPUTS:
        raise ValueError("Rust report must identify every Practice Range startup input")
    root = runtime.resolve(strict=True)
    for name, record in sorted(inputs.items()):
        path = (root / name).resolve()
        if not path.is_relative_to(root) or not path.is_file():
            raise ValueError(f"unsafe or missing runtime input {name}")
        if not isinstance(record, dict) or record.keys() != {"sha256"}:
            raise ValueError(f"missing digest for runtime input {name}")
        if record["sha256"] != file_sha256(path):
            raise ValueError(f"different bytes for runtime input {name}")


def merge_actor_rows(rust_rows, python_rows):
    def by_hero(rows):
        if not isinstance(rows, list) or not rows:
            raise ValueError("empty or invalid actor inventory")
        result = {}
        for row in rows:
            if not isinstance(row, dict):
                raise ValueError("invalid actor row")
            for key in ("hero", "body"):
                value = row.get(key)
                if not isinstance(value, str) or not re.fullmatch(r"0x[0-9A-F]{16}", value):
                    raise ValueError("invalid hero or body identity")
            if row["hero"] in result:
                raise ValueError("duplicate actor identity")
            result[row["hero"]] = row
        return result

    rust, python = by_hero(rust_rows), by_hero(python_rows)
    if rust.keys() != python.keys() or any(rust[key]["body"] != python[key]["body"] for key in rust):
        raise ValueError("different hero or body inventories")
    return [
        {
            "hero": key,
            "body": rust[key]["body"],
            "python": {name: value for name, value in python[key].items() if name not in {"hero", "body"}},
            "rust_bootstrap": rust[key]["bootstrap"],
            "rust_room_factory": rust[key]["admission"],
        }
        for key in sorted(rust)
    ]


class WarningCounts(logging.Handler):
    def __init__(self):
        super().__init__()
        self.counts = Counter()

    def emit(self, record):
        self.counts[WARNING_KINDS.get(record.msg, "other_script_notice")] += 1


def python_rows(reference, runtime):
    """Reuse the reference's in-memory client and production body switch."""
    os.environ["ORPHEUS_RUNTIME_DIR"] = str(runtime)
    os.environ.pop("ORPHEUS_GRAPH_BUNDLE", None)
    sys.path[:0] = [str(reference / "src"), str(reference)]
    from ow174 import paths
    from ow174.game import props
    from ow174.game.collision import StaticCollisionProvider, World, cache_path
    from ow174.game.commands import Command
    from ow174.game.content import PRACTICE_RANGE, heroes
    from ow174.game.match import Match
    from ow174.game.script import decode, expr, graph
    from tests.runtime.test_game_combat import in_world

    if paths.RUNTIME_DIR != runtime or paths.SOURCE_CHECKOUT != reference:
        raise ValueError("unexpected Python source or runtime location")
    if graph.GRAPHS_PATH.resolve() != (runtime / "data/game_graphs_174.json.gz").resolve():
        raise ValueError("unexpected Python graph input")
    mode = PRACTICE_RANGE
    cache = cache_path(mode.map_guid).resolve()
    if cache != (runtime / "collision/0688.col").resolve():
        raise ValueError("unexpected Python Practice Range collision input")
    collision = StaticCollisionProvider(World.load(cache, props.mode_identifiers(mode.mode_guid)))

    events = WarningCounts()
    log = logging.getLogger("ow174.script")
    log.setLevel(logging.INFO)
    log.propagate = False
    log.addHandler(events)
    rows = []
    for hero in sorted(heroes().values(), key=lambda item: item.guid):
        events.counts.clear()
        expr._warned.clear()
        row = {
            "hero": f"0x{hero.guid:016X}",
            "body": f"0x{hero.body:016X}",
            "status": "failed",
            "stage": "room_selection",
        }
        rows.append(row)
        try:
            match = Match(mode, collision_provider=collision)
            match.tick = START_FRAME
            player = match.add_player(1, "Probe", hero.guid, 0, False)
            in_world(match, player)
            if not match.switch_hero(player, hero):
                continue
            row["stage"] = "script_start"
            script = player.body_script
            if script is None:
                continue
            row["strict"] = script.component.strict
            row["stage"] = "initial_tree"
            client = decode.ClientBody(item.get("graph") for item in graph.bodies()[hero.body]["graphs"])
            [tree] = script.frames()
            initial = client.apply(tree)
            if not initial.full or initial.owner:
                continue
            row["stage"] = "first_owner"
            script.stream.arrived(tree.chunk_last)
            player.take_commands([Command(START_FRAME + 1)])
            [owner] = script.frames()
            first = client.apply(owner)
            if not first.owner or first.correction or first.cmfd != START_FRAME + 1:
                continue
            row.update(status="decoded", stage="first_owner")
        except Exception as error:
            row["error_type"] = type(error).__name__
        finally:
            row["script_notice_kinds"] = dict(sorted(events.counts.items()))
    return rows


def git(reference, *args):
    return subprocess.check_output(["git", "-C", str(reference), *args], encoding="utf-8").strip()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-reference", required=True, type=Path)
    parser.add_argument("--python-revision", required=True)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--rust-report", required=True, type=Path)
    parser.add_argument("--private-output", required=True, type=Path)
    args = parser.parse_args(argv)
    reference, runtime, output = (path.resolve() for path in
                                  (args.python_reference, args.runtime, args.private_output))
    if any(output.is_relative_to(path) for path in (ROOT, reference, runtime)):
        raise ValueError("private comparison output must be outside both checkouts and the runtime")
    if output.exists():
        raise ValueError("private comparison output already exists")
    if Path(git(reference, "rev-parse", "--show-toplevel")).resolve() != reference:
        raise ValueError("Python reference must be a checkout root")
    revision = git(reference, "rev-parse", "HEAD")
    if revision != args.python_revision or git(
        reference, "status", "--porcelain", "--untracked-files=all", "--", "src", "tests",
    ):
        raise ValueError("expected the pinned clean Python source and test tree")

    matrix = json.loads(args.rust_report.read_bytes())
    if (matrix.get("schema_version"), matrix.get("scenario"), matrix.get("probe_roster")) != (
        2, SCENARIO, PROBE_ROSTER,
    ) or matrix.get("scene", {}).get("status") != "prepared":
        raise ValueError("expected a prepared Rust actor matrix with the declared one-player roster")
    inputs = matrix["inputs"]
    verify_runtime_inputs(runtime, inputs)
    logging.getLogger().addHandler(logging.NullHandler())
    # Import the pinned sources without consulting or changing checkout caches.
    with tempfile.TemporaryDirectory(prefix="orpheus-python-reference-") as cache:
        sys.pycache_prefix = cache
        sys.dont_write_bytecode = True
        rows = merge_actor_rows(matrix["actors"], python_rows(reference, runtime))
    verify_runtime_inputs(runtime, inputs)
    report = {
        "schema_version": 1,
        "scenario": SCENARIO,
        "python_revision": revision,
        "python_path": "offline-room-selection-in-memory-ack",
        "rust_revision": matrix["source_revision"],
        "rust_path": "independent-strict-factory-and-room-factory",
        "start_frame": START_FRAME,
        "probe_roster": PROBE_ROSTER,
        "inputs": inputs,
        "actors": rows,
    }
    with output.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(report, stream, sort_keys=True, indent=2, allow_nan=False)
        stream.write("\n")
    print(f"Python first owner decoded: {sum(row['python']['status'] == 'decoded' for row in rows)}/{len(rows)}")
    print(f"Rust independent startup passed: {sum(row['rust_bootstrap']['status'] == 'started' for row in rows)}/{len(rows)}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from None
