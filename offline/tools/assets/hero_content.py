"""Write each hero's skin themes and unlocks as JSON, read from a retail install.

  python hero_content.py OUT.json --game-install ROOT HERO_GUID...

Per hero: "skin_themes" (STUHero m_skinThemes, type 0xA6 GUIDs), which the lobby's login record 20505 lists for
the client to load, and "unlocks" (type 0xA5 GUIDs in the hero's m_heroProgression: skins, emotes, poses, sprays,
voice lines, ...), which the hero catalog 24900 marks as owned (research/hero-menu.md), and "defaults" (the
progression's default items by loadout slot), which 24900 equips.
"""
import argparse
import json
import struct

from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, load_storage, stu_header_crc64
from stu_graph import instances

SKIN_THEMES = 0x84625AA3  # STUHero m_skinThemes: inline array of entries
THEME = 0x5E9665E3  # the entry's skin-theme reference
PROGRESSION = 0x485AA39C  # STUHero m_heroProgression
MARKER = bytes([0xFF]) * 8
UNLOCK = 0xDB803F2F  # m_unlocks: the progression's unlock refs, each an 8-byte -1 then the mangled GUID
DEFAULTS = 0x7846C401  # STUProgressionUnlocks field: inline entries whose m_unlocks are the hero's default items
SLOTS = {  # unlock class -> 24900 loadout slot name (upstream's names); one item for the first four, lists after
    0xE05E4446: 'Skin', 0x8E77E8A1: 'HighlightIntro', 0xC6A72877: 'VictoryPose', 0xC23F89EB: 'WeaponSkin',
    0x84515D93: 'Spray', 0x8A9D1F89: 'VoiceLine', 0x0B517D2E: 'Emote'}
LISTS = ('Spray', 'VoiceLine', 'Emote')


def entries(blob, raw):
    """An inline instance array: u32 count, then per element an i32 field bag and that bag's fields."""
    bag_table = struct.unpack_from('<9I', blob)[5]
    count, pos, out = struct.unpack_from('<I', raw)[0], 4, []
    for _ in range(count):
        bag = struct.unpack_from('<i', raw, pos)[0]
        pos += 4
        if bag < 0:  # a null element (Orisa's defaults end with one)
            continue
        field_count, fields = struct.unpack_from('<II', blob, bag_table + 8 * bag)
        entry = {}
        for i in range(field_count):
            key, size = struct.unpack_from('<II', blob, fields + 8 * i)
            if size == 0:
                size = struct.unpack_from('<I', raw, pos)[0]
                pos += 4
            entry[key] = raw[pos:pos + size]
            pos += size
        out.append(entry)
    return out


def skin_themes(storage, hero):
    blob = find_asset_payload(storage, hero).payload
    crc = stu_header_crc64(blob)
    themes = (_decode_asset_ref(int.from_bytes(e[THEME], 'little'), THEME, crc)
              for e in entries(blob, instances(blob)[0][3].get(SKIN_THEMES, b'\0\0\0\0')) if THEME in e)
    # shortcut: keeps themes this install cannot read (Genji's 0x0A500000000049E1 is in the manifest but in no
    # readable bundle); filter them if the client complains at login.
    return [theme for theme in themes if theme >> 52 == 0x0A5]


def progression_of(storage, hero):
    """The hero's STUProgressionUnlocks payload, or None."""
    blob = find_asset_payload(storage, hero).payload
    field = instances(blob)[0][3].get(PROGRESSION)
    if field is None:
        return None
    return find_asset_payload(storage, _decode_asset_ref(int.from_bytes(field, 'little'), PROGRESSION,
                                                         stu_header_crc64(blob))).payload


def defaults(storage, hero):
    """The hero's default items by loadout slot: {"Skin": guid, ..., "VoiceLine": [guid, ...]}. The first item of
    each single slot wins: the defaults list "Heroic" before the "RANDOM" option."""
    progression = progression_of(storage, hero)
    if progression is None or DEFAULTS not in instances(progression)[0][3]:
        return {}
    crc, data = stu_header_crc64(progression), struct.unpack_from('<9I', progression)[7]
    out = {}
    for entry in entries(progression, instances(progression)[0][3][DEFAULTS]):
        # the field points at a dynamic array header {u64 count, u64 elements offset}, each element 16 bytes
        header = data + struct.unpack_from('<I', entry[UNLOCK], 4)[0]
        count, first = struct.unpack_from('<QQ', progression, header)
        for i in range(count):
            at = data + first + 16 * i
            guid = _decode_asset_ref(int.from_bytes(progression[at + 8:at + 16], 'little'), UNLOCK, crc)
            slot = SLOTS.get(instances(find_asset_payload(storage, guid).payload)[0][0])
            if slot in LISTS:
                out.setdefault(slot, []).append(f'{guid:016X}')
            elif slot:
                out.setdefault(slot, f'{guid:016X}')
    return out


def unlocks(storage, hero):
    progression = progression_of(storage, hero)
    if progression is None:
        return []
    crc, found = stu_header_crc64(progression), []
    # shortcut: a byte scan for the -1 marker, not a parse of the nine unlock arrays; the 0x025 prefix filters it.
    for at in range(36, len(progression) - 15):
        if progression[at:at + 8] == MARKER:
            guid = _decode_asset_ref(int.from_bytes(progression[at + 8:at + 16], 'little'), UNLOCK, crc)
            if guid >> 52 == 0x025 and guid not in found:
                found.append(guid)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('out')
    parser.add_argument('--game-install', required=True)
    parser.add_argument('heroes', nargs='+', type=lambda text: int(text, 16))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    table = {f'{hero:016X}': {'skin_themes': [f'{theme:016X}' for theme in skin_themes(storage, hero)],
                              'unlocks': [f'{unlock:016X}' for unlock in unlocks(storage, hero)],
                              'defaults': defaults(storage, hero)}
             for hero in args.heroes}
    with open(args.out, 'w', encoding='utf-8', newline='\n') as out:
        json.dump(table, out, indent=0, sort_keys=True)


if __name__ == '__main__':
    main()
