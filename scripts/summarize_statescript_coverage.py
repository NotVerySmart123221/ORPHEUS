"""Join private StateScript reach observations to a prepared body graph inventory."""

import argparse
from collections import Counter
from dataclasses import dataclass
import gzip
import hashlib
import json
from pathlib import Path
import re


GUID = re.compile(r"(?:0[xX])?[0-9A-Fa-f]{1,16}")
CANONICAL_GUID = re.compile(r"0x[0-9A-F]{16}")
REVISION = re.compile(r"[0-9a-f]{40}")
SCENARIO = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")
CLASS_NAME = re.compile(r"[A-Za-z][A-Za-z0-9_]{0,127}")
COVERAGE = (
    "observed", "client_skipped", "unsupported", "registration_rejection",
    "not_observed", "no_scenario",
)
DIAGNOSTICS = {"registration_rejection", "unsupported"}
SIDECAR_FIELDS = {
    "schema_version", "scenario", "hero", "available", "frames", "truncated",
    "observations", "diagnostics", "graph_bundle_sha256", "source_revision",
}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def load_json(path, compressed=False):
    data = path.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    if compressed:
        data = gzip.decompress(data)
    return json.loads(data, object_pairs_hook=unique_object), digest


def uint(value, bits):
    return type(value) is int and 0 <= value < 1 << bits


def guid(value):
    if isinstance(value, str) and GUID.fullmatch(value):
        if number := int(value, 16):
            return f"0x{number:016X}"
    raise ValueError("invalid catalog or body GUID")


def guid_index(records):
    index = {}
    for key, row in records.items():
        key = guid(key)
        if key in index:
            raise ValueError("duplicate normalized catalog or body GUID")
        index[key] = row
    return index


def runtime_state_metadata(bundle):
    metadata = bundle.get("runtime_state_metadata", {})
    if not isinstance(metadata, dict) or set(metadata) - {
        "plain_classes", "client_states", "projected_message_topics",
    }:
        raise ValueError("invalid runtime state metadata fields")
    plain = metadata.get("plain_classes", [])
    if not isinstance(plain, list) or any(
        not isinstance(name, str) or not CLASS_NAME.fullmatch(name) for name in plain
    ) or len(set(plain)) != len(plain):
        raise ValueError("invalid or duplicate runtime plain state class")

    topics = metadata.get("projected_message_topics", [])
    if not isinstance(topics, list) or any(
        not isinstance(topic, str) or not CANONICAL_GUID.fullmatch(topic) or int(topic, 16) == 0
        for topic in topics
    ) or len(set(topics)) != len(topics):
        raise ValueError("invalid or duplicate projected message topic")

    entries = metadata.get("client_states", [])
    if not isinstance(entries, list):
        raise ValueError("expected a runtime client state list")
    clients = {}
    for row in entries:
        if (
            not isinstance(row, dict) or set(row) != {"graph", "state", "rid", "class"}
            or not uint(row["graph"], 32) or not uint(row["state"], 16)
            or not uint(row["rid"], 16)
            or not isinstance(row["class"], str) or not CLASS_NAME.fullmatch(row["class"])
        ):
            raise ValueError("invalid runtime client state identity")
        graph, slot = row["graph"], row["state"]
        slots = clients.setdefault(graph, {})
        if slot in slots:
            raise ValueError(f"duplicate runtime client state in graph {graph} at slot {slot}")
        slots[slot] = row["rid"], row["class"]
    return set(plain), clients


