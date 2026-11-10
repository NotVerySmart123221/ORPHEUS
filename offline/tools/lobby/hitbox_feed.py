"""Opt-in hit-test feed for tools/research/hitbox_view.py (research/hitbox-drift.md).

OW_HITBOX_VIEW=PORT makes every match update send one JSON datagram to 127.0.0.1:PORT with the geometry the
server's hitscan test uses: the first player's eye and aim as the server moves them, and the posed volumes of
every bot that player can damage, at the view time of the player's last command (Match.view_time, the poses
fire_at_target tests). Drawn from that eye, the volumes are exactly the crosshair positions that hit.
"""
import json
import math
import os
import socket

import hitbox

_port = os.environ.get('OW_HITBOX_VIEW')
_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM) if _port else None


def state(match):
    """The feed's message for the first player with a body, or None."""
    player = next((p for p in match.players if p.client is not None and p.has_body), None)
    if player is None:
        return None
    seen = None if player.last_command is None else match.view_time(player, player.last_command)
    bots = []
    for bot in match._enemy_bots(player):
        if bot.health:
            position, yaw = (bot.position, bot.yaw) if seen is None else bot.pose_at(seen)
            bots.append([[*a, *b, radius, critical] for a, b, radius, critical in
                         hitbox.posed(bot.volumes, position, math.radians(yaw))])
    return {'eye': list(player.eye()), 'yaw': player.mover.yaw, 'pitch': player.mover.pitch,
            'frame': player.commands.last_frame, 'bots': bots}


def publish(match):
    if _sock is None:
        return
    message = state(match)
    if message is not None:
        try:
            _sock.sendto(json.dumps(message).encode(), ('127.0.0.1', int(_port)))
        except OSError:
            pass  # nobody listening, or the datagram was too big: the feed is best-effort
