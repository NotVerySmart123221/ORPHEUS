"""Add locally verified state, message, and asset metadata to a private graph bundle."""

import argparse
import gzip
import io
import json
from pathlib import Path, PurePosixPath
import re
from zipfile import BadZipFile, ZipFile


LISTING_NAME = re.compile(r"([0-9A-Fa-f]{16})\.txt")
GRAPH_HEADER = re.compile(r"^# graph ([0-9A-Fa-f]{16})(?=\s|$)")
STATE_HEADER = re.compile(
    r"^state [^:]+_(\d+):\s+#\s+([A-Za-z][A-Za-z0-9_]{0,127}),"
    r"\s+state\s+(\d+)\s*(?:\(([^)]*)\))?\s*(?=;|$)"
)
PLAIN = "plain State at run time"
CLIENT = "client-only"
SERVER = "server-only"


def listings(path):
    if path.is_dir():
        for item in sorted(path.rglob("*.txt")):
            if name := LISTING_NAME.fullmatch(item.name):
                with item.open(encoding="utf-8-sig") as source:
                    yield int(name[1], 16), source
    else:
        with ZipFile(path) as archive:
            for item in archive.infolist():
                if item.is_dir():
                    continue
                if name := LISTING_NAME.fullmatch(PurePosixPath(item.filename).name):
                    with io.TextIOWrapper(archive.open(item), encoding="utf-8-sig") as source:
                        yield int(name[1], 16), source


def state_markers(source, expected_guid):
    graph_seen = False
    for line_number, line in enumerate(source, start=1):
        if line.startswith("# graph "):
            header = GRAPH_HEADER.match(line)
            if graph_seen or not header or int(header[1], 16) != expected_guid:
                raise ValueError(f"invalid or duplicate graph header at line {line_number}")
            graph_seen = True
        elif line.startswith("state "):
            header = STATE_HEADER.match(line)
            if not graph_seen or not header:
                raise ValueError(f"invalid state header at line {line_number}")
            rid, class_name, slot, tags = header.groups()
            rid, slot = int(rid), int(slot)
            flags = set(tags.split(", ")) if tags else set()
            if (
                not flags <= {PLAIN, CLIENT, SERVER}
                or {CLIENT, SERVER} <= flags
                or rid > 0xFFFF
                or slot > 0xFFFF
            ):
                raise ValueError(f"invalid state metadata at line {line_number}")
            yield slot, rid, class_name, flags
    if not graph_seen:
        raise ValueError("evidence listing has no graph header")


def confirm_client_state(graph, slot, rid, class_name):
    slots, nodes = graph
    node = nodes.get(slots[slot]) if slot < len(slots) else None
    return (
        node is not None
        and node["state"] == slot
        and node["rid"] == rid
        and node["class"] == class_name
        and node["client_only"] in (1, True)
        and node["server_only"] in (0, False)
        and bool(node["fields"].get("m_2BBEEAB8"))
        and not any(
            node.get(field)
            for field in (
                "state_behavior",
                "action_behavior",
                "room_message_entry",
                "host_event_entry",
            )
        )
    )


def build_metadata(evidence, graphs):
    seen_graphs = set()
    classes = {}
    client_states = []
    state_count = 0
    client_outside_pack = 0
    client_unmatched = 0

    for guid, source in listings(evidence):
        graph = guid & 0xFFFFFFFF
        if graph in seen_graphs:
            raise ValueError("duplicate evidence graph index")
        seen_graphs.add(graph)
        prepared = graphs.get(graph)
        marked = set()
        for slot, rid, class_name, flags in state_markers(source, guid):
            if (slot, rid) in marked:
                raise ValueError("duplicate evidence state marker")
            marked.add((slot, rid))
            state_count += 1
            classes[class_name] = classes.get(class_name, True) and PLAIN in flags
            if CLIENT not in flags:
                continue
            if prepared is None:
                client_outside_pack += 1
            elif not confirm_client_state(prepared, slot, rid, class_name):
                client_unmatched += 1
            else:
                client_states.append(
                    {"graph": graph, "state": slot, "rid": rid, "class": class_name}
                )

    if not seen_graphs:
        raise ValueError("no graph listings found in the evidence source")
    plain_classes = sorted(name for name, plain in classes.items() if plain)
    client_states.sort(key=lambda row: (row["graph"], row["state"], row["rid"]))
    metadata = {"plain_classes": plain_classes, "client_states": client_states}
    counts = (len(seen_graphs), state_count, len(seen_graphs & graphs.keys()))
    return metadata, counts, client_outside_pack, client_unmatched


def write_bundle(path, bundle):
    output = path.open("xb")
    try:
        with output, gzip.GzipFile(fileobj=output, filename="", mode="wb", mtime=0) as packed:
            with io.TextIOWrapper(packed, encoding="utf-8") as text:
                json.dump(bundle, text, ensure_ascii=False, allow_nan=False, separators=(",", ":"))
    except BaseException:
        path.unlink(missing_ok=True)
        raise


