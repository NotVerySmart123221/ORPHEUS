"""Find the rifle Volley states in the verified client and resolve their shot info; read-only, no debugger.

Addresses are from the 104319 image (base 0x7FF6D1220000); see research/fire-animation-trace.md.
"""
import argparse
import ctypes
from ctypes import wintypes
import json
import struct
import time

from debug_assert import (
    PROCESS_VM_READ, _kernel32, _read_remote, find_target_module, module_rva, validate_pid,
)

PROCESS_QUERY_INFORMATION = 0x0400
MEM_COMMIT, MEM_IMAGE, WRITABLE = 0x1000, 0x1000000, (0x04, 0x08, 0x40)
MASK = (1 << 64) - 1

VOLLEY_VTABLE_RVA = 0x26547A8  # +0x1D0 = per-shot executor
EXECUTOR_RVA = 0xAE6DF0
ENTITY_KEY_RVA = 0x2EE47FC  # sub_7FF6D1F2D9B0
TABLE_KEY_RVA = 0x2EE4BD0  # sub_7FF6D1F7E030
INFO_COMPONENT = 34  # sub_7FF6D1E01850
SCAN_LIMIT = 16 << 30


class MEMORY_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [("BaseAddress", ctypes.c_void_p), ("AllocationBase", ctypes.c_void_p),
                ("AllocationProtect", wintypes.DWORD), ("PartitionId", wintypes.WORD),
                ("RegionSize", ctypes.c_size_t), ("State", wintypes.DWORD),
                ("Protect", wintypes.DWORD), ("Type", wintypes.DWORD)]


def rol64(value, bits):
    return ((value << bits) | (value >> (64 - bits))) & MASK


def state_entity(qword, base, instance):
    """sub_7FF6D1F2D9B0([instance+0x10]): the statescript instance's entity."""
    return rol64(qword(base + ENTITY_KEY_RVA), 2) ^ 0xF320E42472A94F14 ^ qword(qword(qword(instance + 0x10) + 0x528) + 8)


def entity_world(qword, entity):
    return qword(entity + 0x28) ^ 0x98DE97B50B3BE1E3


def info_map(qword, dword, base, entity):
    """sub_7FF6D1F7E030's map: world component table[34] + 0x20 holds 8 buckets of (info, key) pairs."""
    table = qword(entity_world(qword, entity) + 0x140) ^ ((0x4E3494C633194824 - qword(base + TABLE_KEY_RVA)) & MASK)
    component = qword(table + 8 * INFO_COMPONENT)
    entries = {}
    for bucket in range(8) if component else ():
        bucket = component + 0x20 + 0x90 * bucket
        count = struct.unpack('<i', struct.pack('<I', dword(bucket + 16)))[0]
        pairs = qword(bucket + 8)
        for i in range(min(max(count, 0), 4096)):
            entries[qword(pairs + 16 * i + 8)] = qword(pairs + 16 * i)
    return entries


def scan_for_qword(k32, process, value):
    """Every 8-aligned writable non-image address holding value."""
    return scan_for_bytes(k32, process, struct.pack('<Q', value))


