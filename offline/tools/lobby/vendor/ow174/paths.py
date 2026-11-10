"""Data shipped alongside the audited, pinned game-server subset."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent  # the vendor folder: collision.py keeps its map caches in cache/collision
DATA_DIR = ROOT / "data"
GAME_PATH_FILE = ROOT / "game_path.txt"  # collision.installed_game(); never written here: run.py builds the caches
