"""One local Practice Range, using the pinned MIT game-server subset in vendor/."""
import json
import math
import os
import logging
from pathlib import Path
import struct
import sys
import time
import sqlite3
from datetime import datetime, timezone
from queue import Empty, SimpleQueue

sys.path.insert(0, str(Path(__file__).resolve().parent / 'vendor'))
sys.path.insert(1, str(Path(__file__).resolve().parents[1] / 'assets'))
from ow174.game.content import PRACTICE_RANGE, SOLDIER, GameMap, Spawn, _map_entries, game_map, map_catalog
from ow174.game.server import GameServer as BaseGameServer
from ow174.jam.codec import Schemas
from career import QUICK_PLAY, Career
from kits import KITS, initialize_practice_kit
from soldier import initialize_practice_soldier

REQUEST = 0xA6E53896
HANDOFF = 0x074DAD18
TOURNAMENT_MATCH = 0x692C511B  # what the PLAY screen's Tournament Match card sends (research/play-menu.md)
QUEUE = 0x1C6EC712  # client queue messages: 44100 enter {key}, 44102 cancel {key}, 44103 Ready {roles}, 44107 change role
QUEUE_STATE = 0xB4F8BC62  # 44201 queue joined {key, ...}: the client's own queue entry; 44202 queue left {key, reason}
QUEUE_WAITS = 0xA1498A6A  # 56200: the role cards' estimated waits
ROLES = (1, 2, 3)  # role numbers: damage, tank, support
PICKING, SEARCHING = 1, 3  # queue entry states (+0x54): choosing roles; searching
ARCADE = 0x00A8654E  # 39802 the Arcade's cards, 39810 their dates, 39804 how busy each is
ARCADE_DATA = Path(__file__).resolve().parent / 'vendor' / 'data' / 'arcade_174.json'  # upstream's card catalog
CARD = 0x0630000000000000  # a mode card's GUID (0x063 asset) is its queue key
# Arcade card groups (0x063 indexes, upstream's lists): each group fills one of the Arcade's seven slots. The PvE
# events first (the experiment: they show, their modes are scripted-loading ones the server cannot host yet), then
# the everyday cards.
JUNKENSTEIN = (0x3C, 0x3D, 0x4A, 0x3E, 0x43, 0x44, 0x49, 0x45, *range(0x16B, 0x171))  # Revenge, Endless, challenges
ARCHIVES = (0x29, 0x24, 0x2A, 0x2B, 0x2C, 0x28, 0x2D, 0x2E,  # Uprising
            0x58, 0x3A, 0x59, 0x5A, 0x66, 0x67, 0x68, 0x69,  # Retribution
            *range(0x8A, 0x92))  # Storm Rising
CHALLENGE_MISSIONS = (0x119, 0x112, 0x1A9, 0x116, 0x127, 0x1AA, 0x111, 0x11B, 0x1AB)
ARCADE_GROUPS = (JUNKENSTEIN, ARCHIVES, CHALLENGE_MISSIONS, (0x15C,), (0x34,), (0xEF,), (0x2,))  # + Kanezaka DM,
# Team Deathmatch, Quick Play Classic, Mystery Heroes
FLAG_12 = {0x1C, 0xEF, 0x2}  # card flag 12 (permanent) as the retail capture sets it
# PvE cards (PracticeRange.pve_handoff) hosted on their map in their scripted-loading mode, with a spawn borrowed from
# the normal map's catalog entry: card index -> (map GUID, mode GUID, spawn position, yaw). Uprising (Story) Normal on
# King's Row (Uprising) loads and plays (2026-11-05: hero select, spawn, bots) once 20505 preloads the map's header and
# the mode graph's marker state is on (vendor match.LOADING_GATES); the mission itself is not scripted.
PVE_CARDS = {0x29: (0x08000000000006FE, 0x023000000000000F, (-42.248, 0.202, 2.274), -96.0)}
HERO_CATALOG = 0x70E5A247  # login message 24900: per hero, the owned unlocks and the loadout (research/hero-menu.md)
GALLERY = 0xB68870B8  # client messages from the Hero Gallery: 24500 equip {hero, item, slot}, 24501 item seen
EQUIPS = Path(__file__).resolve().parent / 'data' / 'equips.json'  # gallery equips kept across logins
# The client saves its Options on the server: 22200-22203 each one block of 20802 (the value is the field it fills),
# 22204 the changed entries of the key/value list in SETTING_VALUES (LizRollin's handlers/settings.py).
SETTINGS_BLOCKS = {22200: '+0x78', 22201: '+0x108', 22202: '+0x10D', 22203: '+0x120'}
SETTING_VALUES = '+0x130'
# The PvE maps' omnic spawn markers (tools/assets/map_markers.py): map GUID -> [[x, y, z, yaw], ...]
PVE_MARKERS = Path(__file__).resolve().parent / 'data' / 'pve_markers.json'
PROGRESSION = 0x779A8581  # login message 24300: the account's level, currencies, icon, frame and owned items
PROGRESSION_OUT = 0x7F4F46CB  # 24201 opens a box; 24203 buys an unlock
PROGRESSION_CATALOG = Path(__file__).resolve().parent / 'data' / 'progression_catalog.json'
FIRST_BORDER = 0x0250000000000918  # upstream's portrait border for levels 1-10
# shortcut: no account-wide items, boxes or currencies (upstream copies them from a retail capture); hero-tagged
# icons and sprays are owned through 24900.
ACCOUNT_RECORD = {'+0x50': 2000, '+0x58': FIRST_BORDER, '+0x68': 1, '+0x78': True, '+0x79': True}  # level 1
ACCOUNT = {'+0x0': 1, '+0x8': 1 << 56}  # lobby.login_records' account id
SOCIAL_OUT = 0x75D32AE2  # client settings and lookups; 22206 asks for a career profile
LOBBY = 0x1A9879A4  # lobby state; 20807 is a full career profile
MODE_RULES = 0xB9DCA497  # 27202: the stats each hero has (the profile shows no others), and the player's
CONFIG = 0x1732EBF2  # message 36600: key/value settings the client re-reads live
EVENTS = 0x8B77C823  # message 38900: the active events (celebrations); one picks the main menu's scenes
# Main menu events (upstream's presets that have a menu scene): celebration, content key index, key name (from
# vendor/data/resource_keys_174.json), celebration type (picks event map variants and the event box). Rewards, boxes
# and map swaps are left out (shortcut: no loot boxes or event maps yet).
MENU_EVENTS = {
    'none': None,  # ordinary backdrop catalog, including its random-hero Showcase placement
    'farewell': (0x128, 0x1A3, 0x28B8EBB462B83EEA, 0),  # 1.74 farewell lobby
    'lunar': (0x105, 0x188, 0x30C4FCFD17F18F63, 0x237C),  # Lunar New Year 2022
    'halloween': (0xFB, 0x181, 0x5D21A47DB7D0132D, 0x212E),  # Halloween Terror 2021
    'winter': (0xFF, 0x185, 0x66EC03F9A64F7C76, 0x2168),  # Winter Wonderland 2021
    'anniversary': (0x118, 0x198, 0xC96A78D36B923E65, 0x2428),  # Anniversary Remix vol 3
    'reaper': (0x104, 0x189, 0xC658E67E69912244, 0),  # Reaper's Code of Violence
    'cassidy': (0xF7, 0x17F, 0x6A8CDCC7A0C500D8, 0),  # Cassidy's New Blood
    'malevento': (0xFA, 0, 0, 0),
    'owl': (0x125, 0, 0, 0),  # Overwatch League lobby
}
CELEBRATION, CONTENT_KEY, CELEBRATION_TYPE = 0x0430000000000000, 0x0F10000000000000, 0x0D80000000000000
PARTY = 0x2411DE56  # message 20700: the player's party, each member with a hero and skin for the menu
PARTY_ENTITY = 2 | 0x1D << 40 | 1 << 56  # upstream's party entity id for connection 2 (lobby.login_records' session)
SUPPORTED_BUILDS = 0x04227E56  # CONFIG key: the client builds the config is for, as "[104319]" text
MENU_HERO = 0x87053C32  # CONFIG key: the main menu's hero, as "0x02e..." text (lowercase, as upstream)
UNLOCK_ALL = 0x2704A3F4  # CONFIG bool: tournament preset bypasses cosmetic ownership unless this is cleared
VS_AI_QUEUES = {0x70: 'easy', 0x71: 'medium', 0x72: 'hard'}  # its u32 at byte 2
LOG = logging.getLogger('lobby')


