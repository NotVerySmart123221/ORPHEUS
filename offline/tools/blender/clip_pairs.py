"""Verified first-person pairs, keyed by entity and clip GUID on each side."""

PAIRS = (
    # Soldier reload. Both blend-tree sets reference tree 0x0F80000000000009.
    (('0x04000000000003D1', '0x0A00000000001C00'), ('0x04000000000004BF', '0x0A00000000001D8C')),
    # Ashe alias 84, the default, 119, and 11A insertion branches of tree 63F.
    (('0x040000000000251D', '0x0A00000000005A0A'), ('0x0400000000002523', '0x0A00000000005A4E')),
    (('0x040000000000251D', '0x0A0000000000603E'), ('0x0400000000002523', '0x0A00000000006051')),
    (('0x040000000000251D', '0x0A00000000006020'), ('0x0400000000002523', '0x0A00000000006050')),
)


def paired_clip(entity, clip, target, available, authored=()):
    """Return a verified counterpart only when it exists in the target's clips."""
    for first, second in (*PAIRS, *authored):
        if (entity, clip) == tuple(second):
            first, second = second, first
        if (entity, clip) == tuple(first) and target == second[0] and second[1] in available:
            return second[1]
    return None
