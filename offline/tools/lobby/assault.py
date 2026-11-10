"""Assault rules: the attackers (team 0) take point A, then point B, before time runs out. A defender on the
point stops the capture."""
import math

CAPTURE_RADIUS = 6.0  # shortcut: one horizontal radius for every point; upgrade to the map's capture volumes.
CAPTURE_HEIGHT = 4.0  # shortcut: as above, the height above or below the point position that still counts.
CAPTURE_SECONDS = 10.0  # shortcut: one attacker's pace; upgrade with the per-attacker rates from the wiki.
ROUND_SECONDS = 240.0  # shortcut: not from the mode data or the wiki; check against an Assault wiki revision.
BONUS_SECONDS = 240.0  # shortcut: time added for point B, as above.


def on_point(position, point):
    return (math.hypot(position[0] - point[0], position[2] - point[2]) <= CAPTURE_RADIUS
            and abs(position[1] - point[1]) <= CAPTURE_HEIGHT)


class Assault:
    def __init__(self, points, starts):
        self.points = list(points)
        self.index = 0  # the point under attack: 0 is A
        self.progress = 0.0  # 0..1 of the current point
        self.ends = starts + ROUND_SECONDS
        self.winner = None  # 0 when the attackers take the last point, 1 when time runs out
        self.finished = None
        self.ticked = None

    def overtime(self, now):
        return self.winner is None and now >= self.ends

    def tick(self, now, attackers, defenders):
        """Advance to now with `attackers` and `defenders` on the current point. Returns 'captured' (the
        next point opens), 'won' or 'lost' when that happens, else None."""
        dt, self.ticked = 0.0 if self.ticked is None else now - self.ticked, now
        if self.winner is not None:
            return None
        # shortcut: no progress decay and no capture tiers; overtime lasts while an attacker is on the point.
        if attackers and not defenders:
            self.progress += dt / CAPTURE_SECONDS
        if self.progress >= 1:
            self.index, self.progress = self.index + 1, 0.0
            if self.index < len(self.points):
                self.ends = max(self.ends, now) + BONUS_SECONDS
                return 'captured'
            self.winner, self.finished = 0, now
            return 'won'
        if now >= self.ends and not attackers:
            self.winner, self.finished = 1, now
            return 'lost'
        return None
