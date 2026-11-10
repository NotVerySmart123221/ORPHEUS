import os
import math
import sys
import unittest
from pathlib import Path

from range_bot_hitboxes import envelope, pose, point, sample


class AnimatedHitboxTests(unittest.TestCase):
    def test_parent_uniform_scale_moves_child_and_local_point(self):
        skeleton = [dict(id=1, parent=-1, pos=(0, 1, 0), rot=(0, 0, 0, 1), scale=(2, 2, 2)),
                    dict(id=2, parent=0, pos=(1, 0, 0), rot=(0, 0, 0, 1), scale=(1, 1, 1))]
        result = pose(skeleton, {'bones': {}}, {'bones': {}}, 0)
        self.assertEqual(point(result[2], (1, 0, 0)), (4, 1, 0))

    def test_additive_translation_uses_base_scale_before_delta_scale(self):
        skeleton = [dict(id=1, parent=-1, pos=(0, 0, 0), rot=(0, 0, 0, 1), scale=(2, 2, 2))]
        extra = {'bones': {1: {'pos': {0: (1, 0, 0)}, 'scale': {0: (3, 3, 3)}}}}
        result = pose(skeleton, {'bones': {}}, extra, 0)
        self.assertEqual(point(result[1], (1, 0, 0)), (8, 0, 0))

    def test_child_and_additive_translation_follow_parent_rotation(self):
        q = (0, 0, math.sqrt(.5), math.sqrt(.5))
        skeleton = [dict(id=1, parent=-1, pos=(0, 1, 0), rot=q, scale=(1, 1, 1)),
                    dict(id=2, parent=0, pos=(1, 0, 0), rot=(0, 0, 0, 1), scale=(1, 1, 1))]
        base = {'bones': {}}
        additive = {'bones': {2: {'pos': {0: (1, 0, 0)}}}}
        p = pose(skeleton, base, additive, 0)
        for actual, wanted in zip(point(p[2], (1, 0, 0)), (0, 4, 0)):
            self.assertAlmostEqual(actual, wanted)

    def test_rotation_interpolation_uses_short_arc(self):
        q = (0, 0, math.sqrt(.5), math.sqrt(.5))
        actual = sample({0: q, 2: tuple(-v for v in q)}, (0, 0, 0, 1), 1, True)
        self.assertAlmostEqual(abs(sum(a*b for a,b in zip(actual, q))), 1)

    def test_envelope_covers_both_capsule_ends_through_the_loop(self):
        poses = [[((0, 0, 0), (1, 0, 0), .1, True)],
                 [((0, .08, 0), (1, -.06, 0), .1, True)]]
        a, b, radius, critical = envelope(poses)[0]
        self.assertTrue(critical)
        for frame in poses:
            x, y, r, _ = frame[0]
            self.assertLessEqual(math.dist(a, x) + r, radius)
            self.assertLessEqual(math.dist(b, y) + r, radius)
        self.assertLess(radius, .15)

    @unittest.skipUnless(Path(os.environ.get('OW174_GAME', 'C:/Games/Overwatch')).is_dir(), 'needs retail assets')
    def test_server_envelope_covers_retail_motion_between_extraction_samples(self):
        from anim_export import Assets, parse_animation, parse_skeleton
        from model_hitboxes import chunks, hitboxes
        from range_bot_hitboxes import extract, matrices
        sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'lobby'))
        from hitbox import TRAINING_BOT
        assets = Assets(os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
        try:
            for generated, served in zip(extract(assets), TRAINING_BOT, strict=True):
                self.assertEqual(generated[3], served[3])
                for a, b in zip((*generated[0], *generated[1], generated[2]),
                                (*served[0], *served[1], served[2])):
                    self.assertAlmostEqual(a, b, delta=.000001)
            model = assets.get(0x0D0000000000312E)
            skeleton = parse_skeleton(chunks(model)['mskl'])
            base = parse_animation(assets.get(0x0A00000000003952))
            breath = parse_animation(assets.get(0x0A00000000003950))
            for f in range(8*(breath['frames']-1)):
                transforms = pose(skeleton, base, breath, (f+.5)/8)
                scale = max(1, *(v[2] for v in transforms.values()))
                for (a, b, radius, _), (x, y, swept, _) in zip(
                        hitboxes(model, matrices(transforms)), TRAINING_BOT, strict=True):
                    self.assertLessEqual(max(math.dist(a, x), math.dist(b, y))+radius*scale, swept)
        finally:
            assets.close()

    @unittest.skipUnless(Path(os.environ.get('OW174_GAME', 'C:/Games/Overwatch')).is_dir(), 'needs retail assets')
    def test_pose_matches_independently_read_native_hardpoints(self):
        from anim_export import Assets, hardpoints, parse_animation, parse_skeleton
        from model_hitboxes import chunks
        assets = Assets(os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
        try:
            model = chunks(assets.get(0x0D0000000000312E))
            transforms = pose(parse_skeleton(model['mskl']),
                parse_animation(assets.get(0x0A00000000003952)),
                parse_animation(assets.get(0x0A00000000003950)), 63)
            native = {1: (53.221123, 2.265038, 34.260704), 5: (53.739895, 2.155902, 34.056446),
                      7: (52.654064, 2.164487, 34.232456), 18: (53.229866, 2.789001, 34.300873),
                      29: (52.802189, 2.343049, 34.331387), 30: (53.640209, 2.340308, 34.187996)}
            yaw = math.atan2(54.56507-53.2231285, 42.14788-34.2612335)
            s, c = math.sin(yaw), math.cos(yaw)
            errors = []
            for hp, (x, y, z) in native.items():
                bone, offset, _ = hardpoints(model)[0x0DC0000000000000 | hp]
                actual = point(transforms[bone], offset)
                local = (c*(x-53.2231285)-s*(z-34.2612335), y-1,
                         s*(x-53.2231285)+c*(z-34.2612335))
                errors.append(math.dist(actual, local))
            self.assertLess(max(errors), .005)
            self.assertLess(math.sqrt(sum(e*e for e in errors)/len(errors)), .003)
        finally:
            assets.close()
