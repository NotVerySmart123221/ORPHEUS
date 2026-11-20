"""Inventory structural field evidence from local StateScript listings."""

from __future__ import annotations

from collections import Counter, defaultdict
from dataclasses import dataclass, field
import io
import re
import tokenize


_GRAPH_HEADER = re.compile(r"^# graph ([0-9A-Fa-f]{16})(?=\s|$)")
_NODE_HEADER = re.compile(
    r"^(?:state [A-Za-z0-9_]+_(\d+)|def [A-Za-z0-9_]+_(\d+)\(\)):"
    r"\s+#\s+([A-Za-z][A-Za-z0-9_]{0,127})(?=[,;\s]|$)"
)
_RAW_FIELD = re.compile(r"m_[A-Za-z0-9_]{1,125}\Z")
_REFERENCES = {"var", "shared_var", "parent_var", "parameter", "param"}
_CLOSERS = {"(": ")", "[": "]", "{": "}"}
_MAX_DEPTH = 12
_MAX_EXAMPLES = 4


def _first_statement(lines, class_name):
    first = next(
        (index for index, line in enumerate(lines)
         if line.strip() and not line.lstrip().startswith("#")),
        None,
    )
    if first is None:
        return None
    line = lines[first]
    if not re.match(r"^ {4}(?:unimplemented\s+)?" + re.escape(class_name) + r"\s*\(", line):
        return None

    # The listing is a DSL. Tokenize only its matching owner constructor.
    # This excludes comment text and quoted data from field detection.
    source = line[4:] + "".join(lines[first + 1:])
    tokens = []
    try:
        for item in tokenize.generate_tokens(io.StringIO(source).readline):
            if item.type in (tokenize.NEWLINE, tokenize.ENDMARKER):
                break
            if item.type not in (tokenize.NL, tokenize.COMMENT):
                tokens.append(item)
    except (tokenize.TokenError, IndentationError, SyntaxError):
        return None
    return tokens


def _encloses(tokens, start):
    """Check that an opening token's matching closer is the last token."""
    if start >= len(tokens) or tokens[start].string not in _CLOSERS:
        return False
    expected = []
    for index, item in enumerate(tokens[start:], start):
        if item.type != tokenize.OP:
            continue
        symbol = item.string
        if symbol in _CLOSERS:
            expected.append(_CLOSERS[symbol])
        elif symbol in _CLOSERS.values():
            if not expected or expected.pop() != symbol:
                return False
            if not expected:
                return index == len(tokens) - 1
    return False


def _members(tokens):
    start = depth = 0
    for index, item in enumerate(tokens):
        if item.type == tokenize.OP:
            symbol = item.string
            if symbol in _CLOSERS:
                depth += 1
            elif symbol in _CLOSERS.values():
                depth -= 1
            elif symbol == "," and depth == 0:
                if index > start:
                    yield tokens[start:index]
                start = index + 1
    if start < len(tokens):
        yield tokens[start:]


def _call_open(tokens):
    opaque = bool(tokens and tokens[0].string == "unimplemented")
    opening = 2 if opaque else 1
    if (
        len(tokens) > opening
        and tokens[opening - 1].type == tokenize.NAME
        and tokens[opening].string == "("
        and _encloses(tokens, opening)
    ):
        return opening
    return None


def _is_reference(tokens, index):
    return (
        index + 1 < len(tokens)
        and tokens[index].type == tokenize.NAME
        and tokens[index].string in _REFERENCES
        and tokens[index + 1].string == "["
    )


def _operand_kind(tokens, opening):
    if len(tokens) == 1:
        item = tokens[0]
        if item.type == tokenize.NUMBER:
            return "number"
        if item.type == tokenize.STRING:
            return "string"
        if item.string in ("True", "False"):
            return "boolean"
        if item.string == "None":
            return "null"
    if (
        len(tokens) == 2
        and tokens[0].string in ("+", "-")
        and tokens[1].type == tokenize.NUMBER
    ):
        return "number"
    if _is_reference(tokens, 0) and _encloses(tokens, 1):
        return "reference"
    if opening is not None:
        return "call"
    if tokens[0].string == "[" and _encloses(tokens, 0):
        return "array"
    if tokens[0].string == "{" and _encloses(tokens, 0):
        return "object"
    return "expression"


def _fields(tokens, path, result, depth):
    opening = _call_open(tokens)
    if opening is None or depth > _MAX_DEPTH:
        return
    for argument in _members(tokens[opening + 1:-1]):
        key = argument[0]
        if (
            len(argument) >= 3 and key.type == tokenize.NAME
            and _RAW_FIELD.fullmatch(key.string) and argument[1].string == "="
        ):
            member = f"{path}.{key.string}" if path else key.string
            _operand(argument[2:], member, result, depth)


