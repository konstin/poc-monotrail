#!/usr/bin/env python
import os
import platform
import re
import shutil
import sys
import time
from argparse import ArgumentParser
from pathlib import Path
from shutil import rmtree
from subprocess import check_call, DEVNULL
from typing import List, Union

import pytest

from test.install.utils import get_bin, get_root


def compare_with_pip(
    env_name: str,
    wheels: List[Union[str, Path]],
    monotrail: Path,
    clear_rs: bool = True,
    clear_pip: bool = False,
):
    test_venvs = get_root().joinpath("test-venvs")
    test_venvs.mkdir(exist_ok=True)
    env = test_venvs.joinpath(f"{env_name}")
    env_rs = test_venvs.joinpath(f"{env_name}-rs")
    env_py = test_venvs.joinpath(f"{env_name}-pip")

    # pip install
    if clear_pip and env_py.exists():
        rmtree(env_py)
    if not env_py.exists():
        check_call(["virtualenv", env], stdout=DEVNULL)
        start_pip = time.time()
        if platform.system() == "Windows":
            pip = env.joinpath("Scripts").joinpath("pip.exe")
        else:
            pip = env.joinpath("bin").joinpath("pip")
        check_call([pip, "install", "-q", "--no-deps", *wheels])
        stop_pip = time.time()
        env.rename(env_py)

        print(f"{env_name} pip install took {stop_pip - start_pip:.2f}s")

    # rust install
    if env_rs.exists():
        rmtree(env_rs)
    check_call(["virtualenv", env], stdout=DEVNULL)
    start_rs = time.time()
    check_call(
        [monotrail, "venv-install", *wheels],
        # stdout=DEVNULL,
        env=dict(os.environ, VIRTUAL_ENV=str(env)),
    )
    stop_rs = time.time()
    env.rename(env_rs)
    rust_time = stop_rs - start_rs

    print(f"{env_name} rs install took {rust_time :.2f}s")

    diff_envs(env_name, env_py, env_rs)

    if clear_rs:
        shutil.rmtree(env_rs)


def diff_envs(env_name: str, env_py: Path, env_rs: Path):
    # Filter out paths created by invoking pip and pip itself with on oh horrible regex
    # Better matching suggestions welcome 😬
    dirs = [
        r"__pycache__",
        r"pip",
        r"pip-[^/]+.dist-info",
        r"setuptools",
        r"pkg_resources",
        r"_distutils_hack/__pycache__",
        r"[^/]+/direct_url.json",  # Doesn't make sense in our case to enforce this strictly
    ]
    pattern = (
        (
            r"^(Lib/site-packages/("
            if platform.system() == "Windows"
            else r"^(lib/python3\.8/site-packages/("
        )
        + "|".join(dirs)
        + r")|bin/__pycache__|Scripts/__pycache__|monotrail.lock|.*/__pycache__(/.*)?)"
    )
    if platform.system() == "Windows":
        # -.-
        # two for regex escaping
        pattern = pattern.replace("/", "\\\\")
    env_rs_entries = set()
    for i in env_rs.glob("**/*"):
        if re.match(pattern, str(i.relative_to(env_rs))):
            continue
        env_rs_entries.add(i.relative_to(env_rs))
    env_py_entries = set()
    for i in env_py.glob("**/*"):
        if re.match(pattern, str(i.relative_to(env_py))):
            continue
        env_py_entries.add(i.relative_to(env_py))
    symmetric_difference = env_rs_entries ^ env_py_entries
    if symmetric_difference:
        print(env_name, symmetric_difference)
        sys.exit(1)


@pytest.mark.skipif(sys.platform != "linux", reason="linux only wheel")
def test_purelib_platlib():
    purelib_platlib_wheel = get_root().joinpath(
        "test-data/wheels/purelib_and_platlib-1.0.0-cp38-cp38-linux_x86_64.whl"
    )
    compare_with_pip("purelib_platlib", [purelib_platlib_wheel], get_bin())


def test_tqdm():
    purelib_platlib_wheel = (
        get_root()
        .joinpath("test-data")
        .joinpath("popular-wheels")
        .joinpath("tqdm-4.62.3-py2.py3-none-any.whl")
    )
    compare_with_pip("tqdm", [purelib_platlib_wheel], get_bin())


def test_bio_embeddings_plus():
    """This wheel used to fail to install due to a name normalization mismatch"""
    bio_embeddings_plus_wheel = get_root().joinpath(
        "test-data/wheels/bio_embeddings_PLUS-0.1.1-py3-none-any.whl"
    )
    compare_with_pip("bio_embeddings_plus", [bio_embeddings_plus_wheel], get_bin())


def main():
    parser = ArgumentParser()
    parser.add_argument("wheel")
    args = parser.parse_args()

    wheel = Path(args.wheel)

    env_name = wheel.name.split("-")[0]
    compare_with_pip(env_name, [wheel], get_bin())


if __name__ == "__main__":
    main()
