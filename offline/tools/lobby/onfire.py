"""On Fire: the score a player's play fills and time drains, and when he is on fire.

What the client shows of it is component 64 of the player's entity (LizRollin's onfire.py @ 9f8f3bc, MIT; board.py
writes the component): field 19 the score and field 20 the threshold, for his own fire meter, and field 36, set while
he is on fire, which makes his portrait burn on every client. The meter itself is hidden in this client outside the
Tutorial (their reading of its FireBarVisible), so the burning portrait is what shows.

The rule that fills the score is the server's: it is not in the client nor in its data (003F.054's table is keyed
settings, none named). This is the community's reading of retail (PCGamesN, 20 June 2016, after a player's guide):
- on fire from 250 points; 5 points a second drain all the time;
- an elimination is worth the percent of the target's health the player took (100 for all of it), which is the
  number the client prints beside "ELIMINATED" (25005 +0x88).
- healing is worth 1 point per 5 health (Match._heal).
shortcut: damage blocked (1 per 25), the objectives and the hero-specific sources of that guide are not counted;
add each where the match starts to credit it to a player.
"""
THRESHOLD = 250.0
HEALTH_A_POINT = 5.0  # healing
DRAIN = 5.0  # points a second


class Fire:
    """One player's fire score in a match."""

    def __init__(self) -> None:
        self.score, self.at = 0.0, None

    def tick(self, now: float) -> None:
        """Drain for the time since the last tick."""
        if self.at is not None:
            self.score = max(0.0, self.score - DRAIN * (now - self.at))
        self.at = now

    def add(self, points: float) -> None:
        self.score += points

    @property
    def burning(self) -> bool:
        return self.score >= THRESHOLD
