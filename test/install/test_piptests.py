#!/usr/bin/env python
"""
Use the wheels from pip's tests to cover the edge cases
"""

from test.install.test_compare_pip import compare_with_pip
from test.install.utils import get_bin, get_root


def test_piptests():
    bin = get_bin()
    wheel_paths = list(
        get_root().joinpath("test-data/pip-test-packages/").glob("*.whl")
    )
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
        wheel_paths.remove(
            get_root().joinpath("test-data/pip-test-packages/").joinpath(invalid)
        )
    compare_with_pip("venv-piptests", wheel_paths, bin)


if __name__ == "__main__":
    test_piptests()