def scan_for_bytes(k32, process, needle, align=8, max_region=None, protections=WRITABLE):
    """Every `align`-aligned non-image address with one of `protections` (default: writable) holding the bytes
    `needle`; regions over `max_region` bytes (asset caches) are skipped when it is given."""
    k32.VirtualQueryEx.argtypes = [wintypes.HANDLE, ctypes.c_void_p, ctypes.POINTER(MEMORY_BASIC_INFORMATION), ctypes.c_size_t]
    k32.VirtualQueryEx.restype = ctypes.c_size_t
    hits, address, scanned = [], 0x10000, 0
    info = MEMORY_BASIC_INFORMATION()
    while address < 0x7FFFFFFF0000 and k32.VirtualQueryEx(process, ctypes.c_void_p(address), ctypes.byref(info), ctypes.sizeof(info)):
        start, size = info.BaseAddress or 0, info.RegionSize
        if (info.State == MEM_COMMIT and info.Type != MEM_IMAGE and info.Protect in protections
                and (max_region is None or size <= max_region)):
            for offset in range(0, size, 1 << 20):
                count = min(1 << 20, size - offset)
                scanned += count
                if scanned > SCAN_LIMIT:
                    raise ValueError('scan limit reached')
                buffer, read = ctypes.create_string_buffer(count), ctypes.c_size_t()
                if not k32.ReadProcessMemory(process, ctypes.c_void_p(start + offset), buffer, count, ctypes.byref(read)):
                    # one bad page fails the whole chunk: retry page by page, zero-filling the bad ones
                    pages = []
                    for page in range(0, count, 0x1000):
                        ok = k32.ReadProcessMemory(process, ctypes.c_void_p(start + offset + page), ctypes.byref(buffer, page),
                                                   0x1000, ctypes.byref(read))
                        pages.append(buffer.raw[page:page + 0x1000] if ok else bytes(0x1000))
                    buffer.raw, read.value = b''.join(pages), count
                data, at = buffer.raw[:read.value], 0
                while (at := data.find(needle, at)) >= 0:
                    if at % align == 0:
                        hits.append(start + offset + at)
                    at += 1
        address = start + size
    return hits


def live_key(qword, volley, seconds):
    """The Volley's fields are only filled while it is active: wait for the next burst."""
    deadline = time.monotonic() + seconds
    while True:
        key = qword(volley + 0x1C0)
        if key or time.monotonic() > deadline:
            return {'key': '%X' % key, 'flags': '%X' % qword(volley + 0x40), 'count': qword(volley + 0x280) & 0xFFFFFFFF}
        time.sleep(0.002)


def inspect(pid, watch=0.0):
    validate_pid(pid)
    k32 = _kernel32()
    base, size = find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
    if not process:
        raise OSError('cannot open verified client read-only')
    def read(address, count):
        if not 0x10000 <= address < 0x7FFFFFFFFFFF:
            raise ValueError('bad address %X' % address)
        data = _read_remote(k32, process, address, count)
        if len(data) != count:
            raise ValueError('short read at %X' % address)
        return data
    qword = lambda address: struct.unpack('<Q', read(address, 8))[0]
    dword = lambda address: struct.unpack('<I', read(address, 4))[0]
    rva = lambda address: module_rva(address, base, size)
    try:
        vtable = base + VOLLEY_VTABLE_RVA
        result = {'base': '%X' % base, 'executor_rva': rva(qword(vtable + 0x1D0)), 'volleys': []}
        for volley in scan_for_qword(k32, process, vtable):
            entry = {'address': '%X' % volley}
            try:
                instance = qword(volley + 0x78)
                entity = state_entity(qword, base, instance)
                world = entity_world(qword, entity)
                instance_vtable = qword(instance)
                entry.update(instance='%X' % instance, instance_vtable_rva=rva(instance_vtable),
                             instance_slot70_rva=rva(qword(instance_vtable + 0x70)),
                             entity='%X' % entity, world='%X' % world, world_vtable_rva=rva(qword(world)),
                             world_refs_entity=[hex(o) for o in range(0, 0x800, 8) if qword(world + o) == entity])
                entry['live'] = live_key(qword, volley, watch)
                entry['infos'] = infos = []
                for map_key, info in info_map(qword, dword, base, entity).items():
                    item = {'key': '%X' % map_key, 'info': '%X' % info}
                    if info:
                        anim = qword(info + 0x40)
                        item.update(shots=dword(info + 0x30), anim='%X' % anim, info_head=read(info, 0x80).hex())
                        if anim:
                            item.update(anim_vtable_rva=rva(qword(anim)), anim_head=read(anim, 0x60).hex())
                    infos.append(item)
            except (ValueError, OSError) as error:
                entry['error'] = str(error)
            result['volleys'].append(entry)
        return result
    finally:
        k32.CloseHandle(process)


RECORDS_COMPONENT = 35  # sub_7FF6D1D604B0: the entity's pending ch2 shot records
RECORD_SIZE = 176
PAGE_READONLY = 0x02


