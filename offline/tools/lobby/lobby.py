"""Loopback-only menu prototype for Overwatch 1.74.0.0.104319."""
import argparse
import hmac
import json
import logging
import secrets
import select
import sqlite3
import socket
import struct
from pathlib import Path

from panama import Jam

HELLO_CLIENT = b'HELLO PRO CLIENT\0'
HELLO_SERVER = b'HELLO PRO SERVER\0'
OUT_CONNECT = 0x3C7E3468
IN_CONNECT = 0xF5FF548A
MAX_FRAME = 1024 * 1024
BLOB_KEY_FILE = Path(__file__).resolve().parent / 'vendor' / 'data' / 'lobby_state_blob.key'
LOG = logging.getLogger('lobby')


def blob_key():
    """The build's 64-byte state-blob signing key: a local input, kept out of the source (vendor/data/README.md)."""
    key = BLOB_KEY_FILE.read_bytes()
    if len(key) != 64:
        raise ValueError('lobby_state_blob.key must hold 64 bytes')
    return key


def recvn(connection, size):
    data = bytearray()
    while len(data) < size:
        chunk = connection.recv(size - len(data))
        if not chunk:
            raise ConnectionError('client closed')
        data.extend(chunk)
    return bytes(data)


def frame(payload):
    if not 2 <= len(payload) <= MAX_FRAME:
        raise ValueError('invalid frame length')
    return len(payload).to_bytes(3, 'big') + payload


def state_blob(sequence, port):
    # shortcut: unsigned block relies on tournament mode; retail login needs its own handshake.
    block = bytearray(range(256))
    block[:7] = b'\2\x7f\0\0\1' + struct.pack('<H', port)
    block[176:208] = hmac.digest(blob_key(), block[:176] + block[208:], 'sha256')
    prefix = struct.pack('<I', sequence)
    peer = prefix + bytes.fromhex('00050001efbe000000000000')
    channel = prefix + bytes.fromhex('000d00010000') + struct.pack('<H', port) + socket.inet_aton('127.0.0.1')
    return bytes(block) + peer + channel + struct.pack('<I', 1)


def protocols(payload):
    if len(payload) < 6 or payload[:2] != b'\0\0':
        raise ValueError('expected Base protocol announcement')
    count, = struct.unpack_from('<I', payload, 2)
    if not 1 <= count <= 255 or len(payload) != 6 + 4 * count:
        raise ValueError('invalid protocol announcement length')
    groups = struct.unpack_from(f'<{count}I', payload, 6)
    if len(set(groups)) != count or not {OUT_CONNECT, IN_CONNECT}.issubset(groups):
        raise ValueError('duplicate or missing lobby protocol groups')
    return {crc: index + 1 for index, crc in enumerate(groups)}


LOGIN_HEROES = (0x02E000000000006E, 0x02E000000000016B, 0x02E000000000016C, 0x02E000000000016D, 0x02E000000000016E)
HERO_CONTENT = Path(__file__).resolve().parent / 'data' / 'hero_content.json'  # written by run.py (tools/assets)
CONTENT_KEYS = Path(__file__).resolve().parent / 'vendor' / 'data' / 'resource_keys_174.json'


def hero_content(path=HERO_CONTENT):
    """{hero GUID hex: {"skin_themes": [...], "unlocks": [...]}} from the extracted table; empty before it exists."""
    return json.loads(path.read_text(encoding='utf-8')) if path.exists() else {}


def skin_themes(heroes, table):
    """The heroes' skin themes (type 0xA6 GUIDs)."""
    return [int(theme, 16) for hero in heroes for theme in table.get(f'{hero:016X}', {}).get('skin_themes', ())]


def content_keys(path=CONTENT_KEYS):
    """[(key name, 16 key bytes)] of every named content key, from upstream's vendored table."""
    table = json.loads(path.read_text(encoding='utf-8'))
    return [(int(entry['name'], 16), bytes.fromhex(entry['key'])) for entry in table.values() if entry['name']]


def player_record(name, card=None):
    card = card or {}
    account = struct.pack('<QQ', 1, 0x0100000000000000)
    return (account * 2 + struct.pack('<3Q2I', card.get('+0x20', 0), card.get('+0x28', 0), 0,
                                    card.get('+0x38', 1), 1) + name.encode('ascii') + b'\0')


