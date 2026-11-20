"""Synthetic guards for the opt-in receiver check."""

import errno
import importlib.util
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("receiver", Path(__file__).resolve().parents[1] / "check_receiver_convergence.py")
assert spec is not None and spec.loader is not None
receiver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(receiver)


class ScheduleGuards(unittest.TestCase):
    def test_empty_capture(self):
        with self.assertRaisesRegex(ValueError, "^empty capture$"):
            receiver.check({"rows": [], "end": {}}, None)

    def test_reordered_packets(self):
        with self.assertRaisesRegex(ValueError, "^packet order$"):
            receiver.check({"rows": [{"seq": 2}, {"seq": 1}], "end": {}}, None)

    def test_missing_loss(self):
        with self.assertRaisesRegex(ValueError, "^loss schedule$"):
            receiver.check({"rows": [{"seq": 1, "kind": "accepted"}], "end": {}}, None)

    def test_missing_recovery(self):
        with self.assertRaisesRegex(ValueError, "^loss schedule$"):
            receiver.check({"rows": [{"seq": 1, "kind": "accepted"}, {"seq": 2, "kind": "lost"}], "end": {}}, None)

    def test_early_one_then_zero_cannot_be_hidden_by_final_value(self):
        rows = [{"seq": n, "elapsed": n, "kind": "lost" if n == 3 else "accepted", "bits": 8, "data": [n]}
                for n in range(1, 7)]
        capture = {"rows": rows, "end": {"seq": 6, "elapsed": 6, "release_elapsed": 4, "processed_frame": 100, "processed_elapsed": 7}}

        class Body:
            def __init__(self):
                self.applied = 0
                self.records = {}
                self.vars = {}

            @classmethod
            def for_hero(cls, _hero):
                return cls()

        def fake_header(row, _decode):
            n = row["seq"]
            return (0, 1, False, None, None) if n == 1 else (n, n, True, n, None)

        def fake_apply(_decode, body, raw, _width):
            n = raw[0]
            instances = {}
            writes = []
            if n == 1:
                instances[7] = SimpleNamespace(gone=False, descriptor=True, graph=0x0255, parent=None)
                body.records[7] = (0x0255, None)
            if n == 2:
                writes = [(7, 0x0255, (1, ()), 0), (7, 0x0255, (0, ()), 1)]
                body.vars[(7, 699)] = 0
            first, last, owner, cmfd, _ = fake_header({"seq": n}, None)
            body.applied = last
            return SimpleNamespace(first=first, last=last, owner=owner, cmfd=cmfd, instances=instances), writes

        with (patch.object(receiver, "header", fake_header),
              patch.object(receiver.sprint, "apply_chunk", fake_apply),
              self.assertRaisesRegex(ValueError, "^early 699 write$")):
            receiver.check(capture, SimpleNamespace(ClientBody=Body))


class PathGuards(unittest.TestCase):
    def check_external_symlink(self, name, expected):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            runtime = base / "runtime"
            data = runtime / "data"
            data.mkdir(parents=True)
            outside = base / "outside"
            outside.touch()
            try:
                (data / name).symlink_to(outside)
            except OSError as error:
                if error.errno in (errno.EPERM, errno.EACCES, errno.ENOTSUP, errno.ENOSYS) or getattr(error, "winerror", None) in (50, 1314):
                    self.skipTest("symlink creation unavailable")
                raise
            paths = SimpleNamespace(DATA_DIR=data)
            graph = SimpleNamespace(GRAPHS_PATH=data / "game_graphs_174.json.gz")
            codecs = SimpleNamespace(CODECS_PATH=data / "statescript_codecs_174.json")
            with self.assertRaisesRegex(ValueError, f"^{expected}$"):
                receiver.check_paths(runtime.resolve(), paths, graph, codecs)

    def test_external_graph_symlink(self):
        self.check_external_symlink("game_graphs_174.json.gz", "runtime graph path")

    def test_external_codec_symlink(self):
        self.check_external_symlink("statescript_codecs_174.json", "runtime codec path")


if __name__ == "__main__":
    unittest.main()
