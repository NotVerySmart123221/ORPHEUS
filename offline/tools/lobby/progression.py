"""One durable earned account; all reward effects commit before network replies."""
from contextlib import closing, contextmanager
from datetime import date
import json
import math
from pathlib import Path
import random
import sqlite3
import uuid

LIST_SLOTS = ('Spray', 'VoiceLine', 'Emote')
DUPLICATE_CREDITS = (5, 15, 50, 200)
# Chance in 100 of each rarity (Common, Rare, Epic, Legendary) in an ordinary slot. One slot of a box is Rare or better:
# it gets Common's share as Rare. A box then holds an Epic 18.5% of the time and a Legendary 7.5%, which
# LizRollin's services/lootbox.py gives as the game's own "average chance from containers" figures; the split
# per slot is theirs.
SLOT_ODDS = (83.09, 10.0, 4.98, 1.93)


def xp_needed(level):
    # shortcut: introductory retail curve is unresolved; replace after recovering the level table.
    return 2000 if level == 1 else 20000


class Account:
    def __init__(self, path, catalog):
        if catalog.get('version') != 2:
            raise ValueError('unsupported progression catalog; extract it again')
        self.path, self.items, self.frames = Path(path), catalog['items'], catalog['frames']
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with closing(sqlite3.connect(self.path)) as db, db:
            version = db.execute('PRAGMA user_version').fetchone()[0]
            tables = db.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall()
            if version not in (0, 1) or (version == 0 and tables):
                raise ValueError('unsupported account database; refusing to overwrite it')
            if version == 0:
                db.execute('CREATE TABLE account (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL)')
                db.execute('CREATE TABLE receipts (kind TEXT, id TEXT, data TEXT NOT NULL, PRIMARY KEY(kind,id))')
                state = {'level': 1, 'xp': 0, 'credits': 0, 'owned': sorted(g for g, item in self.items.items()
                          if item.get('default')), 'equips': [], 'boxes': [], 'first_win': '', 'icon': 0}
                db.execute('INSERT INTO account VALUES (1,?)', (json.dumps(state),))
                db.execute('PRAGMA user_version=1')
            elif not {'account', 'receipts'} <= {row[0] for row in tables}:
                raise ValueError('missing account tables; refusing to reset progression')
            self._state(db)

    def _state(self, db):
        row = db.execute('SELECT data FROM account WHERE id=1').fetchone()
        if row is None:
            raise ValueError('missing account state; refusing to reset progression')
        state = json.loads(row[0])
        if (not isinstance(state, dict) or any(type(state.get(k)) is not int for k in ('level', 'xp', 'credits'))
                or not 1 <= state['level'] <= 2147483647 or not 0 <= state['xp'] < xp_needed(state['level'])
                or not 0 <= state['credits'] <= 2147483647
                or any(not isinstance(state.get(k), list) for k in ('owned', 'equips', 'boxes'))
                or not isinstance(state.get('first_win'), str)):
            raise ValueError('invalid account state; refusing to reset progression')
        if (any(not isinstance(g, str) or g not in self.items for g in state['owned'])
                or len(set(state['owned'])) != len(state['owned'])
                or any(not isinstance(b, dict) or b.get('type') != 0 or not isinstance(b.get('id'), str)
                       or len(b['id']) != 32 or any(c not in '0123456789abcdef' for c in b['id']) for b in state['boxes'])
                or type(state.get('icon', 0)) is not int
                or (state.get('icon', 0) and f"{state['icon']:016X}" not in state['owned'])):
            raise ValueError('invalid account inventory; refusing to reset progression')
        return state

    @contextmanager
    def _change(self):
        with closing(sqlite3.connect(self.path, timeout=10)) as db, db:
            db.execute('BEGIN IMMEDIATE')
            state = self._state(db)
            yield db, state
            db.execute('UPDATE account SET data=? WHERE id=1', (json.dumps(state),))
            self._state(db)

    def snapshot(self):
        with closing(sqlite3.connect(self.path)) as db:
            return self._state(db)

    def settle(self, match_id, seconds, won, day):
        """Apply a completed local match once; return the award or None for a retry."""
        if (not isinstance(match_id, str) or not 1 <= len(match_id) <= 128 or type(won) is not bool
                or not isinstance(seconds, (int, float)) or not math.isfinite(seconds) or not 0 <= seconds <= 14400):
            raise ValueError('invalid match settlement')
        if date.fromisoformat(day).isoformat() != day:
            raise ValueError('match day must use YYYY-MM-DD')
        with self._change() as (db, state):
            if db.execute("SELECT 1 FROM receipts WHERE kind='match' AND id=?", (match_id,)).fetchone():
                return None
            # shortcut: 1.75 XP/second plus shipped finish/win bonuses; retail scaling/rounding is unresolved.
            xp = int(seconds * 1.75) + 75 + (250 if won else 0)
            if won and state['first_win'] < day:
                xp += 1500
                state['first_win'] = day
            state['xp'] += xp
            boxes = []
            while state['xp'] >= xp_needed(state['level']):
                state['xp'] -= xp_needed(state['level'])
                state['level'] += 1
                box = {'id': uuid.uuid4().hex, 'type': 0}
                state['boxes'].append(box)
                boxes.append(box)
            for level, guid in self.frames:
                if level <= state['level'] and guid not in state['owned']:
                    state['owned'].append(guid)
            result = {'xp': xp, 'level': state['level'], 'boxes': boxes}
            db.execute('INSERT INTO receipts VALUES (?,?,?)', ('match', match_id, json.dumps(result)))
            return result

    def open_box(self, box_id):
        if not isinstance(box_id, str) or len(box_id) != 32 or any(c not in '0123456789abcdef' for c in box_id):
            raise ValueError('invalid loot box ID')
        with self._change() as (db, state):
            receipt = db.execute("SELECT data FROM receipts WHERE kind='box' AND id=?", (box_id,)).fetchone()
            if receipt:
                return json.loads(receipt[0])
            box = next((b for b in state['boxes'] if b['id'] == box_id), None)
            if box is None:
                return None
            pools = {r: [g for g, item in self.items.items() if box['type'] in item.get('boxes', [])
                         and item['rarity'] == r and not item.get('default')] for r in range(4)}
            if not any(pools[r] for r in (1, 2, 3)):
                raise ValueError('loot catalog has no Rare-or-better rewards; box retained')
            rng, result, owned = random.SystemRandom(), [], set(state['owned'])
            for index in range(4):
                odds = (0, SLOT_ODDS[0] + SLOT_ODDS[1], *SLOT_ODDS[2:]) if index == 0 else SLOT_ODDS
                rarities = [r for r in range(1 if index == 0 else 0, 4) if pools[r]]
                rarity = rng.choices(rarities, weights=[odds[r] for r in rarities])[0]
                candidates = [g for g in pools[rarity] if g not in owned or self.items[g].get('currency')]
                guid = rng.choice(candidates or pools[rarity])
                item = self.items[guid]
                duplicate = guid in owned and not item.get('currency')
                credits = DUPLICATE_CREDITS[rarity] if duplicate else 0
                state['credits'] += item.get('currency', 0) + credits
                if not duplicate and not item.get('currency'):
                    owned.add(guid)
                result.append({'hero': item['hero'], 'item': guid, 'credits': credits, 'duplicate': duplicate,
                               'new': not duplicate and not item.get('currency')})
            rng.shuffle(result)  # the Rare-or-better slot is not always the first shown
            state['owned'] = sorted(owned)
            state['boxes'].remove(box)
            db.execute('INSERT INTO receipts VALUES (?,?,?)', ('box', box_id, json.dumps(result)))
            return result

    def purchase(self, guid):
        item = self.items.get(guid, {})
        if not item.get('price'):
            raise ValueError('item is not for sale')
        with self._change() as (_, state):
            if guid in state['owned']:
                return False
            if state['credits'] < item['price']:
                raise ValueError('insufficient credits')
            state['credits'] -= item['price']
            state['owned'].append(guid)
            return True

    def equip(self, hero, guid, index):
        item = self.items.get(guid, {})
        slot = item.get('slot')
        if (slot is None or (hero not in item.get('heroes', []) and item.get('hero') != 0)
                or type(index) is not int or not 0 <= index < (4 if slot in LIST_SLOTS else 1)):
            raise ValueError('item does not belong in this hero/slot')
        with self._change() as (_, state):
            if guid not in state['owned']:
                raise ValueError('item is not owned')
            if slot == 'Icon':
                state['icon'] = int(guid, 16)
                return
            state['equips'] = [e for e in state['equips'] if (e[0], e[2], e[3]) != (hero, index, slot)]
            state['equips'].append([hero, int(guid, 16), index, slot])
