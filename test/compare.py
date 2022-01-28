#!/usr/bin/env python
import os
import re
import shutil
import sys
import time
from argparse import ArgumentParser
from pathlib import Path
from shutil import rmtree
from subprocess import check_call, DEVNULL


def compare_installer(
    distribution: str,
    install_wheel_rs: str,
    clear_rs: bool = True,
    clear_pip: bool = False,
):
    env_name = distribution.split("-")[0]
    try:
        [wheel] = (
            Path(__file__)
            .parent.parent.joinpath("wheels")
            .glob(f"{distribution}-*.whl")
        )
    except ValueError:
        print(f"Missing wheel for {distribution}")
        sys.exit(1)
    test_venvs = Path("test-venvs")
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
        check_call(
            [env.joinpath("bin").joinpath("pip"), "install", "-q", "--no-deps", wheel]
        )
        stop_pip = time.time()
        env.rename(env_py)

        print(
            f"{env_name} pip install took {stop_pip - start_pip:.2f}s"
        )

    # rust install
    if env_rs.exists():
        rmtree(env_rs)
    check_call(["virtualenv", env], stdout=DEVNULL)
    start_rs = time.time()
    check_call(
        [install_wheel_rs, "install-file", wheel],
        stdout=DEVNULL,
        env=dict(os.environ, VIRTUAL_ENV=env),
    )
    stop_rs = time.time()
    env.rename(env_rs)

    print(
        f"{env_name} rs install took {stop_rs - start_rs:.2f}s"
    )

    # Filter out paths created by invoking pip and pip itself
    pattern = (
        r"^("
        r"lib/python3\.8/site-packages/(__pycache__|pip|pip-[^/]+.dist-info|_distutils_hack/__pycache__)"
        r"|bin/__pycache__"
        r")"
    )
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

    if clear_rs:
        shutil.rmtree(env_rs)


def main():
    parser = ArgumentParser()
    parser.add_argument("install_wheel_rs")
    parser.add_argument("distribution")
    args = parser.parse_args()

    install_wheel_rs = args.install_wheel_rs
    distribution = args.distribution

    compare_installer(distribution, install_wheel_rs)


if __name__ == "__main__":
    main()
