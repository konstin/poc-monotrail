#!/usr/bin/env python
"""
Test with the top 100 pypi wheels and some more
"""
from subprocess import check_call

from test.install_wheel_rs.utils import get_root, compare_with_pip_wheels


def test_popular():
    wheels_dir = get_root().joinpath("test-data").joinpath("popular-wheels")
    if not wheels_dir.is_dir():
        print("Downloading wheels")
        check_call(
            [
                "pip",
                "download",
                "-d",
                wheels_dir,
                "-r",
                get_root().joinpath("test-data").joinpath("popular.txt"),
            ]
        )
    wheels_paths = list(wheels_dir.glob("*.whl"))
    compare_with_pip_wheels("popular", wheels_paths)


if __name__ == "__main__":
    test_popular()
