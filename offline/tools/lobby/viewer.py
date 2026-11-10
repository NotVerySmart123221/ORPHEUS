"""Third-person animation viewer: a harmless hero bot ("mannequin") in front of the Practice Range spawn.

Opt-in: the environment variable OW_VIEWER_FILE names a text file holding one hero name (as in target.PLAYER_KIT,
case ignored, e.g. "Reaper" or "soldier: 76"). The match reads it every CHECK_SECONDS and swaps the mannequin when
the name changes. The mannequin is an enemy hero bot that turns to the nearest enemy, fires its primary in bursts
and reloads, with zero damage (the fire shows only for target.WEAPON_GRAPHS heroes); shoot it to see it die and come
back 3 s later. research/dev-console.md.
"""
import dataclasses
import math
import os
from pathlib import Path

import hitbox
from ow174.game.content import heroes
from target import HERO_SPEED, PLAYER_KIT, TrainingTarget

DISTANCE = 5.0  # metres in front of the spawn
CHECK_SECONDS = 1.0
FIRST = 0x40  # bot numbers from here (entity 0xB0000001 + n << 8); the Range roster uses 0..14


def wanted():
    """The hero name in OW_VIEWER_FILE, '' when the file is empty or unreadable, None when the viewer is off."""
    path = os.environ.get('OW_VIEWER_FILE')
    if not path:
        return None
    try:
        return Path(path).read_text(encoding='utf-8').strip()
    except OSError:
        return ''


def mannequin(name, spawn, serial=0, team=1):
    """The mannequin for hero `name` DISTANCE metres in front of spawn ((x, y, z), yaw degrees), facing it; None for
    a name with no PLAYER_KIT entry. serial picks its bot number, so a swapped-in hero gets a fresh entity."""
    hero = next((h for h in heroes().values() if h.name.lower() == name.lower() and h.name in PLAYER_KIT), None)
    if hero is None:
        return None
    health, weapon = PLAYER_KIT[hero.name]
    (x, y, z), yaw = spawn
    a = math.radians(yaw)
    position = (x + DISTANCE * math.sin(a), y, z + DISTANCE * math.cos(a))
    # shortcut: no abilities or heals (they would hurt or heal the player); upgrade when the viewer needs them.
    # shortcut: heroes without decoded hit volumes take Soldier's; upgrade when hitbox.HEROES covers them.
    return TrainingTarget(position, yaw + 180, hero.body, team, 'shoot', (), 0xB0000001 + ((FIRST + serial % 0x40) << 8),
        hero=hero.guid, volumes=hitbox.HEROES.get(hero.body, hitbox.SOLDIER), speed=HERO_SPEED,
        weapon=dataclasses.replace(weapon, damage=0, far_damage=0), max_health=health)
