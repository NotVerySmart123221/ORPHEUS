"""Encode a verified Python observation using the Rust reader's canonical JSONL format."""
import hashlib
import json


def canonical(value):
    return json.dumps(value, allow_nan=False, separators=(",", ":"), sort_keys=True).encode()


def write_reference(stream, data, inputs, python_revision, adapter_revision):
    if set(data["missing"]) != {"provenance.effective_data_sha256"}:
        raise ValueError("reference still has unobserved fields")
    if data["source_rev"] != python_revision or data["manifest_sha256"] != inputs.runtime_sha256:
        raise ValueError("reference source or manifest differs from the capture")
    digest = hashlib.sha256()

    def line(row):
        encoded = canonical(row) + b"\n"
        digest.update(encoded)
        stream.write(encoded)

    line({
        "kind": "header", "schema": "orpheus.golden.v1", "profile": "soldier-range-v1",
        "scenario": "soldier-range", "build": 104319, "role": "reference",
        "source_rev": python_revision, "reference_rev": python_revision, "adapter_rev": adapter_revision,
        "toolchain": "python-" + data["toolchain"], "platform": "-".join(data["platform"]).lower(),
        "quanta_us": data["quanta_us"], "seed": data["seed"],
        "setup": {k: data[k] for k in ("bot_spawner_slot", "initial_room_tick", "random_model")},
        "runtime_sha256": inputs.runtime_sha256, "profile_sha256": inputs.profile_sha256,
        "schedule_sha256": inputs.schedule_sha256,
    })
    if len(data["ticks"]) != inputs.ticks:
        raise ValueError("wrong reference length")
    for row in data["ticks"]:
        if row["missing"]:
            raise ValueError("unobserved reference state or output")
        record = {k: row[k] for k in ("tick", "elapsed_us", "inputs", "missed", "state", "effects")}
        record["kind"] = "tick"
        projection = canonical({"effects": row["effects"], "state": row["state"]})
        record["digest"] = hashlib.sha256(projection).hexdigest()
        line(record)
    stream.write(canonical({"kind": "end", "ticks": inputs.ticks, "last_tick": inputs.ticks - 1,
                            "sha256": digest.hexdigest()}) + b"\n")
