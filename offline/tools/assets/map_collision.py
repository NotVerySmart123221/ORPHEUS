"""Extract a map's collision (map chunk type 0x11: the static mesh plus the props' convex hulls) for the local
game server.

    python map_collision.py OUT [--map-index 0x688] [--game-install <game-root>]

Chunk 0x0DD000110000IIII (build 104319; layout in research/map-collision.md):
    +0x08 u32 hull count; +0x0C u32 mesh count; +0x20 u64 hull record offset; +0x28 u64 footer offset
    footer: one 56-byte record per mesh: u64 node offset, u64 vertex offset, u64 triangle offset,
        u32 node count, u32 vertex count, u32 triangle count, ... (the first mesh is the map's)
    vertices: f32 x, y, z, pad (16 bytes, world space, y up)
    nodes: 32 bytes each (a BVH; not used here)
    triangles: u32 a, b, c, three neighbour vertices (-1 = open edge), material, filter mask (32 bytes)
    hulls: 88 bytes each: u64 offsets of the vertices (f32 x, y, z; world space), planes, half-edges and
        per-face first half-edges (u8), then at +0x2C u32 vertex, face and half-edge counts, at +0x4C u32 filter mask.
        A half-edge is s8 twin offset, u8 vertex, u8 face, u8 next half-edge of the face.

OUT gets: b'OWCOL2', u32 vertex count, u32 triangle count, f32 x/y/z per vertex, u32 a/b/c per
triangle, u32 mask per triangle. The hulls follow the mesh as fan-triangulated faces.
"""
from __future__ import annotations

import os
import argparse
import hashlib
import struct
import sys
from pathlib import Path

MAGIC = b'OWCOL2'  # OWCOL1 (same layout) lacked the hulls
CHUNK_TYPE = 0x11


def parse_chunk(blob: bytes) -> tuple[list[float], list[int], list[int]]:
    """(flat x/y/z vertices, flat a/b/c triangle indices, masks) from a type-0x11 map chunk: the map's mesh, then
    the props' convex hulls (tables, crates), which the mesh lacks."""
    hull_count = struct.unpack_from('<I', blob, 0x08)[0]
    hull_off, footer = struct.unpack_from('<QQ', blob, 0x20)
    # shortcut: the +0x00 sphere (Paris: 22) and +0x04 capsule (Anubis 1, Volskaya 4) records, 5-10 cm thick and none
    # on the Range, are skipped, until a map's movement needs them
    # shortcut: reads only the first mesh; Paris's second one (a 14-triangle plane at y=6, mask 0x242) is skipped,
    # until a map needs its extra meshes
    node_off, vert_off, tri_off, node_count, vert_count, tri_count = struct.unpack_from('<QQQIII', blob, footer)
    tri_end = tri_off + tri_count * 32
    if node_off - vert_off != vert_count * 16 or tri_off - node_off != node_count * 32 or tri_end > len(blob):
        raise ValueError('collision chunk blocks do not match their counts')
    vertices = []
    for x, y, z, _ in struct.iter_unpack('<4f', blob[vert_off:node_off]):
        vertices += (x, y, z)
    indices, masks = [], []
    for a, b, c, _, _, _, _, mask in struct.iter_unpack('<8I', blob[tri_off:tri_end]):
        if max(a, b, c) >= vert_count:
            raise ValueError('triangle refers past the vertex block')
        indices += (a, b, c)
        masks.append(mask)
    for record in range(hull_off, hull_off + 88 * hull_count, 88):
        vert_off, _, edge_off, face_off = struct.unpack_from('<4Q', blob, record)
        vert_count, face_count, edge_count = struct.unpack_from('<3I', blob, record + 0x2C)
        mask = struct.unpack_from('<I', blob, record + 0x4C)[0]
        if max(vert_off + 12 * vert_count, edge_off + 4 * edge_count, face_off + face_count) > len(blob):
            raise ValueError('hull data runs past the chunk')
        base = len(vertices) // 3
        vertices += struct.unpack_from(f'<{3 * vert_count}f', blob, vert_off)
        edges = list(struct.iter_unpack('<xBxB', blob[edge_off:edge_off + 4 * edge_count]))  # (vertex, next)
        for first in blob[face_off:face_off + face_count]:
            loop, edge = [], first
            while not loop or edge != first:
                if edge >= edge_count or len(loop) == edge_count:
                    raise ValueError('hull face loop does not close')
                vertex, edge = edges[edge]
                loop.append(vertex)
            if len(loop) < 3 or max(loop) >= vert_count:
                raise ValueError('hull face refers past its vertices')
            for k in range(1, len(loop) - 1):
                indices += (base + loop[0], base + loop[k], base + loop[k + 1])
                masks.append(mask)
    return vertices, indices, masks


def write_mesh(path: Path, vertices: list[float], indices: list[int], masks: list[int]) -> None:
    path.write_bytes(MAGIC + struct.pack('<II', len(vertices) // 3, len(masks))
        + struct.pack(f'<{len(vertices)}f', *vertices) + struct.pack(f'<{len(indices)}I', *indices)
        + struct.pack(f'<{len(masks)}I', *masks))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('out', type=Path)
    parser.add_argument('--map-index', type=lambda value: int(value, 0), default=0x688)
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    import practice_range_assets as pra

    guid = 0x0DD0000000000000 | CHUNK_TYPE << 32 | args.map_index
    storage = pra.load_storage(args.game_install)
    try:
        ckey, name = pra.root_manifest_key(storage, pra.ROOT_MANIFEST)
        _, records = pra.parse_cmf(storage.cat(ckey), name)
        content = next((ck for g, _, ck in records if g == guid), None)
        blob = None if content is None else storage.cat(content)
        if blob is None or hashlib.md5(blob).digest() != content:
            raise SystemExit(f'collision chunk {guid:016X} missing or failed its content-key check')
    finally:
        for handle in storage._files.values():
            handle.close()
    vertices, indices, masks = parse_chunk(blob)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    write_mesh(args.out, vertices, indices, masks)
    print(f'{guid:016X}: {len(vertices) // 3} vertices, {len(masks)} triangles -> {args.out}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