def directed_numeric_message(raw):
    if not re.fullmatch(r"0x0240[0-9a-f]{12}:0x0a4000000000[0-9a-f]{4}", raw, re.IGNORECASE):
        raise argparse.ArgumentTypeError("expected native message GUID:parameter GUID")
    topic, parameter = (int(part, 16) for part in raw.split(":"))
    if (topic & 0xFFFF) == 0 or (parameter & 0xFFFF) == 0:
        raise argparse.ArgumentTypeError("expected nonzero message and parameter slots")
    return {"topic": f"0x{topic:016X}", "parameter": f"0x{parameter:016X}"}


def asset_categories(path):
    with path.open(encoding="utf-8") as source:
        catalog = json.load(source)
    if (
        not isinstance(catalog, dict)
        or catalog.get("version") != 1
        or catalog.get("build") != 104319
        or not isinstance(catalog.get("assets"), dict)
        or not catalog["assets"]
    ):
        raise ValueError("expected version 1 asset categories for build 104319")

    def guid(value):
        return (
            isinstance(value, str)
            and re.fullmatch(r"0x[0-9a-fA-F]{16}", value) is not None
            and int(value, 16) != 0
        )

    for asset, rows in catalog["assets"].items():
        if not guid(asset) or not isinstance(rows, list):
            raise ValueError("invalid asset category row")
        for row in rows:
            if (
                not isinstance(row, dict)
                or set(row) != {"type", "id"}
                or not guid(row["type"])
                or not guid(row["id"])
            ):
                raise ValueError("invalid asset category identifier pair")
    return catalog["assets"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--graph-bundle", type=Path, required=True, help="version 1 gzip graph bundle")
    parser.add_argument("--evidence", type=Path, help="local listing ZIP or directory")
    parser.add_argument(
        "--directed-numeric-message",
        action="append",
        default=[],
        type=directed_numeric_message,
        metavar="TOPIC:PARAMETER",
        help="declare a reviewed native numeric room message (repeatable)",
    )
    parser.add_argument("--asset-categories", type=Path, help="local 104319 asset category catalog")
    parser.add_argument("--output", type=Path, required=True, help="new private gzip bundle")
    args = parser.parse_args()
    if not (args.evidence or args.directed_numeric_message or args.asset_categories):
        parser.error("provide --evidence, --directed-numeric-message, or --asset-categories")

    try:
        with gzip.open(args.graph_bundle, "rt", encoding="utf-8") as source:
            bundle = json.load(source)
        if not isinstance(bundle, dict) or bundle.get("version") != 1:
            raise ValueError("expected a version 1 graph bundle")
        if args.evidence is not None:
            if "runtime_state_metadata" in bundle:
                raise ValueError("classification requires a bundle without runtime state metadata")
            graphs = {
                graph["index"]: (graph["states"], {node["pos"]: node for node in graph["nodes"]})
                for graph in bundle["graphs"]
            }
            if len(graphs) != len(bundle["graphs"]):
                raise ValueError("duplicate prepared graph index")
            metadata, counts, outside, unmatched = build_metadata(args.evidence, graphs)
            bundle["runtime_state_metadata"] = metadata
        elif not isinstance(bundle.get("graphs"), list):
            raise ValueError("expected a prepared graph list")
        if args.directed_numeric_message:
            metadata = bundle.setdefault("runtime_state_metadata", {})
            if not isinstance(metadata, dict) or "directed_numeric_messages" in metadata:
                raise ValueError("cannot replace existing directed numeric message metadata")
            routes = sorted(args.directed_numeric_message, key=lambda route: route["topic"])
            if len({route["topic"] for route in routes}) != len(routes):
                raise ValueError("duplicate directed numeric message topic")
            metadata["directed_numeric_messages"] = routes
        if args.asset_categories is not None:
            if "asset_categories" in bundle:
                raise ValueError("cannot replace existing asset category metadata")
            bundle["asset_categories"] = asset_categories(args.asset_categories)
        write_bundle(args.output, bundle)
    except (OSError, ValueError, KeyError, TypeError, BadZipFile) as error:
        parser.error(str(error))

    if args.evidence is not None:
        listings_count, state_count, overlap = counts
        print(f"Read {listings_count} graph listings and {state_count} state markers.")
        print(f"Matched {overlap} of {len(graphs)} prepared graphs.")
        print(f"Recorded {len(metadata['plain_classes'])} plain state classes.")
        print(
            f"Confirmed {len(metadata['client_states'])} client states, "
            f"with {outside} outside the bundle and {unmatched} unmatched within it."
        )
    if args.directed_numeric_message:
        print(f"Declared {len(args.directed_numeric_message)} directed numeric messages.")
    if args.asset_categories is not None:
        print(f"Imported categories for {len(bundle['asset_categories'])} assets.")


if __name__ == "__main__":
    main()
