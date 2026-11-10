"""Prepare map data from the selected local game storage and the server's built-in Practice Range."""

import json
import shutil
import subprocess
from dataclasses import asdict
from pathlib import Path

from ow174.game.casc import RecordCatalog
from ow174.game.collision import chunk_guid
from ow174.game.content import PRACTICE_RANGE
from ow174.paths import ROOT


def write_practice_map(output: Path) -> None:
    scene = PRACTICE_RANGE
    data = {
        f"0x{scene.map_guid:016X}": {
            "name": scene.name,
            "modes": [f"0x{scene.mode_guid:016X}"],
            "placeables": scene.placeables,
            "spawns": [asdict(spawn) for spawn in scene.spawns],
        },
    }
    output.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def write_collision_index(game: Path, data_tool: Path, maps: set[int], output: Path) -> None:
    pwsh = shutil.which("pwsh")
    if pwsh is None:
        raise ValueError("Building the local collision index requires PowerShell 7 (pwsh)")
    if not data_tool.is_file():
        raise ValueError("Select the built DataTool.exe from the pinned OWLib checkout")
    chunks = {map_guid: chunk_guid(map_guid) for map_guid in sorted(maps)}
    requested = ",".join(f"0x{guid:016X}" for guid in chunks.values())
    response = subprocess.run(
        [
            pwsh, "-NoProfile", "-NonInteractive", "-File", str(ROOT / "tools" / "export_casc_records.ps1"),
            "-GameRoot", str(game), "-DataToolDir", str(data_tool.resolve().parent),
            "-CollisionChunks", requested,
        ],
        check=True,
        timeout=300,
        stdout=subprocess.PIPE,
    )
    catalog_path = output.with_name("collision-records.json")
    catalog_path.write_bytes(response.stdout)
    records = RecordCatalog.load(catalog_path, collision_chunks=True).records
    if records.keys() != set(chunks.values()):
        raise ValueError("The local collision index does not cover the selected map chunks")
    data = {f"0x{map_guid:016X}": records[guid] for map_guid, guid in chunks.items()}
    output.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
