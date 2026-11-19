"""Check the prepared Rust Soldier stream with the pinned Python client reader.

Requires an explicit clean Python reference checkout and a prepared runtime. The
temporary chunks are private and are deleted when this command exits.
Run with python -I -S -B scripts/check_sprint_wire.py --python-reference
CHECKOUT --runtime RUNTIME; the reference commit is
the one used by the repository's shared-behavior-contracts CI job. This is an
opt-in prepared-runtime check, not an original-client or transport test.
"""

import argparse
import copy
import hashlib
import importlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

REFERENCE = "b28139d4fa230480c0cea147d369724f97569c30"
TEST = "scene_assets::tests::stock_soldier_owner_session_idles_fires_reloads_and_sprints"
ROOT = Path(__file__).resolve().parents[1]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_writes(observed, owner, graph, expected):
    require(all((own, index) == (owner, graph) for own, index, _, _ in observed), "unexpected 699 owner in scenario")
    require(len(observed) <= 1, "duplicate 699 write")
    values = [value for _, _, value, _ in observed]
    for value, bindings in values:
        require(type(value) is int and bindings == (), "699 type or bindings")
    require(values == expected, "699 writes")


def apply_chunk(decode, body, raw, width):
    """Bound the pinned reader and observe each variable before dict insertion."""
    original_reader, original_var = decode.BitReader, decode.read_var
    observed = []
    variables = []

    class BoundedReader(original_reader):
        def __init__(self, data, pos=0):
            super().__init__(data, pos)
            self.size = width
            observed.append(self)

    def read_var(reader, own, graphs, var=None):
        offset = reader.pos
        result = original_var(reader, own, graphs, var)
        if var == 699:
            variables.append((own, graphs.get(own), result, offset))
        return result

    update = SimpleNamespace(chunk=SimpleNamespace(count=width, getvalue=lambda: bytes(raw)))
    with patch.object(decode, "BitReader", BoundedReader), patch.object(decode, "read_var", read_var):
        frame = body.apply(update)
    require(len(observed) == 1 and observed[0].pos == width, "unconsumed chunk bits")
    return frame, variables


