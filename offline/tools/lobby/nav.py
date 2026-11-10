"""Walkable grid for server bots, sampled from a map's collision mesh (collision.py), with A* paths.

Each 1 m column inside the play area is probed with downward rays; every upward-facing surface with
standing room above it is a node. Neighbouring nodes (8 directions) link when the step between them is
at most STEP up (any drop up to DROP is allowed going down) and nothing solid blocks the knee, waist or
head lines between them (passable). The grid for a map is cached in data/<index>.nav.
"""
import heapq
import logging
import math
import pickle

import collision

log = logging.getLogger(__name__)

SIZE = 1.0  # metres per column
STEP = 0.6  # tallest step up between neighbouring columns
DROP = 3.0  # tallest drop a bot walks off
HEADROOM = 1.8
LEVEL = 0.3  # most a smoothed route's straight line strays from the floor (a stair's nosing)
FLOOR = 0.7  # minimum upward normal of a floor
MARGIN = 25.0  # metres around the spawns and objectives
# Filter bit shared by the floors and walls (masks 0x1FDF, 0x4C0, 0x1B9F); triangles without it, such as
# mask 0x1A at spawn exits, are taken as barriers bots pass. shortcut: inferred from Hanamura, not decoded.
SOLID = 0x80
VERSION = 3
_grids = {}


class Grid:
    def __init__(self, nodes, links):
        self.nodes = nodes  # id -> (x, y, z) feet position
        self.links = links  # id -> [neighbour id]
        self.columns = {}  # (x, z) column -> node ids, for nearest()
        for n, (x, _, z) in nodes.items():
            self.columns.setdefault((math.floor(x / SIZE), math.floor(z / SIZE)), []).append(n)

    def nearest(self, position, within=None):
        """The node closest to position, preferring ones within 2 m of its height; only nodes in the set
        `within` when given."""
        x, y, z = position
        key = lambda n: (math.dist((x, z), (self.nodes[n][0], self.nodes[n][2]))
                         + 10 * max(0.0, abs(self.nodes[n][1] - y) - 2.0))
        cx, cz = math.floor(x / SIZE), math.floor(z / SIZE)
        for r in (2, 8, 32):  # shortcut: the nearest column square with any node, not an exact radius search
            near = [n for i in range(cx - r, cx + r + 1) for j in range(cz - r, cz + r + 1)
                    for n in self.columns.get((i, j), ()) if within is None or n in within]
            if near:
                return min(near, key=key)
        return min(self.nodes if within is None else within, key=key)

    def region(self, position):
        """The nodes reachable from the node nearest position that can also walk back to it."""
        start = self.nearest(position)
        back = {}
        for n, ms in self.links.items():
            for m in ms:
                back.setdefault(m, []).append(n)
        def reach(links):
            seen, todo = {start}, [start]
            while todo:
                for m in links.get(todo.pop(), ()):
                    if m not in seen:
                        seen.add(m)
                        todo.append(m)
            return seen
        return reach(self.links) & reach(back)

    def path(self, start, goal, within=None):
        """Feet positions from near start to the reachable node closest to goal. With `within` (a region()
        holding start) the goal snaps into it, so the search stops there instead of exhausting the map
        when the goal itself cannot be reached."""
        a = self.nearest(start)
        b = self.nearest(goal, within if within is not None and a in within else None)
        target = self.nodes[b]
        best, came = {a: 0.0}, {a: None}
        frontier, closest = [(0.0, a)], a
        while frontier:
            _, n = heapq.heappop(frontier)
            if math.dist(self.nodes[n], target) < math.dist(self.nodes[closest], target):
                closest = n
            if n == b:
                break
            for m in self.links[n]:
                cost = best[n] + math.dist(self.nodes[n], self.nodes[m])
                if cost < best.get(m, math.inf):
                    best[m], came[m] = cost, n
                    heapq.heappush(frontier, (cost + math.dist(self.nodes[m], target), m))
        route, n = [], closest
        while n is not None:
            route.append(self.nodes[n])
            n = came[n]
        return route[::-1]


def floors(mesh, x, z, top, bottom):
    """Heights of the walkable surfaces in the column at x, z, from top down to bottom."""
    found, y = [], top
    while y > bottom:
        hit = mesh.ray((x, y, z), (0.0, -1.0, 0.0), y - bottom)
        if hit is None:
            break
        surface = y - hit[0]
        if mesh.normal(hit[1])[1] ** 2 >= FLOOR ** 2:  # triangles are two-sided
            above = mesh.ray((x, surface + 0.05, z), (0.0, 1.0, 0.0), HEADROOM)
            if above is None:
                found.append(surface)
        y = surface - 0.05
    return found


