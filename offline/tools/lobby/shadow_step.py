"""Where Shadow Step's aim puts Reaper: the client's own targeting query on the mover's collision
(research/lizrollin-runtime.md sections 36 to 40).

The client's targeting state (STU_4C96470A) runs 0x7FF6D1EAEB30 every tick and draws its marker where that says:
1. A sphere of SWEEP is swept from the eye along the aim for the node's reach. Where it touches a floor the first
   start is over the contact, where it touches anything else STEP off it along the surface's normal; the second
   start is STEP back along the aim. A start the sphere cannot get to from where it touched gives way to the other;
   with neither there is no place. Where the sphere touches nothing, the aim's end is both starts.
2. Places are looked for under the line from each start back to the eye (0x7FF6D1EC0E50, its probe 0x7FF6D1EBE800):
   - under the first start and under the point STEP nearer the eye, by a ray straight down, until one is a place;
   - under the line from the second start to the eye, in stretches of about SPAN: each stretch, as a line as
     thick as the sphere, is let down until it touches something. Two stretches after the first place it stops.
   Neither goes deeper than `_depth` (0x7FF6D1EBC760: a cone of CONE under the aim, as seen from the eye).
3. What a ray or a stretch touched is a place if it is a floor (`_ground`) and Reaper may stand there (`_stands`).
4. Of the places found, the one nearest the middle of a picture wins, by its height in the picture alone (the
   first found wins a tie). The picture is taken toward the first start from BACK behind the eye.
What the casts meet is the mover's collision less the placed props with the flag 0x80 (the Range's desks: the
recorded markers go through them and stand on its railings), and no place is on a polytope the chunk flags.

shortcut: the map's own bounds (0x7FF6D1E3F280: its lowest height and its volumes) neither cut the aim short nor
rule a place out; a place on a moving thing is taken as one on the map; the sight rays are 1 cm thick; a placed
prop's own mesh is a floor by the contact's normal, and a ray's hit is checked against the chunk's meshes whatever
it hit. Read or add them when a map needs it.
The node's least distance from the eye (0) and its height limit (unset) are left out: Reaper's node has neither.
"""
import math
from functools import lru_cache

from ow174.game import contacts
from ow174.game.collision import MOVER, NO_STAND, NOT_GROUND, PASSED

SWEEP, SKIN = 0.115, 0.015  # the swept sphere's radius (the node's 0.1 + 0.015) and the 0.015 it is pulled back by
STEP = 0.5  # the node's 0.5: how far off a surface a start is put, and how far apart the first start's two rays are
SPAN = 3.0  # the node's 3: the length of a stretch of the line from the second start
FLOOR = 0.71934  # a surface whose normal is this upright is a floor
CONE = 0.17453277  # radians, the node's angle: how far under the aim's line a place may lie, seen from the eye
SEEN = (0.85, 0.75)  # metres over a place the eye has to see: half the node's 1.5 and 0.1, and the node's 0.75
ROOM = (0.4, 1.5)  # half the node's 0.8 and its 1.5: the sphere that has to fit over a place, and how far it rises
BACK = 1.0  # metres behind the eye the pick's camera stands
NEAR = 0.05  # the floor check looks at a mesh's triangles this near what was touched, from this far over it
TIE = 1.19209e-05  # its margin between two triangles equally near
POINT = ((0.0, 0.0, 0.0),)


@lru_cache(maxsize=8)
def _met(world) -> contacts.StaticWorld:
    """The shapes of a map that the query's casts meet: the mover's, less the props they go through."""
    passed = lambda shape: shape.frame is not None and shape.mask & PASSED  # the chunk's mesh has every bit
    return contacts.StaticWorld([shape for shape in world.engine.shapes if not passed(shape)])


@lru_cache(maxsize=8)
def _meshes(world) -> tuple:
    """The chunk's own meshes (in the map's frame)."""
    return tuple(s.geometry for s in world.engine.shapes if s.kind == contacts.MESH and s.frame is None)


