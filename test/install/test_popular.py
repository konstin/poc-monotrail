#!/usr/bin/env python
"""
Test with the top 100 pypi wheels and some more
"""
from pathlib import Path
from subprocess import check_call

from test.install.test_compare_pip import compare_with_pip
from test.install.utils import get_bin, get_root


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
                Path(__file__).parent.parent.joinpath("popular100.txt"),
            ]
        )
    bin = get_bin()
    wheels_paths = list(wheels_dir.glob("*.whl"))
    compare_with_pip("popular", wheels_paths, bin)


if __name__ == "__main__":
    test_popular()
