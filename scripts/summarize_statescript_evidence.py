"""Build private field, message, timing and variable reports from local evidence."""

import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path
import re
from zipfile import BadZipFile

from build_runtime_state_metadata import CLIENT, SERVER, listings, state_markers
from statescript_field_evidence import FieldInventory
from statescript_message_evidence import collect_messages
from statescript_timing_evidence import collect_timing
from statescript_variable_evidence import collect_variable_access


CLASS_NAME = re.compile(r"[A-Za-z][A-Za-z0-9_]{0,127}")
FIELD_NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]{0,127}")
ASSET_GUID = re.compile(r"0x[0-9A-F]{16}")


def digest_file(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def load_prepared(path):
    with gzip.open(path, "rt", encoding="utf-8") as source:
        bundle = json.load(source)
    if not isinstance(bundle, dict) or bundle.get("version") != 1:
        raise ValueError("expected a version 1 prepared graph bundle")
    graphs = {}
    for graph in bundle["graphs"]:
        index = graph["index"]
        nodes = {node["pos"]: node for node in graph["nodes"]}
        if index in graphs or len(nodes) != len(graph["nodes"]):
            raise ValueError(f"duplicate graph or node position at graph {index}")
        graphs[index] = (graph["states"], nodes, graph.get("fields", {}))
    return graphs


def verified_state(graph, slot, rid, class_name, flags):
    slots, nodes, _ = graph
    node = nodes.get(slots[slot]) if slot < len(slots) else None
    if (
        node is not None
        and node["state"] == slot
        and node["rid"] == rid
        and node["class"] == class_name
        and bool(node["client_only"]) == (CLIENT in flags)
        and bool(node["server_only"]) == (SERVER in flags)
        and node["fields"].get("m_2BBEEAB8")
    ):
        return node["fields"]
    return None


def public_schema_defaults(index, fields):
    """Identify declared instance slots accepted by the Rust public schema shape."""
    schema = fields.get("m_publicSchema")
    if schema is None:
        return {"status": "absent", "default_slots": []}
    unrecognized = {"status": "unrecognized", "default_slots": []}
    if not isinstance(schema, dict) or schema.get("$") not in (
        "STUStatescriptSchema", "STU_77990ED7"
    ):
        return unrecognized
    owner = schema.get("m_89EAAC40")
    if owner is not None and (
        schema["$"] != "STU_77990ED7" or owner != f"0x{0x0580000000000000 | index:016X}"
    ):
        return unrecognized
    entries = schema.get("m_entries")
    if entries is not None and not isinstance(entries, list):
        return unrecognized
    slots = set()
    for entry in entries or []:
        if not isinstance(entry, dict):
            return unrecognized
        identifier = entry.get("m_0D09D2D9")
        if entry.get("m_value") is None or identifier is None:
            continue
        if not isinstance(identifier, str) or not ASSET_GUID.fullmatch(identifier):
            return unrecognized
        if asset := int(identifier, 16):
            slots.add(asset & 0xFFFF)
    return {"status": "recognized", "default_slots": sorted(slots)}


def operand_shapes(fields):
    """Return the distinct kinds at each field path of a single state."""

    if not isinstance(fields, dict):
        raise ValueError("prepared state fields must be an object")
    observed = {}
    pending = [("", fields)]
    while pending:
        path, value = pending.pop()
        variants = observed.setdefault(path, set()) if path else None
        if isinstance(value, dict):
            if variants is not None:
                variants.add("object")
                tag = value.get("$")
                if isinstance(tag, str) and CLASS_NAME.fullmatch(tag):
                    variants.add(f"class:{tag}")
            for field, nested in value.items():
                if FIELD_NAME.fullmatch(field):
                    pending.append((f"{path}.{field}" if path else field, nested))
        elif isinstance(value, list):
            variants.add("array")
            pending.extend((path + "[]", item) for item in value)
        else:
            variants.add(
                {type(None): "null", bool: "boolean", int: "integer", float: "number",
                 str: "string"}[type(value)]
            )
    return observed


class PreparedEvidence:
    def __init__(self, path):
        self.graphs = load_prepared(path)
        self.digest = digest_file(path)
        self.overlap = 0
        self.matched = Counter()
        self.unmatched = Counter()
        self.fields = {}

    def add_graph(self, index, markers):
        graph = self.graphs.get(index)
        if graph is None:
            return
        self.overlap += 1
        for slot, rid, name, flags in markers:
            fields = verified_state(graph, slot, rid, name, flags)
            if fields is None:
                self.unmatched[name] += 1
                continue
            self.matched[name] += 1
            rows = self.fields.setdefault(name, {})
            for path, shapes in operand_shapes(fields).items():
                hits, kinds, examples = rows.get(path, (0, Counter(), {}))
                kinds.update(shapes)
                reference = (index, slot, rid)
                for shape in shapes:
                    examples[shape] = min(reference, examples.get(shape, reference))
                rows[path] = (hits + 1, kinds, examples)

    def report(self):
        classes = []
        for name in sorted(self.matched.keys() | self.unmatched.keys()):
            count = self.matched[name]
            classes.append({
                "class": name,
                "matched_states": count,
                "unmatched_markers": self.unmatched[name],
                "fields": [{
                    "path": path, "present": hits, "missing": count - hits,
                    "kinds": dict(sorted(kinds.items())),
                    "examples": {
                        shape: {"graph": graph, "state": slot, "rid": rid}
                        for shape, (graph, slot, rid) in sorted(examples.items())
                    },
                } for path, (hits, kinds, examples) in sorted(self.fields.get(name, {}).items())],
            })
        return {
            "bundle_sha256": self.digest,
            "bundle_graphs": len(self.graphs),
            "overlapping_graphs": self.overlap,
            "matched_states": self.matched.total(),
            "unmatched_markers": self.unmatched.total(),
            "classes": classes,
        }


def load_families(path):
    if path is None:
        return {}
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict):
        raise ValueError("capability families must be an object mapping names to class arrays")
    for name, classes in value.items():
        if (
            not name.strip()
            or not isinstance(classes, list)
            or not all(isinstance(tag, str) and CLASS_NAME.fullmatch(tag) for tag in classes)
            or len(set(classes)) != len(classes)
        ):
            raise ValueError(f"invalid or duplicate classes in capability family {name!r}")
    return value


