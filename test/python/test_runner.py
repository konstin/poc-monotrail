import os
from subprocess import check_output


def test_flipstring():
    output = check_output(
        [".venv/bin/monotrail_python", "flipstring/flip.py", "hello world!"], text=True
    )
    assert output.splitlines()[-1] == "¡pꞁɹoM oꞁꞁǝH"


def test_numpy_identity_3():
    env = os.environ.copy()
    env["MONOTRAIL_CWD"] = "data_science_project"
    output = check_output(
        [".venv/bin/monotrail_script", "numpy_identity_3"], env=env, text=True
    )
    assert output.splitlines()[-1] == "3.0"
