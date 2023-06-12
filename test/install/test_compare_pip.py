#!/usr/bin/env python
import os
import platform
import re
import shutil
import sys
import time
from argparse import ArgumentParser
from filecmp import cmpfiles
from pathlib import Path
from shutil import rmtree
from subprocess import check_call, DEVNULL
from typing import List, Union, Optional, Set

import pytest

from test.install.utils import get_bin, get_root


def compare_with_pip_wheels(
    env_name: str,
    wheels: List[Union[str, Path]],
    *,
    monotrail: Optional[Path] = None,
    clear_rs: bool = True,
    clear_pip: bool = False,
):
    compare_with_pip_args(
        env_name,
        ["--no-deps", *wheels],
        ["wheel-install", *wheels],
        monotrail=monotrail,
        clear_rs=clear_rs,
        clear_pip=clear_pip,
    )


def compare_with_pip_args(
    env_name: str,
    pip_args: List[Union[str, Path]],
    monotrail_args: List[Union[str, Path]],
    *,
    monotrail: Optional[Path] = None,
    clear_rs: bool = True,
    clear_pip: bool = False,
    cwd: Optional[Path] = None,
    content_diff_exclusions: Optional[Set[Path]] = None,
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
        check_call(["virtualenv", env], stdout=DEVNULL, cwd=cwd)
        start_pip = time.time()
        if platform.system() == "Windows":
            pip = env.joinpath("Scripts").joinpath("pip.exe")
        else:
            pip = env.joinpath("bin").joinpath("pip")
        check_call([pip, "install", "-q", *pip_args], cwd=cwd)
        stop_pip = time.time()
        env.rename(env_py)

        print(f"{env_name} pip install took {stop_pip - start_pip:.2f}s")

    # rust install
    if env_rs.exists():
        rmtree(env_rs)
    check_call(["virtualenv", env], stdout=DEVNULL)
    monotrail = monotrail or get_bin()
    start_rs = time.time()
    check_call(
        [monotrail, *monotrail_args],
        env=dict(os.environ, VIRTUAL_ENV=str(env)),
        cwd=cwd,
    )
    stop_rs = time.time()
    env.rename(env_rs)
    rust_time = stop_rs - start_rs

    print(f"{env_name} rs install took {rust_time :.2f}s")

    diff_envs(env_name, env_py, env_rs, content_diff_exclusions=content_diff_exclusions)

    if clear_rs:
        shutil.rmtree(env_rs)


def diff_envs(
    env_name: str,
    env_py: Path,
    env_rs: Path,
    content_diff_exclusions: Optional[Set[Path]] = None,
):
    """Compare the contents of two virtualenvs, one filled by our implementation and one
    filled by pip, poetry or another python tool we want to compare. Both were renamed
    from the same dir so all path inside are the same.
     * List all files and directories in both virtualenvs
     * Filter out all files and folder that are known to diverge
     * Check that the remaining file lists are identical
     * Filter our all directories and all files known to have different contents such
       as `INSTALLER`
     * Read the contents from both the rs and pip env, normalize quotation marks and
       newline
     * Check that the file contents are identical
    """
    # Filter out paths created by invoking pip and pip itself with a horrible regex
    # Better matching suggestions welcome 😬
    site_ignores = [
        "__pycache__",
        "pip",
        "pip-" + "[^/]+" + re.escape(".dist-info"),
        "setuptools",
        "pkg_resources",
        "_distutils_hack/__pycache__",
        # Doesn't make sense in our case to enforce this strictly
        "[a-zA-Z0-9._-]+" + re.escape(".dist-info/direct_url.json"),
        # poetry doesn't seem to do this (we do currently)
        "[a-zA-Z0-9._-]+" + re.escape(".dist-info/REQUESTED"),
    ]
    if platform.system() == "Windows":
        site_packages = "Lib\\site-packages\\"
    else:
        py_version = rf"{sys.version_info.major}.{sys.version_info.minor}"
        site_packages = rf"lib/python{py_version}/site-packages/"
    pattern = (
        "^("
        + re.escape(site_packages)
        + "("
        + "|".join(site_ignores)
        + ")"
        # TODO: .exe files should likely be identical
        + "|Scripts/.*\\.exe|monotrail.lock|.*/__pycache__(/.*)?"
        + ")"
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
        print(f"Differences in environment {env_name} with pattern {pattern}")
        for rs_only in env_rs_entries - env_py_entries:
            print(f"rust only: {rs_only}")
        for pip_only in env_py_entries - env_rs_entries:
            print(f"pip only: {pip_only}")
        sys.exit(1)

    start = time.time()
    # Filter out directories and files known to have diverging contents
    common = []
    for entry in env_rs_entries:
        if env_py.joinpath(entry).is_dir():
            continue
        # Used for source distribution
        if content_diff_exclusions and entry in content_diff_exclusions:
            continue
        # Ignore INSTALLER and subsequently RECORD
        # TODO(konstin): Use RECORD checker from monotrail on both
        if str(entry).startswith(site_packages):
            in_python_path = Path(str(entry).replace(site_packages, "", 1))
            if len(in_python_path.parts) == 2:
                [folder, file] = in_python_path.parts
                if folder.endswith(".dist-info") and file in ["INSTALLER", "RECORD"]:
                    continue

        common.append(entry)

    # Fast path for the majority of files that is byte-by-bytes identical
    _match, mismatch, errors = cmpfiles(env_py, env_rs, common)

    assert not errors, errors

    # Slow path to compare files again after some normalization
    diverging = []
    for entry in mismatch:
        # Ignore platform newline differences
        content_py = env_py.joinpath(entry).read_bytes().replace(b"\r", b"")
        content_rs = env_rs.joinpath(entry).read_bytes().replace(b"\r", b"")

        # These depend on the tool versions, make them equal across versions
        if (platform.system() == "Windows" and entry.parent.name == "Scripts") or (
            platform.system() != "Windows" and entry.parent.name == "bin"
        ):
            content_py = content_py.replace(b"'", b'"')
            content_rs = content_rs.replace(b"'", b'"')

        if content_py != content_rs:
            diverging.append(entry)
    end = time.time()
    # This is currently slow, remove this print once it is fast
    print(f"Comparing files took {end - start:.2f}")

    if diverging:
        for path in diverging:
            # clickable links for debugging
            print(f"# {path}\n", file=sys.stderr)
            print(
                f"{env_py.joinpath(path).relative_to(os.getcwd())}:1", file=sys.stderr
            )
            print(
                f"{env_rs.joinpath(path).relative_to(os.getcwd())}:1", file=sys.stderr
            )
        raise AssertionError(f"Diverging path:\n{diverging}")


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