def family_report(families, listing, prepared):
    seen = {row["class"]: row["occurrences"] for row in listing["classes"]}
    matched = {row["class"]: row["matched_states"] for row in prepared["classes"]} if prepared else {}
    return {
        name: [{
            "class": tag, "listing_occurrences": seen.get(tag, 0),
            "prepared_matched_states": matched.get(tag, 0) if prepared else None,
        } for tag in sorted(classes)]
        for name, classes in sorted(families.items())
    }


def parameter_slots(endpoints):
    slots = {}
    for endpoint in endpoints:
        for parameter in endpoint["parameters"]:
            slot = parameter["parameter_slot"]
            if slot is None:
                continue
            ids, counts = slots.setdefault(slot, (set(), Counter()))
            ids.add(parameter["parameter_id"])
            counts[endpoint["direction"]] += 1
    return [{
        "slot": slot, "parameter_ids": sorted(ids),
        "send_parameters": counts["send"], "receive_parameters": counts["receive"],
    } for slot, (ids, counts) in sorted(slots.items())]


def message_report(records):
    groups = {}
    unresolved = []
    for record in sorted(records, key=lambda row: (
        row["graph"], row["line"], row["direction"], row["form"],
    )):
        if record["message_id"] is None:
            unresolved.append(record)
        else:
            groups.setdefault(record["message_id"], []).append(record)

    messages = []
    for message_id, endpoints in sorted(groups.items()):
        directions = Counter(endpoint["direction"] for endpoint in endpoints)
        messages.append({
            "message_id": message_id, "sends": directions["send"],
            "receives": directions["receive"], "parameter_slots": parameter_slots(endpoints),
            "endpoints": endpoints,
        })
    return {
        "message_count": len(messages),
        "observed_endpoints": len(records),
        "unresolved_endpoints": unresolved,
        "messages": messages,
    }


