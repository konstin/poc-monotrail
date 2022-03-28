#!/usr/bin/env python
import os
import shutil
import time
from argparse import ArgumentParser
from pathlib import Path
from shutil import rmtree
from subprocess import check_call, DEVNULL
from typing import List, Union

from test.compare import get_bin, get_root, diff_envs


def compare_with_poetry(
    env_name: str,
    poetry_dir: Path,
    install_wheel_rs: Path,
    clear_rs: bool = True,
    clear_poetry: bool = False,
):
    test_venvs = get_root().joinpath("test-venvs")
    test_venvs.mkdir(exist_ok=True)
    env = test_venvs.joinpath(f"{env_name}")
    env_rs = test_venvs.joinpath(f"{env_name}-rs")
    env_poetry = test_venvs.joinpath(f"{env_name}-poetry")

    # poetry install
    if clear_poetry and env_poetry.exists():
        rmtree(env_poetry)
    if not env_poetry.exists():
        check_call(["virtualenv", env], stdout=DEVNULL)
        env_vars = {**os.environ, "VIRTUAL_ENV": env}
        start_pip = time.time()
        check_call(["poetry", "install", "--no-root"], env=env_vars, cwd=poetry_dir)
        stop_pip = time.time()
        env.rename(env_poetry)

        print(f"{env_name} pip install took {stop_pip - start_pip:.2f}s")

    # rust install
    if env_rs.exists():
        rmtree(env_rs)
    check_call(["virtualenv", env], stdout=DEVNULL)
    start_rs = time.time()
    check_call(
        [install_wheel_rs, "poetry-install", poetry_dir.joinpath("pyproject.toml")],
        stdout=DEVNULL,
        env=dict(os.environ, VIRTUAL_ENV=env),
    )
    stop_rs = time.time()
    env.rename(env_rs)
    rust_time = stop_rs - start_rs

    print(f"{env_name} rs install took {rust_time :.2f}s")

    diff_envs(env_name, env_poetry, env_rs)

    if clear_rs:
        shutil.rmtree(env_rs)


def main():
    parser = ArgumentParser()
    parser.add_argument("poetry_dir")
    args = parser.parse_args()

    poetry_dir = Path(args.poetry_dir)

    compare_with_poetry(poetry_dir.name, poetry_dir, get_bin())


if __name__ == "__main__":
    main()
