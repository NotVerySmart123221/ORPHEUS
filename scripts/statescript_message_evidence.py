"""Collect private GameMessage metadata from local StateScript listings."""

import re


CALL = re.compile(
    r"^([ \t]*)(on_game_message|subscribe_game_message|send_game_message|publish_game_message)\("
)
ENTRY_CLASS = re.compile(r":[ \t]*#[ \t]*STUStatescriptEntryGameMessage\b")
STATE_ENTRY = re.compile(
    r"^state [^:]+_(\d+):\s*#\s*STUStatescriptStateGameMessageEntry,\s*state\s+(\d+)\b"
)
SEND_DEFINITION = re.compile(
    r"^def SendGameMessage_(\d+)\(\):\s*#\s*STUStatescriptActionSendGameMessage\b"
)
FORMS = {
    "on_game_message": ("receive", "entry"),
    "subscribe_game_message": ("receive", "subscription"),
    "send_game_message": ("send", "send"),
    "publish_game_message": ("send", "publish"),
}
GUID = r"(?:0[xX])?[0-9A-Fa-f]{16}"
MESSAGE_LITERAL = re.compile(rf"(?:GameMessage|asset)\(\s*({GUID})\s*\)")
PARAMETER_LITERAL = re.compile(rf"asset\(\s*({GUID})\s*\)")
ENTRY_BINDING = re.compile(rf"(.+?)\s+<-\s+param\s+({GUID})", re.DOTALL)
PARAMETER_ROW = re.compile(r"^(?:unimplemented )?[A-Za-z][A-Za-z0-9_]{0,127}\(")
DYNAMIC = re.compile(r"^(?:var|shared_var|param)\[|^(?:member|parent_var|host_slot)\(")
VARIABLE = re.compile(r"(var|shared_var)\[(\d{1,10})\]")
MAX_CALL_CHARS = 32768
MAX_DEPTH = 64
MAX_TERMS = 256
BRACKETS = {"(": ")", "[": "]", "{": "}"}


def read_terms(text, start):
    """Return top-level terms and the end offset of a balanced delimited list."""
    if start >= len(text) or text[start] not in BRACKETS:
        return None
    ends = [BRACKETS[text[start]]]
    terms = []
    begin = start + 1
    quote = None
    escaped = False
    comment = False
    for i in range(begin, min(len(text), start + MAX_CALL_CHARS)):
        char = text[i]
        if comment:
            comment = char != "\n"
        elif quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = None
        elif char in ("'", '"'):
            quote = char
        elif char == "#":
            comment = True
        elif char in BRACKETS:
            ends.append(BRACKETS[char])
            if len(ends) > MAX_DEPTH:
                return None
        elif char in BRACKETS.values():
            if ends.pop() != char:
                return None
            if not ends:
                term = text[begin:i].strip()
                if term:
                    terms.append(term)
                return terms, i + 1
        elif char == "," and len(ends) == 1:
            terms.append(text[begin:i].strip())
            if len(terms) >= MAX_TERMS:
                return None
            begin = i + 1
    return None


def named_term(terms, name):
    for term in terms:
        key, equals, value = term.partition("=")
        if equals and key.strip() == name:
            return value.strip()
    return ""


def canonical_guid(text):
    value = int(text, 16)
    return f"0x{value:016X}" if value else None


def identifier(text, maximum):
    if re.fullmatch(r"[0-9]{1,10}", text):
        value = int(text)
        if value <= maximum:
            return value
    return None


def resolve_message(expression):
    if literal := MESSAGE_LITERAL.fullmatch(expression):
        if message_id := canonical_guid(literal[1]):
            return message_id, "literal"
    if DYNAMIC.match(expression):
        return None, "dynamic"
    return None, "unresolved"


def audience_kind(terms, parsed):
    if not parsed:
        return "unresolved"
    for name in ("targets", "m_target", "m_targets"):
        target = named_term(terms, name)
        if target == "None":
            return "unresolved"
        if DYNAMIC.match(target):
            return "dynamic"
        if target:
            return "explicit"
    return "absent"


