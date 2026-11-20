"""Collect graph-local variable access evidence from StateScript listings."""

from collections import Counter, defaultdict
from dataclasses import dataclass, field
import io
import re
import tokenize

from statescript_field_evidence import _encloses, _members
from statescript_message_evidence import identifier


_SCOPES = {"var": "instance", "shared_var": "shared"}
_ASSIGNMENTS = {"set_var": "set", "stack_contribution": "stack"}
_READ_HEADS = {"if", "elif", "while", "watch", "when", "after"}
_INDIRECT = {"member", "parent_var", "host_slot"}
_HEADER = re.compile(r"^(?:state |def |on_[A-Za-z_]+\(|schema:)")
_LABEL_RID = re.compile(r"^(?:state|def) [A-Za-z0-9_]+_(\d+)(?:\(\))?:")
_ENTRY_RID = re.compile(r"\brid\s*=\s*(\d{1,5})(?=[,\s)])")
_SKIP_TOKENS = {tokenize.NL, tokenize.COMMENT, tokenize.INDENT, tokenize.DEDENT}
_MAX_EXAMPLES = 3
_MAX_STATEMENT_TOKENS = 262144


def _statements(lines):
    # The listing uses display indentation, not Python indentation. Retain
    # physical line numbers while leaving quotation and comments to tokenize.
    text = "\n".join(line.lstrip(" \t").rstrip("\r\n") for line in lines)
    statement = []
    too_long = False
    for token in tokenize.generate_tokens(io.StringIO(text).readline):
        if token.type in (tokenize.NEWLINE, tokenize.ENDMARKER):
            if statement:
                yield statement if not too_long else None
            statement = []
            too_long = False
        elif token.type not in _SKIP_TOKENS and not too_long:
            if len(statement) == _MAX_STATEMENT_TOKENS:
                too_long = True
            else:
                statement.append(token)


def _slot(tokens, start=0):
    if start + 4 <= len(tokens):
        scope, opening, slot, closing = tokens[start:start + 4]
        if (
            scope.type == tokenize.NAME
            and scope.string in _SCOPES
            and opening.string == "["
            and slot.type == tokenize.NUMBER
            and closing.string == "]"
        ):
            index = identifier(slot.string, 0xFFFFFFFF)
            if index is not None:
                return _SCOPES[scope.string], index
    return None


@dataclass
class _Access:
    counts: Counter = field(default_factory=Counter)
    write_kinds: Counter = field(default_factory=Counter)
    default_kinds: Counter = field(default_factory=Counter)
    examples: dict = field(default_factory=lambda: defaultdict(list))

    def add(self, kind, line, rid, detail=None):
        self.counts[kind] += 1
        if kind == "write":
            self.write_kinds[detail] += 1
        elif kind == "schema_default":
            self.default_kinds[detail] += 1
        examples = self.examples[kind]
        example = {"line": line, "rid": rid, "kind": kind}
        if len(examples) < _MAX_EXAMPLES and example not in examples:
            examples.append(example)


