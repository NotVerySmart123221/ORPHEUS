"""Encode a verified Python observation using the Rust reader's canonical JSONL format."""
import hashlib
import json


def canonical(value):
    return json.dumps(value, allow_nan=False, separators=(",", ":"), sort_keys=True).encode()


CAPTURE_SETUP = (
    "bot_spawner_slot", "bot_spawner", "bot_roster_size", "initial_room_tick", "first_scheduled_room_tick",
    "setup_delivery_ticks", "initial_owner_delivery_phase", "host_random_model", "host_initial_seed",
    "player_vm_random_model", "player_vm_initial_seed",
)


def write_trace(stream, data, inputs, *, role, source_revision, reference_revision, adapter_revision, platform, toolchain):
    if role not in ("reference", "candidate"):
        raise ValueError("unknown observation role")
    if set(data["missing"]) - {"provenance.effective_data_sha256"}:
        raise ValueError(f"{role} still has unobserved fields")
    if role == "reference" and (data["source_rev"] != source_revision or data["manifest_sha256"] != inputs.runtime_sha256):
        raise ValueError("reference source or manifest differs from capture")
    digest = hashlib.sha256()

    def line(row):
        encoded = canonical(row) + b"\n"
        digest.update(encoded)
        stream.write(encoded)

    line({
        "kind": "header", "schema": "orpheus.golden.v2", "profile": "soldier-range-v1",
        "scenario": "soldier-range", "build": 104319, "role": role,
        "source_rev": source_revision, "reference_rev": reference_revision, "adapter_rev": adapter_revision,
        "toolchain": toolchain, "platform": platform, "quanta_us": data["quanta_us"],
        "setup": {k: data[k] for k in CAPTURE_SETUP},
        "runtime_sha256": inputs.runtime_sha256, "profile_sha256": inputs.profile_sha256,
        "schedule_sha256": inputs.schedule_sha256,
    })
    if len(data["ticks"]) != inputs.ticks:
        raise ValueError("wrong observation length")
    for row in data["ticks"]:
        if row["missing"]:
            raise ValueError("unobserved state or output")
        record = {k: row[k] for k in ("tick", "elapsed_us", "inputs", "missed", "state", "effects")}
        record["kind"] = "tick"
        projection = canonical({"effects": row["effects"], "state": row["state"]})
        record["digest"] = hashlib.sha256(projection).hexdigest()
        line(record)
    stream.write(canonical({"kind": "end", "ticks": inputs.ticks, "last_tick": inputs.ticks - 1,
                            "sha256": digest.hexdigest()}) + b"\n")
