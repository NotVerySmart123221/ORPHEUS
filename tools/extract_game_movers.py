#!/usr/bin/env python3
"""
Write data/game_movers_174.json: each hero body's character mover values (STUCharacterMoverComponent,
type 0x6EFDBD73), which the server's mover (ow174/game/mover.py) runs the hero with.

Reads DataTool's STU dumps of the bodies (type 003, `extract-stu-type <out> 003 --xml`) for the bodies in
data/game_heroes_174.json and the training bots' bodies in data/ai_spawners_174.json.

    py tools/extract_game_movers.py <XML dump folder>\003

The dump names the fields by hash. Their meaning comes from where the client's mover reads them: it keeps
the component's values at mover +0x418 + the field's offset (TankLib's STU layout), for example the jump
speed at +0x49C (jump 0x7FF789B48880), the gravity scale at +0x4A0 (0x7FF789AD03A0), the speeds at
+0x460..+0x474 (wish 0x7FF789B44340), the capsule at +0x430..+0x444 (stance 0x7FF789B4C3D0).
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from ow174.game.mover import STU_MOVER_FIELDS  # noqa: E402

DATA_DIR = ROOT / "data"
COMPONENT = re.compile(r"<STUCharacterMoverComponent dragon:id=\"\d+\" dragon:name=\"dragon:value\"([^>]*)/>")
ATTRIBUTE = re.compile(r'm_([0-9A-F]{8})="([^"]*)"')

# TankLib's defaults for fields a dump may leave out.
DEFAULTS = {
    "ground_circle_radius": 0.25,
    "ground_circle_rays": 4.0,
    "accel_air_touching": 0.5,
    "character_collision": 1.0,
}


def mover_of(text: str) -> dict | None:
    match = COMPONENT.search(text)
    if match is None:
        return None
    values = dict(DEFAULTS)
    for field, value in ATTRIBUTE.findall(match.group(1)):
        if field in STU_MOVER_FIELDS:
            values[STU_MOVER_FIELDS[field]] = float(value.replace(",", "."))
    values["ground_circle_rays"] = int(values["ground_circle_rays"])
    values["character_collision"] = int(values["character_collision"])
    missing = [name for name in STU_MOVER_FIELDS.values() if name not in values]
    if missing:
        raise ValueError(f"missing {missing}")
    return values


def collect(folder: Path, hero_table: Path, spawners: Path) -> dict:
    heroes = list(json.loads(hero_table.read_text(encoding="utf-8")).values())
    if spawners.is_file():
        heroes += json.loads(spawners.read_text(encoding="utf-8"))["heroes"].values()
    table = {}
    for hero in heroes:
        body = int(hero["body"], 16)
        dump = folder / f"{body & 0xFFFFFFFFFFFF:012X}.003.xml"
        values = mover_of(dump.read_text(encoding="utf-8")) if dump.is_file() else None
        if values is None:
            print(f"skipped {hero['name']}: no character mover in {dump}")
            continue
        table[hero["body"]] = {"name": hero["name"], **values}
    return dict(sorted(table.items()))


def main(argv: list[str]) -> None:
    if len(argv) < 2:
        raise SystemExit(__doc__)
    table = collect(Path(argv[1]), DATA_DIR / "game_heroes_174.json", DATA_DIR / "ai_spawners_174.json")
    out = DATA_DIR / "game_movers_174.json"
    out.write_text(
        json.dumps(table, indent=1, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"{len(table)} bodies -> {out}")


if __name__ == "__main__":
    main(sys.argv)
