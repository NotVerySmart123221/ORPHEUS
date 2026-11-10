"""Each hero body's first-person eye heights from the retail install, for tools/lobby/target.EYE_HEIGHTS.

    python eye_heights.py [--game-install DIR]

Prints `body GUID: (standing, crouched),  # name` per hero body the lobby spawns (content.SPAWN_BODY, the hero's
body, D.Va's pilot). The heights are the body's STUFirstPersonComponent (class 0x9D28963F) fields m_E29993A5
(standing) and m_816B830F (crouched): metres from the feet to the first-person camera. Hitscan shots are taken to
leave from the camera, an assumption the live hitbox overlay checks. A field the body leaves out takes OWLib's
default (1.55, 1.15).
"""
import os
import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby' / 'vendor'))

from ow174.game.content import PILOT_BODY, SPAWN_BODY, heroes  # noqa: E402
from retail_bundle_lookup import find_asset_payload, load_storage  # noqa: E402
from stu_graph import instances  # noqa: E402

FIRST_PERSON, STANDING, CROUCHED = 0x9D28963F, 0xE29993A5, 0x816B830F
DEFAULTS = {STANDING: 1.55, CROUCHED: 1.15}


def eye_heights(blob):
    """(standing, crouched) of a body's STU blob, or None when it has no first-person component."""
    for cls, _, _, fields in instances(blob):
        if cls == FIRST_PERSON:
            return tuple(round(struct.unpack('<f', fields[key])[0], 4) if key in fields else default
                         for key, default in DEFAULTS.items())
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    bodies = {SPAWN_BODY.get(hero.guid, hero.body): hero.name for hero in heroes().values()}
    bodies.update({body: f'{heroes()[hero].name} pilot' for hero, body in PILOT_BODY.items()})
    for body, name in sorted(bodies.items(), key=lambda item: item[1]):
        print(f'    0x{body:016X}: {eye_heights(find_asset_payload(storage, body).payload)},  # {name}')


if __name__ == '__main__':
    main()
