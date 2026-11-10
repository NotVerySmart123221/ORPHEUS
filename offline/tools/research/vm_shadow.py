"""Shadow-run LizRollin's statescript VM beside our hand-mapped kits: for each hero's weapon graph, the states each
side has on and the variables each sends, in four phases (idle, firing, released, reloading). Their VM runs in its
own process from their copy (both packages are named ow174); ours is kits.frame_builder read back by the vendored
decoder, as the client reads it.

    python tools/research/vm_shadow.py [--hero NAME ...] [--lizrollin DIR] [--out REPORT.md]

A difference is a lead, not a verdict: their VM is verified live only for Soldier: 76, and our frames are what the
client was seen to accept. Their phases: 1 s idle, 20 frames of fire, 10 released, then fire held until the ammo
var (53) is 0 and 5 frames more (16 ms frames).
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
FIRE = 0x01
PHASES = ('idle', 'firing', 'released', 'reloading')
AMMO = 53  # the weapon instance's ammo


def value(v) -> str:
    """A variable's value as text both sides agree on."""
    if isinstance(v, tuple) and len(v) == 2 and isinstance(v[1], tuple):
        v = v[0]  # (value, bindings)
    if isinstance(v, bool):
        return str(v)
    if isinstance(v, float):
        return f'{v:.4g}'
    return repr(v)


def theirs(weapons: dict[int, int], lizrollin: Path) -> dict:
    """{hero: {phase: {'states': {state: class}, 'vars': {var: text}}} or {'error': text}} from their VM."""
    sys.path.insert(0, str(lizrollin))
    import logging
    logging.disable(logging.CRITICAL)
    from ow174.game.script.driver import BodyScript

    out = {}
    for hero, weapon in weapons.items():
        try:
            body = BodyScript(hero, 0x01000001, 1000)
            frame = 1000

            def run(count, buttons=0):
                nonlocal frame
                for _ in range(count):
                    frame += 1
                    body.command(frame, buttons)

            def phase():
                snap = body.snapshot()
                own = [i for i, (g, _) in snap.instances.items() if g == weapon]
                if not own:
                    return {'states': {}, 'vars': {}, 'missing': True}
                return {'states': {s: cls for (i, s), (cls, on, _) in snap.states.items() if i == own[0] and on},
                        'vars': {var: value(v) for (i, var), v in snap.vars.items() if i == own[0]}}

            result = {}
            run(60)
            result['idle'] = phase()
            run(20, FIRE)
            result['firing'] = phase()
            run(10)
            result['released'] = phase()
            for _ in range(600):
                run(1, FIRE)
                if phase()['vars'].get(AMMO) in ('0', '0.0'):
                    break
            run(5)
            result['reloading'] = phase()
            out[hero] = result
        except Exception as error:  # a hero their VM cannot run is itself a finding
            out[hero] = {'error': f'{type(error).__name__}: {error}'}
    return out


def ours() -> tuple[dict, dict, dict]:
    """({hero: {phase: {...}}}, {hero: weapon graph}, {hero: name}) from our kits."""
    sys.path.insert(0, str(REPO / 'tools' / 'lobby'))
    import practice  # noqa: F401  (puts vendor/ on the path)
    import oracle
    from kits import KITS, frame_builder
    from ow174.game.content import heroes
    from soldier import RifleUpdate
    from target import PLAYER_KIT

    names = {guid: hero.name for guid, hero in heroes().items()}
    out, weapons = {}, {}
    for hero, kit in KITS.items():
        weapons[hero] = kit.graph.index
        updates = {'idle': None, 'firing': RifleUpdate(kit.capacity - 1, 1, True, True),
                   'released': RifleUpdate(kit.capacity - 1, 2, False, True),
                   'reloading': RifleUpdate(0, 3, False, True, True)}
        result = {}
        for label, update in updates.items():
            frame = oracle.read_full(frame_builder(kit, PLAYER_KIT[names[hero]][1].reload_seconds)(1, update))
            instance = frame.instances[kit.instance]
            result[label] = {'states': {s: None for s, (on, _) in instance.states.items() if on},
                             'vars': {var: value(v) for var, v in instance.vars.items()}}
        out[hero] = result
    return out, weapons, names


