"""Collect private timing operand evidence from local StateScript listings."""

import math
import re

from statescript_message_evidence import identifier, read_terms


WAIT_STATE = re.compile(
    r"^state [^:]+_(\d+):[ \t]*#[ \t]*STUStatescriptStateWait,"
    r"[ \t]*state[ \t]+(\d+)\b[ \t]*(?:\(([^)]*)\))?"
)
AFTER_BLOCK = re.compile(r"^    after\(")
BLOCK_COLON = re.compile(r"[ \t]*:")
NUMBER = re.compile(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[Ee][+-]?[0-9]+)?")
RANDOM = re.compile(r"^(random_between|random_float|random_int)\(")
DYNAMIC = re.compile(
    r"\b(?:var|shared_var|param)\[|\b(?:member|parent_var|element|max|min|host_slot)\("
)
QUOTED = re.compile(r"'(?:\\.|[^'\\])*'" r'|"(?:\\.|[^"\\])*"')
OPAQUE = re.compile(r"\bunimplemented\b|\[\?\]")


def literal_number(expression):
    if len(expression) > 80 or not NUMBER.fullmatch(expression):
        return None
    if not any(marker in expression for marker in ".eE"):
        return int(expression)
    value = float(expression)
    return value if math.isfinite(value) else None


def is_dynamic(expression):
    tokens = QUOTED.sub("", expression)
    return not OPAQUE.search(tokens) and bool(DYNAMIC.search(tokens))


def wait_operand(expression):
    if (duration := literal_number(expression)) is not None:
        return "literal", duration, None
    if call := RANDOM.match(expression):
        parsed = read_terms(expression, call.end() - 1)
        if parsed is not None and parsed[1] == len(expression) and len(parsed[0]) == 2:
            first, second = parsed[0]
            lower, upper = literal_number(first), literal_number(second)
            if lower is not None and upper is not None and lower <= upper:
                return "literal_range", None, {"operator": call[1], "min": lower, "max": upper}
            known_lower = lower is not None or is_dynamic(first)
            known_upper = upper is not None or is_dynamic(second)
            if known_lower and known_upper and (lower is None or upper is None):
                return "dynamic", None, None
        return "unsupported", None, None
    if is_dynamic(expression):
        return "dynamic", None, None
    return "unsupported", None, None


def wait_owner(line):
    if not (header := WAIT_STATE.match(line)):
        return None
    rid = identifier(header[1], 0xFFFF)
    state = identifier(header[2], 0xFFFF)
    if rid is None or state is None:
        return None
    flags = set(header[3].split(", ")) if header[3] else set()
    scope = "unspecified"
    if "client-only" in flags and "server-only" not in flags:
        scope = "client_only"
    elif "server-only" in flags and "client-only" not in flags:
        scope = "server_only"
    return rid, state, scope


def collect_timing(lines, graph) -> list[dict]:
    """Report direct wait-state operands without exporting an expression or its links."""
    text = "\n".join(line.rstrip("\r\n") for line in lines)
    rows = []
    offset = 0
    owner = None
    for line_number, line in enumerate(text.splitlines(keepends=True), start=1):
        line_offset, offset = offset, offset + len(line)
        if line.strip() and not line[0].isspace() and not line.startswith("#"):
            owner = wait_owner(line)
        if not owner or not (call := AFTER_BLOCK.match(line)):
            continue
        parsed = read_terms(text, line_offset + call.end() - 1)
        expression = ""
        if parsed and len(parsed[0]) == 1 and BLOCK_COLON.match(text, parsed[1]):
            expression = parsed[0][0]
        resolution, duration, bounds = wait_operand(expression)
        rid, state, scope = owner
        rows.append(
            {
                "graph": graph,
                "rid": rid,
                "state": state,
                "line": line_number,
                "scope": scope,
                "operation": "wait",
                "field": "after",
                "unit": None,
                "resolution": resolution,
                "duration": duration,
                "range": bounds,
            }
        )
    return rows
