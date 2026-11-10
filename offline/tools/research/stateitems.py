"""A live statescript instance's state items, as the loading screen's predicate reads them. Read-only.

Instance layout (scriptlist.py finds instances): states array +0xA0, count +0xA8. A state item is null until the
client instantiates the state (remote-synced states only once the server turns them on); otherwise {+0x40 flags,
bit 0 active; +0x70 node index; +0x78 owning instance}, and its runtime object is the owner's node pool
([owner+0x1F8]: +0x60 arrays, +0x68 mode) at that index. The predicate (`0x7FF6D1926BA0`) ends a scripted-loading
mode's loading screen when any active state's object is of class 9FC136D7 (research/play-menu.md).

  python stateitems.py PID INSTANCE-ADDRESS-HEX [STATE-INDEX ...]
"""
import struct
import sys

from debug_assert import PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, validate_pid


def main(argv):
    pid, inst = validate_pid(int(argv[1])), int(argv[2], 16)
    k32 = _kernel32()
    base, _ = find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ, False, pid)
    rd = lambda a, n: _read_remote(k32, process, a, n)
    q = lambda a: struct.unpack('<Q', rd(a, 8))[0]
    d = lambda a: struct.unpack('<I', rd(a, 4))[0]
    array, count = q(inst + 0xA0), d(inst + 0xA8)
    print(f'instance {inst:#x} graph {q(inst + 0x1E8):#x} states {count}')
    for j in [int(a) for a in argv[3:]] or range(count):
        item = q(array + 8 * j)
        if not item:
            print(f'  st {j}: null')
            continue
        flags, index, owner = d(item + 0x40), d(item + 0x70), q(item + 0x78)
        try:
            pool = q(owner + 0x1F8)
            arrays, mode = q(pool + 0x60), q(pool + 0x68)
            obj = q((q(arrays) if mode == 1 else q(arrays + 8)) + 8 * index)
            desc = f'{obj:#x} vtable img+{q(obj) - base:#x}' if obj else '0'
        except (ValueError, OSError, struct.error) as error:
            desc = f'? {error!r}'
        print(f'  st {j}: {"active" if flags & 1 else "idle  "} flags {flags:#x} node {index} object {desc}')


if __name__ == '__main__':
    main(sys.argv)
