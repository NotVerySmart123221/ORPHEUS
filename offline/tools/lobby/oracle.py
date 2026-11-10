"""Reads our statescript chunks back the way the 1.74 client does, to catch frames it would misread or crash on.

The reader is LizRollin's decoder: vendor ow174/game/script/decode.py, which reads by the client's own codec
table and a 373-graph catalog (research/lizrollin-runtime.md). oracle_graphs_174.json adds the graphs we send
that the catalog lacks. `watch()` hooks Stream._update, so every chunk
any code sends is read by a per-stream decode.ClientBody, which keeps the client's instance table from chunk
to chunk. Each chunk lands in one bucket:
- ok: read to the end, with every listed instance and binding resolved;
- crash: decode.ClientCrash, the client would crash;
- refused: decode.FrameRefused, the client would drop it and retry it every tick;
- misread: any other decode error. The bits don't line up with the graph and codec data, so the client reads
  the following fields shifted;
- unchecked: a graph not in the catalog, or a state class without a verified payload layout. The stream stays
  unchecked until its next full frame.

    OW_ORACLE=1 python -B -m pytest tools/lobby -q   (conftest.py prints the report after the run)
"""
import contextlib
import json
import os
import weakref
from collections import Counter
from pathlib import Path
from types import SimpleNamespace

from ow174.game import statescript
from ow174.game.bits import BitReader
from ow174.game.script import decode, graph

BUCKETS = ('ok', 'crash', 'refused', 'misread', 'unchecked')
# decode.read_payload's messages for a class the codec table cannot read: the frame is not wrong, just unknown
UNREADABLE = ('no codec for', 'the payload of')


class MissingGraph(decode.DecodeError):
    pass


# Graphs the pinned catalog lacks, from the retail install (tools/assets/graph_record.py; same format)
EXTRA = {record['index']: graph.Graph(record) for record in
         json.loads((Path(__file__).with_name('oracle_graphs_174.json')).read_text(encoding='utf-8'))['graphs']}


def _graph(index):
    found = graph.graph(index) or EXTRA.get(index)
    if found is None:
        raise MissingGraph(f'graph {index:04X} is not in the catalog')
    return found


decode.graph_of = _graph  # every graph lookup in the decoder goes through this name


class Report:
    def __init__(self):
        self.counts = Counter()
        self.problems = Counter()  # (bucket, message) -> chunks
        self.first = {}  # (bucket, message) -> (test, entity) where it was first seen
        self.failures = []  # (bucket, message, entity) of every crash, refused or misread chunk, in order
        self._bodies = weakref.WeakKeyDictionary()  # Stream -> ClientBody, None while unchecked

    def read(self, stream, update):
        """Read one chunk as the client of `stream` would; returns its bucket."""
        if decode.w_u32(BitReader(update.chunk.getvalue())) == 0:  # a full frame resets the client's table
            self._bodies[stream] = decode.ClientBody()
        body = self._bodies.get(stream)
        if body is None:
            return self._add('unchecked', 'a delta on a stream whose last full frame was not checked', stream)
        try:
            body.apply(update)
        except decode.ClientCrash as error:
            bucket, message = 'crash', f'{error.where}: {error.what}'
        except decode.FrameRefused as error:
            bucket, message = 'refused', str(error)
        except MissingGraph as error:
            bucket, message = 'unchecked', str(error)
        except decode.DecodeError as error:
            bucket = 'unchecked' if str(error).startswith(UNREADABLE) else 'misread'
            message = str(error)
        except Exception as error:  # the decoder tripped on data it does not expect: also a misread
            bucket, message = 'misread', f'{type(error).__name__}: {error}'
        else:
            return self._add('ok', None, stream)
        self._bodies[stream] = None  # the client's table is unknown until the next full frame
        return self._add(bucket, message, stream)

    def _add(self, bucket, message, stream):
        self.counts[bucket] += 1
        if bucket in ('crash', 'refused', 'misread'):
            self.failures.append((bucket, message, stream.entity))
        if message is not None:
            key = (bucket, message)
            self.problems[key] += 1
            self.first.setdefault(key, (os.environ.get('PYTEST_CURRENT_TEST', '?'), stream.entity))
        return bucket

    def summary(self, limit=60):
        lines = ['statescript oracle: ' + ', '.join(f'{name} {self.counts[name]}' for name in BUCKETS)]
        for (bucket, message), count in self.problems.most_common(limit):
            test, entity = self.first[(bucket, message)]
            lines.append(f'  {bucket:9} x{count:<5} {message}  [first: {test}, entity {entity:#x}]')
        return lines


def read_full(frame):
    """A full frame payload (owner_full_frame, remote_full_frame) as the client reads it into an empty table: the
    decode.Frame, or the decode error the client would hit."""
    return decode.ClientBody().apply(SimpleNamespace(chunk=statescript.chunk(frame, 0, 1)))


@contextlib.contextmanager
def watch(report=None):
    """Read every chunk a Stream makes while the block runs."""
    report = report or Report()
    original = statescript.Stream._update

    def _update(stream, data, resend):
        update = original(stream, data, resend)
        report.read(stream, update)
        return update

    statescript.Stream._update = _update
    try:
        yield report
    finally:
        statescript.Stream._update = original