def login_records(index, heroes=LOGIN_HEROES, maps=(0x688,), skins=(), keys=(), name='Player', card=None):
    account = struct.pack('<QQ', 1, 0x0100000000000000)
    player = player_record(name, card)
    name = name.encode('ascii') + b'\0'
    hello = account * 2 + struct.pack('<QQ', 2, 1) + name * 2 + bytes(2)
    hello += bytes(1) + bytes(8) + bytes(1)  # each consecutive bool run is bit-packed
    # 20504: content key names, then all their key bytes in the same order (decrypts event content).
    keyring = struct.pack(f'<I{len(keys)}QI', len(keys), *(n for n, _ in keys), 16 * len(keys))
    keyring += b''.join(key for _, key in keys)
    # 20505 advertises available heroes, skin themes, map headers and the core content set.
    content = struct.pack('<I', len(heroes)) + b''.join(struct.pack('<Q', hero) for hero in heroes)
    content += struct.pack(f'<I{len(skins)}QI{len(maps)}QIQ', len(skins), *skins, len(maps),
                           *(0x0790000000000000 | index for index in maps), 1, 0x0308000000000001)
    return [bytes((index, offset)) + body for offset, body in
            ((0, hello), (2, player), (4, keyring), (5, content))]


def handle(connection, port, practice=None):
    connection.settimeout(30)
    if recvn(connection, 17) != HELLO_CLIENT:
        raise ValueError('invalid client greeting')
    connection.sendall(HELLO_SERVER)
    initial = recvn(connection, 40)
    cn = initial[8:]
    sn, challenge = secrets.token_bytes(32), secrets.token_bytes(32)
    connection.sendall(challenge + b'\0' + sn)
    proof = recvn(connection, 40)
    if struct.unpack_from('<Q', proof)[0] != 1:
        raise ValueError('invalid difficulty-zero proof of work')
    if not hmac.compare_digest(proof[8:], hmac.digest(bytes(32), cn + sn, 'sha256')):
        raise ValueError('client proof mismatch; tournament route required')
    tx = Jam(hmac.digest(bytes(32), cn + sn, 'sha256'))
    rx = Jam(hmac.digest(bytes(32), sn + cn, 'sha256'))
    blob = state_blob(secrets.randbits(32), port)
    connection.sendall(hmac.digest(bytes(32), sn + cn, 'sha256') + tx.crypt(blob))
    LOG.info('Handshake proof verified; sent 292-byte type-13 channel block')
    groups = None
    wire_groups = {0: 0x11D82194}
    logged_in = False
    while True:
        if logged_in and practice is not None and practice.account:
            rewards = [r for r in practice.pending_rewards(groups) if r is not None]
            if rewards:
                connection.sendall(tx.crypt(b''.join(frame(r) for r in rewards)))
            if not select.select([connection], [], [], 0.1)[0]:
                continue
        size = int.from_bytes(rx.crypt(recvn(connection, 3)), 'big')
        if not 2 <= size <= MAX_FRAME:
            raise ValueError(f'invalid incoming frame size {size}')
        payload = rx.crypt(recvn(connection, size))
        group, offset = payload[:2]
        LOG.info('RX group=%d crc=%08x offset=%d bytes=%d', group, wire_groups.get(group, 0), offset, size)
        if groups is None:
            groups = protocols(payload)
            wire_groups.update({index: crc for crc, index in groups.items()})
            for index, crc in sorted(wire_groups.items()):
                LOG.info('Protocol index=%d crc=%08x', index, crc)
            connection.sendall(tx.crypt(frame(b'\0\2')))
            LOG.info('Accepted %d groups; OUT=%d IN=%d; sent Base ack',
                     len(groups), groups[OUT_CONNECT], groups[IN_CONNECT])
        elif payload == b'\0\3':
            connection.sendall(tx.crypt(frame(b'\0\3')))
            LOG.info('Keepalive echoed')
        elif group == groups[OUT_CONNECT] and offset == 0 and not logged_in:
            # The client's loader only waits for a map's header asset; 20505's list is what preloads it, so every
            # map the lobby may hand off to must be here (research/play-menu.md, Uprising).
            maps = {0x688} | ({practice.game_map.map_guid & 0xFFFFFFFF} | practice.handoff_maps() if practice else set())
            heroes = LOGIN_HEROES + tuple(h for h in (practice.menu_heroes() + practice.handoff_heroes() if practice else ())
                                          if h not in LOGIN_HEROES)
            table = hero_content()
            records = login_records(groups[IN_CONNECT], heroes, sorted(maps), skin_themes(heroes, table), content_keys(),
                                    name=practice.name if practice else 'Player',
                                    card=practice.player_card() if practice and practice.account else None)
            hero = secrets.choice(practice.menu_heroes()) if practice else 0  # upstream rerolls it per login
            extra = [practice.events(groups), practice.hero_catalog(groups, table), practice.progression(groups),
                     practice.party(groups, hero), practice.menu_hero(groups, hero), practice.leaver_status(groups),
                     practice.settings(groups), practice.career_stats(groups),
                     practice.arcade(groups)] if practice else []
            records += [record for item in extra for record in (item if isinstance(item, list) else [item]) if record]
            connection.sendall(tx.crypt(b''.join(frame(record) for record in records)))
            logged_in = True
            LOG.info('Sent login/player/content records 0x5014, 0x5016, 0x5018, 0x5019, plus %d account records',
                     len(records) - 4)
        elif logged_in and practice is not None:
            response = practice.request(payload, groups)
            if response is not None:  # one frame, or several (a queue step answers with more than one message)
                frames = response if isinstance(response, list) else [response]
                connection.sendall(tx.crypt(b''.join(frame(r) for r in frames if r is not None)))


