"""The chat's hero switch line: which picks post it, the one controller frame that carries it, and the name the client
prints (research/lizrollin-runtime.md section 22)."""
import dataclasses
import unittest
from unittest import mock

import practice  # noqa: F401  (puts vendor/ on the path)
from ow174.game import heroselect, world
from ow174.game import match as M
from ow174.game.content import SOLDIER
from ow174.game.statescript import Asset, Entity
from ow174.game.world import OP_CREATE
from test_target import _match

REAPER = 0x02E0000000000002
SKIRMISH = 0x0230000000000010


def skirmish():
    match, player = _match()
    match.game_map = dataclasses.replace(match.game_map, mode_guid=SKIRMISH)
    match.controller = heroselect.PVP
    return match, player


def pick(match, player, hero, now=5.0):
    match.game_message(player, {'+0xA8': heroselect.PICK_HERO, '+0x80': hero.to_bytes(8, 'little')}, now)


def controller_frames(match, send):
    """The presence bits each controller frame of `send` carried beside the controller's own."""
    with mock.patch.object(heroselect, 'controller_frame', wraps=heroselect.controller_frame) as frame:
        send()
    return [call.args[5] for call in frame.call_args_list]


class SwitchLineTests(unittest.TestCase):
    def test_a_switch_in_a_0c90_mode_waits_for_its_line_and_a_first_pick_or_the_same_hero_does_not(self):
        match, player = skirmish()
        player.has_body = False
        pick(match, player, SOLDIER)  # the first pick
        self.assertIsNone(player.switch_notice)
        pick(match, player, SOLDIER)  # the same hero again
        self.assertIsNone(player.switch_notice)
        pick(match, player, REAPER)
        self.assertEqual(player.switch_notice, (SOLDIER, REAPER))

    def test_the_practice_range_has_no_line(self):
        match, player = _match()  # its controller is 13C1
        pick(match, player, REAPER)
        self.assertIsNone(player.switch_notice)

    def test_the_line_goes_in_one_frame_once_the_client_has_the_frames_before(self):
        match, player = skirmish()
        match.now = 10.0
        pick(match, player, REAPER)  # sends a plain controller frame that closes hero select
        self.assertEqual(controller_frames(match, lambda: match._send_switch_notice(player, 10.5)), [])  # in flight
        player.script.arrived(player.script.data_last)
        self.assertEqual(controller_frames(match, lambda: match._send_switch_notice(player, 10.05)), [])  # too soon
        line = {M.OLD_HERO_BIT: Asset(SOLDIER), M.NEW_HERO_BIT: Asset(REAPER), M.SWITCHER_BIT: Entity(player.entity)}
        self.assertEqual(controller_frames(match, lambda: match._send_switch_notice(player, 10.5)), [line])
        player.script.arrived(player.script.data_last)
        self.assertEqual(controller_frames(match, lambda: match._send_switch_notice(player, 11.0)), [])  # sent once

    def test_after_a_frame_with_a_line_a_plain_one_goes_before_the_next_line(self):
        match, player = skirmish()
        for now, waiting, frames in ((10.0, (SOLDIER, REAPER), 1), (10.5, (REAPER, SOLDIER), 1), (11.0, None, 1)):
            match.now = now
            if waiting:
                player.switch_notice = waiting
            player.script.arrived(player.script.data_last)
            sent = controller_frames(match, lambda: match._send_switch_notice(player, now))
            self.assertEqual(len(sent), frames)
            carried = bool(sent[0])
            self.assertEqual(carried, now != 10.5, now)  # line, plain, line

    def test_the_players_entity_carries_his_name_for_the_line(self):
        match, player = _match()
        create = next(u for u in match.spawn_updates(player) if (u.entity, u.op) == (player.entity, OP_CREATE))
        with mock.patch.object(world, 'create', wraps=world.create) as created:
            record = create.build(0)
        self.assertEqual(created.call_args.args[2][29], [None, 'Player'])  # m_battleTag
        self.assertIn(b'\x06\x00\x00\x00Player', record.getvalue())  # a teString: u32 length, then the bytes


if __name__ == '__main__':
    unittest.main()
