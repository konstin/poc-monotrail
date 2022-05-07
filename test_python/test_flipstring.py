import os
from subprocess import check_call


def test_flipstring():
    env = os.environ.copy()
    env["MONOTRAIL"] = "1"
    check_call([".venv/bin/python", "flipstring/flip.py", "hello world!"], env=env)
