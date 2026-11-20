"""Opt-in encrypted loopback receiver check using the pinned Python reader."""

import argparse
import copy
import importlib.util
import itertools
import json
import os
import subprocess
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location("check_sprint_wire", Path(__file__).with_name("check_sprint_wire.py"))
assert spec is not None and spec.loader is not None
sprint = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sprint)


TEST = "game_service::tests::same_schedule::transport::prepared_owner_post_startup_loss_receipts"


def header(row, decode):
    width = row["bits"]
    raw = bytearray(row["data"])
    sprint.require(type(width) is int and width > 0 and len(raw) == (width + 7) // 8, "chunk bounds")
    reader = decode.BitReader(raw)
    reader.size = width
    first = decode.w_u32(reader)
    last = first + decode.w_var(reader)
    sprint.require(not reader.bit(), "fragmented chunk")
    size = decode.w_var(reader)
    sprint.require(reader.pos + size == width, "declared chunk length")
    reader.bit()
    owner = bool(reader.bit())
    if not owner:
        return first, last, owner, None, None
    reader.bit()
    start = reader.pos
    cmfd = decode.w_var(reader)
    return first, last, owner, cmfd, (start, reader.pos)


def mutate_cmfd(row, decode, value):
    original = header(row, decode)
    sprint.require(original[4] is not None, "cmfd mutation owner")
    assert original[4] is not None
    start, end = original[4]
    raw = bytearray(row["data"])
    payload_width = 8 if end - start == 9 else 16 if end - start == 18 else 32
    sprint.require(end - start in (9, 18, 34) and 0 <= value < 1 << payload_width, "cmfd mutation width")
    for bit in range(payload_width):
        offset = end - payload_width + bit
        raw[offset // 8] = (raw[offset // 8] & ~(1 << (offset % 8))) | (((value >> bit) & 1) << (offset % 8))
    changed = {**row, "data": list(raw)}
    updated = header(changed, decode)
    sprint.require(updated[:3] == original[:3] and updated[3] == value and updated[4] == original[4], "cmfd mutation")
    return changed


def check(capture, decode, suppress_recovery=False):
    rows, end = capture["rows"], capture["end"]
    sprint.require(bool(rows), "empty capture")
    sprint.require(all(a["seq"] < b["seq"] for a, b in itertools.pairwise(rows)), "packet order")
    lost = [i for i, row in enumerate(rows) if row["kind"] == "lost"]
    sprint.require(len(lost) == 1 and lost[0] > 0 and lost[0] + 1 < len(rows), "loss schedule")
    index = lost[0]
    sprint.require(all(row["kind"] == "accepted" for i, row in enumerate(rows) if i != index), "acceptance schedule")
    sprint.require(rows[index + 1]["seq"] == rows[index]["seq"] + 1, "recovery sequence")
    sprint.require(rows[-1]["seq"] == end["seq"] and rows[-1]["elapsed"] == end["elapsed"]
                   and end["release_elapsed"] < end["elapsed"] <= end["processed_elapsed"]
                   and end["processed_frame"] > 0, "incomplete capture")
    body = decode.ClientBody.for_hero(0x02E000000000006E)
    root = None
    recovered = stopped = False
    stop_elapsed = None
    stop_index = None
    known = {}
    removed = set()
    cmfd = 0
    for i, row in enumerate(rows):
        width = row["bits"]
        raw = bytearray(row["data"])
        sprint.require(type(width) is int and width > 0 and len(raw) == (width + 7) // 8, "chunk bounds")
        first, last, owner, frame_cmfd, _ = header(row, decode)
        prior = body.applied
        if i == 0:
            sprint.require(first == 0 and last == 1 and not owner, "baseline")
        else:
            sprint.require(first != 0 and owner and first <= prior + 1 <= last, "stale or gap")
            sprint.require(frame_cmfd is not None and frame_cmfd > cmfd, "stale owner command frame")
        target = copy.deepcopy(body) if i == index else body
        if i == index + 1 and suppress_recovery:
            sprint.require(body.vars.get((root, 699), 0) == 1, "recovery semantic value")
            continue
        frame, writes = sprint.apply_chunk(decode, target, raw, width)
        sprint.require((frame.first, frame.last, frame.owner) == (first, last, owner), "header mismatch")
        sprint.require(not owner or frame.cmfd == frame_cmfd, "owner frame mismatch")
        if i != index:
            if owner:
                cmfd = frame_cmfd
            for number, item in frame.instances.items():
                if item.gone:
                    sprint.require(number in known, "unknown removal")
                    del known[number]
                    removed.add(number)
                elif item.descriptor:
                    descriptor = (item.graph, item.parent)
                    sprint.require(number not in removed, "reintroduced instance")
                    sprint.require(number not in known or known[number] == descriptor, "conflicting descriptor")
                    known[number] = descriptor
                else:
                    sprint.require(number in known, "unknown record")
            sprint.require(body.records == known, "record lifecycle")
        roots = [number for number, descriptor in target.records.items() if descriptor == (0x0255, None)]
        sprint.require(len(roots) == 1, "unique sprint root")
        if root is None:
            root = roots[0]
        sprint.require(roots == [root], "sprint root changed")
        sprint.require(all(own == root and graph == 0x0255 and type(value) is int and bindings == ()
                           for own, graph, (value, bindings), _ in writes), "699 owner type or bindings")
        values = [(value, bindings) for _, _, (value, bindings), _ in writes]
        if i == index:
            before = body.vars.get((root, 699))
            sprint.require(before is None or (type(before) is int and before == 0), "pre-loss semantic value")
            sprint.require(values == [(1, ())], "lost transition")
            sprint.require(target.vars.get((root, 699)) == 1, "lost semantic value")
        elif i == index + 1:
            sprint.require(body.vars.get((root, 699)) == 1, "recovery semantic value")
            sprint.require(values == [(1, ())], "recovery transition")
            recovered = True
        elif i < index:
            sprint.require(not values and (root, 699) not in body.vars, "early 699 write")
        else:
            if values:
                sprint.require(not stopped and row["elapsed"] >= end["release_elapsed"] and values == [(0, ())], "unexpected 699 write")
                stopped = True
                stop_elapsed = row["elapsed"]
                stop_index = i
            expected = 0 if stopped else 1
            sprint.require(type(body.vars.get((root, 699))) is int and body.vars[(root, 699)] == expected, "699 persistence")
    sprint.require(recovered and stopped, "missing recovery or stop")
    sprint.require(stop_elapsed is not None and rows[-1]["elapsed"] > stop_elapsed, "missing post-stop observation")
    sprint.require(type(body.vars.get((root, 699))) is int and body.vars[(root, 699)] == 0, "final semantic value")
    return stop_index


def check_paths(runtime, paths, graph, codecs):
    data = paths.DATA_DIR.resolve()
    graph_path = graph.GRAPHS_PATH.resolve()
    codec_path = codecs.CODECS_PATH.resolve()
    sprint.require(data == (runtime / "data").resolve() and data.is_relative_to(runtime), "runtime data directory")
    sprint.require(graph_path == (runtime / "data/game_graphs_174.json.gz").resolve()
                   and graph_path.is_relative_to(runtime), "runtime graph path")
    sprint.require(codec_path == (runtime / "data/statescript_codecs_174.json").resolve()
                   and codec_path.is_relative_to(runtime), "runtime codec path")


def expect_failure(capture, decode, expected, **kwargs):
    try:
        check(capture, decode, **kwargs)
    except ValueError as error:
        sprint.require(str(error) == expected, "wrong negative failure")
    else:
        raise AssertionError("negative control passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-reference", type=Path, required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    os.environ["ORPHEUS_RUNTIME_DIR"] = str(runtime)
    decode, graph, codecs, paths = sprint.load_reference(args.python_reference)
    check_paths(runtime, paths, graph, codecs)
    with tempfile.TemporaryDirectory(prefix="orpheus-receiver-") as temporary:
        path = Path(temporary) / "chunks.json"
        env = os.environ.copy()
        env["ORPHEUS_RECEIVER_PROBE"] = str(path)
        subprocess.run(["cargo", "test", "--locked", "-p", "orpheus", "--lib", TEST, "--", "--exact", "--ignored"], cwd=sprint.ROOT, env=env, check=True)
        sprint.require(path.is_file(), "prepared test did not produce chunks")
        capture = json.loads(path.read_text(encoding="utf-8"))
        rows = capture["rows"]
        stop_index = check(capture, decode)
        expect_failure(capture, decode, "recovery semantic value", suppress_recovery=True)
        index = next(i for i, row in enumerate(rows) if row["kind"] == "lost")
        previous = header(rows[index - 1], decode)[3]
        sprint.require(previous is not None, "missing previous owner frame")
        altered = list(rows)
        altered[index + 1] = mutate_cmfd(rows[index + 1], decode, previous)
        expect_failure({**capture, "rows": altered}, decode, "stale owner command frame")
        earlier = [i for i in range(index) if header(rows[i], decode)[2]]
        sprint.require(len(earlier) >= 2, "missing earlier owner frames")
        before, stale = earlier[:2]
        previous = header(rows[before], decode)[3]
        sprint.require(previous is not None, "missing earlier owner frame")
        altered = list(rows)
        altered[stale] = mutate_cmfd(rows[stale], decode, previous)
        expect_failure({**capture, "rows": altered}, decode, "stale owner command frame")
        sprint.require(stop_index is not None, "missing stop index")
        assert stop_index is not None
        expect_failure({**capture, "rows": rows[:stop_index + 1]}, decode, "incomplete capture")
    print("PASS: Sprint loss/recovery/stop; suppressed recovery, stale recovery/earlier CmFD and truncation rejected")


if __name__ == "__main__":
    main()