def check(rows, decode, mutate=False):
    """Consume exact chunks, including their declared bit limits, in delivery order."""
    require([row["t"] for row in rows] == [0, 1, *range(17, 901)], "chunk schedule")
    body = decode.ClientBody.for_hero(0x02E000000000006E)
    original_reader = decode.BitReader
    previous_last = 0
    sprint = None
    known = {}
    removed = set()
    writes = []
    changed = False

    for row in rows:
        t, width = row["t"], row["bits"]
        raw = bytearray(row["data"])
        require(type(width) is int and width > 0 and len(raw) == (width + 7) // 8, "chunk length")
        header = original_reader(raw)
        header.size = width
        decode.w_u32(header)
        decode.w_var(header)
        require(not header.bit(), "fragmented chunk")
        declared = decode.w_var(header)
        require(header.pos + declared == width, "declared chunk length")
        # Locate the target value with the independent reader, then replay the
        # chunk from the previous client state with one payload bit flipped.
        if mutate and t == 700:
            before = copy.deepcopy(body)
            _, target = apply_chunk(decode, body, raw, width)
            offsets = [offset for own, graph, _, offset in target if own == sprint and graph == 0x0255]
            require(len(offsets) == 1, "missing mutation target")
            offset = offsets[0]
            tag = sum(((raw[(offset + i) // 8] >> ((offset + i) % 8)) & 1) << i for i in range(4))
            require(tag == 1 and offset + 4 < width, "unexpected mutation encoding")
            raw[(offset + 4) // 8] ^= 1 << ((offset + 4) % 8)
            body = before
            changed = True

        frame, variables = apply_chunk(decode, body, raw, width)
        if t == 0:
            require(frame.first == 0 and frame.last == 1 and not frame.owner, "baseline header")
        else:
            # game_clock::FIRST_TICK for this pinned scenario is 1000.
            require(frame.owner and frame.cmfd == 1000 + t, "owner command frame")
            require(frame.correction == (t != 1), "owner phase")
            require(frame.first == previous_last + 1 and frame.last == frame.first, "chunk sequence")
        previous_last = frame.last
        for number, item in frame.instances.items():
            if item.gone:
                require(number in known, "unknown removal")
                del known[number]
                removed.add(number)
            elif item.descriptor:
                descriptor = (item.graph, item.parent)
                require(number not in removed, "reintroduced instance")
                require(number not in known or known[number] == descriptor, "conflicting descriptor")
                known[number] = descriptor
            else:
                require(number in known, "unknown instance")
        require(body.records == known, "record lifecycle")
        roots = [number for number, descriptor in known.items() if descriptor == (0x0255, None)]
        require(len(roots) == 1, "unique sprint root")
        if sprint is None:
            sprint = roots[0]
        require(roots == [sprint], "sprint root changed")
        expected = [(1, ())] if t == 700 else [(0, ())] if t == 761 else []
        validate_writes(variables, sprint, 0x0255, expected)
        for _, _, (value, _), _ in variables:
            writes.append((t, value))
        key = (sprint, 699)
        if t < 700:
            require(key not in body.vars, "early 699 value")
        else:
            require(type(body.vars.get(key)) is int and body.vars[key] == (1 if t < 761 else 0), "699 cumulative value")
    require(not mutate or changed, "mutation not applied")
    require(writes == [(700, 1), (761, 0)], "699 writes")


def verify_reference(reference):
    entries = subprocess.check_output(["git", "-C", str(reference), "ls-tree", "-rz", REFERENCE, "--", "src/ow174"])
    require(bool(entries), "missing reference tree")
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        header, name = entry.split(b"\t", 1)
        mode, kind, expected = header.split()
        require(mode in (b"100644", b"100755") and kind == b"blob", "unsafe reference tree entry")
        path = reference / os.fsdecode(name)
        relative = path.relative_to(reference)
        require(all(not (reference / Path(*relative.parts[:i])).is_symlink() for i in range(1, len(relative.parts) + 1)), "reference symlink")
        data = path.read_bytes()
        actual = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data, usedforsecurity=False).hexdigest().encode()
        require(actual == expected, "reference content mismatch")


def load_reference(reference):
    require(sys.flags.isolated and sys.flags.no_site and sys.flags.dont_write_bytecode, "invoke Python with -I -S -B")
    reference = reference.resolve(strict=True)
    top = subprocess.check_output(["git", "-C", str(reference), "rev-parse", "--show-toplevel"], text=True).strip()
    require(reference == Path(top).resolve(strict=True), "Python reference must be the checkout root")
    source = reference / "src"
    require((source / "ow174/game/script/decode.py").is_file(), "Python reference source missing")
    revision = subprocess.check_output(["git", "-C", str(reference), "rev-parse", "HEAD"], text=True).strip()
    require(revision == REFERENCE, "wrong Python reference revision")
    status = subprocess.check_output(["git", "-C", str(reference), "status", "--porcelain", "--untracked-files=all", "--ignored", "--", "src/ow174"], text=True)
    require(not status, "modified Python reference")
    verify_reference(reference)
    overrides = [key for key in os.environ if key.startswith("ORPHEUS_") and any(part in key for part in ("GRAPH", "CODEC", "DATA"))]
    require(not overrides, "graph or codec environment override")
    sys.path.append(str(source))
    decode = importlib.import_module("ow174.game.script.decode")
    graph = importlib.import_module("ow174.game.script.graph")
    codecs = importlib.import_module("ow174.game.script.codecs")
    paths = importlib.import_module("ow174.paths")
    for name, module in tuple(sys.modules.items()):
        if name == "ow174" or name.startswith("ow174."):
            location = getattr(module, "__file__", None)
            require(location is not None and Path(location).resolve().is_relative_to(source), "foreign Python reference module")
    return decode, graph, codecs, paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-reference", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    os.environ["ORPHEUS_RUNTIME_DIR"] = str(runtime)
    decode, graph, codecs, paths = load_reference(args.python_reference)
    require(paths.DATA_DIR.resolve() == (runtime / "data").resolve(), "runtime data directory")
    require(graph.GRAPHS_PATH.resolve() == (runtime / "data/game_graphs_174.json.gz").resolve(), "runtime graph path")
    require(codecs.CODECS_PATH.resolve() == (runtime / "data/statescript_codecs_174.json").resolve(), "runtime codec path")
    require(graph.GRAPHS_PATH.resolve().is_relative_to(runtime), "graph outside runtime")
    require(codecs.CODECS_PATH.resolve().is_relative_to(runtime), "codec outside runtime")
    with tempfile.TemporaryDirectory(prefix="orpheus-sprint-wire-") as temporary:
        path = Path(temporary) / "chunks.json"
        env = os.environ.copy()
        env["ORPHEUS_SPRINT_WIRE_PROBE"] = str(path)
        subprocess.run(["cargo", "test", "--locked", "-p", "orpheus", "--lib", TEST, "--", "--exact", "--ignored"], cwd=ROOT, env=env, check=True)
        require(path.is_file(), "prepared test did not produce chunks")
        rows = json.loads(path.read_text(encoding="utf-8"))
        check(rows, decode)
        for name, candidate, mutation, expected in (
            ("missing final chunk", rows[:-1], False, "chunk schedule"),
            ("mutated 699", rows, True, "699 writes"),
            ("malformed length", [*rows[:-1], {**rows[-1], "bits": rows[-1]["bits"] - 1}], False, "declared chunk length"),
        ):
            try:
                check(candidate, decode, mutate=mutation)
            except ValueError as error:
                require(str(error) == expected, f"wrong failure for {name}: {error}")
                print(f"PASS negative control: {name}")
                continue
            raise AssertionError(f"negative control passed: {name}")
    print("PASS: prepared Sprint variable 699 and three prepared negative controls")


if __name__ == "__main__":
    main()
