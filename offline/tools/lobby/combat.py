"""Pure geometry for local server-side hitscan checks.

This only tests a ray against supplied target spheres. It does not model map occlusion,
weapon timing, damage, or client/server movement reconciliation.
"""

import math
from collections.abc import Iterable

TURN = 2 * math.pi / 65536

Vector = tuple[float, float, float]


def aim_direction(yaw: int, pitch: int) -> Vector:
    """Convert signed command angles to a unit aim vector; positive pitch looks down."""
    if type(yaw) is not int or not -32768 <= yaw <= 32767:
        raise ValueError("yaw must be a signed 16-bit angle")
    if type(pitch) is not int or not -32768 <= pitch <= 32767:
        raise ValueError("pitch must be a signed 16-bit angle")
    yaw_radians = yaw * TURN
    pitch_radians = pitch * TURN
    level = math.cos(pitch_radians)
    return (
        math.sin(yaw_radians) * level,
        -math.sin(pitch_radians),
        math.cos(yaw_radians) * level,
    )


def _vector3(name: str, value: Iterable[float]) -> Vector:
    if isinstance(value, (str, bytes, bytearray)):
        raise ValueError(f"{name} must contain three finite numbers")
    try:
        vector = tuple(float(axis) for axis in value)
    except (TypeError, ValueError, OverflowError):
        raise ValueError(f"{name} must contain three finite numbers") from None
    if len(vector) != 3 or not all(math.isfinite(axis) for axis in vector):
        raise ValueError(f"{name} must contain three finite numbers")
    return vector[0], vector[1], vector[2]


def ray_sphere_distance(
    origin: Iterable[float],
    direction: Iterable[float],
    center: Iterable[float],
    radius: float,
    max_distance: float = math.inf,
) -> float | None:
    """Return the nearest ray/sphere intersection distance, or None for a miss.

    Direction is normalized internally. A ray starting inside the sphere hits at distance zero.
    """
    origin3 = _vector3("origin", origin)
    direction3 = _vector3("direction", direction)
    center3 = _vector3("center", center)
    if type(radius) not in (int, float) or not math.isfinite(radius) or radius <= 0:
        raise ValueError("radius must be a finite positive number")
    if type(max_distance) not in (int, float) or math.isnan(max_distance) or max_distance < 0:
        raise ValueError("max_distance must be a non-negative number")

    magnitude = math.hypot(*direction3)
    if magnitude == 0:
        raise ValueError("direction must be non-zero")
    direction3 = tuple(axis / magnitude for axis in direction3)
    offset = tuple(start - target for start, target in zip(origin3, center3, strict=True))
    radius_squared = radius * radius
    if not all(math.isfinite(value) for value in (*offset, radius_squared)):
        raise ValueError("geometry exceeds finite numeric range")
    projection = sum(start * axis for start, axis in zip(offset, direction3, strict=True))
    constant = sum(axis * axis for axis in offset) - radius_squared
    if not math.isfinite(projection) or not math.isfinite(constant):
        raise ValueError("geometry exceeds finite numeric range")
    if constant <= 0:
        return 0.0

    discriminant = projection * projection - constant
    if not math.isfinite(discriminant):
        raise ValueError("geometry exceeds finite numeric range")
    if discriminant < 0:
        return None
    distance = -projection - math.sqrt(discriminant)
    if distance < 0:
        distance = -projection + math.sqrt(discriminant)
    if distance < 0 or distance > max_distance:
        return None
    return distance


def segment_distance(point: Vector, a: Vector, b: Vector) -> float:
    """Distance from point to the segment a-b."""
    axis = [q - p for p, q in zip(a, b)]
    length2 = sum(v * v for v in axis)
    s = 0.0 if not length2 else max(0.0, min(1.0, sum((p - q) * v for p, q, v in zip(point, a, axis)) / length2))
    return math.dist(point, [q + v * s for q, v in zip(a, axis)])


def ray_capsule_distance(origin, direction, a, b, radius: float, max_distance: float = math.inf) -> float | None:
    """Nearest ray/capsule intersection distance (the segment a-b swept by radius), or None for a miss.

    A ray starting inside the capsule hits at distance zero; a == b is a sphere.
    """
    origin3, a3, b3 = _vector3("origin", origin), _vector3("a", a), _vector3("b", b)
    if segment_distance(origin3, a3, b3) <= radius:
        return 0.0
    hits = [hit for hit in (ray_sphere_distance(origin3, direction, end, radius, max_distance) for end in (a3, b3))
        if hit is not None]
    direction3 = _vector3("direction", direction)
    magnitude = math.hypot(*direction3)
    d = [v / magnitude for v in direction3]
    axis = [q - p for p, q in zip(a3, b3)]
    m = [o - p for o, p in zip(origin3, a3)]
    length2, nd, md = (sum(x * y for x, y in zip(u, v)) for u, v in ((axis, axis), (d, axis), (m, axis)))
    side = length2 - nd * nd  # zero when the ray runs along the axis: the end spheres decide
    if side > 1e-12:
        half = length2 * sum(x * y for x, y in zip(m, d)) - md * nd
        constant = length2 * (sum(x * x for x in m) - radius * radius) - md * md
        discriminant = half * half - side * constant
        if discriminant >= 0:
            t = (-half - math.sqrt(discriminant)) / side
            if 0 <= t <= max_distance and 0 <= md + t * nd <= length2:
                hits.append(t)
    return min(hits, default=None)