class _GraphAccess:
    def __init__(self, graph):
        self.graph = graph
        self.variables = defaultdict(_Access)
        self.contexts = Counter()

    def operand(self, tokens, rid, role):
        opaque = any(
            item.type == tokenize.NAME and item.string == "unimplemented" for item in tokens
        )
        indirect = False
        refs = []
        for index, token in enumerate(tokens[:-1]):
            if token.type != tokenize.NAME or (index and tokens[index - 1].string == "."):
                continue
            if token.string in _INDIRECT and tokens[index + 1].string == "(":
                indirect = True
            if token.string not in _SCOPES:
                continue
            if tokens[index + 1].string != "[":
                continue
            if var := _slot(tokens, index):
                refs.append((var, token.start[0]))
            else:
                indirect = True
        if opaque:
            self.contexts["opaque"] += 1
            role = "opaque"
        if indirect or role == "dynamic":
            self.contexts["dynamic"] += 1
            if role != "opaque":
                role = "dynamic"
        if refs and role == "unclassified":
            self.contexts["unclassified"] += 1
        for var, line in refs:
            self.variables[var].add(role, line, rid)
        if opaque:
            return "opaque"
        if role == "dynamic":
            return "dynamic"
        if refs:
            return "variable_reference"
        begin, end = 0, len(tokens)
        while end - begin > 1 and tokens[begin].string == "(" and tokens[end - 1].string == ")":
            begin += 1
            end -= 1
        if end - begin == 1 and tokens[begin].string == "None":
            return "null"
        return "rendered"

    def assignment(self, tokens, rid, line):
        name = tokens[0].string
        if len(tokens) < 3 or tokens[1].string != "(" or not _encloses(tokens, 1):
            self.operand(tokens, rid, "unclassified")
            return
        terms = list(_members(tokens[2:-1]))
        if len(terms) < 2:
            self.operand(tokens, rid, "unclassified")
            return
        explicit = terms[0]
        dynamic_keys = [
            term for term in terms[1:]
            if len(term) >= 2 and term[0].string == "key" and term[1].string == "="
        ]
        target = _slot(explicit) if len(explicit) == 4 and not dynamic_keys else None
        if target is not None:
            self.variables[target].add("write", line, rid, _ASSIGNMENTS[name])
        else:
            self.operand(explicit, rid, "dynamic")
        for term in terms[1:]:
            self.operand(term, rid, "dynamic" if term in dynamic_keys else "read")

    def message_parameters(self, endpoint):
        direction = endpoint["direction"]
        line, rid = endpoint["line"], endpoint.get("rid")
        if "dynamic" in (endpoint.get("resolution"), endpoint.get("audience")):
            self.contexts["dynamic"] += 1
        for param in endpoint["parameters"]:
            resolution = param["operand_resolution"]
            variable = param["variable"]
            if resolution == "dynamic":
                self.contexts["dynamic"] += 1
            if resolution != "direct" or variable is None:
                continue
            var = variable["scope"], variable["slot"]
            if direction == "send":
                self.variables[var].add("read", line, rid)
            elif param["resolution"] == "literal":
                self.variables[var].add("parameter_binding", line, rid)
            else:
                self.contexts["unclassified"] += 1
                self.variables[var].add("unclassified", line, rid)

    def report(self):
        variables = []
        for (scope, slot), access in sorted(self.variables.items()):
            counts = access.counts
            row = {
                "scope": scope,
                "slot": slot,
                "reads": counts["read"],
                "writes": counts["write"],
                "schema_defaults": counts["schema_default"],
                "parameter_bindings": counts["parameter_binding"],
                "dynamic": counts["dynamic"],
                "opaque": counts["opaque"],
                "unclassified": counts["unclassified"],
                "no_observed_value_source": bool(counts["read"]) and not (
                    counts["write"] or access.default_kinds["rendered"]
                    or counts["parameter_binding"]
                ),
                "write_kinds": dict(sorted(access.write_kinds.items())),
                "default_kinds": dict(sorted(access.default_kinds.items())),
                "examples": sorted(
                    (ref for examples in access.examples.values() for ref in examples),
                    key=lambda ref: (ref["line"], ref["kind"]),
                ),
            }
            variables.append(row)
        return {
            "graph": self.graph,
            "variables": variables,
            "dynamic_contexts": self.contexts["dynamic"],
            "opaque_contexts": self.contexts["opaque"],
            "unclassified_contexts": self.contexts["unclassified"],
            "skipped_statements": self.contexts["skipped"],
            "lexical_errors": self.contexts["lexical_error"],
        }


def collect_variable_access(lines, graph, message_endpoints):
    """Report printed access forms, independent of execution and initialization order."""
    lines = lines.splitlines(keepends=True) if isinstance(lines, str) else list(lines)
    graph = graph & 0xFFFFFFFF
    endpoints = [rec for rec in message_endpoints if rec["graph"] & 0xFFFFFFFF == graph]
    endpoint_sites = {endpoint["line"]: endpoint.get("rid") for endpoint in endpoints}
    inventory = _GraphAccess(graph)
    rid = None
    in_schema = False
    try:
        for tokens in _statements(lines):
            if tokens is None:
                inventory.contexts["skipped"] += 1
                rid, in_schema = None, False
                continue
            line = tokens[0].start[0]
            source = lines[line - 1]
            if _HEADER.match(source):
                in_schema = source.startswith("schema:")
                label = _LABEL_RID.match(source)
                if label is None and source.startswith("on_"):
                    label = _ENTRY_RID.search(source)
                rid = endpoint_sites.get(line, identifier(label[1], 0xFFFF) if label else None)
            if line in endpoint_sites:
                continue
            head = tokens[0].string
            schema_row = in_schema and source.startswith("    ") and not source[4:5].isspace()
            if (
                schema_row and len(tokens) > 5 and tokens[4].string == "="
                and (var := _slot(tokens)) is not None
            ):
                kind = inventory.operand(tokens[5:], rid, "read")
                inventory.variables[var].add("schema_default", line, rid, kind)
            elif head in _ASSIGNMENTS:
                inventory.assignment(tokens, rid, line)
            elif head in _READ_HEADS:
                inventory.operand(tokens, rid, "read")
            else:
                inventory.operand(tokens, rid, "unclassified")
    except (tokenize.TokenError, SyntaxError):
        inventory.contexts["lexical_error"] += 1
    for endpoint in endpoints:
        inventory.message_parameters(endpoint)
    return inventory.report()
