"""Launch the verified retail menu with an owned loopback server and exit timer."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time

CLIENT_SHA256 = 'ef67d348ecca066ce1698683fd0fd810075357164b345d83ed9296d676e0ebe2'
SHELL_FOLDERS = r'Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders'
USER_FOLDER = 'OfflineProfile'  # in the game install
HOME = '%USERPROFILE%' + os.sep


def check_client(root):
    exe = root.resolve() / '_retail_' / 'Overwatch.exe'
    with exe.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    if digest != CLIENT_SHA256:
        raise ValueError('Unsupported executable; requires the verified retail build 104319')
    return exe


def documents_value():
    """The user's Documents folder as the registry holds it, unexpanded (by default %USERPROFILE%, then Documents)."""
    import winreg
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, SHELL_FOLDERS) as key:
        return winreg.QueryValueEx(key, 'Personal')[0]


def game_env(root):
    """The game's environment with a user folder of its own inside the install, or None to leave it alone.
    The client keeps Settings_v0.ini, its logs, screenshots and shader cache in the folder Overwatch under Documents
    (SHGetFolderPathW with CSIDL_PERSONAL, 0x7FF6D2A00620), which Overwatch 2 writes too. Windows expands the
    Documents path with the asking process's own USERPROFILE, so a USERPROFILE inside the install moves all of it
    to OfflineProfile/Documents/Overwatch there. The first run copies the settings file the client had."""
    documents = documents_value()
    if not documents.upper().startswith(HOME):
        print(f'Documents is {documents}: not under %USERPROFILE%, so the game keeps its settings there.', flush=True)
        return None
    profile = root.resolve() / USER_FOLDER
    settings = Path('Overwatch', 'Settings', 'Settings_v0.ini')
    ours, theirs = profile / documents[len(HOME):] / settings, Path(os.path.expandvars(documents)) / settings
    if not ours.exists():
        ours.parent.mkdir(parents=True, exist_ok=True)
        if theirs.exists():
            shutil.copy2(theirs, ours)
    return {**os.environ, 'USERPROFILE': str(profile)}


# shortcut: a fixed wait; the client's "Entering game" step still reads the tournament flag a few seconds after
# the login records (research/play-menu.md). Upgrade to the lobby message that ends that step.
LOGIN_SETTLE = 10


def wait_for_game(game, server, timeout=None, lobby_log=None, go_files=()):
    """Wait for the game to exit, or for timeout seconds when given. LOGIN_SETTLE seconds after the lobby has
    logged the player in, create go_files: the ASI plugins that must not act during login wait for theirs (tools/asi)."""
    deadline, logged_in = time.monotonic() + timeout if timeout else None, None
    while True:
        if go_files and logged_in is None and 'Sent login/player/content records' in lobby_log.read_text(encoding='utf-8'):
            logged_in = time.monotonic()
        if go_files and logged_in is not None and time.monotonic() - logged_in >= LOGIN_SETTLE:
            for go in go_files:
                go.touch()
            go_files = ()
        if server.poll() is not None:
            raise RuntimeError('Lobby server exited during the game; inspect this run\'s lobby.log')
        remaining = deadline - time.monotonic() if deadline else 0.25
        if remaining <= 0:
            raise subprocess.TimeoutExpired(game.args, timeout)
        try:
            return game.wait(timeout=min(0.25, remaining))
        except subprocess.TimeoutExpired:
            pass