def _cast(world, start, end, points=POINT, radius: float = 0.0, inside: bool = False):
    """The client's physics query (0x7FF6D1F817C0) on the mover's collision; `inside`: a start inside something is
    a hit."""
    return contacts.scene_sweep(_met(world), start, end, points, radius, MOVER, inside)


def _depth(along: float, aim_y: float, most: float) -> float:
    """How far under a point on the line a place may be, `along` metres from the eye (0x7FF6D1EBC760: the law of
    sines in the triangle of the eye, the point and the cone's edge, with the angle at the point taken as linear in
    the aim's vertical part), the aim's own length at most, plus STEP."""
    angle = math.pi - CONE - (1.0 - aim_y) * math.pi / 2
    return (most if angle <= 0.0 else min(most, along * math.sin(CONE) / math.sin(angle))) + STEP


def _starts(world, eye, aim, reach: float):
    """(the first start, the second, the sphere's radius from here on), or None where the sphere cannot get to
    either."""
    end = tuple(e + a * reach for e, a in zip(eye, aim))
    hit = _cast(world, eye, end, radius=SWEEP)
    if hit is None:
        return end, end, SWEEP
    fraction, point, normal = hit[:3]
    # over a floor the client still multiplies by the normal: STEP times its upright part, straight up
    off = (0.0, STEP * normal[1], 0.0) if normal[1] > FLOOR else tuple(n * STEP for n in normal)
    before = tuple(e + a * max(reach * fraction - SKIN, 0.0) for e, a in zip(eye, aim))
    starts = (tuple(c + o for c, o in zip(point, off)), tuple(c - a * STEP for c, a in zip(point, aim)))
    free = [start for start in starts if _cast(world, before, start, radius=SWEEP - SKIN) is None]
    return (free[0], free[-1], SWEEP - SKIN) if free else None


def _floor(mesh, point):
    """Whether a mesh is a floor where a cast touched it (0x7FF6D1EC5550), None with no triangle there. The
    triangles within NEAR of the point are gone through in their order, each by its own face: walls and undersides
    are not looked at, and of the others the one nearest the point NEAR over the hit decides, a floor the map does
    not bar. One too steep that is as near takes a floor's yes away again."""
    x, y, z = point
    over = (x, y + NEAR, z)
    numbers = mesh.query((x - NEAR, y - NEAR, z - NEAR), (x + NEAR, y + NEAR, z + NEAR), MOVER)
    if not numbers:
        return None
    good, best = False, 1000.0
    for number in numbers:
        triangle = mesh.triangle(number)
        nearest = contacts.closest_on_triangle(*triangle.corners, over)
        gap = sum((a - b) ** 2 for a, b in zip(over, nearest))
        upright = triangle.normal[1]
        if gap > (best - TIE if good else best + TIE) or upright < TIE:
            continue
        good = upright > FLOOR and not triangle.surface & NOT_GROUND
        if upright > FLOOR:
            best = gap
    return good


def _ground(meshes, point, upright: float, barred: int) -> bool:
    """Whether what a cast touched is a floor: by the mesh it touched (`_floor`), or with none (a polytope, or no
    triangle near) by the contact's own normal and by `barred` (a triangle's surface bit, a polytope's flag)."""
    for mesh in meshes:
        floor = _floor(mesh, point)
        if floor is not None:
            return floor
    return upright > FLOOR and not barred


def _stands(world, eye, point, descended: float) -> bool:
    """Whether Reaper may stand on a floor a cast straight down touched (0x7FF6D1EBDA00): the eye in sight of two
    heights over it (from the upper one both ways) and room over it for a sphere of ROOM, swept upward. In these
    casts a start inside something is a hit (their filter's one bit more than the others'; a place 6 cm too near a
    wall is refused in the recorded markers)."""
    x, y, z = point
    y += min(descended, SKIN)
    upper, lower = ((x, y + height, z) for height in SEEN)
    radius, rise = ROOM
    low = y + radius + 0.1
    room = ((x, low, z), (x, low + rise - radius, z), radius)
    checks = ((upper, eye, 0.0), (eye, upper, 0.0), (eye, lower, 0.0), room)
    return not any(_cast(world, start, end, radius=size, inside=True) for start, end, size in checks)