def parameter(guid, expression, source):
    parameter_id = canonical_guid(guid) if guid else None
    variable = None
    operand_resolution = "unresolved"
    if expression in ("", "None"):
        operand_resolution = "absent"
    elif expression is not None:
        direct = VARIABLE.fullmatch(expression)
        if direct:
            if (slot := identifier(direct[2], 0xFFFFFFFF)) is not None:
                variable = {"scope": "instance" if direct[1] == "var" else "shared", "slot": slot}
                operand_resolution = "direct"
        elif DYNAMIC.match(expression):
            operand_resolution = "dynamic"
    return {
        "parameter_id": parameter_id,
        "parameter_slot": (int(parameter_id, 16) & 0xFFFF) if parameter_id else None,
        "resolution": "literal" if parameter_id else "unresolved",
        "variable": variable,
        "operand_resolution": operand_resolution,
        "source": source,
    }


def parameter_rows(expression, direction):
    if expression in ("", "None"):
        return []
    if not expression.startswith("[") or (listed := read_terms(expression, 0)) is None:
        return [parameter(None, None, "params")]
    rows, end = listed
    if end != len(expression):
        return [parameter(None, None, "params")]
    result = []
    for row in rows:
        call = PARAMETER_ROW.match(row)
        parsed = read_terms(row, call.end() - 1) if call else None
        if parsed is None or parsed[1] != len(row):
            result.append(parameter(None, None, "params"))
            continue
        fields, _ = parsed
        key = PARAMETER_LITERAL.fullmatch(named_term(fields, "m_B5051BCE"))
        operand = "m_out_Var" if direction == "receive" else "m_value"
        result.append(parameter(key[1] if key else None, named_term(fields, operand), "params"))
    return result


def entry_tail(text, offset):
    """Read the entry-only parameter bindings and require the declared entry class."""
    while offset < len(text) and text[offset] in " \t":
        offset += 1
    bindings = []
    if text[offset : offset + 1] == "(":
        parsed = read_terms(text, offset)
        if parsed is None:
            return None
        terms, offset = parsed
        for term in terms:
            binding = ENTRY_BINDING.fullmatch(term)
            guid, expression = (binding[2], binding[1]) if binding else (None, None)
            bindings.append(parameter(guid, expression, "entry_binding"))
    if not ENTRY_CLASS.match(text, offset):
        return None
    return bindings


def collect_messages(lines, graph) -> list[dict]:
    """Return bounded metadata records, with no listing text or expression bodies."""
    text = "\n".join(line.rstrip("\r\n") for line in lines)
    records = []
    active_subscription = None
    send_definition = None
    offset = 0
    for line_number, line in enumerate(text.splitlines(keepends=True), start=1):
        line_offset, offset = offset, offset + len(line)
        if line.strip() and not line[0].isspace() and not line.startswith("#"):
            active_subscription = STATE_ENTRY.match(line)
            send_definition = SEND_DEFINITION.match(line)
        call = CALL.match(line)
        if not call:
            continue
        indent, name = call.groups()
        direction, form = FORMS[name]
        if (form == "entry" and indent) or (form != "entry" and not indent):
            continue
        if form == "subscription" and (not active_subscription or indent != "    "):
            continue
        parsed = read_terms(text, line_offset + call.end() - 1)
        terms, tail_offset = parsed if parsed else ([], 0)
        rid = None
        state = None
        parameters = parameter_rows(
            named_term(terms, "params") or named_term(terms, "m_params"), direction
        )
        if form == "entry":
            bindings = entry_tail(text, tail_offset) if parsed else None
            if parsed and bindings is None:
                continue
            if bindings:
                parameters.extend(bindings)
            rid = identifier(named_term(terms, "rid"), 0xFFFF)
        else:
            if form == "subscription":
                rid = identifier(active_subscription[1], 0xFFFF)
                state = identifier(active_subscription[2], 0xFFFF)
            elif form == "send" and send_definition and indent == "    ":
                rid = identifier(send_definition[1], 0xFFFF)
        expression = named_term(terms, "m_gameMessage")
        if not expression and terms and form in ("entry", "send"):
            expression = terms[0]
        message_id, resolution = resolve_message(expression)
        records.append(
            {
                "graph": graph,
                "line": line_number,
                "direction": direction,
                "form": form,
                "rid": rid,
                "state": state,
                "message_id": message_id,
                "resolution": resolution,
                "audience": audience_kind(terms, parsed) if direction == "send" else None,
                "parameters": parameters,
            }
        )
    return records