def _operand(tokens, path, result, depth):
    opening = _call_open(tokens)
    kind = _operand_kind(tokens, opening)
    dynamic = kind == "expression" or any(
        _is_reference(tokens, index) for index in range(len(tokens))
    )
    opaque = any(
        item.type == tokenize.NAME and item.string == "unimplemented" for item in tokens
    )
    result[path].add((kind, dynamic, opaque))
    if depth >= _MAX_DEPTH:
        return
    if opening is not None:
        _fields(tokens, path, result, depth + 1)
    elif kind == "array":
        for item in _members(tokens[1:-1]):
            _nested_item(item, path + "[]", result, depth + 1)


def _nested_item(tokens, path, result, depth):
    if depth >= _MAX_DEPTH:
        return
    if tokens[0].string == "[" and _encloses(tokens, 0):
        for item in _members(tokens[1:-1]):
            _nested_item(item, path + "[]", result, depth + 1)
    else:
        _fields(tokens, path, result, depth)


@dataclass
class _Field:
    present: int = 0
    dynamic: int = 0
    opaque: int = 0
    kinds: Counter = field(default_factory=Counter)
    examples: set = field(default_factory=set)

    def add(self, observations, graph, rid):
        self.present += 1
        self.dynamic += any(dynamic for _, dynamic, _ in observations)
        self.opaque += any(opaque for _, _, opaque in observations)
        self.kinds.update({kind for kind, _, _ in observations})
        self.examples.add((graph, rid))
        if len(self.examples) > _MAX_EXAMPLES:
            self.examples.remove(max(self.examples))


@dataclass
class _Class:
    occurrences: int = 0
    rendered: int = 0
    opaque: int = 0
    fields: dict = field(default_factory=lambda: defaultdict(_Field))


class FieldInventory:
    """Count only identified state and standalone action definitions.

    Named constructor fields are observed shapes. A missing path is merely
    unprinted or unrecognized in this representation. Inlined actions,
    friendly field names and unrecovered members cannot prove raw paths.
    """

    def __init__(self):
        self._graphs = set()
        self._classes = defaultdict(_Class)

    def add_graph(self, graph, lines):
        graph_index = graph & 0xFFFFFFFF
        if graph_index in self._graphs:
            raise ValueError("duplicate field evidence graph index")
        if isinstance(lines, str):
            lines = lines.splitlines(keepends=True)

        graph_seen = False
        nodes = {}
        current = None
        for line in lines:
            if line.startswith("# graph "):
                header = _GRAPH_HEADER.match(line)
                if not header or graph_seen:
                    raise ValueError("invalid or duplicate field evidence graph header")
                guid = int(header[1], 16)
                if graph not in (guid, guid & 0xFFFFFFFF):
                    raise ValueError("mismatched field evidence graph header")
                graph_seen = True
            if line and not line[0].isspace():
                current = None
                if header := _NODE_HEADER.match(line):
                    rid = int(header[1] or header[2])
                    if not graph_seen or rid > 0xFFFF or rid in nodes:
                        raise ValueError("invalid or duplicate field evidence node reference")
                    current = []
                    nodes[rid] = (header[3], current)
            elif current is not None:
                current.append(line)
        if not graph_seen:
            raise ValueError("field evidence listing has no graph header")

        self._graphs.add(graph_index)
        for rid, (name, body) in nodes.items():
            counts = self._classes[name]
            counts.occurrences += 1
            statement = _first_statement(body, name)
            if not statement or _call_open(statement) is None:
                continue
            counts.rendered += 1
            counts.opaque += statement[0].string == "unimplemented"
            fields = defaultdict(set)
            _fields(statement, "", fields, 0)
            for path, observations in fields.items():
                counts.fields[path].add(observations, graph_index, rid)

    def report(self):
        classes = []
        for name, group in sorted(self._classes.items()):
            fields = []
            for path, summary in sorted(group.fields.items()):
                fields.append({
                    "path": path,
                    "present": summary.present,
                    "not_rendered": group.occurrences - summary.present,
                    "rendered_without_path": group.rendered - summary.present,
                    "kinds": dict(sorted(summary.kinds.items())),
                    "dynamic": summary.dynamic,
                    "opaque": summary.opaque,
                    "examples": [
                        {"graph": graph, "rid": rid} for graph, rid in sorted(summary.examples)
                    ],
                })
            classes.append({
                "class": name,
                "occurrences": group.occurrences,
                "rendered": group.rendered,
                "omitted": group.occurrences - group.rendered,
                "opaque": group.opaque,
                "fields": fields,
            })
        return {"graphs": len(self._graphs), "classes": classes}
