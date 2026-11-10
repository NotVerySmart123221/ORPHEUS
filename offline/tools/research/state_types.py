"""Read the verified client's bounded state-factory registry; never attach a debugger or write memory."""
import argparse
import json
from pathlib import Path
import struct

from debug_assert import (
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ, _kernel32, _read_remote,
    find_target_module, module_rva, validate_pid,
)

REGISTRY_RVA = 0x32969E0


def registry_entries(read, registry):
    capacity, count, buckets = struct.unpack_from('<IIQ', read(registry, 32), 16)
    if not 0 < capacity <= 4096 or capacity & (capacity - 1) or not 0 <= count <= 4096:
        raise ValueError('invalid registry capacity/count')
    result, seen = [], set()
    raw = b''.join(read(buckets + offset, min(512, capacity * 8 - offset))
                   for offset in range(0, capacity * 8, 512))
    for (node,) in struct.iter_unpack('<Q', raw):
        while node:
            if node in seen:
                raise ValueError('registry cycle or duplicate node')
            if len(seen) >= count:
                raise ValueError('registry count inconsistent')
            seen.add(node)
            node, value, _, key = struct.unpack('<4Q', read(node, 32))
            result.append((key, value))
    if len(result) != count:
        raise ValueError('registry count inconsistent')
    return result


def inspect(pid):
    validate_pid(pid)
    k32 = _kernel32()
    base, size = find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not process:
        raise OSError('cannot open verified client read-only')
    budget = 2 * 1024 * 1024
    def read(address, count):
        nonlocal budget
        if not 0x10000 <= address < 0x7FFFFFFFFFFF or not 0 < count <= 512 or count > budget:
            raise ValueError('invalid or unbounded registry read')
        budget -= count
        data = _read_remote(k32, process, address, count)
        if len(data) != count:
            raise ValueError('short registry read')
        return data
    def qword(address):
        return struct.unpack('<Q', read(address, 8))[0]
    try:
        entries = registry_entries(read, qword(base + REGISTRY_RVA))
        result = []
        for key, value in entries:
            prototype = qword(value + 0x18)
            vtable = qword(prototype) if prototype else 0
            dsr = qword(vtable + 0x158) if module_rva(vtable, base, size) is not None else 0
            result.append(dict(key_rva=module_rva(key, base, size),
                               constructor_rva=module_rva(qword(value + 8), base, size),
                               factory_address=value, prototype_address=prototype,
                               vtable_address=vtable,
                               prototype_rva=module_rva(prototype, base, size),
                               vtable_rva=module_rva(vtable, base, size),
                               dsr_rva=module_rva(dsr, base, size)))
        return dict(module_base=base, entries=result, bytes_read=2 * 1024 * 1024 - budget)
    finally:
        k32.CloseHandle(process)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = inspect(args.pid)
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(result, output, indent=2)
        output.write('\n')
    print(f"Read {len(result['entries'])} state types ({result['bytes_read']} bytes)")


if __name__ == '__main__':
    main()
