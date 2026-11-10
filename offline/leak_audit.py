"""Check that nothing git tracks or would add under offline/ holds local key material, data files or machine paths.

    python offline/leak_audit.py        exit code 1 when something is found

The key material is read from tools/lobby/vendor/data (README.md there): the state-blob signing input, the manifest
key table and every 16-byte key of the key files. Run it before committing changes under offline/.
"""
import gzip
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE / 'tools' / 'lobby' / 'vendor' / 'data'
BINARY_KEYS = ('lobby_state_blob.key', 'cmf_keytable_104319.bin')
KEY_TABLES = ('resource_keys_174.json', 'tact_keys_174.json', 'collision_keys_174.json')
DATA_SUFFIXES = {'.json', '.bin', '.key', '.sqlite3', '.col', '.collision', '.nav', '.png', '.jpg', '.webp', '.mp4',
                 '.log', '.dmp', '.dll', '.asi', '.exe'}
SLASH = '[/' + chr(92) * 2 + ']'
MACHINE_PATH = re.compile('[A-Z]:' + SLASH + '(Users|Claude|GPT|GitHub|SteamLibrary|IDA)' + SLASH)
HEX_RUN = re.compile('[0-9a-f]{16,}')


def secrets():
    found = set()
    for name in BINARY_KEYS:
        raw = (DATA / name).read_bytes()
        found |= {raw[i:i + 8].hex() for i in range(0, len(raw) - 7, 8)}
    for name in KEY_TABLES:
        found |= set(re.findall('[0-9a-f]{32}', (DATA / name).read_text(encoding='utf-8').lower()))
    return found


def holds_key(text, known):
    for run in HEX_RUN.findall(text.lower()):
        for size in (16, 32):
            if any(run[i:i + size] in known for i in range(0, len(run) - size + 1, 2)):
                return True
    return False


def candidates():
    listed = subprocess.run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '.'],
                            cwd=HERE, capture_output=True, check=True).stdout.decode('utf-8')
    return [HERE / name for name in listed.split('\0') if name and (HERE / name).is_file()]


def main():
    known = secrets()
    assert holds_key('x' + next(iter(known)) + 'x', known) and not holds_key('0123456789abcdef' * 4, set())
    problems = []
    files = candidates()
    for path in files:
        name = path.relative_to(HERE).as_posix()
        raw = path.read_bytes()
        if name.endswith('.gz'):
            raw = gzip.decompress(raw)
        elif path.suffix.lower() in DATA_SUFFIXES:
            problems.append(f'{name}: a data or binary file')
            continue
        text = raw.decode('utf-8', 'replace')
        if holds_key(text, known):
            problems.append(f'{name}: holds key material')
        if MACHINE_PATH.search(text):
            problems.append(f'{name}: holds a machine path')
    print(f'{len(files)} files checked against {len(known)} key fragments: {len(problems)} problems')
    for line in problems:
        print(' ', line)
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
