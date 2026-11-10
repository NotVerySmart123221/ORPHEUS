"""Build a class/field hash -> name table from OWLib's generated STU types (read as text only).

OWLib's TankLib/STU/Types/*.cs carry `[STU(0xHASH, size)]` class and `[STUField(0xHASH, ...)]`
field attributes. The table is current-build data; most hashes are stable since 1.x, but a
missing or renamed entry is possible.

  python stu_names.py <OWLib>/TankLib/STU/Types   # writes stu_names.json
"""
import json
import re
import sys
from pathlib import Path

CLASS = re.compile(r"\[STU\(0x([0-9A-Fa-f]{8})[^\]]*\]\s*public\s+(?:sealed\s+)?class\s+(\w+)(?:\s*:\s*(\w+))?")
FIELD = re.compile(r"\[STUField\(0x([0-9A-Fa-f]{8})[^\]]*\]\s*(?://[^\n]*)?\s*public\s+([\w<>\[\],. ]+?)\s+(\w+)\s*[;=]")
TABLE = Path(__file__).with_name("stu_names.json")


def parse(text):
    """{class_hash: {"name", "parent", "fields": {field_hash: [name, type]}}} for one source file."""
    out = {}
    marks = list(CLASS.finditer(text))
    for i, m in enumerate(marks):
        body = text[m.end():marks[i + 1].start() if i + 1 < len(marks) else len(text)]
        fields = {f.group(1).upper(): [f.group(3), f.group(2).strip()] for f in FIELD.finditer(body)}
        out[m.group(1).upper()] = {"name": m.group(2), "parent": m.group(3), "fields": fields}
    return out


def load():
    """Saved table, or {} when stu_names.json was never built."""
    return json.loads(TABLE.read_text()) if TABLE.exists() else {}


def field_name(table, cls, field):
    """Field name, searching the class's parents."""
    seen = set()
    while cls in table and cls not in seen:
        seen.add(cls)
        entry = table[cls]
        if field in entry["fields"]:
            return entry["fields"][field][0]
        cls = next((h for h, e in table.items() if e["name"] == entry["parent"]), None)
    return None


def main(root):
    table = {}
    for path in sorted(Path(root).rglob("*.cs")):
        table.update(parse(path.read_text(encoding="utf-8", errors="replace")))
    TABLE.write_text(json.dumps(table, indent=0, sort_keys=True))
    print(f"{len(table)} classes, {sum(len(e['fields']) for e in table.values())} fields -> {TABLE}")


if __name__ == "__main__":
    main(sys.argv[1])
