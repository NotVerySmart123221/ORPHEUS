"""List a hero's voice lines: the keys ClientInVoice 27500 sends (message +0x90), their stimulus and debug label.

  python voice_lines.py ENTITY_OR_VOICESET [--stimulus 0x2B] [--label HrSlct]

ENTITY: a body entity (type 0x040); its STUVoiceSetComponent names the voice set (type 0x07A). Voice sets live in
the speech manifest, not the root CMF. The client looks the key up in the set's m_voiceLineGuids (F7FA2CAC) and
plays the parallel m_voiceLineInstances (F51B70C4) entry (research/soldier-runtime/presentation-next-steps-2026-11-07.md).
"""
import os
import argparse
import struct

from hero_content import entries
from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, load_storage, stu_header_crc64
from stu_graph import instances

SPEECH_MANIFEST = "Manifest/Win_SPWin_RDEV_LenUS_speech_EExt.cmf"
VOICE_SET_COMPONENT, VOICE_DEFINITION = 0x8745618A, 0x8A4FF89C
LINE_INSTANCES, LINE_GUIDS = 0xF51B70C4, 0xF7FA2CAC
RUNTIME, STIMULUS, LABEL = 0xBC474019, 0x401F5484, 0x38F3ED5E


def voice_set_of(storage, entity):
    blob = find_asset_payload(storage, entity).payload
    for cls, _, _, fields in instances(blob):
        if cls == VOICE_SET_COMPONENT:
            raw = int.from_bytes(fields[VOICE_DEFINITION], 'little')
            return _decode_asset_ref(raw, VOICE_DEFINITION, stu_header_crc64(blob))
    raise LookupError(f'entity 0x{entity:016X} has no STUVoiceSetComponent')


def _label(blob, data, field):
    """A dynamic-data string: the field points at {u32 length, ..., u64 string offset} (both data-relative)."""
    header = data + struct.unpack_from('<I', field[-4:])[0]
    length = struct.unpack_from('<I', blob, header)[0]
    at = data + struct.unpack_from('<Q', blob, header + 8)[0]
    return blob[at:at + length].decode('utf-8', 'replace')


def voice_lines(storage, voice_set):
    """[(key, stimulus, label)] in m_voiceLineGuids order."""
    blob = find_asset_payload(storage, voice_set, SPEECH_MANIFEST).payload
    crc, data = stu_header_crc64(blob), struct.unpack_from('<9I', blob)[7]
    rows = instances(blob)
    root = rows[0][3]
    header = data + struct.unpack_from('<I', root[LINE_GUIDS][-4:])[0]
    count, first = struct.unpack_from('<QQ', blob, header)
    keys = struct.unpack_from(f'<{count}Q', blob, data + first)
    out = []
    for key, entry in zip(keys, entries(blob, root[LINE_INSTANCES])):
        line = rows[struct.unpack_from('<i', entry[RUNTIME])[0]][3]
        stimulus = _decode_asset_ref(int.from_bytes(line[STIMULUS], 'little'), STIMULUS, crc) if STIMULUS in line else None
        out.append((key, stimulus, _label(blob, data, line[LABEL]) if LABEL in line else ''))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('guid', type=lambda v: int(v, 0))
    parser.add_argument('--stimulus', type=lambda v: int(v, 0), help='low bits of the stimulus GUID, e.g. 0x2B')
    parser.add_argument('--label', help='substring of the debug label')
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    storage = load_storage(args.game_install)
    try:
        voice_set = voice_set_of(storage, args.guid) if args.guid >> 52 == 0x040 else args.guid
        lines = voice_lines(storage, voice_set)
    finally:
        for handle in storage._files.values():
            handle.close()
    print(f'voice set 0x{voice_set:016X}: {len(lines)} lines')
    for i, (key, stimulus, label) in enumerate(lines):
        if args.stimulus is not None and (stimulus or 0) & 0xFFFFFFFF != args.stimulus:
            continue
        if args.label and args.label not in label:
            continue
        print(f'{i:4} key=0x{key:016X} stimulus={stimulus and f"0x{stimulus:016X}"} {label}')


if __name__ == '__main__':
    main()
