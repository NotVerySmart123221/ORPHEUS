"""Stop the offline suite with a clear message when its local inputs are not prepared (tools/lobby/vendor/data/README.md)."""
from pathlib import Path

import pytest

DATA = Path(__file__).resolve().parent / 'tools' / 'lobby' / 'vendor' / 'data'
REQUIRED = ('schemas_174.json', 'game_graphs_174.json.gz', 'game_heroes_174.json', 'statescript_codecs_174.json',
            'lobby_state_blob.key', 'cmf_keytable_104319.bin', 'tact_keys_174.json')
MISSING = [name for name in REQUIRED if not (DATA / name).is_file()]
if MISSING:
    # tools/lobby/conftest.py reads the data when it is imported, so stop before pytest gets there.
    pytest.exit(f'offline suite not run: {DATA} lacks {", ".join(MISSING)}', returncode=5)