LISTS = ('Spray', 'VoiceLine', 'Emote')  # loadout slots that hold several items
LOADOUT_SLOTS = {'Skin': '+0x38', 'HighlightIntro': '+0x40', 'VictoryPose': '+0x48', 'WeaponSkin': '+0x50',
                 'Spray': '+0x58', 'VoiceLine': '+0x70', 'Emote': '+0x88'}  # 24900 hero record slots (upstream)


def loadout(defaults):
    """24900 hero record slots from hero_content's "defaults" ({slot name: GUID text or [GUID text]})."""
    return {LOADOUT_SLOTS[slot]: [int(g, 16) for g in item] if isinstance(item, list) else int(item, 16)
            for slot, item in defaults.items()}


def equipped(entry, saved):
    """A hero's loadout: its defaults (hero_content "defaults"), then each saved gallery equip (item, slot index,
    slot name) in order; a list slot (sprays, voice lines, emotes) takes the item at the index."""
    out = {slot: list(item) if isinstance(item, list) else item for slot, item in entry.get('defaults', {}).items()}
    for item, index, slot in saved:
        if slot in LISTS:
            items = out.setdefault(slot, [])
            items.extend(['0000000000000000'] * max(0, index + 1 - len(items)))
            items[index] = f'{item:016X}'
        else:
            out[slot] = f'{item:016X}'
    return out


def item_slot(storage, item):
    """The loadout slot name of an unlock GUID (its asset's class, hero_content.SLOTS), or None."""
    from hero_content import SLOTS
    from retail_bundle_lookup import find_asset_payload
    from stu_graph import instances
    try:
        return SLOTS.get(instances(find_asset_payload(storage, item).payload)[0][0])
    except (LookupError, ValueError, OSError):
        return None


SKIN_THEME = 0x4C167404  # skin unlock (class E05E4446) field: the skin theme (0A5) it applies


def skin_theme(storage, unlock):
    """The skin theme GUID a skin unlock applies, read from the install; 0 when it names none or can't be read."""
    from retail_bundle_lookup import _decode_asset_ref, find_asset_payload, stu_header_crc64
    from stu_graph import instances
    try:
        blob = find_asset_payload(storage, unlock).payload
        field = instances(blob)[0][3].get(SKIN_THEME)
    except (LookupError, ValueError, OSError):
        return 0
    return _decode_asset_ref(int.from_bytes(field, 'little'), SKIN_THEME, stu_header_crc64(blob)) if field else 0


def defaults_of(entry):
    """Every default item GUID text of a hero_content entry."""
    return {g for item in entry.get('defaults', {}).values() for g in (item if isinstance(item, list) else [item])}


def store_entry(unlock, defaults):
    """A 24900 store entry: retail's default form (unlock level 0, default flag) or upstream's not-for-sale form."""
    if unlock in defaults:
        return {'+0x0': int(unlock, 16), '+0x8': 0, '+0x10': 0, '+0x14': 0, '+0x16': -1, '+0x17': True}
    return {'+0x0': int(unlock, 16), '+0x8': 0, '+0x10': 65535, '+0x14': 0, '+0x16': -1, '+0x17': False}