def launch(root, timeout, server_command=None, map_name=None, mode=None, difficulty='medium', plugins=(),
           event='none', name='Player', progression=False):
    from practice import profile_name
    name = profile_name(name)
    exe = check_client(root)
    # Refuse another service rather than reuse or stop an unowned process.
    with socket.socket() as probe:
        if hasattr(socket, 'SO_EXCLUSIVEADDRUSE'):
            probe.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        probe.bind(('127.0.0.1', 3724))
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        if hasattr(socket, 'SO_EXCLUSIVEADDRUSE'):
            probe.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        probe.bind(('127.0.0.1', 3730))
    tool = Path(__file__).resolve().parent
    index, game_map = 0x688, None  # the Practice Range
    lobby_args = []
    if map_name:
        from practice import pick_map
        game_map = pick_map(map_name, mode, event)
        index = game_map.map_guid & 0xFFFFFFFF
        lobby_args = ['--map', map_name, '--mode', mode, '--difficulty', difficulty]
    lobby_args += ['--event', event, '--game-install', str(root), '--name', name]
    if progression:
        lobby_args.append('--progression')
    if plugins:
        lobby_args.append('--retail-menu')  # shortcut: assumes the plugins include ow_fixes.asi, the only one
    from practice import PVE_CARDS, PracticeRange
    pve = {guid & 0xFFFFFFFF for guid, *_ in PVE_CARDS.values()} if PracticeRange.pve_handoff else set()
    import collision
    for each in {index} | pve:  # the maps' collision meshes, extracted once from this install (PvE: line of sight)
        mesh = tool / 'data' / f'{each:04X}.collision'
        if not collision.current(mesh):
            subprocess.run([sys.executable, str(tool.parent / 'assets' / 'map_collision.py'), str(mesh),
                '--map-index', hex(each), '--game-install', str(root)], check=True)
    collision.build_movement([0x0800000000000000 | each for each in {index} | pve], root)  # the player mover's
    markers = tool / 'data' / 'pve_markers.json'  # the PvE maps' omnic spawn markers (entity 0x16DC), extracted once
    if pve and not markers.exists():  # shortcut: one card, so one map per file; merge the outputs when a second comes
        (guid, mode, *_), = PVE_CARDS.values()
        pending = markers.with_suffix('.tmp')
        try:
            with pending.open('w', encoding='utf-8') as out:
                subprocess.run([sys.executable, str(tool.parent / 'assets' / 'map_markers.py'), f'{guid & 0xFFFFFFFF:X}',
                    hex(mode), '0x04000000000016DC', '--game-install', str(root)], stdout=out, check=True)
            json.loads(pending.read_text(encoding='utf-8'))
            pending.replace(markers)
        finally:
            pending.unlink(missing_ok=True)
    content = tool / 'data' / 'hero_content.json'  # each hero's skin themes, unlocks and defaults, extracted once
    if not content.exists() or '"defaults"' not in content.read_text(encoding='utf-8'):  # tables before defaults
        from practice import PracticeRange
        subprocess.run([sys.executable, str(tool.parent / 'assets' / 'hero_content.py'), str(content), '--game-install',
            str(root), *(f'{hero:X}' for hero in PracticeRange.menu_heroes())], check=True)
    if progression:
        catalog = tool / 'data' / 'progression_catalog.json'
        if not catalog.exists() or json.loads(catalog.read_text(encoding='utf-8')).get('version') != 2:
            subprocess.run([sys.executable, str(tool.parent / 'assets' / 'progression_catalog.py'), str(catalog),
                            '--game-install', str(root), '--heroes', str(content)], check=True)
        from progression import Account
        Account(catalog.parent / 'profiles' / f'{name}.sqlite3', json.loads(catalog.read_text(encoding='utf-8')))
    import nav
    for each in ([game_map] if game_map is not None else []) + [PracticeRange.pve_map(card) for card in PVE_CARDS if pve]:
        nav.for_map(each.map_guid, nav.nav_points(each))  # built once here, not in a match tick (King's Row: ~30 s)
    if game_map is None:  # the Range patrols' grid (~12 s)
        from target import patrol_points
        nav.for_map(0x0800000000000688, patrol_points())
    logdir =tool.parents[1] / 'research' / 'main-menu' / datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
    logdir.mkdir(parents=True)
    server = game = None
    try:
        with (logdir / 'lobby.log').open('w', encoding='utf-8') as log:
            server = subprocess.Popen(server_command or [sys.executable, '-u', str(tool / 'lobby.py'), *lobby_args],
                stdout=subprocess.DEVNULL, stderr=log,
                creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        for _ in range(50):
            if server.poll() is not None:
                raise RuntimeError(f'Lobby server exited; inspect {logdir}')
            try:
                with socket.create_connection(('127.0.0.1', 3724), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.1)
        else:
            raise TimeoutError('Loopback server did not become ready')
        game = subprocess.Popen([str(exe), '--tank_TournamentMode',
            '--lobbyServer=127.0.0.1:3724', '--tank_logpath=' + str(logdir)], cwd=exe.parent, env=game_env(root))
        print(f'Game PID {game.pid}; lobby PID {server.pid}; logs: {logdir}', flush=True)
        if os.environ.get('OW_CLIENT_SYNC') and not any(Path(p).stem == 'ow_fixes' for p in plugins):
            print('OW_CLIENT_SYNC: nothing feeds the client position without --asi tools/asi/build/ow_fixes.asi', flush=True)
        if plugins:
            sys.path.insert(0, str(tool.parent / 'asi'))
            from load_asi import load
            for plugin in plugins:
                Path(plugin).with_suffix('.go').unlink(missing_ok=True)  # a stale signal from an earlier run
                print(f'ASI {plugin}: {"loaded" if load(game.pid, plugin) else "FAILED"}', flush=True)
        print('ow_fixes.asi skips the Pod and name screens.' if plugins else
              'Use Pod 1, Start, then a local name and Enter Game.',
              f'Automatic close in {timeout}s.' if timeout else 'No time limit: exit from the game menu.', flush=True)
        try:
            go_files = [Path(plugin).with_suffix('.go') for plugin in plugins]
            result = wait_for_game(game, server, timeout, logdir / 'lobby.log', go_files)
            print(f'Game exit: {result}', flush=True)
            return result
        except subprocess.TimeoutExpired:
            print('Time limit reached; closing owned game and lobby processes.', flush=True)
            return 124
    finally:
        for process in (game, server):
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


def main():
    from practice import profile_name
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('game_root', type=Path)
    parser.add_argument('--timeout', type=int, help='automatic close after 10..3600 seconds (default: none)')
    parser.add_argument('--map', help='open this catalog map (name or GUID) from the Practice Range card')
    parser.add_argument('--mode', default='0x0230000000000010', help='game mode GUID for --map (default Skirmish)')
    parser.add_argument('--difficulty', choices=('easy', 'medium', 'hard'), default='medium', help='vs-AI bots (default medium)')
    parser.add_argument('--asi', action='append', default=[], metavar='PLUGIN',
                        help='load this ASI plugin into the game (opt-in, repeatable; tools/asi)')
    parser.add_argument('--event', default='none', help='main menu event (practice.MENU_EVENTS; default none: ordinary backdrops)')
    parser.add_argument('--name', type=profile_name, default='Player', help='profile name: 1..12 ASCII letters or digits (default Player)')
    parser.add_argument('--progression', action='store_true', help='earn XP, loot boxes and cosmetics in a separate persistent account')
    args = parser.parse_args()
    if args.timeout is not None and not 10 <= args.timeout <= 3600:
        parser.error('timeout must be 10..3600 seconds')
    try:
        return launch(args.game_root, args.timeout, map_name=args.map, mode=args.mode, difficulty=args.difficulty,
                      plugins=args.asi, event=args.event, name=args.name, progression=args.progression)
    except KeyboardInterrupt:
        return 130
    except (OSError, ValueError, RuntimeError, TimeoutError) as exc:
        parser.exit(1, f'{exc}\n')


if __name__ == '__main__':
    raise SystemExit(main())
