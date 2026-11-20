"""Synthetic two-instance state-link wire tests for the pinned Python client-reader model.

The graphs (0x0CE0 -> 0x0D03 model) and codec table are synthetic (no runtime
pack required). The Rust-to-Python checks are opt-in (set
ORPHEUS_PYTHON_REFERENCE and ORPHEUS_STATE_LINK_WIRE_RUST=1). ClientCrash
(0x7FF78AA8385A) assertions reflect the pinned Python reader model, not a
proven original-client crash; the original game client was not run.
"""

import contextlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.append(str(Path(__file__).resolve().parents[1]))
from check_sprint_wire import apply_chunk, load_reference

SOURCE_GRAPH = 0x0CE0
TARGET_GRAPH = 0x0D03
SOURCE_STATE = 10
TARGET_STATE = 1
TARGET_REMOTE_INDEX = 2
STATE_CLASS = "STU_9028E035"
SYNTHETIC_CODECS = {
    "classes": {
        "9028E035": {
            "name": STATE_CLASS,
            "codec": "link",
            "stec": False,
            "status": "VERIFIED",
            "layout": [{"op": "link", "f": "link"}],
        }
    }
}


def number(value, width):
    if not 0 <= value < 1 << width:
        raise ValueError("synthetic integer width")
    return [(value >> i) & 1 for i in range(width)]


def small(value, width):
    return [0] + number(value, width)


def w_u16(value):
    if value < 16:
        return small(value, 4)
    if value < 256:
        return [1, 0] + number(value, 8)
    return [1, 1] + number(value, 16)


def w_var(value):
    if value < 256:
        return small(value, 8)
    if value < 65536:
        return [1, 0] + number(value, 16)
    return [1, 1] + number(value, 32)


def make_graph(states_bits, remote_bits, state_index, remote_index):
    node = SimpleNamespace(state=state_index, cls=STATE_CLASS)
    return SimpleNamespace(
        states_bits=states_bits,
        remote_bits=remote_bits,
        states=[None] * state_index + [node],
        sync_vars=[],
        owner_states=lambda: [node],
        remote_states=lambda: [node],
        remote_node=lambda idx: node if idx == remote_index else None,
        presence_vars=list,
    )


def build_graphs():
    return {
        SOURCE_GRAPH: make_graph(4, 1, SOURCE_STATE, 0),
        TARGET_GRAPH: make_graph(4, 3, TARGET_STATE, TARGET_REMOTE_INDEX),
    }


