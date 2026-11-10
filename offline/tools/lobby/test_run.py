import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import collision
import run


class LauncherTests(unittest.TestCase):
    def test_progression_is_explicit_and_forwarded_by_cli(self):
        for options, expected in (([], False), (['--progression'], True)):
            with self.subTest(options=options), patch.object(run.sys, 'argv', ['run.py', 'game-root', *options]), \
                    patch.object(run, 'launch') as launch:
                run.main()
                self.assertEqual(launch.call_args.kwargs['progression'], expected)

    def test_profile_name_option_defaults_and_forwards_custom_name(self):
        for options, name in (([], 'Player'), (['--name', 'Ranger7'], 'Ranger7'),
                              (['--name', 'A'], 'A'), (['--name', '012345678901'], '012345678901')):
            with self.subTest(name=name), patch.object(run.sys, 'argv', ['run.py', 'game-root', *options]), \
                    patch.object(run, 'launch') as launch:
                run.main()
                self.assertEqual(launch.call_args.kwargs['name'], name)

    def test_invalid_profile_name_fails_before_client_or_socket_access(self):
        for name in ('', 'A' * 13, 'Two Words', 'Name\0Bad', 'Name\n', 'J\u00f6rg', 'Name#1234', '\u212a'):
            with self.subTest(name=name), \
                    patch.object(run, 'check_client', side_effect=AssertionError('invalid name reached client')) as check, \
                    patch.object(run.socket, 'socket', side_effect=AssertionError('invalid name reached socket')) as socket:
                with self.assertRaisesRegex(ValueError, '1..12 ASCII letters or digits'):
                    run.launch(Path('game-root'), None, name=name)
                check.assert_not_called()
                socket.assert_not_called()

    def test_custom_name_is_passed_to_owned_lobby_process(self):
        import practice
        with tempfile.TemporaryDirectory() as directory:
            tool = Path(directory) / 'tools' / 'lobby'
            data = tool / 'data'
            data.mkdir(parents=True)
            (data / '0688.collision').write_bytes(collision.MAGIC)
            (data / 'hero_content.json').write_text('{"defaults": {}}')
            with patch.object(run, '__file__', str(tool / 'run.py')), \
                    patch.object(run, 'check_client'), patch.object(run.socket, 'socket'), \
                    patch.object(collision, 'build_movement'), \
                    patch.object(practice.PracticeRange, 'pve_handoff', False), patch('nav.for_map'), \
                    patch('target.patrol_points', return_value=[]), \
                    patch.object(run.subprocess, 'Popen', side_effect=RuntimeError('stop before launch')) as start:
                with self.assertRaisesRegex(RuntimeError, 'stop before launch'):
                    run.launch(Path(directory), None, name='Ranger7')
            self.assertEqual(start.call_args.args[0][-2:], ['--name', 'Ranger7'])

    def test_game_gets_a_user_folder_inside_the_install(self):
        with tempfile.TemporaryDirectory() as directory:
            root, documents = Path(directory, 'install'), Path(directory, 'home', 'Docs')
            source = documents / 'Overwatch' / 'Settings' / 'Settings_v0.ini'
            source.parent.mkdir(parents=True)
            source.write_text('[Render.13]')
            ours = root / 'OfflineProfile' / 'Docs' / 'Overwatch' / 'Settings' / 'Settings_v0.ini'
            with patch.dict(run.os.environ, {'USERPROFILE': str(documents.parent)}), \
                    patch.object(run, 'documents_value', return_value=run.HOME + 'Docs'):
                env = run.game_env(root)
                self.assertEqual(env['USERPROFILE'], str(root.resolve() / 'OfflineProfile'))
                self.assertEqual(ours.read_text(), '[Render.13]')  # the first run starts from the settings it had
                ours.write_text('changed')
                run.game_env(root)
                self.assertEqual(ours.read_text(), 'changed')  # later runs keep the game's own file
            with patch.object(run, 'documents_value', return_value=str(documents)):
                self.assertIsNone(run.game_env(root))  # Documents moved off %USERPROFILE%: nothing to redirect

    def test_default_menu_uses_normal_backdrops(self):
        with patch.object(run.sys, 'argv', ['run.py', 'game-root']), patch.object(run, 'launch') as launch:
            run.main()
        self.assertEqual(launch.call_args.kwargs['event'], 'none')

    def test_failed_marker_extraction_leaves_no_cache_and_can_retry(self):
        import practice
        from ow174.game.content import PRACTICE_RANGE
        with tempfile.TemporaryDirectory() as directory:
            tool = Path(directory) / 'tools' / 'lobby'
            data = tool / 'data'
            data.mkdir(parents=True)
            for index in {0x688} | {guid & 0xFFFFFFFF for guid, *_ in practice.PVE_CARDS.values()}:
                (data / f'{index:04X}.collision').write_bytes(collision.MAGIC)
            markers = data / 'pve_markers.json'

            def extract(*args, stdout, **kwargs):
                stdout.write('{partial')
                raise run.subprocess.CalledProcessError(1, args[0])

            with patch.object(run, '__file__', str(tool / 'run.py')), \
                    patch.object(run, 'check_client'), patch.object(run.socket, 'socket'), \
                    patch.object(collision, 'build_movement'), \
                    patch.object(practice.PracticeRange, 'pve_handoff', True), \
                    patch.object(run.subprocess, 'run', side_effect=extract) as extractor, \
                    patch.object(run.subprocess, 'Popen') as start:
                with self.assertRaises(run.subprocess.CalledProcessError):
                    run.launch(Path(directory), None)
                self.assertFalse(markers.exists())
                self.assertEqual(sorted(p.name for p in data.iterdir()),
                                 sorted(p.name for p in data.glob('*.collision')))
                extractor.side_effect = lambda *args, stdout, **kwargs: stdout.write('{partial')
                with self.assertRaises(run.json.JSONDecodeError):
                    run.launch(Path(directory), None)
                self.assertFalse(markers.exists())
                self.assertFalse(markers.with_suffix('.tmp').exists())
                extractor.side_effect = lambda *args, stdout, **kwargs: stdout.write('{}')
                with patch.object(practice.PracticeRange, 'pve_map', return_value=PRACTICE_RANGE), \
                        patch('nav.for_map'), \
                        patch.object(run.subprocess, 'Popen', side_effect=RuntimeError('stop before launch')):
                    (data / 'hero_content.json').write_text('{"defaults": {}}')
                    with self.assertRaisesRegex(RuntimeError, 'stop before launch'):
                        run.launch(Path(directory), None)
                self.assertEqual(markers.read_text(), '{}')
                self.assertEqual(extractor.call_count, 3)
                start.assert_not_called()

    def test_server_failure_ends_wait_without_waiting_for_game_timeout(self):
        game, server = Mock(), Mock()
        server.poll.return_value = 1
        with self.assertRaisesRegex(RuntimeError, 'Lobby server exited'):
            run.wait_for_game(game, server, 600)
        game.wait.assert_not_called()

    def test_no_timeout_waits_for_game_exit(self):
        game, server = Mock(), Mock()
        server.poll.return_value = None
        game.wait.side_effect = [run.subprocess.TimeoutExpired('game', 0.25)] * 3 + [0]
        self.assertEqual(run.wait_for_game(game, server), 0)
        self.assertEqual(game.wait.call_count, 4)

    def test_timeout_still_ends_wait(self):
        game, server = Mock(), Mock()
        server.poll.return_value = None
        game.wait.side_effect = run.subprocess.TimeoutExpired('game', 0.25)
        with self.assertRaises(run.subprocess.TimeoutExpired):
            run.wait_for_game(game, server, 0.01)

    def test_rejects_missing_and_wrong_client_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(FileNotFoundError):
                run.check_client(root)
            (root / '_retail_').mkdir()
            (root / '_retail_' / 'Overwatch.exe').write_bytes(b'unsupported client')
            with self.assertRaisesRegex(ValueError, '104319'):
                run.check_client(root)


if __name__ == '__main__':
    unittest.main()