def collect_evidence(evidence, graph_bundle=None, families=None):
    inventory = FieldInventory()
    prepared = PreparedEvidence(graph_bundle) if graph_bundle else None
    graph_hashes = {}
    messages = []
    timing = []
    variables = []
    state_count = 0
    for guid, source in listings(evidence):
        index = guid & 0xFFFFFFFF
        if index in graph_hashes:
            raise ValueError(f"duplicate evidence graph {index}")
        lines = list(source)
        markers = list(state_markers(lines, guid))
        if len({marker[0] for marker in markers}) != len(markers):
            raise ValueError(f"duplicate state slot in evidence graph {index}")
        state_count += len(markers)
        graph_hashes[index] = (guid, hashlib.sha256("".join(lines).encode("utf-8")).hexdigest())
        inventory.add_graph(guid, lines)
        endpoints = collect_messages(lines, index)
        messages.extend(endpoints)
        timing.extend(collect_timing(lines, index))
        graph_variables = collect_variable_access(lines, index, endpoints)
        graph_variables["prepared_public_schema"] = None
        if prepared is not None:
            prepared.add_graph(index, markers)
            if graph := prepared.graphs.get(index):
                graph_variables["prepared_public_schema"] = public_schema_defaults(index, graph[2])
        variables.append(graph_variables)

    if not graph_hashes:
        raise ValueError("no graph listings found in the evidence source")
    provenance = hashlib.sha256()
    for guid, digest in sorted(graph_hashes.values()):
        provenance.update(guid.to_bytes(8, "big"))
        provenance.update(bytes.fromhex(digest))
    manifest = {
        "schema_version": 1,
        "tool": "statescript-evidence/v3",
        "source_sha256": provenance.hexdigest(),
        "source_digest_basis": "graph GUID and normalized listing digest, ordered by GUID",
        "listing_graphs": len(graph_hashes),
        "state_markers": state_count,
    }

    listing = inventory.report()
    prepared_report = prepared.report() if prepared is not None else None
    provenance_keys = {"schema_version": 1, "source_sha256": manifest["source_sha256"]}
    return {
        "field-contracts.json": {
            **provenance_keys,
            "listing": listing, "prepared_bundle": prepared_report,
            "families": family_report(families or {}, listing, prepared_report),
        },
        "gamemessage-catalog.json": {**provenance_keys, **message_report(messages)},
        "timing-contracts.json": {
            **provenance_keys,
            "resolutions": dict(sorted(Counter(row["resolution"] for row in timing).items())),
            "observations": sorted(timing, key=lambda row: (row["graph"], row["line"])),
        },
        "variable-access.json": {
            **provenance_keys,
            "prepared_bundle_sha256": prepared.digest if prepared is not None else None,
            "graphs": sorted(variables, key=lambda row: row["graph"]),
        },
        "evidence-manifest.json": manifest,
    }


def write_reports(directory, reports):
    directory.mkdir()
    created = []
    try:
        for name, value in reports.items():
            path = directory / name
            with path.open("x", encoding="utf-8", newline="\n") as output:
                created.append(path)
                json.dump(value, output, indent=2, ensure_ascii=False, allow_nan=False)
                output.write("\n")
    except BaseException:
        for path in reversed(created):
            path.unlink(missing_ok=True)
        directory.rmdir()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True, help="local listing ZIP or directory")
    parser.add_argument("--graph-bundle", type=Path, help="optional prepared graph bundle for exact cross-checks")
    parser.add_argument("--families", type=Path, help="optional JSON capability groups keyed by name")
    parser.add_argument("--output-dir", type=Path, required=True, help="new private report directory")
    args = parser.parse_args()
    if args.output_dir.exists():
        parser.error("report directory already exists")
    try:
        reports = collect_evidence(
            args.evidence, args.graph_bundle, load_families(args.families),
        )
        write_reports(args.output_dir, reports)
    except (OSError, ValueError, KeyError, TypeError, BadZipFile) as error:
        parser.error(str(error))

    fields = reports["field-contracts.json"]
    messages = reports["gamemessage-catalog.json"]
    timing = reports["timing-contracts.json"]
    variables = reports["variable-access.json"]
    manifest = reports["evidence-manifest.json"]
    print(f"Read {manifest['listing_graphs']} graphs and {manifest['state_markers']} state markers.")
    print(f"Recorded {len(fields['listing']['classes'])} classes and "
          f"{messages['message_count']} literal message IDs.")
    print(f"Left {len(messages['unresolved_endpoints'])} message endpoints unresolved.")
    print(f"Recorded {len(timing['observations'])} timing sites and "
          f"variable evidence for {len(variables['graphs'])} graphs.")
    if prepared := fields["prepared_bundle"]:
        print(f"Cross-checked {prepared['matched_states']} prepared state slots, "
              f"with {prepared['unmatched_markers']} unmatched markers.")


if __name__ == "__main__":
    main()