def stu_datetime(seconds):
    """A Unix time in the game's packed date: years since 2000 at bit 36, then month, day, hour, minute, second."""
    t = time.gmtime(seconds)
    return ((t.tm_year - 2000) << 36 | t.tm_mon << 32 | t.tm_mday << 27 | t.tm_hour << 22 | t.tm_min << 16
            | t.tm_sec << 10)


def role_record(role):
    """A role entry of a queue (20700 entry, 56200 waits): no wait shown (+0x18/+0x20 = 0 s), no priority pass."""
    return {'+0x0': role, '+0x8': 0, '+0x10': 0, '+0x18': 0, '+0x20': 0, '+0x28': 0, '+0x30': 0, '+0x38': False,
            '+0x39': False}


EVENT_MAPS = {'winter': 'Winter', 'halloween': 'Halloween', 'lunar': 'Lunar'}  # the catalog's "<map> (<this>)"


def pick_map(name, mode, event=None):
    """The catalog map named `name` (or its 0x0800... GUID) played in game mode `mode` (a 0C5 GUID). While `event`
    (a MENU_EVENTS name) runs, the map's variant for it when the catalog has one and the mode can be hosted there:
    the server names the map, and the client loads no variant on its own (research/lizrollin-lobby.md section 5)."""
    names = {entry['name'].lower(): int(entry['guid'], 16) for entry in map_catalog()}
    variant = names.get(f'{name} ({EVENT_MAPS.get(event)})'.lower())
    found = variant and game_map(variant, int(mode, 16))
    if not found:
        found = game_map(names.get(name.lower()) or int(name, 16), int(mode, 16))
    if found is None:
        raise ValueError(f'cannot host {name} in mode {mode}')
    return found


class GameServer(BaseGameServer):
    gameplay_enabled = True
    body_setup = staticmethod(lambda player: (initialize_practice_kit if player.hero.guid in KITS
        else initialize_practice_soldier)(player))


def profile_name(value):
    if not (value.isascii() and value.isalnum() and 1 <= len(value) <= 12):
        raise ValueError('name must be 1..12 ASCII letters or digits')
    return value