def place(world, eye, aim, reach: float):
    """The feet's place (x, y, z) for an aim from `eye` along the unit vector `aim`; None where it finds none."""
    starts = _starts(world, eye, aim, reach)
    if starts is None:
        return None
    first, second, radius = starts
    found = []

    def line(start):  # the unit vector from a start back to the eye, and how far that is
        back = tuple(e - s for e, s in zip(eye, start))
        length = math.hypot(*back)
        return tuple(b / (length or 1.0) for b in back), length

    def probe(sample, along: float, other=None) -> bool:
        """Look under a point of the line: by a ray, or with `other` by the stretch from the point to it."""
        x, y, z = sample
        drop = _depth(along, aim[1], reach)
        if other is None:  # a ray ends on the surface it meets and has that triangle's own normal
            floor = world.ground(x, z, y, y - drop, skip=PASSED)
            spot = floor and ((x, floor.y, z), y - floor.y, _meshes(world), floor.normal[1],
                              floor.surface & NOT_GROUND | floor.mask & NO_STAND)
        else:  # a swept line ends the engine's 1 cm off what it touches, with the contact's normal
            stretch = (POINT[0], tuple(o - s for o, s in zip(other, sample)))
            hit = _cast(world, sample, (x, y - drop, z), stretch, radius)
            shape = hit and hit[3]
            mesh = hit and shape.kind == contacts.MESH
            barred = hit and (shape.geometry.surfaces[hit[4]] & NOT_GROUND if mesh else shape.mask & NO_STAND)
            spot = hit and (hit[1], drop * hit[0], (shape.geometry,) if mesh and shape.frame is None else (),
                            hit[2][1], barred)
        good = bool(spot) and _ground(spot[2], spot[0], *spot[3:]) and _stands(world, eye, *spot[:2])
        if good:
            found.append(spot[0])
        return good

    # The client's loops stop early once they have a place (the switch at img+0x2E2EBC8, on in the runtime image):
    # the first start's after its first, the second start's two stretches on.
    back, length = line(first)
    for i in range(min(2, int(length / STEP + 0.5))):
        if probe(tuple(s + b * i * STEP for s, b in zip(first, back)), length - i * STEP):
            break
    back, length = line(second)
    count = last = int(length / SPAN + 0.5)
    higher = 1 if back[1] >= 0.0 else -1  # each stretch runs from its point to the next one up the line
    for i in range(count):
        if i >= last:
            break
        sample, other = (tuple(s + b * k * length / count for s, b in zip(second, back)) for k in (i, i + higher))
        # nothing up the line from the first point of an aim upward: a ray
        if probe(sample, length - i * length / count, other if i + higher >= 0 else None):
            last = min(last, i + 3)
    if not found:
        return None
    # The client's camera for the pick looks at the first start from BACK behind the eye, upright.
    behind, _ = line(first)
    ahead = tuple(-c for c in behind)
    camera = tuple(e + b * BACK for e, b in zip(eye, behind))
    level = math.hypot(ahead[0], ahead[2])
    across = (-ahead[0] * ahead[1] / level, level, -ahead[2] * ahead[1] / level) if level else (0.0, 1.0, 0.0)

    def seen(p) -> float:  # how far over or under the middle of that camera's picture the point is
        to = tuple(c - e for c, e in zip(p, camera))
        return abs(sum(t * a for t, a in zip(to, across)) / max(sum(t * a for t, a in zip(to, ahead)), 1e-6))

    return min(found, key=seen)
