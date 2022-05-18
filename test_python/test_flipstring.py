import os
from subprocess import check_call


def test_flipstring():
    check_call([".venv/bin/monotrail_python", "flipstring/flip.py", "hello world!"])