def graph_index(bundle):
    if not isinstance(bundle, dict) or not uint(bundle.get("version"), 32) or bundle["version"] != 1:
        raise ValueError("expected a version 1 prepared graph bundle")
    plain_classes, client_slots = runtime_state_metadata(bundle)
    authored_client_classes = {
        row["class"] for row in bundle.get("authored_classes", [])
        if row["behavior"] == "client_only"
    }
    authored_track_classes = {
        row["class"] for row in bundle.get("authored_classes", [])
        if row["behavior"] == "track_entities_state"
    }
    graphs = {}
    dependencies = {}
    for graph in bundle["graphs"]:
        index = graph["index"]
        if not uint(index, 32) or index in graphs:
            raise ValueError("invalid or duplicate prepared graph index")
        nodes = {}
        graph_dependencies = []
        states = graph.get("states", [])
        if not isinstance(states, list) or any(
            position is not None and not uint(position, 16) for position in states
        ):
            raise ValueError("invalid prepared graph state slots")
        annotations = client_slots.pop(index, {})
        for node in graph["nodes"]:
            pos, rid, state, name = (node[key] for key in ("pos", "rid", "state", "class"))
            if (
                not uint(pos, 16) or pos in nodes or not uint(rid, 16)
                or state is not None and not uint(state, 16)
                or not isinstance(name, str) or not name
            ):
                raise ValueError("invalid or duplicate prepared graph node")
            client, server = bool(node["client_only"]), bool(node["server_only"])
            if name in authored_track_classes:
                server = True
            scope = {
                (False, False): "networked", (False, True): "server_only",
                (True, False): "client_only", (True, True): "conflicting",
            }[client, server]
            # Scope and runtime classification describe eligibility only. The sidecar remains
            # the sole source of reached, skipped, unsupported and rejected statuses.
            runtime_classification = None
            client_scope = client or name in authored_client_classes
            if state is not None and state < len(states) and states[state] == pos:
                fields = node.get("fields")
                is_state = isinstance(fields, dict) and bool(fields.get("m_2BBEEAB8"))
                authored = (
                    name.startswith("authored.") or node.get("state_behavior") is not None
                    or node.get("action_behavior") is not None
                    or node.get("room_message_entry") is True
                    or node.get("host_event_entry") is not None
                    or name in authored_track_classes
                )
                if state in annotations:
                    identity = annotations.pop(state)
                    if identity != (rid, name) or not is_state or server or authored:
                        raise ValueError(
                            f"runtime client state in graph {index} at slot {state}"
                            " does not match an eligible state"
                        )
                    client_scope = True
                    runtime_classification = "client_state"
                elif is_state and not authored and not client_scope and name in plain_classes:
                    runtime_classification = "plain_candidate"
            nodes[pos] = {
                "node": pos, "rid": rid, "state": state, "class": name, "scope": scope,
                "server_scope": "client_only" if client_scope else "eligible",
                "runtime_classification": runtime_classification,
            }
            child = target_child_graph(node, states, name in authored_track_classes)
            if child is not None:
                graph_dependencies.append({
                    "source_graph": index, "source_node": pos, "source_rid": rid,
                    "graph": child, "relation": "track_entities_child",
                })
        if annotations:
            raise ValueError(
                f"runtime client state in graph {index} at slot {min(annotations)}"
                " has no matching state"
            )
        graphs[index] = nodes
        dependencies[index] = graph_dependencies
    if client_slots:
        raise ValueError(f"runtime client state references unknown graph {min(client_slots)}")
    return graphs, guid_index(bundle["bodies"]), dependencies


def target_child_graph(node, states, class_declared):
    """Read only the explicit child binding validated by the Rust graph loader."""
    behavior = node.get("state_behavior")
    if behavior != "track_entities" and not class_declared:
        return None
    fields = node.get("fields")
    slot = node["state"]
    if (
        class_declared and behavior not in (None, "track_entities")
        or node.get("client_only") or not (class_declared or node.get("server_only"))
        or node.get("action_behavior") is not None
        or not uint(slot, 16) or slot >= len(states) or states[slot] != node["pos"]
        or states.count(node["pos"]) != 1 or not isinstance(fields, dict)
        or fields.get("m_ADEB6E05") or fields.get("targets") is None
        or not uint(fields.get("child_graph"), 32)
        or not uint(fields.get("target_variable"), 32) or fields["target_variable"] == 0
    ):
        raise ValueError("invalid declared track entities child binding")
    return fields["child_graph"]


