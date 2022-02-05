#!/usr/bin/env python
"""
Use the wheels from pip's tests to cover the edge cases
"""

from pathlib import Path
from subprocess import check_call, DEVNULL

from test.compare import compare_installer, get_bin, get_root


def test_piptests():
    pip_dir = get_root().joinpath("pip")
    if not pip_dir.is_dir():
        check_call(
            [
                "git",
                "clone",
                "--depth",
                "1",
                "--branch",
                "22.0.3",
                "-q",
                "https://github.com/pypa/pip",
                pip_dir,
            ],
            stdout=DEVNULL,
            stderr=DEVNULL,
        )
    bin = get_bin()
    wheel_paths = list(pip_dir.joinpath("tests/data/packages/").glob("*.whl"))
    for invalid in [
        "brokenwheel-1.0-py2.py3-none-any.whl",
        "corruptwheel-1.0-py2.py3-none-any.whl",
        "invalid.whl",
        "priority-1.0-py2.py3-none-any.whl",
        "setuptools-0.9.8-py2.py3-none-any.whl",  # already installed by virtualenv
        "simple.dist-0.1-py1-none-invalid.whl",
        "simplewheel-1.0-py2.py3-none-any.whl",
        "simplewheel-2.0-1-py2.py3-none-any.whl",
        "simplewheel-2.0-py3-fakeabi-fakeplat.whl",
    ]:
        wheel_paths.remove(pip_dir.joinpath("tests/data/packages/").joinpath(invalid))
    compare_installer("venv-piptests", wheel_paths, bin)


if __name__ == "__main__":
    test_piptests()
