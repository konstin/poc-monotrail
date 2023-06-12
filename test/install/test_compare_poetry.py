#!/usr/bin/env python
import os
import platform
import shutil
import sys
import time
from argparse import ArgumentParser
from pathlib import Path
from shutil import rmtree
from subprocess import check_call, DEVNULL, CalledProcessError
from typing import List, Optional

from test.install.test_compare_pip import diff_envs
from test.install.utils import get_bin, get_root


def compare_with_poetry(
    env_base: str,
    project_dir: Path,
    no_dev: bool,
    extras: List[str],
    *,
    monotrail: Optional[Path] = None,
    clear_rs: bool = True,
    clear_poetry: bool = False,
):
    test_venvs = get_root().joinpath("test-venvs")
    test_venvs.mkdir(exist_ok=True)
    env_name = f"{env_base}-{no_dev}-{'-'.join(extras)}"
    env = test_venvs.joinpath(f"{env_name}")
    env_rs = test_venvs.joinpath(f"{env_name}-rs")
    env_poetry = test_venvs.joinpath(f"{env_name}-poetry")

    separator = ";" if platform.system() == "Windows" else ":"
    # Normalize e.g. C:\Users\Me\monotrail\.venv/Scripts
    paths = [Path(path) for path in os.environ["PATH"].split(separator)]
    if env_var_virtualenv := os.environ.get("VIRTUAL_ENV"):
        bin_dir = "Scripts" if platform.system() == "Windows" else "bin"
        paths.remove(Path(env_var_virtualenv).joinpath(bin_dir))
    paths.insert(
        0, env.joinpath("Scripts" if platform.system() == "Windows" else "bin")
    )
    venv_env_vars = os.environ.copy()
    venv_env_vars["PATH"] = separator.join([str(path) for path in paths])
    venv_env_vars["VIRTUAL_ENV"] = str(env)

    # poetry install
    if clear_poetry and env_poetry.exists():
        rmtree(env_poetry)
    if not env_poetry.exists():
        check_call(["virtualenv", env], stdout=DEVNULL)
        start_pip = time.time()
        call = ["poetry", "install", "--no-root"]
        if no_dev:
            call.append("--no-dev")
        if extras:
            for extra in extras:
                call.extend(["-E", extra])
        check_call(call, env=venv_env_vars, cwd=project_dir)
        env.rename(env_poetry)
        stop_pip = time.time()

        print(f"{env_name} pip install took {stop_pip - start_pip:.2f}s")

    # rust install
    if env_rs.exists():
        rmtree(env_rs)
    check_call(["virtualenv", env], stdout=DEVNULL)
    monotrail = monotrail or get_bin()
    start_rs = time.time()
    call = [monotrail, "poetry-install"]
    if no_dev:
        call.append("--no-dev")
    if extras:
        for extra in extras:
            call.extend(["-E", extra])
    try:
        check_call(call, env=venv_env_vars, cwd=project_dir)
    except CalledProcessError:
        env.rename(env_rs)
        sys.exit(1)
    env.rename(env_rs)
    stop_rs = time.time()
    rust_time = stop_rs - start_rs

    print(f"{env_name} rs install took {rust_time :.2f}s")

    diff_envs(env_name, env_poetry, env_rs)

    if clear_rs:
        shutil.rmtree(env_rs)


def test_data_science_project():
    compare_with_poetry(
        "data_science_project",
        get_root().joinpath("data_science_project"),
        False,
        ["tqdm_feature"],
    )


def main():
    parser = ArgumentParser()
    parser.add_argument("project_dir")
    parser.add_argument("--no-dev", action="store_true")
    parser.add_argument("-E", "--extras", nargs="*")
    args = parser.parse_args()

    project_dir = Path(args.project_dir)

    compare_with_poetry(project_dir.name, project_dir, args.no_dev, args.extras)


if __name__ == "__main__":
    main()
