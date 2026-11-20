"""Generate an original draft graph from allowlisted StateScript operand kinds."""

import argparse
import json
from pathlib import Path


SAMPLES = {
    "array": [2, 4],
    "boolean": True,
    "call": {"scaffold_operator": "identity", "scaffold_arguments": [3]},
    "expression": {"scaffold_form": "unimplemented_example"},
    "integer": 7,
    "null": None,
    "number": 1.25,
    "object": {"scaffold_member": False},
    "reference": {"scaffold_scope": "instance", "scaffold_slot": 1},
    "string": "synthetic example",
}


def selected_classes(report, family, class_name):
    if class_name is not None:
        return {class_name}
    if family is None:
        return None

    families = report.get("families")
    members = families.get(family) if isinstance(families, dict) else None
    if not isinstance(members, list):
        raise ValueError("requested family is absent from the field report")
    classes = set()
    for member in members:
        name = member.get("class") if isinstance(member, dict) else None
        if not isinstance(name, str) or not name:
            raise ValueError("expected a class name for every selected family member")
        classes.add(name)
    return classes


def observed_shapes(report, *, family=None, class_name=None):
    """Keep only kind presence. Drop names, counts and source provenance."""
    version = report.get("schema_version") if isinstance(report, dict) else None
    if type(version) is not int or version != 1:
        raise ValueError("expected a version 1 field report")

    classes = selected_classes(report, family, class_name)
    sections = [report.get("listing")]
    if (prepared := report.get("prepared_bundle")) is not None:
        sections.append(prepared)
    shapes = set()
    for section in sections:
        if not isinstance(section, dict) or not isinstance(section.get("classes"), list):
            raise ValueError("expected field report class lists")
        for category in section["classes"]:
            if not isinstance(category, dict):
                raise ValueError("expected an object for each class")
            name = category.get("class")
            if classes is not None and (not isinstance(name, str) or name not in classes):
                continue
            fields = category.get("fields")
            if not isinstance(fields, list):
                raise ValueError("expected a field list for each class")
            for field in fields:
                if not isinstance(field, dict) or not isinstance(field.get("kinds"), dict):
                    raise ValueError("expected an operand kind count object")
                shapes.update(
                    kind for kind, count in field["kinds"].items()
                    if kind in SAMPLES and type(count) is int and count > 0
                )
    if not shapes:
        raise ValueError("field report has no allowlisted operand shapes")
    return shapes


def scaffold(shapes):
    graph_id = 1
    state_slot = 0
    fields = {f"scaffold_example_{kind}": SAMPLES[kind] for kind in sorted(shapes)}
    node = {
        "pos": 0,
        "class": "ScaffoldState",
        "uid": 0,
        "rid": 0,
        "remote": -1,
        "state": state_slot,
        "client_only": False,
        "server_only": True,
        "state_behavior": "plain",
        "fields": fields,
    }
    graph = {
        "index": graph_id,
        "items": 1,
        "header": {
            "m_nodesBitCount": 1,
            "m_statesBitCount": 1,
            "m_remoteSyncNodesBitCount": 1,
            "m_syncVarsBitCount": 1,
        },
        "fields": {},
        "containers": {},
        "nodes": [node],
        "unknown_nodes": [],
        "states": [node["pos"]],
        "entries": [],
        "remote": [],
        "sync_vars": [],
    }
    return {
        "schema_version": 1,
        "case_id": "script-shape-scaffold.synthetic.v1",
        "source": {"basis": "Original generic examples and a single plain state."},
        "evidence": "synthetic-scaffold",
        "review_status": "draft",
        "input": {
            "bundle": {"version": 1, "graphs": [graph], "bodies": {}, "missing": []},
            "start": {
                "entity": 1,
                "instance": 1,
                "graph": graph_id,
                "initial_state": state_slot,
            },
        },
        "expected": {"active_state_slots": [state_slot], "diagnostics": []},
    }


def write_new(path, value):
    contents = json.dumps(value, indent=2, allow_nan=False) + "\n"
    created = False
    try:
        with path.open("x", encoding="utf-8", newline="\n") as destination:
            created = True
            destination.write(contents)
    except OSError:
        if created:
            path.unlink(missing_ok=True)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--field-contracts", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new draft scaffold JSON file")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--family", help="use one exact capability family from the field report")
    selection.add_argument("--class", dest="class_name", help="use one exact class from the field report")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("output file already exists")
    try:
        report = json.loads(args.field_contracts.read_text(encoding="utf-8-sig"))
        shapes = observed_shapes(report, family=args.family, class_name=args.class_name)
        write_new(args.output, scaffold(shapes))
    except (OSError, ValueError, RecursionError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
