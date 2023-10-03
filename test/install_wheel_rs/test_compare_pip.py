#!/usr/bin/env python
import sys
from argparse import ArgumentParser
from pathlib import Path

import pytest

from test.install_wheel_rs.utils import get_root, compare_with_pip_wheels


@pytest.mark.skipif(sys.platform != "linux", reason="linux only wheel")
def test_purelib_platlib():
    purelib_platlib_wheel = get_root().joinpath(
        "test-data/wheels/purelib_and_platlib-1.0.0-cp38-cp38-linux_x86_64.whl"
    )
    compare_with_pip_wheels("purelib_platlib", [purelib_platlib_wheel])


def test_tqdm():
    purelib_platlib_wheel = (
        get_root()
        .joinpath("test-data")
        .joinpath("popular-wheels")
        .joinpath("tqdm-4.62.3-py2.py3-none-any.whl")
    )
    compare_with_pip_wheels("tqdm", [purelib_platlib_wheel])


def test_scripts_ignore_extras():
    miniblack = (
        get_root()
        .joinpath("test-data")
        .joinpath("wheels")
        .joinpath("miniblack-23.1.0-py3-none-any.whl")
    )
    compare_with_pip_wheels("miniblack", [miniblack])


def test_bio_embeddings_plus():
    """This wheel used to fail to install due to a name normalization mismatch"""
    bio_embeddings_plus_wheel = get_root().joinpath(
        "test-data/wheels/bio_embeddings_PLUS-0.1.1-py3-none-any.whl"
    )
    compare_with_pip_wheels("bio_embeddings_plus", [bio_embeddings_plus_wheel])


def main():
    parser = ArgumentParser()
    parser.add_argument("wheel")
    args = parser.parse_args()

    wheel = Path(args.wheel)

    env_name = wheel.name.split("-")[0]
    compare_with_pip_wheels(env_name, [wheel])


if __name__ == "__main__":
    main()