def full_frame_chunk(owner, instance_specs):
    payload = [0, int(owner)]
    if owner:
        payload += [0] + w_var(1)  # correction=False, cmfd=1
    last_id = 0
    for instance_id, graph_index, _ in instance_specs:
        payload += w_u16(instance_id - last_id)
        last_id = instance_id
        payload += number(graph_index, 16) + [0, 0, 0, 0]
    payload += w_u16(0)
    payload += [0]  # no entity vars
    for _, _, state_link_bits in instance_specs:
        payload += [0]  # no extra instance vars
        payload += ([1] if owner else [1, 1]) + state_link_bits
        if owner:
            payload += [1, 1]  # empty owner event list (code 3)
    payload += [0, 0]  # no frame actions
    bits = small(0, 16) + small(0, 8) + [0] + w_var(len(payload)) + payload
    return sum(bit << i for i, bit in enumerate(bits)).to_bytes((len(bits) + 7) // 8, "little"), len(bits)


@contextlib.contextmanager
def synthetic_tables(decode, codecs, graphs):
    def clear_caches():
        codecs.table.cache_clear()
        codecs._index.cache_clear()
        codecs.stec_classes.cache_clear()

    clear_caches()
    fake_path = SimpleNamespace(read_text=lambda encoding="utf-8": json.dumps(SYNTHETIC_CODECS))
    with (
        patch.object(decode, "graph_of", side_effect=graphs.__getitem__),
        patch.object(codecs, "CODECS_PATH", fake_path),
    ):
        try:
            yield
        finally:
            clear_caches()


@unittest.skipUnless(os.environ.get("ORPHEUS_PYTHON_REFERENCE"), "set ORPHEUS_PYTHON_REFERENCE to the pinned checkout")
class StateLinkWireTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.decode, _, cls.codecs, _ = load_reference(Path(os.environ["ORPHEUS_PYTHON_REFERENCE"]))

    def read_frame(self, owner, instance_specs):
        raw, width = full_frame_chunk(owner, instance_specs)
        body = self.decode.ClientBody()
        with synthetic_tables(self.decode, self.codecs, build_graphs()):
            frame, observed = apply_chunk(self.decode, body, raw, width)
        self.assertEqual(observed, [])
        return body, frame

    def test_observer_full_frame_decodes_remote_state_link(self):
        link_bits = [1] + w_u16(2) + number(TARGET_REMOTE_INDEX, 3)
        body, frame = self.read_frame(
            owner=False,
            instance_specs=[
                (1, SOURCE_GRAPH, link_bits),
                (2, TARGET_GRAPH, [0]),
            ],
        )
        self.assertEqual((frame.first, frame.last, frame.owner), (0, 0, False))
        self.assertEqual(frame.instances[1].states[SOURCE_STATE], (True, {"link": (2, TARGET_STATE)}))
        self.assertEqual(frame.instances[2].states[TARGET_STATE], (True, {}))
        self.assertEqual(body.states, {(1, SOURCE_STATE): True, (2, TARGET_STATE): True})

    def test_owner_full_frame_decodes_raw_state_link(self):
        link_bits = [1] + w_u16(2) + number(TARGET_STATE, 4)
        body, frame = self.read_frame(
            owner=True,
            instance_specs=[
                (1, SOURCE_GRAPH, link_bits),
                (2, TARGET_GRAPH, [0]),
            ],
        )
        self.assertEqual((frame.first, frame.last, frame.owner, frame.cmfd), (0, 0, True, 1))
        self.assertEqual(frame.instances[1].states[SOURCE_STATE], (True, {"link": (2, TARGET_STATE)}))
        self.assertEqual(frame.instances[2].states[TARGET_STATE], (True, {}))
        self.assertEqual(body.states, {(1, SOURCE_STATE): True, (2, TARGET_STATE): True})

    def test_absent_link_decodes_as_none(self):
        for owner in (False, True):
            with self.subTest(owner=owner):
                _, frame = self.read_frame(
                    owner=owner,
                    instance_specs=[
                        (1, SOURCE_GRAPH, [0]),
                        (2, TARGET_GRAPH, [0]),
                    ],
                )
                active, payload = frame.instances[1].states[SOURCE_STATE]
                self.assertTrue(active)
                self.assertIsNone(payload.get("link"))
                self.assertEqual(payload, {})

    def test_unknown_target_instance_raises_client_crash(self):
        for owner, link_bits in (
            (False, [1] + w_u16(2) + number(TARGET_REMOTE_INDEX, 3)),
            (True, [1] + w_u16(2) + number(TARGET_STATE, 4)),
        ):
            with self.subTest(owner=owner), self.assertRaises(self.decode.ClientCrash) as ctx:
                self.read_frame(owner=owner, instance_specs=[(1, SOURCE_GRAPH, link_bits)])
            self.assertEqual(ctx.exception.where, "0x7FF78AA8385A")
            self.assertIn("0x7FF78AA8385A", str(ctx.exception))


RUST_STATE_LINK_TEST = (
    "script_snapshot_wire::tests::synthetic_two_instance_registered_state_link_encodes_for_owner_and_observer"
)
REPO_ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(
    os.environ.get("ORPHEUS_PYTHON_REFERENCE") and os.environ.get("ORPHEUS_STATE_LINK_WIRE_RUST") == "1",
    "set ORPHEUS_PYTHON_REFERENCE and ORPHEUS_STATE_LINK_WIRE_RUST=1 to run Rust-to-Python wire tests",
)
class RustStateLinkWireTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.decode, _, cls.codecs, _ = load_reference(Path(os.environ["ORPHEUS_PYTHON_REFERENCE"]))
        with tempfile.TemporaryDirectory() as tmp:
            probe_path = Path(tmp) / "state_link_probe.json"
            env = {**os.environ, "ORPHEUS_STATE_LINK_WIRE_PROBE": str(probe_path)}
            subprocess.run(
                ["cargo", "test", "--locked", "-p", "orpheus", "--lib", RUST_STATE_LINK_TEST, "--", "--exact"],
                cwd=REPO_ROOT,
                env=env,
                check=True,
            )
            cls.chunks = json.loads(probe_path.read_text(encoding="utf-8"))

    def assert_decoded_link(self, frame):
        active_1, payload_1 = frame.instances[1].states[SOURCE_STATE]
        self.assertIs(active_1, True)
        self.assertEqual(payload_1, {"link": (2, TARGET_STATE)})
        active_2, payload_2 = frame.instances[2].states[TARGET_STATE]
        self.assertIs(active_2, True)
        self.assertEqual(payload_2, {})

    def test_rust_observer_full_chunk_decodes_remote_link(self):
        chunk = self.chunks["observer_full"]
        body = self.decode.ClientBody()
        with synthetic_tables(self.decode, self.codecs, build_graphs()):
            frame, observed = apply_chunk(self.decode, body, bytes(chunk["data"]), chunk["bits"])
        self.assertEqual(observed, [])
        self.assertIs(frame.owner, False)
        self.assert_decoded_link(frame)

    def test_rust_owner_delta_chunk_decodes_raw_link(self):
        owner_full = self.chunks["owner_full"]
        owner_delta = self.chunks["owner_delta"]
        body = self.decode.ClientBody()
        with synthetic_tables(self.decode, self.codecs, build_graphs()):
            full_frame, full_observed = apply_chunk(
                self.decode, body, bytes(owner_full["data"]), owner_full["bits"]
            )
            delta_frame, delta_observed = apply_chunk(
                self.decode, body, bytes(owner_delta["data"]), owner_delta["bits"]
            )
        self.assertEqual(full_observed, [])
        self.assertEqual(delta_observed, [])
        self.assertIs(full_frame.owner, False)
        self.assertIs(delta_frame.owner, True)
        self.assert_decoded_link(delta_frame)

    def test_mutated_rust_link_target_instance_raises_client_crash(self):
        for label, setup_chunks, target_chunk in (
            ("observer_full", [], self.chunks["observer_full"]),
            ("owner_delta", [self.chunks["owner_full"]], self.chunks["owner_delta"]),
        ):
            with self.subTest(chunk=label):
                offsets = []
                original_link = self.decode._link

                def capture_link(reader, owner, graphs, _offsets=offsets, _orig=original_link):
                    _offsets.append(reader.pos)
                    return _orig(reader, owner, graphs)

                probe_body = self.decode.ClientBody()
                with synthetic_tables(self.decode, self.codecs, build_graphs()):
                    for chunk in setup_chunks:
                        apply_chunk(self.decode, probe_body, bytes(chunk["data"]), chunk["bits"])
                    with patch.object(self.decode, "_link", capture_link):
                        apply_chunk(self.decode, probe_body, bytes(target_chunk["data"]), target_chunk["bits"])
                self.assertEqual(len(offsets), 2)

                mutated = bytearray(target_chunk["data"])
                bit_pos = offsets[0] + 2  # presence bit (1) + w_u16 short-form tag (0) + bit 0 of target instance 2
                mutated[bit_pos // 8] ^= 1 << (bit_pos % 8)

                body = self.decode.ClientBody()
                with synthetic_tables(self.decode, self.codecs, build_graphs()):
                    for chunk in setup_chunks:
                        apply_chunk(self.decode, body, bytes(chunk["data"]), chunk["bits"])
                    with self.assertRaises(self.decode.ClientCrash) as ctx:
                        apply_chunk(self.decode, body, bytes(mutated), target_chunk["bits"])
                self.assertEqual(ctx.exception.where, "0x7FF78AA8385A")


if __name__ == "__main__":
    unittest.main()
