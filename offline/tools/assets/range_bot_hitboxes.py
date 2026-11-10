"""Print Range bot hit volumes covering idle 3952 frame 0 + the complete additive breathing loop 3950.

The native client chooses its own additive phase. Sweep that small motion instead of guessing its clock.
Uses hash-verified retail assets through anim_export.Assets; no game files are changed.

    python tools/assets/range_bot_hitboxes.py
"""
import os
import argparse
import bisect
import math

from anim_export import Assets, parse_animation, parse_skeleton
from model_hitboxes import chunks, hitboxes


def mul(a, b):
    x, y, z, w = a
    X, Y, Z, W = b
    return (w*X+x*W+y*Z-z*Y, w*Y-x*Z+y*W+z*X, w*Z+x*Y-y*X+z*W, w*W-x*X-y*Y-z*Z)


def rotate(q, v):
    x, y, z, w = q
    a, b, c = v
    tx, ty, tz = 2*(y*c-z*b), 2*(z*a-x*c), 2*(x*b-y*a)
    return (a+w*tx+y*tz-z*ty, b+w*ty+z*tx-x*tz, c+w*tz+x*ty-y*tx)


def point(transform, v):
    p, q, *scale = transform
    return tuple(a+b*(scale[0] if scale else 1) for a, b in zip(p, rotate(q, v)))


def sample(keys, rest, frame, rotation=False):
    if not keys:
        return rest
    frames = sorted(keys)
    i = bisect.bisect_right(frames, frame)
    if not i:
        return keys[frames[0]]
    if i == len(frames):
        return keys[frames[-1]]
    a, b = keys[frames[i-1]], keys[frames[i]]
    t = (frame-frames[i-1])/(frames[i]-frames[i-1])
    if rotation:
        dot = sum(x*y for x, y in zip(a, b))
        if dot < 0:
            b, dot = tuple(-x for x in b), -dot
        if dot < .9995:
            theta = math.acos(max(-1, min(1, dot)))
            return tuple((math.sin((1-t)*theta)*x+math.sin(t*theta)*y)/math.sin(theta)
                         for x, y in zip(a, b))
    out = tuple(x+(y-x)*t for x, y in zip(a, b))
    if rotation:
        length = math.sqrt(sum(x*x for x in out))
        out = tuple(x/length for x in out)
    return out


def pose(skeleton, base, additive, frame):
    out = []
    for bone in skeleton:
        track = base['bones'].get(bone['id'], {})
        p = sample(track.get('pos', {}), bone['pos'], 0)
        q = sample(track.get('rot', {}), bone['rot'], 0, True)
        extra = additive['bones'].get(bone['id'], {})
        base_scale = sample(track.get('scale', {}), bone['scale'], 0)
        extra_scale = sample(extra.get('scale', {}), (1, 1, 1), frame)
        if any(max(s)-min(s) > 1e-6 for s in (base_scale, extra_scale)):
            raise ValueError('nonuniform bone scale requires ellipsoid hit volumes')
        scale = base_scale[0]*extra_scale[0]
        p = point((p, q, base_scale[0]), sample(extra.get('pos', {}), (0, 0, 0), frame))
        q = mul(q, sample(extra.get('rot', {}), (0, 0, 0, 1), frame, True))
        parent = bone['parent']
        if not -1 <= parent < len(out):
            raise ValueError('skeleton must be in parent-first order')
        out.append((p, q, scale) if parent < 0 else
                   (point(out[parent], p), mul(out[parent][1], q), out[parent][2]*scale))
    return {bone['id']: value for bone, value in zip(skeleton, out)}


def matrices(transforms):
    return {bone: (*rotate(q, (scale, 0, 0)), 0, *rotate(q, (0, scale, 0)), 0,
                   *rotate(q, (0, 0, scale)), 0, *p, 1) for bone, (p, q, scale) in transforms.items()}


def envelope(frames):
    out = []
    for volumes in zip(*frames):
        a, b = (tuple((min(v[end][axis] for v in volumes)+max(v[end][axis] for v in volumes))/2
                       for axis in range(3)) for end in (0, 1))
        radius, critical = volumes[0][2:]
        assert all(v[2:] == (radius, critical) for v in volumes)
        drift = max(max(math.dist(a, v[0]), math.dist(b, v[1])) for v in volumes)
        # 2 mm covers sample interpolation/rounding; live native hardpoints agreed within 2 mm RMS.
        out.append((a, b, radius+drift+.002, critical))
    return out


def extract(assets):
    model = assets.get(0x0D0000000000312E)
    skeleton = parse_skeleton(chunks(model)['mskl'])
    base = parse_animation(assets.get(0x0A00000000003952))
    breath = parse_animation(assets.get(0x0A00000000003950))
    assert not base['priority'] & 4 and breath['priority'] & 4
    frames = []
    for f in range(4*(breath['frames']-1)+1):
        transforms = pose(skeleton, base, breath, f/4)
        # The gun bones scale by 1.000052. Bound radius scaling as well as transformed endpoints.
        scale = max(1, *(v[2] for v in transforms.values()))
        frames.append([(a, b, r*scale, critical)
                       for a, b, r, critical in hitboxes(model, matrices(transforms))])
    return envelope(frames)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-install', default=os.environ.get('OW174_GAME', 'C:/Games/Overwatch'))
    args = parser.parse_args()
    assets = Assets(args.game_install)
    try:
        for a, b, radius, critical in extract(assets):
            print(f'    ({tuple(round(x, 6) for x in a)}, {tuple(round(x, 6) for x in b)}, {math.ceil(radius*1e6)/1e6}, {critical}),')
    finally:
        assets.close()