def serve(port=3724, map_name=None, mode=None, difficulty='medium', retail_menu=False, event='none',
          game_install=None, name='Player', progression=False):
    from practice import PRACTICE_RANGE, PracticeRange, pick_map, profile_name
    name = profile_name(name)
    if progression:
        from practice import PROGRESSION_CATALOG
        from progression import Account
        Account(PROGRESSION_CATALOG.parent / 'profiles' / f'{name}.sqlite3',
                json.loads(PROGRESSION_CATALOG.read_text(encoding='utf-8')))
    game_map = pick_map(map_name, mode, event) if map_name else PRACTICE_RANGE
    with socket.socket() as listener:
        if hasattr(socket, 'SO_EXCLUSIVEADDRUSE'):
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        listener.bind(('127.0.0.1', port))
        listener.listen(1)
        LOG.info('Listening on 127.0.0.1:%d', port)
        while True:
            connection, _ = listener.accept()
            with connection:
                try:
                    LOG.info('Client connected')
                    with PracticeRange(game_map=game_map, difficulty=difficulty, retail_menu=retail_menu,
                                       event=event, game_install=game_install, name=name, progression=progression) as practice:
                        handle(connection, port, practice)
                except (OSError, ConnectionError, ValueError, sqlite3.Error) as exc:
                    LOG.info('Connection ended: %s', exc)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=3724)
    parser.add_argument('--log', help='optional log file; parent directory must exist')
    parser.add_argument('--map', help='open this catalog map (name or GUID) instead of the Practice Range')
    parser.add_argument('--mode', default='0x0230000000000010', help='game mode GUID for --map (default Skirmish)')
    parser.add_argument('--difficulty', choices=('easy', 'medium', 'hard'), default='medium', help='vs-AI bots (default medium)')
    parser.add_argument('--retail-menu', action='store_true', help='the client runs tools/asi/ow_fixes.asi')
    from practice import MENU_EVENTS, profile_name
    parser.add_argument('--event', choices=sorted(MENU_EVENTS), default='none', help='main menu event (default none: ordinary backdrops)')
    parser.add_argument('--game-install', help='the retail install, read for equipped items (gallery equips are kept)')
    parser.add_argument('--name', type=profile_name, default='Player', help='profile name: 1..12 ASCII letters or digits (default Player)')
    parser.add_argument('--progression', action='store_true', help='use an earned local account instead of all-unlocked research mode')
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error('port must be 1..65535')
    handlers = [logging.StreamHandler()]
    if args.log:
        handlers.append(logging.FileHandler(args.log, encoding='utf-8'))
    logging.basicConfig(level=logging.INFO, format='%(asctime)s %(message)s', handlers=handlers)
    try:
        serve(args.port, args.map, args.mode, args.difficulty, args.retail_menu, args.event, args.game_install,
              name=args.name, progression=args.progression)
    except KeyboardInterrupt:
        pass
