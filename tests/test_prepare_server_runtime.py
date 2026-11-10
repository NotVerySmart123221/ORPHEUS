"""Generate missing client-derived inputs while retaining authorized source files."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from ow174.paths import CLIENT_CATALOGS, LOBBY_RUNTIME_DATA, MATCH_RUNTIME_DATA
from tools.extract_arcade_cards import ASSET_FLAGS
from tools.prepare_server_runtime import prepare


class RuntimeGenerationTests(unittest.TestCase):
    def setUp(self):
        self.folder = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.source = self.folder / "source"
        self.data = self.source / "data"
        self.data.mkdir(parents=True)
        for name in LOBBY_RUNTIME_DATA + MATCH_RUNTIME_DATA:
            (self.data / name).write_text("prepared fixture", encoding="utf-8")
        descriptors = self.source / "ow174" / "bnet" / "descriptors.pb"
        descriptors.parent.mkdir(parents=True)
        descriptors.write_bytes(b"synthetic descriptors")
        self.game = self.folder / "game"
        (self.game / "data" / "casc" / "data").mkdir(parents=True)
        (self.game / ".build.info").write_text(
            "Product!STRING:0|Version!STRING:0|Active!DEC:1\npro|1.74.0.0.104319|1\n",
            encoding="utf-8",
        )
        self.destination = self.folder / "runtime"

    def test_generates_missing_data_and_collision_preserving_prepared_files(self):
        missing = ("extracted_heroes.json", "game_heroes_174.json", "arcade_cards_174.json")
        for name in missing:
            (self.data / name).unlink()

        def catalogs(game, tool, output):
            self.assertEqual(game, self.game)
            output.mkdir()
            for name in CLIENT_CATALOGS:
                (output / name).write_text(
                    '{"00000000006E.075":{"IsHero":true,"Name":"Fixture"}}', encoding="utf-8",
                )

        def datatool(tool, game, mode, output, asset, flag):
            self.assertEqual((game, mode, flag), (self.game, "extract-stu-type", "--xml"))
            folder = Path(output) / asset
            folder.mkdir(parents=True)
            if asset == "075":
                (folder / "00000000006E.075.xml").write_text(
                    '<Asset hml:name="m_gameplayEntity" GUID="0000000003CF.003"/>', encoding="utf-8",
                )
            elif asset == "0C7":
                flags = " ".join(f'{name}="0"' for name in ASSET_FLAGS)
                (folder / "000000000001.0C7.xml").write_text(f"<Asset {flags}/>", encoding="utf-8")
            else:
                self.fail(f"Unexpected XML type {asset}")

        def collision(command, *, env, **kwargs):
            self.assertEqual(command[-3:], ["0688", "--game", str(self.game)])
            staging = Path(env["ORPHEUS_RUNTIME_DIR"])
            self.assertTrue((staging / "data" / "collision_props_174.json").is_file())
            self.assertFalse((staging / "data" / "lobby_state_blob.key").exists())
            cache = staging / "collision"
            cache.mkdir()
            (cache / "0688.col").write_bytes(b"generated collision fixture")

        with (
            patch("tools.prepare_server_runtime.prepare_catalogs", side_effect=catalogs),
            patch("tools.prepare_client_gameplay.run_data_tool", side_effect=datatool),
            patch("tools.prepare_server_runtime.subprocess.run", side_effect=collision),
        ):
            prepare(
                self.source, self.destination, ["00000688"],
                game=self.game, data_tool=Path("DataTool.exe"), owlib=self.folder,
            )

        data = self.destination / "data"
        heroes = json.loads((data / "game_heroes_174.json").read_text(encoding="utf-8"))
        self.assertEqual(
            list(heroes.values()), [{"name": "Fixture", "class": "", "body": "0x04000000000003CF"}],
        )
        self.assertIn("0x1", json.loads((data / "arcade_cards_174.json").read_text(encoding="utf-8")))
        self.assertEqual((data / "extracted_maps.json").read_text(encoding="utf-8"), "prepared fixture")
        self.assertEqual((data / "game_graphs_174.json.gz").read_text(encoding="utf-8"), "prepared fixture")
        cache = self.destination / "collision" / "0688.col"
        self.assertEqual(cache.read_bytes(), b"generated collision fixture")
        self.assertFalse(any((self.data / name).exists() for name in missing))

    def test_missing_prepared_inputs_fail_before_generation(self):
        (self.data / "retail_templates.json").unlink()
        (self.data / "extracted_heroes.json").unlink()
        with (
            patch("tools.prepare_server_runtime.prepare_catalogs") as generate,
            self.assertRaisesRegex(ValueError, r"Missing runtime inputs: data/retail_templates\.json"),
        ):
            prepare(self.source, self.destination, ["0688"], game=self.game, data_tool=Path("DataTool.exe"))
        generate.assert_not_called()
        self.assertFalse(self.destination.exists())