def body_roots(body):
    candidates = [
        body.get("manager"),
        *body["weapons"],
        *(binding.get("graph") for binding in body["graphs"]),
        *(binding.get("graph") for binding in body.get("server_graphs", [])),
    ]
    roots = [candidate for candidate in candidates if candidate is not None]
    if any(not uint(graph, 32) for graph in roots):
        raise ValueError("invalid body graph root")
    return sorted(set(roots))


@dataclass
class Observation:
    hero: str
    scenario: str
    revision: str
    frames: int
    truncated: bool
    sha256: str
    reached: dict
    diagnostics: dict

    def provenance(self):
        return {
            "scenario": self.scenario, "source_revision": self.revision,
            "frames": self.frames, "truncated": self.truncated, "observation_sha256": self.sha256,
        }


def site_key(row, property_name):
    if (
        not isinstance(row, dict) or set(row) != {"graph", "node", property_name}
        or not uint(row["graph"], 32) or not uint(row["node"], 16)
    ):
        raise ValueError("invalid observation site")
    return row["graph"], row["node"]


def read_observation(path, bundle_sha256):
    row, digest = load_json(path)
    if not isinstance(row, dict) or set(row) != SIDECAR_FIELDS:
        raise ValueError("invalid node observation sidecar fields")
    if row["graph_bundle_sha256"] != bundle_sha256:
        raise ValueError("observation graph bundle digest mismatch")
    if (
        not uint(row["schema_version"], 32) or row["schema_version"] != 1
        or row["available"] is not True
        or not uint(row["frames"], 32) or row["frames"] == 0
        or type(row["truncated"]) is not bool
        or any(
            not isinstance(row[field], str) or not pattern.fullmatch(row[field])
            for field, pattern in (
                ("scenario", SCENARIO), ("hero", CANONICAL_GUID), ("source_revision", REVISION)
            )
        )
        or row["hero"] == "0x0000000000000000"
        or not isinstance(row["observations"], list) or not isinstance(row["diagnostics"], list)
    ):
        raise ValueError("invalid node observation sidecar provenance or values")
    reached = {}
    for site in row["observations"]:
        key = site_key(site, "client_scope_skipped")
        skipped = site["client_scope_skipped"]
        if type(skipped) is not bool or key in reached:
            raise ValueError("invalid or duplicate dispatch observation site")
        reached[key] = skipped
    diagnostics = {}
    for site in row["diagnostics"]:
        key = site_key(site, "kind")
        kind = site["kind"]
        if not isinstance(kind, str) or kind not in DIAGNOSTICS:
            raise ValueError("invalid diagnostic kind")
        diagnostics.setdefault(key, set()).add(kind)
    return Observation(
        row["hero"], row["scenario"], row["source_revision"], row["frames"],
        row["truncated"], digest, reached, diagnostics,
    )


def coverage(skip, diagnoses, observation):
    if observation is None:
        return "no_scenario"
    if skip is True:
        return "client_skipped"
    if "registration_rejection" in diagnoses:
        return "registration_rejection"
    if "unsupported" in diagnoses:
        return "unsupported"
    return "observed" if skip is False else "not_observed"


def site_evidence(observation, key):
    return {
        "client_scope_skipped": observation.reached.get(key) if observation else None,
        "diagnostics": sorted(observation.diagnostics.get(key, ())) if observation else [],
    }