def classes() -> dict:
    """{(graph, state): class} from the vendored catalog."""
    import gzip
    path = REPO / 'tools' / 'lobby' / 'vendor' / 'data' / 'game_graphs_174.json.gz'
    catalog = json.load(gzip.open(path, 'rt', encoding='utf-8'))
    return {(g['index'], n['state']): n['class'] for g in catalog['graphs'] for n in g['nodes']
            if n.get('state') is not None}


def report(our, their, weapons, names, wanted) -> list[str]:
    """A summary (states only we have on, variables both set but to different values), then every difference."""
    kinds = classes()
    summary = ['| Hero | Weapon graph | States on only in ours (phases) | Variables both set, ours / theirs (phase) |',
               '|---|---|---|---|']
    full = ['| Hero | Weapon graph | Phase | Only ours (states) | Only theirs (states) | Variables that differ |',
            '|---|---|---|---|---|---|']
    totals = {'ours': 0, 'theirs': 0, 'vars': 0, 'both': 0}
    for hero in sorted(our, key=lambda h: names.get(h, '')):
        name = names.get(hero, f'{hero:#x}')
        if wanted and name not in wanted:
            continue
        graph = weapons[hero]
        fmt = lambda states: ', '.join(f'{s} {kinds.get((graph, s), "?").replace("STUStatescriptState", "")}'
                                       for s in states)
        other = their.get(str(hero)) or their.get(hero)
        if other is None or 'error' in other:
            summary.append(f'| {name} | {graph:#06x} | their VM: {(other or {}).get("error", "not run")} | |')
            continue
        ours_only, both = {}, []
        for phase in PHASES:
            a, b = our[hero][phase], other[phase]
            if b.get('missing'):
                summary.append(f'| {name} | {graph:#06x} | their VM has no instance of this graph | |')
                break
            sa, sb = set(a['states']), {int(s) for s in b['states']}
            only_a, only_b = sorted(sa - sb), sorted(sb - sa)
            va, vb = a['vars'], {int(k): v for k, v in b['vars'].items()}
            diff = sorted(k for k in va.keys() | vb.keys() if va.get(k) != vb.get(k))
            for state in only_a:
                ours_only.setdefault(state, []).append(phase)
            both += [f'{k}: {va[k]} / {vb[k]} ({phase})' for k in diff if k in va and k in vb]
            totals['ours'] += len(only_a)
            totals['theirs'] += len(only_b)
            totals['vars'] += len(diff)
            if only_a or only_b or diff:
                vars_text = ', '.join(f'{k}: {va.get(k, "-")} / {vb.get(k, "-")}' for k in diff)
                full.append(f'| {name} | {graph:#06x} | {phase} | {fmt(only_a)} | {fmt(only_b)} | {vars_text} |')
        else:
            totals['both'] += len(both)
            states = ', '.join(f'{fmt([s])} ({"/".join(p)})' for s, p in sorted(ours_only.items()))
            summary.append(f'| {name} | {graph:#06x} | {states} | {", ".join(both)} |')
    return (summary + ['', f'Totals: {totals["ours"]} states on only in ours, {totals["theirs"]} only in theirs, '
                           f'{totals["vars"]} variables that differ, {totals["both"]} of them set on both sides.',
                       '', '## Every difference', ''] + full)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--hero', action='append', default=[], help="a hero's name; repeat for more (default: all)")
    parser.add_argument('--lizrollin', type=Path, default=REPO / 'lizrollin-lobby-server')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--emit-theirs', help=argparse.SUPPRESS)  # JSON {hero: weapon graph}: run their side
    args = parser.parse_args()
    if args.emit_theirs:
        weapons = {int(k): v for k, v in json.loads(args.emit_theirs).items()}
        print(json.dumps(theirs(weapons, args.lizrollin)))
        return
    our, weapons, names = ours()
    run = subprocess.run([sys.executable, '-B', __file__, '--emit-theirs', json.dumps(weapons),
                          '--lizrollin', str(args.lizrollin)], capture_output=True, text=True, check=True)
    their = json.loads(run.stdout)
    lines = report(our, their, weapons, names, set(args.hero))
    text = '\n'.join(lines) + '\n'
    if args.out:
        args.out.write_text(text, encoding='utf-8', newline='\n')
    print(text)


if __name__ == '__main__':
    main()