def build(mesh, points):
    """The grid over the box around points (spawns, objectives) plus MARGIN."""
    xs, ys, zs = zip(*points)
    x0, x1 = math.floor(min(xs) - MARGIN), math.ceil(max(xs) + MARGIN)
    z0, z1 = math.floor(min(zs) - MARGIN), math.ceil(max(zs) + MARGIN)
    top, bottom = max(ys) + 30.0, min(ys) - 30.0
    nodes, columns = {}, {}
    for i in range(int((x1 - x0) / SIZE)):
        for k in range(int((z1 - z0) / SIZE)):
            x, z = x0 + (i + 0.5) * SIZE, z0 + (k + 0.5) * SIZE
            for y in floors(mesh, x, z, top, bottom):
                columns.setdefault((i, k), []).append(len(nodes))
                nodes[len(nodes)] = (x, y, z)
    links = {n: [] for n in nodes}
    for (i, k), here in columns.items():
        for di, dk in ((1, 0), (0, 1), (1, 1), (1, -1)):  # each pair once
            for a in here:
                for b in columns.get((i + di, k + dk), ()):
                    pa, pb = nodes[a], nodes[b]
                    rise = pb[1] - pa[1]
                    if abs(rise) > DROP or not passable(mesh, pa, pb):
                        continue
                    if rise <= STEP:
                        links[a].append(b)
                    if -rise <= STEP:
                        links[b].append(a)
    return Grid(nodes, links)


def passable(mesh, a, b):
    """Whether a bot walks straight from feet position a to b without a wall at knee, waist or head height."""
    return all(mesh.clear((a[0], a[1] + h, a[2]), (b[0], max(a[1], b[1]) + h, b[2])) for h in (0.7, 1.1, 1.6))


def smooth(mesh, route):
    """The route without the grid's zigzag: a point goes when the bot walks straight from the last point kept to the
    next one with the floor within LEVEL of that line every half metre, so it neither sinks nor floats."""
    kept = list(route[:1])
    for here, there in zip(route[1:-1], route[2:]):
        if not (passable(mesh, kept[-1], there) and _floored(mesh, kept[-1], there)):
            kept.append(here)
    return kept + list(route[-1:]) if len(route) > 1 else kept


def _floored(mesh, a, b):
    steps = math.ceil(2 * math.dist(a, b) / SIZE)
    for k in range(1, steps):
        x, y, z = (p + (q - p) * k / steps for p, q in zip(a, b))
        if mesh.ray((x, y + LEVEL, z), (0.0, -1.0, 0.0), 2 * LEVEL) is None:
            return False
    return True


def loop(grid, mesh, start, points):
    """A patrol loop through points that never walks into a wall: each blocked leg (the last one back to the first
    too) goes over the grid, and the loop starts at its point nearest start that start walks straight to."""
    route = []
    for a, b in zip(points, points[1:] + points[:1]):
        route.append(a)
        if not passable(mesh, a, b):
            route += smooth(mesh, grid.path(a, b))
    reachable = [point for point in route if passable(mesh, start, point)]
    first = route.index(min(reachable, key=lambda point: math.dist(point, start))) if reachable else 0
    return tuple(route[first:] + route[:first])


def nav_points(game_map):
    """The positions the grid has to cover: spawns and objectives."""
    return [spawn.position for spawn in game_map.spawns] + list(game_map.objectives)


def for_map(map_guid, points):
    """The map's grid from its cache, building and saving it on first use; None without a collision mesh."""
    index = map_guid & 0xFFFFFFFF
    if index not in _grids:
        _grids[index] = _load(index, points)
    return _grids[index]


def _load(index, points):
    path = collision.DATA / f'{index:04X}.nav'
    mesh_path = collision.DATA / f'{index:04X}.collision'
    if not mesh_path.exists():
        return None
    if path.exists() and path.stat().st_mtime >= mesh_path.stat().st_mtime:  # a re-extracted mesh rebuilds the grid
        version, nodes, links = pickle.loads(path.read_bytes())
        if version == VERSION:
            return Grid(nodes, links)
    log.info('building nav grid %s', path.name)
    grid = build(collision.Mesh.load(mesh_path, SOLID), points)
    path.write_bytes(pickle.dumps((VERSION, grid.nodes, grid.links)))
    log.info('nav %s: %d nodes', path.name, len(grid.nodes))
    return grid
