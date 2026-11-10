"""Compare our hand-written weapon tables (target.PLAYER_KIT, from the wiki) with the damage the game's own data gives:
every WeaponVolley of every hero, its hit definition evaluated by LizRollin's statescript VM after a second of idle
frames (combat.hit_def: ModifyHealth's amount, the falloff start / end / floor, the critical multiplier, an
explosion's amount and rings), plus the volley's shots per second, pellets per shot and projectile speed.
Their VM runs in its own process from their copy (both packages are named ow174), as in vm_shadow.py.

    python tools/research/hitdef_shadow.py [--hero NAME ...] [--lizrollin DIR] [--out REPORT.md] [--json FILE]

A difference is a lead: the data is 1.74's, our tables are the wiki's last OW1 revisions. The falloff and the
critical multiplier are read as their combat.py reads them (unverified there).
"""
import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
VOLLEY = 'STUStatescriptStateWeaponVolley'


def theirs(lizrollin: Path, names: set) -> dict:
    """{hero name: {'weapon': graph, 'volleys': [{graph, state, ...numbers}]}} or {'error': text}."""
    sys.path.insert(0, str(lizrollin))
    import logging
    logging.disable(logging.CRITICAL)
    from ow174.game.combat import _number, hit_def
    from ow174.game.script import graph
    from ow174.game.script.driver import BodyScript

    out = {}
    for record in graph.bodies().values():
        name = record.get('name')
        if names and name not in names:
            continue
        try:
            body = BodyScript(int(record['hero'], 16), 0x01000001, 1000)
            for frame in range(1001, 1061):  # a second of idle frames: the graphs' SetVars run
                body.command(frame, 0)
            volleys = []
            for instance in list(body.component.instances.values()):
                for node in instance.graph.nodes:
                    if node.cls != VOLLEY or node.state is None:
                        continue
                    state = instance.node_state(node)

                    def number(field, default=None):
                        cfg = node.field(field)
                        return default if cfg is None else _number(state.evaluate(cfg, watch=False), default)

                    entry = {'graph': instance.graph.index, 'state': node.state,
                             'rate': number('m_numShotsPerSecond'), 'pellets': number('m_numProjectilesPerShot'),
                             'speed': number('m_projectileSpeed'), 'reach': number('m_FEC435C6')}
                    for key, field in (('direct', 'm_6396149F'), ('blast', 'm_D2D11CE9')):
                        cfg = node.field(field)
                        found = hit_def(state, cfg)
                        if found is not None:  # an unset critical multiplier is None, not hit_def's 1.0
                            entry[key] = {'amount': found.amount, 'start': found.falloff_start,
                                          'end': found.falloff_end, 'floor': found.falloff_floor,
                                          'critical': found.critical if cfg.get('m_A872C70E') is not None else None,
                                          'rings': found.rings}
                    volleys.append(entry)
            out[name] = {'volleys': volleys}
        except Exception as error:  # a hero their VM cannot run is itself a finding
            out[name] = {'error': f'{type(error).__name__}: {error}'}
    return out


def ours() -> tuple[dict, dict]:
    """({hero name: Weapon}, {hero name: weapon graph}) from our tables."""
    sys.path.insert(0, str(REPO / 'tools' / 'lobby'))
    import practice  # noqa: F401  (puts vendor/ on the path)
    from kits import KITS
    from ow174.game.content import heroes
    from target import PLAYER_KIT
    names = {guid: hero.name for guid, hero in heroes().items()}
    return ({name: kit[1] for name, kit in PLAYER_KIT.items()},
            {names[guid]: kit.graph.index for guid, kit in KITS.items() if guid in names})


def fmt(v) -> str:
    if v is None:
        return '-'
    if isinstance(v, float):
        return f'{v:.4g}'
    return str(v)


def damaging(volley) -> bool:
    return any((volley.get(k) or {}).get('amount', 0) > 0 for k in ('direct', 'blast'))


def report(weapons, graphs, their) -> list[str]:
    lines = ['# Weapon damage: our tables against the 1.74 data', '',
             'Theirs: every damaging WeaponVolley in the hero\'s weapon graph (`graph.state`), evaluated by '
             'LizRollin\'s VM. Damage `near -> far` over `start-end` m (far = amount x floor); `crit` the '
             'critical multiplier; `blast` an explosion\'s amount and its rings (radius: scale).', '']
    for name in sorted(set(weapons) | set(their)):
        ours_w, data = weapons.get(name), their.get(name, {})
        lines.append(f'## {name}')
        if ours_w is not None:
            fall = f'{ours_w.falloff[0]}-{ours_w.falloff[1]} m' if ours_w.falloff else 'none'
            lines.append(f'- ours: {fmt(ours_w.damage)} -> {fmt(ours_w.far_damage)} over {fall}, '
                         f'{fmt(1 / ours_w.shot_seconds)} shots/s, pellets {ours_w.pellets}, '
                         f'speed {fmt(ours_w.speed)}')
        if 'error' in data:
            lines.append(f'- theirs: {data["error"]}')
        weapon = graphs.get(name)
        for volley in data.get('volleys', []):
            if not damaging(volley):
                continue
            tag = 'weapon' if volley['graph'] == weapon else 'other'
            parts = [f'{volley["graph"]:04X}.{volley["state"]} ({tag})']
            direct = volley.get('direct') or {}
            if direct.get('amount'):
                floor = direct.get('floor')
                far = direct['amount'] * floor if floor is not None else None
                span = (f' over {fmt(direct["start"])}-{fmt(direct["end"])} m'
                        if direct.get('start') is not None else '')
                parts.append(f'{fmt(direct["amount"])} -> {fmt(far)}{span}, crit {fmt(direct.get("critical"))}')
            blast = volley.get('blast') or {}
            if blast.get('amount'):
                rings = ', '.join(f'{fmt(r)}: {fmt(s)}' for r, s in blast.get('rings') or ())
                parts.append(f'blast {fmt(blast["amount"])} ({rings})')
            parts.append(f'{fmt(volley.get("rate"))} shots/s, pellets {fmt(volley.get("pellets"))}, '
                         f'speed {fmt(volley.get("speed"))}, reach {fmt(volley.get("reach"))}')
            lines.append('- theirs ' + '; '.join(parts))
        lines.append('')
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--hero', action='append', default=[], help="a hero's name; repeat for more (default: all)")
    parser.add_argument('--lizrollin', type=Path, default=REPO / 'lizrollin-lobby-server')
    parser.add_argument('--out', type=Path)
    parser.add_argument('--json', type=Path, help='also write their raw numbers here')
    parser.add_argument('--emit-theirs', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.emit_theirs:
        print(json.dumps(theirs(args.lizrollin, set(args.hero))))
        return
    weapons, graphs = ours()
    command = [sys.executable, '-B', __file__, '--emit-theirs', '--lizrollin', str(args.lizrollin)]
    for name in args.hero:
        command += ['--hero', name]
    their = json.loads(subprocess.run(command, capture_output=True, text=True, check=True).stdout)
    if args.json:
        args.json.write_text(json.dumps(their, indent=1), encoding='utf-8', newline='\n')
    if args.hero:
        weapons = {k: v for k, v in weapons.items() if k in args.hero}
    text = '\n'.join(report(weapons, graphs, their)) + '\n'
    if args.out:
        args.out.write_text(text, encoding='utf-8', newline='\n')
    print(text)


if __name__ == '__main__':
    main()
