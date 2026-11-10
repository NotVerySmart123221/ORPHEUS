import ctypes
import unittest

import hw_detector_probe as probe


class Kernel:
    def __init__(self):
        self.state = [0, 0, 0, 0, 0xFFFF0FF0, 0x400]
        self.original = self.state[:]
        self.writes = []
        self.suspended = 0
        self.fail_read = False

    def SuspendThread(self, thread):
        old = self.suspended
        self.suspended += 1
        return old

    def ResumeThread(self, thread):
        old = self.suspended
        self.suspended -= 1
        return old

    def GetThreadContext(self, thread, ptr):
        if self.fail_read:
            self.fail_read = False
            raise OSError("readback failed")
        self.assert_debug_only(ptr)
        for offset, value in zip(probe.DR_OFFSETS, self.state):
            ctypes.c_uint64.from_address(ptr.value + offset).value = value
        return 1

    def SetThreadContext(self, thread, ptr):
        self.assert_debug_only(ptr)
        self.state = [ctypes.c_uint64.from_address(ptr.value + o).value for o in probe.DR_OFFSETS]
        self.writes.append(self.state[:])
        return 1

    def assert_debug_only(self, ptr):
        assert self.suspended == 1
        assert ctypes.c_uint32.from_address(ptr.value + 0x30).value == 0x100010

    def WaitForSingleObject(self, process, milliseconds):
        return 258


class ProbeTests(unittest.TestCase):
    def test_read_control_never_writes_context(self):
        kernel = Kernel()
        result = probe.exercise(kernel, 1, 2, 0x140000000, "read", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.writes, [])
        self.assertEqual(kernel.state, kernel.original)
        self.assertEqual(kernel.suspended, 0)
        self.assertTrue(result["survived"])

    def test_dormant_watch_restores_every_original_debug_register(self):
        kernel = Kernel()
        result = probe.exercise(kernel, 1, 2, 0x140000000, "arm", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.writes[0], [0x140000000, 0, 0, 0, 0xFFFF0FF0, 0xD0401])
        self.assertEqual(kernel.writes[-1], kernel.original)
        self.assertEqual(kernel.state, kernel.original)
        self.assertEqual(kernel.suspended, 0)
        self.assertTrue(result["restored"])

    def test_same_control_writes_original_state(self):
        kernel = Kernel()
        probe.exercise(kernel, 1, 2, 0x140000000, "same", 0, lambda *args, **kw: None)
        self.assertTrue(kernel.writes)
        self.assertTrue(all(state == kernel.original for state in kernel.writes))

    def test_readback_error_still_restores_and_resumes(self):
        kernel = Kernel()
        def emit(phase, **fields):
            if phase == "write":
                kernel.fail_read = True
        with self.assertRaisesRegex(OSError, "readback failed"):
            probe.exercise(kernel, 1, 2, 0x140000000, "arm", 0, emit)
        self.assertEqual(kernel.state, kernel.original)
        self.assertEqual(kernel.suspended, 0)

    def test_conflicting_debug_state_is_rejected_before_write(self):
        kernel = Kernel()
        kernel.state[5] |= 1
        with self.assertRaises(ValueError):
            probe.exercise(kernel, 1, 2, 0x140000000, "arm", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.writes, [])
        self.assertEqual(kernel.suspended, 0)

    def test_wait_error_does_not_skip_restoration(self):
        kernel = Kernel()
        kernel.WaitForSingleObject = lambda *args: 0xFFFFFFFF
        with self.assertRaises(OSError):
            probe.exercise(kernel, 1, 2, 0x140000000, "arm", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.state, kernel.original)
        self.assertEqual(kernel.suspended, 0)

    def test_controls_preserve_active_game_breakpoints(self):
        for mode in ("read", "same"):
            kernel = Kernel()
            kernel.state = [0x1000, 0x2000, 0x3000, 0x4000, 0xFFFF0FF0, 0xFFFF0455]
            kernel.original = kernel.state[:]
            probe.exercise(kernel, 1, 2, 0x140000000, mode, 0, lambda *args, **kw: None)
            self.assertEqual(kernel.state, kernel.original)

    def test_only_reserved_bit_changes_are_tolerated(self):
        before = [1, 2, 3, 4, 0xFFFF0FF0, 0xFFFF0455]
        after = [1, 2, 3, 4, 0, 0xFFFF0055]
        self.assertTrue(probe.same_debug(before, after))
        for index, bit in ((0, 1), (4, 1), (4, 0x4000), (5, 1), (5, 0x10000)):
            changed = after[:]
            changed[index] ^= bit
            self.assertFalse(probe.same_debug(before, changed))

    def test_dormant_execute_control_uses_four_slots_and_restores(self):
        kernel = Kernel()
        probe.exercise(kernel, 1, 2, 0x140000000, "execute", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.writes[0], [0x140000000] * 4 + [0, 0x455])
        self.assertEqual(kernel.state, kernel.original)

    def test_execute_refuses_occupied_slots_but_explicit_replacement_restores_them(self):
        kernel = Kernel()
        kernel.state = [0x1000, 0x2000, 0x3000, 0x4000, 0xFFFF0FF0, 0xFFFF0455]
        kernel.original = kernel.state[:]
        with self.assertRaises(ValueError):
            probe.exercise(kernel, 1, 2, 0x140000000, "execute", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.writes, [])
        probe.exercise(kernel, 1, 2, 0x140000000, "replace-execute", 0, lambda *args, **kw: None)
        self.assertEqual(kernel.writes[0], [0x140000000] * 4 + [0, 0x455])
        self.assertEqual(kernel.state, kernel.original)


if __name__ == "__main__":
    unittest.main()
