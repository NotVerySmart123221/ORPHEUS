"""Read direct animation bindings from a retail STUAnimBlendTreeSet."""
import struct
from collections import Counter, defaultdict

from retail_bundle_lookup import _decode_asset_ref, stu_header_crc64
from stu_graph import instances
from stu_refs import plausible

ALIASES, ITEMS = 0x253EE7C8, 0x6AFCD1A5
PARAMETERS, ANIMATIONS = 0xF6E6D4B1, 0x5AD927D3


def array(blob, dynamic, field, stride):
    count, offset = struct.unpack_from('<QQ', blob, dynamic + struct.unpack('<I', field)[0])
    start = dynamic + offset
    if start + count * stride > len(blob):
        raise ValueError('blend-tree array exceeds the asset')
    return start, count


def item_parameters(blob, items):
    """Embedded parameter instance index for each inline blend-tree item."""
    bag_count, bags = struct.unpack_from('<II', blob, 16)
    count = struct.unpack_from('<I', items)[0]
    pos = 4
    for _ in range(count):
        bag = struct.unpack_from('<i', items, pos)[0]
        pos += 4
        parameter = -1
        if bag >= bag_count:
            raise ValueError('blend-tree item names a missing bag')
        if bag >= 0:
            fields, table = struct.unpack_from('<II', blob, bags + 8 * bag)
            for i in range(fields):
                key, width = struct.unpack_from('<II', blob, table + 8 * i)
                if width == 0:
                    width = struct.unpack_from('<I', items, pos)[0]
                    pos += 4
                if pos + width > len(items):
                    raise ValueError('truncated blend-tree item')
                if key == PARAMETERS:
                    parameter = struct.unpack('<i', items[pos:pos + width])[0]
                pos += width
        yield parameter
    if pos != len(items):
        raise ValueError('blend-tree item array length mismatch')


def direct_bindings(blob):
    """Yield direct (alias, clip) candidates for ambiguity checks across sets."""
    rows = instances(blob)
    root = rows[0][3]
    if ALIASES not in root or ITEMS not in root:
        return
    dynamic = struct.unpack_from('<I', blob, 28)[0]
    start, count = array(blob, dynamic, root[ALIASES], 8)
    aliases = struct.unpack_from(f'<{count}Q', blob, start)
    crc = stu_header_crc64(blob)
    for alias, parameter in zip(aliases, item_parameters(blob, root[ITEMS]), strict=True):
        if not plausible(alias, {0x0C80}) or parameter < 0:
            continue
        if parameter >= len(rows):
            raise ValueError('blend-tree item names a missing parameter instance')
        field = rows[parameter][3].get(ANIMATIONS)
        if field is None:
            continue  # alias proxies can select different clips at runtime
        start, count = array(blob, dynamic, field, 16)
        clips = {_decode_asset_ref(struct.unpack_from('<Q', blob, start + 16 * i + 8)[0], ANIMATIONS, crc)
                 for i in range(count)}
        for clip in clips:
            if plausible(clip, {0x0A00}):
                yield alias, clip


def unambiguous_bindings(bindings):
    candidates = defaultdict(set)
    for alias, clip in bindings:
        candidates[alias].add(clip)
    return {alias: next(iter(clips)) for alias, clips in candidates.items() if len(clips) == 1}


def shared_pairs(parent, child):
    """One-to-one clip pairs joined by a shared authored alias."""
    candidates = {(parent[alias], child[alias]) for alias in parent.keys() & child.keys()}
    parent_counts = Counter(first for first, _ in candidates)
    child_counts = Counter(second for _, second in candidates)
    return {first: second for first, second in sorted(candidates)
            if parent_counts[first] == 1 and child_counts[second] == 1}
