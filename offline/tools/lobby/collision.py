"""Ray casts against a map's original collision (map chunk 0x11: the static mesh and the props' convex hulls;
tools/assets/map_collision.py writes data/<index>.collision). Triangles are two-sided; every filter mask counts as
solid."""
import logging
import math
import struct
from array import array
from pathlib import Path

log = logging.getLogger(__name__)

CELL = 2.0  # metres per side of the uniform lookup grid
DATA = Path(__file__).with_name('data')
MAGIC = b'OWCOL2'  # map_collision.MAGIC; OWCOL1 files lack the props' hulls
_loaded = {}


def current(path):
    """Whether path holds a mesh in this format (run.py re-extracts missing and older ones)."""
    if not path.exists():
        return False
    with path.open('rb') as file:
        return file.read(len(MAGIC)) == MAGIC


class Mesh:
    def __init__(self, vertices, indices):
        """vertices: flat x/y/z; indices: flat a/b/c per triangle."""
        self.tris = array('d')  # per triangle: corner a, edge ab, edge ac
        self.grid = {}
        for i in range(len(indices) // 3):
            a, b, c = (indices[3 * i + k] * 3 for k in range(3))
            pa, pb, pc = list(vertices[a:a + 3]), vertices[b:b + 3], vertices[c:c + 3]
            self.tris.extend(pa)
            self.tris.extend(pb[k] - pa[k] for k in range(3))
            self.tris.extend(pc[k] - pa[k] for k in range(3))
            lo = [math.floor(min(pa[k], pb[k], pc[k]) / CELL) for k in range(3)]
            hi = [math.floor(max(pa[k], pb[k], pc[k]) / CELL) for k in range(3)]
            for x in range(lo[0], hi[0] + 1):
                for y in range(lo[1], hi[1] + 1):
                    for z in range(lo[2], hi[2] + 1):
                        cell = self.grid.get((x, y, z))
                        if cell is None:
                            cell = self.grid[x, y, z] = array('I')
                        cell.append(i)

    @classmethod
    def load(cls, path, bits=None):
        """The mesh in path; with bits, only the triangles whose filter mask has one of them."""
        data = Path(path).read_bytes()
        if data[:6] != MAGIC:
            raise ValueError(f'{path} is not a current collision mesh: rerun tools/assets/map_collision.py')
        nv, nt = struct.unpack_from('<II', data, 6)
        vertices = array('f', data[14:14 + 12 * nv])
        indices = array('I', data[14 + 12 * nv:14 + 12 * nv + 12 * nt])
        if bits is not None:
            masks = array('I', data[14 + 12 * nv + 12 * nt:14 + 12 * nv + 24 * nt])
            indices = array('I', (v for i, mask in enumerate(masks) if mask & bits for v in indices[3 * i:3 * i + 3]))
        return cls(vertices, indices)

    def _hit(self, i, o, d):
        """Distance along unit d from o to triangle i (Moller-Trumbore), or None."""
        t = self.tris
        j = 9 * i
        ax, ay, az, e1x, e1y, e1z, e2x, e2y, e2z = t[j:j + 9]
        px, py, pz = d[1] * e2z - d[2] * e2y, d[2] * e2x - d[0] * e2z, d[0] * e2y - d[1] * e2x
        det = e1x * px + e1y * py + e1z * pz
        if -1e-12 < det < 1e-12:
            return None
        inv = 1.0 / det
        sx, sy, sz = o[0] - ax, o[1] - ay, o[2] - az
        u = (sx * px + sy * py + sz * pz) * inv
        if u < 0 or u > 1:
            return None
        qx, qy, qz = sy * e1z - sz * e1y, sz * e1x - sx * e1z, sx * e1y - sy * e1x
        v = (d[0] * qx + d[1] * qy + d[2] * qz) * inv
        if v < 0 or u + v > 1:
            return None
        dist = (e2x * qx + e2y * qy + e2z * qz) * inv
        return dist if dist >= 0 else None

    def normal(self, i):
        j = 9 * i
        e1x, e1y, e1z, e2x, e2y, e2z = self.tris[j + 3:j + 9]
        n = (e1y * e2z - e1z * e2y, e1z * e2x - e1x * e2z, e1x * e2y - e1y * e2x)
        size = math.hypot(*n) or 1.0
        return tuple(k / size for k in n)

    def ray(self, origin, direction, length):
        """(distance, triangle) of the first surface within length along unit direction, or None."""
        cell = [math.floor(origin[k] / CELL) for k in range(3)]
        step, t_max, t_delta = [0] * 3, [math.inf] * 3, [math.inf] * 3
        for k in range(3):
            if direction[k] > 0:
                step[k], t_max[k] = 1, ((cell[k] + 1) * CELL - origin[k]) / direction[k]
                t_delta[k] = CELL / direction[k]
            elif direction[k] < 0:
                step[k], t_max[k] = -1, (cell[k] * CELL - origin[k]) / direction[k]
                t_delta[k] = -CELL / direction[k]
        best, hit, seen, entered = length, None, set(), 0.0
        while entered <= best:
            for i in self.grid.get(tuple(cell), ()):
                if i not in seen:
                    seen.add(i)
                    dist = self._hit(i, origin, direction)
                    if dist is not None and dist <= best:
                        best, hit = dist, i
            k = t_max.index(min(t_max))
            entered = t_max[k]
            if entered > best:
                break
            cell[k] += step[k]
            t_max[k] += t_delta[k]
        return None if hit is None else (best, hit)

    def clear(self, start, end):
        """Whether nothing solid lies on the segment start-end."""
        delta = [e - s for s, e in zip(start, end)]
        length = math.hypot(*delta)
        if not length:
            return True
        return self.ray(start, [k / length for k in delta], length) is None


def movement_world(game_map):
    """The player mover's collision for a map and its mode: the cache vendor ow174/game/collision.py reads (built
    by build_movement), loaded once; None (logged) without one, and the mover walks on a flat floor."""
    from ow174.game import collision as movement
    return movement.world_for(game_map.map_guid, game_map.name, build_missing=False, mode=game_map.mode_guid)


def build_movement(map_guids, game_install):
    """Build the player mover's caches of these maps from the game install where missing or outdated (~1 s each)."""
    from ow174.game import casc, collision as movement
    storage = None
    for guid in map_guids:
        if guid in movement.collision_keys() and movement.cache_version(movement.cache_path(guid)) != movement.VERSION:
            storage = storage or casc.Storage(casc.game_root(Path(game_install) / '_retail_' / 'Overwatch.exe'))
            log.info('built %s', movement.build(guid, storage))


def for_map(map_guid):
    """The collision mesh for a map, or None (logged) when its file has not been extracted."""
    index = map_guid & 0xFFFFFFFF
    if index not in _loaded:
        path = DATA / f'{index:04X}.collision'
        if path.exists():
            _loaded[index] = Mesh.load(path)
            log.info('collision %s: %d triangles', path.name, len(_loaded[index].tris) // 9)
        else:
            _loaded[index] = None
            log.warning('no collision for map %X: run tools/assets/map_collision.py %s', index, path)
    return _loaded[index]