class PracticeRange:
    account = None  # opt-in earned account; ordinary research sessions keep all unlocks
    pending_settlement, reward_retry_at, reward_failed = None, 0.0, False
    party_hero = 0  # the hero the party card shows (chosen at login)
    arcade_cards = {}  # the Arcade catalog, once sent: a card's queue needs roles or not
    # shortcut: the PvE handoff is switched by the environment (OW_PVE_HANDOFF=1) until the missions have rules
    pve_handoff = os.environ.get('OW_PVE_HANDOFF') == '1'
    queue, queue_state = None, 0  # the key of the role queue the player is in (Quick Play), and its state

    def __init__(self, port=3730, game_map=PRACTICE_RANGE, difficulty='medium', retail_menu=False, event='none',
                 equips=None, game_install=None, name='Player', progression=False):
        self.name = profile_name(name)
        self.game_map = game_map  # the vs-AI map; the Practice Range card opens the Range
        # With tools/asi/ow_fixes.asi the client sends TOURNAMENT_MATCH's message on every menu change.
        self.retail_menu = retail_menu
        self.event = MENU_EVENTS[event]  # the main menu's event (38900)
        self.game = GameServer(host='127.0.0.1', port=port, skin_of=self.skin_of, on_leave=self.player_left)
        self.game.on_finish = self.match_finished
        self.game.difficulty = difficulty
        self.schemas = Schemas()
        self.catalog_heroes = []
        self.hero_table = {}
        self.reward_queue = SimpleQueue()
        if progression:
            from progression import Account
            self.account = Account(PROGRESSION_CATALOG.parent / 'profiles' / f'{self.name}.sqlite3',
                                   json.loads(PROGRESSION_CATALOG.read_text(encoding='utf-8')))
        self.equips_path = equips or EQUIPS
        self.career = Career(self.equips_path.with_name('career.json'))  # the Career Profile's stats
        try:
            self.equips = [(int(h), int(i), int(n), str(s)) for h, i, n, s
                           in json.loads(self.equips_path.read_text(encoding='utf-8'))]
        except (OSError, ValueError, TypeError):  # none saved yet, or a damaged file: start from the defaults
            self.equips = []
        self.settings_path = self.equips_path.with_name('settings.json')  # the saved Options, beside the equips
        try:
            self.saved = json.loads(self.settings_path.read_text(encoding='utf-8'))
        except (OSError, ValueError):  # none saved yet, or a damaged file: the defaults
            self.saved = {}
        self.game_install, self.storage = game_install, None  # the install, read for an equipped item's slot
        self.themes = {}  # skin unlock -> its skin theme, read once
        if self.account:
            self.equips = self.account.snapshot()['equips']

    @staticmethod
    def menu_heroes():
        """Every playable 1.74 hero's GUID, for the login record: the menus and hero select list them, and the
        client loads the bodies of the vs-AI bots from it (without it: "Missing entity definition")."""
        from ow174.game.content import heroes
        return tuple(heroes())

    def hero_catalog(self, groups, table):
        """Message 24900 owning every unlock of every hero in `table` (lobby.hero_content); None when the client
        did not announce the group. Each hero equips its default items, then its saved gallery equips (`equipped`)."""
        if HERO_CATALOG not in groups:
            return None
        self.hero_table = table
        owned = None
        if self.account:
            state = self.account.snapshot()
            owned, self.equips = set(state['owned']), state['equips']
        heroes = [{'+0x0': [{'+0x0': int(unlock, 16), '+0x8': True} for unlock in entry['unlocks']
                            if owned is None or unlock in owned],
                   '+0x30': int(hero, 16), **loadout(equipped(entry, [(i, n, s) for h, i, n, s in self.equips if h == int(hero, 16)]))}
                  for hero, entry in sorted(table.items())]
        self.catalog_heroes = heroes  # the career profile repeats them
        # The gallery lists only a hero's store entries. Defaults get retail's default entry (left out of the hero's
        # count), the rest upstream's not-for-sale entry.
        stores = [{'+0x0': [self.catalog_entry(unlock, defaults_of(entry)) for unlock in entry['unlocks']],
                   '+0x18': int(hero, 16)} for hero, entry in sorted(table.items())]
        return (self.schemas.header(HERO_CATALOG, groups[HERO_CATALOG], 24900)
                + self.schemas.encode(HERO_CATALOG, 24900, {'+0x80': heroes, '+0x98': stores}))

    def slot_of(self, item):
        """The loadout slot name of an unlock, read from the install; None without one."""
        if self.account:
            return self.account.items.get(f'{item:016X}', {}).get('slot')
        return None if self.game_install is None else item_slot(self.install(), item)

    def install(self):
        """The game install's storage, loaded on first use."""
        if self.storage is None:
            from retail_bundle_lookup import load_storage
            self.storage = load_storage(self.game_install)
        return self.storage

    def skin_of(self, account, hero):
        """The game server's (skin theme, golden weapon) for the player's hero: the theme of the last saved Skin
        equip, if 20505 preloaded it (the hero's skin_themes); else (0, False), the body's own look.
        shortcut: no golden weapon; the WeaponSkin slot is not read."""
        skins = [item for h, item, _, slot in self.equips if h == hero and slot == 'Skin']
        if not skins or self.game_install is None:
            return 0, False
        if skins[-1] not in self.themes:
            self.themes[skins[-1]] = skin_theme(self.install(), skins[-1])
        theme = self.themes[skins[-1]]
        if f'{theme:016X}' not in self.hero_table.get(f'{hero:016X}', {}).get('skin_themes', ()):
            LOG.info('Skin %016X: theme %X not preloaded for hero %X; default look', skins[-1], theme, hero)
            return 0, False
        return theme, False

    def progression(self, groups):
        """Message 24300, the account record at level 1; None when the client did not announce the group."""
        if PROGRESSION not in groups:
            return None
        value = {'+0x78': self.account_record()}
        if self.account:
            value['+0xF8'] = {'+0x0': [self.catalog_entry(guid, set()) for guid, item in self.account.items.items()
                                     if item['hero'] == 0]}
        return (self.schemas.header(PROGRESSION, groups[PROGRESSION], 24300)
                + self.schemas.encode(PROGRESSION, 24300, value))

    def catalog_entry(self, guid, defaults):
        entry = store_entry(guid, defaults)
        if self.account:
            item = self.account.items.get(guid, {})
            entry['+0x14'] = item.get('price', 0)
            entry['+0x16'] = 0 if 0 in item.get('boxes', []) else -1
            entry['+0x17'] = item.get('default', False)
        return entry

    def account_record(self):
        if not self.account:
            return ACCOUNT_RECORD
        from progression import xp_needed
        state = self.account.snapshot()
        frame = next((int(g, 16) for level, g in reversed(self.account.frames) if level <= state['level']), FIRST_BORDER)
        return {**ACCOUNT_RECORD, '+0x0': [self.box_entry(box) for box in state['boxes']],
                '+0x30': [{'+0x0': int(g, 16), '+0x8': True} for g in state['owned']
                          if self.account.items.get(g, {}).get('hero') == 0],
                '+0x48': state['xp'], '+0x50': xp_needed(state['level']), '+0x58': frame,
                '+0x60': state.get('icon', 0), '+0x68': state['level'], '+0x6C': state['credits']}

    @staticmethod
    def box_entry(box):
        value = int(box['id'], 16)
        return {'+0x0': {'+0x0': [value & 0xFFFFFFFFFFFFFFFF, value >> 64]}, '+0x10': box['type'], '+0x14': 1}

    def match_finished(self, match, now):
        from ow174.game.match import SETUP_SECONDS
        for player in match.players:
            if player.account_lo == 1 and player.client is not None and player.steps_done >= 4:
                self.count_career(match, player, match.assault.winner == player.team)
                if self.account:
                    self.reward_queue.put((f'{match.id[0]:016X}{match.id[1]:016X}',
                        min(14400, max(0, now - match.assemble_ends - SETUP_SECONDS)),
                        match.assault.winner == player.team, datetime.now(timezone.utc).date().isoformat()))

    def player_left(self, player):
        self.count_career(player.match, player)

    def count_career(self, match, player, won=None):
        """Fold what the match counted for this player (Player.count) into the career: at the match's end with its
        result, and when the player leaves (won None: no game counted). The Practice Range counts nothing, as retail."""
        played, player.career = player.career, {}
        if match.game_map.map_guid != PRACTICE_RANGE.map_guid:
            # shortcut: every other match counts as Quick Play, the one mode key confirmed (career.py); give the vs-AI
            # and PvE cards their own when their keys are known.
            self.career.add_match(QUICK_PLAY, played, won)

    def pending_rewards(self, groups):
        if not self.account or time.monotonic() < self.reward_retry_at:
            return []
        changed = False
        while True:
            if self.pending_settlement is None:
                try:
                    self.pending_settlement = self.reward_queue.get_nowait()
                except Empty:
                    break
            try:
                award = self.account.settle(*self.pending_settlement)
            except sqlite3.Error:
                if not self.reward_failed:
                    LOG.exception('Progression could not save the match; retaining settlement for retry')
                self.reward_failed, self.reward_retry_at = True, time.monotonic() + 1
                break
            changed |= self.reward_failed
            self.pending_settlement, self.reward_failed = None, False
            if award:
                changed = True
                LOG.info('Progression settled: %d XP, level %d, %d new boxes',
                         award['xp'], award['level'], len(award['boxes']))
        return [r for r in (self.hero_catalog(groups, self.hero_table), self.progression(groups),
                self.player_refresh(groups), self.party(groups, self.party_hero)) if r is not None] if changed else []

    def progression_request(self, payload, groups):
        if not self.account or PROGRESSION not in groups:
            return None
        msg_id, body = self.schemas.split(PROGRESSION_OUT, payload)
        if msg_id not in (24200, 24201, 24202, 24203):
            return None
        value = self.schemas.decode(PROGRESSION_OUT, msg_id, body)
        if msg_id in (24200, 24202):
            return None  # reports acknowledge display; they never authorize rewards
        replies = []
        if msg_id == 24201:
            lo, hi = value['+0x78']['+0x0']
            failed = False
            try:
                result = self.account.open_box(f'{lo | hi << 64:032x}') or []
            except (sqlite3.Error, ValueError):
                LOG.exception('Loot box could not be saved; retaining inventory and releasing the reveal')
                result, failed = [], True
            items = [{'+0x0': item['hero'], '+0x8': int(item['item'], 16), '+0x10': item['credits'],
                      '+0x14': i, '+0x1C': item['duplicate'], '+0x1D': item['new']} for i, item in enumerate(result)]
            replies.append(self.schemas.header(PROGRESSION, groups[PROGRESSION], 24305)
                + self.schemas.encode(PROGRESSION, 24305, {'+0x78': {'+0x0': items, '+0x18': value['+0x78']}}))
            if failed:
                try:
                    replies.append(self.progression(groups))
                except sqlite3.Error:
                    LOG.exception('Account snapshot unavailable; retained box will return on relog')
                return replies
            granted = [item['item'] for item in result if item['new']]
        else:
            guid = f"{value['+0x78']:016X}"
            try:
                self.account.purchase(guid)
            except (ValueError, sqlite3.Error) as exc:
                LOG.info('Purchase rejected: %s', exc)
                try:
                    return [self.progression(groups)]
                except sqlite3.Error:
                    LOG.exception('Purchase failed and account snapshot is unavailable; no success acknowledged')
                    return None
            hero = self.account.items[guid]['hero']
            replies.append(self.schemas.header(PROGRESSION, groups[PROGRESSION], 24306)
                + self.schemas.encode(PROGRESSION, 24306, {'+0x78': int(guid, 16), '+0x80': hero}))
            granted = [guid]
        for guid in granted:
            hero = self.account.items[guid]['hero']
            crc, msg = (HERO_CATALOG, 24901) if hero else (PROGRESSION, 24301)
            grant = {'+0x78': hero, '+0x80': int(guid, 16), '+0x88': True} if hero else {'+0x78': int(guid, 16)}
            if crc in groups:
                replies.append(self.schemas.header(crc, groups[crc], msg) + self.schemas.encode(crc, msg, grant))
        try:
            balance = self.account.snapshot()['credits']
        except sqlite3.Error:
            LOG.exception('Reward committed but balance refresh is unavailable; balance returns on relog')
            return replies
        replies.append(self.schemas.header(PROGRESSION, groups[PROGRESSION], 24307)
            + self.schemas.encode(PROGRESSION, 24307, {'+0x78': balance}))
        return replies

    def leaver_status(self, groups):
        """Message 20813, the leaver status: recent-leave bitmask, games counted, penalty and warning ratios (retail's
        recorded values). Without it the party card shows "-75% XP / 0 matches remaining" (research/hero-menu.md)."""
        if LOBBY not in groups:
            return None
        return (self.schemas.header(LOBBY, groups[LOBBY], 20813)
                + self.schemas.encode(LOBBY, 20813, {'+0x78': 0, '+0x80': 20, '+0x88': 0.3, '+0x90': 0.2}))

    def settings(self, groups):
        """Message 20802, the account's saved settings at login (LizRollin's login.py sends it): each block the client
        saved (save_settings), and defaults for the others; None when the client did not announce the group."""
        if LOBBY not in groups:
            return None
        value = self.schemas.empty(LOBBY, 20802)
        value['+0x108']['+0x4'] = True  # upstream's defaults
        value['+0x10D'].update({'+0x0': True, '+0x5': 1})
        # The client's Options defaults (0x7FF6D1CC8650) for the block it keeps at settings system +0x2B8, Gameplay's
        # +0x4 Kill Feed Display (UI 0x6426, +700: gates the feed panel, 016D65A8), +0x5 Always Skip Kill Cam (off)
        # and +0x7 Tips Display (UI 0x62BC).
        value['+0x120'].update({'+0x4': True, '+0x7': True})
        saved = self.saved if isinstance(self.saved, dict) else {}
        saved = {field: saved[field] for field in (*SETTINGS_BLOCKS.values(), SETTING_VALUES)
                 if type(saved.get(field)) is type(value[field])}  # a block of another shape: its default
        try:
            body = self.schemas.encode(LOBBY, 20802, {**value, **saved})
        except Exception:  # a settings.json that no longer fits the layout must not stop the login
            LOG.exception('Saved settings do not fit message 20802; sending the defaults')
            body = self.schemas.encode(LOBBY, 20802, value)
        return self.schemas.header(LOBBY, groups[LOBBY], 20802) + body

    def save_settings(self, msg_id, value):
        """Keep what the client saved for the next login's 20802: a whole block, or (22204) its changed key/value
        entries merged into the list by key (+0x8)."""
        if msg_id == 22204:
            entries = {entry['+0x8']: entry for entry in (*self.saved.get(SETTING_VALUES, ()), *value)}
            self.saved[SETTING_VALUES] = list(entries.values())
        else:
            self.saved[SETTINGS_BLOCKS[msg_id]] = value
        self.settings_path.parent.mkdir(parents=True, exist_ok=True)
        self.settings_path.write_text(json.dumps(self.saved), encoding='utf-8')
        LOG.info('Settings saved (%d)', msg_id)

    def career_profile(self, groups, payload):
        """Answer to the career profile request 22206: our own full profile 20807 (account record, hero records,
        name), repeating the request id; None when the client did not announce the reply's group."""
        if LOBBY not in groups:
            return None
        _, body = self.schemas.split(SOCIAL_OUT, payload)
        request = self.schemas.decode(SOCIAL_OUT, 22206, body)
        # shortcut: always our own profile, with no ratings or endorsements (upstream's 39001 for others).
        if self.account:
            self.hero_catalog(groups, self.hero_table)
        stats = self.career.categories()
        profile = {'+0x0': self.account_record(), '+0x80': self.catalog_heroes, '+0x100': ACCOUNT, '+0x138': self.name,
                   '+0xB0': stats, '+0xC8': stats}
        answer = (self.schemas.header(LOBBY, groups[LOBBY], 20807)
                  + self.schemas.encode(LOBBY, 20807, {'+0x78': request['+0x78'], '+0x90': profile}))
        # 27202 carries the stats too: sent again first, so a match played since the login shows whichever the client reads
        return [self.career_stats(groups), answer]

    def career_stats(self, groups):
        """Message 27202: the stat catalogue (the stats career.py counts, for every playable hero and All Heroes) and
        the career stats; None when the client did not announce the group."""
        if MODE_RULES not in groups:
            return None
        stats = self.career.categories()
        value = {'+0x78': stats, '+0x90': Career.catalogue(self.menu_heroes()), '+0xA8': stats, '+0xC0': []}
        return self.schemas.header(MODE_RULES, groups[MODE_RULES], 27202) + self.schemas.encode(MODE_RULES, 27202, value)

    def events(self, groups, now=None):
        """Message 38900 with the chosen menu event running (started 2 days ago, ends in a year, as upstream); None
        without the group."""
        if EVENTS not in groups:
            return None
        if self.event is None:
            return (self.schemas.header(EVENTS, groups[EVENTS], 38900)
                    + self.schemas.encode(EVENTS, 38900, {'+0x78': []}))
        now = time.time() if now is None else now
        celebration, key, key_name, kind = self.event
        record = {'+0x30': key_name, '+0x38': CONTENT_KEY | key if key else 0, '+0x40': CELEBRATION | celebration,
                  '+0x70': CELEBRATION_TYPE | kind if kind else 0,
                  '+0x48': {'+0x0': stu_datetime(now - 2 * 86400)}, '+0x50': {'+0x0': stu_datetime(now + 365 * 86400)},
                  '+0xB0': -1}  # -1: no event loot box
        return (self.schemas.header(EVENTS, groups[EVENTS], 38900)
                + self.schemas.encode(EVENTS, 38900, {'+0x78': [record]}))

    def arcade(self, groups, now=None):
        """Messages 39802 (the Arcade's cards: the PvE event groups, then the everyday cards; the recorded Quick Play and
        competitive cards), 39810 (each card's dates) and 39804 (how busy each is); None without the group."""
        if ARCADE not in groups:
            return None
        now = int(time.time() if now is None else now)
        data = json.loads(ARCADE_DATA.read_text(encoding='utf-8'))
        self.arcade_cards = data['cards']
        cards = [index for group in ARCADE_GROUPS for index in group if f'0x{index:X}' in data['cards']]
        lists = {'+0x0': [self._card(index, data['cards'][f'0x{index:X}']) for index in cards],
                 '+0x18': data['quick_play'], '+0xD0': data['competitive']}
        windows = [{'+0x0': CARD | index, '+0x8': now - 86400, '+0x10': now + 30 * 86400} for index in cards]
        busy = [{'+0x0': CARD | index, '+0x8': 0.1} for index in cards]
        return [self.schemas.header(ARCADE, groups[ARCADE], msg_id) + self.schemas.encode(ARCADE, msg_id, value)
                for msg_id, value in ((39802, {'+0x78': lists}), (39810, {'+0x78': windows}), (39804, {'+0x78': busy}))]

    @staticmethod
    def _card(index, data):
        """A 39802 card from the catalog entry: flags 12-22 are permanent, the asset's seven, two unused, role queue."""
        card = {'+0x0': CARD | index, '+0x8': 0, '+0x10': int(data['identifier'], 16), '+0x18': int(data['display'], 16),
                '+0x20': [int(reward, 16) for reward in data['rewards']], '+0x38': [], '+0x50': [], '+0x68': [], '+0x80': 0,
                '+0x88': [], '+0xA0': 0, '+0xA4': 24 if index in (0x15C, 0x34) else 0}
        for number, flag in enumerate((index in FLAG_12, *data['flags'], False, False, data['roles'])):
            card[f'+0x{0xA8 + number:X}'] = flag
        return card

    def party(self, groups, hero, skin=0):
        """Message 20700: the player alone in their party, as leader, showing `hero`; while in a role queue, the
        queue entry and the player's role choice (without them the role cards stay grey); None without the group."""
        if PARTY not in groups:
            return None
        self.party_hero = hero
        card = self.player_card()
        member = {'+0x0': card, '+0xB8': hero, '+0xC0': skin, '+0xE0': 5, '+0xE2': True}  # 5: member
        party = {'+0x0': [member], '+0x60': {'+0x0': 1, '+0x8': 1}, '+0x70': {'+0x0': PARTY_ENTITY, '+0x8': 1},
                 '+0x90': 15959616, '+0x94': True, '+0x95': 1, '+0x96': True, '+0x98': True}  # upstream's values
        if self.queue is not None:
            party['+0x18'] = [{'+0x0': {'+0x0': {'+0x0': [role_record(role) for role in ROLES]}}, '+0x18': self.queue,
                              '+0x48': 0, '+0x50': 0.0, '+0x54': self.queue_state}]
            ready = self.queue_state == SEARCHING
            member['+0xC8'] = [{'+0x0': 0, '+0x8': list(ROLES) if ready else [], '+0x20': [], '+0x38': [], '+0x50': [],
                                '+0x68': True, '+0x69': ready}]
        return (self.schemas.header(PARTY, groups[PARTY], 20700)
                + self.schemas.encode(PARTY, 20700, {'+0x78': party}))

    def player_card(self):
        record = self.account_record()
        return {'+0x0': ACCOUNT, '+0x10': ACCOUNT, '+0x20': record.get('+0x60', 0), '+0x28': record['+0x58'],
                '+0x38': record['+0x68'], '+0x3C': 1, '+0x40': self.name}

    def player_refresh(self, groups):
        from lobby import IN_CONNECT, player_record
        if IN_CONNECT in groups:
            return bytes((groups[IN_CONNECT], 2)) + player_record(self.name, self.player_card())

    def menu_hero(self, groups, hero):
        """Message 36600 setting the main menu's hero; None when the client did not announce the group."""
        if CONFIG not in groups:
            return None
        entries = [{'+0x0': SUPPORTED_BUILDS, '+0x8': '[104319]'}, {'+0x0': MENU_HERO, '+0x8': f'0x{hero:016x}'}]
        if self.account:
            entries.append({'+0x0': UNLOCK_ALL, '+0x8': '0'})
        return (self.schemas.header(CONFIG, groups[CONFIG], 36600)
                + self.schemas.encode(CONFIG, 36600, {'+0x78': entries}))

    def __enter__(self):
        self.game.start()
        return self

    def __exit__(self, *_):
        self.game.stop()

    def request(self, payload, groups):
        """The Practice Range card opens the Range; the Tournament Match card (tournament menu) or the Practice
        vs. AI queue (retail menu) opens the hosted map (vs AI)."""
        if len(payload) >= 2 and payload[0] == groups.get(PROGRESSION_OUT):
            return self.progression_request(payload, groups)
        if self.account and len(payload) >= 2 and payload[0] == groups.get(GALLERY) and payload[1] == 0:
            equip = self.schemas.decode(GALLERY, 24500, payload[2:])
            try:
                icon = self.slot_of(equip['+0x80']) == 'Icon'
                if f"{equip['+0x78']:016X}" not in self.hero_table and not (icon and equip['+0x78'] == 0):
                    raise ValueError('unknown hero')
                self.account.equip(equip['+0x78'], f"{equip['+0x80']:016X}", equip['+0x88'])
            except (ValueError, sqlite3.Error) as exc:
                LOG.info('Equip rejected: %s', exc)
                try:
                    return self.hero_catalog(groups, self.hero_table)
                except sqlite3.Error:
                    LOG.exception('Equip failed and account snapshot is unavailable; no success acknowledged')
                    return None
            reply = self.schemas.header(HERO_CATALOG, groups[HERO_CATALOG], 24902) + self.schemas.encode(HERO_CATALOG, 24902, equip)
            try:
                self.equips = self.account.snapshot()['equips']
            except sqlite3.Error:
                LOG.exception('Equip committed but card refresh is unavailable; state returns on relog')
                return reply
            return [reply, self.player_refresh(groups), self.party(groups, self.party_hero)] if icon else reply
        if len(payload) >= 2 and payload[0] == groups.get(SOCIAL_OUT):
            msg_id, body = self.schemas.split(SOCIAL_OUT, payload)
            if msg_id in SETTINGS_BLOCKS or msg_id == 22204:
                self.save_settings(msg_id, self.schemas.decode(SOCIAL_OUT, msg_id, body)['+0x78'])
            return self.career_profile(groups, payload) if msg_id == 22206 else None
        if len(payload) >= 2 and payload[0] == groups.get(QUEUE):
            return self.queue_request(groups, payload)
        if len(payload) < 2 or payload[1] != 0:
            return None
        if payload[0] == groups.get(GALLERY) and HERO_CATALOG in groups:
            msg_id, body = self.schemas.split(GALLERY, payload)
            if msg_id != 24500:
                return None  # 24501 (item seen) needs no answer
            equip = self.schemas.decode(GALLERY, 24500, body)
            slot = self.slot_of(equip['+0x80'])
            if slot is not None:  # kept for the next login's 24900; later equips of a slot win
                key = (equip['+0x78'], equip['+0x80'], equip['+0x88'], slot)
                self.equips = [e for e in self.equips if e != key] + [key]
                self.equips_path.parent.mkdir(parents=True, exist_ok=True)
                self.equips_path.write_text(json.dumps(self.equips), encoding='utf-8')
            return (self.schemas.header(HERO_CATALOG, groups[HERO_CATALOG], 24902)
                    + self.schemas.encode(HERO_CATALOG, 24902, equip))
        if payload[0] == groups.get(TOURNAMENT_MATCH) and not self.retail_menu:
            # shortcut: the request's fields (ids?) are not decoded; any request from the card starts the match
            if len(payload) > 256:
                raise ValueError('invalid tournament-match request')
            return self._handoff(groups, self.game_map)
        if payload[0] != groups.get(REQUEST):
            return None
        end = payload.find(b'\0', 3)
        if len(payload) > 256 or end < 0 or len(payload) != end + 5:
            raise ValueError('invalid create-game request')
        kind, flags = payload[2], struct.unpack_from('<i', payload, end + 1)[0]
        if (kind, flags) != (2, 4):
            return None
        return self._handoff(groups, PRACTICE_RANGE)

    def queue_request(self, groups, payload):
        """The client's queue messages. A Practice vs. AI difficulty (44100 with a vs-AI queue) opens the hosted
        map at once. Quick Play's role queue: 44100 joins (44201, the waits and the party's queue entry open the
        role cards), Ready (44103) starts the search and opens the same hosted map, cancel (44102) leaves (44202).
        Returns the frames to send, or None."""
        msg_id, body = self.schemas.split(QUEUE, payload)
        if msg_id == 44100:
            if len(payload) != 46:
                raise ValueError('invalid queue request')
            key = self.schemas.decode(QUEUE, 44100, body)['+0x78']
            difficulty = VS_AI_QUEUES.get(key['+0x0']['+0x0'] & 0xFFFFFFFF)  # a 0x063... queue GUID; vs AI: 0x70..0x72
            if difficulty is not None:
                self.game.difficulty = difficulty
                return self._handoff(groups, self.game_map)
            if QUEUE_STATE not in groups or QUEUE_WAITS not in groups:
                return None
            card = self.arcade_cards.get(f"0x{key['+0x0']['+0x0'] & 0xFFFFFFFF:X}")
            if card is not None and not card['roles']:  # an Arcade card without roles: it searches until cancelled
                index = key['+0x0']['+0x0'] & 0xFFFFFFFF
                self.queue, self.queue_state = key, SEARCHING  # shortcut: its mode is not hosted (PvE: scripted loading)
                if self.pve_handoff and index in PVE_CARDS:  # the experiment: hand off to the PvE map at once
                    frames = [self._queue_joined(groups), self._queue_left(groups, 1)]
                    self.queue, self.queue_state = None, 0
                    return frames + [self._handoff(groups, self.pve_map(index))]
                LOG.info('Arcade card 0x%X queued (no match for it yet)', index)
                return [self._queue_joined(groups)]
            self.queue, self.queue_state = key, PICKING
            waits = {'+0x78': {'+0x0': {'+0x0': [role_record(role) for role in (0, *ROLES)]}}}  # 0: the flex card
            return [self._queue_joined(groups),
                    self.schemas.header(QUEUE_WAITS, groups[QUEUE_WAITS], 56200)
                    + self.schemas.encode(QUEUE_WAITS, 56200, waits),
                    self.party(groups, self.party_hero)]
        if self.queue is None:
            return None
        if msg_id == 44103:  # Ready: the player is the whole party, so the search starts and the match opens.
            # The match found (44202, reason 1 QueuePopped) drops the client's entry first, or it keeps searching
            # after the match (upstream).
            self.queue_state = SEARCHING
            frames = [self._queue_joined(groups), self.party(groups, self.party_hero), self._queue_left(groups, 1)]
            self.queue, self.queue_state = None, 0
            return frames + [self.party(groups, self.party_hero), self._handoff(groups, self.game_map)]
        if msg_id == 44107:  # Change Role: back to choosing
            self.queue_state = PICKING
            return [self.party(groups, self.party_hero)]
        if msg_id == 44102:  # cancel: the client dropped its own entry already; the party state loses the queue
            left = self._queue_left(groups, 0)
            self.queue, self.queue_state = None, 0
            return [left, self.party(groups, self.party_hero)]
        return None  # 44106 (priority pass role) needs no answer

    def handoff_maps(self):
        """Low 32 bits of the maps the PvE handoff can send the client to; 20505 must advertise their headers."""
        return {guid & 0xFFFFFFFF for guid, *_ in PVE_CARDS.values()} if self.pve_handoff else set()

    def handoff_heroes(self):
        """The omnics' hero GUIDs: 20505 must list them or the client drops the match on their first body
        ("Missing entity definition: 0x040000000000184A", as for the vs-AI bots)."""
        from target import OMNICS
        return tuple(OMNICS) if self.pve_handoff else ()

    @staticmethod
    def pve_map(card):
        """The PvE card's map in its mode, built without game_map's guards (scripted loading, no catalog spawns).
        The players spawn at the card's point (team 0); the map's omnic markers are team 1's spawns, nearest first,
        and the players' point is the objective the omnics walk to. shortcut: the mission's own objective areas and
        wave routes are 0D4A's and unread."""
        map_guid, mode, position, yaw = PVE_CARDS[card]
        entry = _map_entries()[map_guid]
        placeables = tuple(entry.get('placeables_by_mode', {}).get(f'0x{mode:016X}', entry.get('placeables', [])))
        markers = json.loads(PVE_MARKERS.read_text(encoding='utf-8')).get(f'0x{map_guid:016X}', [])
        markers.sort(key=lambda marker: math.dist(marker[:3], position))
        spawns = (Spawn(0, tuple(position), yaw), *(Spawn(1, tuple(marker[:3]), marker[3]) for marker in markers))
        return GameMap(entry['name'], map_guid, mode, placeables, spawns, 'Uprising', (tuple(position),))

    def _queue_joined(self, groups):
        """44201: the client's queue entry for the key it asked for; sent again when the search starts, so the
        client's search timer starts at 0 (upstream)."""
        entry = {'+0x0': self.queue, '+0x30': {'+0x0': 0, '+0x8': 0, '+0x10': 0}, '+0x48': 0, '+0x50': 0.0,
                 '+0x54': 0, '+0x55': False, '+0x56': False}
        return (self.schemas.header(QUEUE_STATE, groups[QUEUE_STATE], 44201)
                + self.schemas.encode(QUEUE_STATE, 44201, {'+0x78': entry}))

    def _queue_left(self, groups, reason):
        """44202: the client drops its entry for the queue (0: the player cancelled, 1: a match was found)."""
        return (self.schemas.header(QUEUE_STATE, groups[QUEUE_STATE], 44202)
                + self.schemas.encode(QUEUE_STATE, 44202, {'+0x78': self.queue, '+0xA8': reason}))

    def _handoff(self, groups, game_map):
        if HANDOFF not in groups:
            raise ValueError('client omitted the game handoff protocol')
        if self.game.in_match(1):
            return None
        handoff, = self.game.create_match(game_map, [(1, self.name, SOLDIER, 0, True)])
        address = list(b'127.0.0.1'.ljust(64, b'\0'))
        value = {'+0x78': False, '+0x80': {
            '+0x0': {'+0x0': handoff.match_id[0], '+0x8': handoff.match_id[1]},
            '+0x28': handoff.conn, '+0x2C': self.game.port,
            '+0x2E': address, '+0x6E': address,
            '+0xAE': list(handoff.key_in), '+0xCE': list(handoff.key_out), '+0xEE': False}}
        LOG.info('%s handoff to 127.0.0.1:%d', game_map.name, self.game.port)
        return self.schemas.header(HANDOFF, groups[HANDOFF], 20600) + self.schemas.encode(HANDOFF, 20600, value)
