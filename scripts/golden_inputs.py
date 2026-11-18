"""Identify the exact prepared pack and canonical schedule used in a private replay."""

import hashlib
import json
import stat
from dataclasses import dataclass
from pathlib import Path


def regular_entry(entry: Path) -> int:
    info = entry.lstat()
    if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
        raise ValueError("linked input in the replay profile")
    return info.st_mode


def file_digest(path: Path) -> bytes:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").digest()


def schedule_digest(schedule: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    count = 0
    with schedule.open("rb") as stream:
        while line := stream.readline(256 * 1024 + 1):
            if len(line) > 256 * 1024 or count == 1_000_000:
                raise ValueError("schedule exceeds the golden profile limits")
            row = json.loads(line)
            if not isinstance(row, dict) or set(row) != {"elapsed_us", "inputs", "missed", "tick"}:
                raise ValueError("unexpected replay schedule fields")
            if type(row["tick"]) is not int or row["tick"] != count:
                raise ValueError("schedule ticks must be contiguous")
            canonical = json.dumps(row, allow_nan=False, sort_keys=True, separators=(",", ":")).encode() + b"\n"
            if line != canonical:
                raise ValueError("expected canonical LF-terminated replay schedule")
            digest.update(line)
            count += 1
    if not count:
        raise ValueError("empty replay schedule")
    return digest.hexdigest(), count


@dataclass(frozen=True)
class ReplayInputs:
    runtime_sha256: str
    profile_sha256: str
    schedule_sha256: str
    ticks: int

    @classmethod
    def read(cls, runtime: Path, schedule: Path) -> "ReplayInputs":
        # Match the private runner's orpheus-runtime-tree-v1 byte format. Include
        # supplemental tables and empty directories. Paths use '/' on both OSes.
        if not stat.S_ISDIR(regular_entry(runtime)) or not stat.S_ISREG(regular_entry(schedule)):
            raise ValueError("expected a prepared directory and regular schedule")
        digest = hashlib.sha256(b"orpheus-runtime-tree-v1\0")
        manifest = None
        pending = [runtime]
        while pending:
            for entry in sorted(pending.pop().iterdir(), key=lambda item: item.name):
                mode = regular_entry(entry)
                name = entry.relative_to(runtime).as_posix().encode("utf-8", "surrogateescape")
                if stat.S_ISDIR(mode):
                    digest.update(b"D" + name + b"\0")
                    pending.append(entry)
                elif stat.S_ISREG(mode):
                    value = file_digest(entry)
                    digest.update(b"F" + name + b"\0" + value)
                    if name == b"manifest.json":
                        manifest = value.hex()
                else:
                    raise ValueError("unexpected file type in the replay profile")
        if manifest is None:
            raise ValueError("prepared runtime manifest is missing")
        schedule_sha256, ticks = schedule_digest(schedule)
        return cls(manifest, digest.hexdigest(), schedule_sha256, ticks)