def volley_vtables(k32, process, read, base):
    """The Volley class vtable and the read-only copies tracer.asi gives its items (slot 43 replaced, 14 and 22 kept)."""
    vtable = base + VOLLEY_VTABLE_RVA
    original = read(vtable, 8 * 23)
    copies = [a for a in scan_for_bytes(k32, process, original[:16], protections=(PAGE_READONLY,), max_region=1 << 20)
              if read(a, 8 * 23) == original]
    return [vtable] + copies


def shot_records(qword, dword, base, entity):
    """(event, key, age, consumed) of each record sub_7FF6D1D604B0 matches against a Volley's (+0x44, +0x1C0)."""
    table = qword(entity_world(qword, entity) + 0x140) ^ ((0x4E3494C633194824 - qword(base + TABLE_KEY_RVA)) & MASK)
    component = qword(table + 8 * RECORDS_COMPONENT)
    count, entries = dword(component + 0xB8), qword(component + 0xB0)
    return [(dword(e + 24), qword(e + 144), dword(e + 160), qword(e + 160) >> 32 & 0xFF)
            for e in (entries + RECORD_SIZE * i for i in range(min(count, 64)))]


def watch_records(pid, graph, seconds):
    """Print each change of the Volleys of `graph`: key +0x1C0, counter +0x44, gate +0x1EC, shot index +0x260, flags
    +0x40, and the entity's pending shot records. Read-only; sampled as fast as the reads go (about 1 ms)."""
    validate_pid(pid)
    k32 = _kernel32()
    base, size = find_target_module(k32, pid)
    process = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
    if not process:
        raise OSError('cannot open verified client read-only')
    def read(address, count):
        data = _read_remote(k32, process, address, count) if 0x10000 <= address < 0x7FFFFFFFFFFF else b''
        if len(data) != count:
            raise ValueError('short read at %X' % address)
        return data
    qword = lambda address: struct.unpack('<Q', read(address, 8))[0]
    dword = lambda address: struct.unpack('<I', read(address, 4))[0]
    try:
        volleys = []
        for vtable in volley_vtables(k32, process, read, base):
            for volley in scan_for_qword(k32, process, vtable):
                try:
                    instance = qword(volley + 0x78)
                    if qword(instance + 0x1E8) == graph:
                        volleys.append((volley, state_entity(qword, base, instance)))
                except (ValueError, OSError):
                    pass
        print('volleys', ['%X node=%d entity=%X' % (v, dword(v + 112), e) for v, e in volleys], flush=True)
        last, start = {}, time.monotonic()
        while time.monotonic() - start < seconds:
            for volley, entity in volleys:
                try:
                    now = (qword(volley + 0x1C0), struct.unpack('<H', read(volley + 0x44, 2))[0],
                           read(volley + 0x1EC, 1)[0], dword(volley + 0x260), qword(volley + 0x40) & 0xFFFF,
                           tuple(shot_records(qword, dword, base, entity)))
                except (ValueError, OSError) as error:
                    now = str(error)
                if now != last.get(volley):
                    last[volley] = now
                    if isinstance(now, str):
                        print('%8.3f %X %s' % (time.monotonic() - start, volley, now), flush=True)
                        continue
                    key, event, gate, index, flags, records = now
                    print('%8.3f %X key=%X event=%d gate=%d index=%d flags=%X records=%s' % (
                        time.monotonic() - start, volley, key, event, gate, index, flags,
                        ' '.join('(ev=%d key=%X age=%d done=%d)' % r for r in records) or '-'), flush=True)
    finally:
        k32.CloseHandle(process)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('pid', type=int)
    parser.add_argument('--watch', type=float, default=0.0, help='seconds to wait for an active Volley')
    parser.add_argument('--records', type=lambda text: int(text, 16), metavar='GRAPH',
                        help='watch the shot-record match of the Volleys of this graph GUID (hex) instead')
    parser.add_argument('--seconds', type=float, default=10.0, help='how long --records watches')
    args = parser.parse_args(argv)
    if args.records is not None:
        watch_records(args.pid, args.records, args.seconds)
        return 0
    print(json.dumps(inspect(args.pid, args.watch), indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