def scenario_report(observation, roots, graphs, dependencies, hero_known=True):
    provenance = observation.provenance() if observation else {
        "scenario": None, "source_revision": None, "frames": None,
        "truncated": None, "observation_sha256": None,
    }
    selected = set(roots)
    observed_graphs = set()
    unmatched = []
    if observation:
        for key in sorted(observation.reached.keys() | observation.diagnostics.keys()):
            graph, pos = key
            if hero_known and pos in graphs.get(graph, {}):
                selected.add(graph)
                observed_graphs.add(graph)
                continue
            reason = "unknown_hero" if not hero_known else (
                "unknown_graph" if graph not in graphs else "unknown_node"
            )
            unmatched.append({
                "graph": graph, "node": pos, "reason": reason, **site_evidence(observation, key),
            })

    # Static references inventory possible children. Only the sidecar proves dispatch.
    pending = list(selected)
    expanded = set()
    edges = []
    while pending:
        graph = pending.pop()
        if graph in expanded:
            continue
        expanded.add(graph)
        for edge in dependencies.get(graph, ()):
            child = edge["graph"]
            edges.append({**edge, "present": child in graphs})
            if child not in selected:
                selected.add(child)
                pending.append(child)

    counts = Counter()
    reported_graphs = []
    for graph in sorted(selected):
        rows = []
        for pos, node in sorted(graphs.get(graph, {}).items()):
            evidence = site_evidence(observation, (graph, pos))
            status = coverage(evidence["client_scope_skipped"], evidence["diagnostics"], observation)
            counts[status] += 1
            rows.append({**node, **evidence, "coverage": status})
        reported_graphs.append({
            "graph": graph, "present": graph in graphs,
            "origin": "body_root" if graph in roots else (
                "observed_non_root" if graph in observed_graphs else "referenced_child"
            ),
            "nodes": rows,
        })
    return {
        **provenance, "counts": {status: counts[status] for status in COVERAGE},
        "graphs": reported_graphs, "unmatched_sites": unmatched,
        "static_dependencies": sorted(
            edges, key=lambda edge: (edge["source_graph"], edge["source_node"], edge["graph"]),
        ),
        "missing_dependency_graphs": sorted({edge["graph"] for edge in edges if not edge["present"]}),
    }


def build_report(graph_bundle, hero_catalog, observation_paths):
    bundle, bundle_sha256 = load_json(graph_bundle, compressed=True)
    graphs, bodies, dependencies = graph_index(bundle)
    catalog, catalog_sha256 = load_json(hero_catalog)
    heroes = guid_index(catalog)
    observations = [read_observation(path, bundle_sha256) for path in observation_paths]
    observations.sort(key=lambda row: (row.hero, row.scenario, row.revision, row.sha256))
    by_hero = {}
    for observation in observations:
        by_hero.setdefault(observation.hero, []).append(observation)

    rows = []
    for hero, profile in sorted(heroes.items()):
        body = guid(profile["body"])
        definition = bodies.get(body)
        if definition is None:
            body_status = "missing"
        elif definition.get("hero") is None or guid(definition["hero"]) != hero:
            body_status = "mismatched_hero"
        else:
            body_status = "present"
        roots = body_roots(definition) if body_status == "present" else []
        rows.append({
            "hero": hero, "body": body, "body_status": body_status,
            "root_graphs": roots, "missing_root_graphs": [g for g in roots if g not in graphs],
            "scenarios": [
                scenario_report(observation, roots, graphs, dependencies)
                for observation in by_hero.get(hero, [None])
            ],
        })
    return {
        "schema_version": 1, "graph_bundle_sha256": bundle_sha256,
        "hero_catalog_sha256": catalog_sha256, "observation_count": len(observations),
        "heroes": rows,
        "unmatched_hero_observations": [
            {"hero": observation.hero, **scenario_report(observation, [], graphs, dependencies, hero_known=False)}
            for observation in observations if observation.hero not in heroes
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--graph-bundle", type=Path, required=True)
    parser.add_argument("--hero-catalog", type=Path, required=True)
    parser.add_argument("--observation", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path, required=True, help="new private JSON file")
    args = parser.parse_args()
    try:
        report = build_report(args.graph_bundle, args.hero_catalog, args.observation)
        payload = json.dumps(report, allow_nan=False, separators=(",", ":"))
        with args.output.open("x", encoding="utf-8", newline="\n") as target:
            target.write(payload + "\n")
    except (OSError, KeyError, AttributeError, TypeError, ValueError, EOFError) as error:
        parser.error(str(error))
    print(f"Reported {len(report['heroes'])} catalog heroes and {report['observation_count']} observations.")


if __name__ == "__main__":
    main()
