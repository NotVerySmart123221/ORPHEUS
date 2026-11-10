"""Every statescript chunk a test sends through a Stream is read as the client would read it (oracle.py). A test
fails when one of its chunks would crash the client, be refused or be misread. OW_ORACLE=1 also prints the full
report, unchecked chunks included, after the run."""
import os

import pytest

import practice  # noqa: F401  installs the pinned vendor path
import oracle

_watch = oracle.watch()
REPORT = _watch.__enter__()


@pytest.fixture(autouse=True)
def client_reads_every_chunk():
    start = len(REPORT.failures)
    yield
    bad = REPORT.failures[start:]
    if bad:
        pytest.fail('the client would not read these statescript chunks:\n'
                    + '\n'.join(f'  {bucket} on entity {entity:#x}: {message}' for bucket, message, entity in bad))


def pytest_terminal_summary(terminalreporter):
    if os.environ.get('OW_ORACLE'):
        for line in REPORT.summary():
            terminalreporter.write_line(line)
