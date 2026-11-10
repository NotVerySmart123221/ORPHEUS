"""Opt-in resync of the server's copy of the player onto the client's own position (research/hitbox-drift.md).

The client never sends its position, so mover.py dead-reckons it from the commands and small mismatches add up;
hitscan shots start at that copy's eye (Player.eye, target.EYE_HEIGHTS). With OW_CLIENT_SYNC=PORT the lobby listens on 127.0.0.1:PORT for
(command frame, feet) datagrams that ow_fixes.asi (tools/asi/client_feed.c) reads from the client's mover inside the
client, which inherits OW_CLIENT_SYNC from run.py. After each command step the copy moves by the client's offset at
the newest frame both sides know, so it sits where the client's body was at that frame. Without datagrams nothing
changes.

A new body (respawn, hero switch) is a new mover here, while the client may still simulate the old body for a frame
or two: samples from the slot that fed the previous body are ignored unless they agree within ACCEPT.

The same feed carries Shadow Step's marker while its aim is open (client_feed.c reads the targeting state's own
place): `marker` gives the newest one, so the server can put Reaper where the client drew it (Match.step_point).

shortcut: one feed for the first player that steps (offline play has one client); upgrade with a player id in the
datagram if a second local client ever plays.
"""
import logging
import math
import os
import socket
import struct

MESSAGE = struct.Struct('<I3fQ')  # command frame, feet x y z, the client's slot (which body): client_feed.c Sample
MARKER = struct.Struct('<I3fB')  # command frame, Shadow Step's marker x y z, whether the place is good: its Marker
MARKER_FRAMES = 30  # a marker older than this many command frames (0.2 s) is not the open aim's any more
KEEP = 256  # frames kept on each side (1.8 s at 7 ms); the client is ahead by a few
LOUD = 0.25  # metres: corrections this big are logged
ACCEPT = 1.0  # metres: the previous body's slot is only trusted this close (a new body starts where both put it)

log = logging.getLogger('ow174.game')


class Resync:
    """The server's and the client's feet by command frame, and the offset between them. `stale` is the client slot
    that fed the previous body."""

    def __init__(self, stale: int | None = None) -> None:
        self.server: dict[int, tuple] = {}  # frame -> the server's feet after stepping that frame
        self.client: dict[int, tuple] = {}  # frame -> (the client's feet after that frame, its slot)
        self.stale, self.slot = stale, None  # slot: the one the last used sample came from
        self.marked = None  # (frame, place, good): Shadow Step's marker as the client last drew it

    def stepped(self, frame: int, position) -> None:
        self.server[frame] = tuple(position)
        if len(self.server) > KEEP:
            del self.server[min(self.server)]

    def observed(self, frame: int, feet, slot: int = 0) -> None:
        self.client[frame] = tuple(feet), slot
        if len(self.client) > KEEP:
            del self.client[min(self.client)]

    def aimed(self, frame: int, place, good: bool) -> None:
        self.marked = (frame, tuple(place), good)

    def correct(self, position: list) -> list | None:
        """Move `position` (the server's feet now) by client minus server at the newest frame both know, and return
        that offset; None when no frame matches yet or the sample is the previous body's. Frames up to it are used
        up; the server's later ones move along."""
        common = max(self.server.keys() & self.client.keys(), default=None)
        if common is None or OBSERVE:
            return None
        feet, slot = self.client[common]
        delta = [c - s for c, s in zip(feet, self.server[common])]
        if slot == self.stale and math.hypot(*delta) > ACCEPT:
            self.server = {f: p for f, p in self.server.items() if f > common}
            self.client = {f: p for f, p in self.client.items() if f > common}
            return None
        self.slot = slot
        for i, d in enumerate(delta):
            position[i] += d
        self.server = {f: tuple(v + d for v, d in zip(feet_then, delta))
                       for f, feet_then in self.server.items() if f > common}
        self.client = {f: p for f, p in self.client.items() if f > common}
        return delta


def receive(sock, resync: Resync) -> None:
    """Every pending datagram into `resync`; anything but a frame and three finite coordinates is dropped."""
    while True:
        try:
            data = sock.recv(64)
        except (BlockingIOError, ConnectionResetError):
            return
        if len(data) == MESSAGE.size:
            frame, *feet, slot = MESSAGE.unpack(data)
            if all(math.isfinite(v) for v in feet):
                resync.observed(frame, feet, slot)
        elif len(data) == MARKER.size:
            frame, *place, good = MARKER.unpack(data)
            if all(math.isfinite(v) for v in place):
                resync.aimed(frame, place, bool(good))


def _listen(port):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(('127.0.0.1', int(port)))
    sock.setblocking(False)
    return sock


_port = os.environ.get('OW_CLIENT_SYNC')
OBSERVE = False  # tools/research/mover_record.py --observe: take the client's feet, never move the server's copy


def active() -> bool:
    """Whether the lobby moves the server's copy of the player to the client's feet."""
    return bool(_port) and not OBSERVE

def marker(frame: int):
    """(place, good) of Shadow Step's marker as the client drew it about command `frame`; None without a fresh one
    (no feed, no aim open). Observing or not: the feed is what the client shows, whoever moves the body."""
    marked = _resync.marked
    if marked is None or abs(frame - marked[0]) > MARKER_FRAMES:
        return None
    return marked[1], marked[2]


_sock = None  # bound on the first step: run.py imports this module too, and must leave the port to the lobby
_resync, _mover = Resync(), None


def stepped(mover, frame: int) -> None:
    """After mover.step for command `frame`: put the mover where the client is. A new mover is a new body, whose
    frames start over."""
    global _sock, _resync, _mover
    if not _port:
        return
    if _sock is None:
        _sock = _listen(_port)
    if mover is not _mover:
        _resync, _mover = Resync(_resync.slot or _resync.stale), mover
    _resync.stepped(frame, mover.position)
    receive(_sock, _resync)
    delta = _resync.correct(mover.position)
    if delta is not None and math.hypot(*delta) > LOUD:
        log.info('[game] client sync frame=%d moved %.3f %.3f %.3f to %.2f %.2f %.2f', frame, *delta, *mover.position)
